/**
 * Consumer B: a cold Node process that loads the shipped addon.
 *
 * Why a *second* consumer at all, and why this way. G05 asks for "two
 * independent cold consumers use identical shipped core/domain". One consumer
 * answers half the question. A second consumer built the same way answers it
 * twice over the same route, which is the kind of evidence that looks like more
 * than it is.
 *
 * So the two here consume the artifacts through different boundaries:
 *
 * | | Consumer A | Consumer B |
 * |---|---|---|
 * | seam | Rust crates, linked | napi addon, required |
 * | language | Rust test | Node script |
 * | input path | compiled in | read at run time |
 * | crates reached | the pricing crate only | both, through the bridge |
 *
 * They share the artifacts and nothing else. Consumer A cannot detect a broken
 * addon; consumer B cannot detect a crate that will not link. A defect in one
 * route is invisible to the other, which is what "independent" has to mean for
 * the pair to cover anything.
 *
 * ## What makes this cold, specifically
 *
 * Three things, all of which are checked rather than assumed:
 *
 * 1. **No symlink into the product tree.** The addon is *copied* into the
 *    scratch directory. A require that resolved through the repository would be
 *    a path into the workspace — the exact thing G05 names as FAIL.
 * 2. **No parity-harness code.** This file does not import `runtime.mjs`,
 *    `compare.mjs`, `canonical.mjs` or any case set. The addon's existing
 *    loader is deliberately not reused: a consumer that shared its loader with
 *    the harness would be the harness, wearing a different filename.
 * 3. **Nothing from `node_modules`.** The script uses only Node built-ins. A
 *    consumer needing library support would have to declare and install it,
 *    and it does not.
 *
 * The assertions again differ from consumer A's. This one checks the *bridge*:
 * that JSON in and JSON out survive the FFI crossing, that key order is
 * preserved, and that a malformed request is refused with a message naming the
 * problem rather than a panic. That last one matters more than it sounds — a
 * panic on a napi boundary aborts the host process, so "the process is still
 * alive" is itself an assertion this consumer makes.
 *
 * Usage: run by the verifier inside a materialized scratch directory, with
 * `ARCHAVA_ADDON` naming the copied addon.
 */

// Node globals this script touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here — the same block `tools/rust/parity/runtime.mjs` carries
// for the same reason.
/* global process, URL */

import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'

// Defensive rather than lazy: the verifier sets this, and a run without it
// should fail with a sentence instead of resolving something unintended.
const addonPath = process.env.ARCHAVA_ADDON
if (!addonPath) throw new Error('ARCHAVA_ADDON must name the addon to load')
const require = createRequire(import.meta.url)
const native = require(addonPath)

const failures = []
const check = (name, run) => {
  try {
    const detail = run()
    process.stdout.write(`PASS ${name}${detail ? `: ${detail}` : ''}\n`)
  } catch (error) {
    failures.push(name)
    process.stdout.write(`FAIL ${name}: ${error.message}\n`)
  }
}

// The shipped pricebook, read at run time rather than compiled in — that
// difference is one of the four rows in the table above. Four levels up, the
// same depth consumer A uses for its own fixture, so a single layout serves
// both and neither can quietly drift onto a different file.
const pricebook = readFileSync(
  new URL('../../../../config/pricing.v1.json', import.meta.url),
  'utf8',
)
const request = JSON.stringify({
  region: 'GLOBAL',
  presence: 'chat',
  capability: 'assist',
  environment: 'existing_site',
})

/** Call the bridge, turning a thrown refusal into text so one path handles both. */
const quoteText = (pricebookText, requestText) => {
  try {
    return native.quoteJson(pricebookText, requestText)
  } catch (error) {
    return `REFUSED ${error.message}`
  }
}

check('the addon exposes the documented entry points', () => {
  const names = Object.keys(native).filter((key) => typeof native[key] === 'function')
  // Not a pinned list — that would break the moment a third slice shipped —
  // but the two this consumer depends on must be present.
  for (const name of ['quoteJson', 'ratesJson']) {
    if (!names.includes(name)) throw new Error(`missing export: ${name}`)
  }
  return names.join(', ')
})

check('a quote crosses the boundary with its 26 keys in order', () => {
  const text = quoteText(pricebook, request)
  if (text.startsWith('REFUSED ')) throw new Error(text.slice(9))
  const quote = JSON.parse(text)
  const keys = Object.keys(quote)
  if (keys.length !== 26)
    throw new Error(`expected 26 keys, found ${keys.length}: ${keys.join(',')}`)
  // Key order is one of the things the parity harness compares as text, so it
  // is worth asserting through the boundary too: a reordered object is a
  // different byte stream even with identical values.
  const head = keys.slice(0, 5).join(',')
  if (head !== 'region,currency,presence,capability,environment')
    throw new Error(`unexpected key order: ${head}`)
  if (quote.fixedCoreSetup !== 2000) throw new Error(`setup ${quote.fixedCoreSetup}`)
  if (quote.currency !== 'USD') throw new Error(`currency ${quote.currency}`)
  return `${keys.length} keys, ${quote.currency} ${quote.fixedCoreSetup}`
})

check('the same pricebook quoted twice is byte-identical', () => {
  const first = quoteText(pricebook, request)
  const second = quoteText(pricebook, request)
  if (first !== second) throw new Error('the bridge is not deterministic')
  return `${first.length} bytes, reproducible`
})

check('a malformed request is refused, not crashed', () => {
  const text = quoteText(pricebook, '{ not json')
  // The process reaching this line at all is the assertion's first half: a
  // panic across a napi boundary aborts the host, so survival is evidence.
  if (!text.startsWith('REFUSED ')) throw new Error(`expected a refusal, got ${text}`)
  if (!/could not deserialize/i.test(text))
    throw new Error(`refusal names nothing actionable: ${text}`)
  return text.slice(9, 79)
})

check('a request for an unknown presence is refused by name', () => {
  const text = quoteText(pricebook, JSON.stringify({ ...JSON.parse(request), presence: 'nope' }))
  if (!text.startsWith('REFUSED ') || !text.includes('nope'))
    throw new Error(`expected the offending value named, got ${text}`)
  return text.slice(9, 79)
})

check('the rates bridge prices a named night', () => {
  const text = native.ratesJson(
    JSON.stringify({ op: 'nightlyRate', unitId: 'garden-twin', night: '2026-10-05' }),
  )
  const rate = JSON.parse(text)
  // 850,000 IDR is the garden twin's base rate in the shipped reference table.
  // Asserted here independently of consumer A and of the parity evidence.
  if (rate.amountMinor !== 850000 || rate.currency !== 'IDR')
    throw new Error(`${rate.currency} ${rate.amountMinor}`)
  return `${rate.currency} ${rate.amountMinor}`
})

check('an op the bridge does not implement is refused rather than guessed', () => {
  let text
  try {
    text = native.ratesJson(JSON.stringify({ op: 'nightlyRates' }))
  } catch (error) {
    text = `REFUSED ${error.message}`
  }
  // A near-miss op name must not default to something plausible. Guessing here
  // would make every typo in a caller look like a rates answer.
  //
  // The refusal is asserted *as a refusal*, not merely as an echo. An earlier
  // shape of this check only looked for the string `nightlyRates` in the output
  // — which a successful answer carrying an echoed field would satisfy just as
  // well. That would let the check pass while the bridge quietly fell back to
  // the singular op, which is the exact defect it exists to catch.
  if (!text.startsWith('REFUSED '))
    throw new Error(`the near-miss op was answered, not refused: ${text}`)
  if (!/no such op/i.test(text)) throw new Error(`refused for some other reason: ${text}`)
  if (!text.includes('nightlyRates'))
    throw new Error(`the near-miss name is not echoed back: ${text}`)
  return text.slice(9, 79)
})

check('the bridge answer is self-consistent for the shipped pricebook', () => {
  const viaBridge = JSON.parse(quoteText(pricebook, request))
  if (viaBridge.region !== 'GLOBAL') throw new Error(`region ${viaBridge.region}`)
  if (viaBridge.presence !== 'chat') throw new Error(`presence ${viaBridge.presence}`)
  if (viaBridge.capability !== 'assist') throw new Error(`capability ${viaBridge.capability}`)
  return 'quote describes the request it was given'
})

if (failures.length > 0) {
  process.stdout.write(`\n${failures.length} check(s) failed: ${failures.join('; ')}\n`)
  process.exit(1)
}
process.stdout.write('\nConsumer B: all checks passed.\n')
