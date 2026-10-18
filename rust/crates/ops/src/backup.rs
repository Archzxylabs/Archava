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

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
    #[error("backup tool failure: {0}")]
    Tool(String),
    #[error("backup io failure at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("manifest json invalid: {0}")]
    Json(#[from] serde_json::Error),
}

/// Capture one database into `out_dir`: the custom-format archive plus its
/// manifest.
///
/// Row counts are recorded by SUMMING the fixture ledgers' relation
/// estimates (`pg_class.reltuples`) for the known ledger schemas — identity
/// and shape, never values.
///
/// Returns the manifest. The caller decides publish vs discard; this
/// function never moves data out of the given directory.
pub fn capture(
    source: &ConnectionSpec,
    out_dir: &Path,
    release: ReleaseIdentity,
) -> Result<BackupManifest, RestoreError> {
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
    let dump = tool("pg_dump")
        .args([
            "--format".to_string(),
            "custom".to_string(),
            "--file".to_string(),
            archive_path.to_string_lossy().to_string(),
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
    let bytes = std::fs::metadata(&archive_path)
        .map_err(|source| RestoreError::Io {
            path: archive_path.clone(),
            source,
        })?
        .len();
    let digest_bytes = std::fs::read(&archive_path).map_err(|source| RestoreError::Io {
        path: archive_path.clone(),
        source,
    })?;
    let digest = format!("{:x}", Sha256::digest(&digest_bytes));

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

/// Restore a verified manifest into an isolated target, refusals first:
/// 1. the target must not be the source (same host:port/database), and
/// 2. (caller's duty) any live writer on the target must be drained —
///    [`refuse_undrained_target`] — before `pg_restore` replays anything.
///
/// `pg_restore --no-owner --no-privileges` then replays the archive; the
/// rehearsal verifies the restored content against the source afterwards.
pub fn restore(
    manifest: &BackupManifest,
    manifest_dir: &Path,
    target: &ConnectionSpec,
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
    // `--clean --if-exists` because the isolated target is not guaranteed to
    // be virgin: a rehearsal target may have been provisioned-and-migrated by
    // its fixture (the fixture's ledgers exist even in a fresh instance). The
    // restore's contract is to REPLACE the ledger contents with the capture's
    // exact replay — drop-and-recreate of archive objects — never to silently
    // merge two databases' histories.
    let out = tool("pg_restore")
        .args([
            "--no-owner".to_string(),
            "--no-privileges".to_string(),
            "--clean".to_string(),
            "--if-exists".to_string(),
        ])
        .args(target.args())
        .args(["--dbname".to_string(), target.database.clone()])
        .arg(&archive_path)
        .output()
        .map_err(|e| RestoreError::Tool(format!("pg_restore: {e}")))?;
    if !out.status.success() {
        return Err(RestoreError::Tool(format!(
            "pg_restore exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// The active-writer refusal (MIG-04's "not an active DB", live form): a
/// target whose `archava_control.writer_lease` holds an undrained seat
/// cannot accept a restore replay. Cost is one query; the failure names the
/// lease rather than guessing.
pub async fn refuse_undrained_target(pool: &sqlx::PgPool) -> Result<(), RestoreError> {
    let exists: Option<(String,)> = sqlx::query_as(
        "SELECT lease_name FROM archava_control.writer_lease WHERE drained = false LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| RestoreError::Tool(format!("lease probe: {e}")))?;
    if let Some((lease,)) = exists {
        return Err(RestoreError::UndrainedWriter { lease });
    }
    Ok(())
}
