/**
 * Measure the Rust-parity denominator from source, on both sides.
 *
 * ## Why this file exists
 *
 * The doc used to carry these numbers by hand: `4,715 production | 7,563`,
 * `4,649 / 2,279`, `3,830`, and a TypeScript side of `125,573`. Those four
 * figures could not all describe the same tree, and the TypeScript one could
 * not be reproduced by any counting rule I tried — raw tracked, tracked minus
 * tests, non-blank non-comment, at the working tree and at the baseline HEAD
 * all give answers between 114,388 and 152,023, none of them 125,573. A
 * denominator that no rule reproduces is not a measurement, and a ratio
 * computed against it is decoration.
 *
 * So the numbers here are derived, not typed. And because a line count is
 * meaningless without its rule, every output states the rule it used.
 *
 * ## The counting rule, stated precisely
 *
 * **Rust.** A line is *test* if it is inside a `#[cfg(test)]` block, or if its
 * whole file sits under a `tests/` directory (integration tests are not
 * `#[cfg(test)]`-gated, and a rule that misses them reports `archava-rates` as
 * having zero test lines — which is how this file's predecessor got that
 * crate wrong). Everything else is *production*.
 *
 * **TypeScript.** A line is *test* if its whole file sits under `test/`,
 * `tests/` or `__tests__/`, or its name matches `.test.`, `.spec.` or `-test.`.
 * Everything else is *production*.
 *
 * **Two figures per side.** `raw` is `wc -l`. `code` excludes blank lines and
 * line comments. Both are printed because the choice between them moves the
 * Rust total by 42% (7,563 raw vs 5,050 code), which is larger than the entire
 * Rust delta between two checkpoints — so a reader who cannot see which number
 * they are reading cannot compare anything.
 *
 * Usage: node tools/rust/count-lines.mjs [--rule raw|code]
 */

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

// Node globals this script touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here. Same convention as `tools/rust/parity/runtime.mjs`.
/* global URL, process */

const ROOT = fileURLToPath(new URL('../..', import.meta.url))
const RUST_CRATES = join(ROOT, 'rust', 'crates')

const TEST_SEGMENTS = new Set(['test', 'tests', '__tests__'])
const TS_TEST_NAME = /\.(test|spec)\.[cm]?[jt]sx?$|-test\.[cm]?[jt]sx?$/

/** True when any path segment is a test directory. */
function underTestDir(path) {
  return path.split(sep).some((segment) => TEST_SEGMENTS.has(segment))
}

function isTestFileName(name) {
  return TS_TEST_NAME.test(name)
}

/** Every file under `dir` matching `match`, recursively. */
function files(dir, match, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      files(full, match, out)
    } else if (match(entry)) {
      out.push(full)
    }
  }
  return out
}

/**
 * Split one file's lines into production and test.
 *
 * Brace matching rather than a regex because `mod tests {` blocks nest and a
 * regex cannot know where one ends. `underTests` short-circuits the whole
 * file: an integration test file is test code regardless of its contents.
 */
function tally(text, underTests) {
  let production = 0
  let test = 0
  let depth = 0
  let testDepth = 0
  // A `#[cfg(test)]` seen but not yet attached to the item it decorates.
  let pendingTestAttr = false
  let line = ''

  const flush = () => {
    const meaningful = line.trim()
    if (meaningful.length > 0 && !meaningful.startsWith('//')) {
      if (testDepth > 0 || underTests) test += 1
      else production += 1
    }
    line = ''
  }

  let index = 0
  while (index < text.length) {
    const char = text[index]

    if (char === '\n') {
      flush()
      index += 1
      continue
    }

    if (text.startsWith('//', index)) {
      const end = text.indexOf('\n', index)
      index = end === -1 ? text.length : end
      continue
    }
    if (text.startsWith('/*', index)) {
      const end = text.indexOf('*/', index + 2)
      index = end === -1 ? text.length : end + 2
      continue
    }
    if (text.startsWith('#[cfg(test)]', index)) {
      pendingTestAttr = true
      index += '#[cfg(test)]'.length
      continue
    }

    line += char

    if (char === '{') {
      depth += 1
      // Only the attribute's own next brace opens the test region; braces
      // inside a comment already skipped above cannot reach here.
      if (pendingTestAttr) {
        testDepth = depth
        pendingTestAttr = false
      }
      index += 1
      continue
    }
    if (char === '}') {
      depth -= 1
      if (testDepth > 0 && depth < testDepth) testDepth = 0
      index += 1
      continue
    }
    index += 1
  }
  flush()

  return { production, test }
}

/**
 * Strip comments, keeping one entry per source line.
 *
 * String literals are tracked first because this is not a cosmetic detail. The
 * workspace glob `packages/*` — quoted with single quotes in
 * `tools/scripts/structure.mjs`, the literal characters p-a-c-k-a-g-e-s, a
 * slash, then an asterisk — begins with the same two characters that open a
 * block comment. A stripper that only looks for comment openers reads it as one,
 * then discards everything until it finds a closing asterisk-slash, or to the end
 * of the file. In that one file the naive version silently ate 75 of 230 code
 * lines. Sixteen such literals exist across the tree.
 *
 * Writing that paragraph was harder than it looks and the reason is worth
 * keeping: spelling the closing delimiter inside a doc comment closes the doc
 * comment. I did exactly that once in this file, and eslint reported
 * "Expected an assignment or function call and instead saw an expression" at
 * line 156 — pointing at my own prose, which a `/**` opener had left stranded
 * as code. The hazard is silent until something parses it.
 *
 * So a `/*` inside a string is kept as code, and only a genuine opener — one
 * seen outside any string — starts a comment.
 *
 * Braces and quotes inside stripped regions go too. That is harmless here
 * because this function is only used for counting, never for parsing.
 */
function stripComments(text) {
  const out = []
  let line = ''
  let index = 0
  while (index < text.length) {
    const char = text[index]

    if (char === '\n') {
      out.push(line)
      line = ''
      index += 1
      continue
    }

    // A quoted run, consumed whole, so its contents cannot be mistaken for
    // comment delimiters. Escapes are honoured so `'\''` does not end early.
    if (char === "'" || char === '"' || char === '`') {
      line += char
      index += 1
      while (index < text.length) {
        const inner = text[index]
        if (inner === '\\') {
          line += text.slice(index, index + 2)
          index += 2
          continue
        }
        if (inner === char || inner === '\n') break
        line += inner
        index += 1
      }
      if (index < text.length && text[index] === char) {
        line += char
        index += 1
      }
      continue
    }

    if (text.startsWith('//', index)) {
      const end = text.indexOf('\n', index)
      index = end === -1 ? text.length : end
      continue
    }
    if (text.startsWith('/*', index)) {
      const end = text.indexOf('*/', index + 2)
      index = end === -1 ? text.length : end + 2
      continue
    }

    line += char
    index += 1
  }
  out.push(line)
  return out
}

/**
 * Count non-blank lines after comments are stripped.
 *
 * Block comments are stripped rather than only line comments, for the reason
 * `stripComments` gives. A blank line and a comment-only line both count as
 * nothing: they carry no implementation, and the point of the `code` rule is
 * implementation.
 */
function codeLines(text) {
  return stripComments(text).filter((line) => line.trim().length > 0).length
}

const rule = process.argv.includes('--rule')
  ? process.argv[process.argv.indexOf('--rule') + 1]
  : 'code'
if (rule !== 'code' && rule !== 'raw') {
  process.stderr.write(`unknown rule "${rule}"; expected "code" or "raw"\n`)
  process.exit(2)
}

// --- Rust side -------------------------------------------------------------

const rustFiles = files(RUST_CRATES, (entry) => entry.endsWith('.rs'))
let rustProduction = 0
let rustTest = 0
const rustRows = []

for (const crateDir of readdirSync(RUST_CRATES)
  .map((name) => join(RUST_CRATES, name))
  .filter((dir) => statSync(dir).isDirectory())
  .sort()) {
  const mine = rustFiles.filter((file) => file.startsWith(`${crateDir}${sep}`))
  let production = 0
  let test = 0
  for (const file of mine) {
    const text = readFileSync(file, 'utf8')
    if (rule === 'raw') {
      // `tests/rates.rs` is test code in full; only a `#[cfg(test)]` scan can
      // split an inline one, and this branch does not run it.
      const lines = text.split('\n').length - (text.endsWith('\n') ? 1 : 0)
      if (underTestDir(file)) test += lines
      else production += lines
    } else {
      const counted = tally(text, underTestDir(file))
      production += counted.production
      test += counted.test
    }
  }
  rustProduction += production
  rustTest += test
  rustRows.push(
    `  ${relative(ROOT, crateDir).padEnd(16)} production=${String(production).padStart(5)}  test=${String(test).padStart(5)}`,
  )
}

// --- TypeScript side -------------------------------------------------------
//
// Counted over the working tree with `node_modules` and `dist` excluded. Not
// `git ls-files`: the whole point is to compare against what actually exists,
// including files not yet committed, rather than against a filtered view.

const SKIP = new Set(['node_modules', 'dist', '.next', 'target', 'coverage'])
const TS_DIRS = ['packages', 'apps', 'tools']
  .map((name) => join(ROOT, name))
  .filter((dir) => statSync(dir).isDirectory())

let tsProduction = 0
let tsTest = 0
for (const dir of TS_DIRS) {
  for (const file of files(dir, (entry) => /\.(ts|tsx|mjs|js)$/.test(entry))) {
    if (file.split(sep).some((segment) => SKIP.has(segment))) continue
    const name = file.split(sep).pop()
    const isTest = underTestDir(file) || isTestFileName(name)
    const text = readFileSync(file, 'utf8')
    if (rule === 'raw') {
      const lines = text.split('\n').length - (text.endsWith('\n') ? 1 : 0)
      if (isTest) tsTest += lines
      else tsProduction += lines
    } else {
      const lines = codeLines(text)
      if (isTest) tsTest += lines
      else tsProduction += lines
    }
  }
}

const rustTotal = rustProduction + rustTest
const tsTotal = tsProduction + tsTest

process.stdout.write(`rule: ${rule}\n`)
process.stdout.write(
  `  Rust  test region = inside #[cfg(test)] block, or whole file under tests/\n`,
)
process.stdout.write(
  `  TS    test region = whole file under test/, tests/ or __tests__/, or name matching .test./.spec./-test.\n\n`,
)

// The `raw` rule cannot split a file that mixes production and inline test
// code, and every crate here does exactly that. Saying so beats printing a
// production figure that is silently the whole file.
if (rule === 'raw') {
  process.stdout.write(
    'NOTE: the raw rule counts whole files and cannot split an inline\n' +
      '`#[cfg(test)]` module, which every crate here uses. Rust production\n' +
      'below therefore includes its test lines, and the Rust share is\n' +
      'correspondingly overstated. Use --rule code for the split.\n\n',
  )
}

process.stdout.write('Rust:\n')
process.stdout.write(`${rustRows.join('\n')}\n`)
process.stdout.write(
  `  ${'total'.padEnd(16)} production=${String(rustProduction).padStart(5)}  test=${String(rustTest).padStart(5)}  both=${String(rustTotal).padStart(5)}\n\n`,
)

process.stdout.write('TypeScript:\n')
process.stdout.write(
  `  ${'total'.padEnd(16)} production=${String(tsProduction).padStart(5)}  test=${String(tsTest).padStart(5)}  both=${String(tsTotal).padStart(5)}\n\n`,
)

if (tsTotal === 0) {
  process.stdout.write('TypeScript total is zero; the ratio below is not meaningful.\n')
} else {
  const share = ((rustProduction / tsProduction) * 100).toFixed(2)
  process.stdout.write(
    `Rust production as a share of TypeScript production: ${rustProduction} / ${tsProduction} = ${share}%\n`,
  )
  process.stdout.write(
    `Same ratio with test code on both sides: ${rustTotal} / ${tsTotal} = ${((rustTotal / tsTotal) * 100).toFixed(2)}%\n`,
  )
}
