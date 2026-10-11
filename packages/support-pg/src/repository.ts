import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  SupportError,
  supportIdSchema,
  supportPrincipalSchema,
  supportMembershipSchema,
  supportMutationSchema,
  supportReceiptSchema,
  casePageSchema,
  assigneePageSchema,
  assigneeSchema,
  caseSchema,
  supportNoteSchema,
  supportTransitionSchema,
  supportAuditSchema,
  type SupportRepository,
  type SupportPrincipal,
  type SupportAccess,
  type SupportPermission,
  type SupportMutation,
  type SupportCommand,
  type SupportCommandStatus,
  type CasePageQuery,
  type AssigneePageQuery,
} from '@archava/support'
import { supportMigrationLedger } from './migrate.js'
type Row = Record<string, unknown>
const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid support timestamp')
  return x.toISOString()
}
const nullable = (x: unknown) => (x === null ? null : instant(x))
const clock = async (client: PoolClient) =>
  instant((await client.query<Row>('SELECT clock_timestamp() AS observed')).rows[0]?.observed)
const record = (r: Row, observed: string) =>
  caseSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    ownerId: r.owner_id,
    assigneeId: r.assignee_id,
    requesterName: r.requester_name,
    subject: r.subject,
    description: r.description,
    priority: r.priority,
    state: r.state,
    dueAt: nullable(r.due_at),
    overdue:
      ['open', 'in_progress', 'waiting'].includes(String(r.state)) &&
      r.due_at instanceof Date &&
      r.due_at.getTime() <= Date.parse(observed),
    cycle: r.cycle,
    resolution: r.resolution,
    resolvedAt: nullable(r.resolved_at),
    closedAt: nullable(r.closed_at),
    revision: r.revision,
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })
const canonical = (v: unknown): string =>
  Array.isArray(v)
    ? '[' + v.map(canonical).join(',') + ']'
    : v !== null && typeof v === 'object'
      ? '{' +
        Object.entries(v)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([k, x]) => JSON.stringify(k) + ':' + canonical(x))
          .join(',') +
        '}'
      : JSON.stringify(v)
const grant = (a: SupportAccess, p: SupportPermission) => {
  if (!a.permissions.includes(p)) throw new SupportError('forbidden')
}
function authorize(a: SupportAccess, c: SupportCommand) {
  grant(a, 'cases.read')
  grant(a, c.operation === 'note.add' ? 'notes.write' : 'cases.write')
  if (c.operation === 'case.assign') {
    grant(a, 'cases.assign')
    if (a.scope !== 'all') throw new SupportError('forbidden')
  }
}
export class PostgresSupportRepository implements SupportRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}
  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable support fingerprint key must have at least 32 characters')
    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_support'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Support requires a non-owner runtime')
    const expected = await supportMigrationLedger(),
      actual = (
        await pool.query<{ id: string; checksum: string }>(
          'SELECT id,checksum FROM archava_support.schema_migrations ORDER BY id',
        )
      ).rows
    if (
      actual.length !== expected.length ||
      actual.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported support ledger')
    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_support'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'cases', 'commands', 'notes', 'transitions', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 6) throw new Error('Support tenant policies must be forced')
    return new PostgresSupportRepository(pool, key)
  }
  private async transaction<T>(
    tenant: string,
    principal: SupportPrincipal,
    work: (client: PoolClient, access: SupportAccess) => Promise<T>,
  ): Promise<T> {
    supportIdSchema.parse(tenant)
    supportPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_support.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new SupportError('forbidden')
        const m = supportMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new SupportError('forbidden')
        return m
      }
      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new SupportError('forbidden')
      let access: SupportAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new SupportError('forbidden')
        access = {
          permissions: actor.permissions.filter((x) => human.permissions.includes(x)),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }
      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      if (
        error &&
        typeof error === 'object' &&
        'code' in error &&
        ['23514', '23503', '23505', '22003', '40P01', '55P03'].includes(String(error.code))
      )
        throw new SupportError('conflict')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedCase(
    client: PoolClient,
    tenant: string,
    a: SupportAccess,
    id: string,
    lock = false,
  ) {
    grant(a, 'cases.read')
    const r = (
      await client.query<Row>(
        `SELECT * FROM archava_support.cases WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4 OR assignee_id=$4)${lock ? ' FOR UPDATE' : ' FOR SHARE'}`,
        [tenant, id, a.scope === 'all', a.ownerId],
      )
    ).rows[0]
    if (!r) throw new SupportError('not_found')
    return r
  }
  access(tenant: string, principal: SupportPrincipal) {
    return this.transaction(tenant, principal, (_client, a) => Promise.resolve(a))
  }
  assignees(tenant: string, principal: SupportPrincipal, raw: AssigneePageQuery) {
    const q = assigneePageSchema.parse(raw)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'cases.read')
      grant(a, 'cases.assign')
      if (a.scope !== 'all') throw new SupportError('forbidden')
      const rows = (
          await client.query<Row>(
            "SELECT principal_id,display_name FROM archava_support.memberships WHERE tenant_id=$1 AND active AND kind='human' AND 'cases.read'=ANY(permissions) AND ($2::text IS NULL OR principal_id>$2) ORDER BY principal_id LIMIT $3",
            [tenant, q.after ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map((r) =>
          assigneeSchema.parse({
            id: r.principal_id,
            displayName: r.display_name || r.principal_id,
          }),
        )
      return { items, next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null }
    })
  }
  cases(tenant: string, principal: SupportPrincipal, raw: CasePageQuery) {
    const q = casePageSchema.parse(raw)
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'cases.read')
      const observed = await clock(client),
        rows = (
          await client.query<Row>(
            "SELECT * FROM archava_support.cases WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3 OR assignee_id=$3) AND ($4::uuid IS NULL OR id>$4) AND ($5::text IS NULL OR state=$5) AND ($6::text IS NULL OR priority=$6) AND ($7::boolean IS NULL OR (state IN ('open','in_progress','waiting') AND due_at IS NOT NULL AND due_at<=$8::timestamptz)=$7) ORDER BY id LIMIT $9",
            [
              tenant,
              a.scope === 'all',
              a.ownerId,
              q.after ?? null,
              q.state ?? null,
              q.priority ?? null,
              q.overdue ?? null,
              observed,
              q.limit + 1,
            ],
          )
        ).rows,
        items = rows.slice(0, q.limit).map((r) => record(r, observed))
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  detail(tenant: string, principal: SupportPrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, async (client, a) => {
      const r = await this.ownedCase(client, tenant, a, id),
        observed = await clock(client)
      const notes = a.permissions.includes('notes.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_support.notes WHERE tenant_id=$1 AND case_id=$2 ORDER BY occurred_at DESC,id DESC LIMIT 101',
              [tenant, id],
            )
          ).rows
        : null
      const transitions = a.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_support.transitions WHERE tenant_id=$1 AND case_id=$2 ORDER BY case_revision DESC LIMIT 101',
              [tenant, id],
            )
          ).rows
        : null
      const audit = a.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_support.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY revision DESC LIMIT 101',
              [tenant, id],
            )
          ).rows
        : null
      return {
        record: record(r, observed),
        notes:
          notes?.slice(0, 100).map((n) =>
            supportNoteSchema.parse({
              id: n.id,
              caseId: n.case_id,
              authorId: n.author_id,
              body: n.body,
              requestId: n.request_id,
              occurredAt: instant(n.occurred_at),
            }),
          ) ?? null,
        notesHasMore: (notes?.length ?? 0) > 100,
        transitions:
          transitions?.slice(0, 100).map((n) =>
            supportTransitionSchema.parse({
              requestId: n.request_id,
              actorId: n.actor_id,
              operation: n.operation,
              previousAssigneeId: n.previous_assignee_id,
              assigneeId: n.assignee_id,
              previousPriority: n.previous_priority,
              priority: n.priority,
              previousDueAt: nullable(n.previous_due_at),
              dueAt: nullable(n.due_at),
              from: n.previous_state,
              to: n.next_state,
              cycle: n.cycle,
              previousResolution: n.previous_resolution,
              resolution: n.resolution,
              reason: n.reason,
              occurredAt: instant(n.occurred_at),
            }),
          ) ?? null,
        transitionsHasMore: (transitions?.length ?? 0) > 100,
        audit:
          audit?.slice(0, 100).map((n) =>
            supportAuditSchema.parse({
              requestId: n.request_id,
              actorId: n.actor_id,
              operation: n.operation,
              recordId: n.record_id,
              revision: n.revision,
              reason: n.reason,
              occurredAt: instant(n.occurred_at),
            }),
          ) ?? null,
        auditHasMore: (audit?.length ?? 0) > 100,
      }
    })
  }
  private fingerprint(tenant: string, principal: SupportPrincipal, input: SupportMutation) {
    return createHmac('sha256', this.key)
      .update(canonical({ domain: 'support', tenant, principal, command: input.command }))
      .digest('hex')
  }
  private async recovery(
    client: PoolClient,
    tenant: string,
    principal: SupportPrincipal,
    a: SupportAccess,
    r: Row,
  ) {
    if (r.actor_id !== principal.id || !r.receipt) throw new SupportError('conflict')
    const receipt = supportReceiptSchema.parse(r.receipt)
    grant(a, receipt.operation === 'note.add' ? 'notes.write' : 'cases.write')
    if (receipt.operation === 'case.assign') {
      grant(a, 'cases.assign')
      if (a.scope !== 'all') throw new SupportError('forbidden')
    }
    await this.ownedCase(client, tenant, a, receipt.id)
    return supportReceiptSchema.parse({ ...receipt, replayed: true })
  }
  commandStatus(
    tenant: string,
    principal: SupportPrincipal,
    id: string,
  ): Promise<SupportCommandStatus> {
    z.uuid().parse(id)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'cases.read')
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_support.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, id],
        )
      ).rows[0]
      return row
        ? { status: 'committed', receipt: await this.recovery(client, tenant, principal, a, row) }
        : { status: 'unknown' }
    })
  }
  mutate(tenant: string, principal: SupportPrincipal, raw: SupportMutation) {
    const input = supportMutationSchema.parse(raw)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.transaction(tenant, principal, async (client, a) => {
      const c = input.command
      authorize(a, c)
      const fingerprint = this.fingerprint(tenant, principal, input),
        claim = await client.query(
          'INSERT INTO archava_support.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING',
          [tenant, input.requestId, principal.id, fingerprint],
        )
      if (claim.rowCount === 0) {
        const r = (
          await client.query<Row>(
            'SELECT * FROM archava_support.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, input.requestId],
          )
        ).rows[0]
        if (!r || r.fingerprint !== fingerprint) throw new SupportError('conflict')
        return this.recovery(client, tenant, principal, a, r)
      }
      const changed = await this.execute(client, tenant, a, input),
        receipt = supportReceiptSchema.parse({
          requestId: input.requestId,
          operation: c.operation,
          id: changed.id,
          revision: changed.revision,
          replayed: false,
        })
      await client.query(
        'INSERT INTO archava_support.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [
          tenant,
          input.requestId,
          principal.id,
          c.operation,
          receipt.id,
          receipt.revision,
          'reason' in c ? c.reason : null,
        ],
      )
      await client.query(
        'INSERT INTO archava_support.transitions(tenant_id,request_id,case_id,actor_id,previous_state,next_state,cycle,case_revision,previous_resolution,resolution,reason,operation,previous_assignee_id,assignee_id,previous_priority,priority,previous_due_at,due_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)',
        [
          tenant,
          input.requestId,
          receipt.id,
          principal.id,
          changed.previous?.state ?? null,
          changed.current.state,
          changed.current.cycle,
          receipt.revision,
          changed.previous?.resolution ?? null,
          changed.current.resolution,
          'reason' in c ? c.reason : null,
          c.operation,
          changed.previous?.assignee_id ?? null,
          changed.current.assignee_id,
          changed.previous?.priority ?? null,
          changed.current.priority,
          changed.previous?.due_at ?? null,
          changed.current.due_at,
        ],
      )
      await client.query(
        'UPDATE archava_support.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, input.requestId, receipt],
      )
      return receipt
    })
  }
  private async execute(
    client: PoolClient,
    tenant: string,
    a: SupportAccess,
    input: SupportMutation,
  ): Promise<{ id: string; revision: number; previous: Row | null; current: Row }> {
    const c = input.command
    if (c.operation === 'case.create') {
      const id = randomUUID(),
        current = (
          await client.query<Row>(
            'INSERT INTO archava_support.cases(tenant_id,id,owner_id,requester_name,subject,description,priority,due_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *',
            [tenant, id, a.ownerId, c.requesterName, c.subject, c.description, c.priority, c.dueAt],
          )
        ).rows[0]
      if (!current) throw new SupportError('unavailable')
      return { id, revision: 1, previous: null, current }
    }
    const previous = await this.ownedCase(client, tenant, a, c.id, true)
    if (previous.revision !== c.expectedRevision) throw new SupportError('conflict')
    let state = String(previous.state),
      cycle = Number(previous.cycle),
      resolution = previous.resolution,
      due = previous.due_at,
      assignee = previous.assignee_id,
      priority = previous.priority,
      resolve = false,
      close = false,
      reopen = false
    if (c.operation === 'note.add')
      await client.query(
        'INSERT INTO archava_support.notes(tenant_id,id,case_id,author_id,body,request_id) VALUES($1,$2,$3,$4,$5,$6)',
        [tenant, randomUUID(), c.id, a.ownerId, c.body, input.requestId],
      )
    else if (
      c.operation === 'case.assign' ||
      c.operation === 'case.set-priority' ||
      c.operation === 'case.set-target'
    ) {
      if (state === 'closed' || state === 'cancelled') throw new SupportError('conflict')
      if (c.operation === 'case.assign') {
        assignee = c.assigneeId
        if (c.assigneeId) {
          const m = (
            await client.query<Row>('SELECT * FROM archava_support.lock_membership($1,$2)', [
              tenant,
              c.assigneeId,
            ])
          ).rows[0]
          if (
            !m ||
            m.kind !== 'human' ||
            !m.active ||
            !Array.isArray(m.permissions) ||
            !m.permissions.includes('cases.read')
          )
            throw new SupportError('forbidden')
        }
      }
      if (c.operation === 'case.set-priority') priority = c.priority
      if (c.operation === 'case.set-target') due = c.dueAt
    } else if (c.operation === 'case.resolve') {
      if (state !== 'in_progress' && state !== 'waiting') throw new SupportError('conflict')
      state = 'resolved'
      resolution = c.resolution
      resolve = true
    } else if (c.operation === 'case.reopen') {
      if (state !== 'resolved' && state !== 'closed') throw new SupportError('conflict')
      state = 'open'
      cycle++
      resolution = null
      due = c.dueAt
      reopen = true
    } else {
      const next: Partial<Record<SupportCommand['operation'], readonly [string, string]>> = {
        'case.start': ['open', 'in_progress'],
        'case.wait': ['in_progress', 'waiting'],
        'case.resume': ['waiting', 'in_progress'],
        'case.close': ['resolved', 'closed'],
      }
      if (c.operation === 'case.cancel') {
        if (!['open', 'in_progress', 'waiting'].includes(state)) throw new SupportError('conflict')
        state = 'cancelled'
      } else {
        const transition = next[c.operation]
        if (!transition || state !== transition[0]) throw new SupportError('conflict')
        state = transition[1]
        close = c.operation === 'case.close'
      }
    }
    const current = (
      await client.query<Row>(
        'UPDATE archava_support.cases SET state=$3,cycle=$4,resolution=$5,due_at=$6,assignee_id=$7,priority=$8,resolved_at=CASE WHEN $9::boolean THEN clock_timestamp() WHEN $11::boolean THEN NULL ELSE resolved_at END,closed_at=CASE WHEN $10::boolean THEN clock_timestamp() WHEN $11::boolean THEN NULL ELSE closed_at END,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2 RETURNING *',
        [tenant, c.id, state, cycle, resolution, due, assignee, priority, resolve, close, reopen],
      )
    ).rows[0]
    if (!current) throw new SupportError('unavailable')
    return { id: c.id, revision: c.expectedRevision + 1, previous, current }
  }
}
