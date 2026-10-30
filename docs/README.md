# Archava documentation

Start with [developer setup and commands](DEVELOPER_GUIDE.md), [current architecture](architecture/README.md), or [host/browser integration](guides/INTEGRATION.md). These are maintained references for the root Rust workspace.

Work toward E2E and release readiness follows the [launch readiness plan](product/LAUNCH_READINESS.md): scope, ordered phases, module/user-flow acceptance, evidence, and handoff.

| Collection | Use it for |
| --- | --- |
| [Architecture](architecture/README.md) | Crate boundaries, runtime composition, subsystem source maps and implementation limits |
| [Guides](guides/INTEGRATION.md) | Host authentication/request contracts and browser embedding |
| [Modules](modules/README.md) | Separate business-module contracts with current Rust source pointers and dated acceptance context |
| [Product](product/README.md) | Requirements, roadmaps and dated research; proposed scope is distinct from implementation |
| [Design](design/README.md) | Implemented ERP/CRM appearance, visual studies and UI snapshots |
| [Archive](archive/README.md) | Earlier contracts, audits, evidence, superseded guides and Rust migration records |

## Verification and document ownership

The [consolidation record](archive/DOCS_CONSOLIDATION_2026-10-29.md) reports checks run for the documentation reorganization. The [preceding verification checkpoint](archive/audits/BUILD_STATUS_2026-10-29.md) retains its actual Rust, database, UI and docs test scopes; those application checks were not rerun merely because a document moved.

Use [deployment](../deploy/README.md) for persistent host packaging and [configuration ownership](../config/README.md) for shared tooling settings versus crate fixtures. The current [workspace appearance](design/WORKSPACE_APPEARANCE.md) owns the ERP/CRM visual implementation.

Keep one canonical page for each topic. Update source references with code, and record implementation/verification separately from plans. Preserve dated evidence rather than editing old pass/fail results. The archive's [relocation manifest](archive/relocations-2026-10-29.json) maps former documentation paths to their current location.

Run `node tools/check-docs.mjs` for maintained Markdown file/heading links. Frozen archive bodies are excluded; their maintained navigation indexes are checked. This gate does not test external sites or certify product completeness.
