/**
 * Tests for the parity comparator.
 *
 * These exist because "no divergence found" is a meaningless claim unless the
 * comparator has been shown to find *some*. If a change ever made `compare`
 * report `match` for everything, this file is what fails.
 *
 * Run with `node --test`, which ships with Node — no new dependency, per the
 * pilot's rule that dependencies come from official registries and stay
 * minimal.
 *
 * ## Why promoting this file was not optional
 *
 * In the pilot this lived under `experiments/`, outside every product glob, and
 * the header claimed that as the reason nothing in the product tree saw it.
 * That reason does not survive promotion: this file now sits under `tools/`, and
 * `vitest.config.ts` globs the packages, apps and tools directories for
 * `.test.ts` files. The reason it is still not double-run is narrower and worth
 * stating precisely — **the globs match the `.ts` extension only**, so a
 * `.test.mjs` file under `tools/` is matched by neither. That is a fact about the
 * extension, not about a directory boundary, and it stops being true the day a
 * glob is widened to cover `.mjs` too. If that happens, this file runs under
 * vitest as well as `node --test`, and node's `node:test` assertions are not
 * vitest's — it would need converting rather than assuming.
 *
 * Until this file was promoted, `compare.mjs`'s own header cited it as the proof
 * that the comparator's verdicts are trustworthy — proof that did not exist in
 * the product tree. A comment is not a test, and a promoted module that carries
 * the promoted module's claims without carrying its evidence is worse than one
 * that admits the gap.
 */

import test from 'node:test'
import assert from 'node:assert/strict'
import { compare, firstDifference } from './compare.mjs'

/** A quote with one difference, so a divergence is trivial to build. */
const QUOTE_A = `{"oneTimeTotal":2000,"recurringMonthlyTotal":249}`
const QUOTE_B = `{"oneTimeTotal":2500,"recurringMonthlyTotal":249}`

test('identical canonical output is a match', () => {
  assert.equal(compare('f', QUOTE_A, QUOTE_A).verdict, 'match')
})

test('a difference in the pricing result is a divergence, not an error class', () => {
  const verdict = compare('f', QUOTE_A, QUOTE_B)
  assert.equal(verdict.verdict, 'diverge')
  // The message names the field, so a human can find it without diffing two
  // full quotes by eye. The offset is checked separately, against a value the
  // test computes rather than a hand-counted constant that would silently
  // drift when the quote string changes.
  assert.match(verdict.message, /offset 17/)
  assert.match(verdict.message, /oneTimeTotal/)
})

test('a longer output on the Rust side is a divergence at the cut point', () => {
  // The prefix is shared, so a naive "does A appear in B" check would call this
  // a match. It is not: the Rust engine emitted keys the TS engine did not.
  const verdict = compare('f', `{"a":1}`, `{"a":1,"b":2}`)
  assert.equal(verdict.verdict, 'diverge')
  // Index 6 is where the shorter string ends — the `}` of the first object.
  assert.match(verdict.message, /offset 6/)
})

test('both engines failing with the same message is an error-class difference', () => {
  const verdict = compare(
    'f',
    'ERROR:PricingError: Unknown presence "telepathy".',
    'ERROR:Error: Unknown presence "telepathy".',
  )
  assert.equal(verdict.verdict, 'error-class-only')
})

test('both engines failing with different messages is a real difference', () => {
  const verdict = compare(
    'f',
    'ERROR:PricingError: Unknown presence "telepathy".',
    'ERROR:Error: Unknown capability "telepathy".',
  )
  assert.equal(verdict.verdict, 'error-message-only')
})

test('only one engine failing is a divergence, not an error class', () => {
  // The symmetric case: a quote on one side against a thrown error on the other
  // must never be classified as "both failed".
  assert.equal(compare('f', QUOTE_A, 'ERROR:Error: boom').verdict, 'diverge')
  assert.equal(compare('f', 'ERROR:PricingError: boom', QUOTE_B).verdict, 'diverge')
})

test('a multi-line error message still matches whole', () => {
  // The `s` flag is load-bearing here: serde's deserialization errors span
  // lines, and without it the regex would only match the first line and call
  // two identical multi-line errors different.
  const message = 'Rust could not deserialize the request:\n  at line 1 column 5\n  expected'
  const verdict = compare('f', `ERROR:PricingError: ${message}`, `ERROR:Error: ${message}`)
  assert.equal(verdict.verdict, 'error-class-only')
})

test('firstDifference finds the offset of the first differing character', () => {
  // Asserted against what the strings actually say, so a wrong constant fails
  // here instead of at runtime.
  assert.equal(firstDifference(QUOTE_A, QUOTE_A), QUOTE_A.length)
  assert.equal(firstDifference(QUOTE_A, QUOTE_B), 17)
  assert.equal(firstDifference('abc', 'abd'), 2)
  assert.equal(firstDifference('', 'abc'), 0)
  // One is a prefix of the other: the index is the shorter length.
  assert.equal(firstDifference('ab', 'abc'), 2)
})

test('a match produces no message', () => {
  // No message keeps the tally cheap and the report clean.
  assert.equal(compare('f', QUOTE_A, QUOTE_A).message, undefined)
})

// The `corrected-expectation` verdict, added after promotion and untested in the
// pilot. These five cases are the ones that decide whether D02's five approved
// `__proto__` fixtures are actually *verified* each run or merely excused — so
// both directions are asserted here, not just the happy one.

test('an approved divergence whose behaviour matches is a corrected expectation', () => {
  const verdict = compare('unknown-units-built-proto', '{}', 'null', {
    rust: 'null',
    reason: 'D02: baseline returns Object.prototype as a room count; port returns null',
  })
  assert.equal(verdict.verdict, 'corrected-expectation')
  // The reason travels with the verdict, so the evidence file explains itself
  // rather than pointing a reader at a document.
  assert.match(verdict.message, /D02: baseline returns Object\.prototype/)
})

test('an approval is ignored when the engines already agree', () => {
  // Approving a fixture that matches would be meaningless, and `compare` must
  // not be talked into reporting a divergence that did not happen.
  const verdict = compare('f', QUOTE_A, QUOTE_A, { rust: 'null', reason: 'irrelevant' })
  assert.equal(verdict.verdict, 'match')
})

test('an approved divergence whose behaviour changed reverts to diverge', () => {
  // The load-bearing case. If the port stopped producing the behaviour D02
  // approved, this must fail the run — otherwise the approval is an assumption
  // wearing an approval's clothes.
  const verdict = compare('unknown-units-built-proto', '{}', 'undefined', {
    rust: 'null',
    reason: 'D02: baseline returns Object.prototype as a room count; port returns null',
  })
  assert.equal(verdict.verdict, 'diverge')
  // And it must say *why* it diverged, naming both what was approved and what
  // was produced, or an operator cannot act on it.
  assert.match(verdict.message, /approved: null/)
  assert.match(verdict.message, /no longer produces it/)
})

test('an unapproved divergence is a plain diverge', () => {
  // The control case: without an approval the same inputs are simply a failure.
  assert.equal(compare('unknown-units-built-proto', '{}', 'null').verdict, 'diverge')
})
