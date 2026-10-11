import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  OrganizationError,
  orgTenantSchema,
  orgPrincipalSchema,
  organizationMembershipSchema,
  organizationSchema,
  affiliationSchema,
  organizationReceiptSchema,
  type OrganizationRepository,
  type OrganizationPrincipal,
  type OrganizationAccess,
  type OrganizationPermission,
  type OrganizationMutation,
  type OrganizationReceipt,
  type OrganizationCommandStatus,
  type OrganizationPageQuery,
  type AffiliationPageQuery,
  type Organization,
  type Affiliation,
} from '@archava/organizations'
import { organizationMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid organization timestamp')
  return x.toISOString()
}

const nullable = (x: unknown) => (x === null || x === undefined ? null : String(x))

const mapOrg = (r: Row): Organization =>
  organizationSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    name: r.name,
    legalName: nullable(r.legal_name),
    taxId: nullable(r.tax_id),
    website: nullable(r.website),
    industry: nullable(r.industry),
    billingAddress: nullable(r.billing_address),
    state: r.state,
    ownerId: r.owner_id,
    revision: r.revision,
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapAffiliation = (r: Row): Affiliation =>
  affiliationSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    organizationId: r.organization_id,
    contactId: r.contact_id,
    role: r.role,
    title: nullable(r.title),
    department: nullable(r.department),
    isPrimary: r.is_primary,
    status: r.status,
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const grant = (a: OrganizationAccess, p: OrganizationPermission) => {
  if (!a.permissions.includes(p)) throw new OrganizationError('forbidden')
}

export class PostgresOrganizationRepository implements OrganizationRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}

  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable organization fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_organizations'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Organizations requires a non-owner runtime')

    const expected = await organizationMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_organizations.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Unsupported organizations ledger')

    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_organizations'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'organizations', 'affiliations', 'commands', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 5) throw new Error('Organizations tenant policies must be forced')

    return new PostgresOrganizationRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: OrganizationPrincipal,
    work: (client: PoolClient, access: OrganizationAccess) => Promise<T>,
  ): Promise<T> {
    orgTenantSchema.parse(tenant)
    orgPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])

      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_organizations.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new OrganizationError('forbidden')
        const m = organizationMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new OrganizationError('forbidden')
        return m
      }

      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new OrganizationError('forbidden')

      let access: OrganizationAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new OrganizationError('forbidden')
        access = {
          permissions: actor.permissions.filter((x: OrganizationPermission) =>
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
        throw new OrganizationError('conflict')
      throw error
    } finally {
      client.release()
    }
  }

  async access(tenantId: string, principal: OrganizationPrincipal): Promise<OrganizationAccess> {
    return this.transaction(tenantId, principal, async (_client, access) => access)
  }

  async listOrganizations(
    tenantId: string,
    principal: OrganizationPrincipal,
    query: OrganizationPageQuery,
  ): Promise<{ items: Organization[]; nextCursor: string | null }> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'organizations.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenantId]

      if (access.scope === 'assigned') {
        params.push(access.ownerId)
        conditions.push(`owner_id = $${params.length}`)
      }
      if (query.state) {
        params.push(query.state)
        conditions.push(`state = $${params.length}`)
      }
      if (query.search) {
        params.push(`%${query.search}%`)
        conditions.push(`(name ILIKE $${params.length} OR legal_name ILIKE $${params.length})`)
      }
      if (query.after) {
        params.push(query.after)
        conditions.push(`id > $${params.length}`)
      }

      params.push(query.limit + 1)
      const sql = `
        SELECT * FROM archava_organizations.organizations
        WHERE ${conditions.join(' AND ')}
        ORDER BY id ASC
        LIMIT $${params.length}
      `
      const rows = (await client.query<Row>(sql, params)).rows
      const hasMore = rows.length > query.limit
      const items = rows.slice(0, query.limit).map(mapOrg)
      const nextCursor = hasMore ? items[items.length - 1]?.id ?? null : null

      return { items, nextCursor }
    })
  }

  async detail(
    tenantId: string,
    principal: OrganizationPrincipal,
    id: string,
  ): Promise<{ organization: Organization; affiliations: Affiliation[] }> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'organizations.read')
      const orgRow = (
        await client.query<Row>(
          'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2',
          [tenantId, id],
        )
      ).rows[0]
      if (!orgRow) throw new OrganizationError('not_found')
      const organization = mapOrg(orgRow)
      if (access.scope === 'assigned' && organization.ownerId !== access.ownerId)
        throw new OrganizationError('not_found')

      const affRows = (
        await client.query<Row>(
          'SELECT * FROM archava_organizations.affiliations WHERE tenant_id = $1 AND organization_id = $2 ORDER BY created_at ASC',
          [tenantId, id],
        )
      ).rows
      const affiliations = affRows.map(mapAffiliation)

      return { organization, affiliations }
    })
  }

  async listAffiliations(
    tenantId: string,
    principal: OrganizationPrincipal,
    organizationId: string,
    query: AffiliationPageQuery,
  ): Promise<{ items: Affiliation[]; nextCursor: string | null }> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'affiliations.read')
      const conditions: string[] = ['tenant_id = $1', 'organization_id = $2']
      const params: unknown[] = [tenantId, organizationId]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.role) {
        params.push(query.role)
        conditions.push(`role = $${params.length}`)
      }
      if (query.after) {
        params.push(query.after)
        conditions.push(`id > $${params.length}`)
      }

      params.push(query.limit + 1)
      const sql = `
        SELECT * FROM archava_organizations.affiliations
        WHERE ${conditions.join(' AND ')}
        ORDER BY id ASC
        LIMIT $${params.length}
      `
      const rows = (await client.query<Row>(sql, params)).rows
      const hasMore = rows.length > query.limit
      const items = rows.slice(0, query.limit).map(mapAffiliation)
      const nextCursor = hasMore ? items[items.length - 1]?.id ?? null : null

      return { items, nextCursor }
    })
  }

  async mutate(
    tenantId: string,
    principal: OrganizationPrincipal,
    mutation: OrganizationMutation,
  ): Promise<OrganizationReceipt> {
    return this.transaction(tenantId, principal, async (client, access) => {
      if (principal.kind !== 'human') throw new OrganizationError('forbidden')

      const fingerprint = createHmac('sha256', this.key)
        .update(JSON.stringify(mutation.data))
        .digest('hex')

      const existing = (
        await client.query<Row>(
          'SELECT * FROM archava_organizations.commands WHERE tenant_id = $1 AND request_id = $2 FOR UPDATE',
          [tenantId, mutation.requestId],
        )
      ).rows[0]

      if (existing) {
        if (existing.actor_id !== principal.id || existing.fingerprint !== fingerprint)
          throw new OrganizationError('conflict')
        if (existing.receipt) {
          return organizationReceiptSchema.parse(existing.receipt)
        }
        throw new OrganizationError('conflict')
      }

      await client.query(
        'INSERT INTO archava_organizations.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
        [tenantId, mutation.requestId, principal.id, fingerprint],
      )

      let receipt: OrganizationReceipt
      const now = (await client.query<{ now: Date }>('SELECT clock_timestamp() AS now')).rows[0]
        ?.now
      const committedAt = instant(now)

      if (mutation.type === 'create_organization') {
        grant(access, 'organizations.write')
        const orgId = randomUUID()
        const d = mutation.data
        await client.query(
          `INSERT INTO archava_organizations.organizations(
            tenant_id, id, name, legal_name, tax_id, website, industry, billing_address, state, owner_id, revision, created_at, updated_at
          ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9, 1, $10, $10)`,
          [
            tenantId,
            orgId,
            d.name,
            d.legalName ?? null,
            d.taxId ?? null,
            d.website ?? null,
            d.industry ?? null,
            d.billingAddress ?? null,
            principal.id,
            now,
          ],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'create_organization', $4, 1, $5)`,
          [tenantId, mutation.requestId, principal.id, orgId, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'create_organization',
          organizationId: orgId,
          affiliationId: null,
          revision: 1,
          committedAt,
        }
      } else if (mutation.type === 'update_organization') {
        grant(access, 'organizations.write')
        const d = mutation.data
        const orgRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.id],
          )
        ).rows[0]
        if (!orgRow) throw new OrganizationError('not_found')
        if (orgRow.state === 'archived') throw new OrganizationError('archived')
        if (Number(orgRow.revision) !== d.revision) throw new OrganizationError('conflict')

        const nextRevision = d.revision + 1
        await client.query(
          `UPDATE archava_organizations.organizations SET
            name = COALESCE($3, name),
            legal_name = CASE WHEN $4::text IS NOT NULL THEN $4 ELSE legal_name END,
            tax_id = CASE WHEN $5::text IS NOT NULL THEN $5 ELSE tax_id END,
            website = CASE WHEN $6::text IS NOT NULL THEN $6 ELSE website END,
            industry = CASE WHEN $7::text IS NOT NULL THEN $7 ELSE industry END,
            billing_address = CASE WHEN $8::text IS NOT NULL THEN $8 ELSE billing_address END,
            revision = $9,
            updated_at = $10
          WHERE tenant_id = $1 AND id = $2`,
          [
            tenantId,
            d.id,
            d.name ?? null,
            d.legalName !== undefined ? d.legalName : null,
            d.taxId !== undefined ? d.taxId : null,
            d.website !== undefined ? d.website : null,
            d.industry !== undefined ? d.industry : null,
            d.billingAddress !== undefined ? d.billingAddress : null,
            nextRevision,
            now,
          ],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'update_organization', $4, $5, $6)`,
          [tenantId, mutation.requestId, principal.id, d.id, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'update_organization',
          organizationId: d.id,
          affiliationId: null,
          revision: nextRevision,
          committedAt,
        }
      } else if (mutation.type === 'archive_organization') {
        grant(access, 'organizations.write')
        const d = mutation.data
        const orgRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.id],
          )
        ).rows[0]
        if (!orgRow) throw new OrganizationError('not_found')
        if (orgRow.state === 'archived') throw new OrganizationError('archived')
        if (Number(orgRow.revision) !== d.revision) throw new OrganizationError('conflict')

        const nextRevision = d.revision + 1
        await client.query(
          `UPDATE archava_organizations.organizations SET
            state = 'archived',
            revision = $3,
            updated_at = $4
          WHERE tenant_id = $1 AND id = $2`,
          [tenantId, d.id, nextRevision, now],
        )

        await client.query(
          `UPDATE archava_organizations.affiliations SET
            status = 'inactive',
            updated_at = $3
          WHERE tenant_id = $1 AND organization_id = $2 AND status = 'active'`,
          [tenantId, d.id, now],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, revision, reason, occurred_at
          ) VALUES($1, $2, $3, 'archive_organization', $4, $5, $6, $7)`,
          [tenantId, mutation.requestId, principal.id, d.id, nextRevision, d.reason, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'archive_organization',
          organizationId: d.id,
          affiliationId: null,
          revision: nextRevision,
          committedAt,
        }
      } else if (mutation.type === 'create_affiliation') {
        grant(access, 'affiliations.write')
        const d = mutation.data
        const orgRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.organizationId],
          )
        ).rows[0]
        if (!orgRow) throw new OrganizationError('not_found')
        if (orgRow.state === 'archived') throw new OrganizationError('archived')

        if (d.isPrimary) {
          const existingPrimary = (
            await client.query<Row>(
              "SELECT id FROM archava_organizations.affiliations WHERE tenant_id = $1 AND organization_id = $2 AND is_primary = true AND status = 'active'",
              [tenantId, d.organizationId],
            )
          ).rows[0]
          if (existingPrimary) throw new OrganizationError('primary_conflict')
        }

        const affId = randomUUID()
        const nextRevision = Number(orgRow.revision) + 1

        await client.query(
          `INSERT INTO archava_organizations.affiliations(
            tenant_id, id, organization_id, contact_id, role, title, department, is_primary, status, created_at, updated_at
          ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9, $9)`,
          [
            tenantId,
            affId,
            d.organizationId,
            d.contactId,
            d.role,
            d.title ?? null,
            d.department ?? null,
            d.isPrimary ?? false,
            now,
          ],
        )

        await client.query(
          'UPDATE archava_organizations.organizations SET revision = $3, updated_at = $4 WHERE tenant_id = $1 AND id = $2',
          [tenantId, d.organizationId, nextRevision, now],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, affiliation_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'create_affiliation', $4, $5, $6, $7)`,
          [tenantId, mutation.requestId, principal.id, d.organizationId, affId, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'create_affiliation',
          organizationId: d.organizationId,
          affiliationId: affId,
          revision: nextRevision,
          committedAt,
        }
      } else if (mutation.type === 'update_affiliation') {
        grant(access, 'affiliations.write')
        const d = mutation.data
        const orgRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.organizationId],
          )
        ).rows[0]
        if (!orgRow) throw new OrganizationError('not_found')
        if (orgRow.state === 'archived') throw new OrganizationError('archived')

        const affRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.affiliations WHERE tenant_id = $1 AND id = $2 AND organization_id = $3 FOR UPDATE',
            [tenantId, d.id, d.organizationId],
          )
        ).rows[0]
        if (!affRow) throw new OrganizationError('not_found')

        if (d.isPrimary && (d.status === undefined || d.status === 'active')) {
          const otherPrimary = (
            await client.query<Row>(
              "SELECT id FROM archava_organizations.affiliations WHERE tenant_id = $1 AND organization_id = $2 AND is_primary = true AND status = 'active' AND id <> $3",
              [tenantId, d.organizationId, d.id],
            )
          ).rows[0]
          if (otherPrimary) throw new OrganizationError('primary_conflict')
        }

        const nextRevision = Number(orgRow.revision) + 1
        await client.query(
          `UPDATE archava_organizations.affiliations SET
            role = COALESCE($4, role),
            title = CASE WHEN $5::text IS NOT NULL THEN $5 ELSE title END,
            department = CASE WHEN $6::text IS NOT NULL THEN $6 ELSE department END,
            is_primary = COALESCE($7, is_primary),
            status = COALESCE($8, status),
            updated_at = $9
          WHERE tenant_id = $1 AND id = $2 AND organization_id = $3`,
          [
            tenantId,
            d.id,
            d.organizationId,
            d.role ?? null,
            d.title !== undefined ? d.title : null,
            d.department !== undefined ? d.department : null,
            d.isPrimary !== undefined ? d.isPrimary : null,
            d.status ?? null,
            now,
          ],
        )

        await client.query(
          'UPDATE archava_organizations.organizations SET revision = $3, updated_at = $4 WHERE tenant_id = $1 AND id = $2',
          [tenantId, d.organizationId, nextRevision, now],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, affiliation_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'update_affiliation', $4, $5, $6, $7)`,
          [tenantId, mutation.requestId, principal.id, d.organizationId, d.id, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'update_affiliation',
          organizationId: d.organizationId,
          affiliationId: d.id,
          revision: nextRevision,
          committedAt,
        }
      } else {
        // remove_affiliation
        grant(access, 'affiliations.write')
        const d = mutation.data
        const orgRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.organizations WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.organizationId],
          )
        ).rows[0]
        if (!orgRow) throw new OrganizationError('not_found')

        const affRow = (
          await client.query<Row>(
            'SELECT * FROM archava_organizations.affiliations WHERE tenant_id = $1 AND id = $2 AND organization_id = $3 FOR UPDATE',
            [tenantId, d.id, d.organizationId],
          )
        ).rows[0]
        if (!affRow) throw new OrganizationError('not_found')

        const nextRevision = Number(orgRow.revision) + 1
        await client.query(
          `UPDATE archava_organizations.affiliations SET status = 'inactive', updated_at = $4 WHERE tenant_id = $1 AND id = $2 AND organization_id = $3`,
          [tenantId, d.id, d.organizationId, now],
        )

        await client.query(
          'UPDATE archava_organizations.organizations SET revision = $3, updated_at = $4 WHERE tenant_id = $1 AND id = $2',
          [tenantId, d.organizationId, nextRevision, now],
        )

        await client.query(
          `INSERT INTO archava_organizations.audit(
            tenant_id, request_id, actor_id, operation, organization_id, affiliation_id, revision, reason, occurred_at
          ) VALUES($1, $2, $3, 'remove_affiliation', $4, $5, $6, $7, $8)`,
          [tenantId, mutation.requestId, principal.id, d.organizationId, d.id, nextRevision, d.reason, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'remove_affiliation',
          organizationId: d.organizationId,
          affiliationId: d.id,
          revision: nextRevision,
          committedAt,
        }
      }

      await client.query(
        'UPDATE archava_organizations.commands SET receipt = $3 WHERE tenant_id = $1 AND request_id = $2',
        [tenantId, mutation.requestId, JSON.stringify(receipt)],
      )

      return receipt
    })
  }

  async commandStatus(
    tenantId: string,
    principal: OrganizationPrincipal,
    requestId: string,
  ): Promise<OrganizationCommandStatus> {
    return this.transaction(tenantId, principal, async (client, _access) => {
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_organizations.commands WHERE tenant_id = $1 AND request_id = $2',
          [tenantId, requestId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'not_committed' }
      return {
        status: 'committed',
        receipt: organizationReceiptSchema.parse(row.receipt),
      }
    })
  }
}
