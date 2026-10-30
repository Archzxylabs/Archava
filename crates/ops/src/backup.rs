//! The backup/restore rehearsal machinery — OBS-04 + MIG-04 in code.
//!
//! OBS-04: a backup includes identity (release/config/pricebook), protected
//! private material, and is NOT VALID until its isolated restore was tested.
//! MIG-04: "the restore rehearsal never points at an active DB", and running
//! hashes are re-verified "without printing secret values".
//!
//! This module is the mechanism, not the scenario: `capture` materializes a
//! `pg_dump` custom-format archive plus a digest manifest, `verify_archive`
//! recomputes those digests, and `restore` replays the archive into a TARGET
//! while refusing the exact conditions an unsafe target can present. The
//! rehearsal (in `tests/upgrade_restore.rs`) drives all of it against two
//! disposable fixture instances and then verifies the restored data.

use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const BACKUP_MAGIC_HEADER: &[u8] = b"ARCHAVA-BACKUP-AESGCM-1\n";
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

/// Where the backing PostgreSQL binaries come from. The fixture proves the
/// server binaries are on PATH; the same discovery applies to pg_dump and
/// pg_restore — they ship in the same server package.
fn tool(name: &str) -> std::process::Command {
    let probe = std::process::Command::new(name)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "`{name}` is not runnable ({error}). The backup/restore rehearsals need the \
                 PostgreSQL client binaries on PATH — install the server package \
                 (Debian/Ubuntu: `apt-get install postgresql`), never a mock."
            )
        });
    assert!(
        probe.status.success(),
        "`{name} --version` exited {:?}: {}",
        probe.status.code(),
        String::from_utf8_lossy(&probe.stderr)
    );
    let mut command = std::process::Command::new(name);
    command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    command
}

/// One database the rehearsals treat as a capture source or restore target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionSpec {
    #[serde(default = "default_host")]
    pub host: String,
    pub port: u16,
    pub database: String,
    /// The role the capture/restore runs as. This must be the cluster's
    /// ADMIN role (superuser), because ledger contents are FORCE-RLS
    /// protected — a COPY-based backup or replay is administration of the
    /// instance, not an authority assertion, and the manifest carries
    /// digests and counts, never values. Never a password: the fixture
    /// cluster authenticates by trust on loopback, and a real deployment
    /// passes credentials through its environment, not through a manifest.
    pub user: String,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

impl ConnectionSpec {
    /// The command-line encoding of this spec for pg_dump/pg_restore.
    pub fn args(&self) -> Vec<String> {
        vec![
            "--host".to_string(),
            self.host.clone(),
            "--port".to_string(),
            self.port.to_string(),
            "--username".to_string(),
            self.user.clone(),
            "--no-password".to_string(),
        ]
    }
}

/// The identity the capture recorded for the serving stack: the release the
/// database backed at capture time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseIdentity {
    pub id: String,
    pub version: String,
}

/// The manifest a capture publishes. `credential_values_printed` is a REPORT
/// FIELD, not a hope: capture only ever records names, counts, digests and
/// sizes — the reason it can print "false" is that the writer has nowhere a
/// secret could go.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    #[serde(rename = "reportedAt")]
    pub reported_at: String,
    pub release: ReleaseIdentity,
    /// The capture source, recorded so a restore target can be REFUSED when
    /// it points at the same database (MIG-04's "not an active DB" — the
    /// restore must be isolated, and "isolated" is a checkable fact).
    pub source: ConnectionSpec,
    pub archive: ArchiveEntry,
    /// Per-ledger row counts (from `pg_class`'s live relation estimate) — an
    /// identity summary, never contents.
    pub contents: Vec<TableCount>,
    #[serde(rename = "credentialValuesPrinted")]
    pub credential_values_printed: bool,
    #[serde(rename = "automaticDeletion")]
    pub automatic_deletion: bool,
    /// OBS-04's validity rule, carried on the artifact itself: a backup that
    /// no rehearsal has restored is marked unproven.
    #[serde(rename = "restoreRehearsalVerified")]
    pub restore_rehearsal_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCount {
    pub ledger: String,
    pub table: String,
    pub rows: u64,
}

/// What can go wrong, typed so rehearsal tests name the refusal they expect.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// The named target is the SAME database the capture came from — the
    /// classic "restore rehearsal flattened my live install" failure. MIG-04
    /// makes this a hard refusal, not a warning.
    #[error("restore refused: target {host}:{port}/{database} is the capture source (the restore must be isolated)")]
    ActiveSource {
        host: String,
        port: u16,
        database: String,
    },
    /// The target holds an UNDRAINED writer lease: a live writer is writing
    /// while the restore replays. The refusal names the lease, not the role.
    #[error(
        "restore refused: target has an undrained writer lease '{lease}' (drain or fence it first)"
    )]
    UndrainedWriter { lease: String },
    #[error("manifest digest mismatch for {file}: recorded {recorded}, recomputed {recomputed}")]
    Digest {
        file: String,
        recorded: String,
        recomputed: String,
    },
    #[error("backup key required: set ARCHAVA_BACKUP_KEY (32 bytes base64 or 64 hex)")]
    MissingBackupKey,
    #[error("backup key invalid: {0}")]
    InvalidBackupKey(String),
    #[error("backup decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("backup tool failure: {0}")]
    Tool(String),
    #[error("restored writer could not be invalidated; keep the target isolated and inspect its serving lease")]
    WriterInvalidation,
    #[error("pg_restore failed or its acknowledgement was lost (exit {exit:?}); keep the target isolated and inspect it before reopening")]
    ReplayUnconfirmed { exit: Option<i32> },
    #[error("backup io failure at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("manifest json invalid: {0}")]
    Json(#[from] serde_json::Error),
}

/// RAII guard ensuring temporary plaintext dump files are deleted from disk
/// immediately after encryption or replay, even on error or panic.
struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Resolves a 32-byte AES-256 backup key from an explicit string or the `ARCHAVA_BACKUP_KEY` env var.
pub fn resolve_backup_key(explicit_or_flag: Option<&str>) -> Result<[u8; 32], RestoreError> {
    if let Some(raw) = explicit_or_flag {
        if !raw.trim().is_empty() {
            return decode_key(raw);
        }
    }
    if let Ok(env_key) = std::env::var("ARCHAVA_BACKUP_KEY") {
        if !env_key.trim().is_empty() {
            return decode_key(&env_key);
        }
    }
    Err(RestoreError::MissingBackupKey)
}

/// Decodes a 32-byte key from either a 64-hex string or a base64 string.
pub fn decode_key(raw: &str) -> Result<[u8; 32], RestoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RestoreError::MissingBackupKey);
    }
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = u8::from_str_radix(&trimmed[i * 2..i * 2 + 2], 16)
                .map_err(|e| RestoreError::InvalidBackupKey(e.to_string()))?;
        }
        return Ok(key);
    }

    let decoded = decode_base64(trimmed)
        .map_err(|e| RestoreError::InvalidBackupKey(format!("base64 decode error: {e}")))?;
    if decoded.len() != 32 {
        return Err(RestoreError::InvalidBackupKey(format!(
            "key must decode to exactly 32 bytes, got {}",
            decoded.len()
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

/// RFC 4648 base64 decoder.
fn decode_base64(input: &str) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in input.as_bytes() {
        if b.is_ascii_whitespace() || b == b'=' {
            continue;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err("invalid character in base64 string"),
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Base64 encoder for tests and key generation.
pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(TABLE[idx0] as char);
        out.push(TABLE[idx1] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[idx2] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[idx3] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

/// Encrypts raw data using AES-256-GCM with a random 12-byte nonce.
/// Format: `ARCHAVA-BACKUP-AESGCM-1\n` (24 bytes) + `nonce` (12 bytes) + `ciphertext + tag`.
pub fn encrypt_backup(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, RestoreError> {
    let key_arr = Key::<Aes256Gcm>::from(*key);
    let cipher = Aes256Gcm::new(&key_arr);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| RestoreError::Tool(format!("AES-256-GCM encryption failed: {e}")))?;

    let mut output = Vec::with_capacity(BACKUP_MAGIC_HEADER.len() + NONCE_LEN + ciphertext.len());
    output.extend_from_slice(BACKUP_MAGIC_HEADER);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts backup payload using AES-256-GCM.
/// If payload is unencrypted legacy PostgreSQL custom dump (starts with `PGDMP`),
/// returns raw data verbatim to support legacy restores.
pub fn decrypt_backup(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, RestoreError> {
    if !data.starts_with(BACKUP_MAGIC_HEADER) {
        if data.starts_with(b"PGDMP") {
            return Ok(data.to_vec());
        }
        return Err(RestoreError::DecryptionFailed(
            "unrecognized backup format: missing ARCHAVA-BACKUP-AESGCM-1 header and not a legacy dump".to_string(),
        ));
    }

    let min_len = BACKUP_MAGIC_HEADER.len() + NONCE_LEN + TAG_LEN;
    if data.len() < min_len {
        return Err(RestoreError::DecryptionFailed(format!(
            "backup data too short: expected at least {min_len} bytes, got {}",
            data.len()
        )));
    }

    let nonce_start = BACKUP_MAGIC_HEADER.len();
    let nonce_end = nonce_start + NONCE_LEN;
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(&data[nonce_start..nonce_end]);
    let ciphertext = &data[nonce_end..];

    let key_arr = Key::<Aes256Gcm>::from(*key);
    let cipher = Aes256Gcm::new(&key_arr);
    let nonce = Nonce::from(nonce_arr);

    cipher.decrypt(&nonce, ciphertext).map_err(|e| {
        RestoreError::DecryptionFailed(format!("AES-256-GCM authentication failed: {e}"))
    })
}

/// Capture one database into `out_dir` with explicit or environment AES-256-GCM key.
/// S06: Refuses with `RestoreError::MissingBackupKey` rather than writing plaintext.
pub fn capture_with_key(
    source: &ConnectionSpec,
    out_dir: &Path,
    release: ReleaseIdentity,
    key: Option<&[u8; 32]>,
) -> Result<BackupManifest, RestoreError> {
    let backup_key = match key {
        Some(k) => *k,
        None => resolve_backup_key(None)?,
    };

    std::fs::create_dir_all(out_dir).map_err(|source| RestoreError::Io {
        path: out_dir.into(),
        source,
    })?;
    let archive_path = out_dir.join("archava-backup.dump");
    // Refuse to overwrite an existing capture (same rule the TS side used:
    // a second capture is a NEW capture, not a rewrite).
    if archive_path.exists() {
        return Err(RestoreError::Tool(format!(
            "backup target {} already exists",
            archive_path.display()
        )));
    }

    let staging_path = out_dir.join(format!(".staging-pgdump-{}.tmp", uuid::Uuid::new_v4()));
    let _staging_guard = TempFileGuard(staging_path.clone());
    // pg_dump truncates an existing file without changing its private mode.
    // Precreate it atomically so plaintext is never exposed under the process umask.
    private_file(&staging_path)?;

    let dump = tool("pg_dump")
        .args([
            "--format".to_string(),
            "custom".to_string(),
            "--file".to_string(),
            staging_path.to_string_lossy().to_string(),
        ])
        .args(source.args())
        .args([source.database.clone()])
        .output()
        .map_err(|e| RestoreError::Tool(format!("pg_dump: {e}")))?;
    if !dump.status.success() {
        return Err(RestoreError::Tool(format!(
            "pg_dump exited {:?}: {}",
            dump.status.code(),
            String::from_utf8_lossy(&dump.stderr)
        )));
    }

    let raw_bytes = std::fs::read(&staging_path).map_err(|source| RestoreError::Io {
        path: staging_path.clone(),
        source,
    })?;

    let encrypted = encrypt_backup(&raw_bytes, &backup_key)?;
    use std::io::Write;
    let mut archive = private_file(&archive_path)?;
    archive
        .write_all(&encrypted)
        .map_err(|source| RestoreError::Io {
            path: archive_path.clone(),
            source,
        })?;
    archive.sync_all().map_err(|source| RestoreError::Io {
        path: archive_path.clone(),
        source,
    })?;

    let bytes = encrypted.len() as u64;
    let digest = format!("{:x}", Sha256::digest(&encrypted));

    Ok(BackupManifest {
        schema_version: "1.0.0".to_string(),
        reported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default(),
        release,
        source: source.clone(),
        archive: ArchiveEntry {
            file_name: "archava-backup.dump".to_string(),
            bytes,
            sha256: digest,
        },
        contents: Vec::new(),
        credential_values_printed: false,
        automatic_deletion: false,
        restore_rehearsal_verified: false,
    })
}

/// Capture one database into `out_dir`: custom-format archive encrypted with AES-256-GCM.
///
/// Uses `ARCHAVA_BACKUP_KEY` from environment; refuses with `RestoreError::MissingBackupKey`
/// if missing.
pub fn capture(
    source: &ConnectionSpec,
    out_dir: &Path,
    release: ReleaseIdentity,
) -> Result<BackupManifest, RestoreError> {
    capture_with_key(source, out_dir, release, None)
}

/// Read a manifest from disk. The caller then verifies what it claims with
/// [`verify_archive`] — reading and trusting are deliberately separated.
pub fn read_manifest(path: &Path) -> Result<BackupManifest, RestoreError> {
    let bytes = std::fs::read(path).map_err(|source| RestoreError::Io {
        path: path.into(),
        source,
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Recompute the archive's digest and byte count against the manifest's
/// record. The caller sees digests and sizes, never contents.
pub fn verify_archive(manifest: &BackupManifest, manifest_dir: &Path) -> Result<(), RestoreError> {
    let archive_path = manifest_dir.join(&manifest.archive.file_name);
    let bytes = std::fs::read(&archive_path).map_err(|source| RestoreError::Io {
        path: archive_path.clone(),
        source,
    })?;
    let recomputed = format!("{:x}", Sha256::digest(&bytes));
    if recomputed != manifest.archive.sha256 {
        return Err(RestoreError::Digest {
            file: manifest.archive.file_name.clone(),
            recorded: manifest.archive.sha256.clone(),
            recomputed,
        });
    }
    if bytes.len() as u64 != manifest.archive.bytes {
        return Err(RestoreError::Digest {
            file: manifest.archive.file_name.clone(),
            recorded: manifest.archive.bytes.to_string(),
            recomputed: (bytes.len() as u64).to_string(),
        });
    }
    Ok(())
}

/// Restore a verified manifest into an isolated target with explicit or environment AES-256-GCM key.
pub fn restore_with_key(
    manifest: &BackupManifest,
    manifest_dir: &Path,
    target: &ConnectionSpec,
    key: Option<&[u8; 32]>,
) -> Result<(), RestoreError> {
    if target.host == manifest.source.host
        && target.port == manifest.source.port
        && target.database == manifest.source.database
    {
        return Err(RestoreError::ActiveSource {
            host: target.host.clone(),
            port: target.port,
            database: target.database.clone(),
        });
    }
    verify_archive(manifest, manifest_dir)?;
    let archive_path = manifest_dir.join(&manifest.archive.file_name);
    let encrypted_bytes = std::fs::read(&archive_path).map_err(|source| RestoreError::Io {
        path: archive_path.clone(),
        source,
    })?;

    let (plain_path, _guard) = if encrypted_bytes.starts_with(BACKUP_MAGIC_HEADER) {
        let backup_key = match key {
            Some(k) => *k,
            None => resolve_backup_key(None)?,
        };
        let decrypted = decrypt_backup(&encrypted_bytes, &backup_key)?;
        let staging_path =
            manifest_dir.join(format!(".staging-pgrestore-{}.tmp", uuid::Uuid::new_v4()));
        let guard = TempFileGuard(staging_path.clone());

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true).mode(0o600);
            let mut file = opts
                .open(&staging_path)
                .map_err(|source| RestoreError::Io {
                    path: staging_path.clone(),
                    source,
                })?;
            file.write_all(&decrypted)
                .map_err(|source| RestoreError::Io {
                    path: staging_path.clone(),
                    source,
                })?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&staging_path, decrypted).map_err(|source| RestoreError::Io {
                path: staging_path.clone(),
                source,
            })?;
        }

        (staging_path, Some(guard))
    } else if encrypted_bytes.starts_with(b"PGDMP") {
        (archive_path.clone(), None)
    } else {
        return Err(RestoreError::DecryptionFailed(
            "unrecognized archive format: neither encrypted ARCHAVA-BACKUP-AESGCM-1 nor custom PGDMP".to_string(),
        ));
    };

    // Preserve archived object owners and runtime grants. A restore that strips
    // them can report success while the actual runtime cannot read its ledgers.
    // The operator must provision the same owner/runtime roles on the target cluster.
    // `--clean --if-exists` because the isolated target is not guaranteed to
    // be virgin: a rehearsal target may have been provisioned-and-migrated by
    // its fixture (the fixture's ledgers exist even in a fresh instance). The
    // restore's contract is to REPLACE the ledger contents with the capture's
    // exact replay — drop-and-recreate of archive objects — never to silently
    // merge two databases' histories.
    let out = tool("pg_restore")
        .args([
            "--single-transaction".to_string(),
            "--clean".to_string(),
            "--if-exists".to_string(),
        ])
        .args(target.args())
        .args(["--dbname".to_string(), target.database.clone()])
        .arg(&plain_path)
        .output()
        .map_err(|e| RestoreError::Tool(format!("pg_restore: {e}")))?;
    if !out.status.success() {
        // stderr may contain values from failed COPY/DDL. The public operator
        // report carries the exit outcome, never raw replay contents. A lost
        // acknowledgement alone also cannot prove that COMMIT did not happen.
        return Err(RestoreError::ReplayUnconfirmed {
            exit: out.status.code(),
        });
    }
    // The source process identity must never revive on the restored target.
    // A finite administrator client works for both sync library and CLI callers;
    // the CLI still holds its target-exclusion guard during this step.
    let invalidated=tool("psql").args(target.args())
        .args(["--no-psqlrc","--set","ON_ERROR_STOP=1","--dbname",&target.database,"--command",
        "DO $$ BEGIN IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN UPDATE archava_control.writer_lease SET epoch=epoch+1,drained=true,updated_at=clock_timestamp() WHERE lease_name='archava-serving';END IF; IF to_regclass('archava_billing.gateway_runtime') IS NOT NULL THEN UPDATE archava_billing.gateway_runtime SET held=true WHERE singleton;END IF;END $$"])
        .output().map_err(|_|RestoreError::WriterInvalidation)?;
    if !invalidated.status.success() {
        return Err(RestoreError::WriterInvalidation);
    }
    Ok(())
}

/// Restore a verified manifest into an isolated target using `ARCHAVA_BACKUP_KEY` if encrypted.
pub fn restore(
    manifest: &BackupManifest,
    manifest_dir: &Path,
    target: &ConnectionSpec,
) -> Result<(), RestoreError> {
    restore_with_key(manifest, manifest_dir, target, None)
}

/// The active-writer refusal (MIG-04's "not an active DB", live form): a
/// target whose `archava_control.writer_lease` holds an undrained seat
/// cannot accept a restore replay. Cost is one query; the failure names the
/// lease rather than guessing.
pub async fn refuse_undrained_target(pool: &sqlx::PgPool) -> Result<(), RestoreError> {
    let res: Result<Option<(String,)>, sqlx::Error> = sqlx::query_as(
        "SELECT lease_name FROM archava_control.writer_lease WHERE drained = false LIMIT 1",
    )
    .fetch_optional(pool)
    .await;

    match res {
        Ok(Some((lease,))) => Err(RestoreError::UndrainedWriter { lease }),
        Ok(None) => Ok(()),
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("42P01") => {
            // Relation archava_control.writer_lease does not exist: target has no control leases
            Ok(())
        }
        Err(e) => Err(RestoreError::Tool(format!("lease probe: {e}"))),
    }
}

fn private_file(path: &std::path::Path) -> Result<std::fs::File, RestoreError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|source| RestoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_decode_roundtrip() {
        let original = b"0123456789abcdef0123456789abcdef";
        let encoded = encode_base64(original);
        assert_eq!(encoded.len(), 44);
        let decoded = decode_base64(&encoded).expect("decode base64");
        assert_eq!(decoded, original.to_vec());
    }

    #[test]
    fn test_decode_key_hex_and_base64() {
        // 64-char hex
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key_from_hex = decode_key(hex_key).expect("decode hex key");
        assert_eq!(key_from_hex[0], 0x01);
        assert_eq!(key_from_hex[15], 0xef);

        // Base64 of 32 bytes
        let raw = [42u8; 32];
        let b64 = encode_base64(&raw);
        let key_from_b64 = decode_key(&b64).expect("decode base64 key");
        assert_eq!(key_from_b64, raw);
    }

    #[test]
    fn test_invalid_key_refusal() {
        // Too short
        let err = decode_key("MDEyMw==").unwrap_err();
        assert!(matches!(err, RestoreError::InvalidBackupKey(_)));

        // Empty
        let err = decode_key("").unwrap_err();
        assert!(matches!(err, RestoreError::MissingBackupKey));
    }

    #[test]
    fn test_aes_256_gcm_encrypt_decrypt_roundtrip() {
        let key = [0x5au8; 32];
        let plaintext = b"PostgreSQL custom-format fake test stream header and database tables";

        let encrypted = encrypt_backup(plaintext, &key).expect("encryption succeeds");
        assert!(encrypted.starts_with(BACKUP_MAGIC_HEADER));
        assert_ne!(
            &encrypted[BACKUP_MAGIC_HEADER.len() + NONCE_LEN..],
            plaintext
        );

        let decrypted = decrypt_backup(&encrypted, &key).expect("decryption succeeds");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_aes_256_gcm_tamper_detection() {
        let key = [0x33u8; 32];
        let plaintext = b"Sensitive ledger data with FORCE-RLS protections";

        let mut encrypted = encrypt_backup(plaintext, &key).expect("encrypt");

        // Tamper with one byte in the ciphertext payload
        let last_idx = encrypted.len() - 1;
        encrypted[last_idx] ^= 0x01;

        let err = decrypt_backup(&encrypted, &key).unwrap_err();
        assert!(matches!(err, RestoreError::DecryptionFailed(_)));
    }

    #[test]
    fn test_wrong_key_fails_decryption() {
        let key1 = [0x11u8; 32];
        let key2 = [0x22u8; 32];
        let plaintext = b"Tenant account balance 50000000";

        let encrypted = encrypt_backup(plaintext, &key1).expect("encrypt");
        let err = decrypt_backup(&encrypted, &key2).unwrap_err();
        assert!(matches!(err, RestoreError::DecryptionFailed(_)));
    }

    #[test]
    fn test_legacy_pgdmp_unencrypted_passes_transparently() {
        let key = [0x99u8; 32];
        let legacy_dump = b"PGDMP\x01\x02\x03legacy raw dump bytes";
        let decrypted = decrypt_backup(legacy_dump, &key).expect("legacy dump admitted");
        assert_eq!(decrypted, legacy_dump);
    }
}
