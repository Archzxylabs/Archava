# Packaged domain portability finding — 8 November 2026

A real isolated consumer could not compile the packaged domain crate at source
`f88831b033be413d9adb80c1f6b33bff1a17e63b812eaf0e4b2ab4f3301bb9dd`.
This is open counterevidence for CFG-03 and RUST-G05, separate from bounded native
application or financial acceptance.

`cargo package` successfully produced core/domain archives offline. The probe
extracted only those archives into a fresh directory outside the checkout and
built a minimal Rust tenant/CRM consumer with an isolated release target. It
copied no private environment or workspace sources. Compilation failed because
the packaged domain's `include_str!` paths still reached outside its crate for
`config/operator-actions.v1.json` and `config/accounting-currencies.v1.json`.

Inputs, archive digests, consumer sources and exit 101 are retained under
`target/prd-completion-20261107/` in `cold-domain-probe-01.json`,
`cold-domain-probe-01.log`, `cold-probe-Cargo.toml`, `cold-probe-main.rs` and the
finite reproduction script `cold-domain-probe.py`. Its owned temporary build
directory was removed after completion. No service or package publication ran.

The fix must make the two public embedded catalogues available inside the
distributable crate, preserve the shared canonical configuration and enforce
byte consistency in verification/provenance. Re-run the cold build from fresh
artifacts after the fix. This small probe alone does not accept the complete
independent Agency/Wholesale CRM consumers or any full industry kit.
