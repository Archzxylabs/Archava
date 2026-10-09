import { randomUUID } from 'node:crypto'
import { describe, it, expect } from 'vitest'
import type { SqlClient, SqlStatement } from '@archava/act-storage'
import { PostgresActHostAttemptLedger, actHostMigration } from '../src/index.js'

const scope = { tenantId: 'tenant-test', sessionId: 'session-test' }
async function fixture() {
  const migration = await actHostMigration()
  const calls: SqlStatement[] = []
  let answer: unknown = null
  let fail = false
  const client: SqlClient = {
    storeId: 'offline',
    capabilities: { transactions: false },
    async run(statement) {
      await Promise.resolve()
      calls.push(statement)
      if (statement.sql.includes('AS safe')) return [{ safe: true }]
      if (statement.sql.includes('schema_migrations'))
        return [{ id: migration.id, checksum: migration.checksum }]
      if (statement.sql.includes('AS count')) return [{ count: 3 }]
      if (fail) throw new Error('Private connection details')
      return [{ result: answer }]
    },
  }
  const ledger = await PostgresActHostAttemptLedger.create(client, scope)
  const record = {
    ...scope,
    actionId: 'booking.create',
    inputDigest: 'a'.repeat(64),
    occurredAt: '2026-10-09T10:00:00.000Z',
    challengeId: randomUUID(),
    expiresAt: 1791536700000,
  }
  const row = {
    tenant_id: scope.tenantId,
    session_id: scope.sessionId,
    action_id: record.actionId,
    input_digest: record.inputDigest,
    intent_id: randomUUID(),
    generation: 1,
    phase: 'pending',
    previous_intent_id: null,
    occurred_at_ms: String(Date.parse(record.occurredAt)),
    challenge_id: record.challengeId,
    expires_at_ms: String(record.expiresAt),
  }
  return {
    client,
    ledger,
    calls,
    record,
    row,
    answer: (value: unknown) => {
      answer = value
    },
    outage: () => {
      fail = true
    },
  }
}
describe('scoped PostgreSQL intent boundary', () => {
  it('parses driver JSON/bigint values and keeps every supplied value parameterized', async () => {
    const f = await fixture()
    f.answer(f.row)
    expect(await f.ledger.open(f.record)).toMatchObject({
      intentId: f.row.intent_id,
      occurredAt: f.record.occurredAt,
      phase: 'pending',
    })
    const call = required(f.calls.at(-1))
    expect(call.sql).not.toContain(scope.tenantId)
    expect(call.params.slice(0, 4)).toEqual([
      scope.tenantId,
      scope.sessionId,
      f.record.actionId,
      f.record.inputDigest,
    ])
    expect(call.params).not.toContain('Raw private command')
  })
  it('distinguishes absent from malformed, mismatched and cross-scope rows', async () => {
    const f = await fixture()
    expect(await f.ledger.findByIntent(f.row.intent_id)).toBeNull()
    for (const patch of [
      { phase: 'invented' },
      { tenant_id: 'other-tenant' },
      { session_id: 'other-session' },
      { intent_id: randomUUID() },
      { occurred_at_ms: '1.5' },
      { generation: 0 },
      { challenge_id: null },
    ]) {
      f.answer({ ...f.row, ...patch })
      await expect(f.ledger.findByIntent(f.row.intent_id)).rejects.toThrow()
    }
  })
  it('fails before SQL for foreign identity and never coerces transition answers or leaks outage text', async () => {
    const f = await fixture(),
      before = f.calls.length
    await expect(f.ledger.open({ ...f.record, tenantId: 'other-tenant' })).rejects.toThrow()
    expect(f.calls).toHaveLength(before)
    f.answer('true')
    await expect(f.ledger.claim(f.row.intent_id, f.record.challengeId)).rejects.toThrow()
    f.outage()
    await expect(f.ledger.claim(f.row.intent_id, f.record.challengeId)).rejects.toThrow(
      'The action attempt could not be verified.',
    )
    await expect(f.ledger.reconcileSuccess(f.row.intent_id, 'malformed')).rejects.toThrow()
  })
  it('refuses unsafe roles, changed schema checksums and incomplete forced RLS', async () => {
    for (const category of ['role', 'schema', 'isolation']) {
      const f = await fixture()
      const bad: SqlClient = {
        ...f.client,
        run: async (statement) => {
          if (category === 'role' && statement.sql.includes('AS safe')) return [{ safe: false }]
          if (category === 'schema' && statement.sql.includes('schema_migrations'))
            return [{ id: '001_host_intents', checksum: '0'.repeat(64) }]
          if (category === 'isolation' && statement.sql.includes('AS count')) return [{ count: 2 }]
          return f.client.run(statement)
        },
      }
      await expect(PostgresActHostAttemptLedger.create(bad, scope)).rejects.toThrow()
    }
    const f = await fixture()
    expect(JSON.stringify(f.ledger)).toBe(
      JSON.stringify({ ledgerId: 'postgres-act-host-intents@1' }),
    )
    await expect(
      PostgresActHostAttemptLedger.create(
        {
          ...f.client,
          run: () => Promise.reject(new Error('private credential-bearing database error')),
        },
        scope,
      ),
    ).rejects.toThrow('The action attempt could not be verified.')
  })
})

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error('Missing fixture result')
  return value
}
