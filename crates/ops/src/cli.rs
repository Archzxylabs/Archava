//! The CLI dispatch. One subcommand at a time, one JSON line out, exit 0/2/1.
//!
//! The contract any consumer can rely on: `ok:true` objects print exactly
//! what they proved; refusal exit codes come with the same JSON shape plus a
//! `refused` field naming the reason. Nothing here reads an environment
//! variable for paths — the operator names every file — so a gate harness
//! can prove what ran.

use crate::backup::{
    capture_with_key, read_manifest, resolve_backup_key, restore_with_key, verify_archive,
    ConnectionSpec, ReleaseIdentity, RestoreError,
};
use crate::doctor::DoctorReport;
use crate::release::{self, CONFIG_PATH, PROFILE_PATH, RELEASES_DIR};
use crate::runbook;
use serde_json::json;
use std::path::{Path, PathBuf};

const USAGE: &str = "archava-ops <command> [flags]

Commands:
  status                       release identity summary from the config source of truth
  doctor                       the operator doctor probe (release verification facts)
  verify-release               recompute artifact digests, check prior archives, exit 0 only when verified
  plan-assembly --request R    plan a module selection against the release catalogue
  backup --port P --db D --out DIR [--user U] [--key K]
  restore --manifest M --port P --db D [--user U] [--key K]
  migrate --runtime-role ROLE  apply the verified native migration manifest as the owner connection
  bootstrap-owner --tenant T --principal P --username E
                               provision the first tenant owner (password from environment)
  bootstrap-payment-service --tenant T --principal P
                               provision a gateway-only Billing service
  resume-payment-creation --confirm-reconciled
                               release the payment hold after restore reconciliation
  drain-writer                 drain the serving writer using the explicit owner connection
  runbook                      print the replacement table this CLI owns
  help                         this text

Every path is named by the operator. Refusals exit 2 and print a JSON line
with their reason.";

/// Exit codes the CLI returns — small constants instead of a fresh enum, so
/// the shell contract stays grep-able.
pub const EXIT_OK: i32 = 0;
pub const EXIT_REFUSED: i32 = 2;
pub const EXIT_INTERNAL: i32 = 1;

/// Print one JSON line and hand back the process exit code.
fn emit(value: serde_json::Value, exit: i32) -> i32 {
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    );
    exit
}

fn refused(reason: String) -> i32 {
    emit(
        json!({
            "schemaVersion": crate::REPORT_SCHEMA,
            "ok": false,
            "refused": reason,
        }),
        EXIT_REFUSED,
    )
}

fn internal(reason: impl std::fmt::Display) -> i32 {
    emit(
        json!({
            "schemaVersion": crate::REPORT_SCHEMA,
            "ok": false,
            "error": reason.to_string(),
        }),
        EXIT_INTERNAL,
    )
}

/// A flag value lookup: `--flag VALUE`, `None` if absent.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    let idx = args.iter().position(|arg| arg == name)?;
    args.get(idx + 1).cloned()
}

/// The known-flag whitelist; an unknown `--anything` is a refusal, not a
/// silent ignore (the same discipline the release gate uses for its flags).
const KNOWN_FLAGS: &[&str] = &[
    "--request",
    "--catalog",
    "--config",
    "--port",
    "--db",
    "--user",
    "--out",
    "--manifest",
    "--key",
    "--runtime-role",
    "--tenant",
    "--principal",
    "--username",
];

pub fn run_cli(argv: Vec<String>) -> i32 {
    let Some(command) = argv.first().cloned() else {
        println!("{USAGE}");
        return EXIT_INTERNAL;
    };
    let rest: Vec<String> = argv[1..].to_vec();
    for seen in &rest {
        if seen.starts_with("--") && !KNOWN_FLAGS.contains(&seen.as_str()) {
            return refused(format!("unknown flag {seen} (known: {KNOWN_FLAGS:?})"));
        }
    }
    match command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            EXIT_OK
        }
        "status" => status_command(&root()),
        "doctor" => doctor_command(&root()),
        "verify-release" => verify_command(&root()),
        "plan-assembly" => plan_command(&root(), &rest),
        "backup" => backup_command(&root(), &rest),
        "restore" => restore_command(&root(), &rest),
        "migrate"
        | "bootstrap-owner"
        | "bootstrap-payment-service"
        | "resume-payment-creation"
        | "drain-writer" => install_command(&root(), &command, &rest),
        "runbook" => emit(
            json!({
                "schemaVersion": crate::REPORT_SCHEMA,
                "ok": true,
                "commands": runbook::table(),
            }),
            EXIT_OK,
        ),
        other => refused(format!(
            "unknown command {other:?} — see `archava-ops help`"
        )),
    }
}

fn root() -> PathBuf {
    std::env::var("ARCHAVA_OPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// The verifier's one response: every check recomputed here, the caller sees
/// the full record. Used by both `status`/`doctor` and `verify-release`.
fn verify(root_dir: &Path) -> Result<release::Verification, release::VerifyError> {
    let native = root_dir.join("release-profile.json");
    let (profile, archives) = if native.exists() {
        (native, root_dir.join("releases"))
    } else {
        (root_dir.join(PROFILE_PATH), root_dir.join(RELEASES_DIR))
    };
    release::verify(&root_dir.join(CONFIG_PATH), &profile, &archives)
}

fn status_command(root_dir: &Path) -> i32 {
    match verify(root_dir) {
        Ok(verification) => emit(
            json!({
                "schemaVersion": crate::REPORT_SCHEMA,
                "ok": true,
                "status": if verification.version_matches && verification.artifacts_verified && verification.priors_preserved {"ready"}else{"unverified"},
                "release": {
                    "id": verification.config_id,
                    "version": verification.config_version,
                    "profileVersion": verification.profile_version,
                    "versionMatches": verification.version_matches,
                    "artifactsVerified": verification.artifacts_verified,
                    "priorArchivesPreserved": verification.priors_preserved,
                    "artifactCount": verification.artifacts.len(),
                    "priorArchiveCount": verification.prior_archives.len(),
                },
                "credentialValuesPrinted": false,
            }),
            EXIT_OK,
        ),
        Err(error) => refused(error.to_string()),
    }
}

fn doctor_command(root_dir: &Path) -> i32 {
    match verify(root_dir) {
        Ok(verification) => {
            let all_verified = verification.version_matches
                && verification.artifacts_verified
                && verification.priors_preserved;
            let report: DoctorReport = if all_verified {
                DoctorReport::ready(&verification)
            } else {
                DoctorReport::refused("release identity did not verify (see notes)")
            };
            let exit = if all_verified { EXIT_OK } else { EXIT_REFUSED };
            emit(serde_json::to_value(&report).unwrap_or(json!({})), exit)
        }
        Err(error) => refused(error.to_string()),
    }
}

fn verify_command(root_dir: &Path) -> i32 {
    match verify(root_dir) {
        Ok(verification) => {
            let verified = verification.version_matches
                && verification.artifacts_verified
                && verification.priors_preserved;
            emit(
                json!({
                    "schemaVersion": crate::REPORT_SCHEMA,
                    "ok": verified,
                    "verification": serde_json::to_value(&verification).unwrap_or(json!({})),
                    "refused": if verified { serde_json::Value::Null } else { json!("release identity did not verify") },
                }),
                if verified { EXIT_OK } else { EXIT_REFUSED },
            )
        }
        Err(error) => refused(error.to_string()),
    }
}

fn plan_command(root_dir: &Path, args: &[String]) -> i32 {
    let Some(request_path) = flag_value(args, "--request") else {
        return refused("plan-assembly requires --request <path to request JSON>".to_string());
    };
    let bytes = match std::fs::read(&request_path) {
        Ok(bytes) => bytes,
        Err(err) => return internal(format!("request unreadable at {request_path}: {err}")),
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => return refused(format!("request JSON invalid: {err}")),
    };
    let catalog_bytes = match std::fs::read(root_dir.join(CONFIG_PATH)) {
        Ok(bytes) => bytes,
        Err(err) => return internal(format!("catalog unreadable at {CONFIG_PATH}: {err}")),
    };
    let catalog: archava_assembly::ReleaseCatalog = match serde_json::from_slice(&catalog_bytes) {
        Ok(catalog) => catalog,
        Err(err) => return refused(format!("catalog JSON invalid: {err}")),
    };
    let request: archava_assembly::AssemblyRequest = match serde_json::from_value(parsed) {
        Ok(request) => request,
        Err(err) => return refused(format!("request shape invalid: {err}")),
    };
    match archava_assembly::plan(&request, &catalog) {
        Ok(plan) => {
            let installation = plan.installation.len();
            let entries = plan.entries.len();
            let authority = plan.authority.clone();
            let unresolved = plan.unresolved_legacy_names.clone();
            let detail: serde_json::Value = serde_json::to_value(&plan).unwrap_or(json!({}));
            emit(
                json!({
                    "schemaVersion": crate::REPORT_SCHEMA,
                    "ok": true,
                    "planned": true,
                    "profile": plan.profile,
                    "installationCount": installation,
                    "entryCount": entries,
                    "authority": authority,
                    "unresolvedLegacyNames": unresolved,
                    "plan": detail,
                }),
                EXIT_OK,
            )
        }
        Err(error) => refused(format!("assembly plan refused: {error}")),
    }
}

fn connection_from(args: &[String]) -> Result<ConnectionSpec, String> {
    let port = flag_value(args, "--port")
        .ok_or("backup/restore requires --port <number>")?
        .parse::<u16>()
        .map_err(|err| format!("--port <number> invalid: {err}"))?;
    let database = flag_value(args, "--db").ok_or("backup/restore requires --db <name>")?;
    // Deliberately NOT defaulted: the caller names the cluster-admin role
    // every time, because a capture under the wrong role fails on
    // FORCE-RLS-protected rather than silently capturing a partial
    // database.
    let user = flag_value(args, "--user").ok_or(
        "backup/restore requires --user <cluster-admin role> (ledger contents are FORCE-RLS protected)",
    )?;
    Ok(ConnectionSpec {
        host: "127.0.0.1".to_string(),
        port,
        database,
        user,
    })
}

// Shared release gate for stateful operator commands; read-only status keeps the full report.
fn operation_release(root_dir: &Path) -> Result<release::Verification, String> {
    let result = verify(root_dir).map_err(|error| error.to_string())?;
    if !result.version_matches || !result.artifacts_verified || !result.priors_preserved {
        return Err("release identity, artifacts or prior archives did not verify".to_string());
    }
    Ok(result)
}

fn backup_command(root_dir: &Path, args: &[String]) -> i32 {
    let spec = match connection_from(args) {
        Ok(spec) => spec,
        Err(reason) => return refused(reason),
    };
    let out = match flag_value(args, "--out") {
        Some(dir) => PathBuf::from(dir),
        None => return refused("backup requires --out <directory>".to_string()),
    };
    // The release identity the backup carries comes from the same verifier
    // `verify-release` uses — a backup cannot claim an identity the tree
    // refuses on its own probe.
    let release = match operation_release(root_dir) {
        Ok(verification) => ReleaseIdentity {
            id: verification.config_id,
            version: verification.config_version,
        },
        Err(error) => return refused(format!("backup requires a verifying release: {error}")),
    };
    let explicit_key = flag_value(args, "--key");
    let key = match resolve_backup_key(explicit_key.as_deref()) {
        Ok(k) => Some(k),
        Err(err) => return refused(err.to_string()),
    };
    match capture_with_key(&spec, &out, release, key.as_ref()) {
        Ok(manifest) => {
            let path = out.join("archava-backup.manifest.json");
            let bytes = serde_json::to_vec_pretty(&manifest)
                .map_err(RestoreError::Json)
                .and_then(|body| {
                    std::fs::write(&path, body).map_err(|source| RestoreError::Io {
                        path: path.clone(),
                        source,
                    })
                });
            match bytes {
                Ok(()) => emit(
                    json!({
                        "schemaVersion": crate::REPORT_SCHEMA,
                        "ok": true,
                        "backup": {
                            "manifestPath": path.to_string_lossy(),
                            "archiveBytes": manifest.archive.bytes,
                            "archiveSha256": manifest.archive.sha256,
                            "credentialValuesPrinted": manifest.credential_values_printed,
                            "restoreRehearsalVerified": manifest.restore_rehearsal_verified,
                        }
                    }),
                    EXIT_OK,
                ),
                Err(error) => internal(error),
            }
        }
        Err(error) => refused(error.to_string()),
    }
}

fn restore_command(root_dir: &Path, args: &[String]) -> i32 {
    if let Err(error) = operation_release(root_dir) {
        return refused(format!("restore requires a verifying release: {error}"));
    }
    let target = match connection_from(args) {
        Ok(spec) => spec,
        Err(reason) => return refused(reason),
    };
    let Some(manifest_path) = flag_value(args, "--manifest") else {
        return refused("restore requires --manifest <path>".to_string());
    };
    let manifest_dir = PathBuf::from(&manifest_path);
    let dir = manifest_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let explicit_key = flag_value(args, "--key");
    let key = match resolve_backup_key(explicit_key.as_deref()) {
        Ok(key) => Some(key),
        Err(error) => return refused(error.to_string()),
    };

    let manifest = match read_manifest(Path::new(&manifest_path)) {
        Ok(manifest) => manifest,
        Err(error) => return refused(error.to_string()),
    };
    if target.host == manifest.source.host
        && target.port == manifest.source.port
        && target.database == manifest.source.database
    {
        return refused("restore target must be separate from the capture source".into());
    }
    if let Err(error) = verify_archive(&manifest, &dir) {
        return refused(error.to_string());
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return refused("restore isolation runtime unavailable".into()),
    };
    let guard = match rt.block_on(crate::restore_target::RestoreTargetGuard::acquire(&target)) {
        Ok(guard) => guard,
        Err(error) => return refused(error.to_string()),
    };
    let restored = restore_with_key(&manifest, &dir, &target, key.as_ref());
    if matches!(
        restored,
        Err(crate::backup::RestoreError::WriterInvalidation
            | crate::backup::RestoreError::ReplayUnconfirmed { .. })
    ) {
        // Fail closed: the connection limit stays zero; dropping the admin
        // session releases only its advisory lock. Operator inspection is needed.
        return refused("restore replay was not confirmed or its writer could not be invalidated; target remains isolated for administrator inspection".into());
    }
    // Always restore the operator's original connection limit, also on pg_restore failure.
    if let Err(error) = rt.block_on(guard.release()) {
        return refused(error.to_string());
    }
    match restored {
        Ok(()) => emit(
            json!({"schemaVersion":crate::REPORT_SCHEMA,"ok":true,"restored":{"target":{"host":target.host,"port":target.port,"database":target.database},"archiveSha256":manifest.archive.sha256,"targetConnectionsExcluded":true}}),
            EXIT_OK,
        ),
        Err(error) => refused(error.to_string()),
    }
}

fn install_command(root: &Path, command: &str, args: &[String]) -> i32 {
    if let Err(error) = operation_release(root) {
        return refused(format!(
            "installation requires a verifying release: {error}"
        ));
    }
    let url = match std::env::var("ARCHAVA_MIGRATION_DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => url,
        _ => {
            return refused(
                "ARCHAVA_MIGRATION_DATABASE_URL must name an explicit owner connection".into(),
            )
        }
    };
    let runtime = tokio::runtime::Runtime::new().expect("operator runtime");
    let result: Result<serde_json::Value, String> = runtime.block_on(async {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&url)
            .await
            .map_err(|_| "owner database connection unavailable".to_string())?;
        if command == "migrate" {
            let role =
                flag_value(args, "--runtime-role").ok_or("migrate requires --runtime-role")?;
            let report = crate::install::migrate(&pool, root, &role)
                .await
                .map_err(|e| e.to_string())?;
            pool.close().await;
            Ok(json!({"migration":report}))
        } else if command == "bootstrap-payment-service" {
            let tenant = flag_value(args, "--tenant")
                .ok_or("bootstrap-payment-service requires --tenant")?;
            let principal = flag_value(args, "--principal")
                .ok_or("bootstrap-payment-service requires --principal")?;
            crate::install::bootstrap_payment_service(&pool, &tenant, &principal)
                .await
                .map_err(|e| e.to_string())?;
            pool.close().await;
            Ok(json!({"paymentService":{"tenant":tenant,"principal":principal}}))
        } else if command == "resume-payment-creation" {
            if args != ["--confirm-reconciled"] { return Err("resume-payment-creation requires --confirm-reconciled after reconciling the merchant dashboard".into()); }
            crate::install::resume_payment_creation(&pool).await.map_err(|e|e.to_string())?;
            pool.close().await;
            Ok(json!({"paymentCreation":{"held":false}}))
        } else if command == "drain-writer" {
            if !args.is_empty() {
                return Err("drain-writer accepts no flags".into());
            }
            let epoch = crate::install::drain_writer(&pool)
                .await
                .map_err(|e| e.to_string())?;
            pool.close().await;
            Ok(json!({"writer":{"drained":epoch.is_some(),"epoch":epoch}}))
        } else {
            let tenant = flag_value(args, "--tenant").ok_or("bootstrap-owner requires --tenant")?;
            let principal =
                flag_value(args, "--principal").ok_or("bootstrap-owner requires --principal")?;
            let username =
                flag_value(args, "--username").ok_or("bootstrap-owner requires --username")?;
            let password = std::env::var("ARCHAVA_OWNER_PASSWORD")
                .map_err(|_| "ARCHAVA_OWNER_PASSWORD is required")?;
            crate::install::bootstrap_owner(&pool, &tenant, &principal, &username, &password)
                .await
                .map_err(|e| e.to_string())?;
            pool.close().await;
            Ok(json!({"owner":{"tenant":tenant,"principal":principal,"username":username}}))
        }
    });
    match result {
        Ok(value) => emit(
            json!({"schemaVersion":crate::REPORT_SCHEMA,"ok":true,"installation":value}),
            EXIT_OK,
        ),
        Err(error) => refused(error),
    }
}

#[cfg(test)]
mod release_gate_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Sandbox(PathBuf);
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Sandbox {
        let root =
            std::env::temp_dir().join(format!("archava-release-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("dist/library")).unwrap();
        std::fs::write(
            root.join(CONFIG_PATH),
            r#"{"id":"fixture","version":"0.1.0"}"#,
        )
        .unwrap();
        std::fs::write(root.join("dist/library/fixture.bin"), b"fixture artifact").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"fixture artifact"));
        let profile = json!({"catalog":{"id":"fixture","version":"0.1.0"},"artifacts":[{"name":"fixture","version":"0.1.0","tarball":"fixture.bin","sha256":digest}]});
        std::fs::write(
            root.join(PROFILE_PATH),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        Sandbox(root)
    }
    #[test]
    fn a_verified_release_can_authorize_an_operator_operation() {
        let f = fixture();
        assert!(operation_release(&f.0).is_ok());
    }
    #[test]
    fn changed_artifact_bytes_cannot_authorize_backup_or_restore() {
        let f = fixture();
        std::fs::write(f.0.join("dist/library/fixture.bin"), b"modified artifact").unwrap();
        assert!(
            operation_release(&f.0).is_err(),
            "a parsed profile is not a verified release"
        );
    }
    #[test]
    fn wrong_identity_or_version_cannot_authorize_backup_or_restore() {
        let f = fixture();
        for config in [
            json!({"id":"other","version":"0.1.0"}),
            json!({"id":"fixture","version":"0.2.0"}),
        ] {
            std::fs::write(f.0.join(CONFIG_PATH), serde_json::to_vec(&config).unwrap()).unwrap();
            assert!(
                operation_release(&f.0).is_err(),
                "both catalog id and version must match"
            );
        }
    }
    #[test]
    fn an_empty_artifact_set_cannot_authorize_backup_or_restore() {
        let f = fixture();
        std::fs::write(
            f.0.join(PROFILE_PATH),
            r#"{"catalog":{"id":"fixture","version":"0.1.0"},"artifacts":[]}"#,
        )
        .unwrap();
        assert!(
            operation_release(&f.0).is_err(),
            "there must be verified artifacts"
        );
    }
    #[test]
    fn native_distribution_uses_its_own_profile_and_refuses_tampering() {
        let f = fixture();
        std::fs::rename(
            f.0.join("dist/library/fixture.bin"),
            f.0.join("fixture.bin"),
        )
        .unwrap();
        std::fs::copy(f.0.join(PROFILE_PATH), f.0.join("release-profile.json")).unwrap();
        assert!(operation_release(&f.0).is_ok());
        std::fs::write(f.0.join("fixture.bin"), b"changed").unwrap();
        assert!(operation_release(&f.0).is_err());
        // A broken native profile must not silently fall back to a legacy profile.
        std::fs::write(f.0.join("release-profile.json"), b"invalid json").unwrap();
        assert!(operation_release(&f.0).is_err());
    }
}
