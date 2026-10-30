# Examples

The current runnable application preview is [crates/slice/examples/slice_server.rs](../crates/slice/examples/slice_server.rs), launched through `node tools/rust/dev-slice.mjs run`.

Earlier TypeScript consumers, packaged Rust consumer templates, addon consumers, and presence demos are removed from the tree and recoverable from git history. None was a root install target or a current deployment recipe.

For a current standalone Rust consumer of pricing/rates, see [the benchmark crate](../tools/rust/benchmark/rust/Cargo.toml). It is kept outside the product workspace and uses explicit path dependencies on those two crates.
