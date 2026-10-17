/* global process, URL */
/**
 * The required-coverage validator: the completion denominator, enforced.
 *
 * ## What RF01 was, and what this file is
 *
 * The 2026-10-16 reaudit proved the release gate could reach
 * `candidate-verified` from five self-declared JSON rows: `exists: true`, a
 * non-empty `sha256`, a hardcoded profile. No producing file was opened, no
 * artifact was hashed, no check existed — the ledger asserted its own truth.
 * `docs/rust/required-coverage.json` is the frozen replacement: the whole scope
 * (R00–R11, G01–G10, OWN-T01–OWN-T10) as items, each item naming the proofs
 * that satisfy it, each proof naming the checks, sources, artifacts and
 * referenced producing files that constitute it.
 *
 * This module is the validator half (the producer half is `evidence.mjs`). It
 * re-opens every referenced file, re-hashes every byte, re-derives every result
 * and compares every identity — and returns the list of everything that does
 * not hold. It never writes, never repairs, and never emits a partial pass:
 * `errors.length === 0` is the only true value, and the caller decides how to
 * report everything short of it.
 *
 * ## The failure semantics, stated exactly
 *
 * - A required check with no entry in the evidence never ran. It is treated
 *   exactly like a failed check — the silent-gap failure the old gate had by
 *   construction (§1.1 item 1: a missing proof is a failing proof).
 * - `result` is recomputed from `checks`. Evidence that claims `pass` while
 *   carrying any non-`pass` check is a contradiction and an error, not a
 *   judgment call.
 * - Identity is compared field-for-field against the LIVE candidate. Evidence
 *   produced on another head, branch, toolchain or release profile is stale by
 *   definition, no matter how green its checks were there.
 * - The source digest is recomputed from the manifest's frozen `sourcePaths`,
 *   not from the evidence's own recorded list, so a producer cannot narrow its
 *   own coverage. The recorded source set must also cover the manifest's.
 * - An evidence file in `dist/rust-evidence/` that names no proof in the frozen
 *   manifest is an *invented proof* and an error. The manifest is closed.
 *
 * Structural problems in the manifest itself (duplicate item IDs, a proof
 * referenced by an item but not defined, empty requiredChecks) THROW rather
 * than return: they are bugs in the denominator, and a validator that graded
 * its own broken rubric would be worse than no validator.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { readEvidence, digestSources, sha256File } from './evidence.mjs'

/** The frozen manifest this validator grades against. */
export const MANIFEST_PATH = 'docs/rust/required-coverage.json'

/** Fields of the live candidate identity that evidence must match exactly. */
const IDENTITY_FIELDS = [
  'head',
  'branch',
  'rustc',
  'cargo',
  'node',
  'pnpm',
  'releaseProfileVersion',
]

/** Parse the frozen manifest, fail-closed on structural problems. */
export function loadManifest(root) {
  const path = join(root, MANIFEST_PATH)
  if (!existsSync(path)) throw new Error(`required coverage manifest missing: ${MANIFEST_PATH}`)
  let manifest
  try {
    manifest = JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    throw new Error(`required coverage manifest unparsable: ${error.message}`)
  }
  const problems = []
  if (!Array.isArray(manifest.items) || manifest.items.length === 0)
    problems.push('items must be a non-empty array')
  if (manifest.proofs === null || typeof manifest.proofs !== 'object')
    problems.push('proofs must be an object keyed by proof ID')

  const seenItems = new Set()
  for (const item of manifest.items ?? []) {
    if (!item.id) problems.push('an item has no id')
    if (seenItems.has(item.id)) problems.push(`duplicate item id ${item.id}`)
    seenItems.add(item.id)
    if (!Array.isArray(item.proofs) || item.proofs.length === 0)
      problems.push(`item ${item.id} names no proofs`)
  }

  const proofs = manifest.proofs ?? {}
  for (const [id, proof] of Object.entries(proofs)) {
    if (proof.kind === 'evidence') {
      if (!Array.isArray(proof.requiredChecks) || proof.requiredChecks.length === 0)
        problems.push(`proof ${id} declares no requiredChecks`)
      if (!Array.isArray(proof.sourcePaths) || proof.sourcePaths.length === 0)
        problems.push(`proof ${id} declares no sourcePaths`)
    } else if (proof.kind === 'document' || proof.kind === 'json') {
      if (!proof.path) problems.push(`proof ${id} declares no path`)
    } else {
      problems.push(`proof ${id} has unknown kind ${JSON.stringify(proof.kind)}`)
    }
  }
  for (const item of manifest.items ?? []) {
    for (const proofId of item.proofs ?? []) {
      if (!proofs[proofId]) problems.push(`item ${item.id} references undefined proof ${proofId}`)
    }
  }
  if (problems.length > 0)
    throw new Error(
      `required coverage manifest is structurally invalid (${problems.length}):\n` +
        problems.map((line) => `  - ${line}`).join('\n'),
    )

  return manifest
}

/** Deep structural equality, used only for the frozen `expect` subsets. */
function deepEqual(left, right) {
  if (Object.is(left, right)) return true
  if (typeof left !== 'object' || typeof right !== 'object' || left === null || right === null)
    return false
  const leftKeys = Object.keys(left)
  const rightKeys = Object.keys(right)
  if (leftKeys.length !== rightKeys.length) return false
  return leftKeys.every((key) => deepEqual(left[key], right[key]))
}

/** Validate one evidence-kind proof. Returns the list of failures found. */
function validateEvidenceProof(root, id, spec, identity) {
  const errors = []
  const evidence = readEvidence(root, id)
  if (!evidence) {
    return [`proof ${id}: no evidence document in dist/rust-evidence/ (missing or unparsable)`]
  }
  if (evidence.proof !== id)
    errors.push(`proof ${id}: evidence file names proof ${JSON.stringify(evidence.proof)}`)
  if (evidence.schemaVersion !== 1)
    errors.push(
      `proof ${id}: unknown evidence schemaVersion ${JSON.stringify(evidence.schemaVersion)}`,
    )
  if (typeof evidence.producer !== 'string' || evidence.producer.length === 0)
    errors.push(`proof ${id}: evidence carries no producer`)

  // Required checks: absent means never ran, and never ran == failed.
  for (const check of spec.requiredChecks) {
    const value = evidence.checks?.[check]
    if (value === undefined)
      errors.push(`proof ${id}: required check ${JSON.stringify(check)} never ran (no entry)`)
    else if (value !== 'pass')
      errors.push(
        `proof ${id}: required check ${JSON.stringify(check)} is ${JSON.stringify(value)}`,
      )
  }
  const extraChecks = Object.keys(evidence.checks ?? {}).filter(
    (check) => !spec.requiredChecks.includes(check),
  )
  if (extraChecks.length > 0)
    errors.push(
      `proof ${id}: evidence carries checks the frozen manifest does not declare (${extraChecks.join(', ')})`,
    )

  // The derived result must agree with the checks it was derived from.
  const allPass = Object.values(evidence.checks ?? {}).every((value) => value === 'pass')
  const derived = allPass ? 'pass' : 'fail'
  if (evidence.result !== derived)
    errors.push(
      `proof ${id}: result ${JSON.stringify(evidence.result)} contradicts its checks (derived ${derived})`,
    )

  // Identity: the proof must belong to THIS candidate.
  for (const field of IDENTITY_FIELDS) {
    if ((evidence.identity?.[field] ?? null) !== (identity?.[field] ?? null))
      errors.push(
        `proof ${id}: identity.${field} is ${JSON.stringify(evidence.identity?.[field] ?? null)}, ` +
          `live candidate has ${JSON.stringify(identity?.[field] ?? null)} (stale or foreign proof)`,
      )
  }

  // Source binding: recomputed from the MANIFEST's frozen paths, and the
  // recorded set must cover them — a producer may not narrow its own coverage.
  let fresh
  try {
    fresh = digestSources(root, spec.sourcePaths)
  } catch (error) {
    return [...errors, `proof ${id}: source binding cannot be recomputed: ${error.message}`]
  }
  const recordedSources = (evidence.binding?.sources ?? []).map((entry) => entry.path)
  const missingSources = fresh.sources
    .map((entry) => entry.path)
    .filter((path) => !recordedSources.includes(path))
  if (missingSources.length > 0)
    errors.push(
      `proof ${id}: recorded sources do not cover the manifest's sourcePaths (missing e.g. ${missingSources.slice(0, 3).join(', ')})`,
    )
  if (evidence.binding?.sourceDigest !== fresh.sourceDigest)
    errors.push(
      `proof ${id}: sourceDigest mismatch (recorded ${evidence.binding?.sourceDigest ?? 'null'}, ` +
        `recomputed ${fresh.sourceDigest}) — covered sources changed after the proof ran`,
    )

  // Artifact bytes: every manifest artifact must be bound, and re-hashed here.
  const recordedArtifacts = new Map(
    (evidence.binding?.artifacts ?? []).map((entry) => [entry.path, entry.sha256]),
  )
  for (const artifact of spec.artifacts ?? []) {
    if (!recordedArtifacts.has(artifact)) {
      errors.push(`proof ${id}: manifest artifact ${artifact} was not bound at write time`)
      continue
    }
    const absolute = join(root, artifact)
    if (!existsSync(absolute)) {
      errors.push(`proof ${id}: artifact ${artifact} no longer exists on disk`)
      continue
    }
    const freshHash = sha256File(absolute).slice(7)
    if (freshHash !== recordedArtifacts.get(artifact))
      errors.push(
        `proof ${id}: artifact ${artifact} hash mismatch (recorded ${recordedArtifacts.get(artifact)}, ` +
          `rehashed ${freshHash}) — the proof read different bytes than these`,
      )
  }

  // Referenced producing files: same treatment.
  const recordedRefs = new Map(
    (evidence.referenced ?? []).map((entry) => [entry.path, entry.sha256]),
  )
  for (const referenced of spec.referenced ?? []) {
    if (!recordedRefs.has(referenced)) {
      errors.push(
        `proof ${id}: manifest referenced file ${referenced} was not recorded at write time`,
      )
      continue
    }
    const absolute = join(root, referenced)
    if (!existsSync(absolute)) {
      errors.push(`proof ${id}: referenced file ${referenced} no longer exists on disk`)
      continue
    }
    const freshHash = sha256File(absolute).slice(7)
    if (freshHash !== recordedRefs.get(referenced))
      errors.push(
        `proof ${id}: referenced file ${referenced} hash mismatch (recorded ${recordedRefs.get(referenced)}, ` +
          `rehashed ${freshHash}) — the producing file changed after the proof ran`,
      )
  }

  // Frozen tallies: every `expect` key must be present and equal in details.
  for (const [key, expected] of Object.entries(spec.expect ?? {})) {
    const actual = evidence.details?.[key]
    if (actual === undefined)
      errors.push(`proof ${id}: details missing frozen tally ${JSON.stringify(key)}`)
    else if (!deepEqual(actual, expected))
      errors.push(
        `proof ${id}: details.${key} is ${JSON.stringify(actual)}, frozen expectation is ${JSON.stringify(expected)}`,
      )
  }

  return errors
}

/** Validate one document/json-kind proof against its contract. */
function validateContractProof(root, id, spec) {
  const absolute = join(root, spec.path)
  if (!existsSync(absolute)) return [`proof ${id}: contract file missing: ${spec.path}`]
  const text = readFileSync(absolute, 'utf8')
  if (spec.kind === 'document') {
    return (spec.contains ?? [])
      .filter((needle) => !text.includes(needle))
      .map((needle) => `proof ${id}: ${spec.path} does not contain ${JSON.stringify(needle)}`)
  }
  let parsed
  try {
    parsed = JSON.parse(text)
  } catch (error) {
    return [`proof ${id}: ${spec.path} is not parsable JSON: ${error.message}`]
  }
  return (spec.keys ?? [])
    .filter((key) => parsed[key] === undefined)
    .map((key) => `proof ${id}: ${spec.path} is missing key ${JSON.stringify(key)}`)
}

/**
 * Validate the whole frozen coverage manifest against the live tree.
 *
 * @param {object} options
 * @param {string} options.root repository root
 * @param {object} options.identity live candidate identity (from
 *   `candidateIdentity(root)`) — evidence must match it field-for-field
 * @returns {{ requiredCoverage: object, errors: string[] }} the normalized
 *   coverage document and every failure found; `errors.length === 0` is the
 *   only state in which required coverage is complete
 */
export function validateCoverage({ root, identity }) {
  const manifest = loadManifest(root)
  const manifestDigest = `sha256:${createHash('sha256')
    .update(readFileSync(join(root, MANIFEST_PATH)))
    .digest('hex')}`

  const errors = []
  const normalizedProofs = Object.entries(manifest.proofs).map(([id, spec]) => ({
    id,
    kind: spec.kind,
    requiredChecks: spec.requiredChecks ?? [],
    sourcePaths: spec.sourcePaths ?? [],
    artifacts: spec.artifacts ?? [],
    referenced: spec.referenced ?? [],
  }))

  for (const [id, spec] of Object.entries(manifest.proofs)) {
    const failures =
      spec.kind === 'evidence'
        ? validateEvidenceProof(root, id, spec, identity)
        : validateContractProof(root, id, spec)
    errors.push(...failures)
  }

  // The manifest is closed: an evidence file it does not name is an invented
  // proof, and an invented proof is precisely the RF01 failure mode.
  const evidenceDir = join(root, 'dist/rust-evidence')
  if (existsSync(evidenceDir)) {
    const declared = Object.entries(manifest.proofs)
      .filter(([, spec]) => spec.kind === 'evidence')
      .map(([id]) => `${id}.json`)
    for (const file of readdirSync(evidenceDir).sort()) {
      if (!file.endsWith('.json')) continue
      if (!declared.includes(file))
        errors.push(
          `proof ${file.slice(0, -5)}: evidence file is not named in the frozen manifest (invented proof)`,
        )
    }
  }

  return {
    requiredCoverage: {
      manifest: MANIFEST_PATH,
      manifestDigest,
      frozen: manifest.frozen ?? null,
      items: manifest.items.map((item) => ({ id: item.id, proofs: item.proofs })),
      proofs: normalizedProofs,
    },
    errors,
  }
}

/** Repo root of this checkout. */
export const repoRoot = fileURLToPath(new URL('../../', import.meta.url))

// Runnable so an auditor can re-run the validator standalone against the same
// frozen manifest: `node tools/rust/coverage.mjs` prints the JSON verdict.
if (import.meta.main) {
  const { requiredCoverage, errors } = validateCoverage({
    root: repoRoot,
    identity: (await import('./evidence.mjs')).candidateIdentity(repoRoot),
  })
  process.stdout.write(
    `${JSON.stringify({ requiredCoverage, errors, complete: errors.length === 0 }, null, 2)}\n`,
  )
  process.exitCode = errors.length === 0 ? 0 : 1
}
