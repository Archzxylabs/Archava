import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { Pool } from 'pg'
import type { prepareConsumers } from './consumers.js'
import type { createCluster } from './cluster.js'
import type { CrmService, CrmPrincipal, MutationReceipt } from '@archava/crm'
import type { OpportunityCommand, OpportunityDetail } from '@archava/crm/pipeline'

export async function verifyPipeline(
  consumers: Awaited<ReturnType<typeof prepareConsumers>>,
  cluster: Awaited<ReturnType<typeof createCluster>>,
  pool: Pool,
  crm: CrmService,
  contacts: readonly [string, string],
  check: (label: string) => void,
) {
  const denied = (error: unknown) =>
    !!error &&
    typeof error === 'object' &&
    'code' in error &&
    ['forbidden', 'not_found'].includes(String(error.code))
  const conflict = (error: unknown) =>
    !!error && typeof error === 'object' && 'code' in error && error.code === 'conflict'
  const owner = { kind: 'human', id: 'hotel-owner' } as const
  const studio = { kind: 'human', id: 'studio-owner' } as const
  const viewer = { kind: 'human', id: 'hotel-viewer' } as const
  const editor = { kind: 'human', id: 'hotel-editor' } as const
  const intent = (command: OpportunityCommand) => ({ requestId: randomUUID(), command })
  const repository = await consumers.postgres.PostgresCrmRepository.create(
    pool,
    cluster.fingerprintKey,
  )
  const pipelines = consumers.pipelines.map((module) => new module.CrmPipelineService(repository))
  const pipeline = pipelines[0]
  const studioPipeline = pipelines[1]
  assert.ok(pipeline && studioPipeline)
  const current = await crm.detail('hotel-demo', owner, contacts[0])
  const create = intent({
    operation: 'opportunity.create',
    contactId: contacts[0],
    contactRevision: current.contact.revision,
    title: 'Group stay inquiry',
  })
  const receipts = await Promise.all(
    Array.from({ length: 8 }, () => pipeline.mutate('hotel-demo', owner, create)),
  )
  const receipt = receipts[0]
  assert.ok(receipt)
  assert.ok(receipts.every((item) => JSON.stringify(item) === JSON.stringify(receipt)))
  assert.equal((await pipeline.detail('hotel-demo', owner, receipt.id)).transitions.length, 1)
  assert.equal(
    (
      await cluster.owner.query('SELECT 1 FROM archava_crm.audit WHERE request_id=$1', [
        create.requestId,
      ])
    ).rowCount,
    1,
  )
  assert.equal(
    (await crm.detail('hotel-demo', owner, contacts[0])).contact.revision,
    current.contact.revision,
  )
  const second = await studioPipeline.mutate(
    'studio-demo',
    studio,
    intent({
      operation: 'opportunity.create',
      contactId: contacts[1],
      contactRevision: (await crm.detail('studio-demo', studio, contacts[1])).contact.revision,
      title: 'Consulting engagement',
    }),
  )
  assert.equal((await studioPipeline.list('studio-demo', studio)).items[0]?.id, second.id)
  await assert.rejects(() => pipeline.detail('hotel-demo', owner, second.id), denied)
  await assert.rejects(() => pipeline.list('studio-demo', owner), denied)
  await studioPipeline.mutate(
    'studio-demo',
    studio,
    intent({
      operation: 'opportunity.move',
      id: second.id,
      expectedRevision: 1,
      stage: 'lost',
      reason: 'Client postponed the engagement',
    }),
  )
  const lost = await studioPipeline.detail('studio-demo', studio, second.id)
  assert.equal(lost.opportunity.lossReason, 'Client postponed the engagement')
  assert.equal(lost.transitions[0]?.to, 'lost')
  await assert.rejects(
    () =>
      studioPipeline.mutate(
        'studio-demo',
        studio,
        intent({
          operation: 'opportunity.move',
          id: second.id,
          expectedRevision: 2,
          stage: 'qualified',
        }),
      ),
    conflict,
  )
  check(
    'two independent installed pipeline consumers; tenant isolation and eight retries produce one history/audit event',
  )

  await assert.rejects(() =>
    pipeline.mutate(
      'hotel-demo',
      owner,
      intent({ ...create.command, contactRevision: 0 } as OpportunityCommand),
    ),
  )
  if (current.contact.revision > 1)
    await assert.rejects(
      () =>
        pipeline.mutate(
          'hotel-demo',
          owner,
          intent({
            ...create.command,
            contactRevision: current.contact.revision - 1,
          } as OpportunityCommand),
        ),
      conflict,
    )
  await assert.rejects(
    () =>
      pipeline.mutate(
        'hotel-demo',
        owner,
        intent({
          operation: 'opportunity.move',
          id: receipt.id,
          expectedRevision: 1,
          stage: 'won',
        }),
      ),
    conflict,
  )
  await assert.rejects(() =>
    pipeline.mutate(
      'hotel-demo',
      owner,
      intent({ operation: 'opportunity.move', id: receipt.id, expectedRevision: 1, stage: 'lost' }),
    ),
  )
  const move = intent({
    operation: 'opportunity.move',
    id: receipt.id,
    expectedRevision: 1,
    stage: 'qualified',
  })
  const race = await Promise.allSettled([
    pipeline.mutate('hotel-demo', owner, move),
    pipeline.mutate(
      'hotel-demo',
      owner,
      intent({
        operation: 'opportunity.move',
        id: receipt.id,
        expectedRevision: 1,
        stage: 'lost',
        reason: 'Budget',
      }),
    ),
  ])
  assert.equal(race.filter((item) => item.status === 'fulfilled').length, 1)
  assert.ok(race.some((item) => item.status === 'rejected' && conflict(item.reason)))
  let detail = await pipeline.detail('hotel-demo', owner, receipt.id)
  assert.equal(detail.transitions.length, 2)
  if (detail.opportunity.stage === 'qualified') {
    await pipeline.mutate(
      'hotel-demo',
      owner,
      intent({
        operation: 'opportunity.move',
        id: receipt.id,
        expectedRevision: 2,
        stage: 'proposal',
      }),
    )
    await pipeline.mutate(
      'hotel-demo',
      owner,
      intent({ operation: 'opportunity.move', id: receipt.id, expectedRevision: 3, stage: 'won' }),
    )
  } else assert.equal(detail.opportunity.lossReason, 'Budget')
  detail = await pipeline.detail('hotel-demo', owner, receipt.id)
  await assert.rejects(
    () =>
      pipeline.mutate(
        'hotel-demo',
        owner,
        intent({
          operation: 'opportunity.move',
          id: receipt.id,
          expectedRevision: detail.opportunity.revision,
          stage: 'new',
        }),
      ),
    conflict,
  )
  assert.deepEqual(await pipeline.mutate('hotel-demo', owner, create), receipt)
  await assert.rejects(
    () =>
      pipeline.mutate('hotel-demo', owner, {
        ...create,
        command: { ...create.command, title: 'Changed intent' } as OpportunityCommand,
      }),
    conflict,
  )
  check(
    'stage races, invalid jumps, mandatory loss reason, terminal outcomes and current revision enforced',
  )

  await assert.rejects(() => pipeline.list('hotel-demo', viewer), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'pipeline.read') WHERE principal_id='hotel-viewer'",
  )
  assert.ok((await pipeline.list('hotel-demo', viewer)).items.length)
  await assert.rejects(() => pipeline.mutate('hotel-demo', viewer, create), denied)
  assert.equal((await pipeline.list('hotel-demo', editor)).items.length, 0)
  await assert.rejects(() => pipeline.detail('hotel-demo', editor, receipt.id), denied)
  await assert.rejects(() => pipeline.mutate('hotel-demo', editor, create), conflict)
  await assert.rejects(
    () => pipeline.mutate('hotel-demo', editor, { ...create, requestId: randomUUID() }),
    denied,
  )
  const bot: CrmPrincipal = { kind: 'service', id: 'hotel-bot', onBehalfOf: 'hotel-owner' }
  await assert.rejects(() => pipeline.mutate('hotel-demo', bot, create), denied)
  await assert.rejects(() => pipeline.list('hotel-demo', bot), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'pipeline.read') WHERE principal_id='hotel-bot'",
  )
  const pipelineModule = consumers.pipelines[0]
  assert.ok(pipelineModule)
  const tools = pipelineModule.createPipelineTools(pipeline, 'hotel-demo', bot)
  const projected = await tools.invoke('crm.opportunities.list', {})
  assert.equal(JSON.stringify(projected).includes('ownerId'), false)
  assert.equal(JSON.stringify(projected).includes('email'), false)
  await assert.rejects(() => tools.invoke('crm.opportunity.move', {}), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_remove(permissions,'pipeline.write') WHERE principal_id='hotel-owner'",
  )
  await assert.rejects(() => pipeline.mutate('hotel-demo', owner, create), denied)
  await cluster.owner.query(
    "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'pipeline.write') WHERE principal_id='hotel-owner'",
  )
  check(
    'pipeline-specific grants, assigned resources, delegation intersection, read-only tools and replay revocation',
  )

  const rollback = intent({
    operation: 'opportunity.create',
    contactId: contacts[0],
    contactRevision: current.contact.revision,
    title: 'Atomic retry inquiry',
  })
  await cluster.owner.query(
    "CREATE FUNCTION public.pipeline_fail_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture'; END $$; CREATE TRIGGER pipeline_fail BEFORE INSERT ON archava_crm.audit FOR EACH ROW EXECUTE FUNCTION public.pipeline_fail_audit()",
  )
  await assert.rejects(() => pipeline.mutate('hotel-demo', owner, rollback))
  assert.equal(
    (
      await cluster.owner.query('SELECT 1 FROM archava_crm.commands WHERE request_id=$1', [
        rollback.requestId,
      ])
    ).rowCount,
    0,
  )
  assert.equal(
    (
      await cluster.owner.query('SELECT 1 FROM archava_crm.opportunities WHERE title=$1', [
        rollback.command.operation === 'opportunity.create' ? rollback.command.title : '',
      ])
    ).rowCount,
    0,
  )
  assert.equal(
    (
      await cluster.owner.query(
        'SELECT 1 FROM archava_crm.opportunity_transitions WHERE request_id=$1',
        [rollback.requestId],
      )
    ).rowCount,
    0,
  )
  await cluster.owner.query(
    'DROP TRIGGER pipeline_fail ON archava_crm.audit; DROP FUNCTION public.pipeline_fail_audit()',
  )
  const atomicReceipt = await pipeline.mutate('hotel-demo', owner, rollback)
  for (let index = 0; index < 8; index++) {
    const [snapshot]: [OpportunityDetail, MutationReceipt] = await Promise.all([
      pipeline.detail('hotel-demo', owner, atomicReceipt.id),
      pipeline.mutate(
        'hotel-demo',
        owner,
        intent({
          operation: 'opportunity.move',
          id: atomicReceipt.id,
          expectedRevision: index + 1,
          stage: index % 2 === 0 ? 'qualified' : 'new',
        }),
      ),
    ])
    assert.equal(snapshot.transitions[0]?.revision, snapshot.opportunity.revision)
    assert.equal(snapshot.transitions[0]?.to, snapshot.opportunity.stage)
  }
  const page = await pipeline.list('hotel-demo', owner, { limit: 1 })
  assert.equal(page.items.length, 1)
  assert.ok(page.next)
  assert.equal(
    (await pipeline.list('hotel-demo', owner, { after: page.next, limit: 1 })).items.length,
    1,
  )
  assert.equal(
    (await pipeline.list('hotel-demo', owner, { stage: 'new', search: 'Atomic' })).items.length,
    1,
  )
  assert.equal((await pipeline.list('hotel-demo', owner, { search: '%' })).items.length, 0)
  assert.equal((await pool.query('SELECT 1 FROM archava_crm.opportunities')).rowCount, 0)
  await assert.rejects(() =>
    cluster.owner.query(
      'UPDATE archava_crm.opportunity_transitions SET reason=$1 WHERE request_id=$2',
      ['tamper', create.requestId],
    ),
  )
  await assert.rejects(() =>
    cluster.owner.query('DELETE FROM archava_crm.opportunity_transitions WHERE request_id=$1', [
      create.requestId,
    ]),
  )
  check(
    'atomic rollback includes command/history/audit; bounded pages, escaped search, forced RLS and immutable history',
  )

  await cluster.owner.query('CREATE DATABASE archava_legacy_fixture')
  const legacy = new Pool({ ...cluster.target, database: 'archava_legacy_fixture' })
  try {
    const first = await readFile(
      new URL('../../../packages/crm-pg/migrations/001_crm_contacts.sql', import.meta.url),
      'utf8',
    )
    await legacy.query(
      'CREATE SCHEMA archava_crm; CREATE TABLE archava_crm.schema_migrations(id text PRIMARY KEY,checksum text NOT NULL,applied_at timestamptz NOT NULL DEFAULT now())',
    )
    await legacy.query(first)
    const checksum = await consumers.postgres.crmMigrationChecksum()
    await legacy.query('INSERT INTO archava_crm.schema_migrations(id,checksum) VALUES($1,$2)', [
      '001_crm_contacts',
      checksum,
    ])
    await legacy.query(
      "INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('legacy','owner','human',ARRAY['contacts.read','contacts.write'],'all')",
    )
    const id = randomUUID(),
      requestId = randomUUID()
    await legacy.query(
      "INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('legacy',$1,'Retained contact','owner')",
      [id],
    )
    await legacy.query(
      "INSERT INTO archava_crm.activities(tenant_id,id,contact_id,kind,summary,actor_id) VALUES('legacy',$1,$2,'note','Retained note','owner')",
      [randomUUID(), id],
    )
    await legacy.query(
      "INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) VALUES('legacy',$1,$2,'Retained task',now()+interval '1 day')",
      [randomUUID(), id],
    )
    await legacy.query(
      "INSERT INTO archava_crm.commands(tenant_id,request_id,actor_id,fingerprint,receipt) VALUES('legacy',$1,'owner',$2,$3)",
      [requestId, 'a'.repeat(64), { kind: 'contact', id, revision: 1 }],
    )
    await legacy.query(
      "INSERT INTO archava_crm.audit(tenant_id,request_id,actor_id,operation,resource_id,revision) VALUES('legacy',$1,'owner','contact.create',$2,1)",
      [requestId, id],
    )
    const snapshot = async () =>
      Promise.all(
        ['memberships', 'contacts', 'activities', 'followups', 'commands', 'audit'].map(
          async (table) =>
            (await legacy.query<Record<string, unknown>>(`SELECT * FROM archava_crm.${table}`))
              .rows,
        ),
      )
    const before = await snapshot()
    await consumers.postgres.migrateCrm(legacy)
    await consumers.postgres.migrateCrm(legacy)
    assert.deepEqual(await snapshot(), before)
    assert.equal((await legacy.query('SELECT * FROM archava_crm.schema_migrations')).rowCount, 2)
    await legacy.query(
      "UPDATE archava_crm.schema_migrations SET checksum='tampered' WHERE id='001_crm_contacts'",
    )
    await assert.rejects(() => consumers.postgres.migrateCrm(legacy), /modified/)
    await legacy.query('UPDATE archava_crm.schema_migrations SET checksum=$1 WHERE id=$2', [
      checksum,
      '001_crm_contacts',
    ])
    await consumers.postgres.migrateCrm(legacy)
  } finally {
    await legacy.end()
  }
  check(
    'real v1-to-v2 upgrade preserves records, audit, receipts and original grants; modified ledger refused',
  )
  return { create, receipt }
}
