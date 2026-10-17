/**
 * Canonical serialization for the parity comparison.
 *
 * Both engines compute the same `f64` arithmetic but print it differently:
 * serde_json emits `2000.0` for an integer-valued float while
 * `JSON.stringify` emits `2000`, and the two disagree on exponent spelling
 * for very small values. Neither engine is wrong, so comparing their raw text
 * would report every single fixture as divergent and answer nothing.
 *
 * The fix is not to loosen the comparison — it is to make both sides print
 * identically by parsing each engine's output back into a value and
 * re-printing it with one canonical writer. That keeps every property that
 * matters:
 *
 * - Key *order* is preserved, because `JSON.parse` preserves insertion order
 *   and the writer walks `Object.keys()` in that order. A key-order change is
 *   still a divergence, which it should be — `engine.ts`'s output literal has
 *   a fixed order and the port must match it.
 * - Value equality stays exact. There is no tolerance anywhere: two `f64`s
 *   differing in the last bit produce different tokens and fail.
 * - `-0` collapses to `0`. `JSON.parse` reads `-0` as the negative zero and
 *   `String(-0)` is `"0"`, so the Rust side's `-0.0` and the TS side's `-0`
 *   both become the same token — which is right, since the JSON spec treats
 *   signed zero as zero.
 *
 * ## Why not compare parsed objects with a deep-equal?
 *
 * A deep-equal throws away the key order, and the order is one of the things
 * being verified — `Quote`'s 26 keys are spelled in a fixed order by
 * `engine.ts`, and the port must match it. A text form keeps that check.
 */

import { createHash } from 'node:crypto'

/**
 * Write one number the way both engines' output is normalized to.
 *
 * @param {number} value
 * @returns {string} canonical token
 */
function writeNumber(value) {
  if (Number.isNaN(value)) return 'NaN'
  if (value === Infinity) return 'Infinity'
  if (value === -Infinity) return '-Infinity'
  if (Object.is(value, -0)) return '0'
  // `String` is the shortest round-tripping form in JS, which is exactly the
  // property needed: the same `f64` prints the same token from both sides.
  return String(value)
}

/**
 * Write a string the way JSON does.
 *
 * Delegating to `JSON.stringify` avoids reimplementing escape rules; it is
 * applied per-string rather than once at the top so that this writer stays in
 * control of number formatting.
 *
 * @param {string} value
 * @returns {string}
 */
function writeString(value) {
  return JSON.stringify(value)
}

/**
 * Walk a value and emit its canonical text.
 *
 * @param {unknown} value
 * @returns {string}
 */
function write(value) {
  if (value === null) return 'null'
  if (typeof value === 'number') return writeNumber(value)
  if (typeof value === 'string') return writeString(value)
  if (typeof value === 'boolean') return value ? 'true' : 'false'
  if (Array.isArray(value)) return `[${value.map(write).join(',')}]`
  if (typeof value === 'object') {
    // `Object.keys` in insertion order — the order the source JSON spelled the
    // keys in, which is the order under test.
    const entries = Object.keys(value)
      .map((key) => `${writeString(key)}:${write(value[key])}`)
      .join(',')
    return `{${entries}}`
  }
  // `undefined` appears only as an array hole or an explicit value; JSON
  // stringifies both as `null`, so this matches.
  return 'null'
}

/**
 * Canonicalize JSON text from either engine.
 *
 * @param {string} text
 * @returns {string} canonical text
 */
export function canonicalize(text) {
  return write(JSON.parse(text))
}

/**
 * Canonicalize an already-parsed value.
 *
 * Used for the TypeScript side, which hands back a live object.
 *
 * @param {unknown} value
 * @returns {string} canonical text
 */
export function canonicalizeValue(value) {
  return write(value)
}

/**
 * SHA-256 of a string, as hex.
 *
 * @param {string} text
 * @returns {string}
 */
export function sha256(text) {
  return createHash('sha256').update(text).digest('hex')
}
