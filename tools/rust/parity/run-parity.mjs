/**
 * The P1 parity harness: run every fixture through the TypeScript engine and
 * the Rust engine, then compare the results.
 *
 * This file deliberately lives outside the product tree. `vitest.config.ts`
 * globs every `test` directory under packages, apps and tools, so any
 * `*.test.ts` here would be picked up by the product's own test run; a plain
 * `.mjs` entry point run by hand is not. Nothing under `experiments/` is
 * reachable from a product glob, which is the property that keeps this pilot
 * out of the build.
 *
 * (The glob above is spelled out in words rather than written literally,
 * because the literal form contains the two characters that would close this
 * very comment block early.)
 *
 * ## What is being compared, and how
 *
 * Queries, not prose: both engines' output is canonicalized by
 * `canonical.mjs` into a single text form, then compared for equality. That
 * comparison is deliberately *exact* — no tolerance, no rounding. The only
 * thing normalized away is how each language prints the same `f64`
 * (serde emits `2000.0`, JS emits `2000`). Key order is preserved and checked.
 *
 * The alternative — comparing parsed objects with a deep-equal — would throw
 * away the key order, and the order is one of the things being verified:
 * `engine.ts`'s output literal spells 26 keys in a fixed order and the port
 * must match it exactly.
 *
 * ## Why assertions come before timing
 *
 * Every fixture is checked here first, before anything is measured. That
 * ordering is why the file holds no timing code of its own: a fixture that
 * fails parity is never one anyone measures.
 *
 * ## Both engines are required
 *
 * The Rust side is never optional in a parity run. A missing addon is a
 * failure, not a "TS only" run, because the pilot's question is whether the
 * two agree — and `runtime.mjs`'s `requireRust()` throws rather than falling
 * back to TypeScript, precisely so a broken addon can never be silently
 * reported as a passing TS result.
 */

import { existsSync } from 'node:fs'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { requireRust } from './runtime.mjs'
import { canonicalize, canonicalizeValue, sha256 } from './canonical.mjs'
// The comparator lives in its own module so `compare.test.mjs` can exercise it.
// Importing it from here is what makes the harness's verdict and the tested
// comparator the same code path.
import { compare } from './compare.mjs'

// Node globals this harness touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

/** The one pricebook both engines read. */
const FIXTURES = fileURLToPath(new URL('../../../config/pricing.v1.json', import.meta.url))
/** The case file both engines are run against. */
const CASES = fileURLToPath(new URL('./cases/pricing.json', import.meta.url))
/**
 * Where the p1 evidence lands, written by this run rather than maintained
 * beside it. `evidence/` sits next to `cases/`, the same convention as p2.
 */
const EVIDENCE = fileURLToPath(new URL('./evidence/p1-parity.json', import.meta.url))

/**
 * Load the TypeScript engine straight from source.
 *
 * `@archava/pricing` and `@archava/config` ship TypeScript with no build step,
 * and the workspace links are absent from this experiment's node_modules on
 * purpose — the pilot adds no dependency to the product. `tsx` resolves the
 * real source paths, so what runs is exactly `packages/pricing/src/engine.ts`.
 *
 * ## Three things about reaching it, all learned the hard way
 *
 * **The depth is four, not three.** This file sits at `Experiments/rust-pilot/
 * node/`, so the product root is `node/` → `rust-pilot/` → `Experiments/` →
 * `Lab/` → `Archava_Platform`. The three-level form this experiment was
 * originally written with resolved at the old `Archava_Platform/experiments/
 * rust-pilot/node/` location, and from here it produces a plausible-looking path
 * under `Experiments/` — the worst kind of failure, because it reads like the
 * product is missing a file it still has.
 *
 * **`tsx` must be resolved by path and imported as ESM.** A plain
 * `import 'tsx/esm/api'` resolves by walking up from *this file*, and nothing
 * above `Experiments/` has a `node_modules` — `tsx` is the product's
 * devDependency, which is exactly what keeps this experiment outside it.
 * `createRequire` with a `paths` array *finds* the package, but loading it
 * through the CJS loader picks its `require` condition and the entry resolves to
 * a `.cjs` re-import of the ESM file, which fails with `MODULE_NOT_FOUND` even
 * though the resolve succeeded. `require.resolve` does not apply that
 * condition, so it hands back the path an ESM `import` can actually load.
 *
 * **The parent URL is a `file://` inside the product, not this file.** `tsImport`
 * needs a `parentURL` to resolve the engine's *own* imports from, and the
 * engine's dependency `@archava/config` is only findable from inside the
 * product tree. Passing this file's URL resolves the engine's imports against
 * wherever the harness happens to live and fails on the product's `.js`-suffixed
 * TypeScript specifiers.
 *
 * The root is resolved from this file's own location rather than hardcoded, for
 * the same reason `crates/pricing` had to stop using `../../../fixtures/`: a
 * harness that names an absolute pilot path is a harness that cannot be moved.
 * This file originally read `../../../Archava_Platform/` — three levels up from
 * `Lab/Experiments/rust-pilot/node/`. Promoted into `tools/rust/parity/`, three
 * levels up is the product root, so the arithmetic happens to still work, but it
 * now works because the depth is *checked* rather than assumed.
 *
 * @returns {Promise<typeof import("../../../../packages/pricing/src/engine.ts")>}
 */
async function loadTypeScriptEngine() {
  const workspaceRoot = fileURLToPath(new URL('../../../', import.meta.url))

  // The relative climb above is arithmetic, and arithmetic is exactly what went
  // wrong for `archava-pricing`'s `include_str!("../../../fixtures/...")` (D10):
  // it resolved correctly from the pilot and broke on promotion. So rather than
  // trust the depth, assert the root is recognisably the product root before
  // handing it to tsx as the parent URL. A wrong root fails here with a readable
  // message instead of three imports later with a module-resolution error.
  const isProductRoot =
    existsSync(join(workspaceRoot, 'package.json')) &&
    existsSync(join(workspaceRoot, 'packages', 'pricing', 'package.json'))
  if (!isProductRoot) {
    throw new Error(
      `Parity harness resolved the workspace root to ${workspaceRoot}, which is not the ` +
        `product root (no package.json / packages/pricing). The harness must live exactly ` +
        `three levels below the product root — it is at tools/rust/parity/.`,
    )
  }

  const enginePath = fileURLToPath(
    new URL('packages/pricing/src/engine.ts', pathToFileURL(`${workspaceRoot}/`)),
  )

  const require = createRequire(import.meta.url)
  const tsxApiPath = require.resolve('tsx/esm/api', { paths: [workspaceRoot] })
  const { tsImport } = await import(pathToFileURL(tsxApiPath).href)

  // A `file://` URL one level under the product root: the source that resolves
  // the engine's own imports from must live inside the tree that has
  // `packages/config`, and the product's `packages/pricing/package.json` is
  // what lets `@archava/config` be found at all.
  return tsImport(enginePath, new URL(`file://${workspaceRoot}`).href)
}

/**
 * Run one fixture through the TypeScript engine.
 *
 * @param {object} PricingEngine the loaded class
 * @param {string} pricebookText the pricebook as JSON text
 * @param {object} raw the raw fixture case
 * @returns {{ok: boolean, quote?: unknown, error?: string}}
 */
function runTypeScript(PricingEngine, pricebookText, raw) {
  // The TS engine takes the parsed pricebook, so both engines are handed the
  // identical bytes and only the parsing differs.
  const engine = new PricingEngine(JSON.parse(pricebookText))
  try {
    return { ok: true, quote: engine.quote(raw.request) }
  } catch (error) {
    return {
      ok: false,
      error: `${error?.name ?? 'Error'}: ${error?.message ?? String(error)}`,
    }
  }
}

/**
 * Run one fixture through the Rust engine.
 *
 * @param {Record<string, Function>} addon the native addon
 * @param {string} pricebookText the pricebook as JSON text
 * @param {object} raw the raw fixture case
 * @returns {{ok: boolean, quote?: unknown, error?: string}}
 */
function runRust(addon, pricebookText, raw) {
  try {
    // Serialized, not passed as an object. The boundary is a string one — that
    // is the entire design decision recorded in `crates/native/src/lib.rs`,
    // where napi would otherwise have its own opinion about how a JS object
    // maps onto a Rust struct. Serializing here means the Rust engine reads
    // the same bytes the TypeScript engine is effectively fed, so the only
    // thing that can differ between the two is the computation.
    //
    // This costs one `JSON.stringify` per call and the benchmark measures that
    // cost honestly rather than hiding it.
    return {
      ok: true,
      quote: canonicalize(addon.quoteJson(pricebookText, JSON.stringify(raw.request))),
    }
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
  // The Rust side has already canonicalized (it arrived as text), so handing
  // its string through the writer once more would be a parse of text that is
  // already canonical. Both paths converge here.
  return typeof outcome.quote === 'string' ? outcome.quote : canonicalizeValue(outcome.quote)
}

/**
 * The parity run.
 *
 * Both engines are required; a missing addon aborts the run.
 */
async function main() {
  // The case file is `{slice, generatedBy, pricebook, count, cases}` — a
  // manifest rather than a bare array, so the digests recorded alongside the
  // run name which file and which generated set they came from.
  const { cases, count } = JSON.parse(readFileSync(CASES, 'utf8'))
  const pricebookText = readFileSync(FIXTURES, 'utf8')

  const engineModule = await loadTypeScriptEngine()
  // The addon parses the pricebook itself, and `runtime.mjs` fails loudly when
  // it is absent — so this line is the gate, not a fallback.
  const addon = requireRust()

  // Checked rather than trusted: `count` is the generator's own tally of what
  // it wrote, so a truncated or hand-edited case file is caught here instead
  // of reporting a confident verdict over a partial set.
  if (count !== cases.length) {
    throw new Error(
      `case file is inconsistent: it declares count ${count} but carries ${cases.length} cases`,
    )
  }

  const rows = []
  const diverge = []
  const errorClassOnly = []
  const errorMessageOnly = []
  for (const fixture of cases) {
    const tsText = serialize(runTypeScript(engineModule.PricingEngine, pricebookText, fixture))
    const rustText = serialize(runRust(addon, pricebookText, fixture))
    const verdict = compare(fixture.id, tsText, rustText)
    rows.push({ id: fixture.id, verdict, ts: tsText, rust: rustText })
    if (verdict.verdict === 'diverge') diverge.push(verdict.message)
    if (verdict.verdict === 'error-class-only') errorClassOnly.push(verdict.message)
    if (verdict.verdict === 'error-message-only') errorMessageOnly.push(verdict.message)
  }

  // Counted separately, because they are not the same claim. A "match" is the
  // two engines agreeing outright. `error-class-only` means they agree on the
  // message but not on the error's type — a real boundary divergence the pilot
  // reports rather than folds into the pass rate. Only `diverge` means the
  // pricing results themselves differ.
  const tally = {
    match: 0,
    'error-class-only': 0,
    'error-message-only': 0,
    diverge: 0,
  }
  for (const row of rows) tally[row.verdict.verdict] += 1

  // The manifest goes to stdout, never into the product tree. Its digests let
  // a rerun prove which data was measured.
  //
  // Note on `passed`/`failed` here versus in p2: p1 has no approved expectations,
  // so `passed` is `match` alone and every non-match is `failed`. p2 counts its
  // five approved `corrected-expectation` verdicts as passed *and* keeps them
  // visible in their own tally key. The two files therefore apply different
  // arithmetic to the same word, which is deliberate — p1 has nothing approved,
  // p2 does — but it means `"failed"` in one is not `"failed"` in the other.
  // Comparing the two files' pass rates without reading this paragraph would
  // read 12 units of unapproved difference as worse than 5 approved ones.
  const manifest = {
    slice: 'p1',
    // Both engines, always — not the value of `RUST_PILOT_RUNTIME`. See the
    // same field in `run-parity-rates.mjs` for why this was changed: the
    // variable defaults to `'ts'`, nothing selects an engine from it, and the
    // harness aborts rather than falling back, so recording it here claimed the
    // one thing this run exists to disprove.
    runtime: 'ts+rust',
    fixtures: cases.length,
    ...tally,
    passed: tally.match,
    failed: cases.length - tally.match,
    pricebookSha256: sha256(pricebookText),
    casesSha256: sha256(readFileSync(CASES, 'utf8')),
    // One digest over every row in order, so a rerun over the same data is
    // provably the same result.
    resultsSha256: sha256(rows.map((row) => `${row.id}:${row.verdict.verdict}`).join('\n')),
  }

  // Written, not just printed. The p1 harness used to report its manifest to
  // the terminal only, and `evidence/p1-parity.json` was left in the tree by
  // hand — so when the case set grew from 346 to 350 to cover absent tier
  // names, the file kept saying `"fixtures": 346` and reported a verdict about
  // a case set no longer on disk. An evidence file that disagrees with the
  // case file it claims to describe is worse than no evidence file, because it
  // is read as proof. Every field here is one the manifest already held.
  writeFileSync(EVIDENCE, `${JSON.stringify(manifest, null, 2)}\n`)

  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`)
  // Divergences are the failures worth reading; the error-class cases are a
  // known, documented boundary difference, so they are summarized but not
  // spammed once per fixture.
  if (diverge.length > 0) {
    process.stdout.write(`\n${diverge.slice(0, 10).join('\n\n')}\n`)
  }
  if (errorClassOnly.length > 0) {
    process.stdout.write(
      `\n${errorClassOnly.length} fixtures agree on the error message but not on the error type ` +
        `(TS throws PricingError, Rust raises napi Error) — see docs/RUST_PILOT_RESULTS.md.\n`,
    )
  }
  if (errorMessageOnly.length > 0) {
    process.stdout.write(`\n${errorMessageOnly.slice(0, 10).join('\n\n')}\n`)
  }
  // Only a real divergence fails the run. The error-class mismatch is
  // reported, recorded, and then judged in the writeup.
  if (diverge.length > 0 || errorMessageOnly.length > 0) {
    process.exitCode = 1
  }
}

await main()
