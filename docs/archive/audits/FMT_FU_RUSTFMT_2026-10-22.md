# FMT-FU — rustfmt pass on the Rust subscriptions surface (2026-10-22)

Card: `t_077c2d73` (parent `t_23799344`, releases the gate `t_be45b375`).
Scope: **layout only**. No logic, no behaviour, no new test, no reverted fix.

## 1. Baseline (before touching anything)

`cargo fmt --all --check` must be invoked from `rust/` — the repo root has no
`Cargo.toml`, and the bare root invocation exits 1 with
`` `cargo metadata` exited with an error: could not find `Cargo.toml` `` — the same
exit code as a formatting failure, which is exactly the false-signal trap documented
in `tools/rust/verify.mjs:201`.

```
cd rust && cargo fmt --all --check        -> exit 1
13 "Diff in" hunks, 3 files:
  rust/crates/adapters/src/erp/subscriptions.rs   1 hunk  (chained-call re-join)
  rust/crates/host/src/erp/subscriptions.rs       1 hunk  (import brace re-wrap)
  rust/crates/host/tests/erp_subscriptions.rs    11 hunks (multi-line arg wraps)
```

Every other Rust file in the tree (spike: `erp/attendance.rs`, `erp/billing.rs`,
`host/src/router.rs`, `tests/erp_{attendance,billing}.rs`) is **already rustfmt-clean
despite being dirty vs HEAD** — so a blanket `cargo fmt --all` would have served no
purpose and only risked foreign edits.

## 2. The change

```
rustfmt --edition 2021 \
  rust/crates/adapters/src/erp/subscriptions.rs \
  rust/crates/host/src/erp/subscriptions.rs \
  rust/crates/host/tests/erp_subscriptions.rs        -> exit 0
```

Exactly three files. Incremental diff (`pre -> post` snapshots, `git diff --no-index
--stat`): **57 insertions, 18 deletions**, all of the shapes rustfmt owns — call
chains re-joined, one `use { … }` brace group re-wrapped, 11 argument lists / macro
calls re-wrapped with trailing commas.

| file                                            | bytes         | sha256 after                                                       |
| ----------------------------------------------- | ------------- | ------------------------------------------------------------------ |
| `rust/crates/adapters/src/erp/subscriptions.rs` | 55911 → 55884 | `fc9084a6e5abc796564162995b7b32b149ebf709cf69a563367ced0d3331317c` |
| `rust/crates/host/src/erp/subscriptions.rs`     | 31761 → 31757 | `ab7a11640430b662493733a5b7a76eb5d916210ad920481c819c2ea6f9337318` |
| `rust/crates/host/tests/erp_subscriptions.rs`   | 62131 → 62529 | `fbaa150dad11f12d568f4cd074931bdecc181811b55f343dd37a59f9ea803e4d` |

## 3. No-semantics proof

`git diff -w` is **not usable as the proof here**, for two independent reasons:

1. `-w` is line-based. A line join (`read\n  .fetch_all(…)` → `read.fetch_all(…)`)
   or an import-brace re-wrap changes line _contents_, so `-w` still reports a
   difference even though the token stream is untouched.
2. The three files already carry the parent card's uncommitted logic fixes
   (`git status` shows ` M`, HEAD is `412f763`), so any `git diff -w` **against HEAD**
   necessarily contains that logic diff and could never be empty.

The proof used instead is token-level, computed on pre/post snapshots:

```
file                                          ws_stripped_equal  token_equal  residual_token_deltas
adapters/src/erp/subscriptions.rs             True               True         []
host/src/erp/subscriptions.rs                 True               True         []
host/tests/erp_subscriptions.rs               False              True         [5 x ',']
```

- For the two `src` files, deleting all whitespace makes the pre and post text
  **identical** — not a single character of code moved.
- For the test file, the only residual difference after whitespace removal is
  **five inserted `,`** — the trailing comma rustfmt appends inside a multi-line
  argument list, which is semantically inert in Rust (it is the same argument list).
  Normalising exactly those (`,(?=[)\]}]`)`) makes the two texts identical.

Independent corroboration that behaviour is unchanged: the suite that exercises this
surface is 17/17 green **on the formatted bytes** (§5), and clippy compiles them with
`-D warnings` (§4).

## 4. Gates on the formatted tree

| command                                                                             | cwd     | exit  | note                                                                        |
| ----------------------------------------------------------------------------------- | ------- | ----- | --------------------------------------------------------------------------- |
| `cargo fmt --all --check`                                                           | `rust/` | **0** | zero bytes of output (was: exit 1, 13 hunks)                                |
| `cargo clippy --all-targets --locked --offline -- -D warnings`                      | `rust/` | **0** | the exact gate form of `tools/rust/verify.mjs:221-225`, `Finished in 6.40s` |
| `cargo test -p archava-host --test erp_subscriptions --offline -- --test-threads=1` | `rust/` | **0** | `test result: ok. 17 passed; 0 failed; 0 ignored` in 856.20s                |

Logs: `/tmp/fmt_baseline.txt`, `/tmp/fmt_final.txt`, `/tmp/clippy.txt`,
`/tmp/erp_subs_test2.txt`, `/tmp/token_proof.txt`.

### Pitfall found while running the suite (cost: one 8-second all-red run)

The first attempt returned `0 passed; 17 failed` in 8.63s. Not the code — the
`archava-pg-fixture` crate puts the cluster's Unix socket under
`std::env::temp_dir()` (`crates/pg-fixture/src/lib.rs:413`), and this worker's
`TMPDIR` (`/home/haikaru/.hermes/profiles/archlab/cache/scratch`) produces a
**113-byte** socket path against PostgreSQL's 107-byte `sun_path` limit:

```
FATAL: could not create any Unix-domain sockets
```

Re-run with a short `TMPDIR=/home/haikaru/.ptmp` (80-byte path) → 17/17. Any future
cargo run of these suites from a kanban/scratch session must export a short `TMPDIR`
(`TMPDIR=/tmp` also works) or it will look like a total test failure.

## 5. Untouched surface

`git status --porcelain -- rust/` is the **same 10 entries** before and after this
card; no file was added, removed, or reverted. The five non-subscriptions entries
(`erp/attendance.rs`, `erp/billing.rs` ×2, `router.rs`, `tests/erp_{attendance,billing}.rs`)
keep mtimes from 19:38–23:17, i.e. before this run — they belong to sibling card
`t_9db93882` and were neither formatted nor reverted here.

Final `cargo fmt --all --check` was re-run **after** clippy and the suite: exit 0,
and the three sha256 values are unchanged from the post-rustfmt snapshot, so the tree
the gate `t_be45b375` is about to certify is byte-identical to the tree tested here.

## 6. Collateral: the JS format gate was red for doc-only reasons

`pnpm format:check` (a gate in the audit card's list, last green under `t_555a90c3`)
was **RED on arrival at this card**, for two untracked markdown files written after
that card closed — `docs/audit/AUDIT_GATE_2026-10-22.md` and
`docs/audit/T02_FU_ENTITLEMENTS_RUST_2026-10-21.md`. Neither is a Rust file and
neither was touched by this card's formatting work, but leaving them would have
handed the audit a red gate it cannot attribute.

Both were normalised with `prettier --write`. Accepted only after a character-level
audit of every delta against pre-write snapshots: the deltas are **table separator
padding** (`|---|` widened to column width) and six `*emphasis*` -> `_emphasis_`
marker normalisations (`*subscription*`, `*is*`, `*login*`), i.e. rendered text is
identical and no literal was mangled — the failure mode `t_555a90c3` hit with a bare
identifier. No prose token, code literal, or table cell value changed.

Result: `pnpm format:check` -> **exit 0**, `All matched files use Prettier code style!`
