/* global process, URL */
/**
 * Builds the Rust artifacts the parity harness and the cold-consumer proof need,
 * and nothing else.
 *
 * ## Why this exists rather than a shell one-liner
 *
 * The four Rust proofs were by-hand invocations, and "by hand" hid one real
 * defect. `tools/rust/parity/runtime.mjs:34` wants
 * `rust/target/release/libarchava_native.node`, but cargo emits
 * `libarchava_native.so` on Linux. Copying it was a step someone had to
 * remember; forgetting it produces a harness that throws, which at least is
 * loud, but the *ordering* problem is quieter. `verify-rust-consumers.mjs`
 * records `addonSha256` of the very same file, so a gate that rebuilt the
 * addon after copying would make the two proofs report two different artifact
 * hashes for one build. This file owns that order so both proofs read one
 * artifact.
 *
 * ## Why `.so` and `.node` are separate names
 *
 * `archava-native` is a `cdylib`. Cargo names the output for the platform, so
 * Linux gives `libarchava_native.so` and Node's `require` wants the `.node`
 * extension for an addon. Both are here; the copy is the seam, not a hack.
 *
 * ## What it deliberately does not do
 *
 * - It does not package anything into `dist/`, so it cannot collide with a
 *   release archive. Everything lands under `rust/target/`, which is gitignored
 *   and regenerable.
 * - It does not run `--release` for the whole workspace: only the addon is
 *   built, because only the addon is consumed by name outside cargo. The
 *   benchmark builds its own binary from its own manifest.
 * - It does not run `cargo package` without `--allow-dirty`. The entire `rust/`
 *   tree is untracked new work, so `cargo package` refuses outright without it
 *   and a gate that died there would have nothing to do with the code.
 */

import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** Absolute path to the repository root: `tools/rust/` up two. */
const root = fileURLToPath(new URL('../../', import.meta.url))
/** The cargo workspace, one level below the root. */
const workspace = join(root, 'rust')

/** The two crates that ship as consumable artifacts (G05). */
const SHIPPED_CRATES = ['archava-pricing', 'archava-rates', 'archava-assembly']

/**
 * Run one cargo command, streaming its output so a failure is legible in the
 * gate log rather than buffered and eventually summarized away.
 *
 * @param {string[]} args the cargo arguments
 */
function cargo(args) {
  process.stdout.write(`  $ cargo ${args.join(' ')}\n`)
  execFileSync('cargo', args, { cwd: workspace, stdio: 'inherit' })
}

/** Newest mtime across a file list, 0 for an empty one. */
function newestOf(files) {
  return files.reduce((newest, file) => Math.max(newest, statSync(file).mtimeMs), 0)
}

const addonSource = join(workspace, 'target/release/libarchava_native.so')
const addonTarget = join(workspace, 'target/release/libarchava_native.node')

// The addon first, because the copy below is what the parity harness loads and
// what the cold-consumer proof hashes. Building it second would leave the
// `.node` older than the `.so` it was copied from, which is not a correctness
// bug but is a staleness lie the mtime check in verify-rust-consumers.mjs would
// correctly refuse.
cargo(['build', '--offline', '--release', '--locked', '-p', 'archava-native'])

if (!existsSync(addonSource)) {
  throw new Error(
    `cargo did not produce ${addonSource}\n` +
      'A cdylib is named for the host platform; on a non-Linux target the extension differs.',
  )
}

// Copied only when it would actually change the bytes. The no-op path keeps the
// addon's mtime stable across gate re-runs, which matters because the staleness
// contract compares artifact mtime against source mtime — touching the artifact
// on every run would satisfy the contract while proving nothing.
if (!existsSync(addonTarget) || newestOf([addonSource]) > newestOf([addonTarget])) {
  cpSync(addonSource, addonTarget)
  process.stdout.write(`  copied addon → ${addonTarget.replace(`${root}/`, '')}\n`)
}

// Packaged after the addon, and with `--allow-dirty`, for the reason the header
// gives: the whole `rust/` tree is untracked candidate work, so cargo's clean-tree
// requirement would stop the package step before it looked at any crate.
cargo(['package', '--offline', '--allow-dirty', ...SHIPPED_CRATES.flatMap((name) => ['-p', name])])

const missing = SHIPPED_CRATES.filter(
  (name) => !existsSync(join(workspace, `target/package/${name}-0.1.0.crate`)),
)
if (missing.length > 0)
  throw new Error(`cargo package produced no tarball for: ${missing.join(', ')}`)

process.stdout.write('PASS rust artifacts built: release addon + packaged crates\n')
