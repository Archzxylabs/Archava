//! The CLI dispatch. One subcommand at a time, one JSON line out, exit 0/2/1.
//!
//! The contract any consumer can rely on: `ok:true` objects print exactly
//! what they proved; refusal exit codes come with the same JSON shape plus a
//! `refused` field naming the reason. Nothing here reads an environment
//! variable for paths — the operator names every file — so a gate harness
//! can prove what ran.

use crate::backup::{
    capture, read_manifest, restore, verify_archive, ConnectionSpec, ReleaseIdentity, RestoreError,
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
  backup --port P --db D --out DIR [--user U]
  restore --manifest M --port P --db D [--user U]
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
    release::verify(
        &root_dir.join(CONFIG_PATH),
        &root_dir.join(PROFILE_PATH),
        &root_dir.join(RELEASES_DIR),
    )
}

fn status_command(root_dir: &Path) -> i32 {
    match verify(root_dir) {
        Ok(verification) => emit(
            json!({
                "schemaVersion": crate::REPORT_SCHEMA,
                "ok": true,
                "status": "ready",
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
    let release = match verify(root_dir) {
        Ok(verification) => ReleaseIdentity {
            id: verification.config_id,
            version: verification.config_version,
        },
        Err(error) => return refused(format!("backup requires a verifying release: {error}")),
    };
    match capture(&spec, &out, release) {
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
    if let Err(error) = verify(root_dir) {
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
    match read_manifest(Path::new(&manifest_path)) {
        Ok(manifest) => match restore(&manifest, &dir, &target) {
            Ok(()) => match verify_archive(&manifest, &dir) {
                Ok(()) => emit(
                    json!({
                        "schemaVersion": crate::REPORT_SCHEMA,
                        "ok": true,
                        "restored": {
                            "target": { "host": target.host, "port": target.port, "database": target.database },
                            "archiveSha256": manifest.archive.sha256,
                        }
                    }),
                    EXIT_OK,
                ),
                Err(error) => refused(error.to_string()),
            },
            Err(error) => refused(error.to_string()),
        },
        Err(error) => refused(error.to_string()),
    }
}
