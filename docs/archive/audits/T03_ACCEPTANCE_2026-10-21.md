# T03 — format:check and lint gates restored

Date: 2026-10-21
Scope: root gates (`eslint.config.mjs`, `package.json`, `.prettierignore`), maintained
`tools/**` sources, and the untracked T02 scope suites

## Finding

Two root gates were red and unusable as signals:

1. **`pnpm format:check`** — Prettier reported every maintained file it had not yet
   normalised: `tools/rust/*.mjs`, `tools/rust/erp/*`, `docs/rust/*`, and the vendored
   consumer metadata under `examples/rust-consumers/crate/deps/**`.
2. **`pnpm lint`** — 49 errors plus heap exhaustion. The flat config never declared Node
   globals for JS/MJS/CJS, so `process`, `Buffer`, `URL` and friends were `no-undef` in
   every `tools/**` script; `examples/**` was being type-aware-linted against a program
   that does not exist, producing `no-unsafe-*` noise in two standalone consumers; and the
   run needed ~4.4 GB RSS, more than the default V8 old-space allows once CI runs anything
   else at the same time.

## Change

### 1. Node globals live in the config, not in 99 file headers

`eslint.config.mjs` now declares the Node runtime globals once, for the three script
flavours the repo actually ships (`.js`/`.mjs`/`.cjs`, plus `tools/**` and root
`*.config.mjs`): `process`, `Buffer`, `console`, `global`, `globalThis`, `__dirname`,
`__filename`, `require`, `module`, `exports`, the timer family, `queueMicrotask`,
`structuredClone`, `performance`, `URL`/`URLSearchParams`, `TextEncoder`/`TextDecoder`,
`AbortController`/`AbortSignal`, `Blob`, `File`, `FormData`, `Headers`, `Request`,
`Response`, `fetch`, `crypto`, `atob`/`btoa`, `Event`/`EventTarget`, `MessageChannel`/
`MessagePort`/`MessageEvent`, `BroadcastChannel`, the three stream constructors, the two
compression streams, `WebSocket` and `navigator`.

The set is written out instead of pulled from the `globals` package so it is auditable in
the diff and adds no dependency. Because a name declared both by config and by an inline
`/* global */` comment is a `no-redeclare` violation, the 99 now-redundant per-file
comments were deleted: the config is the single source of truth, and the deletion is the
only reason those files appear in the diff.

The type-aware block covers `**/*.{ts,tsx,mts,cts}` (a `.d.mts` added in this task was
otherwise parsed without type information, which is a fatal rule-load error, not a
diagnostic). The two standalone consumers — `examples/rust-consumers/erp/hospitality/**`
and `.../professional-services/**` — are installed copies of the packed library, not
workspace source: they resolve `@archava/*` only through their own `node_modules` after
`library:pack`, so a type-aware pass against the root program reads every import as
unresolved `any` and reports `no-unsafe-*` on correct code. They therefore keep **full
syntax-level lint coverage** through a scoped block (recommended JS + TS rules and the
repo's own custom rules: `no-explicit-any`, `no-non-null-assertion`,
`consistent-type-imports`, `no-console`, `eqeqeq`, `no-unused-vars`) and drop only the
type-aware half, which they get from their own `tsc --noEmit` — `module-consumers.ts` runs
typecheck + build + verify per consumer and fails the stage on any non-zero exit. Their
`.mjs`/`.js` scripts are not in that block on purpose: they fall through to the repo-wide
JS block (Node globals, no type-aware rules), the same treatment every `tools/**` script
gets, so `no-console` does not fire on verifier scripts that exist to print results. No
rule is disabled globally anywhere, and no file is skipped by the linter.

### 2. The remaining diagnostics were fixed, not silenced

- `tools/rust/erp/module-consumers.ts` — the recursive hasher's `directory` / `current`
  parameters were implicit `any`, so every `readdir`/`join`/`readFile` argument was
  `no-unsafe-argument`; typed as `string` with `Promise<string>` / `Promise<void>`
  returns. The consumer manifest is parsed with an explicit `ConsumerManifest` shape
  (mirroring `packages/config/src/release.ts`, which uses the same idiom) instead of
  `any`, and the dependency pins are built as `[string, string]` tuples so
  `Object.fromEntries` yields `Record<string, string>`.
- `tools/rust/erp/module-consumers.witnesses.d.mts` (new) — the witness sidecar stays
  plain JavaScript because `node --test` imports it without a transpiler, so its shapes
  are declared in a sibling declaration file. This is what removes the `any` at the
  import boundary; the sidecar itself is unchanged.
- `tools/rust/browser-slice.mjs` — the `pendingNotice` binding was never read; the
  `waitText` call it wrapped is an assertion, so it became a bare `await` and the
  assertion still runs.
- `tools/rust/k05-k04-consumers.mjs` — dropped `stat` from the `node:fs/promises` import,
  the dead `wired` binding, and a dead label.
- `tools/rust/legacy-replacement.mjs` — dropped the unused `execFileSync` import and the
  `BANNED_SPAWN` regex, after grepping that the constant is referenced nowhere and its
  replacement (`ALLOWED_SPAWN`/engine checks) is what the script actually uses.
- The four T02 scope suites (`packages/{billing,attendance,laundry,subscriptions}-pg/test/scope.test.ts`,
  still untracked from T02) — the `detail!` / `listQuery!` / `scoped!` non-null assertions
  were replaced with explicit narrowing guards (`if (!detail) throw new Error(...)`) and a
  no-op `db[table as keyof Db] as Row[]` cast was removed. Every `expect(...)` is
  unchanged; the suites still assert the same statements, they just no longer need `!`.

### 3. Heap is bounded by the checked-in invocation

`package.json`:

```
lint     : node --max-old-space-size=6144 ./node_modules/eslint/bin/eslint.js . --max-warnings=0
lint:fix : node --max-old-space-size=6144 ./node_modules/eslint/bin/eslint.js . --fix
```

Project loading was reduced first (the two consumer programs above); what remains is the
real program set and needs the headroom. Measured peak RSS for the green run is 4.4 GB, so
6144 MB leaves ~1.6 GB above the working set while still bounding the process. `CI` calls
`pnpm lint`, so the bound is what CI gets — no manual `NODE_OPTIONS`.

### 4. Prettier scope

`.prettierignore` gains one narrow entry, `examples/rust-consumers/crate/deps/`, with the
reason in the file: those files are unpacked from released `.crate` archives
(`.cargo_vcs_info.json` and friends) and are checksummed upstream artifacts — rewriting
them would make the vendored tree disagree with the tarball, and the next unpack would
restore the original bytes, so the gate would flap. The consumers' own sources stay
checked.

56 maintained files were then run through `prettier --write`. `docs/rust/required-coverage.json`
(a frozen manifest read by the tools, never written by them) was re-serialised only; a
JSON parse-and-compare against `HEAD` confirms the value is identical. The `docs/audit/**`
and `docs/rust/**.md` changes are table-cell padding and line wrapping — no sentence,
number, verdict or heading changed. One exception was repaired instead of accepted: Prettier's
emphasis parser read the bare `rate_limited` in `docs/rust/EXECUTION_STATE.md` (RN07 row) as an
emphasis opener and rewrote it to `rate*limited`, so that literal is now a code span
(`rate_limited`) and the neighbouring `*debug-build*` emphasis sits in Prettier's normalised
`_debug-build_` form — same rendering, same claim, no mangled identifier.

## Measured evidence

| Gate       | Command                                                      | Result                                                    | Log                          |
| ---------- | ------------------------------------------------------------ | --------------------------------------------------------- | ---------------------------- |
| format     | `pnpm format:check`                                          | exit 0, "All matched files use Prettier code style!"      | `/tmp/t03-fmt-posttest2.log` |
| lint       | `pnpm lint`, `env -u NODE_OPTIONS pnpm lint`                 | exit 0, 0 errors, 0 warnings, 51 s, peak child RSS 4.4 GB | `/tmp/t03-lint-final2.log`   |
| structure  | `pnpm structure`                                             | exit 0                                                    | `/tmp/t03-structure.log`     |
| typecheck  | `pnpm typecheck`                                             | exit 0, 48 s                                              | `/tmp/t03-typecheck.log`     |
| test       | `pnpm test`                                                  | exit 0, 145 files / 1896 tests passed                     | `/tmp/t03-test.log`          |
| build      | `pnpm build`                                                 | exit 0                                                    | `/tmp/t03-build.log`         |
| rust fmt   | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | exit 0                                                    | `/tmp/t03-rust.log`          |
| whitespace | `git diff --check`                                           | exit 0                                                    | `/tmp/t03-rust.log`          |

Baseline for comparison: `pnpm format:check` listed 54 files at the start of this run (the
plan's 36 was measured earlier in the day; sibling tasks added tests and docs since), and
`pnpm lint` reported 49 errors.

## Clarification of the heap acceptance

The plan's acceptance 2 was written as `env -u NODE_OPTIONS pnpm lint`, and it failed for a reason
that is not about unsetting the variable: the **previous** `lint` script was a bare `eslint .`, so
the heap was whatever the caller's environment supplied. With nothing supplied the run hit the
default old-space limit and aborted (V8 exit 134); the 49 diagnostics only ever appeared when a
large heap was exported by hand. That is the failure this task removed — the bound now lives in the
checked-in script, so no caller-supplied environment is needed for the gate to work.

Consequence for acceptance: `env -u NODE_OPTIONS pnpm lint` is now a **valid** verification and it
passes — the audit measured exit 0 with an effective limit of 6192 MiB, because the script's
`--max-old-space-size=6144` survives the unset. Both `pnpm lint` and `env -u NODE_OPTIONS pnpm lint`
are green; neither depends on a heap exported outside the repo. What was wrong in the plan was the
diagnosis (the default heap exhausts, so unsetting it was never the fix), not the command as a check.

## Observations outside T03 scope

- Root `tsc --noEmit -p tsconfig.json` is not part of any gate (`pnpm typecheck` runs the
  workspace packages plus `apps/web`) and currently reports four pre-existing errors in
  `tools/library/test/manifest.test.ts` and `tools/rust/benchmark/test/coverage.test.ts`.
  The `tools/rust/erp/module-consumers.ts` errors this task was chasing are gone (that
  file is clean under the same run). The orphan root project is a separate hygiene finding
  and was deliberately not bundled into T03.
- The tree is shared with concurrent tasks; two sibling files (a `tools/library/verify-*`
  script and a new `docs/audit` note) arrived unformatted mid-run and were formatted, which
  is why `format:check` was re-run at the end.

## Files changed

- `eslint.config.mjs` — Node globals, `mts`/`cts` in the typed block, scoped syntax-level consumer coverage.
- `package.json` — bounded heap in `lint` and `lint:fix`.
- `.prettierignore` — narrow vendored-crate exclusion with rationale.
- `tools/rust/erp/module-consumers.ts`, `tools/rust/erp/module-consumers.witnesses.d.mts` (new) — typed hasher + manifest + witness shapes.
- `tools/rust/browser-slice.mjs`, `tools/rust/k05-k04-consumers.mjs`, `tools/rust/legacy-replacement.mjs` — dead binding/import/label removal.
- `packages/{billing,attendance,laundry,subscriptions}-pg/test/scope.test.ts` — narrowing guards replacing `!`.
- 99 script files — redundant inline `/* global */` headers removed (see §1).
- 56 files — Prettier formatting.
