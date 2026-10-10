import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  ProjectError,
  projectTenantSchema,
  projectPrincipalSchema,
  projectMembershipSchema,
  projectSchema,
  projectTaskSchema,
  projectMilestoneSchema,
  projectAuditSchema,
  projectMutationSchema,
  projectReceiptSchema,
  projectPageSchema,
  PROJECT_TRANSITIONS,
  TASK_TRANSITIONS,
  type ProjectRepository,
  type ProjectPrincipal,
  type ProjectPermission,
  type ProjectAccess,
  type ProjectPageQuery,
  type ProjectMutation,
  type ProjectReceipt,
  type Project,
  type ProjectDetail,
  type ProjectCommandStatus,
} from '@archava/projects'
import { projectMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>
const instant = (value: unknown) => {
  if (!(value instanceof Date) || !Number.isFinite(value.getTime()))
    throw new Error('Invalid project timestamp')
  return value.toISOString()
}
const nullableInstant = (value: unknown) => (value === null ? null : instant(value))
const project = (row: Row) =>
  projectSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    ownerId: row.owner_id,
    title: row.title,
    scopeSummary: row.scope_summary,
    scopeVersion: row.scope_version,
    scopeConfirmedAt: nullableInstant(row.scope_confirmed_at),
    scopeConfirmedBy: row.scope_confirmed_by,
    state: row.state,
    revision: row.revision,
    dueAt: nullableInstant(row.due_at),
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const task = (row: Row) =>
  projectTaskSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    projectId: row.project_id,
    milestoneId: row.milestone_id,
    title: row.title,
    required: row.required,
    state: row.state,
    revision: row.revision,
    dueAt: nullableInstant(row.due_at),
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const milestone = (row: Row) =>
  projectMilestoneSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    projectId: row.project_id,
    title: row.title,
    required: row.required,
    state: row.state,
    revision: row.revision,
    dueAt: nullableInstant(row.due_at),
    reachedAt: nullableInstant(row.reached_at),
    evidence: row.evidence,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const audit = (row: Row) =>
  projectAuditSchema.parse({
    requestId: row.request_id,
    actorId: row.actor_id,
    operation: row.operation,
    projectId: row.project_id,
    resourceId: row.resource_id,
    revision: row.revision,
    projectRevision: row.project_revision,
    reason: row.reason,
    occurredAt: instant(row.occurred_at),
  })
function canonical(value: unknown): string {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']'
  if (typeof value === 'object' && value !== null)
    return (
      '{' +
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, item]) => JSON.stringify(key) + ':' + canonical(item))
        .join(',') +
      '}'
    )
  return JSON.stringify(value)
}
function requireGrant(access: ProjectAccess, permission: ProjectPermission): void {
  if (!access.permissions.includes(permission)) throw new ProjectError('forbidden')
}

/** Own domain schema. This adapter never reads or writes CRM, signatures, invoices or payments. */
export class PostgresProjectRepository implements ProjectRepository {
  private readonly pool: Pool
  private readonly fingerprintKey: string
  private constructor(pool: Pool, fingerprintKey: string) {
    this.pool = pool
    this.fingerprintKey = fingerprintKey
  }
  static async create(pool: Pool, fingerprintKey: string) {
    if (fingerprintKey.length < 32)
      throw new Error('Stable project fingerprint key must have at least 32 characters')
    const role = await pool.query<{ unsafe: boolean }>(
      `SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_projects'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user`,
    )
    if (role.rows[0]?.unsafe !== false)
      throw new Error('Projects require a non-owner runtime without superuser/BYPASSRLS')
    const existing = await pool.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_projects.schema_migrations ORDER BY id',
    )
    const expected = await projectMigrationLedger()
    if (
      existing.rows.length !== expected.length ||
      existing.rows.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported project schema version/checksum')
    const policies = await pool.query<{ count: number }>(
      "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_projects'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
      [['memberships', 'projects', 'tasks', 'milestones', 'commands', 'audit']],
    )
    if (policies.rows[0]?.count !== 6)
      throw new Error('Project tenant row policies must be enabled/forced')
    return new PostgresProjectRepository(pool, fingerprintKey)
  }
  private async transaction<T>(
    tenant: string,
    principal: ProjectPrincipal,
    permission: ProjectPermission | null,
    work: (client: PoolClient, access: ProjectAccess) => Promise<T>,
  ): Promise<T> {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string) => {
        const row = (
          await client.query<Row>('SELECT * FROM archava_projects.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!row) throw new ProjectError('forbidden')
        const result = projectMembershipSchema.parse({
          principalId: row.principal_id,
          kind: row.kind,
          permissions: row.permissions,
          scope: row.scope,
          active: row.active,
        })
        if (!result.active) throw new ProjectError('forbidden')
        return result
      }
      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new ProjectError('forbidden')
      let access: ProjectAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new ProjectError('forbidden')
        access = {
          permissions: actor.permissions.filter((x) => human.permissions.includes(x)),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }
      if (permission) requireGrant(access, permission)
      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedProject(
    client: PoolClient,
    tenant: string,
    access: ProjectAccess,
    id: string,
    lock: 'share' | 'update' = 'share',
  ): Promise<Project> {
    const row = (
      await client.query<Row>(
        `SELECT * FROM archava_projects.projects WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR ${lock === 'update' ? 'UPDATE' : 'SHARE'}`,
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new ProjectError('not_found')
    return project(row)
  }
  access(tenant: string, principal: ProjectPrincipal): Promise<ProjectAccess> {
    return this.transaction(tenant, principal, null, (_client, access) => Promise.resolve(access))
  }
  list(tenant: string, principal: ProjectPrincipal, raw: ProjectPageQuery) {
    const query = projectPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'projects.read', async (client, access) => {
      const search = query.search.replace(/[\\%_]/g, '\\$&')
      const rows = (
        await client.query<Row>(
          `SELECT * FROM archava_projects.projects WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) AND ($5::text IS NULL OR state=$5) AND title ILIKE $6 ESCAPE '\\' ORDER BY id LIMIT $7`,
          [
            tenant,
            access.scope === 'all',
            access.ownerId,
            query.after ?? null,
            query.state ?? null,
            '%' + search + '%',
            query.limit + 1,
          ],
        )
      ).rows
      const items = rows.slice(0, query.limit).map(project)
      return { items, next: rows.length > query.limit ? (items.at(-1)?.id ?? null) : null }
    })
  }
  detail(tenant: string, principal: ProjectPrincipal, rawId: string): Promise<ProjectDetail> {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'projects.read', async (client, access) => {
      // All child writers take the parent update lock. These reads observe one consistent project revision.
      const record = await this.ownedProject(client, tenant, access, id)
      const tasks = access.permissions.includes('tasks.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 ORDER BY created_at,id LIMIT 100',
              [tenant, id],
            )
          ).rows.map(task)
        : null
      const milestones = access.permissions.includes('milestones.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_projects.milestones WHERE tenant_id=$1 AND project_id=$2 ORDER BY created_at,id LIMIT 100',
              [tenant, id],
            )
          ).rows.map(milestone)
        : null
      const history = access.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_projects.audit WHERE tenant_id=$1 AND project_id=$2 ORDER BY project_revision DESC LIMIT 51',
              [tenant, id],
            )
          ).rows
        : null
      return {
        project: record,
        tasks,
        milestones,
        audit: history?.slice(0, 50).map(audit) ?? null,
        auditHasMore: (history?.length ?? 0) > 50,
      }
    })
  }
  private async receiptAccess(
    client: PoolClient,
    tenant: string,
    access: ProjectAccess,
    receipt: ProjectReceipt,
  ) {
    requireGrant(access, 'projects.read')
    if (receipt.kind === 'task') requireGrant(access, 'tasks.read')
    if (receipt.kind === 'milestone') requireGrant(access, 'milestones.read')
    await this.ownedProject(client, tenant, access, receipt.projectId)
  }
  commandStatus(
    tenant: string,
    principal: ProjectPrincipal,
    rawId: string,
  ): Promise<ProjectCommandStatus> {
    if (principal.kind !== 'human') throw new ProjectError('forbidden')
    const requestId = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'projects.read', async (client, access) => {
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_projects.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, requestId],
        )
      ).rows[0]
      if (!row) return { status: 'not_committed', requestId }
      if (row.actor_id !== principal.id) throw new ProjectError('forbidden')
      const receipt = projectReceiptSchema.parse(row.receipt)
      await this.receiptAccess(client, tenant, access, receipt)
      return { status: 'committed', requestId, receipt }
    })
  }
  mutate(
    tenant: string,
    principal: ProjectPrincipal,
    raw: ProjectMutation,
  ): Promise<ProjectReceipt> {
    if (principal.kind !== 'human')
      throw new ProjectError('forbidden', 'Project machine writes are not registered.')
    const mutation = projectMutationSchema.parse(raw),
      command = mutation.command
    const permission: ProjectPermission = command.operation.startsWith('project.')
      ? 'projects.write'
      : command.operation.startsWith('task.')
        ? 'tasks.write'
        : 'milestones.write'
    const fingerprint = createHmac('sha256', this.fingerprintKey)
      .update(canonical({ tenant, principal, command }))
      .digest('hex')
    return this.transaction(tenant, principal, permission, async (client, access) => {
      requireGrant(access, 'projects.read')
      const claimed = await client.query(
        'INSERT INTO archava_projects.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id',
        [tenant, mutation.requestId, principal.id, fingerprint],
      )
      if (!claimed.rows.length) {
        const existing = (
          await client.query<Row>(
            'SELECT * FROM archava_projects.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, mutation.requestId],
          )
        ).rows[0]
        if (!existing || existing.actor_id !== principal.id || existing.fingerprint !== fingerprint)
          throw new ProjectError('conflict')
        const receipt = projectReceiptSchema.parse(existing.receipt)
        await this.receiptAccess(client, tenant, access, receipt)
        return receipt
      }
      let receipt: ProjectReceipt
      if (command.operation === 'project.create') {
        const id = randomUUID()
        await client.query(
          'INSERT INTO archava_projects.projects(tenant_id,id,owner_id,title,scope_summary,due_at) VALUES($1,$2,$3,$4,$5,$6)',
          [tenant, id, access.ownerId, command.title, command.scopeSummary, command.dueAt],
        )
        receipt = { kind: 'project', id, projectId: id, revision: 1, projectRevision: 1 }
      } else {
        const record = await this.ownedProject(client, tenant, access, command.projectId, 'update')
        if (record.revision !== command.projectRevision || record.revision >= 2_147_483_647)
          throw new ProjectError('conflict')
        if (command.operation === 'project.transition') {
          if (!PROJECT_TRANSITIONS[record.state].includes(command.state))
            throw new ProjectError('conflict', 'Project transition refused.')
          if (command.state === 'delivered') {
            requireGrant(access, 'tasks.read')
            requireGrant(access, 'milestones.read')
            const counts = (
              await client.query<{ required: number; pending: number; milestones: number }>(
                `SELECT (SELECT count(*)::integer FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 AND required) AS required,
              (SELECT count(*)::integer FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 AND required AND state<>'done') AS pending,
              (SELECT count(*)::integer FROM archava_projects.milestones WHERE tenant_id=$1 AND project_id=$2 AND required AND state<>'reached') AS milestones`,
                [tenant, record.id],
              )
            ).rows[0]
            if (!counts || counts.required === 0 || counts.pending > 0 || counts.milestones > 0)
              throw new ProjectError('conflict', 'Required delivery work is incomplete.')
          }
          await client.query(
            `UPDATE archava_projects.projects SET state=$3,scope_confirmed_at=CASE WHEN $3='active' THEN COALESCE(scope_confirmed_at,now()) ELSE scope_confirmed_at END,
            scope_confirmed_by=CASE WHEN $3='active' THEN COALESCE(scope_confirmed_by,$4) ELSE scope_confirmed_by END WHERE tenant_id=$1 AND id=$2`,
            [tenant, record.id, command.state, principal.id],
          )
          receipt = {
            kind: 'project',
            id: record.id,
            projectId: record.id,
            revision: record.revision + 1,
            projectRevision: record.revision + 1,
          }
        } else {
          if (['delivered', 'closed', 'cancelled'].includes(record.state))
            throw new ProjectError('conflict', 'Project is no longer writable.')
          if (command.operation === 'task.create' || command.operation === 'milestone.create') {
            const table = command.operation === 'task.create' ? 'tasks' : 'milestones'
            const count =
              (
                await client.query<{ count: number }>(
                  `SELECT count(*)::integer AS count FROM archava_projects.${table} WHERE tenant_id=$1 AND project_id=$2`,
                  [tenant, record.id],
                )
              ).rows[0]?.count ?? 0
            if (count >= 100)
              throw new ProjectError('conflict', 'Project exceeds the supported work item limit.')
            const id = randomUUID()
            if (command.operation === 'task.create') {
              if (command.milestoneId) {
                requireGrant(access, 'milestones.read')
                const parent = (
                  await client.query<Row>(
                    'SELECT * FROM archava_projects.milestones WHERE tenant_id=$1 AND project_id=$2 AND id=$3',
                    [tenant, record.id, command.milestoneId],
                  )
                ).rows[0]
                if (!parent) throw new ProjectError('not_found')
                if (parent.state !== 'open')
                  throw new ProjectError('conflict', 'Reached milestones cannot gain work.')
              }
              await client.query(
                'INSERT INTO archava_projects.tasks(tenant_id,id,project_id,milestone_id,title,required,due_at) VALUES($1,$2,$3,$4,$5,$6,$7)',
                [
                  tenant,
                  id,
                  record.id,
                  command.milestoneId,
                  command.title,
                  command.required,
                  command.dueAt,
                ],
              )
            } else
              await client.query(
                'INSERT INTO archava_projects.milestones(tenant_id,id,project_id,title,required,due_at) VALUES($1,$2,$3,$4,$5,$6)',
                [tenant, id, record.id, command.title, command.required, command.dueAt],
              )
            receipt = {
              kind: table === 'tasks' ? 'task' : 'milestone',
              id,
              projectId: record.id,
              revision: 1,
              projectRevision: record.revision + 1,
            }
          } else if (command.operation === 'task.transition') {
            requireGrant(access, 'tasks.read')
            const row = (
              await client.query<Row>(
                'SELECT * FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE',
                [tenant, record.id, command.id],
              )
            ).rows[0]
            if (!row) throw new ProjectError('not_found')
            const item = task(row)
            if (
              item.revision !== command.expectedRevision ||
              item.revision >= 2_147_483_647 ||
              !TASK_TRANSITIONS[item.state].includes(command.state)
            )
              throw new ProjectError('conflict')
            if (item.milestoneId && command.state !== 'done') {
              const parent = (
                await client.query<Row>(
                  'SELECT state FROM archava_projects.milestones WHERE tenant_id=$1 AND project_id=$2 AND id=$3',
                  [tenant, record.id, item.milestoneId],
                )
              ).rows[0]
              if (parent?.state === 'reached')
                throw new ProjectError('conflict', 'Reached milestone work cannot reopen.')
            }
            await client.query(
              'UPDATE archava_projects.tasks SET state=$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3',
              [tenant, record.id, item.id, command.state],
            )
            receipt = {
              kind: 'task',
              id: item.id,
              projectId: record.id,
              revision: item.revision + 1,
              projectRevision: record.revision + 1,
            }
          } else {
            requireGrant(access, 'milestones.read')
            requireGrant(access, 'tasks.read')
            const row = (
              await client.query<Row>(
                'SELECT * FROM archava_projects.milestones WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE',
                [tenant, record.id, command.id],
              )
            ).rows[0]
            if (!row) throw new ProjectError('not_found')
            const item = milestone(row)
            if (
              item.revision !== command.expectedRevision ||
              item.revision >= 2_147_483_647 ||
              item.state !== 'open'
            )
              throw new ProjectError('conflict')
            const count = (
              await client.query<{ total: number; pending: number }>(
                `SELECT count(*)::integer AS total,count(*) FILTER(WHERE required AND state<>'done')::integer AS pending FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 AND milestone_id=$3`,
                [tenant, record.id, item.id],
              )
            ).rows[0]
            if (!count || count.total === 0 || count.pending > 0)
              throw new ProjectError('conflict', 'Milestone work is incomplete.')
            await client.query(
              "UPDATE archava_projects.milestones SET state='reached',revision=revision+1,reached_at=now(),evidence=$4,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
              [tenant, record.id, item.id, command.evidence],
            )
            receipt = {
              kind: 'milestone',
              id: item.id,
              projectId: record.id,
              revision: item.revision + 1,
              projectRevision: record.revision + 1,
            }
          }
        }
        await client.query(
          'UPDATE archava_projects.projects SET revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND id=$2',
          [tenant, record.id],
        )
      }
      const reason =
        'reason' in command
          ? (command.reason ?? null)
          : command.operation === 'milestone.complete'
            ? command.evidence
            : null
      await client.query(
        'INSERT INTO archava_projects.audit(tenant_id,request_id,actor_id,operation,project_id,resource_id,revision,project_revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)',
        [
          tenant,
          mutation.requestId,
          principal.id,
          command.operation,
          receipt.projectId,
          receipt.id,
          receipt.revision,
          receipt.projectRevision,
          reason,
        ],
      )
      await client.query(
        'UPDATE archava_projects.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, mutation.requestId, receipt],
      )
      return receipt
    })
  }
}
