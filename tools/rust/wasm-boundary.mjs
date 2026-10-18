/* global process, URL */
/**
 * The wasm portability boundary — one step of the fast gate, and the producer
 * of evidence `P-WASM-PORTABLE`.
 *
 * ## What the boundary is, and why it is a gate step now
 *
 * The workspace ships one structural claim: every non-host crate compiles for
 * `wasm32-unknown-unknown`, and the host crate cannot. That claim existed as a
 * by-hand row in the EXECUTION_STATE gate table — measured once, recorded, and
 * then never guarded again, which is the same defect the other six Rust proofs
 * had before `pnpm rust:verify` existed. This step runs both directions on
 * every invocation and fails when either moves:
 *
 * 1. `portable crates compile for wasm32` — `cargo check` over the seven
 *    portable crates for the wasm target. A regression here is the browser
 *    side of the split breaking.
 * 2. `host crate refused by wasm target (structural boundary)` — the same
 *    cargo check against `archava-host` must FAIL. A host crate that started
 *    compiling for wasm would mean sqlx/tokio/axum portability work had landed
 *    silently, moving the boundary without a decision. Here a non-zero exit
 *    (cargo's 101) is the PASS, and an exit 0 is the failure — recorded in
 *    those words so a reader of the evidence never has to guess which way the
 *    assertion runs.
 * 3. `no secret-shaped strings in portable sources (wasm surface)` — the
 *    portable sources are scanned for machine-bound and secret-shaped literals:
 *    connection URIs, absolute machine paths, env-var reads, key material. The
 *    wasm compile already proves host std I/O cannot type-check here; this scan
 *    covers what compilation cannot see — a literal that would embed one
 *    machine's shape into every module built from these sources.
 *
 * ## Why this script writes its own evidence
 *
 * The other fast-gate proofs have one subprocess each, so `verify.mjs` writes
 * their evidence from the step exit. These three checks come from three
 * different subprocess results — and one of them is a *required failure* — so
 * the producer that knows the truth is this file. It writes
 * `dist/rust-evidence/P-WASM-PORTABLE.json` through the same producer module
 * (`evidence.mjs`) every other proof uses, with the same candidate identity
 * and source binding, and it writes it on failure too.
 */

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { writeEvidence } from './evidence.mjs'
import { loadManifest } from './coverage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = join(root, 'rust')

/**
 * The portable set, frozen as the seven crates the wasm-clean boundary covers.
 *
 * The workspace members NOT here, each a deliberate absence rather than an
 * omission:
 *
 * - `archava-host` (sqlx/tokio/axum) — native-only by design, and its absence
 *   is not an omission but the other half of the assertion: check 2 below
 *   requires it to be REFUSED by the wasm target.
 * - `archava-adapters` (sqlx/tokio) — the database cohort; RLS lives in
 *   PostgreSQL, which no wasm target can host.
 * - `archava-native` (napi) — the Node addon, native-only by design.
 * - `archava-pg-fixture` (sqlx + std::process driving initdb/pg_ctl) —
 *   RF02's DEV-only test fixture. It is the furthest possible from a wasm
 *   surface: it spawns local processes, and no production crate depends on it,
 *   so the crate graph already proves nothing portable can reach it.
 *
 * `archava-context` joined with the R09 slice: a portable leaf on
 * serde/serde_json/archava-core only — the turn-context surfaces a browser
 * embed builds its visitor turn from, with the store in memory and no host
 * I/O of any kind.
 */
const PORTABLE_CRATES = [
  'archava-core',
  'archava-domain',
  'archava-pricing',
  'archava-rates',
  'archava-assembly',
  'archava-capacity',
  'archava-context',
]

const HOST_CRATE = 'archava-host'

const CHECK_PORTABLE = 'portable crates compile for wasm32'
const CHECK_HOST_REFUSAL = 'host crate refused by wasm target (structural boundary)'
const CHECK_SECRETS = 'no secret-shaped strings in portable sources (wasm surface)'

/**
 * Secret-shaped literals the portable sources must not contain.
 *
 * Chosen to be high-signal for this repository specifically: connection URIs
 * and machine paths are how the owner database's shape could leak into shipped
 * code, and env-var reads are how a build could become machine-dependent. The
 * wasm target compile is what proves no host API type-checks; these patterns
 * are what prove no *literal* smuggles the same dependency past the compiler.
 */
const SECRET_PATTERNS = [
  { name: 'postgres connection URI', pattern: /postgres(?:ql)?:\/\//i },
  { name: 'absolute machine home path', pattern: /\/home\/[A-Za-z0-9._-]+\// },
  { name: 'machine temp path', pattern: /\/tmp\/[A-Za-z0-9._-]+/ },
  { name: 'env-var read', pattern: /std::env::var|process\.env/ },
  { name: 'private key material', pattern: /BEGIN [A-Z ]*PRIVATE KEY/ },
]

/** Every .rs and .toml file under the portable crate directories. */
function portableFiles() {
  const files = []
  for (const crate of PORTABLE_CRATES) {
    const dir = join(workspace, 'crates', crate.replace('archava-', ''))
    const walk = (path) => {
      for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) =>
        a.name < b.name ? -1 : 1,
      )) {
        const absolute = join(path, entry.name)
        if (entry.isDirectory()) walk(absolute)
        else if (/\.(rs|toml)$/.test(entry.name)) files.push(relative(root, absolute))
      }
    }
    walk(dir)
  }
  return files.sort()
}

/**
 * Run one cargo check, recording the exit code rather than throwing on it.
 *
 * Every invocation of this function has an expected direction (pass or
 * refusal), and both directions are evidence. `maxBuffer` is raised because a
 * first wasm build prints its full dependency compile log.
 */
function cargoCheck(args) {
  const started = Date.now()
  try {
    execFileSync('cargo', args, {
      cwd: workspace,
      stdio: 'pipe',
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    })
    return { exitCode: 0, seconds: Number(((Date.now() - started) / 1000).toFixed(1)) }
  } catch (error) {
    return {
      exitCode: error.status ?? null,
      seconds: Number(((Date.now() - started) / 1000).toFixed(1)),
    }
  }
}

const wasmTarget = ['--target', 'wasm32-unknown-unknown', '--locked', '--offline']

// Check 1: the portable set must compile for wasm.
process.stdout.write('  $ cargo check (portable crates, wasm32-unknown-unknown)\n')
const portable = cargoCheck([
  'check',
  ...PORTABLE_CRATES.flatMap((name) => ['-p', name]),
  ...wasmTarget,
])
const portablePass = portable.exitCode === 0
process.stdout.write(
  `     ${portablePass ? '✓' : '✗'} portable crates ${portablePass ? 'compile' : `REFUSED (exit ${portable.exitCode})`} (${portable.seconds}s)\n`,
)

// Check 2: the host crate must be REFUSED by the wasm target. Exit 0 here is
// the failure: it would mean the browser/server boundary moved silently.
process.stdout.write('  $ cargo check (host crate, wasm32-unknown-unknown — must fail)\n')
const host = cargoCheck(['check', '-p', HOST_CRATE, ...wasmTarget])
const hostRefused = host.exitCode !== 0
process.stdout.write(
  `     ${hostRefused ? '✓' : '✗'} host crate ${hostRefused ? `refused (exit ${host.exitCode})` : 'COMPILED FOR WASM — the boundary moved'} (${host.seconds}s)\n`,
)

// Check 3: scan the portable sources for secret-shaped literals.
const files = portableFiles()
const findings = []
for (const file of files) {
  const text = readFileSync(join(root, file), 'utf8')
  for (const { name, pattern } of SECRET_PATTERNS) {
    const line = text.split('\n').findIndex((source) => pattern.test(source))
    if (line !== -1) findings.push(`${file}:${line + 1} matches ${name}`)
  }
}
const secretsPass = findings.length === 0
process.stdout.write(
  `     ${secretsPass ? '✓' : '✗'} ${files.length} portable source files scanned — ` +
    `${secretsPass ? 'no secret-shaped literals' : `${findings.length} finding(s)`}\n`,
)
for (const finding of findings) process.stdout.write(`       ${finding}\n`)

// Write the evidence through the shared producer module, pass or fail.
const spec = loadManifest(root).proofs['P-WASM-PORTABLE']
const evidence = writeEvidence(root, {
  proof: 'P-WASM-PORTABLE',
  checks: {
    [CHECK_PORTABLE]: portablePass
      ? 'pass'
      : `fail: portable crates refused by wasm target (cargo check exit ${portable.exitCode})`,
    [CHECK_HOST_REFUSAL]: hostRefused
      ? 'pass'
      : 'fail: host crate compiled for wasm32 — the structural boundary moved and needs a decision',
    [CHECK_SECRETS]: secretsPass ? 'pass' : `fail: ${findings.slice(0, 5).join('; ')}`,
  },
  details: {
    portableCrates: PORTABLE_CRATES,
    portableExitCode: portable.exitCode,
    portableSeconds: portable.seconds,
    hostRefusalExitCode: host.exitCode,
    hostRefusalSeconds: host.seconds,
    scannedFiles: files.length,
    findings,
  },
  sourcePaths: spec.sourcePaths,
  artifacts: spec.artifacts ?? [],
  referenced: spec.referenced ?? [],
  producer: 'node tools/rust/wasm-boundary.mjs',
})
process.stdout.write(`  evidence → dist/rust-evidence/P-WASM-PORTABLE.json (${evidence.result})\n`)

if (evidence.result !== 'pass') {
  process.stdout.write('FAIL wasm portable boundary — see the checks above\n')
  process.exitCode = 1
} else {
  process.stdout.write('PASS wasm portable boundary — both directions held\n')
}
