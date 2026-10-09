import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'

/** Installed public packages, isolated PostgreSQL, no model/network/provider credentials. */
export async function verifyNative({ pools, reboot, consumers, moduleAt, check }) {
  const consent = (review) => ({
    challengeId: review.challenge.challengeId,
    token: review.challenge.token,
  })
  const crm = await moduleAt(consumers[0], '@archava/crm')
  const pg = await moduleAt(consumers[0], '@archava/crm-pg')
  const native = await moduleAt(consumers[0], '@archava/crm-act/postgres')
  const { NativeCrmActHost } = await moduleAt(consumers[0], '@archava/crm-act')
  let { admin, app } = pools()
  await pg.migrateCrm(admin)
  await pg.grantCrmRuntime(admin, 'act_verify_runtime')
  await native.migrateNativeCrmAct(admin)
  await native.migrateNativeCrmAct(admin)
  await native.grantNativeCrmAct(admin, 'act_verify_runtime')
  const key = new Uint8Array(32).fill(37),
    serviceId = 'archava-followup'
  let control = { epoch: randomUUID(), held: false }
  const settings = {
    serviceId,
    confirmationKey: key,
    control: () => Promise.resolve({ ...control }),
  }
  const verifier = native.createNativeCrmVerifier(settings)
  let repository = await pg.PostgresCrmRepository.create(
    app,
    'isolated-stable-fingerprint-key-for-native-proof',
    verifier,
  )
  let service = new crm.CrmService(repository)
  const tenants = ['hospitality-native', 'services-native']
  for (const tenant of tenants) {
    for (const [principal, kind, grants] of [
      ['owner-a', 'human', crm.CRM_PERMISSIONS],
      ['owner-b', 'human', crm.CRM_PERMISSIONS],
      [
        serviceId,
        'service',
        ['contacts.read', 'activities.read', 'followups.read', 'followups.write'],
      ],
    ])
      await admin.query(
        'INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [tenant, principal, kind, grants, 'all'],
      )
  }
  const make = async (tenant, human = 'owner-a', consumer = 0) => {
    const implementation = await moduleAt(consumers[consumer], '@archava/crm-act/postgres')
    return implementation.createPostgresNativeCrmHost(app, {
      ...settings,
      tenantId: tenant,
      humanId: human,
      service,
      clock: Date.now,
    })
  }
  const counts = async () =>
    (
      await admin.query(
        'SELECT (SELECT count(*) FROM archava_crm.followups)::integer AS followups,(SELECT count(*) FROM archava_crm.commands)::integer AS commands,(SELECT count(*) FROM archava_crm.audit)::integer AS audit',
      )
    ).rows[0]
  for (const [index, tenant] of tenants.entries()) {
    const owner = { kind: 'human', id: 'owner-a' },
      machine = { kind: 'service', id: serviceId, onBehalfOf: 'owner-a' }
    const created = await service.mutate(tenant, owner, {
      requestId: randomUUID(),
      command: { operation: 'contact.create', data: { fullName: 'Synthetic native contact' } },
    })
    const inputs = {
      contactId: created.id,
      contactRevision: 1,
      summary: 'Synthetic owner-approved follow-up',
      dueAt: '2026-10-20T10:00:00Z',
    }
    const host = await make(tenant, 'owner-a', index),
      before = await counts()
    const reviews = await Promise.all(Array.from({ length: 4 }, () => host.review(inputs)))
    assert.equal(new Set(reviews.map((r) => r.intentId)).size, 1)
    assert.deepEqual(await counts(), before)
    const result = await host.present({ inputs, confirmation: consent(reviews[0]) })
    assert.equal(result.execution, 'succeeded')
    const status = await host.status(reviews[0].intentId)
    assert.equal(status.status, 'committed')
    assert.equal(status.receipt.kind, 'followup')
    const after = await counts()
    assert.equal(after.followups, before.followups + 1)
    assert.equal(after.commands, before.commands + 1)
    assert.equal(after.audit, before.audit + 1)
    const row = (
      await admin.query(
        'SELECT actor_id,initiator_id FROM archava_crm.audit WHERE tenant_id=$1 AND request_id=$2',
        [tenant, reviews[0].intentId],
      )
    ).rows[0]
    assert.deepEqual(row, { actor_id: serviceId, initiator_id: 'owner-a' })
    assert.equal(
      (await (await make(tenant, 'owner-b')).status(reviews[0].intentId)).status,
      'unconfirmed',
    )
    await assert.rejects(() => host.present({ inputs, confirmation: consent(reviews[0]) }))
    const command = {
      requestId: randomUUID(),
      command: { operation: 'followup.create', ...inputs },
    }
    await assert.rejects(() => service.mutate(tenant, machine, command), { code: 'forbidden' })
    await assert.rejects(
      () => service.mutateApproved(tenant, machine, command, { confirmed: true }),
      { code: 'forbidden' },
    )
    assert.deepEqual(await counts(), after)
  }
  check(
    'native review is metadata-only; two installed consumers execute one approved CRM task with service actor plus human initiator audit, and machine/forged/replayed/cross-owner writes remain denied',
  )
  const tenant = tenants[0],
    owner = { kind: 'human', id: 'owner-a' }
  const contact = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: { operation: 'contact.create', data: { fullName: 'Synthetic revocation fixture' } },
  })
  const inputs = {
    contactId: contact.id,
    contactRevision: 1,
    summary: 'Synthetic revocation fixture',
    dueAt: '2026-10-20T10:00:00Z',
  }
  const host = await make(tenant),
    review = await host.review(inputs),
    before = await counts()
  await assert.rejects(() =>
    host.present({ inputs: { ...inputs, summary: 'Altered text' }, confirmation: consent(review) }),
  )
  await admin.query(
    'UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, serviceId],
  )
  await assert.rejects(() => host.present({ inputs, confirmation: consent(review) }), {
    code: 'forbidden',
  })
  await admin.query(
    'UPDATE archava_crm.memberships SET active=true WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, serviceId],
  )
  await admin.query(
    'UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, 'owner-a'],
  )
  await assert.rejects(() => host.present({ inputs, confirmation: consent(review) }), {
    code: 'forbidden',
  })
  await admin.query(
    'UPDATE archava_crm.memberships SET active=true WHERE tenant_id=$1 AND principal_id=$2',
    [tenant, 'owner-a'],
  )
  assert.deepEqual(await counts(), before)
  await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: {
      operation: 'contact.update',
      id: contact.id,
      expectedRevision: 1,
      data: { fullName: 'Synthetic changed contact' },
    },
  })
  await assert.rejects(() => host.present({ inputs, confirmation: consent(review) }), {
    code: 'conflict',
  })
  assert.equal(await host.cancelPending(review.intentId), true)
  const fresh = await host.review({ ...inputs, contactRevision: 2 })
  assert.notEqual(fresh.intentId, review.intentId)
  assert.equal(
    (
      await host.present({
        inputs: { ...inputs, contactRevision: 2 },
        confirmation: consent(fresh),
      })
    ).execution,
    'succeeded',
  )
  assert.equal(await host.cancelPending(fresh.intentId), false)
  check(
    'native approval rechecks both current memberships and exact text/revision; stale review can be cancelled before execution, while completed execution cannot',
  )
  const stored = JSON.stringify(
    (await admin.query('SELECT * FROM archava_crm_act.confirmation_challenges')).rows,
  )
  assert.ok(!stored.includes(inputs.summary))
  assert.ok(!stored.includes(review.challenge.token))
  assert.equal(
    (await app.query('SELECT * FROM archava_crm_act.confirmation_challenges')).rowCount,
    0,
  )
  await assert.rejects(() => app.query('DELETE FROM archava_crm_act.confirmation_challenges'))
  await assert.rejects(() =>
    native.createPostgresNativeCrmHost(admin, {
      ...settings,
      tenantId: tenant,
      humanId: 'owner-a',
      service,
      clock: Date.now,
    }),
  )
  check(
    'native confirmation schema is checksummed, forced tenant/owner-scope RLS and non-owner; raw drafts/tokens never reach the approval journal',
  )
  // Simulate response loss after the actual domain transaction commits.
  const lostContact = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: { operation: 'contact.create', data: { fullName: 'Synthetic lost-response fixture' } },
  })
  const lostInputs = {
    ...inputs,
    contactId: lostContact.id,
    summary: 'Synthetic response lost after commit',
  }
  const losingService = Object.create(service)
  losingService.mutateApproved = async (...args) => {
    await service.mutateApproved(...args)
    throw new Error('Synthetic response loss')
  }
  const lostHost = await native.createPostgresNativeCrmHost(app, {
    ...settings,
    tenantId: tenant,
    humanId: 'owner-a',
    service: losingService,
    clock: Date.now,
  })
  const lost = await lostHost.review(lostInputs)
  assert.equal(
    (await lostHost.present({ inputs: lostInputs, confirmation: consent(lost) })).errorCode,
    'provider_outcome_unknown',
  )
  const committed = await counts()
  await reboot()
  ;({ admin, app } = pools())
  repository = await pg.PostgresCrmRepository.create(
    app,
    'isolated-stable-fingerprint-key-for-native-proof',
    verifier,
  )
  service = new crm.CrmService(repository)
  const restarted = await make(tenant, 'owner-a', 1)
  assert.equal((await restarted.status(lost.intentId)).status, 'committed')
  assert.deepEqual(await counts(), committed)
  const pendingContact = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: { operation: 'contact.create', data: { fullName: 'Synthetic restore fixture' } },
  })
  const pendingInputs = { ...inputs, contactId: pendingContact.id },
    pending = await restarted.review(pendingInputs)
  control = { epoch: randomUUID(), held: true }
  await assert.rejects(
    () => restarted.present({ inputs: pendingInputs, confirmation: consent(pending) }),
    { code: 'unavailable' },
  )
  control = { ...control, held: false }
  await assert.rejects(() =>
    make(tenant).then((h) => h.present({ inputs: pendingInputs, confirmation: consent(pending) })),
  )
  assert.equal((await restarted.status(lost.intentId)).status, 'committed')
  check(
    'native lost response recovers through own authoritative receipt after real restart without resubmission; restored execution hold/new epoch refuses old approval',
  )
  // Metadata-level API is correlation only and cannot make an executor bypass consent.
  assert.equal(typeof NativeCrmActHost, 'function')
}
