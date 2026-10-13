import { createHash, createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  FulfillmentError,
  fulfillmentReceiptSchema,
  fulfillmentAccessSchema,
  jobSchema,
  jobLineSchema,
  findingSchema,
  custodyEventSchema,
  jobDetailSchema,
  CUSTODY_STAGE,
  EXCEPTION_DISPOSITIONS,
  type FulfillmentRepository,
  type FulfillmentPrincipal,
  type FulfillmentMutation,
  type FulfillmentReceipt,
  type FulfillmentAccess,
  type FulfillmentCommandStatus,
  type FulfillmentPermission,
  type JobPageQuery,
  type Job,
  type JobStatus,
  type JobDetail,
  type JobLine,
  type Finding,
  type CustodyRecord,
} from '@archava/fulfillment'
import { fulfillmentMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

function instant(d: unknown): string {
  if (d instanceof Date) return d.toISOString()
  if (typeof d === 'string') return new Date(d).toISOString()
  return new Date().toISOString()
}

function nullable<T>(v: unknown): T | null {
  return v === null || v === undefined ? null : (v as T)
}

function grant(access: FulfillmentAccess, permission: FulfillmentPermission): void {
  if (!access.permissions.includes(permission)) {
    throw new FulfillmentError('forbidden', `Missing required permission: ${permission}`)
  }
}

const ALLOWED_TRANSITIONS: Readonly<Record<JobStatus, readonly JobStatus[]>> = Object.freeze({
  draft: ['released', 'canceled'],
  released: ['in_progress', 'canceled'],
  in_progress: ['completed', 'canceled'],
  completed: [],
  canceled: [],
})

const DISPATCH_STAGE_EVENTS: ReadonlySet<string> = new Set(['dispatched', 'delivered', 'returned'])

function isException(disposition: string): boolean {
  return (EXCEPTION_DISPOSITIONS as readonly string[]).includes(disposition)
}

function mapJob(row: Row, tenant: string): Job {
  return jobSchema.parse({
    id: row.id,
    tenantId: tenant,
    jobCode: row.job_code,
    kind: row.kind,
    title: row.title,
    ownerRef: nullable<string>(row.owner_ref),
    status: row.status,
    consumedCount: row.consumed_count,
    producedCount: row.produced_count,
    scrapCount: row.scrap_count,
    unreconciledCount: row.unreconciled_count,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
}

function mapLine(row: Row, tenant: string): JobLine {
  return jobLineSchema.parse({
    id: row.id,
    tenantId: tenant,
    jobId: row.job_id,
    direction: row.direction,
    itemRef: row.item_ref,
    quantity: row.quantity,
    disposition: row.disposition,
    evidenceRef: nullable<string>(row.evidence_ref),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

function mapFinding(row: Row, tenant: string): Finding {
  return findingSchema.parse({
    id: row.id,
    tenantId: tenant,
    jobId: row.job_id,
    category: row.category,
    disposition: row.disposition,
    quantity: nullable<number>(row.quantity),
    reason: row.reason,
    evidenceRef: nullable<string>(row.evidence_ref),
    reconcilesId: nullable<string>(row.reconciles_id),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

function mapCustody(row: Row, tenant: string): CustodyRecord {
  return custodyEventSchema.parse({
    id: row.id,
    tenantId: tenant,
    jobId: row.job_id,
    event: row.event,
    fromRef: nullable<string>(row.from_ref),
    toRef: nullable<string>(row.to_ref),
    evidenceRef: nullable<string>(row.evidence_ref),
    note: nullable<string>(row.note),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

export class PostgresFulfillmentRepository implements FulfillmentRepository {
  constructor(
    private readonly pool: Pool,
    private readonly key: string = '',
  ) {}

  static async create(pool: Pool, key: string): Promise<PostgresFulfillmentRepository> {
    if (key.length < 32)
      throw new Error('Stable fulfillment fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_fulfillment'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Fulfillment requires a non-owner runtime')

    const expected = await fulfillmentMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_fulfillment.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Fulfillment schema migrations mismatch')

    return new PostgresFulfillmentRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: FulfillmentPrincipal,
    work: (client: PoolClient, access: FulfillmentAccess) => Promise<T>,
  ): Promise<T> {
    const client = await this.pool.connect()
    try {
      const userRes = await client.query<{ usename: string; usesuper: boolean }>(
        'SELECT usename, usesuper FROM pg_user WHERE usename = current_user',
      )
      if (userRes.rows[0]?.usesuper) {
        throw new FulfillmentError('forbidden', 'Superuser connections are prohibited')
      }

      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])

      const memRes = await client.query<Row>(
        'SELECT * FROM archava_fulfillment.lock_membership($1, $2)',
        [tenant, principal.id],
      )
      const mem = memRes.rows[0]
      if (!mem || !mem.active) {
        throw new FulfillmentError('forbidden', 'Principal is inactive or not enrolled in tenant')
      }

      const access: FulfillmentAccess = {
        ownerId: principal.id,
        permissions: (mem.permissions as FulfillmentPermission[]) ?? [],
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
    principal: FulfillmentPrincipal,
    mutation: FulfillmentMutation,
  ): Promise<FulfillmentReceipt> {
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = this.key
        ? createHmac('sha256', this.key).update(JSON.stringify(mutation)).digest('hex')
        : createHash('sha256').update(JSON.stringify(mutation)).digest('hex')

      const existingCmd = (
        await client.query<Row>(
          'SELECT receipt, fingerprint FROM archava_fulfillment.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) {
          throw new FulfillmentError('conflict', 'Command payload fingerprint mismatch on replay')
        }
        if (existingCmd.receipt) {
          return fulfillmentReceiptSchema.parse({
            ...existingCmd.receipt,
            idempotencyReplay: true,
          })
        }
      } else {
        await client.query(
          'INSERT INTO archava_fulfillment.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
          [tenant, mutation.commandId, access.ownerId, fingerprint],
        )
      }

      const audit = async (operation: string, jobId: string, revision: number, reason: string) => {
        await client.query(
          'INSERT INTO archava_fulfillment.audit(tenant_id, request_id, actor_id, operation, job_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
          [tenant, mutation.commandId, access.ownerId, operation, jobId, revision, reason],
        )
      }

      const loadJob = async (jobId: string) => {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_fulfillment.jobs WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, jobId],
          )
        ).rows[0]
        if (!row) throw new FulfillmentError('not_found', 'Job not found')
        return row
      }

      const assertOpen = (job: Row) => {
        if (job.status === 'completed' || job.status === 'canceled') {
          throw new FulfillmentError(
            'invalid_transition',
            'Evidence cannot be recorded on a completed or canceled job',
          )
        }
      }

      let receipt: FulfillmentReceipt

      switch (mutation.type) {
        case 'create_job': {
          grant(access, 'fulfillment.write')
          const jobId = randomUUID()
          const jobRow = (
            await client.query<Row>(
              `INSERT INTO archava_fulfillment.jobs(
                tenant_id, id, job_code, kind, title, owner_ref, status, owner_id
              ) VALUES($1, $2, $3, $4, $5, $6, 'draft', $7)
              RETURNING *`,
              [
                tenant,
                jobId,
                mutation.jobCode,
                mutation.kind,
                mutation.title,
                mutation.ownerRef ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!jobRow) throw new FulfillmentError('not_found', 'Failed to create job')

          await audit('create_job', jobId, 1, `Created ${mutation.kind} job ${mutation.jobCode}`)

          receipt = fulfillmentReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'create_job',
            jobId,
            status: 'draft',
            revision: 1,
            idempotencyReplay: false,
            executedAt: instant(jobRow.created_at),
          })
          break
        }

        case 'record_line': {
          grant(access, 'fulfillment.write')
          const job = await loadJob(mutation.jobId)
          assertOpen(job)
          if (job.kind === 'production') grant(access, 'fulfillment.produce')

          const quantity = mutation.quantity
          const nextConsumed =
            Number(job.consumed_count) +
            (mutation.disposition === 'normal' && mutation.direction === 'consume' ? quantity : 0)
          const nextProduced =
            Number(job.produced_count) +
            (mutation.disposition === 'normal' && mutation.direction === 'produce' ? quantity : 0)
          const nextScrap =
            Number(job.scrap_count) + (mutation.disposition === 'scrap' ? quantity : 0)

          await client.query(
            `UPDATE archava_fulfillment.jobs
             SET consumed_count=$1, produced_count=$2, scrap_count=$3, updated_at=clock_timestamp()
             WHERE tenant_id=$4 AND id=$5`,
            [nextConsumed, nextProduced, nextScrap, tenant, mutation.jobId],
          )

          const lineId = randomUUID()
          const lineRow = (
            await client.query<Row>(
              `INSERT INTO archava_fulfillment.job_lines(
                tenant_id, id, job_id, direction, item_ref, quantity, disposition, evidence_ref, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                lineId,
                mutation.jobId,
                mutation.direction,
                mutation.itemRef,
                quantity,
                mutation.disposition,
                mutation.evidenceRef ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!lineRow) throw new FulfillmentError('not_found', 'Failed to record job line')

          await audit(
            'record_line',
            mutation.jobId,
            Number(job.revision),
            `Recorded ${mutation.direction} line ${mutation.itemRef} x${quantity} (${mutation.disposition})`,
          )

          receipt = fulfillmentReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_line',
            jobId: mutation.jobId,
            lineId,
            status: `${mutation.direction}/${mutation.disposition}`,
            revision: Number(job.revision),
            idempotencyReplay: false,
            executedAt: instant(lineRow.recorded_at),
          })
          break
        }

        case 'record_finding': {
          grant(access, 'fulfillment.write')
          const job = await loadJob(mutation.jobId)
          assertOpen(job)
          if (mutation.category === 'delivery') grant(access, 'fulfillment.dispatch')
          if (mutation.reconcilesId && mutation.disposition !== 'accepted')
            throw new FulfillmentError(
              'bad_request',
              'Only an accepted finding can reconcile an earlier exception',
            )

          let nextUnreconciled = Number(job.unreconciled_count)

          if (isException(mutation.disposition)) {
            nextUnreconciled += 1
          } else if (mutation.disposition === 'accepted' && mutation.reconcilesId) {
            const target = (
              await client.query<Row>(
                'SELECT * FROM archava_fulfillment.findings WHERE tenant_id=$1 AND id=$2',
                [tenant, mutation.reconcilesId],
              )
            ).rows[0]
            if (!target) throw new FulfillmentError('not_found', 'Reconciliation target not found')
            if (target.job_id !== mutation.jobId)
              throw new FulfillmentError(
                'conflict',
                'Reconciliation target belongs to a different job',
              )
            if (!isException(target.disposition as string))
              throw new FulfillmentError(
                'conflict',
                'Only partial, rejected, lost or damaged findings require reconciliation',
              )
            const already = (
              await client.query<Row>(
                'SELECT 1 FROM archava_fulfillment.findings WHERE tenant_id=$1 AND reconciles_id=$2',
                [tenant, mutation.reconcilesId],
              )
            ).rows[0]
            if (already)
              throw new FulfillmentError('conflict', 'Finding has already been reconciled')
            nextUnreconciled = Math.max(0, nextUnreconciled - 1)
          }

          await client.query(
            'UPDATE archava_fulfillment.jobs SET unreconciled_count=$1, updated_at=clock_timestamp() WHERE tenant_id=$2 AND id=$3',
            [nextUnreconciled, tenant, mutation.jobId],
          )

          const findingId = randomUUID()
          const findingRow = (
            await client.query<Row>(
              `INSERT INTO archava_fulfillment.findings(
                tenant_id, id, job_id, category, disposition, quantity, reason, evidence_ref, reconciles_id, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
              RETURNING *`,
              [
                tenant,
                findingId,
                mutation.jobId,
                mutation.category,
                mutation.disposition,
                mutation.quantity ?? null,
                mutation.reason,
                mutation.evidenceRef ?? null,
                mutation.reconcilesId ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!findingRow) throw new FulfillmentError('not_found', 'Failed to record finding')

          await audit(
            'record_finding',
            mutation.jobId,
            Number(job.revision),
            `Recorded ${mutation.category} finding (${mutation.disposition}): ${mutation.reason}`,
          )

          receipt = fulfillmentReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_finding',
            jobId: mutation.jobId,
            findingId,
            status: mutation.disposition,
            revision: Number(job.revision),
            idempotencyReplay: false,
            executedAt: instant(findingRow.recorded_at),
          })
          break
        }

        case 'record_custody': {
          grant(access, 'fulfillment.write')
          const job = await loadJob(mutation.jobId)
          assertOpen(job)
          if (DISPATCH_STAGE_EVENTS.has(mutation.event)) grant(access, 'fulfillment.dispatch')

          const history = (
            await client.query<{ event: string }>(
              'SELECT event FROM archava_fulfillment.custody_events WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
              [tenant, mutation.jobId],
            )
          ).rows

          if (history.length === 0) {
            if (mutation.event !== 'received')
              throw new FulfillmentError(
                'custody_out_of_order',
                'Custody chain must begin with received',
              )
          } else {
            let maxStage = -1
            let furthest = 'received'
            for (const item of history) {
              const stage = CUSTODY_STAGE[item.event]
              if (stage !== undefined && stage > maxStage) {
                maxStage = stage
                furthest = item.event
              }
            }
            if (mutation.event === 'returned') {
              if (maxStage < (CUSTODY_STAGE.dispatched ?? Number.POSITIVE_INFINITY))
                throw new FulfillmentError(
                  'custody_out_of_order',
                  'Return requires a prior dispatched custody event',
                )
            } else {
              const stage = CUSTODY_STAGE[mutation.event] ?? -1
              if (stage <= maxStage)
                throw new FulfillmentError(
                  'custody_out_of_order',
                  `Custody ${mutation.event} must follow ${furthest}`,
                )
            }
          }

          const custodyId = randomUUID()
          const custodyRow = (
            await client.query<Row>(
              `INSERT INTO archava_fulfillment.custody_events(
                tenant_id, id, job_id, event, from_ref, to_ref, evidence_ref, note, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                custodyId,
                mutation.jobId,
                mutation.event,
                mutation.fromRef ?? null,
                mutation.toRef ?? null,
                mutation.evidenceRef ?? null,
                mutation.note ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!custodyRow) throw new FulfillmentError('not_found', 'Failed to record custody event')

          await audit(
            'record_custody',
            mutation.jobId,
            Number(job.revision),
            `Recorded custody event ${mutation.event}`,
          )

          receipt = fulfillmentReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_custody',
            jobId: mutation.jobId,
            custodyId,
            status: mutation.event,
            revision: Number(job.revision),
            idempotencyReplay: false,
            executedAt: instant(custodyRow.recorded_at),
          })
          break
        }

        case 'change_job_status': {
          grant(access, 'fulfillment.write')
          const job = await loadJob(mutation.jobId)

          if (job.revision !== mutation.expectedRevision) {
            throw new FulfillmentError('conflict', 'Optimistic revision conflict on job')
          }

          const allowed = ALLOWED_TRANSITIONS[job.status as JobStatus] ?? []
          if (!allowed.includes(mutation.targetStatus))
            throw new FulfillmentError(
              'invalid_transition',
              `Job cannot move from ${String(job.status)} to ${mutation.targetStatus}`,
            )

          if (mutation.targetStatus === 'completed' && Number(job.unreconciled_count) > 0)
            throw new FulfillmentError(
              'unreconciled_exceptions',
              'Delivery or inspection findings must be reconciled before completion',
            )

          const nextRev = Number(job.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_fulfillment.jobs
               SET status=$1, revision=$2, updated_at=clock_timestamp()
               WHERE tenant_id=$3 AND id=$4
               RETURNING *`,
              [mutation.targetStatus, nextRev, tenant, mutation.jobId],
            )
          ).rows[0]
          if (!updated) throw new FulfillmentError('not_found', 'Job update failed')

          await audit('change_job_status', mutation.jobId, nextRev, mutation.reason)

          receipt = fulfillmentReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'change_job_status',
            jobId: mutation.jobId,
            status: mutation.targetStatus,
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }
      }

      await client.query(
        'UPDATE archava_fulfillment.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3',
        [receipt, tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: FulfillmentPrincipal,
    commandId: string,
  ): Promise<FulfillmentCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_fulfillment.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'recorded',
        receipt: fulfillmentReceiptSchema.parse(row.receipt),
      }
    })
  }

  async getAccess(tenant: string, principal: FulfillmentPrincipal): Promise<FulfillmentAccess> {
    return this.transaction(tenant, principal, (_client, access) =>
      Promise.resolve(fulfillmentAccessSchema.parse(access)),
    )
  }

  async listJobs(
    tenant: string,
    principal: FulfillmentPrincipal,
    query: JobPageQuery,
  ): Promise<readonly Job[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'fulfillment.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.kind) {
        params.push(query.kind)
        conditions.push(`kind = $${params.length}`)
      }

      params.push(query.limit ?? 50)
      const limitParam = `$${params.length}`
      params.push(query.offset ?? 0)
      const offsetParam = `$${params.length}`

      const res = await client.query<Row>(
        `SELECT * FROM archava_fulfillment.jobs
         WHERE ${conditions.join(' AND ')}
         ORDER BY created_at ASC, id ASC
         LIMIT ${limitParam} OFFSET ${offsetParam}`,
        params,
      )

      return res.rows.map((row) => mapJob(row, tenant))
    })
  }

  async getJobDetail(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<JobDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'fulfillment.read')
      const jobRow = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.jobs WHERE tenant_id=$1 AND id=$2',
          [tenant, jobId],
        )
      ).rows[0]
      if (!jobRow) return null

      const lineRows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.job_lines WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows

      const findingRows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.findings WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows

      const custodyRows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.custody_events WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows

      return jobDetailSchema.parse({
        job: mapJob(jobRow, tenant),
        lines: lineRows.map((row) => mapLine(row, tenant)),
        findings: findingRows.map((row) => mapFinding(row, tenant)),
        custody: custodyRows.map((row) => mapCustody(row, tenant)),
      })
    })
  }

  async listLines(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly JobLine[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'fulfillment.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.job_lines WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows
      return rows.map((row) => mapLine(row, tenant))
    })
  }

  async listFindings(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly Finding[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'fulfillment.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.findings WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows
      return rows.map((row) => mapFinding(row, tenant))
    })
  }

  async listCustody(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly CustodyRecord[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'fulfillment.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_fulfillment.custody_events WHERE tenant_id=$1 AND job_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, jobId],
        )
      ).rows
      return rows.map((row) => mapCustody(row, tenant))
    })
  }
}
