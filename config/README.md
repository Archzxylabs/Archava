# Shared configuration

This directory holds maintained configuration/catalogues consumed across the repository. Each file has a documented owner and named active consumers.

| File | Owner and consumer |
| --- | --- |
| `operator-actions.v1.json` | Installed business form metadata and permission vocabularies; consumed by `archava-domain::operator`, the Leptos native forms, Owner catalogue and explicit Ops owner bootstrap. Domain transactions remain the authorization boundary. |
| `modules.v1.json` | Module registry: one descriptor per installed business module (schema, ledger, source, agent routing and readable collections). Consumed by `archava-domain::modules`, agent routing and provider projection in the host, `tools/product/catalogue.mjs`, and verified against the wiring by `tools/check-modules.mjs`. See [module registry](../docs/architecture/MODULE_REGISTRY.md). |
| `accounting-currencies.v1.json` | Fixed SIX ISO 4217 currency/minor-unit snapshot; consumed by the financial domain parser, native book setup and form display. The accounting migration independently enforces the same version. Payment-provider units need their own explicit mapping. |
| `refusal-reasons.v1.json` | Refusal reason catalogue: database guard message → stable code and English/Indonesian sentence. Consumed by `archava-domain::erp::refusals`, the adapter error classifier and the host refusal responses; `tools/test/refusal-reasons.test.mjs` checks every key against `migrations/`. See [refusal reasons](../docs/architecture/REFUSAL_REASONS.md). |
| `library-release.v1.json` | Release/assembly catalogue read by `archava-ops` and `tools/release/sbom.mjs` |
| `payment-providers.example.json` | Deployment example copied by `tools/rust/package-native.mjs`; consumed by the optional native payment registry. Keys stay in environment variables. |
| `email-providers.example.json` | Deployment example copied by `tools/rust/package-native.mjs`; consumed by the optional native email registry. The Resend key stays in an environment variable named by `apiKeyEnv`. |

The release catalogue still carries the earlier library identity contract. Its alignment with native binary/site packaging remains a separate [release task](../docs/architecture/README.md); active consumers are the reason it stays here.

Other data has explicit owners:

- [Pricing test fixtures](../crates/pricing/tests/fixtures/README.md): historical reference values used by regression tests, not current product pricing.
- [Archived template configuration](../docs/archive/history/config/README.md): former configurator input, not a current runtime setting.
- Environment-specific credentials and database URLs belong in deployment environment variables, using `deploy/*.env.example` as the documented reference. Never place filled secrets here.

Only add a file here when its current cross-component consumers and owner can be named. Keep single-crate test fixtures beside their tests and historical catalogues in the archive.

The four public Domain catalogues (forms, currencies, module registry, refusal reasons) also have generated package-local snapshots in
[crates/domain/embedded](../crates/domain/embedded/README.md). After editing their
canonical files here, run `node tools/rust/domain-assets.mjs --write`. The
structure gate and build provenance require byte equality. This explicit helper
copies only those four public JSON files; it never copies filled environment files
or provider credentials into a Cargo package.
