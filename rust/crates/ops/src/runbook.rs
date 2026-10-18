//! The operator runbook registry — the honest front door for two of R10's
//! three P-CLI-OPERATIONS checks.
//!
//! "Operator commands replaced with Rust equivalents" and "runbook commands
//! executed" are both checkable ONLY if the table of replacements is itself
//! machine-readable and the gate EXECUTES it. `table()` is the table; the
//! producer (`tools/rust/release-ops.mjs`) runs every DOMESTIC command
//! through the built binary and every REHEARSAL command through the test
//! binary, and records each row's exit. A row that neither runs nor is
//! excused with a recorded reason fails the check both ways.

/// One replacement row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReplacedCommand {
    /// The Rust command the runbook names, spelled as an operator would type
    /// or execute it.
    pub command: String,
    /// The first-party surface it replaces, with the file the surface lived
    /// in before R10.
    pub replaces: String,
    /// What executing the row proves — written to be quoted in evidence.
    pub proves: String,
    /// How the proof path executes the row: `cli` (subprocess through the
    /// built binary) or `cargo-test` (the rehearsal suite).
    pub kind: String,
}

/// The full replacement table, in the order the operator's runbook lists it.
///
/// Byte-for-byte honesty clock: rows whose "replacement" is a rehearsal are
/// rows for duties the TS operator had ISOLATED (backup/restore/upgrade),
/// and their execute path is the cargo suite that provisions two disposable
/// databases and runs the isolation rules against them.
pub fn table() -> Vec<ReplacedCommand> {
    vec![
        ReplacedCommand {
            command: "archava-ops status".to_string(),
            replaces: "apps/crm operator `status` (release identity + workspace facts)"
                .to_string(),
            proves: "the runtime's release identity prints from the config source of truth, without credential values".to_string(),
            kind: "cli".to_string(),
        },
        ReplacedCommand {
            command: "archava-ops doctor".to_string(),
            replaces: "apps/crm operator `doctor` (release verification + reporting)".to_string(),
            proves: "doctor reports ready only when every artifact digest and prior archive verified".to_string(),
            kind: "cli".to_string(),
        },
        ReplacedCommand {
            command: "archava-ops verify-release".to_string(),
            replaces:
                "release-side dist↔config identity checks (pack/manifest verify surface)"
                    .to_string(),
            proves: "version match, per-artifact digest recompute, prior-archive integrity, no-overwrite".to_string(),
            kind: "cli".to_string(),
        },
        ReplacedCommand {
            command: "archava-ops plan-assembly".to_string(),
            replaces: "assembly planning CLI (`packages/config/src/assembly.ts` planAssembly path)"
                .to_string(),
            proves: "a module selection plans with the port's resolution, authority and unresolved-legacy fields".to_string(),
            kind: "cli".to_string(),
        },
        ReplacedCommand {
            command: "archava-ops runbook".to_string(),
            replaces: "the first-party runbook listing itself (this table)".to_string(),
            proves: "the operator can name every replaced command and read what it proves".to_string(),
            kind: "cli".to_string(),
        },
        ReplacedCommand {
            command: "cargo test -p archava-ops --test upgrade_restore (fresh/prior upgrade, backup capture→isolated restore, fenced handover, rollback)".to_string(),
            replaces: "operator `backup`/`restore` + upgrade orchestration against memory-resident TS fixtures (apps/crm/scripts/upgrade.mjs)".to_string(),
            proves: "fresh install upgrade, prior-profile upgrade, backup→isolated restore refusal rules, fenced single-writer handover, drained rollback rehearsal (MIG-03..MIG-05, MIG-08)".to_string(),
            kind: "cargo-test".to_string(),
        },
    ]
}
