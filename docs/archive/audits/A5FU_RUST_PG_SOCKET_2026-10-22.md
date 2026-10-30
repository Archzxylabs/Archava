# A5-FU (Rust) — the fixture's Unix socket no longer depends on `temp_dir()`

Scope: the Rust residual of the A5 finding, which the A5 card (`t_526bcda7`) could
not touch because A5 forbade `rust/**`. Card: **`t_90dc34a7`**. Claim format is
`claim → file:line` plus the command output that measured it; line numbers are
post-change. Tree is dirty by design (the audit's standing rule: no checkout, no
commit), so the pre-image in `git diff` is `HEAD`, not the tree the audit read.

Related: `docs/audit/ARCHLAB_AUDIT_2026-10-21.md` §A5/§1.3 (the `0 passed / 17
failed` symptom) and `tools/library/pg-fixture.mjs` (the TS half of the same
finding, fixed in `t_526bcda7`).

## 1. The defect

`rust/crates/pg-fixture/src/lib.rs:413` (pre-change) put every instance under
`std::env::temp_dir()`:

```rust
let dir = std::env::temp_dir().join(format!("{DIR_PREFIX}{}-{nanos}", std::process::id()));
```

The cluster then bound its Unix socket **inside** that directory (`-k
<instance_dir>`, pre-change `:729`), whose own comment admitted the limit but
assumed the base was short — _"socket in the instance directory (short path —
unix socket paths cap near 108 bytes)"_. Nothing checked that assumption.

`sockaddr_un.sun_path` is 108 bytes **including** the terminating NUL, so 107
bytes is the largest bindable path. The socket PostgreSQL creates is
`<instance_dir>/.s.PGSQL.<port>` (14 bytes for a 16-bit port), and the instance
name is `archava-pg-fixture-<pid>-<nanos>` (~46 bytes). A `TMPDIR` of 48 bytes or
more therefore overflows:

| `TMPDIR`                              | socket path             | binds? |
| ------------------------------------- | ----------------------- | ------ |
| `/tmp` (4)                            | 4 + 1 + 46 + 14 = 65    | yes    |
| kanban scratch `…/cache/scratch` (52) | 52 + 1 + 46 + 14 = 113  | **no** |
| the audit's long `TMPDIR` (105)       | 105 + 1 + 46 + 14 = 166 | **no** |

The failure mode was not "a long path" but _where it surfaced_: `initdb`
succeeded, `pg_ctl start` failed with `could not start server`, and every
consumer suite reported `0 passed / N failed` in seconds without naming the
socket. The kanban worker's own `TMPDIR` (`…/profiles/archlab/cache/scratch`, 52
bytes) was already on the wrong side of the line — the trap that
`rust_test_suite_environment_traps` records as "always run with `TMPDIR=/tmp`"
is this bug, not a law of nature.

## 2. The fix — one mechanism, chosen before `initdb`

One mechanism per cluster, as the card requires: the instance still binds a Unix
socket inside its own directory (`-k`); only the **directory** is chosen so that
socket fits. TCP-only was the other allowed option and was rejected because it
would change the fixture's connection story, not just its path handling.

- `lib.rs:411` — `fn instance_dir(port: u16) -> PathBuf`. The port is now an
  _input_: the socket lives at `<dir>/.s.PGSQL.<port>`, so the length check must
  run on the exact path that will be bound, not an approximation.
- `lib.rs:886-887` — the port is therefore drawn before the directory
  (`let port = free_port(); let dir = instance_dir(port);`).
- `lib.rs:550` — `fn choose_socket_base(temp_dir, forced, instance_name, port)
-> Result<PathBuf, SocketPathTooLong>`: `temp_dir()` wins when its socket path
  fits (a caller who set `TMPDIR` meant it), otherwise `/tmp`; an explicit
  override wins outright and is **never** silently substituted.
- `lib.rs:439` — `pub const SUN_PATH_MAX: usize = 107`, named and public because
  it is the fixture's contract, not an internal detail.
- `lib.rs:465` — `pub fn socket_path(socket_dir, port)`, so a test can assert on
  the same path the server was started with — "the check passed" and "the server
  bound it" cannot drift apart.
- `ARCHAVA_PG_SOCKET_DIR` — the override knob, under the name the TS helper
  already uses, so both languages of the fixture share one knob.

`instance_dir` panics with `SocketPathTooLong` **before `initdb`**, so the
diagnosis replaces the misleading start failure instead of trailing it:

```
pg-fixture: the PostgreSQL unix socket path does not fit in sockaddr_un.sun_path (107 bytes max)
  socket path : /tmp/ffffff…/archava-pg-fixture-1129913-1792623636279338239/.s.PGSQL.37583 (167 bytes)
  socket dir  : /tmp/ffffff…/archava-pg-fixture-1129913-1792623636279338239
  temp_dir()  : /home/haikaru/.hermes/profiles/archlab/cache/scratch (52 bytes)
  forced by ARCHAVA_PG_SOCKET_DIR: /tmp/ffffff…
fix: set ARCHAVA_PG_SOCKET_DIR to a short directory (e.g. ARCHAVA_PG_SOCKET_DIR=/tmp) or run with TMPDIR=/tmp — the fixture already falls back to /tmp on its own, so reaching this message means the path was forced longer than the fallback can hold
```

A forced override that does not fit is the one case that can still reach that
message: it is deliberate, because the override exists so a diagnosis can be
reproduced exactly. The repair path is the _default_ one, not the override.

## 3. Reaper contract preserved

The name format `<DIR_PREFIX><pid>-<nanos>` is untouched —
`creator_pid_from_name` parses the same input, and
`the_instance_name_keeps_the_creator_pid_contract` asserts it against the real
format. What changed is the **set of bases swept**:

- `lib.rs:701` — `fn reap_bases_from(temp_dir, forced) -> Vec<PathBuf>` returns
  the forced base (if any), `temp_dir()`, and `/tmp`, deduplicated in preference
  order, so `TMPDIR=/tmp` does not scan the same directory twice.
- `lib.rs:683` — `clean_stale_dirs()` loops over `reap_bases()`.
- `lib.rs:733` — `fn clean_stale_dir(base)` is the per-base sweep; the verdict
  still comes from `reap_verdict` on real inputs, unchanged.

Sweeping only `temp_dir()` would orphan every instance created under the
fallback — the opposite of what a reaper is for — so the reaper follows the new
base decision instead of assuming it.

## 4. Tests

Unit (`src/lib.rs`, 16 total, 6 new — the decision, without mutating the
process-global environment):

- `a_long_temp_dir_falls_back_to_a_bindable_base` — 105-byte `temp_dir()` →
  base is `/tmp` and the socket fits.
- `a_short_temp_dir_is_still_preferred` — the fallback is conditional, not a new
  default.
- `the_limit_is_checked_against_the_real_socket_name` — boundary: exactly 107
  bytes accepted, 108 refused, on the real `.s.PGSQL.<port>` name.
- `the_limit_counts_bytes_not_characters` — a 50-character, 109-byte path is
  refused: `sun_path` is a byte array.
- `a_forced_long_socket_dir_is_reported_not_repaired` — the message carries the
  socket path, its length, the 107 limit, `temp_dir()`, and the knob.
- `the_reaper_sweeps_every_base_the_fixture_can_create_in` — all bases swept,
  deduplicated.

Integration (`tests/socket_dir.rs`, `tests/socket_dir_forced.rs` — separate
binaries, one cluster each, because the override is process-global):

- `a_cluster_binds_its_socket_whatever_tmpdir_is` **provisions a real cluster**
  and asserts the socket exists, is a socket, fits in 107 bytes, and that the
  base is `/tmp` exactly when `temp_dir()` could not have held it.
- `a_forced_long_socket_dir_names_the_sun_path_limit` — `should_panic(expected =
"sockaddr_un.sun_path")`, i.e. the fail-fast fires before `initdb`.

## 5. Measured

```
$ TMPDIR=/tmp/a5fu-tmpdir/TT…(106 bytes) cargo test -p archava-pg-fixture --offline --locked -- --test-threads=1
running 16 tests … test result: ok. 16 passed; 0 failed
     Running tests/socket_dir.rs
test a_cluster_binds_its_socket_whatever_tmpdir_is ... ok
     Running tests/socket_dir_forced.rs
test a_forced_long_socket_dir_names_the_sun_path_limit - should panic ... ok
EXIT=0

$ TMPDIR=<same 106 bytes> cargo test -p archava-host --test readiness --offline --locked -- --test-threads=1
running 5 tests … test result: ok. 5 passed; 0 failed; finished in 5.16s
EXIT=0

$ cargo fmt --all --check          # from rust/
FMT_EXIT=0

$ cargo test -p archava-pg-fixture --offline --locked -- --test-threads=1
# no TMPDIR override: this worker's TMPDIR is the 52-byte scratch dir, i.e. the
# 113-byte socket path that failed before this change
running 16 tests … test result: ok. 16 passed; 0 failed
test a_cluster_binds_its_socket_whatever_tmpdir_is ... ok
test a_forced_long_socket_dir_names_the_sun_path_limit - should panic ... ok
EXIT=0

$ ls -d /tmp/archava-pg-fixture-*  # after the runs
none
```

The `archava-host` run is the consumer proof: `readiness.rs` goes through
`provision` → pool → router, which is the path that reported `0 passed / N
failed` under a long `TMPDIR`. No fixture directory was left behind in either
base.

## 6. What this card does not claim

- The ERP host suites (`erp_billing` / `erp_subscriptions` / `erp_attendance`,
  ~44 min serial) were **not** re-run: another worker's run of exactly those
  held the cargo lock for this card's whole editing phase, and the fixture is
  shared, so `readiness` + the fixture's own suite are the evidence for the
  fixture change. Their green/red status is the gate's business, not this
  document's.
- Diff is limited to `rust/crates/pg-fixture/**` (plus this note). No `tools/**`,
  no `packages/**`, no `apps/**`.
