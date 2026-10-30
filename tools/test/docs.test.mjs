import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { checkDocs, markdownLinks, isMaintained } from '../check-docs.mjs'

function fixture(t, files) {
  const root = mkdtempSync(join(tmpdir(), 'archava-doc-links-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  for (const [name, body] of Object.entries(files)) {
    const path = join(root, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, body)
  }
  return root
}

test('Markdown extracts balanced and angle destinations, skips code examples', () => {
  const links = markdownLinks('[one](assets/a(b).png) [two](<folder with spaces/a.md>)\n' +
    '```md\n[fake](missing)\n```\n`[example](missing)`\n[ref]: <target.md>\n')
  assert.deepEqual(links.map((x) => x.href), ['assets/a(b).png', 'folder with spaces/a.md', 'target.md'])
})
test('missing source links and heading fragments fail while valid headings pass', (t) => {
  const root = fixture(t, { 'docs/README.md': '[ok](guide.md#setup) [bad](guide.md#absent) [missing](gone.rs)', 'docs/guide.md': '# Setup\n' })
  const result = checkDocs(root)
  assert.equal(result.checkedLinks, 3)
  assert.deepEqual(result.problems.map((p) => p.reason), ['heading does not exist', 'target does not exist'])
})
test('archives remain frozen while their maintained indexes are checked', (t) => {
  const root = fixture(t, { 'docs/archive/history/original.md': '[old](gone.ts)', 'docs/archive/history/README.md': '[record](original.md)', 'docs/archive/audits/evidence/proof.md': '[old](gone.ts)' })
  assert.equal(isMaintained('docs/archive/rust/DECISIONS.md'), false)
  assert.equal(isMaintained('docs/archive/reference/CRM_RUNTIME.md'), false)
  assert.equal(isMaintained('docs/archive/reference/README.md'), true)
  assert.equal(isMaintained('docs/archive/consolidation.md'), true)
  assert.deepEqual(checkDocs(root), { files: 1, checkedLinks: 1, problems: [] })
})
test('repository entry guides cannot retain links to removed document locations', (t) => {
  const root = fixture(t, { 'docs/README.md': '# Docs\n', 'README.md': '[old architecture](docs/ARCHITECTURE.md)' })
  const result = checkDocs(root)
  assert.equal(result.problems.length, 1)
  assert.equal(result.problems[0].file, 'README.md')
})
test('machine paths and repository escapes fail, web URLs are outside scope', (t) => {
  const root = fixture(t, { 'docs/README.md': '[local](file:///home/user/x.md) [absolute](/home/user/x.md) [escape](../../outside.md) [web](https://example.com)' })
  assert.equal(checkDocs(root).problems.length, 3)
})
test('encoded spaces, same-page anchors, duplicate headings and source line annotations work', (t) => {
  const root = fixture(t, { 'docs/README.md': '# Setup\n# Setup\n[again](#setup-1) [space](a%20b.md#api-name) [source](../source.rs#L4-L9)', 'docs/a b.md': '# API `name`\n', 'source.rs': '// code' })
  assert.equal(checkDocs(root).problems.length, 0)
})
