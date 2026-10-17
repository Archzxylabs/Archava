/* global process, URL */
/**
 * Candidate-bound proof evidence: write it, read it, recompute what it claims.
 *
 * ## Why this module exists (RF01 / RA06)
 *
 * The 2026-10-16 reaudit found the release gate treating the inventory JSON's
 * own cached fields — `exists: true`, a non-empty `sha256`, a hardcoded profile
 * — as proof that the referenced artifacts existed and were valid. Five rows of
 * self-declared booleans made `requiredInventoryComplete` true while no
 * producing file was ever opened. Two probes demonstrated it: identical
 * zero-hash flags flipped the verdict to `candidate-verified` with `dist/`
 * absent, and a single invented proof did the same.
 *
 * The fix has two halves. This module is the *producer* half: proof evidence
 * written by the gate that actually ran the proof, carrying (a) the real check
 * results, (b) the candidate identity at production time, and (c) a binding —
 * digests of the source paths the proof covers and the artifact bytes it read,
 * recomputable by anyone. `coverage.mjs` is the *validator* half: it re-opens
 * every referenced file, re-hashes everything, and refuses what does not match.
 * An `exists: true` field is never read as evidence again.
 *
 * ## The evidence schema (schemaVersion 1)
 *
 * ```json
 * {
 *   "schemaVersion": 1,
 *   "proof": "P-FAST-GATE",
 *   "result": "pass",                    // "pass" iff every check below is "pass"
 *   "checks": { "<name>": "pass" | "fail": <reason> },
 *   "details": { ... producer-specific tallies ... },
 *   "producedAt": "<ISO>", "producer": "<command or tool>",
 *   "identity": { head, branch, rustc, cargo, node, pnpm, releaseProfileVersion },
 *   "binding": {
 *     "sourceDigest": "sha256:...",      // over binding.sources
 *     "sources": [ { "path", "sha256" } ],
 *     "artifacts": [ { "path", "sha256" } ]   // bytes that existed at write time
 *   },
 *   "referenced": [ { "path", "sha256" } ]    // producing files read (parity evidence, …)
 * }
 * ```
 *
 * `result` is derived, never asserted: `writeEvidence` computes it from the
 * checks map. A producer cannot claim `pass` while carrying a failing check.
 * `checks` values are `pass` or `fail: <reason>`; a check that never ran has no
 * entry, and `coverage.mjs` treats a *missing required check* exactly like a
 * failed one — the silent-gap failure mode the old gate had by construction.
 *
 * ## What the binding protects against
 *
 * - **Stale**: sources changed after the proof ran → recomputed sourceDigest
 *   differs → the proof is invalidated for *its own* covered paths only (a
 *   proof covers what its manifest entry declares), so unrelated work is not
 *   re-measured without reason.
 * - **Wrong artifact**: evidence records the sha256 of the bytes it read; the
 *   validator re-hashes the file. A rebuilt or replaced artifact invalidates
 *   the proof that read the old one.
 * - **Wrong identity**: identity is captured here and compared to the live
 *   candidate by the validator (head, branch, toolchain, release profile).
 *
 * Digests are over sorted `relativePath\0sha256\n` lines, so a rename, a
 * content edit, or an added file inside a covered path all move the digest, and
 * the order of traversal cannot.
 */

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

export const EVIDENCE_DIR = 'dist/rust-evidence'
const SCHEMA_VERSION = 1

/** Directories never digested as "source": build output and vendored trees. */
const SKIP_DIRS = new Set(['target', 'node_modules', 'dist', '.git', '.cargo'])

/** sha256 of one buffer as `sha256:<hex>`. */
export function sha256File(path) {
  return `sha256:${createHash('sha256').update(readFileSync(path)).digest('hex')}`
}

/** Every regular file under `path`, relative to `root`, sorted, skip rules applied. */
function walk(root, path, out) {
  const entries = readdirSync(path, { withFileTypes: true }).sort((a, b) =>
    a.name < b.name ? -1 : 1,
  )
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue
      walk(root, join(path, entry.name), out)
    } else if (entry.isFile()) {
      out.push(relative(root, join(path, entry.name)))
    }
  }
}

/**
 * Digest the files under `sourcePaths` (repo-relative), fail-closed.
 *
 * A path that does not exist, or that contains no files, is an error rather
 * than an empty contribution: a proof whose coverage silently digests to
 * "nothing" is the missing-coverage failure the validator exists to catch, and
 * it must be caught at the point where the claim is made.
 */
export function digestSources(root, sourcePaths) {
  const files = []
  for (const declared of sourcePaths) {
    const absolute = join(root, declared)
    if (!existsSync(absolute)) throw new Error(`binding source path does not exist: ${declared}`)
    const before = files.length
    const stat = statSync(absolute)
    if (stat.isFile()) files.push(declared)
    else walk(root, absolute, files)
    if (files.length === before)
      throw new Error(`binding source path contains no files: ${declared}`)
  }
  const unique = [...new Set(files)].sort()
  const hash = createHash('sha256')
  for (const rel of unique) hash.update(`${rel}\0${sha256File(join(root, rel)).slice(7)}\n`)
  return {
    sourceDigest: `sha256:${hash.digest('hex')}`,
    sources: unique.map((path) => ({ path, sha256: sha256File(join(root, path)).slice(7) })),
  }
}

/** Run a command for its trimmed stdout; `null` when unavailable. */
function capture(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8' })
  if (result.status !== 0 || result.error) return null
  return result.stdout.trim()
}

/**
 * The live candidate identity: git head/branch, pinned toolchain versions and
 * the release profile from `config/` (the source of truth — not `dist/`, which
 * is a build product a stale checkout could carry).
 */
export function candidateIdentity(root) {
  const configPath = join(root, 'config/library-release.v1.json')
  let releaseProfileVersion = null
  if (existsSync(configPath)) {
    try {
      releaseProfileVersion = JSON.parse(readFileSync(configPath, 'utf8')).version ?? null
    } catch {
      releaseProfileVersion = null
    }
  }
  return {
    head: capture('git', ['rev-parse', 'HEAD'], root),
    branch: capture('git', ['rev-parse', '--abbrev-ref', 'HEAD'], root),
    rustc: capture('rustc', ['--version'], root),
    cargo: capture('cargo', ['--version'], root),
    node: process.version,
    pnpm: capture('pnpm', ['--version'], root),
    releaseProfileVersion,
  }
}

/**
 * Write one proof's evidence file.
 *
 * @param {string} root repository root
 * @param {object} spec
 * @param {string} spec.proof proof ID (must exist in the frozen manifest)
 * @param {Record<string, string>} spec.checks check name → `"pass"` / `"fail: reason"`
 * @param {object} [spec.details] producer tallies the validator compares to the manifest
 * @param {string[]} spec.sourcePaths repo-relative paths this proof covers
 * @param {string[]} [spec.artifacts] repo-relative artifact files whose bytes were read
 * @param {string[]} [spec.referenced] producing files read (recorded with their hashes)
 * @param {string} spec.producer the command/tool that produced the result
 * @returns the evidence object written
 */
export function writeEvidence(root, spec) {
  const { proof, checks, sourcePaths, artifacts = [], referenced = [], producer } = spec
  if (typeof proof !== 'string' || proof.length === 0)
    throw new Error('writeEvidence: proof ID required')
  if (checks === undefined || Object.keys(checks).length === 0)
    throw new Error(
      `writeEvidence(${proof}): at least one check is required — ` +
        'an evidence file with no checks cannot be validated and must not be written',
    )
  for (const [name, value] of Object.entries(checks))
    if (typeof value !== 'string')
      throw new Error(`writeEvidence(${proof}): check "${name}" must be a string`)

  const identity = candidateIdentity(root)
  const binding = digestSources(root, sourcePaths)
  binding.artifacts = artifacts.map((path) => {
    const absolute = join(root, path)
    if (!existsSync(absolute))
      throw new Error(`writeEvidence(${proof}): artifact missing at write time: ${path}`)
    return { path, sha256: sha256File(absolute).slice(7) }
  })

  const allPass = Object.values(checks).every((value) => value === 'pass')
  const evidence = {
    schemaVersion: SCHEMA_VERSION,
    proof,
    result: allPass ? 'pass' : 'fail',
    checks,
    ...(spec.details !== undefined ? { details: spec.details } : {}),
    producedAt: new Date().toISOString(),
    producer,
    identity,
    binding,
    referenced: referenced.map((path) => {
      const absolute = join(root, path)
      if (!existsSync(absolute))
        throw new Error(`writeEvidence(${proof}): referenced producing file missing: ${path}`)
      return { path, sha256: sha256File(absolute).slice(7) }
    }),
  }

  const dir = join(root, EVIDENCE_DIR)
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, `${proof}.json`), `${JSON.stringify(evidence, null, 2)}\n`)
  return evidence
}

/**
 * Read one proof's evidence back from disk.
 *
 * Returns `null` when absent or unparsable — never a partial object. The
 * validator turns `null` into a named "missing" failure; this function's job is
 * only to refuse to hand it a half-read claim.
 */
export function readEvidence(root, proof) {
  const path = join(root, EVIDENCE_DIR, `${proof}.json`)
  if (!existsSync(path)) return null
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return null
  }
}

/** Repository root of this checkout, for producers at the top level. */
export const repoRoot = fileURLToPath(new URL('../../', import.meta.url))

/** Path separator used when writing repo-relative paths into evidence. */
export const PATH_SEP = sep
