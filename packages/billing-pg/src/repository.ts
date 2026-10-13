import { createHash, createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  BillingError,
  billingReceiptSchema,
  billingAccessSchema,
  invoiceSchema,
  invoiceLineSchema,
  invoiceCorrectionSchema,
  paymentAttemptSchema,
  paymentEventSchema,
  invoiceDetailSchema,
  type BillingRepository,
  type BillingPrincipal,
  type BillingMutation,
  type BillingReceipt,
  type BillingAccess,
  type BillingCommandStatus,
  type BillingPermission,
  type InvoicePageQuery,
  type Invoice,
  type InvoiceStatus,
  type InvoiceDetail,
  type InvoiceLine,
  type InvoiceCorrection,
  type PaymentAttempt,
  type PaymentEvent,
} from '@archava/billing'
import { billingMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

function instant(d: unknown): string {
  if (d instanceof Date) return d.toISOString()
  if (typeof d === 'string') return new Date(d).toISOString()
  return new Date().toISOString()
}

function nullable<T>(v: unknown): T | null {
  return v === null || v === undefined ? null : (v as T)
}

/**
 * `bigint` money columns arrive from `pg` as strings, never numbers.
 *
 * Coercing here — once, at the row boundary — keeps every amount downstream an
 * exact integer, which is the whole reason the columns are `bigint` in the first
 * place. A float creeping in at this seam is exactly the drift the domain is
 * built to prevent.
 */
function minor(v: unknown): number {
  if (typeof v === 'number') return v
  const parsed = Number(v)
  if (!Number.isSafeInteger(parsed)) {
    throw new BillingError('conflict', 'Stored amount is outside the safe integer range')
  }
  return parsed
}

function grant(access: BillingAccess, permission: BillingPermission): void {
  if (!access.permissions.includes(permission)) {
    throw new BillingError('forbidden', `Missing required permission: ${permission}`)
  }
}

const ALLOWED_TRANSITIONS: Readonly<Record<InvoiceStatus, readonly InvoiceStatus[]>> =
  Object.freeze({
    draft: ['issued', 'void'],
    issued: ['settled', 'void'],
    settled: ['issued'],
    void: [],
  })

function mapInvoice(row: Row, tenant: string): Invoice {
  return invoiceSchema.parse({
    id: row.id,
    tenantId: tenant,
    invoiceCode: row.invoice_code,
    customerRef: row.customer_ref,
    currency: row.currency,
    status: row.status,
    grossMinor: minor(row.gross_minor),
    correctionMinor: minor(row.correction_minor),
    settledMinor: minor(row.settled_minor),
    outstandingMinor: minor(row.outstanding_minor),
    consumerRef: row.consumer_ref,
    revision: row.revision,
    createdAt: instant(row.created_at),
    issuedAt: nullable<string>(row.issued_at) === null ? null : instant(row.issued_at),
    settledAt: nullable<string>(row.settled_at) === null ? null : instant(row.settled_at),
    voidedAt: nullable<string>(row.voided_at) === null ? null : instant(row.voided_at),
  })
}

function mapLine(row: Row): InvoiceLine {
  return invoiceLineSchema.parse({
    id: row.id,
    invoiceId: row.invoice_id,
    description: row.description,
    quantity: row.quantity,
    unitMinor: minor(row.unit_minor),
    lineTotalMinor: minor(row.line_total_minor),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

function mapCorrection(row: Row): InvoiceCorrection {
  return invoiceCorrectionSchema.parse({
    id: row.id,
    invoiceId: row.invoice_id,
    amountMinor: minor(row.amount_minor),
    reason: row.reason,
    reversesId: nullable<string>(row.reverses_id),
    createdAt: instant(row.created_at),
    recordedBy: row.recorded_by,
  })
}

function mapAttempt(row: Row): PaymentAttempt {
  return paymentAttemptSchema.parse({
    id: row.id,
    invoiceId: row.invoice_id,
    amountMinor: minor(row.amount_minor),
    currency: row.currency,
    providerRef: nullable<string>(row.provider_ref),
    recordedAt: instant(row.created_at),
    recordedBy: row.recorded_by,
  })
}

function mapEvent(row: Row): PaymentEvent {
  return paymentEventSchema.parse({
    id: row.id,
    invoiceId: row.invoice_id,
    providerId: row.provider_id,
    providerEventId: row.provider_event_id,
    outcome: row.outcome,
    amountMinor: minor(row.amount_minor),
    currency: row.currency,
    verifiedAt: instant(row.verified_at),
    recordedBy: row.recorded_by,
  })
}

export class PostgresBillingRepository implements BillingRepository {
  constructor(
    private readonly pool: Pool,
    private readonly key: string = '',
  ) {}

  static async create(pool: Pool, key: string): Promise<PostgresBillingRepository> {
    if (key.length < 32)
      throw new Error('Stable billing fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_billing'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Billing requires a non-owner runtime')

    const expected = await billingMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_billing.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Billing schema migrations mismatch')

    return new PostgresBillingRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: BillingPrincipal,
    work: (client: PoolClient, access: BillingAccess) => Promise<T>,
  ): Promise<T> {
    const client = await this.pool.connect()
    try {
      const userRes = await client.query<{ usename: string; usesuper: boolean }>(
        'SELECT usename, usesuper FROM pg_user WHERE usename = current_user',
      )
      if (userRes.rows[0]?.usesuper) {
        throw new BillingError('forbidden', 'Superuser connections are prohibited')
      }

      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])

      const memRes = await client.query<Row>(
        'SELECT * FROM archava_billing.lock_membership($1, $2)',
        [tenant, principal.id],
      )
      const mem = memRes.rows[0]
      if (!mem || !mem.active) {
        throw new BillingError('forbidden', 'Principal is inactive or not enrolled in tenant')
      }

      const access: BillingAccess = {
        ownerId: principal.id,
        permissions: (mem.permissions as BillingPermission[]) ?? [],
        scope: (mem.scope as 'all' | 'assigned') ?? 'assigned',
      }

      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK').catch(() => {})
      throw error
    } finally {
      client.release()
    }
  }

  async execute(
    tenant: string,
    principal: BillingPrincipal,
    mutation: BillingMutation,
  ): Promise<BillingReceipt> {
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = this.key
        ? createHmac('sha256', this.key).update(JSON.stringify(mutation)).digest('hex')
        : createHash('sha256').update(JSON.stringify(mutation)).digest('hex')

      const existingCmd = (
        await client.query<Row>(
          'SELECT receipt, fingerprint FROM archava_billing.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) {
          throw new BillingError('conflict', 'Command payload fingerprint mismatch on replay')
        }
        if (existingCmd.receipt) {
          return billingReceiptSchema.parse({
            ...existingCmd.receipt,
            idempotencyReplay: true,
          })
        }
      } else {
        await client.query(
          'INSERT INTO archava_billing.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
          [tenant, mutation.commandId, access.ownerId, fingerprint],
        )
      }

      const audit = async (
        operation: string,
        invoiceId: string | null,
        revision: number,
        reason: string,
      ) => {
        await client.query(
          'INSERT INTO archava_billing.audit(tenant_id, request_id, actor_id, operation, invoice_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
          [tenant, mutation.commandId, access.ownerId, operation, invoiceId, revision, reason],
        )
      }

      const loadInvoice = async (invoiceId: string) => {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, invoiceId],
          )
        ).rows[0]
        if (!row) throw new BillingError('not_found', 'Invoice not found')
        return row
      }

      let receipt: BillingReceipt

      switch (mutation.type) {
        case 'create_invoice': {
          grant(access, 'billing.write')
          const invoiceId = randomUUID()
          const grossMinor = mutation.lines.reduce(
            (sum, line) => sum + line.quantity * line.unitMinor,
            0,
          )
          const invoiceRow = (
            await client.query<Row>(
              `INSERT INTO archava_billing.invoices(
                tenant_id, id, invoice_code, customer_ref, currency, status, gross_minor, outstanding_minor, consumer_ref, owner_id
              ) VALUES($1, $2, $3, $4, $5, 'draft', $6, $6, $7, $8)
              RETURNING *`,
              [
                tenant,
                invoiceId,
                mutation.invoiceCode,
                mutation.customerRef,
                mutation.currency,
                grossMinor,
                mutation.consumerRef,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!invoiceRow) throw new BillingError('not_found', 'Failed to create invoice')

          let firstLineId: string | null = null
          for (const line of mutation.lines) {
            const lineId = randomUUID()
            if (firstLineId === null) firstLineId = lineId
            const lineRow = (
              await client.query<Row>(
                `INSERT INTO archava_billing.invoice_lines(
                  tenant_id, id, invoice_id, description, quantity, unit_minor, line_total_minor, recorded_by
                ) VALUES($1, $2, $3, $4, $5, $6, $7, $8)
                RETURNING *`,
                [
                  tenant,
                  lineId,
                  invoiceId,
                  line.description,
                  line.quantity,
                  line.unitMinor,
                  line.quantity * line.unitMinor,
                  access.ownerId,
                ],
              )
            ).rows[0]
            if (!lineRow) throw new BillingError('not_found', 'Failed to record invoice line')
          }

          await audit(
            'create_invoice',
            invoiceId,
            1,
            `Created invoice ${mutation.invoiceCode} with ${mutation.lines.length} priced line(s)`,
          )

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'create_invoice',
            invoiceId,
            lineId: firstLineId,
            status: 'draft',
            outstandingMinor: grossMinor,
            revision: 1,
            idempotencyReplay: false,
            executedAt: instant(invoiceRow.created_at),
          })
          break
        }

        case 'issue_invoice': {
          grant(access, 'billing.settle')
          const invoice = await loadInvoice(mutation.invoiceId)

          if (invoice.revision !== mutation.expectedRevision) {
            throw new BillingError('conflict', 'Optimistic revision conflict on invoice')
          }
          if (invoice.status !== 'draft')
            throw new BillingError(
              'invalid_transition',
              `Only a draft invoice can be issued; this one is ${String(invoice.status)}`,
            )

          const nextRev = Number(invoice.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_billing.invoices
               SET status='issued', issued_at=clock_timestamp(), revision=$1
               WHERE tenant_id=$2 AND id=$3
               RETURNING *`,
              [nextRev, tenant, mutation.invoiceId],
            )
          ).rows[0]
          if (!updated) throw new BillingError('not_found', 'Invoice update failed')

          await audit('issue_invoice', mutation.invoiceId, nextRev, mutation.reason)

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'issue_invoice',
            invoiceId: mutation.invoiceId,
            status: 'issued',
            outstandingMinor: minor(updated.outstanding_minor),
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.issued_at),
          })
          break
        }

        case 'append_correction': {
          grant(access, 'billing.settle')
          const invoice = await loadInvoice(mutation.invoiceId)

          if (invoice.status !== 'issued')
            throw new BillingError(
              'invalid_transition',
              `Corrections apply to an issued invoice; this one is ${String(invoice.status)}`,
            )
          if (mutation.reversesId) {
            const target = (
              await client.query<Row>(
                'SELECT * FROM archava_billing.invoice_corrections WHERE tenant_id=$1 AND id=$2',
                [tenant, mutation.reversesId],
              )
            ).rows[0]
            if (!target)
              throw new BillingError('not_found', 'Correction being reversed was not found')
            if (target.invoice_id !== mutation.invoiceId)
              throw new BillingError(
                'conflict',
                'Correction being reversed belongs to a different invoice',
              )
          }

          const nextCorrection = minor(invoice.correction_minor) + mutation.amountMinor
          const nextOutstanding =
            minor(invoice.gross_minor) + nextCorrection - minor(invoice.settled_minor)
          const nextRev = Number(invoice.revision) + 1

          await client.query(
            'UPDATE archava_billing.invoices SET correction_minor=$1, outstanding_minor=$2, revision=$3 WHERE tenant_id=$4 AND id=$5',
            [nextCorrection, nextOutstanding, nextRev, tenant, mutation.invoiceId],
          )

          const correctionId = randomUUID()
          const correctionRow = (
            await client.query<Row>(
              `INSERT INTO archava_billing.invoice_corrections(
                tenant_id, id, invoice_id, amount_minor, reason, reverses_id, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7)
              RETURNING *`,
              [
                tenant,
                correctionId,
                mutation.invoiceId,
                mutation.amountMinor,
                mutation.reason,
                mutation.reversesId ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!correctionRow)
            throw new BillingError('not_found', 'Failed to record invoice correction')

          await audit(
            'append_correction',
            mutation.invoiceId,
            nextRev,
            `Appended correction of ${mutation.amountMinor} minor units: ${mutation.reason}`,
          )

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'append_correction',
            invoiceId: mutation.invoiceId,
            correctionId,
            status: 'issued',
            outstandingMinor: nextOutstanding,
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(correctionRow.created_at),
          })
          break
        }

        case 'void_invoice': {
          grant(access, 'billing.settle')
          const invoice = await loadInvoice(mutation.invoiceId)

          if (invoice.revision !== mutation.expectedRevision) {
            throw new BillingError('conflict', 'Optimistic revision conflict on invoice')
          }
          if (!ALLOWED_TRANSITIONS[invoice.status as InvoiceStatus]?.includes('void'))
            throw new BillingError(
              'invalid_transition',
              `A ${String(invoice.status)} invoice cannot be voided`,
            )
          if (minor(invoice.settled_minor) !== 0)
            throw new BillingError(
              'unbalanced_settlement',
              'An invoice with recorded settlement must be refunded, not voided',
            )

          const nextRev = Number(invoice.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_billing.invoices
               SET status='void', voided_at=clock_timestamp(), revision=$1
               WHERE tenant_id=$2 AND id=$3
               RETURNING *`,
              [nextRev, tenant, mutation.invoiceId],
            )
          ).rows[0]
          if (!updated) throw new BillingError('not_found', 'Invoice update failed')

          await audit('void_invoice', mutation.invoiceId, nextRev, mutation.reason)

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'void_invoice',
            invoiceId: mutation.invoiceId,
            status: 'void',
            outstandingMinor: minor(updated.outstanding_minor),
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.voided_at),
          })
          break
        }

        case 'record_payment_attempt': {
          grant(access, 'billing.write')
          const invoice = await loadInvoice(mutation.invoiceId)

          if (invoice.status === 'void')
            throw new BillingError(
              'invalid_transition',
              'A voided invoice cannot take a payment attempt',
            )

          const attemptId = randomUUID()
          const attemptRow = (
            await client.query<Row>(
              `INSERT INTO archava_billing.payment_attempts(
                tenant_id, id, invoice_id, amount_minor, currency, provider_ref, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7)
              RETURNING *`,
              [
                tenant,
                attemptId,
                mutation.invoiceId,
                mutation.amountMinor,
                invoice.currency,
                mutation.providerRef ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!attemptRow) throw new BillingError('not_found', 'Failed to record payment attempt')

          await audit(
            'record_payment_attempt',
            mutation.invoiceId,
            Number(invoice.revision),
            `Recorded payment attempt of ${mutation.amountMinor} minor units (no settlement effect)`,
          )

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_payment_attempt',
            invoiceId: mutation.invoiceId,
            attemptId,
            status: 'attempted',
            outstandingMinor: minor(invoice.outstanding_minor),
            revision: Number(invoice.revision),
            idempotencyReplay: false,
            executedAt: instant(attemptRow.created_at),
          })
          break
        }

        case 'record_provider_event': {
          grant(access, 'billing.write')
          const invoice = await loadInvoice(mutation.invoiceId)

          if (invoice.currency !== mutation.currency)
            throw new BillingError(
              'unbalanced_settlement',
              'Provider event currency does not match the invoiced currency',
            )

          const settledMinor = minor(invoice.settled_minor)
          const grossMinor = minor(invoice.gross_minor)
          const correctionMinor = minor(invoice.correction_minor)
          const nextRev = Number(invoice.revision) + 1

          let nextSettled = settledMinor
          let nextStatus = invoice.status as InvoiceStatus
          let statusLabel = String(invoice.status)

          if (mutation.outcome === 'succeeded') {
            if (invoice.status !== 'issued')
              throw new BillingError(
                'invalid_transition',
                `A ${String(invoice.status)} invoice cannot accept a successful settlement`,
              )
            nextSettled = settledMinor + mutation.amountMinor
            const nextOutstanding = grossMinor + correctionMinor - nextSettled
            if (nextOutstanding < 0)
              throw new BillingError(
                'unbalanced_settlement',
                'Settlement exceeds the outstanding balance',
              )
            if (nextOutstanding === 0) {
              nextStatus = 'settled'
              statusLabel = 'settled'
            }
          } else if (mutation.outcome === 'refunded') {
            if (settledMinor < mutation.amountMinor)
              throw new BillingError(
                'unbalanced_settlement',
                'Refund exceeds the amount already settled',
              )
            nextSettled = settledMinor - mutation.amountMinor
            if (invoice.status === 'settled') {
              nextStatus = 'issued'
              statusLabel = 'issued'
            }
          } else {
            statusLabel = 'failed'
          }

          const outstanding = grossMinor + correctionMinor - nextSettled
          // A failed notification moves no money and no state, so it never
          // touches the invoice row — not even its revision. Everything that
          // makes the balance change is confined to the two outcomes below.
          const movesBalance = mutation.outcome !== 'failed'

          if (movesBalance) {
            const updated = (
              await client.query<Row>(
                `UPDATE archava_billing.invoices
                 SET settled_minor=$1, outstanding_minor=$2, status=$3, revision=$4,
                     settled_at=CASE WHEN $3='settled' THEN clock_timestamp() ELSE settled_at END
                 WHERE tenant_id=$5 AND id=$6
                 RETURNING *`,
                [nextSettled, outstanding, nextStatus, nextRev, tenant, mutation.invoiceId],
              )
            ).rows[0]
            if (!updated) throw new BillingError('not_found', 'Invoice update failed')
          }

          const eventId = randomUUID()
          const eventRow = (
            await client.query<Row>(
              `INSERT INTO archava_billing.payment_events(
                tenant_id, id, invoice_id, provider_id, provider_event_id, outcome, amount_minor, currency, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                eventId,
                mutation.invoiceId,
                mutation.providerId,
                mutation.providerEventId,
                mutation.outcome,
                mutation.amountMinor,
                mutation.currency,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!eventRow) throw new BillingError('not_found', 'Failed to record provider event')

          await audit(
            'record_provider_event',
            mutation.invoiceId,
            movesBalance ? nextRev : Number(invoice.revision),
            `Provider ${mutation.providerId} reported ${mutation.outcome} for ${mutation.amountMinor} minor units [${mutation.providerEventId}]`,
          )

          receipt = billingReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_provider_event',
            invoiceId: mutation.invoiceId,
            eventId,
            status: statusLabel,
            outstandingMinor: outstanding,
            revision: movesBalance ? nextRev : Number(invoice.revision),
            idempotencyReplay: false,
            executedAt: instant(eventRow.verified_at),
          })
          break
        }
      }

      await client.query(
        'UPDATE archava_billing.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3',
        [receipt, tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: BillingPrincipal,
    commandId: string,
  ): Promise<BillingCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_billing.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'recorded',
        receipt: billingReceiptSchema.parse(row.receipt),
      }
    })
  }

  async getAccess(tenant: string, principal: BillingPrincipal): Promise<BillingAccess> {
    return this.transaction(tenant, principal, (_client, access) =>
      Promise.resolve(billingAccessSchema.parse(access)),
    )
  }

  async listInvoices(
    tenant: string,
    principal: BillingPrincipal,
    query: InvoicePageQuery,
  ): Promise<readonly Invoice[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.customerRef) {
        params.push(query.customerRef)
        conditions.push(`customer_ref = $${params.length}`)
      }

      params.push(query.limit ?? 50)
      const limitParam = `$${params.length}`
      params.push(query.offset ?? 0)
      const offsetParam = `$${params.length}`

      const res = await client.query<Row>(
        `SELECT * FROM archava_billing.invoices
         WHERE ${conditions.join(' AND ')}
         ORDER BY created_at ASC, id ASC
         LIMIT ${limitParam} OFFSET ${offsetParam}`,
        params,
      )

      return res.rows.map((row) => mapInvoice(row, tenant))
    })
  }

  async getInvoiceDetail(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<InvoiceDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const invoiceRow = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2',
          [tenant, invoiceId],
        )
      ).rows[0]
      if (!invoiceRow) return null

      const lineRows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.invoice_lines WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows

      const correctionRows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.invoice_corrections WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY created_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows

      const attemptRows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.payment_attempts WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY created_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows

      const eventRows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.payment_events WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY verified_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows

      return invoiceDetailSchema.parse({
        invoice: mapInvoice(invoiceRow, tenant),
        lines: lineRows.map((row) => mapLine(row)),
        corrections: correctionRows.map((row) => mapCorrection(row)),
        attempts: attemptRows.map((row) => mapAttempt(row)),
        events: eventRows.map((row) => mapEvent(row)),
      })
    })
  }

  async listLines(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceLine[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.invoice_lines WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows
      return rows.map((row) => mapLine(row))
    })
  }

  async listCorrections(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceCorrection[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.invoice_corrections WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY created_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows
      return rows.map((row) => mapCorrection(row))
    })
  }

  async listAttempts(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentAttempt[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.payment_attempts WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY created_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows
      return rows.map((row) => mapAttempt(row))
    })
  }

  async listEvents(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentEvent[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'billing.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_billing.payment_events WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY verified_at ASC, id ASC',
          [tenant, invoiceId],
        )
      ).rows
      return rows.map((row) => mapEvent(row))
    })
  }
}
