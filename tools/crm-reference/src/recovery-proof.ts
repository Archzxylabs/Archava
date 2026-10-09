import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import type { Pool } from 'pg'
import type { CrmPrincipal } from '@archava/crm'
import type { prepareConsumers } from './consumers.js'
import type { createCluster } from './cluster.js'

type Consumers = Awaited<ReturnType<typeof prepareConsumers>>
type Cluster = Awaited<ReturnType<typeof createCluster>>
const human = (id: string): CrmPrincipal => ({ kind: 'human', id })
const denied = (error: unknown) =>
  error !== null &&
  typeof error === 'object' &&
  'code' in error &&
  ['forbidden', 'not_found'].includes(String(error.code))

export async function verifyCommandRecovery(
  consumers: Consumers,
  cluster: Cluster,
  pool: Pool,
  contactIds: readonly string[],
  check: (label: string) => void,
) {
  const checkpoints = []
  assert.notEqual(consumers.crmModules[0]?.CrmService, consumers.crmModules[1]?.CrmService)
  assert.notEqual(
    consumers.postgresModules[0]?.PostgresCrmRepository,
    consumers.postgresModules[1]?.PostgresCrmRepository,
  )
  for (const [index, tenant, principal] of [
    [0, 'hotel-demo', human('hotel-owner')],
    [1, 'studio-demo', human('studio-owner')],
  ] as const) {
    const crm = consumers.crmModules[index]
    const postgres = consumers.postgresModules[index]
    const contactId = contactIds[index]
    assert.ok(crm && postgres && contactId)
    const service = new crm.CrmService(
      await postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey),
    )
    const revision = (await service.detail(tenant, principal, contactId)).contact.revision
    const requestId = randomUUID()
    const receipt = await service.mutate(tenant, principal, {
      requestId,
      command: {
        operation: 'followup.create',
        contactId,
        contactRevision: revision,
        summary: 'Synthetic recovery fixture',
        dueAt: '2026-10-11T10:00:00Z',
      },
    })
    const expected = { status: 'committed', requestId, receipt }
    const before = await cluster.owner.query(
      'SELECT (SELECT count(*) FROM archava_crm.commands) AS commands, (SELECT count(*) FROM archava_crm.audit) AS audit, (SELECT count(*) FROM archava_crm.followups) AS tasks',
    )
    for (let read = 0; read < 3; read += 1)
      assert.deepEqual(await service.commandStatus(tenant, principal, requestId), expected)
    const after = await cluster.owner.query(
      'SELECT (SELECT count(*) FROM archava_crm.commands) AS commands, (SELECT count(*) FROM archava_crm.audit) AS audit, (SELECT count(*) FROM archava_crm.followups) AS tasks',
    )
    assert.deepEqual(after.rows, before.rows)
    checkpoints.push({ tenant, principal, requestId, expected })
  }
  check(
    'two independent cold-installed recovery consumers verify their own receipts with zero command/audit/task changes',
  )

  const crm = consumers.crmModules[0]
  const postgres = consumers.postgresModules[0]
  const checkpoint = checkpoints[0]
  const contactId = contactIds[0]
  assert.ok(crm && postgres && checkpoint && contactId)
  const service = new crm.CrmService(
    await postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey),
  )
  const { tenant, principal, requestId } = checkpoint
  assert.deepEqual(await service.commandStatus(tenant, human('hotel-viewer'), requestId), {
    status: 'unconfirmed',
    requestId,
  })
  await assert.rejects(() => service.commandStatus('studio-demo', principal, requestId), denied)
  await assert.rejects(
    () =>
      service.commandStatus(
        tenant,
        { kind: 'service', id: 'hotel-bot', onBehalfOf: principal.id },
        requestId,
      ),
    denied,
  )
  await assert.rejects(
    () =>
      crm.createCrmTools(service, tenant, principal).invoke('crm.command.status', { requestId }),
    denied,
  )
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_remove(permissions,'followups.read') WHERE tenant_id=$1 AND principal_id=$2",
    [tenant, principal.id],
  )
  await assert.rejects(() => service.commandStatus(tenant, principal, requestId), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'followups.read') WHERE tenant_id=$1 AND principal_id=$2",
    [tenant, principal.id],
  )
  await cluster.owner.query(
    'UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, principal.id],
  )
  await assert.rejects(() => service.commandStatus(tenant, principal, requestId), denied)
  await cluster.owner.query(
    'UPDATE archava_crm.memberships SET active=true WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, principal.id],
  )
  check(
    'recovery binds tenant and human initiator, rechecks current read grants/revocation and remains outside model tools',
  )

  const editor = human('hotel-editor')
  const created = await service.mutate(tenant, editor, {
    requestId: randomUUID(),
    command: {
      operation: 'contact.create',
      data: { fullName: 'Synthetic assigned recovery fixture' },
    },
  })
  const assignedId = randomUUID()
  await service.mutate(tenant, editor, {
    requestId: assignedId,
    command: {
      operation: 'followup.create',
      contactId: created.id,
      contactRevision: 1,
      summary: 'Assigned recovery fixture',
      dueAt: '2026-10-11T10:00:00Z',
    },
  })
  assert.equal((await service.commandStatus(tenant, editor, assignedId)).status, 'committed')
  await cluster.owner.query(
    'UPDATE archava_crm.contacts SET owner_id=$3 WHERE tenant_id=$1 AND id=$2',
    [tenant, created.id, principal.id],
  )
  await assert.rejects(() => service.commandStatus(tenant, editor, assignedId), denied)
  check('an own receipt no longer exposes its resource after contact reassignment')

  const locker = await cluster.owner.connect()
  const pendingId = randomUUID()
  let pending: ReturnType<typeof service.mutate> | null = null
  try {
    await locker.query('BEGIN')
    await locker.query(
      'SELECT id FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
      [tenant, contactId],
    )
    const revision = (await service.detail(tenant, principal, contactId)).contact.revision
    pending = service.mutate(tenant, principal, {
      requestId: pendingId,
      command: {
        operation: 'followup.create',
        contactId,
        contactRevision: revision,
        summary: 'In-flight recovery fixture',
        dueAt: '2026-10-11T10:00:00Z',
      },
    })
    // Install a rejection handler immediately; cleanup still waits for the result.
    void pending.catch(() => {})
    const deadline = Date.now() + 2000
    let waiting = false
    while (Date.now() < deadline && !waiting) {
      waiting =
        (
          await cluster.owner.query<{ waiting: boolean }>(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='crm_app' AND wait_event_type='Lock' AND query LIKE '%archava_crm.contacts%') AS waiting",
          )
        ).rows[0]?.waiting === true
      if (!waiting) await new Promise((resolve) => setTimeout(resolve, 10))
    }
    assert.equal(waiting, true, 'Fixture command did not reach its contact lock')
    assert.deepEqual(await service.commandStatus(tenant, principal, pendingId), {
      status: 'unconfirmed',
      requestId: pendingId,
    })
    await locker.query('COMMIT')
    const receipt = await pending
    assert.deepEqual(await service.commandStatus(tenant, principal, pendingId), {
      status: 'committed',
      requestId: pendingId,
      receipt,
    })
  } finally {
    await locker.query('ROLLBACK')
    locker.release()
    await pending?.catch(() => {})
  }
  check(
    'an actually blocked in-flight transaction stays unconfirmed and later commits under the original UUID without resubmission',
  )
  return checkpoints
}
