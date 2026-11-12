# Packaged contract consumers — 8 November 2026

Bounded **local portable-contract proof**. This is not OWN-V1, full K04/K05 business
acceptance, configurable labels in the product UI, or whole-product completion.

Serving-input snapshot: `fcd94e70cfd4388b9ca7fcd41798213d4bfae1c424b822c6eb9de74bd4556f28` (497 inputs).
The [earlier cold-package finding](PRD_COLD_PACKAGE_FINDING_2026-11-08.md) remains
an immutable failed f888 run. The fix moves the two public catalogue embeds into
the Domain package and requires byte equality with canonical config in structure
verification and build provenance. No runtime secret/config file is embedded.

## Actual execution

Command: `node tools/rust/cold-consumers.mjs --output target/prd-completion-20261107/cold-consumers-02`.
Actual exit **0**. Rust 1.96.0, Node 22.22.3, Cargo offline. Current core/domain
are packaged once. Each consumer extracts those identical archives into a separate
temporary root outside the repository, with its own build directory and generated
locked minimal manifest. Cached third-party registry dependencies are used; no
workspace/absolute source import, private env copy, product database, provider call,
service, registry publication, or public deployment is involved.

Each consumer directly declares only core, domain and the JSON wire library.
The normal dependency trees exclude host/adapters/slice, Axum, SQLx, Leptos,
Reqwest and Tokio. Actual archive hashes:

- `archava-core`: `91e849f5e33453556e1d73c1c6b18e6ad2a1b2fd6036764468e8c3236fc5769b`.
- `archava-domain`: `b1a2b7761f875be67b0c0b7e2e424220faebd0ec949e6ecc54ce3b2fb4003720`.

Both applications run successfully. Each has **5 tests passed, 0 failed, 0
ignored**: shared Unicode contact grammar and rejected forged fields; tri-state
contact patches; canonical pipeline transitions despite different local labels;
tenant/missing-row refusals; and its own business extension. Agency/K05 adds
project/task lifecycle rules. Wholesale/K04 adds validated order quantities and
rejects forged approval fields. These calls use shared packaged rules directly;
no Archava core source is forked.

The owned temporary roots are removed on completion. Persistent evidence remains
under `target/prd-completion-20261107/cold-consumers-02/`: archive bytes, source and
tool identities, both exact manifests/lockfiles, fixture snapshots, dependency
trees, command logs and `result.json`. Evidence result SHA-256:
`f37971bba9493b661be3fc754bfe28d2d3142b3dc7b5ac83156431c70733412f`.

## Limits and remaining work

This closes the actual package-external catalogue compile defect for these two
contract consumers. It does not prove reusable persistent application assembly,
installed/configured module compatibility, both full businesses, their upgrades,
or owner UI/chat configuration. The [consumer scope](../../tools/consumers/README.md)
and [Owner semantic review](PRD_OWNER_TRACEABILITY_REVIEW_2026-11-08.md) preserve
those distinctions. CFG-03 and RUST-G05 remain partial. Later serving source
requires its own native build/gate/browser/profile evidence; these library tests
do not accept the newer runtime automatically.

Run 01 also passed ten tests, but did not retain the generated lockfile bytes.
Run 02 corrects that evidence retention and is the recorded repeatable proof;
the first run is preserved without rewriting it.
