import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import type { Pool } from 'pg'
import type { CrmPrincipal } from '@archava/crm'
import type { prepareConsumers } from './consumers.js'

/** Both cold-installed business consumers use the public aggregate contract against real PG. */
export async function verifyOverview(
  consumers: Awaited<ReturnType<typeof prepareConsumers>>,
  operator: Pool,
  runtime: Pool,
  fingerprintKey: string,
  check: (label: string) => void,
) {
  const asOf = '2026-10-09T08:00:00.000Z',
    owner: CrmPrincipal = { kind: 'human', id: 'overview-owner' },
    staff: CrmPrincipal = { kind: 'human', id: 'overview-staff' },
    machine: CrmPrincipal = { kind: 'service', id: 'overview-bot', onBehalfOf: staff.id },
    tenants = ['overview-hotel', 'overview-studio'] as const,
    read = ['contacts.read', 'followups.read', 'pipeline.read']
  const services = await Promise.all(
    consumers.crmModules.map(async (crm, index) => {
      const pg = consumers.postgresModules[index]
      assert.ok(pg)
      return new crm.CrmService(
        await pg.PostgresCrmRepository.create(runtime, fingerprintKey),
        () => asOf,
      )
    }),
  )
  const assignedContacts: string[] = []
  for (const tenant of [...tenants, 'overview-empty']) {
    for (const [id, kind, permissions, scope] of [
      [owner.id, 'human', read, 'all'],
      [staff.id, 'human', read, 'assigned'],
      [machine.id, 'service', read, 'all'],
      ['overview-no-tasks', 'human', ['contacts.read', 'pipeline.read'], 'all'],
      ['overview-no-pipeline', 'human', ['contacts.read', 'followups.read'], 'all'],
      ['overview-no-contacts', 'human', ['followups.read', 'pipeline.read'], 'all'],
    ])
      await operator.query(
        'INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [tenant, id, kind, permissions, scope],
      )
    if (tenant === 'overview-empty') continue
    const contactId = randomUUID(),
      assignedId = randomUUID(),
      transferableId = randomUUID()
    assignedContacts.push(transferableId)
    for (let index = 0; index < 123; index++)
      await operator.query(
        'INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,owner_id) VALUES($1,$2,$3,$4,$5)',
        [
          tenant,
          index === 0
            ? contactId
            : index === 121
              ? assignedId
              : index === 122
                ? transferableId
                : randomUUID(),
          'Private example ' + index,
          'private@example.test',
          index >= 121 ? staff.id : owner.id,
        ],
      )
    for (let index = 0; index < 26; index++)
      await operator.query(
        'INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) VALUES($1,$2,$3,$4,$5)',
        [
          tenant,
          randomUUID(),
          index === 24 ? assignedId : index === 25 ? transferableId : contactId,
          'Private task',
          index < 12 || index === 24
            ? '2026-10-09T07:59:59.999999Z'
            : index === 25
              ? asOf
              : '2026-10-09T08:00:00.000001Z',
        ],
      )
    await operator.query(
      "INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at,status) VALUES($1,$2,$3,'Done task',$4,'completed')",
      [tenant, randomUUID(), contactId, asOf],
    )
    for (let index = 0; index < 27; index++) {
      const stage =
        ['new', 'qualified', 'proposal', 'won', 'lost'][index] ??
        (index === 26 ? 'proposal' : 'new')
      await operator.query(
        'INSERT INTO archava_crm.opportunities(tenant_id,id,contact_id,owner_id,title,stage,loss_reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [
          tenant,
          randomUUID(),
          index === 26 ? assignedId : contactId,
          index === 26 ? staff.id : owner.id,
          'Private inquiry',
          stage,
          stage === 'lost' ? 'Not proceeding' : null,
        ],
      )
    }
  }
  const counts = async () =>
    (
      await operator.query<Record<string, unknown>>(`SELECT
    (SELECT count(*) FROM archava_crm.contacts) AS contacts,
    (SELECT count(*) FROM archava_crm.followups) AS followups,
    (SELECT count(*) FROM archava_crm.opportunities) AS opportunities,
    (SELECT count(*) FROM archava_crm.activities) AS activities,
    (SELECT count(*) FROM archava_crm.commands) AS commands,
    (SELECT count(*) FROM archava_crm.audit) AS audit`)
    ).rows
  const before = await counts()
  for (const [index, tenant] of tenants.entries()) {
    const service = services[index],
      conversation = consumers.conversations[index]
    assert.ok(service && conversation)
    const snapshot = await service.overview(tenant, owner)
    assert.equal(snapshot.contacts, 123)
    assert.deepEqual(snapshot.followups, { open: 26, overdue: 13, upcoming: 13, completed: 1 })
    assert.deepEqual(snapshot.pipeline, {
      total: 27,
      active: 25,
      new: 22,
      qualified: 1,
      proposal: 2,
      won: 1,
      lost: 1,
    })
    assert.equal(snapshot.asOf, asOf)
    assert.ok(
      !/private@example|Private example|Private inquiry|ownerId|email/.test(
        JSON.stringify(snapshot),
      ),
    )
    const assigned = await service.overview(tenant, staff)
    assert.equal(assigned.contacts, 2)
    assert.deepEqual(assigned.followups, { open: 2, overdue: 1, upcoming: 1, completed: 0 })
    assert.equal(assigned.pipeline?.proposal, 1)
    assert.equal(assigned.pipeline?.total, 1)
    assert.deepEqual(await service.overview(tenant, machine), assigned)
    const reply = await conversation
      .createCrmConversation(service, new conversation.LocalCrmPlanner())
      .ask(
        { tenantId: tenant, principal: owner, turnId: 'overview-turn', occurredAt: asOf },
        { utterance: 'ringkas kondisi CRM' },
      )
    assert.equal(reply.data?.kind, 'overview')
    assert.equal(reply.source, 'crm')
    if (reply.data?.kind !== 'overview') throw new Error('Overview projection missing')
    assert.deepEqual(reply.data.snapshot, snapshot)
    assert.equal(reply.planner.providerId, 'archava-crm-local')
  }
  check(
    'two installed business consumers share complete CRM aggregates beyond page limits, microsecond due boundaries, stage totals and local Archava truth without private record fields',
  )
  const service = services[0]
  assert.ok(service)
  const empty = await service.overview('overview-empty', owner)
  assert.equal(empty.contacts, 0)
  assert.deepEqual(empty.followups, { open: 0, overdue: 0, upcoming: 0, completed: 0 })
  assert.equal(empty.pipeline?.total, 0)
  assert.equal(
    (await service.overview(tenants[0], { kind: 'human', id: 'overview-no-tasks' })).followups,
    null,
  )
  assert.equal(
    (await service.overview(tenants[0], { kind: 'human', id: 'overview-no-pipeline' })).pipeline,
    null,
  )
  await assert.rejects(
    () => service.overview(tenants[0], { kind: 'human', id: 'overview-no-contacts' }),
    { code: 'forbidden' },
  )
  await assert.rejects(() => service.overview('hotel-demo', owner), { code: 'forbidden' })
  check(
    'overview distinguishes permitted empty data from missing section grants and refuses missing contacts or tenant membership',
  )
  await operator.query(
    "UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read','followups.read'] WHERE tenant_id=$1 AND principal_id=$2",
    [tenants[0], staff.id],
  )
  assert.equal((await service.overview(tenants[0], staff)).pipeline, null)
  assert.equal((await service.overview(tenants[0], machine)).pipeline, null)
  await operator.query(
    "UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read','pipeline.read'] WHERE tenant_id=$1 AND principal_id=$2",
    [tenants[0], machine.id],
  )
  assert.equal((await service.overview(tenants[0], machine)).followups, null)
  assert.equal((await service.overview(tenants[0], staff)).followups?.open, 2)
  await operator.query(
    'UPDATE archava_crm.memberships SET permissions=$3 WHERE tenant_id=$1 AND principal_id=ANY($2::text[])',
    [tenants[0], [staff.id, machine.id], read],
  )
  await operator.query('UPDATE archava_crm.contacts SET owner_id=$3 WHERE tenant_id=$1 AND id=$2', [
    tenants[0],
    assignedContacts[0],
    owner.id,
  ])
  assert.equal((await service.overview(tenants[0], staff)).contacts, 1)
  assert.equal((await service.overview(tenants[0], staff)).pipeline?.total, 1)
  assert.equal((await service.overview(tenants[0], machine)).followups?.open, 1)
  await operator.query(
    'UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2',
    [tenants[0], staff.id],
  )
  await assert.rejects(() => service.overview(tenants[0], staff), { code: 'forbidden' })
  await assert.rejects(() => service.overview(tenants[0], machine), { code: 'forbidden' })
  check(
    'overview rechecks human/service delegation intersections, resource reassignment and inactive membership without cached grants',
  )
  assert.deepEqual(await counts(), before)
  check(
    'dashboard and conversation overview reads leave all CRM record, command and audit counts unchanged and request no provider',
  )
}
