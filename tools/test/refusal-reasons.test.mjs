import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { checkRefusalReasons, coverageLines, raisedMessages } from '../check-refusal-reasons.mjs'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const entry = (code) => ({ code, message: 'This was refused; do the other thing.', message_id: 'Ini ditolak; lakukan hal yang lain.' })

function fixture(t, reasons, sql) {
  const root = mkdtempSync(join(tmpdir(), 'archava-refusal-reasons-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const write = (path, content) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), content) }
  write('config/refusal-reasons.v1.json', JSON.stringify({ schemaVersion: 1, reasons }))
  for (const [path, content] of Object.entries(sql)) write(`migrations/${path}`, content)
  return root
}

test('every catalogue key is the message of a database guard', () => {
  const result = checkRefusalReasons(ROOT)
  assert.deepEqual(result.problems, [])
  assert.ok(result.entries >= 30, `${result.entries} entries`)
})

test('uncatalogued guard messages are reported per ledger without failing', (t) => {
  const result = checkRefusalReasons(ROOT)
  for (const line of coverageLines(result)) t.diagnostic(line)
  assert.ok(result.ledgers.some((row) => row.ledger === 'accounting-pg' && row.catalogued > 0))
  for (const row of result.ledgers) assert.equal(row.catalogued + row.uncatalogued, row.guards, row.ledger)
})

test('guard messages are read with their SQLSTATE, across lines and quotes', () => {
  const sql = [
    "-- RAISE EXCEPTION 'Only a comment' USING ERRCODE='23514';",
    "IF x THEN RAISE EXCEPTION 'First rule' USING ERRCODE='23514';END IF;",
    "IF y THEN\n  RAISE EXCEPTION 'Second rule,\n over lines' USING ERRCODE = '23503';END IF;",
    "RAISE EXCEPTION 'It''s plain';",
    "RAISE EXCEPTION 'Cannot move from % to %', OLD.state, NEW.state;",
  ].join('\n')
  assert.deepEqual(raisedMessages(sql), [
    { message: 'First rule', state: '23514', dynamic: false },
    { message: 'Second rule,\n over lines', state: '23503', dynamic: false },
    { message: "It's plain", state: 'P0001', dynamic: false },
    { message: 'Cannot move from % to %', state: 'P0001', dynamic: true },
  ])
})

test('a stale key, a duplicate code and a malformed entry are problems', (t) => {
  const root = fixture(t, {
    'First rule': entry('first_rule'),
    'First rule ': entry('first_rule_spaced'),
    'Removed rule': entry('removed_rule'),
    'Second rule': entry('first_rule'),
    'Third rule': { ...entry('Third-Rule'), message_id: '', note: 'x' },
    'Moved from % to %': entry('moved'),
  }, {
    'kiosk-pg/001_kiosk.sql': "RAISE EXCEPTION 'First rule' USING ERRCODE='23514';\nRAISE EXCEPTION 'Second rule' USING ERRCODE='23514';\nRAISE EXCEPTION 'Third rule';\nRAISE EXCEPTION 'Fourth rule' USING ERRCODE='23514';\nRAISE EXCEPTION 'Moved from % to %',a,b USING ERRCODE='23514';\n",
  })
  const result = checkRefusalReasons(root)
  const has = (text) => assert.ok(result.problems.some((problem) => problem.includes(text)), `${text}\n${result.problems.join('\n')}`)
  has('"First rule " matches no RAISE EXCEPTION')
  has('"Removed rule" matches no RAISE EXCEPTION')
  has('"Moved from % to %" matches no RAISE EXCEPTION')
  has('code first_rule is used by both')
  has('"Third rule" needs a snake_case code')
  has('"Third rule" needs a message_id sentence')
  has('"Third rule" has unknown members: note')
  assert.equal(result.problems.length, 7)
  assert.deepEqual(result.ledgers.map(({ ledger, guards, catalogued, uncatalogued, dynamic }) => ({ ledger, guards, catalogued, uncatalogued, dynamic })), [
    { ledger: 'kiosk-pg', guards: 4, catalogued: 2, uncatalogued: 2, dynamic: 1 },
  ])
  assert.deepEqual(result.outsideGuardState, ['Third rule'])
})
