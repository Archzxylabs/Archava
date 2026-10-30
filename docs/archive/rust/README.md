# Frozen Rust migration records

These records describe the migration and original cutover snapshots. They retain paths such as `rust/crates/`, older proof producers and the statuses recorded at that time. They are not current runbooks, and this cleanup does not refresh their acceptance verdicts.

Use [current architecture](../../architecture/README.md), [developer commands](../../DEVELOPER_GUIDE.md), [verification status](../audits/BUILD_STATUS_2026-10-29.md) and [deployment](../../../deploy/README.md) for the root workspace.

| Document | Purpose |
| --- | --- |
| [ARCHXY1_ACCEPTANCE_MATRIX.md](ARCHXY1_ACCEPTANCE_MATRIX.md) | ARCHXY 1 — Acceptance Matrix (traceability index) |
| [CUTOVER_STATUS.md](CUTOVER_STATUS.md) | Rust-first cutover status |
| [DECISIONS.md](DECISIONS.md) | Rust migration decisions — ADR log |
| [EXECUTION_STATE.md](EXECUTION_STATE.md) | Rust migration — execution state |
| [FULL_RUST_AUDIT_PACKET.md](FULL_RUST_AUDIT_PACKET.md) | Full Rust audit packet |
| [FULL_RUST_COMPLETION_REPORT.md](FULL_RUST_COMPLETION_REPORT.md) | Full Rust completion report |
| [KNOWN_DIFFERENCES.md](KNOWN_DIFFERENCES.md) | Known differences and corrected expectations |
| [MIGRATION_INVENTORY.md](MIGRATION_INVENTORY.md) | Rust migration inventory — R00 baseline |
| [PERSISTENT_RUNTIME.md](PERSISTENT_RUNTIME.md) | Persistent Rust runtime |

| Additional record | Purpose |
| --- | --- |
| [Migration inventory JSON](migration-inventory.json) | Frozen machine-readable baseline |
| [Required coverage JSON](required-coverage.json) | Frozen coverage contract, not today's measured test result |
| [ERP scope](archxy2/ERP_SCOPE.md) | Historical behavior/contract/module/proof scope |
| [ERP execution state](archxy2/EXECUTION_STATE.md) | Historical ERP migration progress |
| [Integration requests](archxy2/INTEGRATION_REQUESTS.md) | Historical handoff packet requests |

Original records retain their bytes after moving under `archive/rust/`. This maintained index gives current destinations; the [relocation manifest](../relocations-2026-10-29.json) maps pre-consolidation paths.
