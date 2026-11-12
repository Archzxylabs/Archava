# Module registry

`config/modules.v1.json` declares each installed business module once. It is the single place that says a module exists, which schema and ledger it owns, where its source lives, and what Ava may route to and read from it. Everything else either reads that declaration or is checked against it.

The registry describes wiring. It is not a grant and not acceptance: domain transactions still re-authorize every action, and a completely registered module still needs its own integration and browser evidence.

## Descriptor

```json
{
  "id": "laundry",
  "label": "Laundry",
  "namespace": "archava_laundry",
  "ledger": "laundry-pg",
  "source": "laundry",
  "suite": "erp_laundry",
  "spec": "docs/modules/LAUNDRY_MODULE.md",
  "readPath": "/erp/laundry/orders",
  "uiPath": "/app/erp?module=laundry",
  "control": "module-laundry-create",
  "tab": "Laundry",
  "typedDetail": true,
  "agent": { "rank": 5, "keywords": ["laundry", "linen", "cucian"], "collections": ["orders"] }
}
```

| Field | Meaning |
| --- | --- |
| `id` | Public id and the `/erp/<id>/…` route segment. Lower-case alphanumeric, unique. |
| `namespace` | PostgreSQL schema that owns the module's tables. Unique; must match the form catalogue and the migration manifest. |
| `ledger` | Directory under `migrations/`. |
| `source` | File stem under `erp/` in the domain, adapters and host crates. |
| `suite` | Integration suite in `crates/host/tests/`. |
| `spec` | The module's maintained specification. |
| `readPath`, `uiPath`, `control` | The read route, workspace URL and create control the product audit and browser gates exercise. |
| `tab` | Optional. Label of the module's tab in the Operations workspace, for modules served by the generic operator workspace. Modules with a dedicated view omit it. Tabs appear in registry order. |
| `typedDetail` | Optional. `true` when the workspace read accepts a record id for a typed detail view. |
| `agent.rank` | Position in keyword routing; lower ranks match first. Unique. |
| `agent.keywords` | Lower-case phrases that route an utterance to the module. |
| `agent.collections` | Response collections the provider projection may read. Anything not listed stays withheld. |

## What reads the registry

| Consumer | Uses |
| --- | --- |
| [`archava_domain::modules`](../../crates/domain/src/modules.rs) | Typed registry compiled from the embedded snapshot |
| [Agent keyword routing](../../crates/host/src/agent/planner.rs) | `agent.rank`, `agent.keywords` |
| [Provider projection](../../crates/host/src/agent/disclosure/fields.rs) | `agent.collections` |
| [Product catalogue](../../tools/product/catalogue.mjs) | Module list for the product audit and browser gates |
| [Operations tabs](../../crates/slice/src/operations/mod.rs) and [detail loading](../../crates/slice/src/operations/operator.rs) | `tab`, `typedDetail` |
| [Workspace domains](../../crates/adapters/src/erp/projections.rs) | `namespace` for each workspace's tenant transaction and module-enabled check |

The domain crate compiles a byte-identical snapshot from `crates/domain/embedded/`. After editing the registry run `node tools/rust/domain-assets.mjs --write`; the structure gate and build provenance refuse a stale snapshot.

## What is checked against it

These registration points cannot be derived from data, because they are Rust module declarations, handler references or compile-time embeds. [`tools/check-modules.mjs`](../../tools/check-modules.mjs) verifies each one for every module and names the file to change when one is missing. It runs in `tools/rust/verify.mjs` and CI.

| Point | Where |
| --- | --- |
| Form catalogue with the same schema | `config/operator-actions.v1.json` |
| Migration directory and manifest ledger | `migrations/<ledger>/`, `migrations/manifest.v1.json` |
| Embedded migrations and readiness | `crates/slice/src/bin/archava_server.rs` |
| Fixture ledger mapping | `crates/pg-fixture/src/lib.rs` |
| Source file and `mod` declaration, per layer | `crates/{domain,adapters,host}/src/erp/` |
| Routes under `/erp/<id>/` and the declared read route | `crates/host/src/routes/erp.rs` |
| Extractor coverage | `crates/host/src/extractors.rs` |
| Agent command dispatch | `crates/host/src/business.rs` |
| Integration suite and specification | `crates/host/tests/`, `docs/modules/` |

The gate also refuses the reverse case: a module that has forms or an agent dispatch arm but no descriptor.

## Adding a module

1. Add its descriptor to `config/modules.v1.json` and its forms to `config/operator-actions.v1.json`, then run `node tools/rust/domain-assets.mjs --write`.
2. Run `node tools/check-modules.mjs --explain <id>`. It lists every registration point as present or missing, with the file and what to add.
3. Work through the missing points until the command reports the module completely registered. Migrations follow the four-step registration in `AGENTS.md`.
4. Run `node tools/rust/verify.mjs`, then add the module's own integration evidence.

## Not yet covered

The registry does not yet drive, and the gate does not yet check, these per-module points. They remain hand-maintained:

- The `WorkspaceDomain` variants, their collection tables and access resolvers in `crates/adapters/src/erp/projections.rs`, and the typed operator workspace in `crates/adapters/src/erp/operator.rs`. The host mounts `/erp/<id>/workspace` for every entry of `WorkspaceDomain::ALL`, so the variant list is the only place a workspace domain is enumerated.
- The ten Operations tabs with dedicated views (catalog, sales, purchasing, invoices, accounting, support, missions, suggestions, import, voice) and their view dispatch in `crates/slice/src/operations/mod.rs`. Tabs for modules served by the generic operator workspace come from the registry.
- Owner-chat module aliases in `crates/host/src/own.rs` and the assembly contracts in `crates/assembly`, which use the assembly vocabulary (`crm-contacts`, `fieldServices`) rather than these ids.
- Typed form references in `crates/domain/src/operator.rs`.
- Fixture migration lists and runtime grants beyond the schema-to-directory mapping.

The server bootstrap and the fixture still each hold their own ledger registration; the gate checks both against the manifest but does not remove the duplication.
