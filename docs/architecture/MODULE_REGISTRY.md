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
| `agent.aggregates` | Optional. Aggregate lookups Ava may request, one entry per collection (which must be in `agent.collections`): `sums` (integer amount fields a sum may name; a count is always offered), `dimensions` (`{id, class}`: a group-by and the AI-sharing data category of its labels, `business_facts` or `names`), `filters`, and `date` (which date `dateFrom`/`dateTo` and `month` read). Names only: the SQL behind each one is fixed in the adapters. Anything not listed is refused. |
| `owner.aliases` | Optional. Other single lower-case words an owner may call the module by when configuring the workspace in chat (`hotel`, `penginapan`). The id always counts. A word names one module. |
| `owner.requires` | Optional. Module ids that must be enabled for this module to be enabled. A module that names none requires only the mandatory base, `crm-contacts`. Describing a business in chat enables these with the module. |

## What reads the registry

| Consumer | Uses |
| --- | --- |
| [`archava_domain::modules`](../../crates/domain/src/modules.rs) | Typed registry compiled from the embedded snapshot |
| [Agent keyword routing](../../crates/host/src/agent/planner.rs) | `agent.rank`, `agent.keywords` |
| [Provider projection](../../crates/host/src/agent/disclosure/fields.rs) | `agent.collections` |
| [Aggregate lookups](../../crates/host/src/agent/aggregate.rs) and [their SQL](../../crates/adapters/src/erp/aggregates.rs) | `agent.aggregates`: what the planner is offered and what a request is checked against; the adapter refuses a name it does not implement and a unit test keeps the two lists equal |
| [Product catalogue](../../tools/product/catalogue.mjs) | Module list for the product audit and browser gates |
| [Operations tabs](../../crates/slice/src/operations/mod.rs) and [detail loading](../../crates/slice/src/operations/operator.rs) | `tab`, `typedDetail` |
| [Workspace domains](../../crates/adapters/src/erp/projections.rs) | `namespace` for each workspace's tenant transaction and module-enabled check |
| [Owner configuration](../../crates/host/src/own.rs) | `owner.aliases` for the chat grammar, `owner.requires` for the owner catalogue |
| [Host build script](../../crates/host/build.rs) | `id`, `source`: generates `for_each_erp_module!`, which [agent dispatch](../../crates/host/src/business.rs) and the [command routes](../../crates/host/src/routes/erp.rs) expand |

Agent dispatch and the routes `POST /erp/<id>/commands` and `GET /erp/<id>/commands/status` are generated, not listed. A module's host source must expose the two handlers as `command` and `status`; with those in place a descriptor is enough for the module to be mounted and for approved agent plans to reach it. A missing handler is a compile error naming `crate::erp::<source>::command`.

The domain crate compiles a byte-identical snapshot from `crates/domain/embedded/`. After editing the registry run `node tools/rust/domain-assets.mjs --write`; the structure gate and build provenance refuse a stale snapshot.

## What is checked against it

These registration points cannot be derived from data, because they are Rust module declarations, handler references or compile-time embeds. [`tools/check-modules.mjs`](../../tools/check-modules.mjs) verifies each one for every module and names the file to change when one is missing. It runs in `tools/rust/verify.mjs` and CI.

| Point | Where |
| --- | --- |
| Form catalogue with the same schema | `config/operator-actions.v1.json` |
| Migration directory and manifest ledger | `migrations/<ledger>/`, `migrations/manifest.v1.json` |
| The ledger is not excluded from the embedded migrations | `crates/slice/build.rs` |
| The fixture applies the ledger | `crates/pg-fixture/src/lib.rs` |
| Source file and `mod` declaration, per layer | `crates/{domain,adapters,host}/src/erp/` |
| `command` and `status` handlers | `crates/host/src/erp/<source>.rs` |
| The declared read route | `crates/host/src/routes/erp.rs` |
| Extractor coverage | `crates/host/src/extractors.rs` |
| Integration suite and specification | `crates/host/tests/`, `docs/modules/` |

The gate also refuses the reverse case: a module that has forms but no descriptor, and a command route or dispatch arm written by hand beside the generated ones.

## Adding a module

1. Add its descriptor to `config/modules.v1.json` and its forms to `config/operator-actions.v1.json`, then run `node tools/rust/domain-assets.mjs --write`.
2. Run `node tools/check-modules.mjs --explain <id>`. It lists every registration point as present or missing, with the file and what to add.
3. Work through the missing points until the command reports the module completely registered. Migrations are registered in the manifest, as described in `AGENTS.md`.
4. Run `node tools/rust/verify.mjs`, then add the module's own integration evidence.

## Not yet covered

The registry does not yet drive, and the gate does not yet check, these per-module points. They remain hand-maintained:

- The `WorkspaceDomain` variants, their collection tables and access resolvers in `crates/adapters/src/erp/projections.rs`, and the typed operator workspace in `crates/adapters/src/erp/operator.rs`. The host mounts `/erp/<id>/workspace` for every entry of `WorkspaceDomain::ALL`, so the variant list is the only place a workspace domain is enumerated.
- The ten Operations tabs with dedicated views (catalog, sales, purchasing, invoices, accounting, support, missions, suggestions, import, voice) and their view dispatch in `crates/slice/src/operations/mod.rs`. Tabs for modules served by the generic operator workspace come from the registry.
- The assembly contracts in `crates/assembly`, which use the assembly vocabulary (`contacts`, `fieldServices`) rather than these ids, and the two CRM capabilities the owner catalogue adds by hand (`crm-contacts`, `crm-pipeline`).
- Typed form references in `crates/domain/src/operator.rs`.
- The SQL of an aggregate (`SOURCES` in `crates/adapters/src/erp/aggregates.rs`): its table, columns, states and the page read's predicates. The registry declares the public names; adding an entry without its SQL fails `the_registry_and_the_sql_table_declare_exactly_the_same_names`, and the domain test that lists the declared aggregates fails until the new one is reviewed for disclosure.
- The fixture's per-ledger provisioning and runtime grants. Its migration lists and directories come from the migration manifest.

Migrations are registered once, in `migrations/manifest.v1.json`: the server embeds from it at build time and the fixture reads it. The gate refuses a migration embedded by hand.
