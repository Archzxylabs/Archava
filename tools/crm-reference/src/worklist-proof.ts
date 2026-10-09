import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import type { Pool } from 'pg'
import type { CrmPrincipal } from '@archava/crm'
import type { prepareConsumers } from './consumers.js'

/** Installed public contracts against a private real database; no original owner data. */
export async function verifyWorklist(
  consumers: Awaited<ReturnType<typeof prepareConsumers>>,
  operator: Pool,
  runtime: Pool,
  fingerprintKey: string,
  check: (label: string) => void,
) {
  const owner: CrmPrincipal = { kind: 'human', id: 'queue-owner' }
  const staff: CrmPrincipal = { kind: 'human', id: 'queue-staff' }
  const machine: CrmPrincipal = { kind: 'service', id: 'queue-bot', onBehalfOf: staff.id }
  const asOf = '2026-10-09T08:00:00.000Z'
  const services = await Promise.all(
    consumers.crmModules.map(async (crm, index) => {
      const pg = consumers.postgresModules[index]
      assert.ok(pg)
      return new crm.CrmService(await pg.PostgresCrmRepository.create(runtime, fingerprintKey))
    }),
  )
  const tenants = ['queue-hotel', 'queue-studio'] as const
  const ids = new Map<string, { ownerContact: string; staffContact: string; taskIds: string[] }>()
  for (const tenant of tenants) {
    for (const [principal, kind, permissions, scope] of [
      [owner.id, 'human', ['contacts.read', 'followups.read'], 'all'],
      [staff.id, 'human', ['contacts.read', 'followups.read'], 'assigned'],
      ['queue-bot', 'service', ['contacts.read', 'followups.read'], 'all'],
      ['queue-no-contact', 'human', ['followups.read'], 'all'],
      ['queue-no-task', 'human', ['contacts.read'], 'all'],
    ])
      await operator.query(
        'INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [tenant, principal, kind, permissions, scope],
      )
    const ownerContact = randomUUID(),
      staffContact = randomUUID()
    for (const [id, principal, name] of [
      [ownerContact, owner.id, 'Owner example'],
      [staffContact, staff.id, 'Staff example'],
    ])
      await operator.query(
        'INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,owner_id) VALUES($1,$2,$3,$4,$5)',
        [tenant, id, name, 'private@example.test', principal],
      )
    const taskIds = Array.from({ length: 5 }, () => randomUUID())
    for (const [index, dueAt, status, contact] of [
      [0, '2026-10-09T07:59:59.123456Z', 'open', ownerContact],
      [1, '2026-10-09T07:59:59.123457Z', 'open', staffContact],
      [2, asOf, 'open', staffContact],
      [3, asOf, 'open', staffContact],
      [4, '2026-10-01T00:00:00Z', 'completed', ownerContact],
    ] as const)
      await operator.query(
        'INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at,status) VALUES($1,$2,$3,$4,$5,$6)',
        [tenant, taskIds[index], contact, 'Call literal %_\\ example', dueAt, status],
      )
    ids.set(tenant, { ownerContact, staffContact, taskIds })
  }
  const counts = async () =>
    (
      await operator.query<Record<string, unknown>>(
        'SELECT (SELECT count(*) FROM archava_crm.commands) AS commands,(SELECT count(*) FROM archava_crm.audit) AS audit,(SELECT count(*) FROM archava_crm.followups) AS followups',
      )
    ).rows
  const before = await counts()
  for (const [index, tenant] of tenants.entries()) {
    const service = services[index]
    const fixture = ids.get(tenant)
    assert.ok(service && fixture)
    const all = await service.listFollowups(tenant, owner, { asOf })
    assert.equal(all.items.length, 4)
    assert.equal(all.items[0]?.id, fixture.taskIds[0])
    assert.equal(all.items[1]?.id, fixture.taskIds[1])
    assert.equal(all.items[0]?.dueAt, '2026-10-09T07:59:59.123456Z')
    const seen: string[] = []
    let page = await service.listFollowups(tenant, owner, { asOf, limit: 1 })
    while (true) {
      seen.push(...page.items.map((item) => item.id))
      if (!page.next) break
      page = await service.listFollowups(tenant, owner, {
        asOf: page.asOf,
        limit: 1,
        after: page.next,
      })
      assert.ok(seen.length <= 4)
    }
    assert.deepEqual(
      seen,
      all.items.map((item) => item.id),
    )
    assert.equal(
      (await service.listFollowups(tenant, owner, { asOf, bucket: 'overdue' })).items.length,
      2,
    )
    assert.equal(
      (await service.listFollowups(tenant, owner, { asOf, bucket: 'upcoming' })).items.length,
      2,
    )
    assert.equal(
      (await service.listFollowups(tenant, owner, { asOf, search: '%_\\' })).items.length,
      4,
    )
    assert.equal(
      (await service.listFollowups(tenant, owner, { asOf, search: "' OR true --" })).items.length,
      0,
    )
    assert.ok(!JSON.stringify(all).includes('private@example.test'))
    const assigned = await service.listFollowups(tenant, staff, { asOf })
    assert.equal(assigned.items.length, 3)
    assert.ok(assigned.items.every((item) => item.contactId === fixture.staffContact))
    assert.deepEqual(await service.listFollowups(tenant, machine, { asOf }), assigned)
    const module = consumers.conversations[index]
    assert.ok(module)
    const reply = await module
      .createCrmConversation(service, new module.LocalCrmPlanner())
      .ask(
        { tenantId: tenant, principal: staff, turnId: 'queue-turn', occurredAt: asOf },
        { utterance: 'tampilkan follow-up terlambat' },
      )
    assert.equal(reply.data?.kind, 'followups')
    if (reply.data?.kind !== 'followups') throw new Error('Queue projection missing')
    assert.equal(reply.data.items.length, 1)
    assert.equal(reply.data.items[0]?.contactId, fixture.staffContact)
    for (const id of ['queue-no-contact', 'queue-no-task'])
      await assert.rejects(() => service.listFollowups(tenant, { kind: 'human', id }, { asOf }), {
        code: 'forbidden',
      })
  }
  check(
    'two installed consumers share a read-only due worklist with exact-time keyset pagination, escaped search, completed exclusion and server-time buckets',
  )
  const service = services[0]
  assert.ok(service)
  await assert.rejects(() => service.listFollowups('hotel-demo', owner, { asOf }), {
    code: 'forbidden',
  })
  await operator.query(
    "UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read'] WHERE tenant_id=$1 AND principal_id=$2",
    [tenants[0], staff.id],
  )
  await assert.rejects(() => service.listFollowups(tenants[0], staff, { asOf }), {
    code: 'forbidden',
  })
  await assert.rejects(() => service.listFollowups(tenants[0], machine, { asOf }), {
    code: 'forbidden',
  })
  await operator.query(
    "UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read','followups.read'] WHERE tenant_id=$1 AND principal_id=$2",
    [tenants[0], staff.id],
  )
  const firstFixture = ids.get(tenants[0])
  assert.ok(firstFixture)
  await operator.query('UPDATE archava_crm.contacts SET owner_id=$3 WHERE tenant_id=$1 AND id=$2', [
    tenants[0],
    firstFixture.staffContact,
    owner.id,
  ])
  assert.equal((await service.listFollowups(tenants[0], staff, { asOf })).items.length, 0)
  assert.deepEqual(await counts(), before)
  check(
    'worklist rechecks tenant, both read grants, delegated staff scope, revocation and reassignment; dashboard/Archava reads create zero command/audit/task writes',
  )
  for (const [index, tenant] of tenants.entries()) {
    const current = services[index],
      fixture = ids.get(tenant),
      module = consumers.conversations[index]
    assert.ok(current && fixture && module)
    const hiddenId = randomUUID()
    for (let offset = 0; offset < 21; offset++)
      await operator.query(
        'INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) VALUES($1,$2,$3,$4,$5)',
        [
          tenant,
          offset === 20 ? hiddenId : randomUUID(),
          fixture.ownerContact,
          'Task beyond contact page',
          offset === 20 ? '2030-01-02T00:00:00Z' : '2030-01-01T00:00:00Z',
        ],
      )
    assert.ok(
      !(await current.detail(tenant, owner, fixture.ownerContact)).followups.some(
        (item) => item.id === hiddenId,
      ),
    )
    const detail = await current.followup(tenant, owner, hiddenId)
    assert.equal(detail.followup.id, hiddenId)
    assert.equal(detail.contact.id, fixture.ownerContact)
    assert.ok(!JSON.stringify(detail).includes('private@example.test'))
    await assert.rejects(() => current.followup(tenant, staff, hiddenId), { code: 'not_found' })
    for (const principal of ['queue-no-contact', 'queue-no-task'])
      await assert.rejects(
        () => current.followup(tenant, { kind: 'human', id: principal }, hiddenId),
        { code: 'forbidden' },
      )
    await operator.query(
      "UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read','followups.read','followups.write'] WHERE tenant_id=$1 AND principal_id=$2",
      [tenant, owner.id],
    )
    const beforeDraft = await counts()
    const reply = await module
      .createCrmConversation(current, new module.LocalCrmPlanner())
      .ask(
        { tenantId: tenant, principal: owner, turnId: 'complete-prepare', occurredAt: asOf },
        { utterance: 'tinjau penyelesaian follow-up', followupId: hiddenId },
      )
    assert.equal(reply.data?.kind, 'completion_draft')
    assert.deepEqual(await counts(), beforeDraft)
  }
  check(
    'exact task detail and completion preparation share both installed consumers, reach tasks beyond the contact page, recheck both read grants/resource scope and create no mutation',
  )
}
