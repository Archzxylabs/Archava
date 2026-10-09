/** Explicit synthetic live evaluation. Ordinary tests never call Google or read this key. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { lstat, readFile, mkdir, writeFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { Pool } from 'pg'
import { setDefaultResultOrder } from 'node:dns'
import type { CrmConversationReply } from '@archava/crm-assistant'
import type { CrmCommand as Command } from '@archava/crm'
import { prepareConsumers } from './consumers.js'
import { createCluster } from './cluster.js'
import { referenceShutdown } from './shutdown.js'

const root = resolve(import.meta.dirname, '../../..')
const output = join(root, 'dist/crm-gemini-proof/proof.json')
setDefaultResultOrder('ipv4first')
function required<T>(items: readonly T[], index = 0): T {
  const item = items[index]
  assert.ok(item !== undefined, 'Missing evaluation fixture')
  return item
}
if (process.argv.slice(2).join(' ') !== '--live') {
  process.stderr.write('Use crm:gemini-eval --live for the bounded synthetic Google evaluation.\n')
  process.exitCode = 2
} else {
  const shutdown = referenceShutdown()
  let consumers: Awaited<ReturnType<typeof prepareConsumers>> | undefined
  let cluster: Awaited<ReturnType<typeof createCluster>> | undefined
  let pool: Pool | undefined
  const cases: { id: string; consumer: string; operation: string; dataKind: string | null }[] = []
  const checks: string[] = []
  let failure: string | null = null
  let snapshots: unknown[] = []
  let plannerSnapshots: (() => unknown[]) | undefined
  const check = (label: string) => {
    checks.push(label)
    process.stdout.write('PASS ' + label + '\n')
  }
  try {
    const home = resolve(process.env['ARCHAVA_CRM_HOME'] ?? join(root, '.local/crm'))
    const file = join(home, 'gemini-planner.json')
    for (const [path, directory] of [
      [home, true],
      [file, false],
    ] as const) {
      const info = await lstat(path)
      assert.ok(!info.isSymbolicLink() && (info.mode & 0o077) === 0)
      assert.equal(directory ? info.isDirectory() : info.isFile(), true)
      assert.ok(!process.getuid || info.uid === process.getuid())
      if (!directory) assert.ok(info.size <= 65536)
    }
    const raw: unknown = JSON.parse(await readFile(file, 'utf8'))
    consumers = await prepareConsumers()
    const config = required(consumers.geminiModules).geminiPlannerConfigSchema.parse(raw)
    // A disabled host configuration can still be explicitly evaluated. No auto-enabling.
    assert.ok(config.apiKey.length >= 20)
    cluster = await createCluster(consumers.postgres, consumers.crm.CRM_PERMISSIONS)
    pool = new Pool(cluster.application)
    let repository = await consumers.postgres.PostgresCrmRepository.create(
      pool,
      cluster.fingerprintKey,
    )
    let service = new consumers.crm.CrmService(repository)
    const tenantIds = ['hotel-demo', 'studio-demo'] as const
    const principals = [
      { kind: 'human', id: 'hotel-owner' },
      { kind: 'human', id: 'studio-owner' },
    ] as const
    const pipelines = consumers.pipelines.map((module) => new module.CrmPipelineService(repository))
    const contacts: string[] = []
    const opportunities: string[] = []
    const mutation = (command: Command) => ({ requestId: randomUUID(), command })
    for (const [index, tenant] of tenantIds.entries()) {
      const principal = required(principals, index)
      const contact = await service.mutate(
        tenant,
        principal,
        mutation({
          operation: 'contact.create',
          data: {
            fullName: index === 0 ? 'Fictional hotel guest' : 'Fictional consulting client',
            email: 'synthetic@example.test',
            organization: null,
          },
        }),
      )
      contacts.push(contact.id)
      const opportunity = await required(pipelines, index).mutate(tenant, principal, {
        requestId: randomUUID(),
        command: {
          operation: 'opportunity.create',
          contactId: contact.id,
          contactRevision: 1,
          title: 'Demo engagement',
        },
      })
      opportunities.push(opportunity.id)
      await required(pipelines, index).mutate(tenant, principal, {
        requestId: randomUUID(),
        command: {
          operation: 'opportunity.move',
          id: opportunity.id,
          expectedRevision: 1,
          stage: 'qualified',
        },
      })
      if (index === 0)
        await required(pipelines, index).mutate(tenant, principal, {
          requestId: randomUUID(),
          command: {
            operation: 'opportunity.move',
            id: opportunity.id,
            expectedRevision: 2,
            stage: 'proposal',
          },
        })
    }
    const planners = consumers.geminiModules.map(
      (module) =>
        new module.GeminiCrmPlanner({
          apiKey: config.apiKey,
          projectTier: config.projectTier,
          maxRequests: 13,
          minimumIntervalMs: 15000,
        }),
    )
    plannerSnapshots = () => planners.map((planner) => planner.snapshot())
    const conversations = consumers.conversations.map((module, index) =>
      module.createCrmConversation(service, required(planners, index), pipelines[index]),
    )
    const context = (index: number) => ({
      tenantId: required(tenantIds, index),
      principal: required(principals, index),
      turnId: randomUUID(),
      occurredAt: new Date().toISOString(),
    })
    const fixtureCluster = cluster
    const snapshot = async () =>
      (
        await fixtureCluster.owner.query<Record<string, string>>(`SELECT
      (SELECT count(*) FROM archava_crm.contacts) AS contacts,
      (SELECT count(*) FROM archava_crm.activities) AS activities,
      (SELECT count(*) FROM archava_crm.followups) AS followups,
      (SELECT count(*) FROM archava_crm.commands) AS commands,
      (SELECT count(*) FROM archava_crm.audit) AS audit,
      (SELECT count(*) FROM archava_crm.opportunities) AS opportunities,
      (SELECT count(*) FROM archava_crm.opportunity_transitions) AS transitions`)
      ).rows[0]
    const before = await snapshot()
    const corpus = required(consumers.pilotModules).GEMINI_PILOT_CASES
    let lastCallAt = 0
    let prepared: CrmConversationReply | undefined
    let draftIndex = 0
    for (const [caseIndex, item] of corpus.entries()) {
      if (shutdown.requested) throw new Error('interrupted')
      const waitMs = Math.max(0, 16000 - (Date.now() - lastCallAt))
      if (waitMs) await Promise.race([delay(waitMs), shutdown.wait()])
      if (shutdown.requested) throw new Error('interrupted')
      // Keep filter fixtures meaningful while exercising both independent installations.
      const index = item.id === 'proposal-id' ? 0 : item.id === 'qualified-en' ? 1 : caseIndex % 2
      lastCallAt = Date.now()
      const reply = await required(conversations, index).ask(context(index), {
        utterance: item.utterance,
        contactId: contacts[index],
        opportunityId: opportunities[index],
        draft: {
          summary: 'Synthetic follow-up for human review',
          dueAt: '2026-10-15T02:00:00.000Z',
        },
      })
      assert.equal(reply.planner.providerId, 'google-gemini-crm')
      const operation = item.expected.operation
      const expectedKind = {
        'followups.list': 'followups',
        'followup.complete.prepare': 'completion_draft',
        'contacts.list': 'contacts',
        'contact.summary': 'summary',
        'followup.prepare': 'draft',
        'opportunities.list': 'opportunities',
        'opportunity.summary': 'opportunity',
        unsupported: 'help',
      }[operation]
      assert.equal(reply.data?.kind, expectedKind)
      if (reply.data?.kind === 'contacts') assert.equal(reply.data.items[0]?.id, contacts[index])
      if (reply.data?.kind === 'summary') assert.equal(reply.data.contact.id, contacts[index])
      if (reply.data?.kind === 'opportunities')
        assert.equal(reply.data.items[0]?.id, opportunities[index])
      if (reply.data?.kind === 'opportunity')
        assert.equal(reply.data.opportunity.id, opportunities[index])
      if (reply.data?.kind === 'draft') {
        prepared = reply
        draftIndex = index
      }
      assert.ok(!JSON.stringify(reply).includes('synthetic@example.test'))
      assert.deepEqual(await snapshot(), before)
      cases.push({
        id: item.id,
        consumer: index === 0 ? 'hospitality' : 'professional-services',
        operation,
        dataKind: reply.data?.kind ?? null,
      })
      process.stdout.write('PASS live synthetic case ' + item.id + '\n')
    }
    snapshots = planners.map((planner) => planner.snapshot())
    check('13 live plans in two cold-installed consumers; reads/drafts create zero CRM writes')
    const calls = required(planners).snapshot().requests
    await assert.rejects(() =>
      required(conversations).ask(
        { ...context(0), principal: { kind: 'human', id: 'public-customer' } },
        { utterance: required(corpus).utterance },
      ),
    )
    assert.equal(required(planners).snapshot().requests, calls)
    await assert.rejects(
      () => required(planners).plan('Private client name and confidential requirement'),
      (error: unknown) =>
        !!error && typeof error === 'object' && 'code' in error && error.code === 'synthetic_only',
    )
    assert.equal(required(planners).snapshot().requests, calls)
    check('unauthorized callers and arbitrary private text cause zero provider requests')
    assert.ok(prepared?.data?.kind === 'draft')
    const draft = prepared.data.draft
    // Explicit evaluator approval models the human Save click. The model has no mutation port.
    const reviewed = mutation({
      operation: 'followup.create',
      contactId: draft.contactId,
      contactRevision: draft.contactRevision,
      summary: draft.summary,
      dueAt: draft.dueAt,
    })
    const receipts = await Promise.all(
      Array.from({ length: 8 }, () =>
        service.mutate(required(tenantIds, draftIndex), required(principals, draftIndex), reviewed),
      ),
    )
    assert.ok(receipts.every((receipt) => JSON.stringify(receipt) === JSON.stringify(receipts[0])))
    assert.equal(
      (
        await cluster.owner.query('SELECT 1 FROM archava_crm.audit WHERE request_id=$1', [
          reviewed.requestId,
        ])
      ).rowCount,
      1,
    )
    await assert.rejects(() =>
      service.mutate(
        required(tenantIds, draftIndex),
        { kind: 'service', id: 'hotel-bot', onBehalfOf: 'hotel-owner' },
        reviewed,
      ),
    )
    check(
      'explicit human save plus eight retries yields one task/audit; machine writes remain refused',
    )
    await pool.end()
    await cluster.restart()
    pool = new Pool(cluster.application)
    repository = await consumers.postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey)
    service = new consumers.crm.CrmService(repository)
    assert.deepEqual(
      await service.mutate(
        required(tenantIds, draftIndex),
        required(principals, draftIndex),
        reviewed,
      ),
      receipts[0],
    )
    assert.equal(
      (
        await service.detail(
          required(tenantIds, draftIndex),
          required(principals, draftIndex),
          draft.contactId,
        )
      ).followups.length,
      1,
    )
    check('reviewed human task and original retry receipt survive actual PostgreSQL restart')
  } catch (error) {
    const known = [
      'budget_exhausted',
      'busy',
      'rate_limited',
      'timeout',
      'provider_unavailable',
      'invalid_response',
      'synthetic_only',
      'plan_mismatch',
    ]
    const code = error && typeof error === 'object' && 'code' in error ? String(error.code) : ''
    failure = known.includes(code) ? code : 'evaluation_failed'
    process.stderr.write('Gemini evaluation stopped: ' + failure + '. No retry or paid fallback.\n')
    process.exitCode = 1
  } finally {
    snapshots = plannerSnapshots?.() ?? snapshots
    await mkdir(resolve(output, '..'), { recursive: true })
    await writeFile(
      output,
      JSON.stringify(
        {
          generatedAt: new Date().toISOString(),
          passed: failure === null,
          failure,
          tierDeclaration: 'free',
          providerReceives: 'committed synthetic text only',
          cases,
          checks,
          planners: snapshots,
          consumers: consumers?.proofs ?? [],
          scope:
            'bounded intent planning, CRM reads/draft, human save; autonomous Act gates remain pending',
        },
        null,
        2,
      ) + '\n',
    )
    if (pool && !pool.ended) await pool.end()
    await cluster?.close()
    await consumers?.close()
  }
}
