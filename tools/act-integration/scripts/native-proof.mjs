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
  for (const [index, tenant] of tenants.entries()) {
    const contact = await service.mutate(tenant, owner, {
      requestId: randomUUID(),
      command: { operation: 'contact.create', data: { fullName: 'Synthetic completion contact' } },
    })
    const task = await service.mutate(tenant, owner, {
      requestId: randomUUID(),
      command: {
        operation: 'followup.create',
        contactId: contact.id,
        contactRevision: 1,
        summary: 'Synthetic completion task',
        dueAt: '2026-10-20T10:00:00Z',
      },
    })
    const host = await make(tenant, 'owner-a', index),
      inputs = { followupId: task.id, expectedRevision: 1 }
    const before = await counts()
    const reviews = await Promise.all(
      Array.from({ length: 4 }, () => host.reviewCompletion(inputs)),
    )
    assert.equal(new Set(reviews.map((x) => x.intentId)).size, 1)
    assert.deepEqual(await counts(), before)
    await assert.rejects(
      () =>
        host.presentCompletion({
          inputs: { ...inputs, expectedRevision: 2 },
          confirmation: consent(reviews[0]),
        }),
      { code: 'conflict' },
    )
    await assert.rejects(() =>
      make(tenant, 'owner-b').then((h) =>
        h.presentCompletion({ inputs, confirmation: consent(reviews[0]) }),
      ),
    )
    const creationReview = await host.review({
      contactId: contact.id,
      contactRevision: 2,
      summary: 'Unused creation challenge',
      dueAt: '2026-10-20T10:00:00Z',
    })
    await assert.rejects(() =>
      host.presentCompletion({ inputs, confirmation: consent(creationReview) }),
    )
    assert.equal(await host.cancelPending(creationReview.intentId), true)
    const command = {
      requestId: randomUUID(),
      command: { operation: 'followup.complete', id: task.id, expectedRevision: 1 },
    }
    const machine = { kind: 'service', id: serviceId, onBehalfOf: 'owner-a' }
    await assert.rejects(() => service.mutate(tenant, machine, command), { code: 'forbidden' })
    await assert.rejects(
      () => service.mutateApproved(tenant, machine, command, { confirmed: true }),
      { code: 'forbidden' },
    )
    assert.deepEqual(await counts(), before)
    const executions = await Promise.allSettled(
      reviews.map((review) => host.presentCompletion({ inputs, confirmation: consent(review) })),
    )
    assert.equal(
      executions.filter((x) => x.status === 'fulfilled' && x.value.execution === 'succeeded')
        .length,
      1,
    )
    assert.equal((await service.followup(tenant, owner, task.id)).followup.status, 'completed')
    assert.equal((await host.status(reviews[0].intentId)).receipt.revision, 2)
    const after = await counts()
    assert.equal(after.followups, before.followups)
    assert.equal(after.commands, before.commands + 1)
    assert.equal(after.audit, before.audit + 1)
    assert.deepEqual(
      (
        await admin.query(
          'SELECT actor_id,initiator_id,operation FROM archava_crm.audit WHERE tenant_id=$1 AND request_id=$2',
          [tenant, reviews[0].intentId],
        )
      ).rows[0],
      { actor_id: serviceId, initiator_id: 'owner-a', operation: 'followup.complete' },
    )
    await assert.rejects(() => host.reviewCompletion(inputs), { code: 'conflict' })
    assert.equal(await host.cancelPending(reviews[0].intentId), false)
  }
  check(
    'two installed native consumers complete one explicitly reviewed task under concurrent presentations, with exact revision and one service/human audit; forged, cross-owner and cross-action consent remains denied',
  )

  const completionContact = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: { operation: 'contact.create', data: { fullName: 'Synthetic completion revocation' } },
  })
  const completionTask = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: {
      operation: 'followup.create',
      contactId: completionContact.id,
      contactRevision: 1,
      summary: 'Completion revocation fixture',
      dueAt: '2026-10-20T10:00:00Z',
    },
  })
  const completionHost = await make(tenant),
    completionInputs = { followupId: completionTask.id, expectedRevision: 1 }
  const completionReview = await completionHost.reviewCompletion(completionInputs),
    beforeCompletion = await counts()
  for (const principal of [serviceId, owner.id]) {
    await admin.query(
      'UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2',
      [tenant, principal],
    )
    await assert.rejects(
      () =>
        completionHost.presentCompletion({
          inputs: completionInputs,
          confirmation: consent(completionReview),
        }),
      { code: 'forbidden' },
    )
    await admin.query(
      'UPDATE archava_crm.memberships SET active=true WHERE tenant_id=$1 AND principal_id=$2',
      [tenant, principal],
    )
  }
  assert.deepEqual(await counts(), beforeCompletion)
  await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: { operation: 'followup.complete', id: completionTask.id, expectedRevision: 1 },
  })
  await assert.rejects(
    () =>
      completionHost.presentCompletion({
        inputs: completionInputs,
        confirmation: consent(completionReview),
      }),
    { code: 'conflict' },
  )
  assert.equal(await completionHost.cancelPending(completionReview.intentId), true)
  check(
    'completion rechecks current service/human membership and refuses an already completed stale review before spending consent',
  )

  const lostTask = await service.mutate(tenant, owner, {
    requestId: randomUUID(),
    command: {
      operation: 'followup.create',
      contactId: completionContact.id,
      contactRevision: 2,
      summary: 'Synthetic completion lost response',
      dueAt: '2026-10-20T10:00:00Z',
    },
  })
  const completionLosingService = Object.create(service)
  completionLosingService.mutateApproved = async (...args) => {
    await service.mutateApproved(...args)
    throw new Error('Synthetic completion response loss')
  }
  const completionLosingHost = await native.createPostgresNativeCrmHost(app, {
    ...settings,
    tenantId: tenant,
    humanId: owner.id,
    service: completionLosingService,
    clock: Date.now,
  })
  const lostCompletionInputs = { followupId: lostTask.id, expectedRevision: 1 },
    lostCompletion = await completionLosingHost.reviewCompletion(lostCompletionInputs)
  assert.equal(
    (
      await completionLosingHost.presentCompletion({
        inputs: lostCompletionInputs,
        confirmation: consent(lostCompletion),
      })
    ).errorCode,
    'provider_outcome_unknown',
  )
  const completedCounts = await counts()
  await reboot()
  ;({ admin, app } = pools())
  repository = await pg.PostgresCrmRepository.create(
    app,
    'isolated-stable-fingerprint-key-for-native-proof',
    verifier,
  )
  service = new crm.CrmService(repository)
  const completionRestarted = await make(tenant, 'owner-a', 1)
  assert.equal((await completionRestarted.status(lostCompletion.intentId)).receipt.id, lostTask.id)
  assert.deepEqual(await counts(), completedCounts)
  assert.equal((await service.followup(tenant, owner, lostTask.id)).followup.revision, 2)
  check(
    'lost completion response reconciles from the original receipt after real restart and another installed consumer, without repeating the task transition',
  )
  const staffId = 'completion-staff'
  await admin.query(
    "INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,'human',$3,'assigned')",
    [tenant, staffId, ['contacts.read', 'contacts.write', 'followups.read', 'followups.write']],
  )
  const staffPrincipal = { kind: 'human', id: staffId }
  const staffContact = await service.mutate(tenant, staffPrincipal, {
    requestId: randomUUID(),
    command: { operation: 'contact.create', data: { fullName: 'Synthetic assigned completion' } },
  })
  const staffTask = await service.mutate(tenant, staffPrincipal, {
    requestId: randomUUID(),
    command: {
      operation: 'followup.create',
      contactId: staffContact.id,
      contactRevision: 1,
      summary: 'Assigned task',
      dueAt: '2026-10-20T10:00:00Z',
    },
  })
  const staffHost = await make(tenant, staffId),
    staffInputs = { followupId: staffTask.id, expectedRevision: 1 }
  const staffReview = await staffHost.reviewCompletion(staffInputs),
    beforeAssignment = await counts()
  await admin.query('UPDATE archava_crm.contacts SET owner_id=$3 WHERE tenant_id=$1 AND id=$2', [
    tenant,
    staffContact.id,
    owner.id,
  ])
  await assert.rejects(
    () => staffHost.presentCompletion({ inputs: staffInputs, confirmation: consent(staffReview) }),
    { code: 'not_found' },
  )
  assert.deepEqual(await counts(), beforeAssignment)
  assert.equal((await service.followup(tenant, owner, staffTask.id)).followup.status, 'open')
  const heldReview = await completionRestarted.reviewCompletion(staffInputs)
  control = { epoch: randomUUID(), held: true }
  await assert.rejects(
    () =>
      completionRestarted.presentCompletion({
        inputs: staffInputs,
        confirmation: consent(heldReview),
      }),
    { code: 'unavailable' },
  )
  control = { ...control, held: false }
  await assert.rejects(() =>
    make(tenant).then((h) =>
      h.presentCompletion({ inputs: staffInputs, confirmation: consent(heldReview) }),
    ),
  )
  assert.deepEqual(await counts(), beforeAssignment)
  check(
    'completion refuses reassigned staff resources and restored execution hold/old-epoch consent, preserving task state and domain audit',
  )
  assert.equal(typeof NativeCrmActHost, 'function')
}
