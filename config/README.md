# Shared configuration

This directory holds maintained configuration/catalogues consumed across the repository. Each file has a documented owner and named active consumers.

| File | Owner and consumer |
| --- | --- |
| `library-release.v1.json` | Release/assembly catalogue read by `archava-ops` and `tools/release/sbom.mjs` |
| `payment-providers.example.json` | Deployment example copied by `tools/rust/package-native.mjs`; consumed by the optional native payment registry. Keys stay in environment variables. |
| `email-providers.example.json` | Deployment example copied by `tools/rust/package-native.mjs`; consumed by the optional native email registry. The Resend key stays in an environment variable named by `apiKeyEnv`. |

The release catalogue still carries the earlier library identity contract. Its alignment with native binary/site packaging remains a separate [release task](../docs/architecture/README.md); active consumers are the reason it stays here.

Other data has explicit owners:

- [Pricing test fixtures](../crates/pricing/tests/fixtures/README.md): historical reference values used by regression tests, not current product pricing.
- [Archived template configuration](../docs/archive/history/config/README.md): former configurator input, not a current runtime setting.
- Environment-specific credentials and database URLs belong in deployment environment variables, using `deploy/*.env.example` as the documented reference. Never place filled secrets here.

Only add a file here when its current cross-component consumers and owner can be named. Keep single-crate test fixtures beside their tests and historical catalogues in the archive.
