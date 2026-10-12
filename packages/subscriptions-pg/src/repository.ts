import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  SubscriptionError,
  subscriptionTenantSchema,
  subscriptionPrincipalSchema,
  subscriptionMembershipSchema,
  subscriptionSchema,
  entitlementSchema,
  usageEventSchema,
  subscriptionReceiptSchema,
  type SubscriptionRepository,
  type SubscriptionPrincipal,
  type SubscriptionAccess,
  type SubscriptionPermission,
  type SubscriptionMutation,
  type SubscriptionReceipt,
  type SubscriptionCommandStatus,
  type SubscriptionPageQuery,
  type Subscription,
  type Entitlement,
  type UsageEvent,
  type SubscriptionDetail,
} from '@archava/subscriptions'
import { subscriptionMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid subscription timestamp')
  return x.toISOString()
}

const nullable = (x: unknown) => (typeof x === 'string' ? x : null)
const nullableDate = (x: unknown) =>
  x instanceof Date && Number.isFinite(x.getTime()) ? x.toISOString() : null

const mapSubscription = (r: Row): Subscription =>
  subscriptionSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    contactId: r.contact_id,
    planCode: r.plan_code,
    planName: r.plan_name,
    status: r.status,
    billingInterval: r.billing_interval,
    unitPrice: r.unit_price,
    currency: r.currency,
    quantity: r.quantity,
    currentPeriodStart: nullableDate(r.current_period_start),
    currentPeriodEnd: nullableDate(r.current_period_end),
    cancelAtPeriodEnd: r.cancel_at_period_end,
    paymentReference: nullable(r.payment_reference),
    ownerId: r.owner_id,
    revision: r.revision,
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapEntitlement = (r: Row): Entitlement =>
  entitlementSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    subscriptionId: nullable(r.subscription_id),
    contactId: r.contact_id,
    featureCode: r.feature_code,
    featureName: r.feature_name,
    entitlementType: r.entitlement_type,
    allocatedQuantity: typeof r.allocated_quantity === 'number' ? r.allocated_quantity : null,
    consumedQuantity: r.consumed_quantity,
    status: r.status,
    validFrom: instant(r.valid_from),
    validUntil: nullableDate(r.valid_until),
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapUsageEvent = (r: Row): UsageEvent =>
  usageEventSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    entitlementId: r.entitlement_id,
    subscriptionId: nullable(r.subscription_id),
    contactId: r.contact_id,
    unitsConsumed: r.units_consumed,
    idempotencyKey: r.idempotency_key,
    evidenceRef: nullable(r.evidence_ref),
    recordedBy: r.recorded_by,
    recordedAt: instant(r.recorded_at),
  })

const grant = (a: SubscriptionAccess, p: SubscriptionPermission) => {
  if (!a.permissions.includes(p)) throw new SubscriptionError('forbidden')
}

export class PostgresSubscriptionRepository implements SubscriptionRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}

  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable subscription fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_subscriptions'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Subscriptions requires a non-owner runtime')

    const expected = await subscriptionMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_subscriptions.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Unsupported subscriptions ledger')

    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_subscriptions'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'subscriptions', 'entitlements', 'usage_events', 'commands', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 6) throw new Error('Subscriptions tenant policies must be forced')

    return new PostgresSubscriptionRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: SubscriptionPrincipal,
    work: (client: PoolClient, access: SubscriptionAccess) => Promise<T>,
  ): Promise<T> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])

      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_subscriptions.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new SubscriptionError('forbidden')
        const m = subscriptionMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new SubscriptionError('forbidden')
        return m
      }

      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new SubscriptionError('forbidden')

      let access: SubscriptionAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new SubscriptionError('forbidden')
        access = {
          permissions: actor.permissions.filter((x: SubscriptionPermission) =>
            human.permissions.includes(x),
          ),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }

      const res = await work(client, access)
      await client.query('COMMIT')
      return res
    } catch (error) {
      await client.query('ROLLBACK')
      throw error
    } finally {
      client.release()
    }
  }

  async execute(
    tenant: string,
    principal: SubscriptionPrincipal,
    mutation: SubscriptionMutation,
  ): Promise<SubscriptionReceipt> {
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = createHmac('sha256', this.key)
        .update(JSON.stringify({ tenant, mutation }))
        .digest('hex')

      const existing = (
        await client.query<Row>(
          'SELECT receipt, fingerprint FROM archava_subscriptions.commands WHERE tenant_id=$1 AND request_id=$2 FOR UPDATE',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existing) {
        if (existing.fingerprint !== fingerprint) throw new SubscriptionError('conflict')
        if (existing.receipt) {
          const stored = subscriptionReceiptSchema.parse(existing.receipt)
          return { ...stored, idempotencyReplay: true }
        }
      } else {
        await client.query(
          'INSERT INTO archava_subscriptions.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
          [tenant, mutation.commandId, access.ownerId, fingerprint],
        )
      }

      let receipt: SubscriptionReceipt

      switch (mutation.type) {
        case 'create_subscription': {
          grant(access, 'subscriptions.write')
          const subId = randomUUID()
          const subRow = (
            await client.query<Row>(
              `INSERT INTO archava_subscriptions.subscriptions(
                tenant_id, id, contact_id, plan_code, plan_name, status,
                billing_interval, unit_price, currency, quantity, owner_id
              ) VALUES($1, $2, $3, $4, $5, 'draft', $6, $7, $8, $9, $10)
              RETURNING *`,
              [
                tenant,
                subId,
                mutation.contactId,
                mutation.planCode,
                mutation.planName,
                mutation.billingInterval,
                mutation.unitPrice,
                mutation.currency,
                mutation.quantity,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!subRow) throw new SubscriptionError('not_found', 'Failed to create subscription')

          for (const ent of mutation.initialEntitlements) {
            const entId = randomUUID()
            await client.query(
              `INSERT INTO archava_subscriptions.entitlements(
                tenant_id, id, subscription_id, contact_id, feature_code,
                feature_name, entitlement_type, allocated_quantity, valid_until
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
              [
                tenant,
                entId,
                subId,
                mutation.contactId,
                ent.featureCode,
                ent.featureName,
                ent.entitlementType,
                ent.allocatedQuantity ?? null,
                ent.validUntil ?? null,
              ],
            )
          }

          await client.query(
            'INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, subscription_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'create_subscription',
              subId,
              `Created ${mutation.planCode}`,
            ],
          )

          receipt = subscriptionReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'create_subscription',
            subscriptionId: subId,
            revision: 1,
            status: 'draft',
            idempotencyReplay: false,
            executedAt: instant(subRow.created_at),
          })
          break
        }

        case 'record_payment': {
          grant(access, 'subscriptions.payment')
          const sub = (
            await client.query<Row>(
              'SELECT * FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.subscriptionId],
            )
          ).rows[0]
          if (!sub) throw new SubscriptionError('not_found')

          const nextRev = (sub.revision as number) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_subscriptions.subscriptions
               SET status='active', payment_reference=$1, current_period_start=$2,
                   current_period_end=$3, revision=$4, updated_at=clock_timestamp()
               WHERE tenant_id=$5 AND id=$6
               RETURNING *`,
              [
                mutation.paymentReference,
                mutation.periodStart,
                mutation.periodEnd,
                nextRev,
                tenant,
                mutation.subscriptionId,
              ],
            )
          ).rows[0]
          if (!updated)
            throw new SubscriptionError('not_found', 'Subscription not found for update')

          // Re-activate suspended entitlements
          await client.query(
            `UPDATE archava_subscriptions.entitlements
             SET status='active', updated_at=clock_timestamp()
             WHERE tenant_id=$1 AND subscription_id=$2 AND status='suspended'`,
            [tenant, mutation.subscriptionId],
          )

          await client.query(
            'INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, subscription_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'record_payment',
              mutation.subscriptionId,
              nextRev,
              `Payment confirmed: ${mutation.paymentReference}`,
            ],
          )

          receipt = subscriptionReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_payment',
            subscriptionId: mutation.subscriptionId,
            revision: nextRev,
            status: 'active',
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }

        case 'consume_usage': {
          grant(access, 'subscriptions.usage')
          const ent = (
            await client.query<Row>(
              'SELECT * FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.entitlementId],
            )
          ).rows[0]
          if (!ent) throw new SubscriptionError('not_found')

          if (ent.status === 'suspended')
            throw new SubscriptionError('suspended', 'Entitlement is currently suspended')
          if (ent.status === 'revoked')
            throw new SubscriptionError('forbidden', 'Entitlement has been revoked')
          if (ent.status === 'exhausted')
            throw new SubscriptionError('exhausted', 'Entitlement quota exhausted')

          if (ent.valid_until && new Date(ent.valid_until as string).getTime() < Date.now()) {
            await client.query(
              "UPDATE archava_subscriptions.entitlements SET status='exhausted' WHERE tenant_id=$1 AND id=$2",
              [tenant, ent.id],
            )
            throw new SubscriptionError('exhausted', 'Entitlement validity period has expired')
          }

          if (ent.subscription_id) {
            const sub = (
              await client.query<Row>(
                'SELECT status FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2',
                [tenant, ent.subscription_id],
              )
            ).rows[0]
            if (sub) {
              if (sub.status === 'past_due')
                throw new SubscriptionError('past_due', 'Subscription is past due')
              if (
                sub.status === 'paused' ||
                sub.status === 'canceled' ||
                sub.status === 'expired'
              ) {
                throw new SubscriptionError('suspended', `Subscription is ${sub.status}`)
              }
            }
          }

          const currentConsumed = ent.consumed_quantity as number
          const allocated = ent.allocated_quantity as number | null
          const nextConsumed = currentConsumed + mutation.units

          if (allocated !== null && nextConsumed > allocated) {
            throw new SubscriptionError(
              'exhausted',
              `Quota exceeded: requested ${mutation.units}, remaining ${allocated - currentConsumed}`,
            )
          }

          const nextStatus =
            allocated !== null && nextConsumed >= allocated ? 'exhausted' : 'active'
          await client.query(
            'UPDATE archava_subscriptions.entitlements SET consumed_quantity=$1, status=$2, updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4',
            [nextConsumed, nextStatus, tenant, ent.id],
          )

          const usageId = randomUUID()
          const usageRow = (
            await client.query<Row>(
              `INSERT INTO archava_subscriptions.usage_events(
                tenant_id, id, entitlement_id, subscription_id, contact_id,
                units_consumed, idempotency_key, evidence_ref, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                usageId,
                ent.id,
                ent.subscription_id,
                ent.contact_id,
                mutation.units,
                mutation.idempotencyKey,
                mutation.evidenceRef ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!usageRow) throw new SubscriptionError('not_found', 'Failed to record usage event')

          await client.query(
            'INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, subscription_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'consume_usage',
              ent.subscription_id,
              `Consumed ${mutation.units} units from ${String(ent.feature_code)}`,
            ],
          )

          receipt = subscriptionReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'consume_usage',
            subscriptionId: nullable(ent.subscription_id),
            entitlementId: ent.id as string,
            usageEventId: usageId,
            status: nextStatus,
            idempotencyReplay: false,
            executedAt: instant(usageRow.recorded_at),
          })
          break
        }

        case 'change_status': {
          grant(access, 'subscriptions.write')
          const sub = (
            await client.query<Row>(
              'SELECT * FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.subscriptionId],
            )
          ).rows[0]
          if (!sub) throw new SubscriptionError('not_found')

          if (sub.revision !== mutation.expectedRevision) {
            throw new SubscriptionError('conflict', 'Optimistic revision conflict')
          }

          const nextRev = Number(sub.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_subscriptions.subscriptions
               SET status=$1, revision=$2, updated_at=clock_timestamp()
               WHERE tenant_id=$3 AND id=$4
               RETURNING *`,
              [mutation.targetStatus, nextRev, tenant, mutation.subscriptionId],
            )
          ).rows[0]
          if (!updated) throw new SubscriptionError('not_found', 'Subscription update failed')

          if (mutation.targetStatus === 'canceled' || mutation.targetStatus === 'paused') {
            const targetEntStatus = mutation.targetStatus === 'canceled' ? 'revoked' : 'suspended'
            await client.query(
              `UPDATE archava_subscriptions.entitlements
               SET status=$1, updated_at=clock_timestamp()
               WHERE tenant_id=$2 AND subscription_id=$3 AND status != 'revoked'`,
              [targetEntStatus, tenant, mutation.subscriptionId],
            )
          } else if (mutation.targetStatus === 'active') {
            await client.query(
              `UPDATE archava_subscriptions.entitlements
               SET status='active', updated_at=clock_timestamp()
               WHERE tenant_id=$1 AND subscription_id=$2 AND status='suspended'`,
              [tenant, mutation.subscriptionId],
            )
          }

          await client.query(
            'INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, subscription_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'change_status',
              mutation.subscriptionId,
              nextRev,
              `Changed status to ${mutation.targetStatus}: ${mutation.reason}`,
            ],
          )

          receipt = subscriptionReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'change_status',
            subscriptionId: mutation.subscriptionId,
            revision: nextRev,
            status: mutation.targetStatus,
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }

        case 'grant_entitlement': {
          grant(access, 'subscriptions.write')
          const entId = randomUUID()
          const entRow = (
            await client.query<Row>(
              `INSERT INTO archava_subscriptions.entitlements(
                tenant_id, id, subscription_id, contact_id, feature_code,
                feature_name, entitlement_type, allocated_quantity, valid_until
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                entId,
                mutation.subscriptionId ?? null,
                mutation.contactId,
                mutation.featureCode,
                mutation.featureName,
                mutation.entitlementType,
                mutation.allocatedQuantity ?? null,
                mutation.validUntil ?? null,
              ],
            )
          ).rows[0]
          if (!entRow) throw new SubscriptionError('not_found', 'Failed to grant entitlement')

          await client.query(
            'INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, subscription_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'grant_entitlement',
              mutation.subscriptionId ?? null,
              `Granted ${mutation.featureCode} to contact ${mutation.contactId}`,
            ],
          )

          receipt = subscriptionReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'grant_entitlement',
            subscriptionId: mutation.subscriptionId ?? null,
            entitlementId: entId,
            status: 'active',
            idempotencyReplay: false,
            executedAt: instant(entRow.created_at),
          })
          break
        }
      }

      await client.query(
        'UPDATE archava_subscriptions.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3',
        [receipt, tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: SubscriptionPrincipal,
    commandId: string,
  ): Promise<SubscriptionCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_subscriptions.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'recorded',
        receipt: subscriptionReceiptSchema.parse(row.receipt),
      }
    })
  }

  async getAccess(tenant: string, principal: SubscriptionPrincipal): Promise<SubscriptionAccess> {
    return this.transaction(tenant, principal, (_client, access) => Promise.resolve(access))
  }

  async listSubscriptions(
    tenant: string,
    principal: SubscriptionPrincipal,
    query: SubscriptionPageQuery,
  ): Promise<readonly Subscription[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'subscriptions.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.contactId) {
        params.push(query.contactId)
        conditions.push(`contact_id = $${params.length}`)
      }
      if (access.scope === 'assigned') {
        params.push(access.ownerId)
        conditions.push(`owner_id = $${params.length}`)
      }

      params.push(query.limit)
      const limitParam = params.length
      params.push(query.offset)
      const offsetParam = params.length

      const rows = (
        await client.query<Row>(
          `SELECT * FROM archava_subscriptions.subscriptions
           WHERE ${conditions.join(' AND ')}
           ORDER BY created_at DESC
           LIMIT $${limitParam} OFFSET $${offsetParam}`,
          params,
        )
      ).rows

      return rows.map(mapSubscription)
    })
  }

  async getSubscriptionDetail(
    tenant: string,
    principal: SubscriptionPrincipal,
    id: string,
  ): Promise<SubscriptionDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'subscriptions.read')
      const sub = (
        await client.query<Row>(
          'SELECT * FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2',
          [tenant, id],
        )
      ).rows[0]
      if (!sub) return null

      if (access.scope === 'assigned' && sub.owner_id !== access.ownerId) {
        return null
      }

      const entitlements = (
        await client.query<Row>(
          'SELECT * FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND subscription_id=$2 ORDER BY created_at ASC',
          [tenant, id],
        )
      ).rows.map(mapEntitlement)

      return {
        subscription: mapSubscription(sub),
        entitlements,
      }
    })
  }

  async listEntitlements(
    tenant: string,
    principal: SubscriptionPrincipal,
    targetId: string,
  ): Promise<readonly Entitlement[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'subscriptions.read')
      const isUuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
        targetId,
      )
      if (isUuid) {
        const sub = (
          await client.query<Row>(
            'SELECT id FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2',
            [tenant, targetId],
          )
        ).rows[0]
        if (!sub) throw new SubscriptionError('not_found')
        const rows = (
          await client.query<Row>(
            'SELECT * FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND subscription_id=$2 ORDER BY created_at ASC',
            [tenant, targetId],
          )
        ).rows
        return rows.map(mapEntitlement)
      } else {
        const rows = (
          await client.query<Row>(
            'SELECT * FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND contact_id=$2 ORDER BY created_at DESC',
            [tenant, targetId],
          )
        ).rows
        return rows.map(mapEntitlement)
      }
    })
  }

  async listUsageEvents(
    tenant: string,
    principal: SubscriptionPrincipal,
    entitlementId: string,
  ): Promise<readonly UsageEvent[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'subscriptions.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_subscriptions.usage_events WHERE tenant_id=$1 AND entitlement_id=$2 ORDER BY recorded_at DESC',
          [tenant, entitlementId],
        )
      ).rows
      return rows.map(mapUsageEvent)
    })
  }
}
