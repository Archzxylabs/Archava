import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  CrmError,
  tenantSchema,
  principalSchema,
  membershipSchema,
  contactSchema,
  activitySchema,
  followupSchema,
  idSchema,
  pageSchema,
  mutationSchema,
  type CrmPrincipal,
  type CrmPermission,
  type CrmAccess,
  type CrmRepository,
  type CrmPage,
  type ContactPage,
  type ContactDetail,
  type CrmMutation,
  type MutationReceipt,
} from '@archava/crm'
import { CRM_MIGRATION, crmMigrationChecksum } from './migrate.js'

type Row = Record<string, unknown>
const timestamp = (value: unknown): string => {
  if (!(value instanceof Date) || !Number.isFinite(value.getTime()))
    throw new Error('Invalid database timestamp')
  return value.toISOString()
}
const contact = (row: Row) =>
  contactSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    fullName: row.full_name,
    email: row.email,
    organization: row.organization,
    ownerId: row.owner_id,
    revision: row.revision,
    createdAt: timestamp(row.created_at),
    updatedAt: timestamp(row.updated_at),
  })
const activity = (row: Row) =>
  activitySchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    contactId: row.contact_id,
    kind: row.kind,
    summary: row.summary,
    actorId: row.actor_id,
    createdAt: timestamp(row.created_at),
  })
const followup = (row: Row) =>
  followupSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    contactId: row.contact_id,
    summary: row.summary,
    dueAt: timestamp(row.due_at),
    status: row.status,
    revision: row.revision,
    createdAt: timestamp(row.created_at),
    updatedAt: timestamp(row.updated_at),
  })
const receiptSchema = z
  .object({
    kind: z.enum(['contact', 'activity', 'followup']),
    id: idSchema,
    revision: z.number().int().positive(),
  })
  .strict()
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

export class PostgresCrmRepository implements CrmRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly fingerprintKey: string,
  ) {}

  static async create(pool: Pool, fingerprintKey: string): Promise<PostgresCrmRepository> {
    if (fingerprintKey.length < 32)
      throw new Error(
        'CRM fingerprint key must contain at least 32 characters and remain stable across restarts',
      )
    const role = await pool.query<{ unsafe: boolean }>(
      `SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_crm'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user`,
    )
    if (role.rows[0]?.unsafe !== false)
      throw new Error('CRM runtime must use a non-owner role without superuser/BYPASSRLS')
    const migrations = await pool.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_crm.schema_migrations',
    )
    if (
      migrations.rows.length !== 1 ||
      migrations.rows[0]?.id !== CRM_MIGRATION ||
      migrations.rows[0]?.checksum !== (await crmMigrationChecksum())
    )
      throw new Error('Unsupported CRM schema version/checksum')
    const policies = await pool.query<{ count: number }>(
      "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_crm'::regnamespace AND relrowsecurity AND relforcerowsecurity",
    )
    if (policies.rows[0]?.count !== 6)
      throw new Error('CRM tenant row policies are not enabled/forced')
    return new PostgresCrmRepository(pool, fingerprintKey)
  }

  private async transaction<T>(
    tenant: string,
    principal: CrmPrincipal,
    permission: CrmPermission | null,
    run: (client: PoolClient, access: CrmAccess) => Promise<T>,
  ): Promise<T> {
    tenantSchema.parse(tenant)
    principalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const access = await this.authorize(client, tenant, principal)
      if (permission && !access.permissions.includes(permission))
        throw new CrmError('forbidden', 'CRM permission denied.')
      const result = await run(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      throw error
    } finally {
      client.release()
    }
  }

  private async authorize(
    client: PoolClient,
    tenant: string,
    principal: CrmPrincipal,
  ): Promise<CrmAccess> {
    const member = async (id: string) => {
      const result = await client.query<Row>('SELECT * FROM archava_crm.lock_membership($1,$2)', [
        tenant,
        id,
      ])
      const row = result.rows[0]
      if (!row) throw new CrmError('forbidden', 'CRM membership required.')
      const parsed = membershipSchema.parse({
        principalId: row.principal_id,
        kind: row.kind,
        permissions: row.permissions,
        scope: row.scope,
        active: row.active,
      })
      if (!parsed.active) throw new CrmError('forbidden', 'CRM membership is inactive.')
      return parsed
    }
    const actor = await member(principal.id)
    if (actor.kind !== principal.kind)
      throw new CrmError('forbidden', 'CRM principal kind mismatch.')
    if (principal.kind === 'human')
      return { permissions: actor.permissions, ownerId: actor.principalId, scope: actor.scope }
    const human = await member(principal.onBehalfOf)
    if (human.kind !== 'human') throw new CrmError('forbidden', 'A human initiator is required.')
    return {
      permissions: actor.permissions.filter((permission) => human.permissions.includes(permission)),
      ownerId: human.principalId,
      scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
    }
  }

  private async ownedContact(
    client: PoolClient,
    tenant: string,
    access: CrmAccess,
    id: string,
    lock = false,
  ) {
    const result = await client.query<Row>(
      `SELECT * FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) ${lock ? 'FOR UPDATE' : ''}`,
      [tenant, id, access.scope === 'all', access.ownerId],
    )
    const row = result.rows[0]
    if (!row) throw new CrmError('not_found', 'CRM contact not found.')
    return contact(row)
  }

  access(tenant: string, principal: CrmPrincipal): Promise<CrmAccess> {
    return this.transaction(tenant, principal, null, (_client, access) => Promise.resolve(access))
  }
  list(tenant: string, principal: CrmPrincipal, raw: CrmPage): Promise<ContactPage> {
    const page = pageSchema.parse(raw)
    return this.transaction(tenant, principal, 'contacts.read', async (client, access) => {
      const search = page.search.replace(/[\\%_]/g, (char) => '\\' + char)
      const result = await client.query<Row>(
        `SELECT * FROM archava_crm.contacts WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) AND full_name ILIKE $5 ESCAPE '\\' ORDER BY id LIMIT $6`,
        [
          tenant,
          access.scope === 'all',
          access.ownerId,
          page.after ?? null,
          '%' + search + '%',
          page.limit + 1,
        ],
      )
      const items = result.rows.slice(0, page.limit).map(contact)
      return { items, next: result.rows.length > page.limit ? (items.at(-1)?.id ?? null) : null }
    })
  }
  detail(tenant: string, principal: CrmPrincipal, rawId: string): Promise<ContactDetail> {
    const id = idSchema.parse(rawId)
    return this.transaction(tenant, principal, 'contacts.read', async (client, access) => {
      const record = await this.ownedContact(client, tenant, access, id)
      const activities = access.permissions.includes('activities.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_crm.activities WHERE tenant_id=$1 AND contact_id=$2 ORDER BY created_at DESC,id DESC LIMIT 20',
              [tenant, id],
            )
          ).rows.map(activity)
        : []
      const followups = access.permissions.includes('followups.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2 ORDER BY due_at,id LIMIT 20',
              [tenant, id],
            )
          ).rows.map(followup)
        : []
      return { contact: record, activities, followups }
    })
  }

  mutate(tenant: string, principal: CrmPrincipal, raw: CrmMutation): Promise<MutationReceipt> {
    if (principal.kind !== 'human')
      throw new CrmError('forbidden', 'Machine CRM writes require a verified Act integration.')
    const mutation = mutationSchema.parse(raw)
    const command = mutation.command
    const permission: CrmPermission = command.operation.startsWith('contact.')
      ? 'contacts.write'
      : command.operation === 'activity.add'
        ? 'activities.write'
        : 'followups.write'
    const fingerprint = createHmac('sha256', this.fingerprintKey)
      .update(canonical({ tenant, principal, command }))
      .digest('hex')
    return this.transaction(tenant, principal, permission, async (client, access) => {
      const claimed = await client.query<Row>(
        'INSERT INTO archava_crm.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id',
        [tenant, mutation.requestId, principal.id, fingerprint],
      )
      if (claimed.rows.length === 0) {
        const existing = (
          await client.query<Row>(
            'SELECT * FROM archava_crm.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, mutation.requestId],
          )
        ).rows[0]
        if (!existing || existing.actor_id !== principal.id || existing.fingerprint !== fingerprint)
          throw new CrmError('conflict', 'CRM request identity already belongs to another command.')
        const receipt = receiptSchema.parse(existing.receipt)
        if (receipt.kind === 'contact') await this.ownedContact(client, tenant, access, receipt.id)
        else {
          const table = receipt.kind === 'activity' ? 'activities' : 'followups'
          const row = (
            await client.query<Row>(
              `SELECT contact_id FROM archava_crm.${table} WHERE tenant_id=$1 AND id=$2`,
              [tenant, receipt.id],
            )
          ).rows[0]
          if (!row || typeof row.contact_id !== 'string')
            throw new CrmError('not_found', 'CRM resource not found.')
          await this.ownedContact(client, tenant, access, row.contact_id)
        }
        return receipt
      }
      let receipt: MutationReceipt
      if (command.operation === 'contact.create') {
        const id = randomUUID()
        await client.query(
          'INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,organization,owner_id) VALUES($1,$2,$3,$4,$5,$6)',
          [
            tenant,
            id,
            command.data.fullName,
            command.data.email,
            command.data.organization,
            access.ownerId,
          ],
        )
        receipt = { kind: 'contact', id, revision: 1 }
      } else if (command.operation === 'followup.complete') {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_crm.followups WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, command.id],
          )
        ).rows[0]
        if (!row || typeof row.contact_id !== 'string')
          throw new CrmError('not_found', 'CRM follow-up not found.')
        await this.ownedContact(client, tenant, access, row.contact_id)
        if (row.revision !== command.expectedRevision || row.status !== 'open')
          throw new CrmError('conflict', 'CRM follow-up revision changed.')
        await client.query(
          "UPDATE archava_crm.followups SET status='completed',revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND id=$2",
          [tenant, command.id],
        )
        receipt = { kind: 'followup', id: command.id, revision: command.expectedRevision + 1 }
      } else {
        const id = command.operation === 'contact.update' ? command.id : command.contactId
        const record = await this.ownedContact(client, tenant, access, id, true)
        const expected =
          command.operation === 'contact.update'
            ? command.expectedRevision
            : command.contactRevision
        if (record.revision !== expected)
          throw new CrmError('conflict', 'CRM contact revision changed.')
        if (command.operation === 'contact.update') {
          await client.query(
            'UPDATE archava_crm.contacts SET full_name=$3,email=$4,organization=$5,revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND id=$2',
            [
              tenant,
              id,
              command.data.fullName ?? record.fullName,
              command.data.email === undefined ? record.email : command.data.email,
              command.data.organization === undefined
                ? record.organization
                : command.data.organization,
            ],
          )
          receipt = { kind: 'contact', id, revision: expected + 1 }
        } else {
          const resourceId = randomUUID()
          if (command.operation === 'activity.add')
            await client.query(
              'INSERT INTO archava_crm.activities(tenant_id,id,contact_id,kind,summary,actor_id) VALUES($1,$2,$3,$4,$5,$6)',
              [tenant, resourceId, id, command.kind, command.summary, principal.id],
            )
          else
            await client.query(
              'INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) VALUES($1,$2,$3,$4,$5)',
              [tenant, resourceId, id, command.summary, command.dueAt],
            )
          await client.query(
            'UPDATE archava_crm.contacts SET revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND id=$2',
            [tenant, id],
          )
          receipt = {
            kind: command.operation === 'activity.add' ? 'activity' : 'followup',
            id: resourceId,
            revision: 1,
          }
        }
      }
      await client.query(
        'INSERT INTO archava_crm.audit(tenant_id,request_id,actor_id,operation,resource_id,revision) VALUES($1,$2,$3,$4,$5,$6)',
        [tenant, mutation.requestId, principal.id, command.operation, receipt.id, receipt.revision],
      )
      await client.query(
        'UPDATE archava_crm.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, mutation.requestId, receipt],
      )
      return receipt
    })
  }
}
