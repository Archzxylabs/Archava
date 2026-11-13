import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { checkModules, registryProblems } from '../check-modules.mjs'

const descriptor = () => ({
  id: 'kiosk', label: 'Kiosk', namespace: 'archava_kiosk', ledger: 'kiosk-pg', source: 'kiosk_sales', suite: 'erp_kiosk',
  spec: 'docs/modules/KIOSK_MODULE.md', readPath: '/erp/kiosk/sales', uiPath: '/app/erp?module=kiosk', control: 'module-kiosk-create',
  agent: { rank: 1, keywords: ['kiosk'], collections: ['sales'] },
})

/** A minimal tree in which one module is completely registered. */
function fixture(t, modules = [descriptor()]) {
  const root = mkdtempSync(join(tmpdir(), 'archava-modules-test-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const write = (path, content) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), content) }
  write('config/modules.v1.json', JSON.stringify({ schemaVersion: 1, modules }))
  write('config/operator-actions.v1.json', JSON.stringify({ schemaVersion: 1, modules: [{ id: 'kiosk', namespace: 'archava_kiosk', actions: [] }] }))
  write('migrations/kiosk-pg/001_kiosk.sql', '')
  write('migrations/manifest.v1.json', JSON.stringify({ schemaVersion: 1, ledgers: [{ namespace: 'archava_kiosk', migrations: [{ id: '001_kiosk', path: 'migrations/kiosk-pg/001_kiosk.sql', sha256: '0' }] }] }))
  write('crates/slice/src/bin/archava_server.rs', 'include_bytes!("../../../../migrations/kiosk-pg/001_kiosk.sql");\n("archava_kiosk", kiosk.as_slice()),\n')
  write('crates/pg-fixture/src/lib.rs', 'match namespace {\n    "archava_kiosk" => "kiosk-pg",\n    _ => "commerce-pg",\n}\n')
  for (const layer of ['domain', 'adapters', 'host']) {
    write(`crates/${layer}/src/erp/kiosk_sales.rs`, layer === 'host' ? 'pub(crate) async fn command(\n) {}\npub async fn status() {}\n' : '')
    write(`crates/${layer}/src/erp/mod.rs`, 'pub mod kiosk_sales;\n')
  }
  write('crates/host/src/routes/erp.rs', 'for_each_erp_module!(mount)\n.route("/erp/kiosk/sales", get(crate::erp::kiosk_sales::sales))\n')
  write('crates/host/src/extractors.rs', '("erp/kiosk_sales.rs", include_str!("erp/kiosk_sales.rs")),\n')
  write('crates/host/src/business.rs', 'let response = for_each_erp_module!(dispatch);\n')
  write('crates/host/build.rs', 'const REGISTRY: &str = "../domain/embedded/modules.v1.json";\n')
  write('crates/host/tests/erp_kiosk.rs', '')
  write('docs/modules/KIOSK_MODULE.md', '# Kiosk\n')
  const edit = (path, change) => writeFileSync(join(root, path), change(readFileSync(join(root, path), 'utf8')))
  return { root, write, edit }
}

test('a completely registered module passes every registration point', (t) => {
  assert.deepEqual(registryProblems({ schemaVersion: 1, modules: [{ ...descriptor(), tab: 'Kiosk', typedDetail: true }] }), [])
  const { root } = fixture(t)
  const result = checkModules(root)
  assert.deepEqual(result.problems, [])
  assert.equal(result.modules.length, 1)
  assert.ok(result.modules[0].checks.length >= 16)
  assert.ok(result.modules[0].checks.every((check) => check.ok))
})

test('each missing registration point is named with the file to change', async (t) => {
  const cases = [
    ['form catalogue', (f) => f.write('config/operator-actions.v1.json', JSON.stringify({ modules: [] })), /kiosk: form catalogue — config\/operator-actions\.v1\.json/],
    ['form schema mismatch', (f) => f.write('config/operator-actions.v1.json', JSON.stringify({ modules: [{ id: 'kiosk', namespace: 'archava_other' }] })), /kiosk: form catalogue/],
    ['manifest', (f) => f.write('migrations/manifest.v1.json', JSON.stringify({ schemaVersion: 1, ledgers: [] })), /kiosk: migration manifest — migrations\/manifest\.v1\.json/],
    ['manifest path outside ledger', (f) => f.edit('migrations/manifest.v1.json', (s) => s.replace('migrations/kiosk-pg/', 'migrations/other-pg/')), /kiosk: migration manifest/],
    ['server embed', (f) => f.write('crates/slice/src/bin/archava_server.rs', ''), /kiosk: server embed — .*include_bytes! migrations\/kiosk-pg\/001_kiosk\.sql/],
    ['fixture ledger', (f) => f.edit('crates/pg-fixture/src/lib.rs', (s) => s.replace('"archava_kiosk" => "kiosk-pg",', '')), /kiosk: fixture ledger — crates\/pg-fixture\/src\/lib\.rs/],
    ['adapter declaration', (f) => f.write('crates/adapters/src/erp/mod.rs', '// pub mod kiosk_sales_extra;\n'), /kiosk: adapters declaration — crates\/adapters\/src\/erp\/mod\.rs: declare `pub mod kiosk_sales;`/],
    ['command handler', (f) => f.write('crates/host/src/erp/kiosk_sales.rs', 'pub(crate) async fn kiosk_command() {}\npub async fn status() {}\n'), /kiosk: command handlers — crates\/host\/src\/erp\/kiosk_sales\.rs: define `pub\(crate\) async fn command`;/],
    ['status handler', (f) => f.write('crates/host/src/erp/kiosk_sales.rs', 'pub async fn command() {}\nasync fn status() {}\n'), /kiosk: command handlers — .*define `pub\(crate\) async fn status`;/],
    ['read route', (f) => f.edit('crates/host/src/routes/erp.rs', (s) => s.replace('"/erp/kiosk/sales"', '"/erp/kiosk/sold"')), /kiosk: read route — declare the route "\/erp\/kiosk\/sales"/],
    ['extractor coverage', (f) => f.write('crates/host/src/extractors.rs', ''), /kiosk: extractor coverage/],
    ['routes not generated', (f) => f.edit('crates/host/src/routes/erp.rs', (s) => s.replace('for_each_erp_module!(mount)', 'Router::new()')), /crates\/host\/src\/routes\/erp\.rs: must expand for_each_erp_module!\(mount\)/],
    ['dispatch not generated', (f) => f.write('crates/host/src/business.rs', 'match module {}\n'), /crates\/host\/src\/business\.rs: must expand for_each_erp_module!\(dispatch\)/],
    ['generator reads another file', (f) => f.write('crates/host/build.rs', ''), /crates\/host\/build\.rs: must generate for_each_erp_module!/],
  ]
  for (const [name, breakIt, expected] of cases) {
    await t.test(name, (t) => {
      const f = fixture(t)
      breakIt(f)
      const { problems } = checkModules(f.root)
      assert.ok(problems.some((problem) => expected.test(problem)), `${name}: ${JSON.stringify(problems)}`)
    })
  }
})

test('a workspace read path is satisfied by the workspace domain list, not a route literal', (t) => {
  const f = fixture(t, [{ ...descriptor(), readPath: '/erp/kiosk/workspace' }])
  assert.ok(checkModules(f.root).problems.some((problem) => /kiosk: read route — .*WorkspaceDomain/.test(problem)))
  f.write('crates/adapters/src/erp/projections.rs', 'match self {\n    Self::Kiosk => "kiosk",\n}\n')
  assert.deepEqual(checkModules(f.root).problems, [])
  f.write('crates/adapters/src/erp/projections.rs', 'match self {\n    Self::Kiosks => "kiosks",\n}\n')
  assert.equal(checkModules(f.root).problems.length, 1)
})

test('missing source, suite and specification files are reported separately', (t) => {
  const f = fixture(t)
  for (const path of ['crates/domain/src/erp/kiosk_sales.rs', 'crates/host/tests/erp_kiosk.rs', 'docs/modules/KIOSK_MODULE.md']) rmSync(join(f.root, path))
  const { problems } = checkModules(f.root)
  assert.deepEqual(problems.map((problem) => problem.split(' — ')[0]).sort(), ['kiosk: domain source', 'kiosk: integration suite', 'kiosk: specification'])
})

test('a module wired without a descriptor is refused', (t) => {
  const f = fixture(t)
  f.write('config/operator-actions.v1.json', JSON.stringify({ modules: [{ id: 'kiosk', namespace: 'archava_kiosk' }, { id: 'ghost', namespace: 'archava_ghost' }] }))
  f.edit('crates/host/src/business.rs', (s) => `${s}"phantom" => call!(crate::erp::phantom::command),\n`)
  f.edit('crates/host/src/routes/erp.rs', (s) => `${s}.route("/erp/spectre/commands/status", get(x))\n`)
  const { problems } = checkModules(f.root)
  assert.ok(problems.some((problem) => /^ghost: has forms .* but no descriptor/.test(problem)))
  assert.ok(problems.some((problem) => /business\.rs: phantom is wired by hand/.test(problem)))
  assert.ok(problems.some((problem) => /routes\/erp\.rs: spectre is wired by hand/.test(problem)))
})

test('the registry refuses ambiguous or malformed descriptors before comparing wiring', (t) => {
  assert.match(registryProblems(null)[0], /expected \{"schemaVersion":1/)
  const twin = { ...descriptor(), label: 'Twin' }
  const duplicate = registryProblems({ schemaVersion: 1, modules: [descriptor(), twin] })
  for (const key of ['id', 'namespace', 'rank']) assert.ok(duplicate.some((problem) => problem.includes(`repeats ${key}`)), key)
  const bad = { ...descriptor(), id: 'Kiosk-1', namespace: 'kiosk', source: 'kiosk-sales', tab: '', typedDetail: 'yes', agent: { rank: 0, keywords: ['Kiosk'], collections: [] } }
  delete bad.spec
  const problems = registryProblems({ schemaVersion: 1, modules: [bad] }).join('\n')
  for (const expected of [/id must be lower-case alphanumeric/, /namespace must start with archava_/, /source must be a Rust module name/, /spec must be a non-empty string/, /agent\.rank must be a positive integer/, /agent\.keywords must be lower-case/, /agent\.collections must be a non-empty list/, /tab must be a non-empty string when present/, /typedDetail must be true or false when present/]) assert.match(problems, expected)
  const f = fixture(t, [bad])
  assert.equal(checkModules(f.root).modules.length, 0)
  assert.throws(() => assert.deepEqual(checkModules(f.root).problems, []))
})

test('a missing registry is a problem, not a silent pass', (t) => {
  const f = fixture(t)
  rmSync(join(f.root, 'config/modules.v1.json'))
  assert.deepEqual(checkModules(f.root).problems, ['Missing or invalid config/modules.v1.json'])
})
