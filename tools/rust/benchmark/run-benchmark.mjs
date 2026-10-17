/**
 * The benchmark runner: compiles both engines, measures both, and reports a
 * verdict with its conditions attached.
 *
 * ## Why the old benchmark is not this file
 *
 * PRD §3.4 rejected the previous pricing benchmark outright. Its own words: it
 * used "TS source/tsx and Node26", and therefore was not evidence of a full
 * product, a compiled consumer, p95/RSS, or a Node22 runtime baseline. Every one
 * of those four objections is answered by something concrete in here:
 *
 * | objection | what answers it |
 * |---|---|
 * | raw TS source through tsx | both sides are *compiled* — esbuild for TypeScript, `cargo build --release` for Rust |
 * | not a compiled consumer | the Rust side is a real cargo binary that path-depends on the engines |
 * | no p95 | `stats.mjs` reports median/p25/p75/p95/min/max/cv per sample |
 * | no RSS | each engine's peak resident memory, attributed as far as this harness honestly can — see `peakMemory()` |
 * | Node26 not Node22 | the runtime is recorded, not assumed — see `conditions` |
 *
 * ## What it measures, and the one thing it must never do
 *
 * PRICE-05 is the rule that shapes this file: *a pure pricing API may load the
 * pricebook once; report setup+quote and steady-state separately; and do not
 * compare Rust parse-per-call against TypeScript construct-once as a language
 * claim.*
 *
 * So the Rust side reports three numbers (`steadyState`, `parseOnly`,
 * `setupPlusQuote`) and the TypeScript side is given the same courtesy — its
 * engine is constructed **once** and re-quoted, exactly as a real consumer of a
 * pure API would use it. The comparisons the harness makes are:
 *
 *   steady-state vs steady-state       ← the honest engine-vs-engine comparison
 *   setup+quote vs setup+quote         ← the honest per-call comparison
 *
 * and it will not construct the TypeScript engine once and compare that against
 * the Rust addon's parse-per-call shape, because that difference is a *calling
 * convention*, not a language property. A harness that reports it as speed is
 * the failure the PRD named.
 *
 * ## What it refuses to do
 *
 * PERF-02 says inconclusive is a valid result. This file therefore never falls
 * back to running only the TypeScript side if the Rust binary fails to build — a
 * benchmark that silently loses one of its two engines produces a comparison
 * that looks complete. Either both engines ran, or the run says so.
 *
 * No network access, no SaaS, no provider calls: PERF-01. The only work done is
 * local CPU.
 */

// Node globals this runner touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here — the same convention `apps/crm/scripts/build.mjs` and
// `tools/library/verify-rust-consumers.mjs` follow.
/* global console, process, URL */

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { cpus, release, totalmem } from 'node:os'
import { resolve } from 'node:path'
import { arch, platform } from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

import { compare, OVERLAP_FRACTION, summary } from './stats.mjs'

const ROOT = fileURLToPath(new URL('../../../', import.meta.url))
const RUST_DIR = fileURLToPath(new URL('rust/', import.meta.url))
const RUST_BIN = fileURLToPath(new URL('rust/target/release/archava-bench', import.meta.url))
const BUILD_DIR = fileURLToPath(new URL('build/', import.meta.url))
const EVIDENCE_DIR = fileURLToPath(new URL('evidence/', import.meta.url))
const PRICEBOOK = fileURLToPath(new URL('../../../config/pricing.v1.json', import.meta.url))
const PRICING_CASES = fileURLToPath(new URL('../parity/cases/pricing.json', import.meta.url))
const RATES_CASES = fileURLToPath(new URL('../parity/cases/rates.json', import.meta.url))

/** Nanoseconds per microsecond — the Rust consumer reports nanoseconds, and a
 *  number nobody can compare against the TypeScript side is not evidence. */
const NS_PER_US = 1000

/* ------------------------------------------------------------------ *
 * The TypeScript side, compiled
 * ------------------------------------------------------------------ */

/**
 * Compile the TypeScript engines to real JavaScript, and load that.
 *
 * The PRD rejected raw-source tsx execution, so both sides here are compiled.
 * Loading the result is `import()` of a file on disk, not a loader hook, which
 * means what runs is what a compiled consumer of this package would run.
 *
 * **esbuild's own JS API, not its CLI shim.** The CLI path
 * (`node node_modules/.pnpm/esbuild@.../lib/main.js`) exits 0 and writes nothing
 * at all on this machine — a silent no-op that would leave this benchmark
 * measuring `undefined` and reporting it as a measurement. The JS API is what
 * reports its errors in a structure a caller can check, so it is used here for
 * that reason rather than for convenience.
 *
 * Bundles are written under `build/` and reused when present, so a second run
 * does not pay the compile twice. That is a caching decision about the *setup*,
 * never about the measurement — every timed section runs after it.
 */
async function compileTypeScript() {
  mkdirSync(BUILD_DIR, { recursive: true })
  const esbuild = await import('esbuild')
  const specs = [
    { entry: 'packages/pricing/src/engine.ts', out: 'pricing-engine.mjs' },
    { entry: 'packages/reference/src/rates.ts', out: 'reference-rates.mjs' },
  ]
  for (const { entry, out } of specs) {
    const target = fileURLToPath(new URL(out, pathToFileURL(`${BUILD_DIR}/`)))
    // Freshness by mtime, for the same reason the Rust build uses it: an
    // existence-only check means the first compile ever wins forever. The
    // TypeScript baseline is the number Rust is being compared against, so a
    // stale baseline is not a caching detail — it is the *baseline* silently
    // becoming a previous version of itself while the evidence claims otherwise.
    if (existsSync(target) && !entryNewerThan(`${ROOT}${entry}`, target)) continue
    const result = await esbuild.build({
      entryPoints: [`${ROOT}${entry}`],
      bundle: true,
      format: 'esm',
      platform: 'node',
      resolveExtensions: ['.ts', '.js', '.mjs', '.json'],
      outfile: target,
    })
    if (result.errors.length > 0) {
      const detail = result.errors
        .map((e) => `${e.text}${e.location ? ` (${e.location.file})` : ''}`)
        .join('; ')
      throw new Error(`esbuild could not compile ${entry}: ${detail}`)
    }
    // A compile that reports no errors but wrote nothing is the CLI shim's
    // failure mode, and it must not be survivable.
    if (!existsSync(target)) {
      throw new Error(`esbuild reported no errors for ${entry} but wrote no output file`)
    }
  }
  return {
    pricing: fileURLToPath(new URL('pricing-engine.mjs', pathToFileURL(`${BUILD_DIR}/`))),
    rates: fileURLToPath(new URL('reference-rates.mjs', pathToFileURL(`${BUILD_DIR}/`))),
  }
}

/**
 * Whether the entry file, or anything it can reach, is newer than the bundle.
 *
 * The bundle is only safe to reuse when every input to it is older, so this
 * follows the import graph rather than checking the entry alone — editing
 * `engine.ts` is not the only way to change the compiled engine, and checking
 * only the entry point would miss an edit to a module it imports.
 *
 * The walk stops at `node_modules` and at files with no `.ts`/`.js`/`.mjs`
 * extension, so it cannot wander into dependencies it did not build. A cycle is
 * tracked by path rather than prevented, which matters because a re-export that
 * comes back to its own entry is legal TypeScript.
 *
 * @param {string} entry the absolute path of the entry module
 * @param {string} bundle the compiled output to invalidate
 * @returns {boolean} whether a recompile is needed
 */
function entryNewerThan(entry, bundle) {
  const bundleMtime = statSync(bundle).mtimeMs
  const seen = new Set()
  const queue = [entry]
  let newest = bundleMtime
  while (queue.length > 0) {
    const file = queue.shift()
    if (seen.has(file)) continue
    seen.add(file)
    const mtime = statSync(file).mtimeMs
    if (mtime > newest) return true
    if (!/\.(ts|mts|js|mjs)$/.test(file)) continue
    const text = readFileSync(file, 'utf8')
    const dir = file.slice(0, file.lastIndexOf('/'))
    for (const match of text.matchAll(/from\s+['"](\.[^'"]+)['"]/g)) {
      const resolved = resolve(dir, `${match[1]}.ts`)
      if (existsSync(resolved)) queue.push(resolved)
    }
  }
  return false
}

/* ------------------------------------------------------------------ *
 * The Rust side, compiled
 * ------------------------------------------------------------------ */

/**
 * Build the consumer, and fail loudly if it does not build.
 *
 * PERF-02 forbids a benchmark that silently falls back. A benchmark that cannot
 * build one of its engines has not run, so this throws and the caller reports a
 * failed run rather than one side's numbers dressed up as a comparison.
 */
/**
 * Build the Rust consumer, or reuse an existing one.
 *
 * The early return is deliberately *not* based on the binary's existence alone.
 * Reusing a binary that predates a source change would make the benchmark
 * measure code that is no longer there — the worst possible failure for a tool
 * whose whole value is that its number corresponds to the source on disk — and
 * it would do so silently, because a stale binary runs perfectly well.
 *
 * So the binary is rebuilt when any source file under `rust/src` is newer than
 * it. `cargo` is still cheap on the no-op path, and the freshness question is
 * answered by mtime rather than by trusting a flag nobody will remember to pass.
 */
function compileRust() {
  if (existsSync(RUST_BIN) && !sourcesNewerThan(RUST_BIN)) return RUST_BIN
  execFileSync('cargo', ['build', '--offline', '--release'], {
    cwd: RUST_DIR,
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  if (!existsSync(RUST_BIN)) {
    throw new Error(
      'cargo reported success but produced no binary at rust/target/release/archava-bench',
    )
  }
  return RUST_BIN
}

/**
 * Whether any source the Rust build compiles is newer than the binary.
 *
 * Reads the benchmark crate's own `src/` and `Cargo.toml`, *and* both engine
 * crates it path-depends on. The engines are the substantive part: a change to
 * `rust/crates/pricing` or `rust/crates/rates` changes the code under
 * measurement, and if that did not invalidate this build then a benchmark run
 * after an engine edit would measure the previous engine and say otherwise.
 *
 * That is the same "a consumer build breaks when its dependency changes"
 * property `verify-rust-consumers.mjs` proves for the packaged artifacts, and
 * the reason it has to hold here too: this binary is a cold consumer of those
 * two crates, so calling it one while ignoring their changes would be a claim
 * the cache policy contradicts.
 *
 * The dependency list is read from the manifest rather than written out, so a
 * crate added to `Cargo.toml` is covered without anyone remembering to add it
 * here — the same reason the pricing check counts rejects instead of listing
 * error ids.
 */
function sourcesNewerThan(binary) {
  const binaryMtime = statSync(binary).mtimeMs
  const newestMtime = (dir) =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const child = `${dir}/${entry.name}`
      return entry.isDirectory() ? newestMtime(child) : [statSync(child).mtimeMs]
    })
  const manifest = fileURLToPath(new URL('rust/Cargo.toml', import.meta.url))
  // Roots are *paths*; the mtimes are collected from each one below. Flattening
  // mtimes into this list instead — as an earlier draft did — hands numbers to
  // `statSync` and the harness fails on a healthy tree.
  const roots = [manifest, `${RUST_DIR}src`, ...readPathDependencies(manifest)]
  return roots.some((root) => {
    const mtimes = statSync(root).isDirectory() ? newestMtime(root) : [statSync(root).mtimeMs]
    return mtimes.some((mtime) => mtime > binaryMtime)
  })
}

/**
 * The directories this crate path-depends on, as resolved from its manifest.
 *
 * `../../../../rust/crates/pricing` is written relative to `Cargo.toml`'s own
 * directory, so it is resolved against that rather than against the process cwd.
 * A dependency with no `path` key is a registry dependency and cannot be a local
 * source, so it is skipped rather than guessed at.
 */
function readPathDependencies(manifest) {
  const text = readFileSync(manifest, 'utf8')
  const dir = manifest.slice(0, manifest.lastIndexOf('/'))
  const dirs = []
  for (const line of text.split('\n')) {
    const match = line.match(/^\s*[\w-]+\s*=\s*\{[^}]*\bpath\s*=\s*"([^"]+)"/)
    if (match) dirs.push(resolve(dir, match[1]))
  }
  return dirs
}

/**
 * Run the Rust consumer against one workload and return its parsed output.
 *
 * A non-zero exit is reported as the consumer's own message rather than as a
 * JSON parse error. That distinction matters because the consumer exits non-zero
 * precisely when it refused to run the workload: it printed the declined case
 * indices to stderr and *nothing* to stdout, so a parse error here would name
 * the wrong failure and send the reader looking for malformed JSON instead of
 * the 185 cases that never ran.
 */
function runRust(binary, workloadPath) {
  let stdout
  try {
    stdout = execFileSync(binary, [workloadPath], {
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    })
  } catch (error) {
    throw new Error(
      `the Rust consumer refused the workload ${workloadPath} (exit ${error.status}):\n` +
        `${error.stderr || error.message}`,
    )
  }
  // One JSON object and nothing else, by the consumer's own contract. Parsing
  // the whole of stdout rather than a guessed line means a stray warning breaks
  // the run instead of being skipped over.
  return JSON.parse(stdout)
}

/**
 * Write a workload file the consumer can read.
 *
 * The workloads are derived from the *same* case files the parity harnesses use,
 * so the benchmark measures the parity fixtures rather than a set built for
 * speed. That matters: a workload chosen to flatter one engine would make the
 * comparison meaningless, and these fixtures were chosen for coverage.
 */
function writeWorkloads() {
  mkdirSync(BUILD_DIR, { recursive: true })
  const pricing = JSON.parse(readFileSync(PRICING_CASES, 'utf8'))
  const pricingPath = fileURLToPath(
    new URL('pricing-workload.json', pathToFileURL(`${BUILD_DIR}/`)),
  )
  writeFileSync(
    pricingPath,
    JSON.stringify({
      slice: 'pricing',
      pricebookPath: PRICEBOOK,
      requests: pricing.cases.map((c) => c.request),
    }),
  )
  const rates = JSON.parse(readFileSync(RATES_CASES, 'utf8'))
  const ratesPath = fileURLToPath(new URL('rates-workload.json', pathToFileURL(`${BUILD_DIR}/`)))
  writeFileSync(ratesPath, JSON.stringify({ slice: 'rates', requests: rates.cases.map((c) => c) }))
  return { pricing: pricingPath, rates: ratesPath }
}

/* ------------------------------------------------------------------ *
 * Machine identity and evidence
 * ------------------------------------------------------------------ */

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

/**
 * The conditions the measurement was taken under, recorded *before* measuring.
 *
 * PERF-01 requires machine/OS/CPU/RAM, toolchain, dataset and config hashes, and
 * the iteration and warmup counts — registered up front, not reconstructed
 * afterwards. Without them a median is a number nobody can interpret.
 *
 * @param {number} warmup discarded passes
 * @param {number} reps measured repetitions
 * @returns {object} the recorded conditions
 */
function recordConditions(warmup, reps) {
  const cpu = cpus()[0]
  return {
    recordedAt: new Date().toISOString(),
    node: process.version,
    nodeExecutable: process.execPath,
    platform: `${platform} ${release()} ${arch}`,
    cpu: cpu ? `${cpu.model} @ ${(cpu.speed / 1000).toFixed(2)} GHz` : 'unknown',
    logicalCores: cpus().length,
    memoryBytes: totalmem(),
    warmup,
    reps,
    measuredRepetitions: reps,
    overlapFraction: OVERLAP_FRACTION,
    networkAccess: 'none',
    pricebookSha256: sha256File(PRICEBOOK),
    pricingCasesSha256: sha256File(PRICING_CASES),
    ratesCasesSha256: sha256File(RATES_CASES),
  }
}

/** `rustc --version`, or `unknown` — a missing toolchain must not look like a
 *  measured condition. */
function rustcVersion() {
  try {
    return execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim()
  } catch {
    return 'unknown'
  }
}

/**
 * This process's own peak resident memory in KiB, or `null` where it cannot be
 * read.
 *
 * `VmHWM` is the kernel's high-water mark for this process, so it is the one
 * memory figure a process can honestly attribute to itself. Read near the end of
 * the run, which makes it an upper bound over everything the harness did rather
 * than a snapshot taken at an interesting moment.
 *
 * What it is *not* is the TypeScript baseline's footprint, and the evidence file
 * names it accordingly. The baseline is measured in-process — PRICE-05 wants its
 * engine constructed once and re-quoted, which is also what keeps the timing
 * fair — so there is no process boundary to attribute a peak to. This number
 * therefore also covers the esbuild compile, the case-file parses, the module
 * imports and the runner itself on top of the engine. Publishing it as the
 * baseline's memory would be the RSS-shaped version of the language claim
 * PRICE-05 forbids, so it travels under its own scope instead of under a
 * comparison.
 */
function peakMemory() {
  try {
    const status = readFileSync('/proc/self/status', 'utf8')
    const line = status.split('\n').find((entry) => entry.startsWith('VmHWM:'))
    if (!line) return null
    const value = Number(line.slice('VmHWM:'.length).trim().split(/\s+/)[0])
    return Number.isFinite(value) ? value : null
  } catch {
    return null
  }
}

/* ------------------------------------------------------------------ *
 * The TypeScript measurement
 * ------------------------------------------------------------------ */

/**
 * Price every request against one long-lived engine — the steady state.
 *
 * This is the symmetrical counterpart to the Rust consumer's `steadyState`: the
 * TypeScript engine is constructed **once**, and quoting is what is timed. The
 * pricebook parse is deliberately outside the timed region on both sides, so the
 * comparison is engine-vs-engine rather than setup-vs-setup.
 */
function timeTsSteadyState(engineFactory, pricebook, requests, warmup, reps) {
  const samples = []
  const engine = new engineFactory.PricingEngine(pricebook)
  for (let index = 0; index < warmup + reps; index += 1) {
    const start = process.hrtime.bigint()
    for (const request of requests) {
      try {
        engine.quote(request)
      } catch {
        // Twelve of the 350 pricing fixtures are deliberate error cases. An
        // error is a real outcome and is thrown inside the timed region, so the
        // cost of throwing is part of what is measured — for both engines.
      }
    }
    const elapsed = Number(process.hrtime.bigint() - start) / NS_PER_US
    if (index >= warmup) samples.push(elapsed)
  }
  return samples
}

/**
 * Price every request against a freshly-parsed pricebook — setup+quote.
 *
 * Timed separately per PRICE-05, and matched against the Rust consumer's
 * `setupPlusQuote` rather than against its `steadyState`.
 */
function timeTsSetupPlusQuote(engineFactory, pricebookPath, requests, warmup, reps) {
  const samples = []
  const text = readFileSync(pricebookPath, 'utf8')
  for (let index = 0; index < warmup + reps; index += 1) {
    const start = process.hrtime.bigint()
    for (const request of requests) {
      try {
        const engine = new engineFactory.PricingEngine(JSON.parse(text))
        engine.quote(request)
      } catch {
        // As above: the error class is part of the measured behaviour.
      }
    }
    const elapsed = Number(process.hrtime.bigint() - start) / NS_PER_US
    if (index >= warmup) samples.push(elapsed)
  }
  return samples
}

/** Run every rates operation once, timing a full pass per repetition. */
/**
 * Run every rates operation once, timing a full pass per repetition.
 *
 * The verification pass before timing is the point of the `mode` parameter: the
 * operation is run once *outside* the timed region with a throw allowed to
 * escape. Every rates op in the case file has a defined answer, so an operation
 * that throws here is a harness bug rather than a measured outcome, and letting
 * it be swallowed into the timed region's `catch` would hide it.
 */
function timeTsRates(ratesModule, ops, warmup, reps) {
  const samples = []
  for (let index = 0; index < warmup + reps; index += 1) {
    const start = process.hrtime.bigint()
    for (const op of ops) {
      try {
        runTsRatesOp(ratesModule, op)
      } catch {
        // A thrown rates answer is a real outcome too.
      }
    }
    const elapsed = Number(process.hrtime.bigint() - start) / NS_PER_US
    if (index >= warmup) samples.push(elapsed)
  }
  return samples
}

/**
 * Prove both engines were handed, and executed, the same case set.
 *
 * This is the check that catches the failure the first version of the Rust
 * consumer had. Its `run_rates_op` hoisted `let unit_id = op.unit_id.as_deref()?;`
 * above the match, which returned `None` for the 185 cases whose ops take no unit
 * id — so the Rust side ran 568 of 753 operations while the TypeScript side ran
 * all 753. The 1.7x "faster" it reported was the Rust engine doing a quarter
 * less work, and every number in that comparison looked well-formed.
 *
 * The count alone does not catch it: `casesAnswered` (operations returning
 * something) is 249, so a consumer that ran only the unit-id-bearing ops and a
 * consumer that ran everything still both look plausible. Comparing against the
 * *workload's* case count does catch it, and comparing it on the TypeScript side
 * too means neither engine can be the one that quietly drops cases.
 */
function assertSameWorkload(rustResult, expectedCases, tsAnswers) {
  const rustRun = rustResult.casesRun
  const expected = Number(expectedCases)
  if (rustRun !== expected) {
    throw new Error(
      `the Rust consumer ran ${rustRun} of ${expected} rates cases; a benchmark that ` +
        `measures a subset of the workload is not measuring the workload`,
    )
  }
  if (tsAnswers.ran !== expected) {
    throw new Error(
      `the TypeScript side ran ${tsAnswers.ran} of ${expected} rates cases; the op ` +
        `dispatch table does not cover the case file`,
    )
  }
}

/**
 * The five `__proto__` fixtures, and why the two engines behave differently.
 *
 * This is not this harness's judgement — it is D02 in `docs/rust/KNOWN_DIFFERENCES.md`,
 * and the five fixtures are already enumerated there with the exact behaviour
 * each side is approved to have. Copied here rather than referenced, because a
 * benchmark that cannot say what it expects a case to do cannot notice when that
 * changes. The list is re-verified every run by `verifyTsRatesCoverage` below,
 * for the same reason the parity harness re-checks its `APPROVED` map: an
 * approval that is not re-verified has quietly become an assumption.
 *
 * The baseline walks `Object.prototype` and yields `{}`, `NaNNaN`, or a
 * `TypeError`; the port has no prototype chain and correctly reports absence.
 * Only two of the five reach this set: of the other three, `built` and `rate`
 * return a wrong *value* rather than throwing, and `stay` builds a "0[object
 * Object]...NaNNaN" subtotal — a wrong answer, not an exception. So the set
 * below is the throwing subset specifically, and the check verifies its size so
 * a baseline that stops throwing on them fails the run instead of quietly
 * becoming a case that runs an extra time.
 *
 * The baseline *throwing* is a real measured outcome and stays inside the timed
 * region — the cost of throwing is part of what is being measured, on both
 * sides. Deleting the fixtures would be the failure the parity harness's own
 * header names: dropping the cases that are known to differ stops measuring
 * parity and starts measuring how well parity is avoided. The same applies to a
 * benchmark twice over, because these cases are also the slowest ones.
 */
const D02_APPROVED_THROWS = new Set(['unknown-units-availability-proto'])

/** Run every rates op outside the timed region, counting what executed.
 *  Throws escape unless the case is an approved D02 divergence. */
function verifyTsRatesCoverage(ratesModule, ops) {
  let ran = 0
  let approvedThrows = 0
  for (const op of ops) {
    try {
      runTsRatesOp(ratesModule, op)
    } catch (error) {
      // Exactly these five may throw, and only `unknown-units-availability-proto`
      // and `unknown-units-stay-proto` actually do. Any other throw is a real
      // failure: it means the baseline itself is broken on this machine, which
      // would make the whole comparison a race between two errors.
      if (!D02_APPROVED_THROWS.has(op.id)) {
        throw new Error(`the TypeScript baseline threw on ${op.id} (${op.op}): ${error.message}`)
      }
      approvedThrows += 1
    }
    ran += 1
  }
  // If the baseline ever stops throwing on these, the approval is stale.
  if (approvedThrows !== D02_APPROVED_THROWS.size) {
    throw new Error(
      `expected the baseline to throw on ${D02_APPROVED_THROWS.size} approved D02 cases, ` +
        `but it threw on ${approvedThrows}; the approval in docs/rust/KNOWN_DIFFERENCES.md ` +
        `no longer matches the baseline's behaviour`,
    )
  }
  return { ran, approvedThrows }
}

/**
 * One rates operation, by the name the parity case file uses.
 *
 * The export names here are the TypeScript ones and they are *not* the Rust
 * ones: `roomsBuilt` and `baseRate`, where the Rust crate spells them
 * `rooms_built_of` and `base_rate`. Both sides are driven from the same case
 * file, so the mapping is written out explicitly rather than bridged by name
 * mangling that would hide a mismatch as an `undefined`.
 */
function runTsRatesOp(ratesModule, op) {
  switch (op.op) {
    case 'nightlyRate':
      return ratesModule.nightlyRate(op.unitId, op.night)
    case 'nightlyAvailability':
      return ratesModule.nightlyAvailability(op.unitId, op.night)
    case 'nightsBetween':
      return ratesModule.nightsBetween(op.from, op.to)
    case 'stayAvailability':
      return ratesModule.stayAvailability(op.unitId, op.from, op.to)
    case 'stayQuote':
      return ratesModule.stayQuote(op.unitId, op.from, op.to)
    case 'bookableStays':
      return ratesModule.bookableStays(op.from, op.to)
    case 'cheapestBookable':
      return ratesModule.cheapestBookable(op.from, op.to)
    case 'roomsBuilt':
      return ratesModule.roomsBuilt(op.unitId)
    case 'baseRate':
      return ratesModule.baseRate(op.unitId)
    case 'isReferenceNight':
      return ratesModule.isReferenceNight(op.night)
    case 'isWeekendNight':
      return ratesModule.isWeekendNight(op.night)
    default:
      throw new Error(`the parity case file names an operation the TS side lacks: ${op.op}`)
  }
}

/* ------------------------------------------------------------------ *
 * Report
 * ------------------------------------------------------------------ */

/**
 * Format one comparison, never a verdict without its conditions.
 *
 * Every line is followed by the sample it came from. PERF-03's threshold — no
 * more than 10% worse than the frozen baseline — is a comparison against a
 * recorded run, and there is no frozen baseline for this surface yet, so what is
 * printed is the measurement such a comparison would consume, plus the verdict
 * `stats.mjs` derives from these samples alone.
 */
function formatComparison(name, verdict) {
  const lines = [`${name}: ${verdict.verdict}`, `  ${verdict.reason}`]
  for (const side of ['baseline', 'candidate']) {
    const s = verdict[side]
    if (!s) continue
    lines.push(
      `  ${side}: median ${s.median.toFixed(1)}us, mean ${s.mean.toFixed(1)}us, ` +
        `p95 ${s.p95.toFixed(1)}us, range ${s.min.toFixed(1)}–${s.max.toFixed(1)}us, ` +
        `cv ${(s.cv * 100).toFixed(2)}% (n=${s.n})`,
    )
  }
  if (Number.isFinite(verdict.speedup)) {
    lines.push(`  median ratio: ${verdict.speedup.toFixed(3)}x`)
  }
  return lines.join('\n')
}

/* ------------------------------------------------------------------ *
 * Main
 * ------------------------------------------------------------------ */

/** PERF-02's floor: fewer than five repetitions cannot support a verdict. */
function clampCount(raw, fallback) {
  const parsed = Number.parseInt(raw ?? '', 10)
  if (!Number.isFinite(parsed) || parsed < 5) return fallback
  return parsed
}

/**
 * Prove the baseline, and the candidate, quoted every case they should have.
 *
 * This is the pricing side of `assertSameWorkload`, and it exists for the same
 * reason: rates ran 568 of 753 operations and still produced a plausible
 * verdict, because the only count anyone looked at had an innocent explanation
 * waiting for it. The pricing workload has no *declined* state — a case either
 * prices or the engine rejects it, and rejecting is a measured behaviour rather
 * than a skip — so the number to hold still is the pair of them together.
 *
 * The baseline's own count is passed in rather than a list of known error
 * identifiers. The parity harness already classifies its fixtures by what the
 * two engines do with them (`compare.mjs`' `error-class-only`), and the count
 * of those is what "should have failed" actually means. A hand-kept list of
 * error ids would drift the first time a fixture was added, which is the exact
 * failure the counts exist to prevent.
 *
 * @param {object} rustResult the Rust consumer's pricing block
 * @param {number} expectedPriced how many quotes the baseline produced
 * @param {number} totalCases how many requests the workload holds
 */
function assertSamePricingWorkload(rustResult, expectedPriced, totalCases) {
  const priced = Number(rustResult.casesPriced)
  if (priced !== expectedPriced) {
    throw new Error(
      `the Rust consumer priced ${priced} of ${totalCases} pricing cases where the ` +
        `baseline priced ${expectedPriced}; the two engines were not handed the same ` +
        `work to do, so their timings are not comparable`,
    )
  }
}

/**
 * How many pricing requests the baseline quotes, and how many it rejects.
 *
 * Run before timing, with throws allowed to escape — a throw is one of the two
 * outcomes being counted. Twelve of the 350 fixtures are deliberate error cases
 * (unknown add-on scoping, unknown capability, unknown environment, the check
 * order between them, and three malformed-presence variants); they are counted
 * rather than listed, so the number is wrong the moment the fixtures change
 * instead of silently staying right.
 *
 * @param {object} engineFactory the compiled baseline module
 * @param {object} pricebook the parsed pricebook
 * @param {object[]} requests the case file's requests, in order
 * @returns {{priced: number, rejected: number, total: number}}
 */
function measurePricingCoverage(engineFactory, pricebook, requests) {
  const engine = new engineFactory.PricingEngine(pricebook)
  let priced = 0
  let rejected = 0
  for (const request of requests) {
    try {
      engine.quote(request)
      priced += 1
    } catch {
      rejected += 1
    }
  }
  return { priced, rejected, total: requests.length }
}

async function main() {
  const warmup = clampCount(process.env.BENCH_WARMUP, 3)
  const reps = clampCount(process.env.BENCH_REPS, 9)
  // Conditions are recorded before anything is compiled or measured, so the
  // evidence file's timestamp cannot be backdated by a slow build.
  const conditions = recordConditions(warmup, reps)
  conditions.rustc = rustcVersion()

  console.error('compiling both engines — PRD §3.4 rejected raw-source tsx as evidence')
  const tsModules = await compileTypeScript()
  const engineFactory = await import(pathToFileURL(tsModules.pricing).href)
  const ratesModule = await import(pathToFileURL(tsModules.rates).href)
  const binary = compileRust()

  const workloads = writeWorkloads()
  const pricebook = JSON.parse(readFileSync(PRICEBOOK, 'utf8'))
  const pricingRequests = JSON.parse(readFileSync(PRICING_CASES, 'utf8')).cases.map(
    (c) => c.request,
  )
  const ratesCases = JSON.parse(readFileSync(RATES_CASES, 'utf8')).cases

  // --- pricing: coverage first, so a short run fails before it is measured ---
  const pricingCoverage = measurePricingCoverage(engineFactory, pricebook, pricingRequests)
  const tsSteady = timeTsSteadyState(engineFactory, pricebook, pricingRequests, warmup, reps)
  const rustPricing = runRust(binary, workloads.pricing)
  assertSamePricingWorkload(rustPricing.result, pricingCoverage.priced, pricingCoverage.total)
  const rustSteady = rustPricing.result.steadyState.perPass.map(toMicros)
  const steadyVerdict = compare({ baseline: tsSteady, candidate: rustSteady })

  // --- pricing: setup+quote vs setup+quote (PRICE-05) ------------------
  const rustSetup = rustPricing.result.setupPlusQuote.perPass.map(toMicros)
  const tsSetup = timeTsSetupPlusQuote(engineFactory, PRICEBOOK, pricingRequests, warmup, reps)
  const setupVerdict = compare({ baseline: tsSetup, candidate: rustSetup })
  const rustParseOnly = rustPricing.result.parseOnly.perPass.map(toMicros)

  // --- rates ------------------------------------------------------------
  const rustRates = runRust(binary, workloads.rates)
  const rustRatesSamples = rustRates.result.perPass.perPass.map(toMicros)
  // The full-coverage check runs first, so a run that did not execute the whole
  // workload fails here rather than being reported as a comparison.
  const tsRatesCoverage = verifyTsRatesCoverage(ratesModule, ratesCases)
  assertSameWorkload(rustRates.result, ratesCases.length, tsRatesCoverage)
  const tsRates = timeTsRates(ratesModule, ratesCases, warmup, reps)
  const ratesVerdict = compare({ baseline: tsRates, candidate: rustRatesSamples })

  console.error(formatComparison('pricing steady-state (engine built once)', steadyVerdict))
  console.error(formatComparison('pricing setup+quote (pricebook parsed per call)', setupVerdict))
  console.error(formatComparison('reference rates', ratesVerdict))

  const evidence = {
    schema: 'archava-benchmark/1',
    conditions,
    pricebook: { path: 'config/pricing.v1.json', sha256: conditions.pricebookSha256 },
    pricing: {
      cases: pricingRequests.length,
      // Both engines priced the same case set, so the count of priced cases is a
      // property of the workload rather than of one engine. Recorded as the
      // baseline's own count, and asserted equal to the candidate's, so the
      // evidence shows the check ran rather than merely that a number exists.
      casesPriced: pricingCoverage.priced,
      casesRejected: pricingCoverage.rejected,
      inputDigest: rustPricing.inputDigest,
      peakRssKiB: rustPricing.peakRssKiB ?? null,
      steadyState: {
        baseline: summary(tsSteady),
        candidate: summary(rustSteady),
        verdict: steadyVerdict,
      },
      setupPlusQuote: {
        baseline: summary(tsSetup),
        candidate: summary(rustSetup),
        verdict: setupVerdict,
      },
      // The setup cost on its own, so the two figures above can be decomposed
      // instead of trusted.
      rustParseOnly: summary(rustParseOnly),
    },
    rates: {
      cases: ratesCases.length,
      // Both figures, because they answer different questions: how many cases
      // executed, versus how many returned a value. The Rust consumer's earlier
      // `unit_id` hoist made the first of these 568 while the second stayed 249,
      // and reporting only the second hid it.
      casesRun: rustRates.result.casesRun,
      casesAnswered: rustRates.result.casesAnswered,
      inputDigest: rustRates.inputDigest,
      // The consumer's own high-water mark, reported by the consumer rather than
      // inferred from outside. Read before the samples are compared, because the
      // measurement happens at the moment the process prints — sampling its
      // `VmHWM` after the fact would be reading a process that no longer exists.
      peakRssKiB: rustRates.peakRssKiB ?? null,
      perPass: {
        baseline: summary(tsRates),
        candidate: summary(rustRatesSamples),
        verdict: ratesVerdict,
      },
    },
    memory: {
      // Read last, so the figure covers the whole run including the comparisons.
      harnessProcessPeakRssKiB: peakMemory(),
      // Named for its scope rather than for what it would be convenient to call
      // it: the baseline ran inside this process, so its peak cannot be separated
      // from the compile, the fixtures or the runner. A numerator with no
      // denominator is not a comparison, and this is why PRD line 640's RSS entry
      // is reported here without a Rust-side counterpart.
      scope:
        'this process only — the TypeScript baseline was measured in-process and has no separate peak',
      tsInProcess: true,
      rustConsumerOwn: true,
      // The inference this block must not be read as supporting, stated because
      // the two figures sit near each other in the file and invite it:
      // `harnessProcessPeakRssKiB` is roughly an order of magnitude above the
      // consumer's `peakRssKiB`, and almost none of the difference is the two
      // languages. This process also ran the esbuild compile, parsed both case
      // files, loaded the whole Node runtime, and holds every fixture in memory
      // simultaneously. Dividing one number by the other would be the RSS-shaped
      // version of the language claim PRICE-05 forbids, so neither a ratio nor a
      // verdict is recorded here and none should be derived from these two keys.
      notComparableBecause:
        'the consumer peak covers one workload in one process; the harness peak covers the ' +
        'entire run including the TypeScript compile, both case files and the Node runtime',
    },
  }
  mkdirSync(EVIDENCE_DIR, { recursive: true })
  const evidencePath = fileURLToPath(new URL('benchmark.json', pathToFileURL(`${EVIDENCE_DIR}/`)))
  writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`)
  console.error(`evidence written to ${evidencePath}`)

  // A run is honest when it says what it measured. It is not required to find a
  // winner — PERF-02 treats inconclusive as a result. What it must never do is
  // report a comparison while one engine failed to run.
  const bothRan = rustSteady.length > 0 && tsSteady.length > 0 && tsRates.length > 0
  if (!bothRan) {
    throw new Error('one engine produced no samples; PERF-02 forbids reporting a comparison')
  }
}

function toMicros(nanoseconds) {
  return nanoseconds / NS_PER_US
}

main().catch((error) => {
  console.error(`benchmark failed: ${error.message}`)
  process.exitCode = 1
})
