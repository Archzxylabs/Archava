/* global process */
/**
 * P-K05-K04 producer: two business-model consumers of the shipped core/CRM.
 *
 * K05 (agency / project-services) and K04 (wholesale-distribution) are the two
 * `general_business` kit scopes this lane's consumers are graded on. The claim
 * the required checks encode — "K05 agency + K04 wholesale consume identical
 * shipped core/CRM without fork" — is this producer's narrow contract:
 *
 * 1. ONE artifact set is cut from the shipped runtime (`archava-core`:
 *    tenant ids, the role/permission tables and the action registry;
 *    `archava-domain`: the CRM command policy gate; `archava-adapters`: the
 *    tenant-scoped PostgreSQL seam; `archava-assistant`/`archava-own`: the
 *    host's composed leaves; `archava-host`: the real CRM HTTP/authority
 *    surface; and `archava-pg-fixture`: the disposable PostgreSQL fixture) —
 *    once, digested, and then given to BOTH consumers. A fork between the two
 *    consumers' copies moves a tree hash, and the "identical core/CRM artifact
 *    hashes" check refuses it.
 * 2. The two consumers are materialized from ONE committed template
 *    (`examples/rust-kit-consumers/kit-template`) whose witness code is
 *    byte-identical; the only generated difference is `kit.config.json` — the
 *    kit's identity and the action set its workspace enables. That is the
 *    "without fork" construction: business-model difference is configuration
 *    over one shipped surface, never a second implementation.
 * 3. Each consumer builds OUTSIDE the product workspace, offline, with an
 *    empty `CARGO_HOME` (no registry cache to fall back on), from the artifact
 *    bundle only — no product path may be declared or referenced (the
 *    manifest-path check and the template's structure enforce it), and its
 *    witnesses must decide against the shipped policy gate exactly as the
 *    configuration records.
 *
 * ## Why this lives in the release gate, not the fast gate
 *
 * Its `producedBy` is `pnpm rust:verify-release`: the artifacts are cut at
 * release-verification time, and the two consumer builds cost real minutes.
 * `verify-release.mjs` wires the step and binds the frozen checks.
 *
 * ## Honesty notes the details blob carries
 *
 * - "core/CRM" means the shipped Rust runtime both business models consume:
 *   the policy core plus the real CRM host, adapters and fixture. The kits'
 *   own TypeScript modules are verified on their own lane by the module gates,
 *   and the product browser surface is verified by the browser proof. Nothing
 *   here should be read as "the whole K05 product runs in Rust".
 * - The consumer witness drives the real router against its own disposable
 *   PostgreSQL (`archava_host::router` + `archava_pg_fixture::provision`), and
 *   creates/updates/reads/replays contact, activity, follow-up and opportunity
 *   rows. A consumer that compiles but does not run that named witness fails
 *   the assembly check; the fixture identity it prints is recorded in the
 *   details blob.
 * - The shipped bundles are source-tree tarballs resolved through the
 *   consumer's own workspace inheritance (see the template's Cargo.toml
 *   header). `cargo package` cannot emit an offline-consumable tarball for a
 *   crate whose dependency on another workspace crate is registry-style; the
 *   template documents why the bundle form is the one the artifact boundary
 *   takes, and the digest check is what makes it a checked fact.
 * - The consumer runs against the configured kit scope from
 *   `packages/kits/src/catalogue.ts`; the producer cross-checks each kit's
 *   recorded identity fields against that catalogue text so a config cannot
 *   drift from the kit it claims to serve.
 */

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile, cp } from 'node:fs/promises'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from './coverage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const rustWorkspace = join(root, 'rust')

/**
 * The required checks, frozen in the manifest under `P-K05-K04`. The producer
 * refuses a manifest that demands anything else (and refuses to run when a
 * required check has no producer wired here) — the same both-directions guard
 * the named-witness producers run, typed for step-level checks: each check
 * passes only when the run's evidence says the whole consumer succeeded.
 */
export const REQUIRED_CHECKS = [
  'K05 consumer assembled and verified',
  'K04 consumer assembled and verified',
  'identical core/CRM artifact hashes',
]

/**
 * The two business-model configurations, from `packages/kits/src/catalogue.ts`.
 *
 * The kit block is the kit's identity from that catalogue, verbatim. The
 * workspace block is the demo tenant each consumer runs as (real slugs — they
 * must pass the shipped tenant rule). `enabledActions` is the client
 * configuration both kits' workspaces would carry: the action ids the
 * shipped registry carries that THIS business model's modules use. The
 * `disabledProbe` is an action the OTHER kit's spine needs that this kit's
 * modules do not run — the shipped registry still carries it (the anti-fork
 * witness asserts that), but this workspace's configuration scopes it off, and
 * the gate answers `action_disabled_for_client` — the business-model boundary
 * riding the shipped gate, not a forked registry.
 *
 * `actions` rows are the decision witnesses: one `ActionRequest` each, with the
 * decision shape (allowed / confirmation-needed + verbatim prompt / denied +
 * denial reason) pinned from the shipped action table. The `roleExpectations`
 * rows pin the platform's role/permission table — identical for both kits on
 * purpose, because the five roles are platform vocabulary, not kit extensions.
 */
export const KIT_CONSUMERS = {
  K04: {
    kit: {
      id: 'K04',
      slug: 'wholesale-distribution',
      label: 'Distributor dan wholesale B2B',
      domainRequirement: 'domain.wholesale-distribution',
      recordOwner: 'wholesale-distribution',
      apiContract: 'wholesale-distribution.api.v1',
    },
    workspace: { tenant: 'wholesale-b2b-demo', foreignTenant: 'agency-studio-demo' },
    registryVersion: '1.2.0',
    enabledActions: [
      'product.read',
      'availability.read',
      'order.status.read',
      'cart.item.add',
      'cart.item.remove',
      'checkout.start',
      'admin.config.update',
      'admin.knowledge.reindex',
    ],
    disabledProbe: 'crm.followup.create',
    invalidTenants: ['Not-A-Tenant', 'has_underscore', '', 'double--dash'],
    roleExpectations: [
      { role: 'owner', permission: 'write', resource: 'settings', granted: true },
      { role: 'owner', permission: 'configure', resource: 'settings', granted: true },
      { role: 'owner', permission: 'configure', resource: 'members', granted: true },
      { role: 'admin', permission: 'write', resource: 'products', granted: true },
      { role: 'admin', permission: 'write', resource: 'settings', granted: false },
      { role: 'editor', permission: 'read', resource: 'analytics', granted: true },
      { role: 'editor', permission: 'read', resource: 'members', granted: false },
      { role: 'editor', permission: 'write', resource: 'leads', granted: false },
      { role: 'viewer', permission: 'read', resource: 'analytics', granted: true },
      { role: 'viewer', permission: 'write', resource: 'products', granted: false },
      { role: 'archava_assistant', permission: 'read', resource: 'content', granted: true },
      { role: 'archava_assistant', permission: 'read', resource: 'products', granted: false },
    ],
    actions: [
      {
        actionId: 'order.status.read',
        role: 'archava_assistant',
        capability: 'transact',
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'cart.item.add',
        role: 'archava_assistant',
        capability: 'transact',
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'cart.item.add',
        role: 'archava_assistant',
        capability: 'act',
        expect: { decision: 'Denied', reason: 'capability_insufficient' },
      },
      {
        actionId: 'checkout.start',
        role: 'archava_assistant',
        capability: 'transact',
        expect: {
          decision: 'ConfirmationRequired',
          mode: 'user_confirm',
          prompt: 'This involves a payment or an account change. Please confirm before I continue.',
        },
      },
      {
        actionId: 'checkout.start',
        role: 'archava_assistant',
        capability: 'transact',
        confirmed: true,
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'checkout.start',
        role: 'archava_assistant',
        capability: 'transact',
        confirmed: true,
        availableOnPage: false,
        expect: { decision: 'Denied', reason: 'action_unavailable_on_page' },
      },
      {
        actionId: 'cart.item.add',
        role: 'archava_assistant',
        capability: 'transact',
        declined: true,
        expect: { decision: 'Denied', reason: 'declined_by_visitor' },
      },
      {
        actionId: 'cart.item.add',
        role: 'viewer',
        capability: 'transact',
        confirmed: true,
        expect: { decision: 'Denied', reason: 'role_not_allowed' },
      },
      {
        actionId: 'admin.config.update',
        role: 'owner',
        capability: 'enterprise',
        expect: { decision: 'Denied', reason: 'admin_action_requires_human' },
      },
      {
        actionId: 'admin.config.update',
        role: 'owner',
        capability: 'enterprise',
        humanApproved: true,
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'crm.followup.create',
        role: 'archava_assistant',
        capability: 'act',
        confirmed: true,
        expect: { decision: 'Denied', reason: 'action_disabled_for_client' },
      },
    ],
  },
  K05: {
    kit: {
      id: 'K05',
      slug: 'project-services',
      label: 'Agency, studio, jasa proyek',
      domainRequirement: 'domain.project-services',
      recordOwner: 'project-services',
      apiContract: 'project-services.api.v1',
    },
    workspace: { tenant: 'agency-studio-demo', foreignTenant: 'wholesale-b2b-demo' },
    registryVersion: '1.2.0',
    enabledActions: [
      'product.read',
      'crm.lead.create',
      'crm.followup.create',
      'crm.followup.complete',
      'admin.config.update',
      'admin.knowledge.reindex',
    ],
    disabledProbe: 'checkout.start',
    invalidTenants: ['Not-A-Tenant', 'has_underscore', '', 'double--dash'],
    roleExpectations: [
      { role: 'owner', permission: 'write', resource: 'settings', granted: true },
      { role: 'owner', permission: 'configure', resource: 'settings', granted: true },
      { role: 'owner', permission: 'configure', resource: 'members', granted: true },
      { role: 'admin', permission: 'write', resource: 'products', granted: true },
      { role: 'admin', permission: 'write', resource: 'settings', granted: false },
      { role: 'editor', permission: 'read', resource: 'analytics', granted: true },
      { role: 'editor', permission: 'read', resource: 'members', granted: false },
      { role: 'editor', permission: 'write', resource: 'leads', granted: false },
      { role: 'viewer', permission: 'read', resource: 'analytics', granted: true },
      { role: 'viewer', permission: 'write', resource: 'products', granted: false },
      { role: 'archava_assistant', permission: 'read', resource: 'content', granted: true },
      { role: 'archava_assistant', permission: 'read', resource: 'products', granted: false },
    ],
    actions: [
      {
        actionId: 'crm.followup.create',
        role: 'archava_assistant',
        capability: 'act',
        confirmed: true,
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'crm.followup.create',
        role: 'archava_assistant',
        capability: 'act',
        expect: {
          decision: 'ConfirmationRequired',
          mode: 'user_confirm',
          prompt: 'This change is reversible. Shall I proceed?',
        },
      },
      {
        actionId: 'crm.followup.complete',
        role: 'archava_assistant',
        capability: 'act',
        confirmed: true,
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'crm.followup.complete',
        role: 'archava_assistant',
        capability: 'assist',
        confirmed: true,
        expect: { decision: 'Denied', reason: 'capability_insufficient' },
      },
      {
        actionId: 'crm.followup.create',
        role: 'archava_assistant',
        capability: 'act',
        declined: true,
        expect: { decision: 'Denied', reason: 'declined_by_visitor' },
      },
      {
        actionId: 'crm.followup.create',
        role: 'viewer',
        capability: 'act',
        confirmed: true,
        expect: { decision: 'Denied', reason: 'role_not_allowed' },
      },
      {
        actionId: 'admin.config.update',
        role: 'owner',
        capability: 'enterprise',
        expect: { decision: 'Denied', reason: 'admin_action_requires_human' },
      },
      {
        actionId: 'admin.config.update',
        role: 'owner',
        capability: 'enterprise',
        humanApproved: true,
        expect: { decision: 'Allowed' },
      },
      {
        actionId: 'checkout.start',
        role: 'archava_assistant',
        capability: 'transact',
        expect: { decision: 'Denied', reason: 'action_disabled_for_client' },
      },
    ],
  },
}

/**
 * The shipped crates both business models consume, cut once as bundles.
 *
 * `directory` is where the bundle unpacks inside the consumer's `deps/`. All
 * but the fixture carry their product workspace version; `pg-fixture` keeps
 * the bare name because the shipped host/adapters manifests name it with the
 * relative path `../pg-fixture` in their dev-dependency tables, and that
 * inheritance must resolve without rewriting the artifact.
 */
export const SHIPPED_CRATES = [
  { crate: 'archava-core', source: 'crates/core', directory: 'archava-core-0.1.0' },
  { crate: 'archava-domain', source: 'crates/domain', directory: 'archava-domain-0.1.0' },
  { crate: 'archava-adapters', source: 'crates/adapters', directory: 'archava-adapters-0.1.0' },
  {
    crate: 'archava-assistant',
    source: 'crates/assistant',
    directory: 'archava-assistant-0.1.0',
  },
  { crate: 'archava-own', source: 'crates/own', directory: 'archava-own-0.1.0' },
  { crate: 'archava-host', source: 'crates/host', directory: 'archava-host-0.1.0' },
  { crate: 'archava-pg-fixture', source: 'crates/pg-fixture', directory: 'pg-fixture' },
]

const TEMPLATE = join(root, 'examples/rust-kit-consumers/kit-template')

/**
 * The policy witnesses the template has always shipped, plus the C04 CRM
 * witness. A consumer that silently drops one of these is not the same
 * consumer even if the remaining tests are green.
 */
const TEMPLATE_WITNESSES = [
  'the_kit_identity_and_workspace_bounds_are_the_configured_ones',
  'the_workspace_grants_match_the_shipped_role_table',
  'the_kit_action_configuration_decides_as_recorded',
  'the_shipped_registry_carries_both_business_models_without_a_preference',
  'a_record_from_another_workspace_is_refused_through_the_seam',
  'a_packaged_consumer_runs_the_real_crm_read_write_recovery_flow',
]

/**
 * The migration directories the shipped fixture applies when it provisions a
 * disposable PostgreSQL. They are copied VERBATIM into the scratch root the
 * fixture resolves as `consumer/deps/pg-fixture/../../..`, so each consumer
 * runs the real ledger rather than a test-local schema.
 */
const MIGRATION_PACKAGES = [
  'commerce-pg',
  'identity-pg',
  'crm-pg',
  'purchasing-pg',
  'foodservice-pg',
  'organizations-pg',
  'proposals-pg',
  'projects-pg',
  'scheduling-pg',
  'accommodation-pg',
  'assets-pg',
  'billing-pg',
  'subscriptions-pg',
  'attendance-pg',
  'provider-runtime',
]

/**
 * Copy the migration inputs and the owner-ledger migration into the scratch
 * root. The fixture computes its root from its own compile-time manifest path
 * (`<consumer>/deps/pg-fixture/../../..`), which is the scratch root shared by
 * both consumers — one byte-identical set of migrations, applied twice against
 * two isolated PostgreSQL instances.
 */
async function copyMigrationInputs(scratch) {
  const packages = join(scratch, 'packages')
  for (const name of MIGRATION_PACKAGES) {
    const source = join(root, 'packages', name, 'migrations')
    if (!existsSync(source))
      throw new Error(`the shipped migration directory is missing: packages/${name}/migrations`)
    await cp(source, join(packages, name, 'migrations'), { recursive: true })
  }
  const owner = join(rustWorkspace, 'crates/host/migrations/owner')
  if (!existsSync(owner))
    throw new Error('the owner-configuration migration is missing: rust/crates/host/migrations/owner')
  await cp(owner, join(scratch, 'rust/crates/host/migrations/owner'), { recursive: true })
}

/**
 * Run cargo in `cwd`, offline unless asked otherwise, with optional
 * per-call environment overrides. Carried from
 * `tools/library/verify-rust-consumers.mjs` (same arguments, same rule: an
 * override that empties `CARGO_HOME` is what turns "built from vendor/" into a
 * checked fact rather than a machine-warmth assumption).
 */
const cargo = (args, cwd, label, extraEnv = {}) => {
  const result = spawnSync('cargo', args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, ...extraEnv },
  })
  if (result.status !== 0)
    throw new Error(`${label} failed (exit ${result.status}):\n${result.stdout}\n${result.stderr}`)
  return result.stdout
}

async function sha256(file) {
  const digest = createHash('sha256')
  digest.update(await readFile(file))
  return `sha256:${digest.digest('hex')}`
}

/** Every file under `directory`, relative and sorted, for a stable walk. */
async function tree(directory, prefix = '', out = []) {
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const relative = `${prefix}${entry.name}`
    const file = join(directory, entry.name)
    if (entry.isDirectory()) await tree(file, `${relative}/`, out)
    else out.push({ path: relative, file })
  }
  return out
}

/**
 * A digest of every file name + content under a directory. This is the
 * anti-fork instrument: the two consumers' `deps/` trees are digested
 * independently and REFUSED when they differ — an implementation the producer
 * did not ship cannot enter one consumer without moving its hash.
 */
async function treeDigest(directory) {
  const digest = createHash('sha256')
  for (const { path, file } of await tree(directory)) {
    digest.update(path)
    digest.update(await readFile(file))
  }
  return `sha256:${digest.digest('hex')}`
}

/**
 * The manifest-path check, carried from `verify-rust-consumers.mjs`: strip
 * comments first (a documented avoidance is not a dependency), then every
 * `path =` value either stays inside the consumer (`deps/`, `vendor`) or is an
 * escape/absolute-product-path offender. The consumers must not be able to
 * reach the product tree, because a consumer that can is not a consumer.
 */
async function assertNoProductPath(directory, label) {
  const offenders = []
  for (const { file } of await tree(directory)) {
    if (!/Cargo\.toml$/.test(file) && !/Cargo\.lock$/.test(file)) continue
    const text = await readFile(file, 'utf8')
    const live = text
      .split('\n')
      .map((line) => line.replace(/(^|\s)#.*$/, ''))
      .join('\n')
    if (/Cargo\.lock$/.test(file)) {
      if (live.includes(root)) offenders.push(`${file}: absolute product path in the lock`)
      continue
    }
    for (const match of live.matchAll(/path\s*=\s*"([^"]+)"/g)) {
      const declared = match[1]
      if (isAbsolute(declared)) {
        offenders.push(`${file}: absolute path ${declared}`)
        continue
      }
      // Resolve relative to the manifest that declares it — the shipped
      // crates' dev-dependency tables legitimately name `../pg-fixture`, and
      // that is inside the consumer. What must never resolve is a path that
      // leaves the consumer root or names the product's `crates/` tree.
      const target = resolve(dirname(file), declared)
      const fromConsumer = relative(directory, target)
      if (fromConsumer.startsWith('..') || isAbsolute(fromConsumer))
        offenders.push(`${file}: escapes the consumer at ${declared}`)
      else if (declared.split(/[\\/]/).includes('crates'))
        offenders.push(`${file}: points into the product tree at ${declared}`)
    }
  }
  if (offenders.length > 0)
    throw new Error(`${label} reached into the product tree:\n${offenders.join('\n')}`)
}

/**
 * Vendor the consumer's third-party closure from a lock resolved against the
 * materialized manifest closure. The template cannot ship a usable
 * `Cargo.lock`: its path dependencies do not exist until `deps/` is unpacked,
 * so the lock is generated here and then frozen by `--locked` on vendor and
 * test. The anti-drift instruments are the manifest closure, the vendored
 * source tree and the two consumers' identical `deps/` digests — not a
 * template lock that pretends the unmaterialized tree is buildable.
 */
async function vendorClosure(consumer) {
  const manifest = await readFile(join(consumer, 'Cargo.toml'), 'utf8')
  for (const { crate, directory } of SHIPPED_CRATES) {
    if (!manifest.includes(`path = "deps/${directory}"`))
      throw new Error(`the consumer no longer consumes the shipped ${crate}`)
  }
  // The lockfile is resolved from the manifest closure once, then every
  // subsequent cargo invocation is `--locked`: vendor and test cannot move a
  // version behind the evidence. `generate-lockfile` itself cannot be
  // `--locked` — its job is to write the pin the later commands assert.
  cargo(['generate-lockfile', '--offline'], consumer, 'consumer lockfile pin')
  const out = cargo(
    ['vendor', '--offline', '--locked', join(consumer, 'vendor')],
    consumer,
    'cargo vendor',
  )
  if (!/\[source\.vendored-sources\]/.test(out))
    throw new Error('cargo vendor did not report a vendored-sources configuration')
  await mkdir(join(consumer, '.cargo'), { recursive: true })
  await writeFile(
    join(consumer, '.cargo/config.toml'),
    '# Written by tools/rust/k05-k04-consumers.mjs — never hand-edited, never staged.\n' +
      '[source.crates-io]\n' +
      'replace-with = "vendored-sources"\n' +
      '\n' +
      '[source.vendored-sources]\n' +
      'directory = "vendor"\n',
  )
}

/**
 * The workspace version the shipped crates inherit, parsed from the product
 * workspace's `[workspace.package]` table rather than restated here: a bundle
 * whose name disagrees with the manifests inside it is worse than no bundle.
 */
function readWorkspaceVersion() {
  const manifest = readFileSync(join(rustWorkspace, 'Cargo.toml'), 'utf8')
  const match = /^\[workspace\.package\]\n(?:(?!\[).)*^version = "([^"]+)"$/ms.exec(manifest)
  if (!match) throw new Error('the product workspace does not declare a package version')
  return match[1]
}

/** `workspaceVersion` is used by `vendorClosure`'s manifest check; order matters. */
const workspaceVersion = readWorkspaceVersion()

/** The candidate identity the proof is bound to, read at proof time. */
function gitIdentity() {
  const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' })
  const branch = spawnSync('git', ['rev-parse', '--abbrev-ref', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  })
  if (head.status !== 0) throw new Error(`cannot read the candidate HEAD: ${head.stderr}`)
  return {
    head: head.stdout.trim(),
    branch: branch.status === 0 ? branch.stdout.trim() : null,
  }
}

/**
 * The kit catalogue cross-check: each kit's recorded identity fields must
 * appear verbatim in `packages/kits/src/catalogue.ts`, so a configuration
 * naming a kit it no longer matches is caught HERE, before a consumer is
 * materialized under that name.
 */
function assertKitCatalogueSync(kitId) {
  const config = KIT_CONSUMERS[kitId]
  const catalogue = readFileSync(join(root, 'packages/kits/src/catalogue.ts'), 'utf8')
  const problems = []
  for (const [field, value] of Object.entries(config.kit)) {
    if (!catalogue.includes(`'${value}'`))
      problems.push(`kit field ${field} is "${value}" — not found in packages/kits/src/catalogue.ts`)
  }
  if (problems.length > 0)
    throw new Error(
      `${kitId}'s consumer configuration has drifted from the shipped kit catalogue:\n` +
        problems.join('\n'),
    )
}

/**
 * Cut ONE bundle per shipped crate — `tar czf` over the crate's directory,
 * renamed inside the archive to the crate's standard directory name. The
 * SAME tarball file is what BOTH consumers receive, and its digest is the
 * first half of the identical-artifacts check.
 *
 * @param {string} bundles directory to write the bundles into
 */
async function cutBundles(bundles) {
  await mkdir(bundles, { recursive: true })
  const artifacts = []
  for (const { crate, source, directory } of SHIPPED_CRATES) {
    if (!existsSync(join(rustWorkspace, source)))
      throw new Error(`the shipped crate source is missing: rust/${source}`)
    const sourceName = basename(source)
    const bundle = join(bundles, `${crate}-${workspaceVersion}.src.tar.gz`)
    const result = spawnSync(
      'tar',
      ['czf', bundle, '--transform', `s,^${sourceName},${directory},`, sourceName],
      { cwd: join(rustWorkspace, 'crates'), encoding: 'utf8' },
    )
    if (result.status !== 0) throw new Error(`could not cut ${crate}: ${result.stderr}`)
    artifacts.push({
      crate,
      directory,
      bundle: basename(bundle),
      tarballSha256: await sha256(bundle),
    })
  }
  return artifacts
}

/**
 * Materialize one consumer: template copy, generated config, the shipped
 * bundles unpacked into `deps/` (once per consumer, from the SAME file), then
 * the vendored closure and the empty-`CARGO_HOME` offline test run.
 *
 * Exported for `verify.test.mjs`'s drift assertion without launching cargo.
 *
 * @param {string} scratch the scratch root this run is using
 * @param {string} kitId 'K04' | 'K05'
 * @param {Array<{crate: string, bundle: string, tarballSha256: string}>} artifacts
 * @returns {Promise<{kit: string, directory: string, tests: number, tested: string[], depsDigest: string, codeDigest: string, crm: object, passed: boolean}>}
 */
async function materializeConsumer(scratch, kitId, artifacts) {
  const config = KIT_CONSUMERS[kitId]
  const consumer = join(scratch, kitId.toLowerCase())
  await cp(TEMPLATE, consumer, { recursive: true })
  await writeFile(
    join(consumer, 'kit.config.json'),
    `${JSON.stringify(config, null, 2)}\n`,
    'utf8',
  )

  const deps = join(consumer, 'deps')
  await mkdir(deps, { recursive: true })
  for (const { bundle } of artifacts) {
    const source = join(scratch, 'bundles', bundle)
    const result = spawnSync('tar', ['xzf', source, '-C', deps], { encoding: 'utf8' })
    if (result.status !== 0) throw new Error(`could not unpack ${bundle}: ${result.stderr}`)
  }
  const depsDigest = await treeDigest(deps)
  // The witness code is the second half of the no-fork claim. Both consumers
  // are materialized from the same template, and this digest makes a
  // post-materialization edit to one consumer's tests a refusal rather than a
  // silent second implementation.
  const codeDigest = await treeDigest(join(consumer, 'tests'))

  await vendorClosure(consumer)
  await assertNoProductPath(consumer, `the ${kitId} consumer`)

  // The cold claim, proven rather than assumed: an empty `CARGO_HOME` leaves
  // no registry cache. Any dependency that does not come out of `vendor/`
  // through the config this script wrote fails here instead of passing on
  // machine warmth.
  const emptyCargoHome = join(scratch, 'empty-cargo-home')
  await mkdir(emptyCargoHome, { recursive: true })
  // `--nocapture` is required for the witness's C04_CONSUMER line: libtest
  // captures a passing test's stdout, and that line is the fixture identity
  // the producer records. The test-name lines remain on stdout either way.
  const testOutput = cargo(
    ['test', '--offline', '--locked', '--', '--nocapture'],
    consumer,
    `${kitId} consumer tests`,
    { CARGO_HOME: emptyCargoHome },
  )
  // Cargo reports one `test result` line PER test binary. The template has
  // two (`crm_witness` and `kit_witness`), so taking the first summary would
  // under-report the consumer. Count the named tests the run actually
  // reported instead; the set is also the witness evidence.
  const tested = []
  for (const line of testOutput.split('\n')) {
    const match = /^test (\S+) \.\.\. ok\b/.exec(line)
    if (match) tested.push(match[1])
  }
  const tests = tested.length
  if (tests === 0) throw new Error(`${kitId} consumer ran no tests — not a result, a silence`)

  // C04 is not satisfied by a consumer that only compiles. The named witness
  // must have run, and its fixture line proves it provisioned a real,
  // isolated PostgreSQL instance and exercised the real CRM flow.
  const missingWitnesses = TEMPLATE_WITNESSES.filter((name) => !tested.includes(name))
  if (missingWitnesses.length > 0)
    throw new Error(`${kitId} consumer did not run: ${missingWitnesses.join(', ')}`)
  const crmLine = testOutput
    .split('\n')
    .find((line) => line.startsWith('C04_CONSUMER '))
  if (crmLine === undefined)
    throw new Error(`${kitId} consumer ran the C04 witness but printed no fixture identity`)
  const crm = Object.fromEntries(
    crmLine
      .slice('C04_CONSUMER '.length)
      .trim()
      .split(/\s+/)
      .map((field) => field.split('=')),
  )
  if (crm.tenant !== config.workspace.tenant)
    throw new Error(
      `${kitId} consumer ran as ${crm.tenant}, not the configured ${config.workspace.tenant}`,
    )

  process.stdout.write(
    `  ✓ consumer ${kitId} (${config.kit.slug}): ${tests} witnesses green in the scratch tree — ` +
      `CRM flow on fixture port ${crm.fixture_port}\n`,
  )
  return {
    kit: kitId,
    directory: consumer,
    tests,
    tested,
    depsDigest,
    codeDigest,
    crm,
    passed: tests > 0,
  }
}

/**
 * Assemble both consumers from one artifact cut, verify them offline, and
 * compare the artifact digests. Returns the graded evidence object and writes
 * `dist/rust-k05-k04-proof.json` on EVERY run — a failing run must record its
 * own truth rather than leave a stale green file readable.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, problems: string[] }}}
 */
export async function runK05K04Consumers() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-K05-K04']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-K05-K04 is not an evidence proof in the frozen manifest')

  const problems = []
  const required = [...(spec.requiredChecks ?? [])].sort()
  const wired = [...REQUIRED_CHECKS].sort()
  for (const check of required)
    if (!REQUIRED_CHECKS.includes(check))
      problems.push(`no producer wired for required check ${JSON.stringify(check)}`)
  for (const check of REQUIRED_CHECKS)
    if (!required.includes(check))
      problems.push(`producer names check ${JSON.stringify(check)} the manifest does not require`)

  // The kit identities are configuration, and configuration drifts: check
  // them against the catalogue before anything is materialized under them.
  for (const kitId of ['K04', 'K05']) assertKitCatalogueSync(kitId)

  const identity = gitIdentity()
  const scratch = await mkdtemp(join('/tmp/opencode', 'archava-kit-consumers-'))
  const artifacts = await cutBundles(join(scratch, 'bundles'))
  let migrationInputs = {}
  record: try {
    // The fixture reads the applied ledgers from the scratch root. Copy them
    // once, byte-for-byte, before either consumer provisions its database.
    await copyMigrationInputs(scratch)
    migrationInputs = {
      packages: await treeDigest(join(scratch, 'packages')),
      owner: await treeDigest(join(scratch, 'rust/crates/host/migrations/owner')),
    }

    const k05 = await materializeConsumer(scratch, 'K05', artifacts)
    const k04 = await materializeConsumer(scratch, 'K04', artifacts)

    // The identical-artifacts check: digests over each consumer's `deps/`
    // tree, computed independently, must agree. A consumer whose copy was
    // edited in place — the fork the check exists for — diverges here. The
    // bundle digests recorded at cut time ride alongside as the shipped
    // bytes the two trees must serialize.
    const artifactsIdentical = k05.depsDigest === k04.depsDigest
    const codeIdentical = k05.codeDigest === k04.codeDigest
    const identical = artifactsIdentical && codeIdentical
    if (!artifactsIdentical)
      problems.push(
        `the consumers' artifact trees diverge (${k05.depsDigest.slice(0, 18)} vs ` +
          `${k04.depsDigest.slice(0, 18)} at ${k05.kit}/${k04.kit})`,
      )
    if (!codeIdentical)
      problems.push(
        `the consumers' witness code diverges (${k05.codeDigest.slice(0, 18)} vs ` +
          `${k04.codeDigest.slice(0, 18)} at ${k05.kit}/${k04.kit})`,
      )

    const checks = {
      'K05 consumer assembled and verified': {
        passed: k05.passed,
        tests: k05.tests,
        witnesses: k05.tested,
        crm: k05.crm,
      },
      'K04 consumer assembled and verified': {
        passed: k04.passed,
        tests: k04.tests,
        witnesses: k04.tested,
        crm: k04.crm,
      },
      'identical core/CRM artifact hashes': {
        passed: identical,
        bundles: Object.fromEntries(
          artifacts.map(({ crate, tarballSha256 }) => [crate, tarballSha256]),
        ),
        consumerDigests: { K05: k05.depsDigest, K04: k04.depsDigest },
        consumerCodeDigests: { K05: k05.codeDigest, K04: k04.codeDigest },
        migrations: migrationInputs,
        consumerCrm: { K05: k05.crm, K04: k04.crm },
      },
    }
    const allPassed = Object.values(checks).every((check) => check.passed) && problems.length === 0
    process.stdout.write(
      `     ${allPassed ? '✓' : '✗'} ${REQUIRED_CHECKS.map((name) => `${checks[name].passed ? '✓' : '✗'} ${name}`).join(' — ')}\n`,
    )
    await rm(scratch, { recursive: true, force: true })
    return { ok: allPassed, checks, problems, artifacts, candidate: identity }
  } catch (error) {
    problems.push(String(error.message ?? error))
    await rm(scratch, { recursive: true, force: true })
    const checks = Object.fromEntries(
      REQUIRED_CHECKS.map((name) => [name, { passed: false, witnesses: [] }]),
    )
    return { ok: false, checks, problems, artifacts, candidate: identity }
  }
}

// Guarded so importing this file in a test cannot start cargo.
if (import.meta.main) {
  const outcome = await runK05K04Consumers()
  const proofPath = join(root, 'dist/rust-k05-k04-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(outcome, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-k05-k04-proof.json\n`)
  if (outcome.ok) {
    process.stdout.write('PASS K05/K04 consumers — both business models verified against one artifact cut\n')
  } else {
    process.stdout.write(
      `FAIL K05/K04 consumers — ${outcome.problems.length} problem(s):\n` +
        outcome.problems.map((p) => `  - ${p}`).join('\n') +
        '\n',
    )
    process.exitCode = 1
  }
}
