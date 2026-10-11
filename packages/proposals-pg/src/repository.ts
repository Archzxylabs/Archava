import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  ProposalError,
  proposalTenantSchema,
  proposalPrincipalSchema,
  proposalMembershipSchema,
  proposalSchema,
  proposalScopeItemSchema,
  proposalScopeVersionSchema,
  proposalChangeRequestSchema,
  commandReceiptSchema,
  type ProposalRepository,
  type ProposalPrincipal,
  type ProposalAccess,
  type ProposalPermission,
  type ProposalMutation,
  type CommandReceipt,
  type CommandStatusResult,
  type ProposalPageQuery,
  type Proposal,
  type ProposalDetail,
  type ProposalScopeItem,
  type ProposalScopeVersion,
  type ProposalChangeRequest,
} from '@archava/proposals'
import { proposalMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid proposal timestamp')
  return x.toISOString()
}

const nullable = (x: unknown) => (x === null || x === undefined ? null : String(x))

const mapProposal = (r: Row): Proposal =>
  proposalSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    title: r.title,
    summary: r.summary,
    organizationId: r.organization_id ? String(r.organization_id) : null,
    contactId: nullable(r.contact_id),
    leadId: nullable(r.lead_id),
    currency: r.currency,
    totalEstimatedValue: Number(r.total_estimated_value),
    state: r.state,
    currentScopeVersion: Number(r.current_scope_version),
    acceptedAt: r.accepted_at ? instant(r.accepted_at) : null,
    acceptedBy: nullable(r.accepted_by),
    linkedProjectId: r.linked_project_id ? String(r.linked_project_id) : null,
    ownerId: r.owner_id,
    revision: Number(r.revision),
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapScopeItem = (r: Row): ProposalScopeItem =>
  proposalScopeItemSchema.parse({
    id: r.id,
    proposalId: r.proposal_id,
    scopeVersion: Number(r.scope_version),
    title: r.title,
    description: r.description,
    estimatedDays: r.estimated_days !== null && r.estimated_days !== undefined ? Number(r.estimated_days) : null,
    estimatedValue: Number(r.estimated_value),
    deliverableType: r.deliverable_type,
    createdAt: instant(r.created_at),
  })

const mapScopeVersion = (r: Row): ProposalScopeVersion =>
  proposalScopeVersionSchema.parse({
    id: r.id,
    proposalId: r.proposal_id,
    scopeVersion: Number(r.scope_version),
    changeRequestId: r.change_request_id ? String(r.change_request_id) : null,
    summary: r.summary,
    totalValue: Number(r.total_value),
    totalDays: Number(r.total_days),
    confirmedAt: instant(r.confirmed_at),
    confirmedBy: r.confirmed_by,
    items: typeof r.items === 'string' ? JSON.parse(r.items) : r.items,
  })

const mapChangeRequest = (r: Row): ProposalChangeRequest =>
  proposalChangeRequestSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    proposalId: r.proposal_id,
    changeNumber: Number(r.change_number),
    title: r.title,
    reason: r.reason,
    deltaSummary: r.delta_summary,
    timeDeltaDays: Number(r.time_delta_days),
    costDeltaValue: Number(r.cost_delta_value),
    resultingScopeVersion: r.resulting_scope_version ? Number(r.resulting_scope_version) : null,
    state: r.state,
    submittedAt: r.submitted_at ? instant(r.submitted_at) : null,
    decidedAt: r.decided_at ? instant(r.decided_at) : null,
    decidedBy: nullable(r.decided_by),
    ownerId: r.owner_id,
    revision: Number(r.revision),
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const grant = (a: ProposalAccess, p: ProposalPermission) => {
  if (!a.permissions.includes(p)) throw new ProposalError('forbidden')
}

export class PostgresProposalRepository implements ProposalRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}

  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable proposal fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_proposals'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Proposals requires a non-owner runtime')

    const expected = await proposalMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_proposals.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Unsupported proposals ledger')

    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_proposals'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'proposals', 'scope_items', 'scope_versions', 'change_requests', 'commands', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 7) throw new Error('Proposals tenant policies must be forced')

    return new PostgresProposalRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: ProposalPrincipal,
    work: (client: PoolClient, access: ProposalAccess) => Promise<T>,
  ): Promise<T> {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])

      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_proposals.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new ProposalError('forbidden')
        const m = proposalMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new ProposalError('forbidden')
        return m
      }

      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new ProposalError('forbidden')

      let access: ProposalAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new ProposalError('forbidden')
        access = {
          permissions: actor.permissions.filter((x: ProposalPermission) =>
            human.permissions.includes(x),
          ),
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
        throw new ProposalError('conflict')
      throw error
    } finally {
      client.release()
    }
  }

  async access(tenant: string, principal: ProposalPrincipal): Promise<ProposalAccess> {
    return await this.transaction(tenant, principal, async (_c, a) => a)
  }

  async list(
    tenant: string,
    principal: ProposalPrincipal,
    query: ProposalPageQuery,
  ): Promise<{ items: readonly Proposal[]; nextCursor?: string }> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'proposals.read')
      const params: unknown[] = [tenant, query.limit + 1]
      let sql = 'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1'
      let pIdx = 3

      if (access.scope === 'assigned') {
        sql += ` AND owner_id=$${pIdx++}`
        params.push(access.ownerId)
      }
      if (query.state) {
        sql += ` AND state=$${pIdx++}`
        params.push(query.state)
      }
      if (query.organizationId) {
        sql += ` AND organization_id=$${pIdx++}`
        params.push(query.organizationId)
      }
      if (query.search) {
        sql += ` AND title ILIKE $${pIdx++}`
        params.push(`%${query.search}%`)
      }
      if (query.cursor) {
        sql += ` AND id > $${pIdx++}`
        params.push(query.cursor)
      }
      sql += ' ORDER BY id ASC LIMIT $2'

      const rows = (await client.query<Row>(sql, params)).rows
      const hasMore = rows.length > query.limit
      const sliced = hasMore ? rows.slice(0, query.limit) : rows
      const items = sliced.map(mapProposal)
      const nextCursor = hasMore && sliced.length > 0 ? (sliced[sliced.length - 1]?.id as string) : undefined

      return { items, nextCursor }
    })
  }

  async detail(
    tenant: string,
    principal: ProposalPrincipal,
    proposalId: string,
  ): Promise<ProposalDetail> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'proposals.read')
      const propRow = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2',
          [tenant, proposalId],
        )
      ).rows[0]
      if (!propRow) throw new ProposalError('not_found')
      const proposal = mapProposal(propRow)
      if (access.scope === 'assigned' && proposal.ownerId !== access.ownerId)
        throw new ProposalError('forbidden')

      const itemsRows = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=$3 ORDER BY created_at ASC',
          [tenant, proposalId, proposal.currentScopeVersion],
        )
      ).rows
      const currentItems = itemsRows.map(mapScopeItem)

      const versionsRows = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.scope_versions WHERE tenant_id=$1 AND proposal_id=$2 ORDER BY scope_version ASC',
          [tenant, proposalId],
        )
      ).rows
      const versions = versionsRows.map(mapScopeVersion)

      const crRows = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND proposal_id=$2 ORDER BY change_number ASC',
          [tenant, proposalId],
        )
      ).rows
      const changeRequests = crRows.map(mapChangeRequest)

      return { proposal, currentItems, versions, changeRequests }
    })
  }

  async listVersions(
    tenant: string,
    principal: ProposalPrincipal,
    proposalId: string,
  ): Promise<readonly ProposalScopeVersion[]> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'proposals.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.scope_versions WHERE tenant_id=$1 AND proposal_id=$2 ORDER BY scope_version ASC',
          [tenant, proposalId],
        )
      ).rows
      return rows.map(mapScopeVersion)
    })
  }

  async listChangeRequests(
    tenant: string,
    principal: ProposalPrincipal,
    proposalId: string,
  ): Promise<readonly ProposalChangeRequest[]> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'changerequests.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND proposal_id=$2 ORDER BY change_number ASC',
          [tenant, proposalId],
        )
      ).rows
      return rows.map(mapChangeRequest)
    })
  }

  async mutate(
    tenant: string,
    principal: ProposalPrincipal,
    mutation: ProposalMutation,
  ): Promise<CommandReceipt> {
    return await this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = createHmac('sha256', this.key)
        .update(JSON.stringify({ tenant, principal: principal.id, mutation }))
        .digest('hex')

      const existingCmd = (
        await client.query<Row>(
          'SELECT * FROM archava_proposals.commands WHERE tenant_id=$1 AND command_id=$2',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) throw new ProposalError('conflict')
        return commandReceiptSchema.parse(existingCmd.receipt)
      }

      await client.query(
        'INSERT INTO archava_proposals.commands(tenant_id, command_id, actor_id, fingerprint) VALUES($1,$2,$3,$4)',
        [tenant, mutation.commandId, access.ownerId, fingerprint],
      )

      let receipt: CommandReceipt

      if (mutation.type === 'create_proposal') {
        grant(access, 'proposals.write')
        const proposalId = randomUUID()
        const totalVal = mutation.scopeItems.reduce((acc: number, it) => acc + (it.estimatedValue ?? 0), 0)

        const propRow = (
          await client.query<Row>(
            `INSERT INTO archava_proposals.proposals(
              tenant_id, id, title, summary, organization_id, contact_id, lead_id,
              currency, total_estimated_value, state, current_scope_version, owner_id, revision
            ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'draft',1,$10,1)
            RETURNING *`,
            [
              tenant,
              proposalId,
              mutation.title,
              mutation.summary,
              mutation.organizationId,
              mutation.contactId,
              mutation.leadId,
              mutation.currency,
              totalVal,
              access.ownerId,
            ],
          )
        ).rows[0]!

        for (const it of mutation.scopeItems) {
          await client.query(
            `INSERT INTO archava_proposals.scope_items(
              tenant_id, id, proposal_id, scope_version, title, description, estimated_days, estimated_value, deliverable_type
            ) VALUES($1,$2,$3,1,$4,$5,$6,$7,$8)`,
            [
              tenant,
              randomUUID(),
              proposalId,
              it.title,
              it.description,
              it.estimatedDays,
              it.estimatedValue,
              it.deliverableType,
            ],
          )
        }

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(propRow.created_at),
          proposalId,
          scopeVersion: 1,
          revision: 1,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'create_proposal',$4,1,$5)`,
          [tenant, mutation.commandId, access.ownerId, proposalId, propRow.created_at],
        )
      } else if (mutation.type === 'update_proposal') {
        grant(access, 'proposals.write')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'draft') throw new ProposalError('scope_frozen')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET title=$1, summary=$2, organization_id=COALESCE($3, organization_id), contact_id=COALESCE($4, contact_id), lead_id=COALESCE($5, lead_id), revision=$6, updated_at=clock_timestamp()
             WHERE tenant_id=$7 AND id=$8
             RETURNING *`,
            [
              mutation.title,
              mutation.summary,
              mutation.organizationId ?? null,
              mutation.contactId ?? null,
              mutation.leadId ?? null,
              nextRev,
              tenant,
              mutation.proposalId,
            ],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'update_proposal',$4,$5,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'set_scope_items') {
        grant(access, 'proposals.write')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'draft') throw new ProposalError('scope_frozen')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        await client.query(
          'DELETE FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=1',
          [tenant, mutation.proposalId],
        )

        const totalVal = mutation.scopeItems.reduce((acc: number, it) => acc + (it.estimatedValue ?? 0), 0)
        for (const it of mutation.scopeItems) {
          await client.query(
            `INSERT INTO archava_proposals.scope_items(
              tenant_id, id, proposal_id, scope_version, title, description, estimated_days, estimated_value, deliverable_type
            ) VALUES($1,$2,$3,1,$4,$5,$6,$7,$8)`,
            [
              tenant,
              randomUUID(),
              mutation.proposalId,
              it.title,
              it.description,
              it.estimatedDays,
              it.estimatedValue,
              it.deliverableType,
            ],
          )
        }

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET total_estimated_value=$1, revision=$2, updated_at=clock_timestamp()
             WHERE tenant_id=$3 AND id=$4
             RETURNING *`,
            [totalVal, nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          scopeVersion: 1,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'set_scope_items',$4,$5,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'submit_proposal') {
        grant(access, 'proposals.write')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'draft') throw new ProposalError('invalid_state')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET state='submitted', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'submit_proposal',$4,$5,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'accept_proposal') {
        grant(access, 'proposals.decide')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'submitted') throw new ProposalError('invalid_state')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        // Fetch v1 items to freeze snapshot into scope_versions table!
        const items = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=1 ORDER BY created_at ASC',
            [tenant, mutation.proposalId],
          )
        ).rows.map(mapScopeItem)

        const totalVal = items.reduce((acc: number, it: ProposalScopeItem) => acc + (it.estimatedValue ?? 0), 0)
        const totalDays = items.reduce((acc: number, it: ProposalScopeItem) => acc + (it.estimatedDays ?? 0), 0)

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET state='accepted', accepted_at=clock_timestamp(), accepted_by=$1, linked_project_id=$2, revision=$3, updated_at=clock_timestamp()
             WHERE tenant_id=$4 AND id=$5
             RETURNING *`,
            [access.ownerId, mutation.linkedProjectId, nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        await client.query(
          `INSERT INTO archava_proposals.scope_versions(
            tenant_id, id, proposal_id, scope_version, change_request_id, summary, total_value, total_days, confirmed_at, confirmed_by, items
          ) VALUES($1,$2,$3,1,NULL,$4,$5,$6,$7,$8,$9)`,
          [
            tenant,
            randomUUID(),
            mutation.proposalId,
            updated.summary,
            totalVal,
            totalDays,
            updated.accepted_at,
            access.ownerId,
            JSON.stringify(items),
          ],
        )

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          scopeVersion: 1,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'accept_proposal',$4,$5,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'reject_proposal') {
        grant(access, 'proposals.decide')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'submitted') throw new ProposalError('invalid_state')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET state='rejected', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, reason, occurred_at)
           VALUES($1,$2,$3,'reject_proposal',$4,$5,$6,$7)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, mutation.reason, updated.updated_at],
        )
      } else if (mutation.type === 'withdraw_proposal') {
        grant(access, 'proposals.write')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (!['draft', 'submitted'].includes(row.state as string)) throw new ProposalError('invalid_state')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET state='withdrawn', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, reason, occurred_at)
           VALUES($1,$2,$3,'withdraw_proposal',$4,$5,$6,$7)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, mutation.reason, updated.updated_at],
        )
      } else if (mutation.type === 'link_project') {
        grant(access, 'proposals.write')
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!row) throw new ProposalError('not_found')
        if (row.state !== 'accepted') throw new ProposalError('invalid_state')
        if (Number(row.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.proposals
             SET linked_project_id=$1, revision=$2, updated_at=clock_timestamp()
             WHERE tenant_id=$3 AND id=$4
             RETURNING *`,
            [mutation.projectId, nextRev, tenant, mutation.proposalId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: mutation.proposalId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, revision, occurred_at)
           VALUES($1,$2,$3,'link_project',$4,$5,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'create_change_request') {
        grant(access, 'changerequests.write')
        const propRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (!propRow) throw new ProposalError('not_found')
        if (propRow.state !== 'accepted')
          throw new ProposalError('invalid_state', 'Change request requires an accepted proposal.')

        const activeCr = (
          await client.query<Row>(
            "SELECT id FROM archava_proposals.change_requests WHERE tenant_id=$1 AND proposal_id=$2 AND state IN ('draft','submitted')",
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        if (activeCr)
          throw new ProposalError('active_change_request_exists', 'An open change request already exists.')

        const countRow = (
          await client.query<Row>(
            'SELECT COALESCE(MAX(change_number), 0)::integer AS max_num FROM archava_proposals.change_requests WHERE tenant_id=$1 AND proposal_id=$2',
            [tenant, mutation.proposalId],
          )
        ).rows[0]
        const changeNumber = (Number(countRow?.max_num) || 0) + 1
        const crId = randomUUID()

        const crRow = (
          await client.query<Row>(
            `INSERT INTO archava_proposals.change_requests(
              tenant_id, id, proposal_id, change_number, title, reason, delta_summary,
              time_delta_days, cost_delta_value, state, owner_id, revision
            ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'draft',$10,1)
            RETURNING *`,
            [
              tenant,
              crId,
              mutation.proposalId,
              changeNumber,
              mutation.title,
              mutation.reason,
              mutation.deltaSummary,
              mutation.timeDeltaDays,
              mutation.costDeltaValue,
              access.ownerId,
            ],
          )
        ).rows[0]!

        // Save new scope items temporarily keyed under negative version or specific table?
        // Or store draft new scope items directly associated with CR in a json or scope_items table?
        // Let's store them in scope_items keyed to (scope_version = 0) with a change_request_id column, OR
        // let's store the proposed items in a column, or scope_version = (10000 + changeNumber).
        // Better yet: we can store proposed items in change_requests table as json or insert when approved!
        // Notice: approve_change_request receives new scope items!
        // We can store proposed items in audit / change_requests delta. Let's add new_items jsonb to change_requests or pass on approval!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(crRow.created_at),
          proposalId: mutation.proposalId,
          changeRequestId: crId,
          revision: 1,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, occurred_at)
           VALUES($1,$2,$3,'create_change_request',$4,$5,1,$6)`,
          [tenant, mutation.commandId, access.ownerId, mutation.proposalId, crId, crRow.created_at],
        )
      } else if (mutation.type === 'update_change_request') {
        grant(access, 'changerequests.write')
        const crRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.changeRequestId],
          )
        ).rows[0]
        if (!crRow) throw new ProposalError('not_found')
        if (crRow.state !== 'draft') throw new ProposalError('invalid_state')
        if (Number(crRow.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.change_requests
             SET title=$1, reason=$2, delta_summary=$3, time_delta_days=$4, cost_delta_value=$5, revision=$6, updated_at=clock_timestamp()
             WHERE tenant_id=$7 AND id=$8
             RETURNING *`,
            [
              mutation.title,
              mutation.reason,
              mutation.deltaSummary,
              mutation.timeDeltaDays,
              mutation.costDeltaValue,
              nextRev,
              tenant,
              mutation.changeRequestId,
            ],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: String(updated.proposal_id),
          changeRequestId: mutation.changeRequestId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, occurred_at)
           VALUES($1,$2,$3,'update_change_request',$4,$5,$6,$7)`,
          [tenant, mutation.commandId, access.ownerId, updated.proposal_id, mutation.changeRequestId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'submit_change_request') {
        grant(access, 'changerequests.write')
        const crRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.changeRequestId],
          )
        ).rows[0]
        if (!crRow) throw new ProposalError('not_found')
        if (crRow.state !== 'draft') throw new ProposalError('invalid_state')
        if (Number(crRow.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.change_requests
             SET state='submitted', submitted_at=clock_timestamp(), revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.changeRequestId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: String(updated.proposal_id),
          changeRequestId: mutation.changeRequestId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, occurred_at)
           VALUES($1,$2,$3,'submit_change_request',$4,$5,$6,$7)`,
          [tenant, mutation.commandId, access.ownerId, updated.proposal_id, mutation.changeRequestId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'approve_change_request') {
        grant(access, 'changerequests.decide')
        const crRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.changeRequestId],
          )
        ).rows[0]
        if (!crRow) throw new ProposalError('not_found')
        if (crRow.state !== 'submitted') throw new ProposalError('invalid_state')
        if (Number(crRow.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const proposalId = String(crRow.proposal_id)
        const propRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, proposalId],
          )
        ).rows[0]
        if (!propRow) throw new ProposalError('not_found')
        if (propRow.state !== 'accepted') throw new ProposalError('invalid_state')

        const nextScopeVersion = Number(propRow.current_scope_version) + 1
        const nextPropRevision = Number(propRow.revision) + 1
        const nextCrRevision = mutation.expectedRevision + 1

        // Current scope items
        const currentItems = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=$3 ORDER BY created_at ASC',
            [tenant, proposalId, propRow.current_scope_version],
          )
        ).rows.map(mapScopeItem)

        // Clone current items into nextScopeVersion and append amendment delta item!
        for (const it of currentItems) {
          await client.query(
            `INSERT INTO archava_proposals.scope_items(
              tenant_id, id, proposal_id, scope_version, title, description, estimated_days, estimated_value, deliverable_type
            ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)`,
            [
              tenant,
              randomUUID(),
              proposalId,
              nextScopeVersion,
              it.title,
              it.description,
              it.estimatedDays,
              it.estimatedValue,
              it.deliverableType,
            ],
          )
        }

        // Add amendment scope item
        const amendmentItemId = randomUUID()
        await client.query(
          `INSERT INTO archava_proposals.scope_items(
            tenant_id, id, proposal_id, scope_version, title, description, estimated_days, estimated_value, deliverable_type
          ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'service')`,
          [
            tenant,
            amendmentItemId,
            proposalId,
            nextScopeVersion,
            `CR-${crRow.change_number}: ${crRow.title}`,
            crRow.delta_summary,
            Number(crRow.time_delta_days) || 0,
            Number(crRow.cost_delta_value) || 0,
          ],
        )

        // Compute new totals
        const newItems = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=$3 ORDER BY created_at ASC',
            [tenant, proposalId, nextScopeVersion],
          )
        ).rows.map(mapScopeItem)

        const totalVal = newItems.reduce((acc: number, it: ProposalScopeItem) => acc + (it.estimatedValue ?? 0), 0)
        const totalDays = newItems.reduce((acc: number, it: ProposalScopeItem) => acc + (it.estimatedDays ?? 0), 0)

        // Update proposal
        await client.query(
          `UPDATE archava_proposals.proposals
           SET current_scope_version=$1, total_estimated_value=$2, revision=$3, updated_at=clock_timestamp()
           WHERE tenant_id=$4 AND id=$5`,
          [nextScopeVersion, totalVal, nextPropRevision, tenant, proposalId],
        )

        // Update change request
        const updatedCr = (
          await client.query<Row>(
            `UPDATE archava_proposals.change_requests
             SET state='approved', resulting_scope_version=$1, decided_at=clock_timestamp(), decided_by=$2, revision=$3, updated_at=clock_timestamp()
             WHERE tenant_id=$4 AND id=$5
             RETURNING *`,
            [nextScopeVersion, access.ownerId, nextCrRevision, tenant, mutation.changeRequestId],
          )
        ).rows[0]!

        // Record scope version snapshot
        await client.query(
          `INSERT INTO archava_proposals.scope_versions(
            tenant_id, id, proposal_id, scope_version, change_request_id, summary, total_value, total_days, confirmed_at, confirmed_by, items
          ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)`,
          [
            tenant,
            randomUUID(),
            proposalId,
            nextScopeVersion,
            mutation.changeRequestId,
            `Agreed Scope Version ${nextScopeVersion} (Amendment via CR-${crRow.change_number})`,
            totalVal,
            totalDays,
            updatedCr.decided_at,
            access.ownerId,
            JSON.stringify(newItems),
          ],
        )

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updatedCr.updated_at),
          proposalId,
          changeRequestId: mutation.changeRequestId,
          scopeVersion: nextScopeVersion,
          revision: nextCrRevision,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, occurred_at)
           VALUES($1,$2,$3,'approve_change_request',$4,$5,$6,$7)`,
          [tenant, mutation.commandId, access.ownerId, proposalId, mutation.changeRequestId, nextCrRevision, updatedCr.updated_at],
        )
      } else if (mutation.type === 'reject_change_request') {
        grant(access, 'changerequests.decide')
        const crRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.changeRequestId],
          )
        ).rows[0]
        if (!crRow) throw new ProposalError('not_found')
        if (crRow.state !== 'submitted') throw new ProposalError('invalid_state')
        if (Number(crRow.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.change_requests
             SET state='rejected', decided_at=clock_timestamp(), decided_by=$1, revision=$2, updated_at=clock_timestamp()
             WHERE tenant_id=$3 AND id=$4
             RETURNING *`,
            [access.ownerId, nextRev, tenant, mutation.changeRequestId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: String(updated.proposal_id),
          changeRequestId: mutation.changeRequestId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, reason, occurred_at)
           VALUES($1,$2,$3,'reject_change_request',$4,$5,$6,$7,$8)`,
          [tenant, mutation.commandId, access.ownerId, updated.proposal_id, mutation.changeRequestId, nextRev, mutation.reason, updated.updated_at],
        )
      } else if (mutation.type === 'withdraw_change_request') {
        grant(access, 'changerequests.write')
        const crRow = (
          await client.query<Row>(
            'SELECT * FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.changeRequestId],
          )
        ).rows[0]
        if (!crRow) throw new ProposalError('not_found')
        if (!['draft', 'submitted'].includes(crRow.state as string)) throw new ProposalError('invalid_state')
        if (Number(crRow.revision) !== mutation.expectedRevision) throw new ProposalError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_proposals.change_requests
             SET state='withdrawn', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.changeRequestId],
          )
        ).rows[0]!

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          proposalId: String(updated.proposal_id),
          changeRequestId: mutation.changeRequestId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_proposals.audit(tenant_id, command_id, actor_id, operation, proposal_id, change_request_id, revision, reason, occurred_at)
           VALUES($1,$2,$3,'withdraw_change_request',$4,$5,$6,$7,$8)`,
          [tenant, mutation.commandId, access.ownerId, updated.proposal_id, mutation.changeRequestId, nextRev, mutation.reason, updated.updated_at],
        )
      } else {
        throw new ProposalError('invalid_request')
      }

      await client.query(
        'UPDATE archava_proposals.commands SET receipt=$1 WHERE tenant_id=$2 AND command_id=$3',
        [JSON.stringify(receipt), tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async commandStatus(
    tenant: string,
    principal: ProposalPrincipal,
    commandId: string,
  ): Promise<CommandStatusResult> {
    return await this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_proposals.commands WHERE tenant_id=$1 AND command_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'committed',
        receipt: commandReceiptSchema.parse(row.receipt),
      }
    })
  }
}
