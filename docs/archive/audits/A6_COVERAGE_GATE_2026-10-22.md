# A6 — coverage gate: per-package thresholds + ratchet from a measured baseline

- **Date:** 2026-10-22
- **Repo:** `/home/haikaru/Archverse/Lab/Archava_Platform` (`feat/laundry-1.17.0`, dirty tree preserved, nothing committed)
- **Finding:** `docs/audit/ARCHLAB_AUDIT_2026-10-21.md:218-224` (A6, P3)
- **Scope:** test/coverage configuration only — `vitest.config.ts`, `.prettierignore`, `docs/audit/evidence/a6/**`. No `packages/*/src/**` and no `rust/**` change; no test was added to move a number.

> A6 as written: _"30% line coverage bukan angka yang bisa dipertahankan untuk platform
> yang menjual auditabilitas. Tidak ada `thresholds`, jadi tidak ada gate. Minimal:
> threshold per package untuk `packages/_/src` dengan ratchet, atau tandai suite
> host/proof sebagai sumber coverage utama agar angkanya tidak menyesatkan."\*

**Verdict: closed.** Coverage is now gated three ways — one global floor, one floor per
package/app scope (66 globs), and a documented ratchet rule. The gate was proven live: with a
threshold raised above the measurement the run exits **1** and names the violation; restored to
the measurement it exits **0** and reproduces the baseline to the decimal.

---

## 1. Measured baseline (the only source of the numbers)

```
$ TMPDIR=/tmp pnpm exec vitest run --coverage
 Test Files  153 passed (153)
      Tests  1931 passed (1931)

=============================== Coverage summary ===============================
Statements   : 32.38% ( 7629/23555 )
Branches     : 33.91% ( 5001/14745 )
Functions    : 41.21% ( 1497/3632 )
Lines        : 33.2% ( 7159/21558 )
================================================================================
EXIT=0
```

- Full log: `docs/audit/evidence/a6/gate-run-at-baseline.log`
- Raw map from that run: `docs/audit/evidence/a6/coverage-final-baseline.json` (istanbul format, 286 files)

Every threshold in `vitest.config.ts` is one of the percentages below. They were not typed from
this printout by hand: `docs/audit/evidence/a6/emit-thresholds.mjs` reads the raw map and emits the
table using the same istanbul summary the gate compares against, so a row can be re-derived after
any run instead of being guessed.

### Per scope (istanbul `percent()`: floor, two decimals — the gate's own arithmetic)

| scope                          | files | lines % | functions % | branches % | statements % |
| ------------------------------ | ----: | ------: | ----------: | ---------: | -----------: |
| `apps/crm`                     |    14 |    7.18 |        9.28 |       6.47 |         6.82 |
| `apps/web`                     |    12 |   10.52 |       15.42 |      14.96 |        10.16 |
| `packages/accommodation`       |     5 |   24.72 |       21.33 |      25.49 |        23.28 |
| `packages/accommodation-pg`    |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/acl`                 |     4 |   92.59 |       83.33 |      86.66 |        90.96 |
| `packages/act`                 |     3 |   89.71 |      100.00 |      85.88 |        89.18 |
| `packages/act-booking`         |     3 |   89.30 |      100.00 |      84.41 |        86.36 |
| `packages/act-confirmation`    |     3 |   95.58 |      100.00 |      90.37 |        91.37 |
| `packages/act-confirmation-pg` |     3 |   95.72 |      100.00 |      93.10 |        95.65 |
| `packages/act-email`           |     3 |   96.64 |      100.00 |      91.48 |        94.37 |
| `packages/act-host`            |     6 |   90.06 |       91.80 |      83.67 |        88.25 |
| `packages/act-host-pg`         |     2 |   66.27 |       63.63 |      69.23 |        65.62 |
| `packages/act-postmark`        |     4 |   99.29 |      100.00 |      91.83 |        97.43 |
| `packages/act-storage`         |     7 |   88.40 |       92.85 |      79.83 |        86.22 |
| `packages/adapters`            |     6 |   93.33 |       94.02 |      89.80 |        92.00 |
| `packages/assets`              |     5 |    9.81 |       15.00 |       2.42 |         9.53 |
| `packages/assets-pg`           |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/assistant`           |     7 |   96.98 |       95.00 |      89.02 |        95.16 |
| `packages/attendance`          |     5 |    7.05 |       13.15 |       1.18 |         6.80 |
| `packages/attendance-pg`       |     2 |   46.73 |       82.35 |      36.56 |        44.44 |
| `packages/billing`             |     6 |   15.01 |       28.07 |      11.61 |        14.54 |
| `packages/billing-pg`          |     2 |   79.00 |       92.15 |      65.16 |        76.67 |
| `packages/capacity`            |     5 |   10.31 |        9.75 |       3.33 |         9.82 |
| `packages/capacity-pg`         |     2 |   21.94 |       41.17 |      13.69 |        20.33 |
| `packages/chat`                |     4 |   82.76 |       89.77 |      76.39 |        82.73 |
| `packages/commerce`            |     5 |   24.62 |       23.94 |      30.53 |        23.87 |
| `packages/commerce-pg`         |     3 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/config`              |    14 |   92.36 |       94.87 |      80.99 |        92.30 |
| `packages/core`                |     2 |   98.24 |      100.00 |      95.23 |        98.38 |
| `packages/crm`                 |    11 |   25.87 |       29.20 |      36.63 |        25.00 |
| `packages/crm-act`             |     3 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/crm-assistant`       |     8 |   32.20 |       49.12 |      33.66 |        31.55 |
| `packages/crm-pg`              |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/decision`            |     7 |   77.98 |       80.88 |      67.82 |        72.83 |
| `packages/field-services`      |     5 |   11.46 |       17.50 |       8.82 |        11.20 |
| `packages/field-services-pg`   |     2 |   74.31 |       69.44 |      68.10 |        71.67 |
| `packages/foodservice`         |     5 |   21.99 |       21.95 |      23.84 |        20.97 |
| `packages/foodservice-pg`      |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/formal-education`    |     5 |   12.30 |       17.50 |       4.66 |        12.25 |
| `packages/formal-education-pg` |     2 |   81.85 |       81.57 |      68.18 |        78.17 |
| `packages/fulfillment`         |     5 |    7.60 |       12.50 |       1.09 |         7.35 |
| `packages/fulfillment-pg`      |     2 |   69.26 |       40.90 |      75.29 |        66.26 |
| `packages/identity-pg`         |     3 |   16.99 |       25.92 |      19.44 |        16.86 |
| `packages/kits`                |     2 |  100.00 |      100.00 |     100.00 |       100.00 |
| `packages/knowledge`           |     5 |   91.86 |       89.79 |      77.22 |        90.10 |
| `packages/laundry`             |     5 |   10.11 |       15.38 |       3.48 |         9.78 |
| `packages/laundry-pg`          |     2 |   85.71 |       91.30 |      79.08 |        83.93 |
| `packages/organizations`       |     5 |   10.90 |       11.90 |       1.82 |        10.42 |
| `packages/organizations-pg`    |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/presence`            |     2 |   45.00 |        0.00 |       0.00 |        42.85 |
| `packages/pricing`             |     3 |   91.37 |       90.47 |      75.67 |        88.18 |
| `packages/projects`            |     5 |   27.96 |       26.53 |      28.90 |        26.85 |
| `packages/projects-pg`         |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/proposals`           |     5 |   10.59 |        9.30 |       2.95 |        10.11 |
| `packages/proposals-pg`        |     2 |   19.74 |       38.46 |      10.27 |        17.59 |
| `packages/provider-runtime`    |     1 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/purchasing`          |     5 |   23.21 |       23.37 |      22.99 |        21.75 |
| `packages/purchasing-pg`       |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/reference`           |     6 |   94.84 |       85.39 |      78.75 |        88.67 |
| `packages/scheduling`          |     6 |   36.95 |       41.17 |      35.78 |        36.09 |
| `packages/scheduling-pg`       |     3 |    0.00 |        0.00 |       0.00 |         0.00 |
| `packages/sdk`                 |     6 |   98.24 |       96.38 |      91.84 |        97.24 |
| `packages/subscriptions`       |     5 |    8.88 |       14.28 |       2.38 |         8.45 |
| `packages/subscriptions-pg`    |     2 |   51.37 |       85.29 |      39.50 |        49.37 |
| `packages/support`             |     5 |   25.25 |       27.58 |      25.30 |        23.97 |
| `packages/support-pg`          |     2 |    0.00 |        0.00 |       0.00 |         0.00 |
| **TOTAL**                      |   286 |   33.20 |       41.21 |      33.91 |        32.38 |

Regenerate: `node docs/audit/evidence/a6/per-package-coverage.mjs docs/audit/evidence/a6/coverage-final-baseline.json --md`
(same file also writes the plain-text variant used below: `docs/audit/evidence/a6/per-package-coverage.txt`).

### What the headline number does and does not describe

The unit suite is the _only_ source of this report. Two other suites run on this platform and
contribute nothing to it: the built-artifact proofs in `vitest.artifacts.config.ts` (no coverage
instrumentation) and the `tools/library/verify-*.mjs` harnesses, which drive the `-pg` adapters
and the CRM host against a real PostgreSQL. The Rust suites (`cargo test`, 49 tests across the
detached trio) are a third lane entirely. So `33.20%` is "the vitest unit suite covers a third of
the instrumented TS surface", not "the platform is a third tested".

Two further distinctions, both from `docs/audit/evidence/a6/tested-surface.txt`:

| scope                                     | files | lines % | functions % | branches % | statements % |
| ----------------------------------------- | ----: | ------: | ----------: | ---------: | -----------: |
| all instrumented (what the gate enforces) |   286 |   33.20 |       41.21 |      33.91 |        32.38 |
| zero-coverage packages (12 scopes)        |    26 |    0.00 |        0.00 |       0.00 |         0.00 |
| scopes with any coverage (54)             |   260 |   38.50 |       48.55 |      40.59 |        37.67 |

`2964` of the `21558` instrumented lines (`13.75%`) sit in twelve packages that have no unit test
at all. They stay in `coverage.include` — dropping them would push the headline to 38.50% while a
seventh of the source silently stops being counted, which is precisely the misleading number A6
flags. They are instead pinned explicitly at `0` in the config (`ZERO_COVERAGE_PACKAGES`) so the
untested surface stays visible in every report.

### Why this run measures 33.20% and the audit recorded 31.05%

`docs/audit/2026-10-21-archlab-audit.json` → `/gates/unit_coverage` records
`{statements_pct: 30.32, branches_pct: 32.29, functions_pct: 37.37, lines_pct: 31.05, thresholds_configured: false}`.
(The card body labels those as line/function/branch; the JSON keys are the authoritative labels —
statements 30.32, branches 32.29, functions 37.37, lines 31.05. Same run, and the gate now covers
all four.)

Today's tree measures higher on every metric, and the difference is the tree, not the arithmetic:
relative to `HEAD`, this worktree carries **7 new untracked test files** and **+895 lines across 7
modified test files** (`git diff --stat -- '*.test.ts' '*.test.tsx'`), i.e. the remediation work
that landed between the audit and this card. Re-measuring the 2026-10-21 tree would mean reverting
the dirty worktree, which this card explicitly forbids, so the gate is built on today's measurement
and no claim is made about what the old tree would score today.

The consequence worth stating plainly: the audit's 30% and today's 33% are both "a third", and the
gate is set to hold today's third, not to celebrate it. Raising it is the A6-adjacent work of
writing tests, not of editing this config.

---

## 2. The gate

`vitest.config.ts` (single config; `vitest.artifacts.config.ts` has no coverage block and is
untouched):

```ts
coverage: {
  provider: 'v8',
  include: ['packages/*/src/**/*.ts', 'apps/*/src/**/*.ts'],
  exclude: ['**/*.d.ts', '**/index.ts'],
  thresholds: {
    ...GLOBAL_COVERAGE_THRESHOLDS,     // lines 33.2, functions 41.21, branches 33.91, statements 32.38
    ...PACKAGE_COVERAGE_THRESHOLDS,    // 66 globs: 54 measured + 12 pinned at 0
  },
}
```

Design decisions, all of them load-bearing:

1. **Global floors are the measured aggregate, not a rounded-down comfort number.** Vitest applies
   the global block to _every_ instrumented file (including files matched by a glob below), so this
   is the floor the whole report must clear.
2. **Per-package floors are glob keys**, `'<scope>/src/**/*.ts' → { lines, functions, branches, statements }`.
   Vitest matches them against paths relative to the config root, so the key is the package, and the
   comparison is against that package's aggregate.
3. **`perFile` stays `false`.** A per-file gate would fail on every small module that exists to be
   wired together — it teaches people to sprinkle `/* v8 ignore */` instead of writing tests, which
   is a coverage number that goes up while the code gets worse. Per-package is the granularity that
   survives contact with this tree.
4. **Negative thresholds were considered and rejected** for the zero-coverage packages. They would
   lock the _size_ of the untested surface rather than its coverage percentage; the global floor
   already fails when untested code grows, so the extra mechanism would only add a confusing number
   to read.
5. **One source of truth for the numbers.** The `MEASURED_COVERAGE` table in the config is emitted
   by `docs/audit/evidence/a6/emit-thresholds.mjs` from the raw map, not transcribed.

### Ratchet rule (written in the config, enforced by review)

- Thresholds move **up**, never down. Coverage improves → re-run the suite → raise the row to the
  new measured value, in the same change that adds the tests.
- Coverage drops → the gate fails → the fix is to restore (or delete) the tests, not to edit the
  number. Lowering a row is a coverage regression recorded as a config diff, and it should be
  visible in review as exactly that.
- A package moving out of `ZERO_COVERAGE_PACKAGES` must appear in `MEASURED_COVERAGE` in the same
  change — the 0 floor is a marker, and a marker that outlives its package is a lie.

---

## 3. Proof the gate is live

Script: `docs/audit/evidence/a6/prove-gate.sh` (restores the config through a `trap`, so an
interrupted run cannot leave an inflated threshold behind).

**Run with thresholds raised above the measured baseline** — global `lines` 33.2 → 34.2 and
`packages/core` `lines` 98.24 → 99.9:

```
$ bash docs/audit/evidence/a6/prove-gate.sh
armed: global lines 33.2 -> 34.2, packages/core lines 98.24 -> 99.9
EXIT=1
--- threshold violations ---
ERROR: Coverage for lines (33.2%) does not meet global threshold (34.2%)
ERROR: Coverage for lines (98.24%) does not meet "packages/core/src/**/*.ts" threshold (99.9%)
```

Both mechanisms fired in one run: the global floor and a per-package glob. Log:
`docs/audit/evidence/a6/gate-run-thresholds-armed.log`.

**Restored to the measured baseline** (sha256 of the config identical before and after the script,
`2f4c7e9be4047afadb24f6ae8fddc1b971d9dfb213e6555755b6aba7ec101bb8`):

```
EXIT=0
Statements   : 32.38% ( 7629/23555 )
Branches     : 33.91% ( 5001/14745 )
Functions    : 41.21% ( 1497/3632 )
Lines        : 33.2% ( 7159/21558 )
```

Log: `docs/audit/evidence/a6/gate-run-restored.log`. The percentages are byte-identical to the
baseline run, i.e. the suite is deterministic across runs and the floors sit exactly on the
measurement — no headroom, no drift.

---

## 4. Formatting

Targeted check on every file this card writes:

```
$ npx prettier --check vitest.config.ts docs/audit/evidence/a6/*.mjs docs/audit/A6_COVERAGE_GATE_2026-10-22.md
Checking formatting...
All matched files use Prettier code style!
PRETTIER_EXIT=0

$ npx eslint vitest.config.ts
ESLINT_EXIT=0
```

Logs: `docs/audit/evidence/a6/format-check.log`, `docs/audit/evidence/a6/eslint.log`.
(`.prettierignore` is not a Prettier target itself — Prettier has no parser for it; the file is
consumed, not formatted.)

Repo-wide, after the last write and one `prettier --write` on the generated table:

```
$ pnpm format:check
Checking formatting...
All matched files use Prettier code style!
REPO_FORMAT_EXIT=0
```

Log: `docs/audit/evidence/a6/format-check-repo.log`. Mid-run this check was transiently red on
`docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md` and `docs/audit/A4_EVIDENCE_DRIFT_2026-10-22.md` — both
owned by other audit tasks running concurrently against this shared worktree, both rewritten and
re-formatted by their owners while this card ran. They were left alone rather than reformatted from
here (editing another worker's live file is how you lose their edit) and are green now.

`.prettierignore` gained one entry, `docs/audit/evidence/**/*.json`, with the reason stated inline:
a raw coverage map is captured bytes, and reformatting it would make the artifact stop matching the
run that produced it. Markdown under the same tree stays checked.

## 5. Files

| file                                                                          | change                                                                                                                                                                                                                                                                             |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vitest.config.ts`                                                            | `test.coverage.thresholds` added: global floors + 66 per-scope globs; `MEASURED_COVERAGE`, `ZERO_COVERAGE_PACKAGES`, `GLOBAL_COVERAGE_THRESHOLDS`, `PACKAGE_COVERAGE_THRESHOLDS`; comments naming the coverage source and the ratchet rule. No other test/coverage option changed. |
| `.prettierignore`                                                             | one entry (`docs/audit/evidence/**/*.json`) + reason.                                                                                                                                                                                                                              |
| `docs/audit/evidence/a6/per-package-coverage.mjs`                             | per-scope aggregator on `istanbul-lib-coverage` (same arithmetic as the gate); resolves the transitive dep through `node_modules/.pnpm` because pnpm's strict layout does not expose it at the root.                                                                               |
| `docs/audit/evidence/a6/emit-thresholds.mjs`                                  | emits the config table from the raw map.                                                                                                                                                                                                                                           |
| `docs/audit/evidence/a6/prove-gate.sh`                                        | arms the gate above baseline, captures exit 1, restores, captures exit 0.                                                                                                                                                                                                          |
| `docs/audit/evidence/a6/tested-surface.txt`                                   | full report / zero-coverage / covered-scopes split.                                                                                                                                                                                                                                |
| `docs/audit/evidence/a6/per-package-coverage.{md,txt}`                        | the table above.                                                                                                                                                                                                                                                                   |
| `docs/audit/evidence/a6/per-package-coverage.py`                              | superseded first pass, kept for provenance: it rounds where istanbul floors, so it reads 0.01 high. Do not use it to set a threshold.                                                                                                                                              |
| `docs/audit/evidence/a6/gate-run-{at-baseline,thresholds-armed,restored}.log` | raw runs with `EXIT=` appended.                                                                                                                                                                                                                                                    |
| `docs/audit/evidence/a6/coverage-final-baseline.json`                         | raw istanbul map of the baseline run (the ratchet's reference artifact).                                                                                                                                                                                                           |
| `docs/audit/evidence/a6/baseline-coverage.log`                                | first baseline run (pre-threshold) with `EXIT=0`.                                                                                                                                                                                                                                  |

## 6. Reproduce

```bash
cd ~/Archverse/Lab/Archava_Platform
TMPDIR=/tmp pnpm exec vitest run --coverage          # gate green at baseline, 153 files / 1931 tests
node docs/audit/evidence/a6/per-package-coverage.mjs docs/audit/evidence/a6/coverage-final-baseline.json --md
bash docs/audit/evidence/a6/prove-gate.sh            # arms above baseline (exit 1) then restores (exit 0)
npx prettier --check vitest.config.ts .prettierignore docs/audit/evidence/a6/*.mjs
```

Nothing was committed; the worktree stays dirty exactly as it was found, plus the files above.
