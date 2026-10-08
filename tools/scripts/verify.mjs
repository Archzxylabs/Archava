#!/usr/bin/env node
/* global process */
/**
 * One command that answers "is this repo green?" (V1.1 §14).
 *
 * Every gate it runs is a script that already exists in `package.json`; this
 * file adds no checks of its own. That is deliberate: a verify script that
 * quietly re-implements a gate drifts from the one a developer runs by hand, and
 * the first thing to stop being true would be the claim that they are the same
 * run.
 *
 * Gates run in the order a change would break them: install, then structure,
 * then lint, then the ones that execute code. Install is special-cased to stop
 * the run early, because a failed install makes every later failure a symptom
 * rather than a finding.
 *
 * The list is the same list, in the same order, as `.github/workflows/ci.yml`,
 * so "CI passed" and "`pnpm verify` passed" are one fact. Structure is a gate in
 * this table rather than a prefix on the `verify` script precisely because of
 * that: a check that runs outside the table is a check a laptop and CI can
 * quietly disagree about.
 *
 * Usage: `pnpm verify` (or `node tools/scripts/verify.mjs`).
 * Exit code 0 means every gate passed; non-zero means at least one did not, and
 * the failing one is named in the summary.
 */

import { spawnSync } from 'node:child_process'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))))

/** The pnpm executable by platform — Windows needs the `.cmd` shim resolved. */
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'

/** Every gate, in run order. `stopOnFailure` halts the rest of the run. */
const GATES = [
  { name: 'install', args: ['install', '--frozen-lockfile'], stopOnFailure: true },
  { name: 'structure', args: ['structure'] },
  { name: 'format', args: ['format:check'] },
  { name: 'lint', args: ['lint'] },
  { name: 'typecheck', args: ['typecheck'] },
  { name: 'test', args: ['test'] },
  { name: 'build', args: ['build'] },
  { name: 'reuse', args: ['reuse:verify'] },
]

/** Actions used for the console, guarded so a piped run never crashes. */
const paint = {
  green: (text) => (process.stdout.isTTY ? `[32m${text}[0m` : text),
  red: (text) => (process.stdout.isTTY ? `[31m${text}[0m` : text),
  dim: (text) => (process.stdout.isTTY ? `[2m${text}[0m` : text),
}

const results = []

for (const gate of GATES) {
  process.stdout.write(`\n${paint.dim('──')} ${gate.name}\n`)
  const started = Date.now()
  const run = spawnSync(pnpm, gate.args, { cwd: repoRoot, stdio: 'inherit' })
  const seconds = ((Date.now() - started) / 1000).toFixed(1)
  const ok = run.status === 0
  results.push({ name: gate.name, ok, seconds })

  // A signal (SIGTERM, SIGKILL) reports as a null status, which is not a test
  // failure but is not a pass either — say so rather than calling it green.
  if (run.error) {
    process.stdout.write(`${paint.red('✗')} ${gate.name}: ${run.error.message}\n`)
  } else if (!ok) {
    process.stdout.write(`${paint.red('✗')} ${gate.name} failed\n`)
  }
  if (!ok && gate.stopOnFailure) break
}

process.stdout.write('\n')
for (const result of results) {
  const mark = result.ok ? paint.green('✓') : paint.red('✗')
  process.stdout.write(`${mark} ${result.name.padEnd(11)} ${result.seconds}s\n`)
}

const failed = results.filter((result) => !result.ok)
if (failed.length > 0) {
  const names = failed.map((result) => result.name).join(', ')
  process.stdout.write(`\n${paint.red(`${failed.length} gate(s) failed: ${names}`)}\n`)
  process.exitCode = 1
} else {
  process.stdout.write(`\nAll ${results.length} gates passed.\n`)
}
