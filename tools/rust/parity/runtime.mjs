/**
 * Runtime selector for the experiment host.
 *
 * Default runtime is TypeScript, always. Rust is opt-in per process via
 * RUST_PILOT_RUNTIME=rust, and only the experiment host/adapter ever reads it.
 *
 * The rule this file exists to enforce: while measuring Rust, a missing or
 * broken addon must be LOUD. `requireRust()` throws rather than falling back,
 * so a Rust failure can never be silently reported as a TS result. The
 * product's TS path never loads this file at all — removing the whole
 * experiment leaves it working untouched.
 */

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

// Node globals this selector touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

const require = createRequire(import.meta.url)

/**
 * Where the locally built addon is copied after `cargo build --release`.
 *
 * `../../../rust/target/...` because this module sits at `tools/rust/parity/` and
 * the Cargo workspace is the product's `rust/` directory, not a sibling of the
 * harness. It previously read `../target/...` — correct from the pilot's
 * `Experiments/rust-pilot/node/`, where `target/` *was* one level up, and wrong
 * from here. Same defect class as D10: a relative path that resolved where the
 * code was written and not where it lives.
 */
const ADDON_PATH = fileURLToPath(
  new URL('../../../rust/target/release/libarchava_native.node', import.meta.url),
)

/**
 * @returns {"ts"|"rust"} the requested runtime, from the environment.
 */
export function requestedRuntime() {
  const value = process.env.RUST_PILOT_RUNTIME || 'ts'
  if (value !== 'ts' && value !== 'rust')
    throw new Error(`RUST_PILOT_RUNTIME must be "ts" or "rust", got ${JSON.stringify(value)}`)
  return value
}

/**
 * Load the Rust addon, failing loudly if it is absent.
 *
 * @returns {Record<string, Function>} the addon exports.
 * @throws when the addon has not been built — never falls back to TS.
 */
export function requireRust() {
  try {
    return require(ADDON_PATH)
  } catch (cause) {
    throw new Error(
      `Rust runtime requested but the native addon is unusable at ${ADDON_PATH}: ${cause?.message ?? cause}\n` +
        `Build it first: cargo build --release -p archava-native && cp target/release/libarchava_native.so target/release/libarchava_native.node`,
      { cause },
    )
  }
}

/**
 * True when the addon is present and loadable. Used by the harness to report
 * artifact state in the evidence manifest — NOT used to choose a fallback.
 */
export function rustAvailable() {
  try {
    requireRust()
    return true
  } catch {
    return false
  }
}
