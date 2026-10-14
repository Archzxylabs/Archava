/* global process */
/**
 * Generate the current-profile acceptance manifest for the local library.
 *
 * This script performs no verification of its own. It reads the proof artifacts that
 * `pnpm verify`, `pnpm security:verify` and the standalone browser/upgrade/capacity
 * commands already produced, and reports exactly what they proved — no more.
 *
 * Why this exists: the previous acceptance manifest was written by hand, and hand-written
 * numbers drift from the machine evidence that has to back them. This repo already admits
 * the failure mode in README.md: "Nothing here gates the _prose_." A generator cannot fix
 * prose, but it can own the manifest so the manifest is never stale silently.
 *
 * The rules below are the point of the file. Each one refuses a way the output could look
 * complete while being wrong.
 */
import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { resolve, join, relative } from 'node:path'

const root = resolve(import.meta.dirname, '../..')

/** Commands an operator runs, in order, to reproduce the evidence this manifest reports. */
const REPRODUCE = [
  'pnpm verify',
  'pnpm security:verify',
  'pnpm crm:benchmark',
  'pnpm crm:verify-browser',
  'pnpm crm:verify-upgrade --cold-prior',
  'pnpm accommodation:verify --cold-prior',
  'pnpm kits:verify',
  'pnpm capacity:verify',
  'pnpm library:manifest',
]

/**
 * Every source the manifest depends on, with a role and whether it declares a profile.
 * A source listed here and missing on disk is an error, never a silently dropped section.
 */
const SOURCES = [
  { key: 'security', file: 'dist/security-proof/results.json', label: 'security-proof' },
  { key: 'artifacts', file: 'dist/library/artifacts.json', label: null },
  { key: 'releaseProfile', file: 'dist/library/release-profile.json', label: null },
  { key: 'kits', file: 'dist/kit-proof/results.json', label: 'kit-proof' },
  { key: 'browser', file: 'dist/browser-proof/results.json', label: 'browser-proof' },
  {
    key: 'browserMinimal',
    file: 'dist/browser-proof/minimal-results.json',
    label: 'browser-minimal-proof',
  },
  { key: 'upgrade', file: 'dist/upgrade-proof/results.json', label: null },
  { key: 'capacity', file: 'dist/delivery-capacity-proof/results.json', label: 'capacity-proof' },
  {
    key: 'capacityHost',
    file: 'dist/delivery-capacity-host-proof/results.json',
    label: 'capacity-host-proof',
  },
]

/** Read one required source or fail naming its path. Missing evidence is an error. */
export const loadSource = async (source) => {
  const absolute = join(root, source.file)
  if (!existsSync(absolute))
    throw new Error(
      `Missing acceptance evidence ${relative(root, absolute)}; run its producing command first`,
    )
  const parsed = JSON.parse(await readFile(absolute, 'utf8'))
  if (parsed === null || typeof parsed !== 'object')
    throw new Error(`Acceptance evidence ${relative(root, absolute)} is not a JSON object`)
  return parsed
}

/**
 * Build the manifest from an already-loaded source map.
 *
 * Exported so the test suite exercises this exact function rather than spawning the CLI.
 * `generatedAt` is passed in by the caller so a deterministic input can produce
 * deterministic output.
 */
export const buildManifest = async (sources, { generatedAt, generatedBy }) => {
  const trusted = sources.trusted
  const profile = trusted.version
  if (typeof profile !== 'string' || profile.length === 0)
    throw new Error('Trusted catalogue has no version; cannot establish an acceptance profile')

  // Rule: the profile must agree everywhere a source declares one. This is the check that
  // would have refused the stale hand-written manifest the moment the profile advanced.
  for (const source of SOURCES) {
    if (!source.label) continue
    const declared = sources[source.key]?.profile
    if (declared === undefined)
      throw new Error(`${source.label} declares no profile; refusing to assume ${profile}`)
    if (declared !== profile)
      throw new Error(
        `${source.label} declares profile ${declared} but the trusted catalogue is ${profile}`,
      )
  }

  const security = sources.security
  const upgrade = sources.upgrade
  const artifacts = sources.artifacts
  const releaseProfile = sources.releaseProfile
  const kits = sources.kits

  if (!Array.isArray(security.commands)) throw new Error('security-proof has no commands array')
  if (!Array.isArray(security.controls)) throw new Error('security-proof has no controls array')
  if (!Array.isArray(artifacts.artifacts))
    throw new Error('artifact inventory has no artifacts array')

  // Rule: migration/recovery must have been tested against the profile being accepted.
  // An upgrade proof from an older target would let the manifest claim what it never proved.
  if (upgrade.toProfile !== profile)
    throw new Error(
      `upgrade-proof targets ${upgrade.toProfile} but the accepted profile is ${profile}`,
    )

  // Rule: operational kit acceptance is copied, never derived. A non-zero value is a
  // separate acceptance decision made elsewhere and must be reviewed, not inherited.
  const operationalKitsAccepted = Number(kits.operationalKitsAccepted)
  if (!Number.isInteger(operationalKitsAccepted))
    throw new Error('kit-proof operationalKitsAccepted is not an integer')

  const inventory = {
    artifacts: artifacts.artifacts.length,
    catalogSha256: security.catalogSha256,
    artifactSha256: security.artifactSha256,
    catalog: {
      host: releaseProfile.catalog.host,
      contracts: releaseProfile.catalog.contracts,
      externals: releaseProfile.catalog.externals.length,
      databaseLedgers: releaseProfile.catalog.databaseLedgers,
    },
  }

  const evidence = [
    {
      source: 'dist/security-proof/results.json',
      label: 'root/runtime security matrix',
      command: 'pnpm security:verify',
      checkedAt: security.checkedAt,
      checks: security.commands.length,
      controls: security.controls.length,
    },
    {
      source: 'dist/kit-proof/results.json',
      label: 'kit planner',
      command: 'pnpm kits:verify',
      checkedAt: kits.checkedAt ?? null,
      checks: kits.checks.length,
    },
    {
      source: 'dist/browser-proof/results.json',
      label: 'browser regression',
      command: 'pnpm crm:verify-browser',
      checkedAt: sources.browser.checkedAt ?? null,
      checks: sources.browser.checks.length,
      providerCalls: sources.browser.providerCalls,
    },
    {
      source: 'dist/browser-proof/minimal-results.json',
      label: 'browser regression, minimal composition',
      command: 'pnpm crm:verify-browser',
      checkedAt: sources.browserMinimal.checkedAt ?? null,
      checks: sources.browserMinimal.checks.length,
      providerCalls: sources.browserMinimal.providerCalls,
    },
    {
      source: 'dist/upgrade-proof/results.json',
      label: 'prior-profile upgrade and recovery',
      command: 'pnpm crm:verify-upgrade --cold-prior',
      checkedAt: upgrade.checkedAt ?? null,
      checks: upgrade.checks.length,
      providerCalls: upgrade.providerCalls,
    },
    {
      source: 'dist/delivery-capacity-proof/results.json',
      label: 'pooled delivery capacity',
      command: 'pnpm capacity:verify',
      checkedAt: sources.capacity.checkedAt ?? null,
      checks: sources.capacity.checks.length,
      providerCalls: sources.capacity.providerCalls,
    },
    {
      source: 'dist/delivery-capacity-host-proof/results.json',
      label: 'delivery capacity host composition',
      command: 'pnpm capacity:verify',
      checkedAt: sources.capacityHost.checkedAt ?? null,
      checks: sources.capacityHost.checks.length,
      providerCalls: sources.capacityHost.providerCalls,
    },
  ]

  return {
    schemaVersion: '1.0.0',
    profile,
    generatedAt,
    generatedBy,
    inventory,
    unit: { passed: security.unit.passed, failed: security.unit.failed },
    security: {
      controlsPassed: controlsPassed(security.controls),
      controls: security.controls.length,
      commands: security.commands.length,
      checkedAt: security.checkedAt,
      node: security.node,
      distributionScope: security.distributionScope,
      installPolicy: security.installPolicy,
      audits: security.audits,
      // Copied verbatim. A limit re-typed here is a limit this manifest could quietly drop.
      limits: security.limits,
    },
    kits: {
      candidates: kits.candidates,
      operationalKitsAccepted,
      // Which narrow domain scopes are accepted is a decision the kit ledger records in
      // `docs/STARTER_KIT_IMPLEMENTATION.md`, not something any of these machines proves.
      // It is deliberately absent here: the capacity proof that carries a field of this
      // name hardcodes an empty list because capacity is not a kit-acceptance domain, so
      // copying it would report proof that does not exist.
    },
    upgrade: {
      priorCommit: upgrade.priorCommit,
      fromProfile: upgrade.fromProfile,
      toProfile: upgrade.toProfile,
      checks: upgrade.checks.length,
      providerCalls: upgrade.providerCalls,
      binaryRollbackIsSchemaRollback: upgrade.binaryRollbackIsSchemaRollback,
    },
    evidence,
    reproduce: REPRODUCE,
    unfinished: [
      'Browser, upgrade and capacity evidence comes from standalone commands, not from pnpm security:verify. A manifest is only as fresh as the last run of those commands; the checkedAt values above are per source.',
      '13 moderate OpenTelemetry-chain template findings remain open; see security.limits.realtimeTemplate.',
      'Certification and penetration-test gates remain false; see security.limits.',
      'The 40 operating kit families remain incomplete; operationalKitsAccepted is copied, not derived.',
    ],
  }
}

const controlsPassed = (controls) =>
  controls.filter((control) => control.result === 'passed').length

/** Main: load every source, build the manifest and write it beside the 1.3.0 baseline. */
export const main = async ({ generatedAt = new Date().toISOString() } = {}) => {
  const sources = {
    trusted: JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8')),
  }
  for (const source of SOURCES) sources[source.key] = await loadSource(source)
  const manifest = await buildManifest(sources, {
    generatedAt,
    generatedBy: relative(root, import.meta.dirname) + '/manifest.mjs',
  })
  const destination = resolve(
    root,
    'docs/audit/local-library',
    `acceptance-${manifest.profile}.json`,
  )
  await mkdir(resolve(root, 'docs/audit/local-library'), { recursive: true })
  // Written the same way every other generated file in this repo is written: stable key
  // order from buildManifest's own literal, so the whole file except `generatedAt` is
  // reproducible and reviewable as a diff. Proving that needs a fixed timestamp, so the
  // test suite exercises determinism through `buildManifest` with a pinned `generatedAt`
  // rather than through this write path.
  //
  // `JSON.stringify` expands arrays the repo's prettier config would keep on one line, so
  // `pnpm format` normally rewrites this file's whitespace after generation. That is only a
  // whitespace change and does not touch the evidence; `pnpm format:check` then covers the
  // result like every other committed file. Trying to format inside this script instead
  // resolves prettier's default printWidth rather than the repo's own config, so it could
  // not reproduce what the CLI does.
  await writeFile(destination, JSON.stringify(manifest, null, 2) + '\n')
  process.stdout.write(
    `Wrote ${relative(root, destination)} for profile ${manifest.profile}: ${manifest.security.controls} controls, ${manifest.security.controlsPassed} passed, ${manifest.inventory.artifacts} artifacts, ${manifest.unit.passed} unit tests, ${manifest.kits.operationalKitsAccepted} operational kits accepted\n`,
  )
  return manifest
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) await main()
