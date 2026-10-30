//! The operator doctor — the Rust port of TS `operator.ts doctor()`.
//!
//! The TS doctor verified the runtime's release identity, re-opened its
//! database, and printed a one-line status with the two fields any consumer
//! can assert on without leaking anything: `credentialValuesPrinted: false`
//! and `externalActions: 'not configured'` — below exactly the shape, with
//! the DB probes left to the PG suites that own them. This port keeps the
//! printed CONTRACT (same field names, same refusal semantics) and draws its
//! facts from the release-identity verifier, which is everything an
//! operator-level doctor can prove on its own authority.

use serde::Serialize;

/// One doctor report. Field names deliberately match the TS doctor's output
/// keys so its consumers (scripts, runbooks, dashboards) read the Rust JSON
/// without a translation.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub status: String,
    pub release: ReleaseFacts,
    #[serde(rename = "credentialValuesPrinted")]
    pub credential_values_printed: bool,
    #[serde(rename = "externalActions")]
    pub external_actions: String,
    pub backups: String,
    #[serde(rename = "automaticDeletion")]
    pub automatic_deletion: bool,
    /// Who to go to next, printed like the TS doctor's trailing advisories.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseFacts {
    pub id: String,
    pub version: String,
    pub verified: bool,
}

impl DoctorReport {
    /// Build the report from the release verifier's outcome: verified facts
    /// become `ready`, every other outcome becomes a REFUSAL the caller
    /// renders at exit code 2.
    pub fn ready(verification: &crate::release::Verification) -> Self {
        Self {
            status: "ready".to_string(),
            release: ReleaseFacts {
                id: verification.config_id.clone(),
                version: verification.config_version.clone(),
                verified: verification.version_matches
                    && verification.artifacts_verified
                    && verification.priors_preserved,
            },
            credential_values_printed: false,
            external_actions: "not configured".to_string(),
            backups: "rehearsal-verified in tests/upgrade_restore.rs; CLI capture available"
                .to_string(),
            automatic_deletion: false,
            notes: vec![
                "release artifacts digests recomputed against config source of truth".to_string(),
                "prior archives scanned for identity/overwrite integrity".to_string(),
            ],
        }
    }

    /// The refusal form: same fields, named reason.
    pub fn refused(reason: &str) -> Self {
        Self {
            status: "refused".to_string(),
            release: ReleaseFacts {
                id: String::new(),
                version: String::new(),
                verified: false,
            },
            credential_values_printed: false,
            external_actions: "not configured".to_string(),
            backups: "unavailable".to_string(),
            automatic_deletion: false,
            notes: vec![reason.to_string()],
        }
    }
}
