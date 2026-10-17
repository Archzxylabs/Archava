import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'
import { requireRust } from '../parity/runtime.mjs'

/* global URL */

/**
 * The p1 error-contract disposition, proven rather than restated.
 *
 * ## Why this file exists
 *
 * `docs/rust/KNOWN_DIFFERENCES.md` D01 records that p1 has 12 `error-class-only`
 * differences. That record is a *claim about the two engines' behaviour*, and
 * until this file existed nothing checked it. It could have drifted in either
 * direction without a single gate noticing: if the port had started matching,
 * D01 would have been describing a difference that no longer existed; if the
 * port had broken further, D01 would have been describing a difference that was
 * no longer the only one. `EXPECTED_PARITY` in `verify.mjs` pins the counts, but
 * a count is not a contract — this file is what says *what* the 12 are.
 *
 * ## What the disposition actually is
 *
 * Measured on this tree, by running all 350 pricing cases through the addon:
 * **all 12 are engine refusals whose messages are character-identical to
 * `engine.ts`'s**. Not one is a serde deserialization error. The only thing that
 * differs is `error.name` — `Error` from the addon, `PricingError` from
 * TypeScript.
 *
 * That difference is not a defect in the port. It is a capability boundary:
 * napi 3.14.2's `JsError::into_value` calls `napi_create_error`, which takes a
 * code and a message and has no name field, so every `napi::Error` crossing into
 * JavaScript is an `Error`. No amount of work in `archava-native` can change it.
 *
 * So the two halves of §1.1 item 2 resolve differently, and this file is what
 * proves each half:
 *
 * - **The crate boundary (shipping)** preserves the contract. `PricingError`
 *   keeps its message, and `PricingErrorKind::kind()` is a real discriminant the
 *   TypeScript engine does not even have.
 * - **The addon boundary (not shipping)** cannot carry `name`. It is recorded as
 *   a versioned non-shipping disposition rather than fixed, because it is not
 *   fixable at that boundary and no product package consumes it — a grep of
 *   `apps/`, `packages/`, `tools/` and `config/` for the addon's exports finds
 *   only its own build step and Consumer B.
 *
 * ## What the fixture is not
 *
 * Every request below is read out of `tools/rust/parity/cases/pricing.json` by
 * id. It is not hand-written, because the first hand-written version of this
 * file was wrong in a way that would have hidden the real finding: it used
 * `region: 'MARS'` to reach the unknown-region refusal, and `MARS` is not an
 * engine refusal at all — `region` is a typed enum in Rust, so an out-of-variant
 * string is rejected by serde at the boundary (`InvalidArg`, "unknown variant
 * `MARS`, expected `GLOBAL` or `ID`"), while TypeScript's untyped string reaches
 * the engine. Reading the cases keeps this file aligned with the requests the
 * parity evidence actually classified.
 */

const root = fileURLToPath(new URL('../../../', import.meta.url))
const require = createRequire(import.meta.url)

/**
 * The case ids that the live p1 run classifies as `error-class-only`.
 *
 * Twelve, exactly, and read from the shared case file rather than duplicated
 * here: if a case is renamed or removed the test fails naming the id, which is
 * the same discipline `assertParityExpectations` applies to the counts, applied
 * to the identities behind the count. `verify.mjs`'s `EXPECTED_PARITY` pins the
 * number; this list pins which twelve.
 */
const ERROR_CLASS_ONLY_IDS = [
  'absent-region',
  'absent-presence',
  'absent-capability',
  'absent-environment',
  'invalid-presence',
  'invalid-capability',
  'invalid-environment',
  'invalid-all-three-check-order',
  'invalid-capability-and-environment',
  'invalid-presence-empty-string',
  'invalid-presence-in-id-only',
  'invalid-whitespace-presence',
]

/**
 * The requests the boundary itself rejects, which is the other half of the
 * discrimination.
 *
 * `region` rejects in serde because it is a typed enum; `derived` is rejected
 * because `QuoteRequestInit` is `deny_unknown_fields`. Both reach the boundary
 * without either engine ever seeing them, so both must be distinguishable from
 * an engine refusal by more than their message text.
 */
const BOUNDARY_REJECTIONS = [
  { id: 'out-of-variant region', request: { region: 'MARS' } },
  { id: 'unknown field', request: { region: 'GLOBAL', derived: true } },
]

function shippedPricebook() {
  return readFileSync(`${root}/config/pricing.v1.json`, 'utf8')
}

/**
 * The parity cases, by id.
 *
 * Read from the shared file so there is exactly one definition of what was
 * measured. A copy here would be a second definition, and the two would drift.
 */
function casesById() {
  const file = JSON.parse(readFileSync(`${root}/tools/rust/parity/cases/pricing.json`, 'utf8'))
  return new Map(file.cases.map((entry) => [entry.id, entry]))
}

/**
 * Load the TypeScript engine, through the same route the parity harness uses.
 *
 * Not re-implemented: `run-parity.mjs` builds a `tsx` importer against the
 * workspace root after checking the root is recognisably the product root. A
 * second loader would be a second way to locate the baseline, and the two would
 * eventually disagree about which file the baseline is.
 *
 * @returns {Promise<typeof import("../../../../packages/pricing/src/engine.ts")>}
 */
async function loadBaseline() {
  const enginePath = fileURLToPath(
    new URL('../../../packages/pricing/src/engine.ts', import.meta.url),
  )
  const tsxApiPath = require.resolve('tsx/esm/api', { paths: [root] })
  const { tsImport } = await import(tsxApiPath)
  return tsImport(enginePath, new URL(`file://${root}/`).href)
}

/** The message the baseline throws for one request, or null if it accepted it. */
function baselineMessage(engine, request) {
  try {
    engine.quote(request)
    return null
  } catch (error) {
    return error.message
  }
}

/** The error the addon throws for one request, or null if it accepted it. */
function addonError(addon, pricebookText, request) {
  try {
    addon.quoteJson(pricebookText, JSON.stringify(request))
    return null
  } catch (error) {
    return error
  }
}

test('every one of the 12 error-class-only cases is an engine error with a matching message', async (t) => {
  const addon = requireRust()
  const pricebookText = shippedPricebook()
  const baseline = await loadBaseline()
  const engine = new baseline.PricingEngine(JSON.parse(pricebookText))
  const cases = casesById()

  for (const id of ERROR_CLASS_ONLY_IDS) {
    const entry = cases.get(id)
    assert.ok(entry, `the parity case file no longer carries ${id}; D01's list of 12 is stale`)

    await t.test(`${id}: the message survives the boundary character for character`, () => {
      const tsMessage = baselineMessage(engine, entry.request)
      assert.notEqual(tsMessage, null, 'the baseline accepted a request it should refuse')

      const rustError = addonError(addon, pricebookText, entry.request)
      assert.ok(rustError, 'the port accepted a request it should refuse')

      // The disposition's whole content, and the reason D01 is a *disposition*
      // rather than a regression. If this ever fails, D01 is no longer accurate
      // and must be rewritten — the rule `assertParityExpectations` enforces for
      // the counts, applied to the thing the counts are counting.
      assert.equal(
        rustError.message,
        tsMessage,
        `the addon's message for ${id} drifted from engine.ts`,
      )
    })
  }
})

test('the addon boundary cannot carry the error name, and this is measured rather than assumed', () => {
  const addon = requireRust()
  const pricebookText = shippedPricebook()
  const cases = casesById()

  for (const id of ERROR_CLASS_ONLY_IDS) {
    const rustError = addonError(addon, pricebookText, cases.get(id).request)
    assert.ok(rustError, `${id} was refused`)

    // The whole reason D01 is a disposition rather than a fix. Asserting the
    // *absence* of the capability is what stops a future improvement from being
    // reported as a regression, and equally stops someone "fixing" D01 by
    // editing its prose while the boundary still cannot do the thing.
    assert.equal(rustError.name, 'Error', `${id} carried something other than a plain Error`)
    assert.notEqual(rustError.name, 'PricingError')
  }
})

test('all 12 carry the structured status of an engine refusal, not a malformed request', () => {
  const addon = requireRust()
  const pricebookText = shippedPricebook()
  const cases = casesById()

  for (const id of ERROR_CLASS_ONLY_IDS) {
    const rustError = addonError(addon, pricebookText, cases.get(id).request)
    assert.ok(rustError, `${id} was refused`)

    // A boundary change that started wrapping engine refusals in a
    // deserialization-shaped message would silently reclassify these 12 in the
    // parity evidence — from `error-class-only` to `deserializeError` — and the
    // count is the only part `EXPECTED_PARITY` pins. This is the check that
    // catches the reclassification before the evidence does.
    assert.equal(rustError.code, 'GenericFailure', `${id} was not an engine refusal`)
    assert.doesNotMatch(rustError.message, /could not deserialize/)
  }
})

test('a request the boundary itself rejects is distinguishable from an engine refusal', () => {
  const addon = requireRust()
  const pricebookText = shippedPricebook()

  // The other half of the same discrimination. Asserting both directions is what
  // makes it real — a check that only asserted `GenericFailure` would pass just
  // as happily if every error, including these, carried that code.
  for (const { id, request } of BOUNDARY_REJECTIONS) {
    const rustError = addonError(addon, pricebookText, request)
    assert.ok(rustError, `${id} was refused`)
    assert.equal(rustError.code, 'InvalidArg', `${id} reached the engine`)
    assert.match(rustError.message, /could not deserialize the request/)
  }
})
