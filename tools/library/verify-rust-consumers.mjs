/**
 * G05 / TEST-06: two independent cold consumers of the shipped Rust core.
 *
 * The claim this file is entitled to make is narrow, and it is worth stating
 * before the code: *two programs with no knowledge of the product tree build
 * against the artifacts and get a correct answer out of them*. That is what G05
 * asks for — "Two independent cold consumers use identical shipped core/domain
 * via supported minimal dependency closure" — and what TEST-06 asks for: "build
 * without product workspace links/private `.env`/owner DB/generated `dist`".
 *
 * What it does NOT claim, and must not be read as claiming:
 *
 * - Nothing about the TypeScript baseline. Whether the Rust engine agrees with
 *   the TS engine is `tools/rust/parity`'s claim, and these consumers
 *   deliberately do not re-check it. A consumer that recomputed parity would be
 *   a second harness, not a second consumer.
 * - Nothing about the rest of the domain. Two crates are packaged and consumed;
 *   the other five are not, and no percentage is implied by two over seven.
 * - Nothing about production behaviour. These are local builds on this machine
 *   — no deployment, no network, no provider call.
 *
 * ## What "cold" means here, concretely
 *
 * Three checks, each asserted rather than assumed. A run that quietly reached
 * into the workspace would otherwise produce a green result over a false claim,
 * and nothing in the evidence would reveal it.
 *
 * 1. **No product path in any resolved manifest.** Parsed, not grepped — see
 *    `assertNoProductPath`. G05's FAIL is "Import source workspace/absolute
 *    machine paths", so this is the check that separates a proof from a copy.
 * 2. **`--offline` throughout.** Nothing resolves from a registry, so a
 *    dependency found in the machine's `~/.cargo` cache cannot masquerade as a
 *    dependency a clean machine would have. The consumer's third-party closure
 *    is vendored by the same run.
 * 3. **The addon is copied, not linked.** Consumer B requires a *copy* of the
 *    addon inside its own scratch directory. A symlink would let the two
 *    consumers share the product's build output as if they were independent.
 *
 * ## The two consumers, and why they are not one route tested twice
 *
 * | | Consumer A (`crate/`) | Consumer B (`addon/host/`) |
 * |---|---|---|
 * | artifact | two `.crate` tarballs, unpacked | one `.node` addon, copied |
 * | seam | linked Rust crates | napi FFI boundary |
 * | language | Rust integration test | Node script |
 * | pricebook in | `include_str!`, compile time | `readFileSync`, run time |
 * | crates reached | `archava-pricing` only | both, through the addon |
 * | detects | a crate that will not link | a bridge that mangles JSON |
 *
 * They share the artifacts and nothing else. Each is blind to the other's
 * failure mode, which is the only sense in which a pair of consumers can be
 * called independent.
 *
 * ## Why this is not `verify-consumers.mjs`
 *
 * That script proves the *npm* cold-consumer path: it packs the library,
 * installs from tarballs, and verifies the installed release. This one covers
 * the Rust artifacts, which are not npm packages and cannot be installed by
 * one. Different surfaces; neither substitutes for the other.
 */

import { mkdtemp, cp, readFile, writeFile, mkdir, rm, readdir, stat } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve, dirname, isAbsolute, basename } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'

/* global process */

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const packageDir = join(root, 'rust/target/package')
const addonPath = join(root, 'rust/target/release/libarchava_native.node')
const consumers = join(root, 'examples/rust-consumers')
const evidence = join(root, 'dist/rust-consumer-proof.json')

/**
 * The crates this consumer pair consumes, named as `cargo package` names them.
 *
 * Three, not seven (and the addon bridge covers them on the napi seam). The
 * other four are not packaged for consumption and nothing here should suggest
 * they are. The list is written out rather than globed: a glob would silently
 * grow to cover a crate nobody had checked. `archava-assembly` joined this
 * list with P-PLANNING-CONFORMANCE's third check — the cold-consumer planning
 * proof — so the planning engine is consumed the way a customer consumes it
 * rather than only asserted about inside the workspace.
 */
const shippedCrates = [
  { crate: 'archava-pricing', tarball: 'archava-pricing-0.1.0.crate' },
  { crate: 'archava-rates', tarball: 'archava-rates-0.1.0.crate' },
  { crate: 'archava-assembly', tarball: 'archava-assembly-0.1.0.crate' },
]

/**
 * Run cargo in `cwd`, offline, with an optional per-call environment override.
 *
 * The override exists for one caller: consumer A's test run points `CARGO_HOME`
 * at an empty directory, which is what turns "the build used `vendor/`" from an
 * assumption into a fact — with the source replacement configured and no
 * registry cache to fall back on, a dependency that did not come from `vendor/`
 * is a hard failure rather than a silently warmer build.
 */
const cargo = (args, cwd, label, extraEnv = {}) => {
  const result = spawnSync('cargo', args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, ...extraEnv },
    // Offline is enforced in the environment and not only on the command line,
    // so a cargo subcommand that spawns its own resolver cannot reach out.
  })
  if (result.status !== 0)
    throw new Error(`${label} failed (exit ${result.status}):\n${result.stdout}\n${result.stderr}`)
  return result.stdout
}

async function sha256(file) {
  const digest = createHash('sha256')
  digest.update(await readFile(file))
  return digest.digest('hex')
}

/** Newest modification time among `files`, as milliseconds since the epoch. */
async function newestOf(files) {
  let newest = null
  for (const file of files) {
    const mtime = (await stat(file)).mtimeMs
    if (newest === null || mtime > newest) newest = mtime
  }
  return newest
}

/**
 * Which artifact was built from which crate.
 *
 * Spelled out rather than derived from the filename. The two naming schemes do
 * not agree — `archava-pricing` ships as `archava-pricing-0.1.0.crate`, but
 * `archava-native` ships as `libarchava_native.node`, so no single prefix rule
 * maps one to the other. A regex that "mostly" works here fails on the addon
 * specifically, which is the artifact the whole proof rests on.
 */
const artifactSourceGroup = [
  { tarball: 'archava-pricing-0.1.0.crate', group: 'archava-pricing' },
  { tarball: 'archava-rates-0.1.0.crate', group: 'archava-rates' },
  { tarball: 'archava-assembly-0.1.0.crate', group: 'archava-assembly' },
  { addon: 'libarchava_native.node', group: 'archava-native' },
]

/** The crates whose sources each artifact was built from, by directory. */
function sourceGroups() {
  return [
    { group: 'archava-pricing', directory: 'pricing' },
    { group: 'archava-rates', directory: 'rates' },
    { group: 'archava-assembly', directory: 'assembly' },
    { group: 'archava-native', directory: 'native' },
  ]
}

/**
 * TEST-06 names *stale result* as a required failure, and nothing else catches
 * it. A green consumer run against an artifact built before the last source edit
 * proves that the artifact still works — which is a weaker claim than the one
 * being made, and would read as the stronger one.
 *
 * So each artifact is compared against its own sources, by mtime. That is the
 * only comparison available: `rust/target/` is gitignored, so there is no commit
 * timestamp to compare against and no hash chain from source to `.node` that
 * cargo itself records.
 *
 * One comparison per artifact, not one comparison for the set. The set-level
 * version was written first and was wrong in the way that matters: taking the
 * newest artifact and comparing it to the newest source means a single stale
 * tarball is masked by a fresh sibling, and the gate passes on exactly the case
 * it exists to catch. Per-artifact is also the honest shape of the check, since
 * a stale `pricing` crate says nothing about `rates`.
 */
async function staleArtifacts(artifacts) {
  const byGroup = new Map()
  for (const { group, directory } of sourceGroups()) {
    // `Cargo.toml` counts as a source: bumping a dependency version there changes
    // what gets built without editing a line of `.rs`, and the artifact is stale
    // for exactly that reason. `tree()` returns `{name, path}` pairs, which is
    // why the walk's results are mapped to paths before being compared.
    const files = (await tree(join(root, `rust/crates/${directory}`))).map((entry) => entry.path)
    byGroup.set(group, await newestOf(files))
  }
  const stale = []
  for (const artifact of artifacts) {
    const name = basename(artifact)
    const mapping = artifactSourceGroup.find(
      (entry) => entry.tarball === name || entry.addon === name,
    )
    if (!mapping) throw new Error(`no source group for artifact ${artifact}`)
    const sourceAt = byGroup.get(mapping.group)
    const builtAt = (await stat(artifact)).mtimeMs
    if (builtAt < sourceAt) stale.push({ artifact: name, group: mapping.group, builtAt, sourceAt })
  }
  return stale
}

/** Every file under `directory`, relative and sorted, for a stable walk. */
async function tree(directory, prefix = '', out = []) {
  const entries = await readdir(directory, { withFileTypes: true })
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const file = join(directory, entry.name)
    const name = `${prefix}${entry.name}`
    if (entry.isDirectory()) await tree(file, `${name}/`, out)
    else out.push({ name, path: file })
  }
  return out
}

/**
 * The G05 check: no resolved manifest may depend on a path inside the product.
 *
 * Parsed, not grepped, and the reason is not pedantry. A manifest that
 * *mentions* the workspace — a comment explaining that it deliberately avoids
 * one, which this repo's own consumer carries — satisfies a substring search
 * while being entirely cold. A substring check would fail a correct consumer,
 * and a check that fails correct consumers gets disabled the first time it
 * inconveniences someone.
 *
 * So this reads every `path = "..."` value in every resolved `Cargo.toml`,
 * resolves it against the manifest that declared it, and rejects anything
 * landing inside the repository. The lockfiles are scanned separately for the
 * repository's absolute path, which cannot appear in an honest lock and is not
 * the kind of string a comment would trip over.
 */
async function assertNoProductPath(directory, label) {
  const offenders = []
  for (const { path: file } of await tree(directory)) {
    if (/Cargo\.toml$/.test(file)) {
      const text = await readFile(file, 'utf8')
      // Strip comments before reading values, so a documented avoidance of the
      // workspace cannot be mistaken for a dependency on it.
      const live = text
        .split('\n')
        .map((line) => line.replace(/(^|\s)#.*$/, ''))
        .join('\n')
      for (const match of live.matchAll(/path\s*=\s*"([^"]+)"/g)) {
        const declared = match[1]
        if (declared.includes('/../')) offenders.push(`${file}: escapes its own root`)
        else if (declared.startsWith(root) || (isAbsolute(declared) && declared.startsWith(root)))
          offenders.push(`${file}: absolute product path ${declared}`)
        else if (declared.split('/').includes('crates'))
          offenders.push(`${file}: points into the product tree at ${declared}`)
      }
    } else if (/Cargo\.lock$/.test(file)) {
      // An absolute path in a lock is not something cargo writes.
      if ((await readFile(file, 'utf8')).includes(root))
        offenders.push(`${file}: absolute product path`)
    }
  }
  if (offenders.length > 0)
    throw new Error(`${label} reached into the product tree:\n${offenders.join('\n')}`)
}

/** Materialize the two packaged crates as path dependencies, unpacked. */
async function materializeCrates(consumer) {
  const deps = join(consumer, 'deps')
  await mkdir(deps, { recursive: true })
  const unpacked = []
  for (const { crate, tarball } of shippedCrates) {
    const source = join(packageDir, tarball)
    const digest = await sha256(source)
    unpacked.push({ crate, tarball, tarballSha256: digest })
    // Unpacked with tar rather than by cargo, because the point is that these
    // are *files* handed to a consumer. `cargo package` has already resolved
    // each crate's `workspace = true` fields into a standalone manifest, so
    // every extracted directory is self-contained: no parent workspace, no
    // inherited version, nothing to walk up to.
    const result = spawnSync('tar', ['xzf', source, '-C', deps], { encoding: 'utf8' })
    if (result.status !== 0) throw new Error(`could not unpack ${tarball}: ${result.stderr}`)
  }
  return unpacked
}

/**
 * Vendor the consumer's third-party closure, from the committed pin only.
 *
 * `cargo vendor` walks the consumer's own lockfile and copies every registry
 * crate it needs into `vendor/`. Three things make the closure the claim says
 * it is:
 *
 * - `--locked` on both commands. The consumer's `Cargo.lock` is committed as
 *   the closure's pin (ADR-R10, shared with `pnpm rust:bootstrap`'s locked
 *   fetch). `generate-lockfile --offline --locked` *asserts* that pin rather
 *   than re-resolving it — a manifest that moved ahead of the lock fails here,
 *   with cargo's own message, instead of vendoring a closure the bootstrap
 *   never warmed;
 * - `--offline`, so the closure can only come from this machine's cache and
 *   the coldness claim is actually exercised;
 * - the written `.cargo/config.toml` below, which is what makes `vendor/` the
 *   *only* source the consumer can build from. Measured, not assumed: with an
 *   empty `CARGO_HOME` and no replacement config, `cargo test --offline`
 *   fails with "no matching package named serde found" — before this write
 *   existed, the proof passed only because it quietly built out of the
 *   machine's registry cache, the exact dependency the ADR denies.
 */
async function vendorClosure(consumer) {
  const manifest = await readFile(join(consumer, 'Cargo.toml'), 'utf8')
  for (const { crate } of shippedCrates)
    if (!manifest.includes(`path = "deps/${crate}-0.1.0"`))
      throw new Error(`the consumer no longer consumes the shipped ${crate}`)
  cargo(
    ['generate-lockfile', '--offline', '--locked'],
    join(consumer),
    'consumer lockfile (the committed Cargo.lock pin)',
  )
  const out = cargo(
    ['vendor', '--offline', '--locked', join(consumer, 'vendor')],
    consumer,
    'cargo vendor',
  )
  // `cargo vendor` prints the configuration block it would like; printing is
  // all it does. We write the block ourselves, into the scratch consumer, and
  // the consumer tree itself stays free of a committed config (`.cargo/` is
  // generated, like `vendor/` and `deps/` — see the crate's .gitignore).
  if (!/\[source\.vendored-sources\]/.test(out))
    throw new Error('cargo vendor did not report a vendored-sources configuration')
  await mkdir(join(consumer, '.cargo'), { recursive: true })
  await writeFile(
    join(consumer, '.cargo/config.toml'),
    '# Written by vendorClosure (tools/library/verify-rust-consumers.mjs).\n' +
      '[source.crates-io]\n' +
      'replace-with = "vendored-sources"\n' +
      '\n' +
      '[source.vendored-sources]\n' +
      'directory = "vendor"\n',
  )
}

const scratch = await mkdtemp(join(tmpdir(), 'archava-rust-consumers-'))
const record = { consumers: [], identicalArtifacts: true, offlineOnly: true }

try {
  // ------------------------------------------------- staleness, before anything else
  // TEST-06 lists "stale result" among the failures that must appear. This is
  // where it appears. An artifact built before the last source edit is not the
  // artifact the consumers are being asked to prove, and a consumer run against
  // it passes anyway — which is precisely why it needs its own gate rather than
  // trust in the build step having been remembered.
  const artifacts = [...shippedCrates.map(({ tarball }) => join(packageDir, tarball)), addonPath]
  for (const artifact of artifacts) {
    // Named explicitly, with the remediation, because "ENOENT" on its own sends
    // an operator looking for the wrong problem. The Rust workspace is a build
    // step, and on a clean checkout it has not run.
    if (!existsSync(artifact))
      throw new Error(
        `the shipped artifact is missing: ${artifact}\nBuild it first: cargo package --offline -p archava-pricing -p archava-rates -p archava-assembly (into rust/target/package), and cargo build --release (into rust/target/release).`,
      )
  }
  const stale = await staleArtifacts(artifacts)
  if (stale.length > 0) {
    const details = stale
      .map(
        (entry) =>
          `  ${entry.artifact} built ${new Date(entry.builtAt).toISOString()}, ` +
          `but ${entry.group} sources changed ${new Date(entry.sourceAt).toISOString()}`,
      )
      .join('\n')
    throw new Error(
      `the shipped artifacts are older than the sources they were built from:\n${details}\nRe-run cargo package and cargo build --release, then re-run this proof.`,
    )
  }
  record.artifactFreshness = {
    everyArtifactNewerThanItsSources: true,
    artifactsChecked: artifacts.length,
  }

  // The scratch tree mirrors the repository's own layout exactly:
  // `<scratch>/examples/rust-consumers/...` and `<scratch>/config/`.
  //
  // This is not tidiness. Both consumers locate the shipped pricebook by
  // climbing four levels from their own file, which is correct from their
  // location *in the repository* — `include_str!` resolves relative to the
  // source file (D10) and Node's URL resolution likewise. Laying the scratch
  // tree out any shallower, and the consumer no longer compiles against the
  // file it was written against; laying it out deeper, and it silently reads a
  // different one. Keeping the depth identical is what lets the committed
  // consumer be *run* against the scratch copy unmodified, which is the whole
  // claim: the same source, built against the same input, from a different tree.
  await cp(join(consumers, '../..'), join(scratch, 'examples'), {
    recursive: true,
    // Only the consumers are copied; the rest of `examples/` is irrelevant.
    filter: (source) => source === consumers || source.startsWith(`${consumers}/`),
  })
  await mkdir(join(scratch, 'config'), { recursive: true })
  await writeFile(
    join(scratch, 'config/pricing.v1.json'),
    await readFile(join(root, 'config/pricing.v1.json')),
  )

  // ---------------------------------------------------------------- consumer A
  const crateConsumer = join(scratch, 'examples/rust-consumers/crate')
  await cp(join(consumers, 'crate'), crateConsumer, { recursive: true })
  const unpacked = await materializeCrates(crateConsumer)
  await vendorClosure(crateConsumer)
  await assertNoProductPath(crateConsumer, 'consumer A')
  // The vendored-sources claim, proven rather than asserted: an empty
  // `CARGO_HOME` means no registry cache exists to fall back on. Every
  // third-party crate the build resolves must come out of `vendor/`, through
  // the `.cargo/config.toml` vendorClosure wrote — a dependency that slipped
  // past vendoring fails this line instead of passing on machine warmth.
  const emptyCargoHome = join(scratch, 'empty-cargo-home')
  await mkdir(emptyCargoHome, { recursive: true })
  const testOutput = cargo(['test', '--offline', '--locked'], crateConsumer, 'consumer A tests', {
    CARGO_HOME: emptyCargoHome,
  })
  const tests = Number(/test result: ok\. (\d+) passed;/.exec(testOutput)?.[1] ?? 0)
  // The named tests, parsed the same way the pg suite producers parse their
  // harness output: a check graded by name is produced by the test with that
  // name, not by a count a deleted test would not change.
  const namedTests = []
  for (const line of testOutput.split('\n')) {
    const match = /^test (\S+) \.\.\. ok\b/.exec(line)
    if (match) namedTests.push(match[1])
  }
  // An empty suite would otherwise match this regex's absence and read as zero
  // tests passing, which is not a result anyone should file as green.
  if (tests === 0) throw new Error('consumer A ran no tests')
  const lock = await readFile(join(crateConsumer, 'Cargo.lock'), 'utf8')
  for (const { crate } of shippedCrates)
    if (!new RegExp(`name = "${crate}"\\nversion = "0\\.1\\.0"`).test(lock))
      throw new Error(`consumer A did not resolve ${crate} from the shipped artifact`)
  record.consumers.push({
    consumer: 'crate',
    mode: 'linked-rust-crates',
    tests,
    namedTests,
    vendoredCrates: (await readdir(join(crateConsumer, 'vendor'))).length,
    artifacts: unpacked,
    coldOffline: true,
    // True only because the test run above used an empty CARGO_HOME with the
    // source replacement in force: the closure came from `vendor/` and from
    // nowhere else.
    vendoredSourceOnly: true,
    productTreeReached: false,
  })
  process.stdout.write(
    `PASS consumer A (Rust crates): ${tests} tests green, offline, no product path.\n`,
  )

  // ---------------------------------------------------------------- consumer B
  const addonConsumer = join(scratch, 'examples/rust-consumers/addon')
  await cp(join(consumers, 'addon'), addonConsumer, { recursive: true })
  const copiedAddon = join(addonConsumer, 'host/libarchava_native.node')
  // Copied, not symlinked. A symlink would let consumer B pass while reaching
  // straight into the product's target directory — a product path by another
  // name, and one that would survive any manifest check.
  await cp(addonPath, copiedAddon)
  await assertNoProductPath(addonConsumer, 'consumer B')
  const addonSha = await sha256(addonPath)
  if ((await sha256(copiedAddon)) !== addonSha)
    throw new Error('the copied addon is not the shipped artifact')
  const run = spawnSync(process.execPath, [join(addonConsumer, 'host/verify.mjs')], {
    encoding: 'utf8',
    env: { ...process.env, ARCHAVA_ADDON: copiedAddon },
  })
  if (run.status !== 0)
    throw new Error(`consumer B failed (exit ${run.status}):\n${run.stdout}\n${run.stderr}`)
  const passed = run.stdout.split('\n').filter((line) => line.startsWith('PASS ')).length
  if (passed === 0) throw new Error('consumer B reported no checks — that is not a pass')
  record.consumers.push({
    consumer: 'addon',
    mode: 'napi-bridge-from-node',
    checks: passed,
    addonSha256: addonSha,
    addonCopied: true,
    coldOffline: true,
    productTreeReached: false,
  })
  process.stdout.write(
    `PASS consumer B (napi bridge): ${passed} checks green, addon copied, no product path.\n`,
  )

  // --------------------------------------------------------- cross-consumer gate
  // The two consumers are not compared against each other: their artifacts,
  // languages and seams differ by design. What is compared is that both started
  // from the *same* shipped core — the same pricebook bytes, and two crates whose
  // tarball hashes are recorded per consumer. That is the "identical shipped
  // core/domain" half of G05's PASS wording, and it is the half a single
  // consumer cannot establish on its own.
  record.pricebookSha256 = await sha256(join(root, 'config/pricing.v1.json'))
  if (record.consumers.some((entry) => entry.productTreeReached))
    throw new Error('a consumer reached the product tree')
  record.node = process.version
  record.cargo = spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim()
  await mkdir(dirname(evidence), { recursive: true })
  await writeFile(evidence, `${JSON.stringify(record, null, 2)}\n`)
  process.stdout.write(`PASS two independent cold consumers; evidence at ${evidence}\n`)
} finally {
  // The scratch tree holds the unpacked crates and the whole vendored closure.
  // It is disposable by construction; the evidence file records what happened
  // inside it, including the hashes that would let anyone re-check it.
  await rm(scratch, { recursive: true, force: true })
}
