import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { writeFile, mkdir } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { Pool } from 'pg'
import type { CrmCommand, CrmMutation, CrmPrincipal } from '@archava/crm'
import { createCluster } from './cluster.js'
import { prepareConsumers } from './consumers.js'
import { startReferenceServer } from './server.js'
import { referenceShutdown } from './shutdown.js'

const shutdown = referenceShutdown()
const consumers = await prepareConsumers()
const cluster = await createCluster(consumers.postgres, consumers.crm.CRM_PERMISSIONS).catch(
  async (error: unknown) => {
    await consumers.close()
    throw error
  },
)
let pool = new Pool(cluster.application)
let server: Awaited<ReturnType<typeof startReferenceServer>> | null = null
const checks: string[] = []
const check = (label: string) => {
  checks.push(label)
  process.stdout.write(`PASS ${label}\n`)
}
const human = (id: string): CrmPrincipal => ({ kind: 'human', id })
const owner = human('hotel-owner')
const studio = human('studio-owner')
const viewer = human('hotel-viewer')
const editor = human('hotel-editor')
const denied = (error: unknown) =>
  typeof error === 'object' &&
  error !== null &&
  'code' in error &&
  ['forbidden', 'not_found'].includes(String(error.code))
const conflict = (error: unknown) =>
  typeof error === 'object' && error !== null && 'code' in error && error.code === 'conflict'
const mutation = (command: CrmCommand): CrmMutation => ({ requestId: randomUUID(), command })
try {
  const { crm, postgres } = consumers
  await postgres.migrateCrm(cluster.owner)
  assert.equal(
    (await cluster.owner.query('SELECT * FROM archava_crm.schema_migrations')).rowCount,
    1,
  )
  await assert.rejects(() =>
    postgres.PostgresCrmRepository.create(cluster.owner, cluster.fingerprintKey),
  )
  check('migration ledger repeatability and non-owner runtime requirement')
  let service = new crm.CrmService(
    await postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey),
  )
  const create = mutation({
    operation: 'contact.create',
    data: {
      fullName: 'Hotel guest example',
      email: 'guest@example.test',
      organization: 'Example hotel',
    },
  })
  const receipts = await Promise.all(
    Array.from({ length: 8 }, () => service.mutate('hotel-demo', owner, create)),
  )
  const contactId = receipts[0]?.id
  assert.ok(contactId)
  assert.ok(receipts.every((item) => item.id === contactId))
  assert.equal(
    (
      await cluster.owner.query('SELECT * FROM archava_crm.audit WHERE request_id=$1', [
        create.requestId,
      ])
    ).rowCount,
    1,
  )
  check('eight concurrent identical requests produce one contact and one audit receipt')

  const second = await service.mutate(
    'studio-demo',
    studio,
    mutation({
      operation: 'contact.create',
      data: {
        fullName: 'Consulting client example',
        email: null,
        organization: 'Example consultancy',
      },
    }),
  )
  await assert.rejects(() => service.list('studio-demo', owner), denied)
  await assert.rejects(() => service.detail('hotel-demo', owner, second.id), denied)
  await assert.rejects(() => service.list('hotel-demo', null))
  await assert.rejects(() => service.list('hotel-demo', human('public-customer')), denied)
  check('two business contexts persist separate records; cross-tenant and public callers refused')

  assert.equal(
    (await service.detail('hotel-demo', owner, contactId)).contact.email,
    'guest@example.test',
  )
  const updates = await Promise.allSettled([
    service.mutate(
      'hotel-demo',
      owner,
      mutation({
        operation: 'contact.update',
        id: contactId,
        expectedRevision: 1,
        data: { fullName: 'Garden guest' },
      }),
    ),
    service.mutate(
      'hotel-demo',
      owner,
      mutation({
        operation: 'contact.update',
        id: contactId,
        expectedRevision: 1,
        data: { fullName: 'Other guest' },
      }),
    ),
  ])
  assert.equal(updates.filter((item) => item.status === 'fulfilled').length, 1)
  const loser = updates.find((item) => item.status === 'rejected')
  assert.ok(loser?.status === 'rejected' && conflict(loser.reason))
  assert.equal(
    (await service.detail('hotel-demo', owner, contactId)).contact.email,
    'guest@example.test',
  )
  assert.deepEqual(await service.mutate('hotel-demo', owner, create), receipts[0])
  await assert.rejects(
    () =>
      service.mutate('hotel-demo', owner, {
        ...create,
        command: {
          operation: 'contact.create',
          data: { fullName: 'Different command', email: null, organization: null },
        },
      }),
    conflict,
  )
  check(
    'optimistic revision race, preserved optional fields and canonical retry/new-intent separation',
  )

  await assert.rejects(
    () =>
      service.mutate(
        'hotel-demo',
        viewer,
        mutation({
          operation: 'contact.update',
          id: contactId,
          expectedRevision: 2,
          data: { fullName: 'Forbidden' },
        }),
      ),
    denied,
  )
  assert.equal((await service.list('hotel-demo', editor)).items.length, 0)
  await assert.rejects(() => service.detail('hotel-demo', editor, contactId), denied)
  const own = await service.mutate(
    'hotel-demo',
    editor,
    mutation({
      operation: 'contact.create',
      data: { fullName: 'Assigned editor inquiry', email: null, organization: null },
    }),
  )
  assert.equal((await service.detail('hotel-demo', editor, own.id)).contact.ownerId, 'hotel-editor')
  check('viewer writes refused and assigned-only resource access enforced')

  const bot: CrmPrincipal = { kind: 'service', id: 'hotel-bot', onBehalfOf: 'hotel-owner' }
  const tools = crm.createCrmTools(service, 'hotel-demo', bot)
  const projected = (await tools.invoke('crm.contacts.list', {})) as {
    items: Record<string, unknown>[]
  }
  assert.ok(projected.items.length > 0)
  assert.ok(projected.items.every((item) => !('email' in item)))
  await assert.rejects(
    () =>
      tools.invoke('crm.followup.prepare', {
        contactId,
        summary: 'Follow up',
        dueAt: '2026-10-10T10:00:00Z',
      }),
    denied,
  )
  await assert.rejects(() => service.mutate('hotel-demo', bot, create), denied)
  await assert.rejects(() => tools.invoke('crm.contact.delete', { id: contactId }), denied)
  await assert.rejects(() => service.list('hotel-demo', human('hotel-bot')), denied)
  check('machine/initiator permission intersection, projected read tools and no machine write path')

  const draft = (await crm
    .createCrmTools(service, 'hotel-demo', owner)
    .invoke('crm.followup.prepare', {
      contactId,
      summary: 'Discuss inquiry',
      dueAt: '2026-10-10T10:00:00Z',
    })) as { contactRevision: number; status: string; requiresApproval: boolean }
  assert.equal(draft.status, 'draft')
  assert.equal(draft.requiresApproval, true)
  assert.equal((await service.detail('hotel-demo', owner, contactId)).followups.length, 0)
  const task = await service.mutate(
    'hotel-demo',
    owner,
    mutation({
      operation: 'followup.create',
      contactId,
      contactRevision: draft.contactRevision,
      summary: 'Discuss inquiry',
      dueAt: '2026-10-10T10:00:00Z',
    }),
  )
  const record = await service.detail('hotel-demo', owner, contactId)
  await service.mutate(
    'hotel-demo',
    owner,
    mutation({
      operation: 'activity.add',
      contactId,
      contactRevision: record.contact.revision,
      kind: 'note',
      summary: 'Reviewed hotel inquiry',
    }),
  )
  const completion = mutation({ operation: 'followup.complete', id: task.id, expectedRevision: 1 })
  await service.mutate('hotel-demo', owner, completion)
  await service.mutate('hotel-demo', owner, completion)
  const detail = await service.detail('hotel-demo', owner, contactId)
  assert.equal(detail.followups[0]?.status, 'completed')
  assert.equal(detail.activities.length, 1)
  check('draft remains read-only; human activity/follow-up lifecycle persisted and audited')

  const paged = await service.list('hotel-demo', owner, { limit: 1 })
  assert.equal(paged.items.length, 1)
  assert.ok(paged.next)
  const next = await service.list('hotel-demo', owner, { limit: 1, after: paged.next })
  assert.notEqual(next.items[0]?.id, paged.items[0]?.id)
  await assert.rejects(() => service.list('hotel-demo', owner, { limit: 101 }))
  assert.equal(
    (await service.list('hotel-demo', owner, { search: "%' OR 1=1 --" })).items.length,
    0,
  )
  await assert.rejects(() => service.mutate('hotel-demo', owner, { ...create, role: 'owner' }))
  check('bounded keyset pagination, parameterized hostile search and strict mutation schema')

  const unscoped = await pool.query('SELECT * FROM archava_crm.contacts')
  assert.equal(unscoped.rowCount, 0)
  const connection = await pool.connect()
  try {
    await connection.query('BEGIN')
    await connection.query("SELECT set_config('archava.tenant_id','hotel-demo',true)")
    const tenants = await connection.query<{ tenant_id: string }>(
      'SELECT DISTINCT tenant_id FROM archava_crm.contacts',
    )
    assert.deepEqual(
      tenants.rows.map((item) => item.tenant_id),
      ['hotel-demo'],
    )
    await connection.query('ROLLBACK')
  } finally {
    connection.release()
  }
  await assert.rejects(() =>
    cluster.owner.query(
      'INSERT INTO archava_crm.activities(tenant_id,id,contact_id,kind,summary,actor_id) VALUES($1,$2,$3,$4,$5,$6)',
      ['hotel-demo', randomUUID(), second.id, 'note', 'Cross-tenant link', 'hotel-owner'],
    ),
  )
  await assert.rejects(() =>
    cluster.owner.query("UPDATE archava_crm.audit SET operation='tampered'"),
  )
  check('forced tenant RLS, pool scope reset, composite tenant foreign keys and immutable audit')

  const atomic = mutation({
    operation: 'contact.create',
    data: { fullName: 'Atomic test', email: null, organization: null },
  })
  const before = (await cluster.owner.query('SELECT * FROM archava_crm.contacts')).rowCount
  await cluster.owner.query('REVOKE INSERT ON archava_crm.audit FROM crm_app')
  await assert.rejects(() => service.mutate('hotel-demo', owner, atomic))
  assert.equal((await cluster.owner.query('SELECT * FROM archava_crm.contacts')).rowCount, before)
  assert.equal(
    (
      await cluster.owner.query('SELECT * FROM archava_crm.commands WHERE request_id=$1', [
        atomic.requestId,
      ])
    ).rowCount,
    0,
  )
  await cluster.owner.query('GRANT INSERT ON archava_crm.audit TO crm_app')
  await service.mutate('hotel-demo', owner, atomic)
  check('audit failure rolls back records and command identity; same request can recover')

  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET active=false WHERE principal_id='hotel-viewer'",
  )
  await assert.rejects(() => service.list('hotel-demo', viewer), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET active=true WHERE principal_id='hotel-viewer'",
  )
  await pool.end()
  await cluster.restart()
  await postgres.migrateCrm(cluster.owner)
  pool = new Pool(cluster.application)
  service = new crm.CrmService(
    await postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey),
  )
  assert.equal((await service.detail('hotel-demo', owner, contactId)).activities.length, 1)
  assert.deepEqual(await service.mutate('hotel-demo', owner, create), receipts[0])
  check(
    'membership revocation rechecked; database restart preserves records, audit and retry receipt',
  )

  server = await startReferenceServer(service, consumers.publicDirectory, crm.createCrmApi)
  const token = server.token(owner)
  const readToken = server.token(viewer)
  const apiOrigin = server.origin
  const get = (path: string, token?: string) =>
    fetch(apiOrigin + path, { headers: token ? { authorization: 'Bearer ' + token } : {} })
  assert.equal((await get('/api/crm/hotel-demo/contacts')).status, 401)
  assert.equal((await get('/api/crm/studio-demo/contacts', token)).status, 403)
  const write = await fetch(server.origin + '/api/crm/hotel-demo/commands', {
    method: 'POST',
    headers: { authorization: 'Bearer ' + readToken, 'content-type': 'application/json' },
    body: JSON.stringify(create),
  })
  assert.equal(write.status, 403)
  const csrf = await fetch(server.origin + '/api/crm/hotel-demo/commands', {
    method: 'POST',
    headers: {
      authorization: 'Bearer ' + token,
      origin: 'https://other.invalid',
      'content-type': 'application/json',
    },
    body: JSON.stringify(create),
  })
  assert.equal(csrf.status, 403)
  const tooLarge = await fetch(server.origin + '/api/crm/hotel-demo/commands', {
    method: 'POST',
    headers: { authorization: 'Bearer ' + token, 'content-type': 'application/json' },
    body: ' '.repeat(17_000),
  })
  assert.equal(tooLarge.status, 413)
  check(
    'authenticated HTTP API rejects anonymous, cross-tenant, viewer, cross-origin and oversized requests',
  )

  const root = resolve(import.meta.dirname, '../../..')
  await mkdir(join(root, 'dist/crm-proof'), { recursive: true })
  await writeFile(
    join(root, 'dist/crm-proof/results.json'),
    JSON.stringify(
      {
        node: process.version,
        checks,
        consumers: consumers.proofs,
        postgres: 'real disposable cluster',
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(
    `${checks.length}/${checks.length} CRM checks passed; compiled module hashes match across both consumers.\n`,
  )
  if (process.argv.includes('--browser') && !shutdown.requested) {
    await writeFile(
      '/tmp/archava-crm-browser-target.json',
      JSON.stringify({ origin: server.origin }),
    )
    process.stdout.write('CRM browser target ready; press Ctrl-C to stop.\n')
    await shutdown.wait()
  }
} finally {
  await server?.close()
  await pool.end()
  await cluster.close()
  await consumers.close()
}
