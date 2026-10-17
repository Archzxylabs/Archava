/**
 * The p2 parity harness: the reference rates slice, both engines, compared.
 *
 * This is the second half of the pilot's method. The first slice (p1) proved the
 * question is answerable at all: 346 pricing fixtures, 338 identical and 8
 * differing only in the error's *type* at the napi boundary. This slice asks the
 * same question about a different kind of code — not arithmetic over a pricebook
 * but closed-table lookups, exclusive windows, and money in integer minor units.
 *
 * ## What is compared here, and how
 *
 * The same four verdicts and the same canonical writer as p1, reused rather than
 * reimplemented: `compare.mjs` and `canonical.mjs` are imported, not copied. A
 * second implementation of the comparator would be a second thing that can be
 * subtly wrong.
 *
 * Both engines are handed the *identical request bytes*. The TypeScript side is
 * called positionally — `nightlyRate(unitId, night)` — and the exact same four
 * strings are serialized into one JSON request for `rates_json`. So the two
 * sides cannot disagree about what was asked; only the answering can differ.
 *
 * ## The absent case
 *
 * `rates_json` returns the JSON literal `null` for a request the fixtures do not
 * cover, and the baseline returns `undefined`. `canonicalizeValue(undefined)`
 * writes `null`, so the two converge — which is why this bridge can be a
 * string-in / string-out one with no special casing on the boundary. It is safe
 * *here* because no accessor in this slice ever returns a `null` field; that is
 * a property of this slice, not a general rule for the bridge.
 *
 * ## One divergence is expected, and it is not excused
 *
 * `__proto__` is the case, and the baseline's behaviour on it is worse than a
 * wrong `roomsBuilt`. The baseline reads `BASE_RATES[unitId]`, which walks the
 * prototype chain and returns `Object.prototype` rather than `undefined`. The
 * port's `BTreeMap::get` has no prototype chain and returns `None`.
 *
 * The baseline then feeds that `Object.prototype` into arithmetic:
 *
 *   roomsBuilt('__proto__')  -> {}                                  (expects number)
 *   baseRate('__proto__')    -> {}                                  (expects number)
 *   nightlyRate('__proto__') -> amountMinor: {}                     (a number field)
 *   stayQuote('__proto__')   -> subtotalMinor: "0[object Object][object Object]
 *                                            [object Object][object Object]NaNNaN"
 *                               taxMinor: NaN, totalMinor: "...NaNNaN"
 *                               (a string field where the type says money)
 *   nightlyAvailability(...) -> TypeError: (CLOSED_NIGHTS[unitId] ?? []).includes
 *                               is not a function
 *
 * The port is right about all five and the baseline is wrong about all five. The
 * divergence is reported, the run exits non-zero, and the finding is written into
 * the evidence file with both engines' output. A parity harness that quietly
 * drops the case it knows will differ is not measuring parity — it is measuring
 * how well it avoids measuring parity.
 *
 * The `error-class-only` count of 0 here is worth noting against p1's 8: the
 * difference is that the baseline *throws* on this slice only for the one
 * `nightlyAvailability` path, and that one diverges outright rather than landing
 * on a message-only match.
 */

import { existsSync } from 'node:fs'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { requireRust } from './runtime.mjs'
// Reused, not reimplemented: the comparator is the thing that decides the
// verdict, and `compare.test.mjs` proves *this* code path catches what it claims.
import { compare } from './compare.mjs'
import { canonicalize, canonicalizeValue, sha256 } from './canonical.mjs'

// Node globals this harness touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

/** The case file both engines are run against. */
const CASES = fileURLToPath(new URL('./cases/rates.json', import.meta.url))
/** Where the evidence lands. */
const EVIDENCE = fileURLToPath(new URL('./evidence/p2-parity.json', import.meta.url))
/**
 * The product root, three levels up from `tools/rust/parity/`.
 *
 * This constant is asserted rather than assumed, and it has to be: it previously
 * read `../../../Archava_Platform/`, a path hardcoded to the pilot's location
 * under `Lab/Experiments/rust-pilot/node/`. That is the same defect class as
 * `archava-pricing`'s `include_str!("../../../fixtures/...")` (D10) — a relative
 * climb that resolved from where the code was written and breaks wherever it is
 * moved to. `assertProductRoot()` below is what turns a silent wrong resolution
 * into a readable failure.
 */
const WORKSPACE_ROOT = fileURLToPath(new URL('../../../', import.meta.url))
/** The baseline, straight from source. */
const RATES_PATH = fileURLToPath(
  new URL('packages/reference/src/rates.ts', pathToFileURL(`${WORKSPACE_ROOT}/`)),
)

/**
 * Load the baseline `rates.ts` as a live module.
 *
 * `@archava/reference` ships TypeScript with no build step, and its workspace
 * links are absent from this experiment's node_modules on purpose — the pilot
 * adds no dependency to the product. So the same three-step incantation p1 uses
 * is repeated here rather than shared into a `loader.mjs`: the resolution rules
 * are the same, but this file's imports differ enough that a shared module would
 * be parameterized on both paths *and* the parent URL, which is more indirection
 * than the duplication costs.
 *
 * The `parentURL` must be a `file://` inside the product: `rates.ts` imports
 * `@archava/config`, `@archava/assistant` and `@archava/knowledge`, and those are
 * only resolvable from inside the tree that has `packages/config`. Passing this
 * file's own URL resolves them against wherever the harness happens to live and
 * fails.
 *
 * @returns {Promise<typeof import("../../../../packages/reference/src/rates.ts")>}
 */
async function loadBaseline() {
  assertProductRoot()
  const require = createRequire(import.meta.url)
  const tsxApiPath = require.resolve('tsx/esm/api', { paths: [WORKSPACE_ROOT] })
  const { tsImport } = await import(pathToFileURL(tsxApiPath).href)
  return tsImport(RATES_PATH, new URL(`file://${WORKSPACE_ROOT}`).href)
}

/**
 * Check the computed product root really is the product root.
 *
 * `WORKSPACE_ROOT` is three `..` from `tools/rust/parity/`. Relative arithmetic
 * that happens to be right is indistinguishable from relative arithmetic that is
 * right *here* — which is precisely how `archava-pricing`'s fixture path broke on
 * promotion (D10): it resolved from the pilot's directory and not from the
 * crate's. Asserting the root's identity converts an invisible wrong resolution
 * into an immediate, named failure, and costs one stat.
 *
 * @throws when the computed root is not a recognisable product root.
 */
function assertProductRoot() {
  const ok =
    existsSync(join(WORKSPACE_ROOT, 'package.json')) &&
    existsSync(join(WORKSPACE_ROOT, 'packages', 'reference', 'package.json'))
  if (!ok) {
    throw new Error(
      `Parity harness resolved the workspace root to ${WORKSPACE_ROOT}, which is not the ` +
        `product root. The harness must live exactly three levels below the product ` +
        `root — it is at tools/rust/parity/.`,
    )
  }
}

/**
 * Every op the bridge dispatches on, against the baseline function of the same
 * name.
 *
 * One table rather than a `switch` in the runner, so the argument order — which
 * is the thing that can be silently transposed — is stated once per op, next to
 * the strings it is handed.
 *
 * @type {Record<string, (rates: object, request: Record<string, string>) => unknown>}
 */
const OPS = {
  nightlyRate: (rates, r) => rates.nightlyRate(r.unitId, r.night),
  nightlyAvailability: (rates, r) => rates.nightlyAvailability(r.unitId, r.night),
  nightsBetween: (rates, r) => rates.nightsBetween(r.from, r.to),
  stayAvailability: (rates, r) => rates.stayAvailability(r.unitId, r.from, r.to),
  stayQuote: (rates, r) => rates.stayQuote(r.unitId, r.from, r.to),
  bookableStays: (rates, r) => rates.bookableStays(r.from, r.to),
  cheapestBookable: (rates, r) => rates.cheapestBookable(r.from, r.to),
  roomsBuilt: (rates, r) => rates.roomsBuilt(r.unitId),
  baseRate: (rates, r) => rates.baseRate(r.unitId),
  isReferenceNight: (rates, r) => rates.isReferenceNight(r.night),
  isWeekendNight: (rates, r) => rates.isWeekendNight(r.night),
}

/** The exact set the generator emits, checked so a new op cannot go unrun. */
const EXPECTED_OPS = Object.keys(OPS).sort()

/**
 * Run one case through the TypeScript engine.
 *
 * @param {object} rates the loaded baseline module
 * @param {Record<string, string>} request the raw case, minus id/op/note
 * @returns {{ok: boolean, quote?: unknown, error?: string}}
 */
function runTypeScript(rates, request) {
  const call = OPS[request.op]
  // A missing op is a harness failure, not a parity result. It means the
  // generator emits an op the bridge does not dispatch, and reporting it as a
  // divergence would blame the port for the harness's own gap.
  if (!call) {
    throw new Error(
      `case ${request.id} names op ${request.op}, which the harness has no baseline call for`,
    )
  }
  try {
    return { ok: true, quote: call(rates, request) }
  } catch (error) {
    return {
      ok: false,
      error: `${error?.name ?? 'Error'}: ${error?.message ?? String(error)}`,
    }
  }
}

/**
 * Run one case through the Rust engine.
 *
 * The request is serialized, never passed as an object: the boundary is a string
 * one so the Rust side reads the same bytes the TypeScript side was effectively
 * handed. The five keys are named explicitly rather than spread from the case,
 * because the bridge deserializes strictly — a stray key is a rejection, not an
 * ignored property. `id` and `note` are the harness's own bookkeeping and are
 * deliberately absent.
 *
 * Naming them here rather than omitting them with a rest pattern is also what
 * keeps `no-unused-vars` quiet under this repo's config (which ignores `^_` for
 * function args only), but the honest reason is the one above: the field list is
 * the contract with the deserializer, and a contract should be spelled out.
 *
 * @param {Record<string, Function>} addon the native addon
 * @param {Record<string, string>} item one case
 * @returns {{ok: boolean, quote?: unknown, error?: string}}
 */
function runRust(addon, item) {
  const { op, unitId, night, from, to } = item
  try {
    const answer = addon.ratesJson(JSON.stringify({ op, unitId, night, from, to }))
    return { ok: true, quote: canonicalize(answer) }
  } catch (error) {
    return {
      ok: false,
      error: `${error?.name ?? 'Error'}: ${error?.message ?? String(error)}`,
    }
  }
}

/**
 * Canonicalize one engine's outcome.
 *
 * @param {{ok: boolean, quote?: unknown, error?: string}} outcome
 * @returns {string} canonical text, or an `ERROR:` line for a thrown failure
 */
function serialize(outcome) {
  if (!outcome.ok) return `ERROR:${outcome.error}`
  // The Rust side has already canonicalized (it arrived as text), so handing its
  // string through the writer once more would be a parse of text that is already
  // canonical. Both paths converge here, exactly as in p1.
  return typeof outcome.quote === 'string' ? outcome.quote : canonicalizeValue(outcome.quote)
}

/**
 * The parity run.
 *
 * Both engines are required; a missing addon aborts the run rather than
 * degrading into a "TS only" pass.
 */
async function main() {
  const casesText = readFileSync(CASES, 'utf8')
  const { cases, count } = JSON.parse(casesText)

  // Checked rather than trusted: `count` is the generator's own tally of what it
  // wrote, so a truncated or hand-edited case file is caught here instead of
  // reporting a confident verdict over a partial set.
  if (count !== cases.length) {
    throw new Error(
      `case file is inconsistent: it declares count ${count} but carries ${cases.length} cases`,
    )
  }

  // Every op in the file must have a baseline call. A generator that starts
  // emitting an op the harness does not know about would otherwise run it
  // against nothing and record a false match.
  const present = [...new Set(cases.map((item) => item.op))].sort()
  if (present.join(',') !== EXPECTED_OPS.join(',')) {
    throw new Error(
      `case file covers ops this harness does not: ${present.join(', ')} ` +
        `(expected ${EXPECTED_OPS.join(', ')})`,
    )
  }

  const rates = await loadBaseline()
  // `runtime.mjs` fails loudly when the addon is absent, so this line is a gate,
  // not a fallback.
  const addon = requireRust()

  // The five `__proto__` fixtures, with the behaviour the port is approved to
  // have instead of the baseline's. Recorded per D02: the baseline walks
  // `Object.prototype` and produces `{}`, a string containing `NaNNaN`, or a
  // `TypeError` where the type says money or a number; the port has no prototype
  // chain and correctly reports absence as `null`.
  //
  // These are approved, not excused. Each entry states the exact canonical string
  // the port must still produce, and `compare.mjs` re-checks it on every run — so
  // if the port's `__proto__` behaviour ever changes, this stops being an approved
  // expectation and becomes a `diverge` that fails the run. An approval that is
  // not re-verified is an approval that has quietly become an assumption.
  //
  // The alternative — deleting the fixtures — is what the harness's own header
  // warns against: dropping the cases that are known to differ stops measuring
  // parity and starts measuring how well parity is avoided.
  const APPROVED = new Map([
    [
      'unknown-units-built-proto',
      {
        rust: 'null',
        reason: 'D02: baseline returns Object.prototype as a room count; port returns null',
      },
    ],
    [
      'unknown-units-rate-proto',
      {
        rust: 'null',
        reason: 'D02: baseline returns Object.prototype as a base rate; port returns null',
      },
    ],
    [
      'unknown-units-nightly-proto',
      { rust: 'null', reason: 'D02: baseline yields amountMinor: {}; port returns null' },
    ],
    [
      'unknown-units-availability-proto',
      {
        rust: 'null',
        reason:
          'D02: baseline throws TypeError on .includes over Object.prototype; port returns null',
      },
    ],
    [
      'unknown-units-stay-proto',
      {
        rust: 'null',
        reason: 'D02: baseline builds a "0[object Object]...NaNNaN" subtotal; port returns null',
      },
    ],
  ])

  // An approved id that no case file contains is a stale approval — the fixture
  // was removed or renamed, and the approval is now documenting nothing. Checking
  // this here rather than trusting the list keeps the two from drifting apart
  // silently, the same way D08 records a ledger drifting from its migrations.
  const caseIds = new Set(cases.map((item) => item.id))
  for (const id of APPROVED.keys()) {
    if (!caseIds.has(id)) {
      throw new Error(
        `approved corrected expectation names ${id}, which the case file does not contain. ` +
          `An approval for a fixture that no longer exists documents nothing; remove it or restore the fixture.`,
      )
    }
  }

  const rows = []
  const diverge = []
  const corrected = []
  const errorClassOnly = []
  const errorMessageOnly = []
  for (const item of cases) {
    const tsText = serialize(runTypeScript(rates, item))
    const rustText = serialize(runRust(addon, item))
    const verdict = compare(item.id, tsText, rustText, APPROVED.get(item.id))
    rows.push({ id: item.id, verdict, ts: tsText, rust: rustText })
    if (verdict.verdict === 'diverge') diverge.push(verdict.message)
    if (verdict.verdict === 'corrected-expectation') corrected.push(verdict.message)
    if (verdict.verdict === 'error-class-only') errorClassOnly.push(verdict.message)
    if (verdict.verdict === 'error-message-only') errorMessageOnly.push(verdict.message)
  }

  // Counted separately, because they are not the same claim. A "match" is the
  // two engines agreeing outright. `error-class-only` means they agree on the
  // message but not on the error's type — a real boundary divergence the pilot
  // reports rather than folds into the pass rate. Only `diverge` means the
  // results themselves differ.
  //
  // `corrected-expectation` is in its own bucket and is deliberately *not*
  // folded into `match`. They are different claims: a match is the engines
  // agreeing, a corrected expectation is the engines disagreeing in a direction
  // that has been reviewed and recorded. Merging them would let a future reader
  // count 758 agreements where 5 are known disagreements — which is exactly the
  // inflation this file exists to prevent.
  const tally = {
    match: 0,
    'corrected-expectation': 0,
    'error-class-only': 0,
    'error-message-only': 0,
    diverge: 0,
  }
  for (const row of rows) tally[row.verdict.verdict] += 1

  // Divergences are recorded with both engines' output, not just a message, so
  // the evidence file can be read without rerunning anything. The `__proto__`
  // finding is the reason this array exists.
  const findings = rows
    .filter((row) => row.verdict.verdict === 'diverge')
    .map((row) => ({ id: row.id, ts: row.ts, rust: row.rust }))

  // Approved divergences are recorded the same way, with the reason attached, so
  // the evidence file carries *why* they differ rather than leaving a reader to
  // reconcile a 748 match count against five known disagreements.
  const correctedExpectations = rows
    .filter((row) => row.verdict.verdict === 'corrected-expectation')
    .map((row) => ({
      id: row.id,
      ts: row.ts,
      rust: row.rust,
      approved: APPROVED.get(row.id),
    }))

  const manifest = {
    slice: 'p2',
    // Both engines, always — not the value of `RUST_PILOT_RUNTIME`.
    //
    // This field was `requestedRuntime()`, which reports the *environment
    // variable* and defaults to `'ts'`. Nothing selects an engine from it —
    // `requireRust()` above throws when the addon is absent rather than falling
    // back — so a run that executed both engines wrote `"runtime": "ts"` into
    // its own evidence. An evidence file whose job is to prove Rust was
    // exercised should not state the opposite, so it now states what ran. The
    // variable is still consulted at the top of the run for its one real effect
    // (rejecting a malformed value); it simply no longer describes the run.
    runtime: 'ts+rust',
    fixtures: cases.length,
    ...tally,
    // `passed` counts matches *and* approved corrected expectations, because both
    // are states the run is green on. `match` on its own is left in `tally` at its
    // own number, so nobody has to subtract to find how many were outright
    // agreements — the two figures are both in the file and neither is implied by
    // the other.
    passed: tally.match + tally['corrected-expectation'],
    failed: cases.length - (tally.match + tally['corrected-expectation']),
    casesSha256: sha256(casesText),
    // One digest over every row in order, so a rerun over the same data is
    // provably the same result.
    resultsSha256: sha256(rows.map((row) => `${row.id}:${row.verdict.verdict}`).join('\n')),
    findings,
    // The approvals themselves, not just their count. `tally` reports that five
    // fixtures were corrected; this reports *which* five, what each engine said,
    // and the reason recorded for each. Without it the evidence file asserts five
    // known disagreements and leaves the reader to go and find them — the exact
    // "reconcile 748 against five" gap the comment above names.
    //
    // This block was computed and then never added to the manifest: eslint
    // caught the unused binding, and the run was green anyway because nothing
    // downstream read the approvals. A parity harness that records a corrected
    // expectation in its own tally but not in its evidence has done the harder
    // half of the job and skipped the half an auditor can check.
    correctedExpectations,
  }

  writeFileSync(EVIDENCE, `${JSON.stringify(manifest, null, 2)}\n`)
  // The whole manifest goes to stdout, same as p1. An earlier revision omitted
  // `findings` here on the grounds that "753 rows twice over" is unreadable —
  // but `findings` only ever holds *divergences*, which is zero on a green run,
  // and `correctedExpectations` holds five. Neither is large, both are the part
  // a reader most wants, and the omission had to be spelled with a rest
  // destructuring that this repo's `no-unused-vars` rejects. Printing the same
  // object that was written is both simpler and the more honest output: stdout
  // and the evidence file are now provably the same document.
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`)

  if (diverge.length > 0) {
    process.stdout.write(`\n${diverge.slice(0, 10).join('\n\n')}\n`)
  }
  if (errorClassOnly.length > 0) {
    process.stdout.write(
      `\n${errorClassOnly.length} fixtures agree on the error message but not on the error type ` +
        `(TS throws, Rust raises napi Error) — a boundary difference, not a logic one.\n`,
    )
  }
  if (errorMessageOnly.length > 0) {
    process.stdout.write(`\n${errorMessageOnly.slice(0, 10).join('\n\n')}\n`)
  }
  // Divergences and message-only differences both fail the run. The error-class
  // mismatch is reported and recorded, then judged in the writeup — same rule as
  // p1.
  if (diverge.length > 0 || errorMessageOnly.length > 0) {
    process.exitCode = 1
  }
}

await main()
