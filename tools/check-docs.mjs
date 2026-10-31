/** Local navigation gate for maintained docs. Frozen records retain original paths. */
import { existsSync, readFileSync, readdirSync, realpathSync, statSync } from 'node:fs'
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const archiveIndexes = new Set([
  'archive/README.md', 'archive/audits/README.md', 'archive/rust/README.md',
  'archive/reference/README.md', 'archive/history/README.md',
  'archive/history/design/README.md', 'archive/history/plans/README.md',
  'archive/history/progress/README.md',
  'archive/history/subsystem-drafts-2026-10-29/README.md',
  'archive/product-research/README.md',
])
const entryGuides = [
  'README.md', 'CONTRIBUTING.md', 'SECURITY.md', 'config/README.md',
  'deploy/README.md', 'migrations/README.md', 'tools/README.md',
]

export function isMaintained(path) {
  const name = path.replaceAll('\\', '/').replace(/^docs\//, '')
  if (!name.endsWith('.md')) return false
  // Archive-root overviews remain navigable; frozen bodies live in its collections.
  if (name.startsWith('archive/')) return name.split('/').length === 2 || archiveIndexes.has(name)
  return true
}

function prose(text) {
  let fence
  return text.split('\n').map((line) => {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1]
    if (marker && !fence) { fence = marker; return '' }
    if (fence) {
      if (marker?.[0] === fence[0] && marker.length >= fence.length) fence = undefined
      return ''
    }
    return line
  }).join('\n').replace(/(`+)([^\n]*?)\1/g, (match) => ' '.repeat(match.length))
}

export function markdownLinks(text) {
  const body = prose(text)
  const links = []
  const add = (href, offset) => links.push({ href, line: body.slice(0, offset).split('\n').length })
  for (const match of body.matchAll(/!?\[[^\]\n]*\]\(/g)) {
    let pos = match.index + match[0].length
    const start = pos
    let depth = 1
    let angle = body[pos] === '<'
    while (pos < body.length && depth) {
      const char = body[pos++]
      if (char === '\\') { pos++; continue }
      if (angle) { if (char === '>') angle = false; continue }
      if (char === '(') depth++
      if (char === ')') depth--
    }
    if (depth) continue
    const value = body.slice(start, pos - 1).trim()
    const href = value.startsWith('<') ? value.slice(1, value.indexOf('>')) : value.split(/\s+["']/)[0]
    add(href, match.index)
  }
  // Definitions are checked even when their reference is not used on the page.
  for (const match of body.matchAll(/^ {0,3}\[[^\]\n]+\]:\s*(?:<([^>]+)>|(\S+))/gm)) add(match[1] ?? match[2], match.index)
  for (const match of body.matchAll(/<(file:\/\/[^>]+)>/g)) add(match[1], match.index)
  return links
}

function headings(text) {
  const ids = new Set()
  const used = new Map()
  // Inline code is meaningful in headings; only fenced content is removed here.
  let fence
  const lines = text.split('\n').filter((line) => {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1]
    if (marker && !fence) { fence = marker; return false }
    if (fence) {
      if (marker?.[0] === fence[0] && marker.length >= fence.length) fence = undefined
      return false
    }
    return true
  })
  for (const line of lines) {
    for (const match of line.matchAll(/\bid=["']([^"']+)["']/g)) ids.add(match[1])
    const heading = /^ {0,3}#{1,6}\s+(.+?)(?:\s+#+)?$/.exec(line)?.[1]
    if (!heading) continue
    const slug = heading.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1').replace(/<[^>]+>/g, '')
      .toLowerCase().replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu, '').replace(/\s/g, '-')
    let id = slug
    let suffix = used.get(slug) ?? 0
    while (ids.has(id)) id = `${slug}-${++suffix}`
    used.set(slug, suffix); ids.add(id)
  }
  return ids
}

export function checkDocs(root = REPO_ROOT) {
  root = realpathSync(root)
  const docs = resolve(root, 'docs')
  const files = []
  function walk(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = resolve(dir, entry.name)
      if (entry.isDirectory()) walk(path)
      else if (entry.isFile() && isMaintained(relative(docs, path))) files.push(path)
    }
  }
  walk(docs)
  // Entry points outside docs must follow the same canonical destinations.
  for (const name of entryGuides) {
    const path = resolve(root, name)
    if (existsSync(path)) files.push(path)
  }
  const problems = []
  let checkedLinks = 0
  for (const file of files.sort()) {
    const text = readFileSync(file, 'utf8')
    for (const { href: encoded, line } of markdownLinks(text)) {
      const problem = (reason) => problems.push({ file: relative(root, file), line, href: encoded, reason })
      let href
      try { href = decodeURIComponent(encoded.replace(/\\([_() ])/g, '$1')) } catch { problem('invalid URL encoding'); continue }
      if (/^file:/i.test(href) || isAbsolute(href) || /^[A-Za-z]:[\\/]/.test(href)) {
        problem('use a portable repository-relative link'); continue
      }
      if (/^[a-z][a-z\d+.-]*:/i.test(href) || href.startsWith('//')) continue
      checkedLinks++
      const [location, fragment] = href.split('#', 2)
      const target = resolve(dirname(file), location.split('?', 1)[0] || '.')
      const dest = location ? target : file
      const rel = relative(root, dest)
      if (rel === '..' || rel.startsWith(`..${sep}`)) { problem('target is outside the repository'); continue }
      if (!existsSync(dest)) { problem('target does not exist'); continue }
      const real = relative(root, realpathSync(dest))
      if (real === '..' || real.startsWith(`..${sep}`)) { problem('target resolves outside the repository'); continue }
      if (fragment && dest.endsWith('.md') && statSync(dest).isFile() && !headings(readFileSync(dest, 'utf8')).has(fragment)) problem('heading does not exist')
    }
  }
  return { files: files.length, checkedLinks, problems }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = checkDocs()
  for (const p of result.problems) process.stderr.write(`${p.file}:${p.line}: ${p.reason}: ${p.href}\n`)
  process.stdout.write(`Documentation: ${result.files} maintained files, ${result.checkedLinks} local links, ${result.problems.length} problems\n`)
  process.exitCode = result.problems.length ? 1 : 0
}
