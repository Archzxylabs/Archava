import { createHash, createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  FieldServicesError,
  JOB_ACTIONS,
  JOB_TRANSITIONS,
  fieldReceiptSchema,
  fieldCommandStatusSchema,
  fieldJobSchema,
  fieldJobDetailSchema,
  fieldAccessSchema,
  fieldCommandSchema,
  visitRecordSchema,
  workLineSchema,
  signoffRecordSchema,
  fieldAuditEntrySchema,
  type FieldCommandStatus,
  type FieldPrincipal,
  type FieldRepository,
  type FieldReceipt,
  type FieldAccess,
  type FieldJob,
  type FieldJobDetail,
  type FieldJobPageQuery,
  type FieldAuditEntry,
  type JobAction,
  type JobStatus,
  type VisitRecord,
  type WorkLine,
  type SignoffRecord,
  type FieldCommand,
} from '@archava/field-services'
import { fieldMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

function instant(d: unknown): string {
  if (d instanceof Date) return d.toISOString()
  if (typeof d === 'string') return new Date(d).toISOString()
  return new Date().toISOString()
}

function nullable<T>(v: unknown): T | null {
  return v === null || v === undefined ? null : (v as T)
}

function grant(access: FieldAccess, permission: string): void {
  if (!(access.permissions as readonly string[]).includes(permission)) {
    throw new FieldServicesError('forbidden', `Missing required permission: ${permission}`)
  }
}

const ALLOWED_TRANSITIONS = JOB_TRANSITIONS

function mapJob(row: Row): FieldJob {
  return fieldJobSchema.parse({
    jobId: row.job_id,
    customerRef: row.customer_ref,
    siteRef: row.site_ref,
    summary: row.summary,
    priority: row.priority,
    status: row.status,
    revision: Number(row.revision),
    appointmentRef: row.appointment_ref,
    assetRef: nullable<string>(row.asset_ref),
    assignedPrincipal: nullable<string>(row.assigned_principal),
    laborHours: Number(row.labor_hours),
    partsPieces: Number(row.parts_pieces),
    createdBy: row.created_by,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
}

function mapVisit(row: Row): VisitRecord {
  return visitRecordSchema.parse({
    visitId: row.visit_id,
    jobId: row.job_id,
    outcome: row.outcome,
    note: row.note,
    technicianRef: row.technician_ref,
    visitedAt: instant(row.visited_at),
  })
}

function mapLine(row: Row): WorkLine {
  return workLineSchema.parse({
    lineId: row.line_id,
    kind: row.kind,
    description: row.description,
    quantity: Number(row.quantity),
    unit: row.unit,
    partRef: nullable<string>(row.part_ref),
  })
}

function mapSignoff(row: Row): SignoffRecord {
  return signoffRecordSchema.parse({
    signoffId: row.signoff_id,
    jobId: row.job_id,
    signerRef: row.signer_ref,
    accepted: row.accepted,
    comment: nullable<string>(row.comment),
    signedAt: instant(row.signed_at),
  })
}

export class PostgresFieldRepository implements FieldRepository {
  constructor(
    private readonly pool: Pool,
    private readonly key: string = '',
  ) {}

  static async create(pool: Pool, key: string): Promise<PostgresFieldRepository> {
    if (key.length < 32)
      throw new Error('Stable field fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_field'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Field services requires a non-owner runtime')

    const expected = await fieldMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_field.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Field services schema migrations mismatch')

    return new PostgresFieldRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: FieldPrincipal,
    work: (client: PoolClient, access: FieldAccess) => Promise<T>,
  ): Promise<T> {
    const client = await this.pool.connect()
    try {
      const userRes = await client.query<{ usename: string; usesuper: boolean }>(
        'SELECT usename, usesuper FROM pg_user WHERE usename = current_user',
      )
      if (userRes.rows[0]?.usesuper) {
        throw new FieldServicesError('forbidden', 'Superuser connections are prohibited')
      }

      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])

      const memRes = await client.query<Row>(
        'SELECT * FROM archava_field.lock_membership($1, $2)',
        [tenant, principal.id],
      )
      const mem = memRes.rows[0]
      if (!mem || !mem.active) {
        throw new FieldServicesError('forbidden', 'Principal is inactive or not enrolled in tenant')
      }

      const access = fieldAccessSchema.parse({
        ownerId: principal.id,
        permissions: (mem.permissions as string[]) ?? [],
        scope: (mem.scope as 'all' | 'assigned') ?? 'assigned',
      })

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
    principal: FieldPrincipal,
    command: FieldCommand,
  ): Promise<FieldReceipt> {
    // A service principal acts on a human's behalf and cannot be the actor of
    // record for a mutation: whoever is liable for the dispatch must be
    // identifiable, so automation may read but never writes here.
    if (principal.kind === 'service') {
      throw new FieldServicesError(
        'forbidden',
        'Service principals cannot record field mutations directly',
      )
    }
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = this.key
        ? createHmac('sha256', this.key).update(JSON.stringify(command)).digest('hex')
        : createHash('sha256').update(JSON.stringify(command)).digest('hex')

      // The command envelope is the trust boundary: an action the union does not
      // declare must fail as a bounded domain error, not reach the switch below.
      let guarded: FieldCommand
      try {
        guarded = fieldCommandSchema.parse(command)
      } catch {
        throw new FieldServicesError('validation_failed', 'Field command envelope is not valid')
      }

      const existingCmd = (
        await client.query<Row>(
          'SELECT command_id, action, job_id, revision, fingerprint, receipt FROM archava_field.commands WHERE tenant_id=$1 AND command_id=$2',
          [tenant, command.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) {
          throw new FieldServicesError('conflict', 'Command payload fingerprint mismatch on replay')
        }
        // A recorded command is final: a retry replays the recorded receipt.
        if (!existingCmd.receipt) {
          throw new FieldServicesError('unknown_effect', 'Command effect is not yet known')
        }
        return fieldReceiptSchema.parse({ ...existingCmd.receipt })
      }

      const audit = async (action: JobAction, jobId: string, actorRef: string) => {
        await client.query(
          'INSERT INTO archava_field.audit(entry_id, job_id, tenant_id, action, actor_ref, command_id) VALUES($1,$2,$3,$4,$5,$6)',
          [randomUUID(), jobId, tenant, action, actorRef, command.commandId],
        )
      }

      const loadJob = async (jobId: string) => {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_field.jobs WHERE tenant_id=$1 AND job_id=$2 FOR UPDATE',
            [tenant, jobId],
          )
        ).rows[0]
        if (!row) throw new FieldServicesError('not_found', 'Field job not found')
        return row
      }

      /** Refuses any transition the frozen forward chain does not allow. */
      const moveTo = (job: Row, next: JobStatus) => {
        const current = job.status as JobStatus
        const allowed = ALLOWED_TRANSITIONS[current]
        if (!allowed.includes(next)) {
          throw new FieldServicesError(
            'illegal_transition',
            `Cannot move a ${current} field job to ${next}`,
          )
        }
      }

      let jobId = ''
      let receipt: Pick<FieldReceipt, 'jobId' | 'status' | 'revision'>

      switch (guarded.action) {
        case 'create_job': {
          grant(access, 'field.write')
          jobId = guarded.job.jobId
          const jobRow = (
            await client.query<Row>(
              `INSERT INTO archava_field.jobs(
                job_id, tenant_id, customer_ref, site_ref, summary, priority, status,
                appointment_ref, asset_ref, assigned_principal, created_by
              ) VALUES($1,$2,$3,$4,$5,$6,'requested',$7,$8,NULL,$9)
              RETURNING *`,
              [
                jobId,
                tenant,
                guarded.job.customerRef,
                guarded.job.siteRef,
                guarded.job.summary,
                guarded.job.priority,
                guarded.job.appointmentRef,
                guarded.job.assetRef,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!jobRow) throw new FieldServicesError('not_found', 'Failed to create field job')
          await audit(guarded.action, jobId, access.ownerId)
          receipt = { jobId, status: 'requested', revision: 1 }
          break
        }

        case 'assign_job': {
          grant(access, 'field.dispatch')
          const job = await loadJob(guarded.jobId)
          moveTo(job, 'assigned')
          jobId = guarded.jobId
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs SET status='assigned', assigned_principal=$3,
               revision = revision + 1, updated_at = now()
               WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId, guarded.assignedPrincipal],
            )
          ).rows[0]
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = { jobId, status: 'assigned', revision: Number(updated.revision) }
          break
        }

        case 'record_visit': {
          grant(access, 'field.dispatch')
          const job = await loadJob(guarded.jobId)
          moveTo(job, 'visited')
          jobId = guarded.jobId
          await client.query(
            `INSERT INTO archava_field.visits(
              visit_id, job_id, outcome, note, technician_ref
            ) VALUES($1,$2,$3,$4,$5)`,
            [
              guarded.visit.visitId,
              jobId,
              guarded.visit.outcome,
              guarded.visit.note,
              guarded.visit.technicianRef,
            ],
          )
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs SET status='visited', revision = revision + 1,
               updated_at = now() WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId],
            )
          ).rows[0]
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = { jobId, status: 'visited', revision: Number(updated.revision) }
          break
        }

        case 'append_work_line': {
          grant(access, 'field.write')
          const job = await loadJob(guarded.jobId)
          if (job.status === 'approved' || job.status === 'canceled') {
            throw new FieldServicesError(
              'illegal_transition',
              'A closed field job accepts no further work lines',
            )
          }
          // Labour and parts are claimed, not observed remotely: a technician
          // who was never on site cannot bill the customer for the work.
          if (job.status === 'requested' || job.status === 'assigned') {
            throw new FieldServicesError(
              'illegal_transition',
              'Work evidence requires a recorded site visit',
            )
          }
          jobId = guarded.jobId
          const line = guarded.line
          // Evidence is only meaningful once the technician has been on site, and
          // revising it after a signoff pulls the job back so it is signed again.
          // Both directions are explicit: a fresh line promotes `visited` to
          // `evidenced`, and a line added after signoff demotes `signed_off` back.
          if (job.status === 'visited' || job.status === 'signed_off') {
            moveTo(job, 'evidenced')
          }
          await client.query(
            `INSERT INTO archava_field.work_lines(
              line_id, job_id, kind, description, quantity, unit, part_ref
            ) VALUES($1,$2,$3,$4,$5,$6,$7)`,
            [
              line.lineId,
              jobId,
              line.kind,
              line.description,
              line.quantity,
              line.unit,
              line.partRef,
            ],
          )
          // Hours and pieces accumulate on their own axes, never one bucket.
          const hours = line.kind === 'labour' ? Number(line.quantity) : 0
          const pieces = line.kind === 'part' ? line.quantity : 0
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs
               SET labor_hours = labor_hours + $3, parts_pieces = parts_pieces + $4,
                   status = 'evidenced', revision = revision + 1, updated_at = now()
               WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId, hours, Math.round(pieces)],
            )
          ).rows[0]
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = {
            jobId,
            status: updated.status as JobStatus,
            revision: Number(updated.revision),
          }
          break
        }

        case 'record_signoff': {
          grant(access, 'field.signoff')
          const job = await loadJob(guarded.jobId)
          // A rejected signoff is evidence of refusal, not proof of acceptance:
          // the job stays at (or returns to) `evidenced`, so approval is never
          // reachable off the strength of a rejection alone.
          if (guarded.signoff.accepted) {
            moveTo(job, 'signed_off')
          } else if (job.status === 'signed_off') {
            moveTo(job, 'evidenced')
          }
          jobId = guarded.jobId
          const signoffStatus = guarded.signoff.accepted ? 'signed_off' : 'evidenced'
          await client.query(
            `INSERT INTO archava_field.signoffs(
              signoff_id, job_id, signer_ref, accepted, comment
            ) VALUES($1,$2,$3,$4,$5)`,
            [
              guarded.signoff.signoffId,
              jobId,
              guarded.signoff.signerRef,
              guarded.signoff.accepted,
              guarded.signoff.comment,
            ],
          )
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs SET status=$3, revision = revision + 1,
               updated_at = now() WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId, signoffStatus],
            )
          ).rows[0]
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = { jobId, status: signoffStatus, revision: Number(updated.revision) }
          break
        }

        case 'approve_job': {
          grant(access, 'field.signoff')
          const job = await loadJob(guarded.jobId)
          moveTo(job, 'approved')
          jobId = guarded.jobId
          const accepted = (
            await client.query<{ signoff_id: string }>(
              `SELECT s.signoff_id FROM archava_field.signoffs s
               JOIN archava_field.jobs j ON j.job_id = s.job_id
               WHERE j.tenant_id = $1 AND s.job_id = $2 AND s.accepted
               ORDER BY s.signed_at ASC, s.signoff_id ASC LIMIT 1`,
              [tenant, jobId],
            )
          ).rows[0]
          if (!accepted) {
            throw new FieldServicesError(
              'conflict',
              'Cannot approve a field job without an accepted customer signoff',
            )
          }
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs SET status='approved', revision = revision + 1,
               updated_at = now() WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId],
            )
          ).rows[0]
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = { jobId, status: 'approved', revision: Number(updated.revision) }
          break
        }

        case 'cancel_job': {
          grant(access, 'field.dispatch')
          const job = await loadJob(guarded.jobId)
          moveTo(job, 'canceled')
          jobId = guarded.jobId
          const updated = (
            await client.query<Row>(
              `UPDATE archava_field.jobs SET status='canceled', revision = revision + 1,
               updated_at = now() WHERE tenant_id=$1 AND job_id=$2 RETURNING *`,
              [tenant, jobId],
            )
          ).rows[0]
          await audit(guarded.action, jobId, access.ownerId)
          if (!updated) throw new FieldServicesError('not_found', 'Field job update failed')
          receipt = { jobId, status: 'canceled', revision: Number(updated.revision) }
          break
        }
      }

      const finalReceipt = fieldReceiptSchema.parse({
        commandId: guarded.commandId,
        action: guarded.action,
        jobId,
        status: receipt.status,
        revision: receipt.revision,
        recordedAt: new Date().toISOString(),
      })

      await client.query(
        `INSERT INTO archava_field.commands(
          command_id, job_id, tenant_id, action, fingerprint, payload, receipt, status, revision, created_by
        ) VALUES($1,$2,$3,$4,$5,$6,$7,'accepted',$8,$9)`,
        [
          command.commandId,
          jobId,
          tenant,
          command.action,
          fingerprint,
          JSON.stringify(command),
          JSON.stringify(finalReceipt),
          finalReceipt.revision,
          access.ownerId,
        ],
      )

      return finalReceipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: FieldPrincipal,
    commandId: string,
  ): Promise<FieldCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT command_id, action, job_id, receipt FROM archava_field.commands WHERE tenant_id=$1 AND command_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row) {
        return fieldCommandStatusSchema.parse({
          commandId,
          action: JOB_ACTIONS[0],
          jobId: 'unknown',
          status: 'unknown',
          revision: null,
          recordedAt: null,
        })
      }
      const recorded = fieldReceiptSchema.parse(row.receipt)
      return fieldCommandStatusSchema.parse({
        commandId: row.command_id,
        action: row.action,
        jobId: row.job_id,
        status: 'accepted',
        revision: recorded.revision,
        recordedAt: recorded.recordedAt,
      })
    })
  }

  async getAccess(tenant: string, principal: FieldPrincipal): Promise<FieldAccess> {
    return this.transaction(tenant, principal, (_client, access) =>
      Promise.resolve(fieldAccessSchema.parse(access)),
    )
  }

  async listJobs(
    tenant: string,
    principal: FieldPrincipal,
    query: FieldJobPageQuery,
  ): Promise<readonly FieldJob[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'field.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      // An assigned-scope principal sees only the dispatch that is theirs.
      if (access.scope === 'assigned') {
        params.push(access.ownerId)
        conditions.push(
          `(assigned_principal = $${params.length} OR created_by = $${params.length})`,
        )
      }
      if (query.cursor) {
        params.push(query.cursor)
        conditions.push(`job_id > $${params.length}`)
      }

      params.push(query.limit ?? 50)
      const limitParam = `$${params.length}`
      const res = await client.query<Row>(
        `SELECT * FROM archava_field.jobs
         WHERE ${conditions.join(' AND ')}
         ORDER BY job_id ASC
         LIMIT ${limitParam}`,
        params,
      )
      return res.rows.map((row) => mapJob(row))
    })
  }

  async getJobDetail(
    tenant: string,
    principal: FieldPrincipal,
    jobId: string,
  ): Promise<FieldJobDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'field.read')
      const jobRow = (
        await client.query<Row>(
          'SELECT * FROM archava_field.jobs WHERE tenant_id=$1 AND job_id=$2',
          [tenant, jobId],
        )
      ).rows[0]
      if (!jobRow) return null

      const visitRows = (
        await client.query<Row>(
          'SELECT * FROM archava_field.visits WHERE job_id=$1 ORDER BY visited_at ASC, visit_id ASC',
          [jobId],
        )
      ).rows
      const lineRows = (
        await client.query<Row>(
          'SELECT * FROM archava_field.work_lines WHERE job_id=$1 ORDER BY recorded_at ASC, line_id ASC',
          [jobId],
        )
      ).rows
      const signoffRows = (
        await client.query<Row>(
          'SELECT * FROM archava_field.signoffs WHERE job_id=$1 ORDER BY signed_at ASC, signoff_id ASC',
          [jobId],
        )
      ).rows

      const lastSignoff = signoffRows[signoffRows.length - 1]

      return fieldJobDetailSchema.parse({
        job: mapJob(jobRow),
        visits: visitRows.map((row) => mapVisit(row)),
        workLines: lineRows.map((row) => mapLine(row)),
        signoff: lastSignoff ? mapSignoff(lastSignoff) : null,
      })
    })
  }

  async listAudit(
    tenant: string,
    principal: FieldPrincipal,
    jobId: string,
  ): Promise<readonly FieldAuditEntry[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'audit.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_field.audit WHERE tenant_id=$1 AND job_id=$2 ORDER BY at ASC, entry_id ASC',
          [tenant, jobId],
        )
      ).rows
      return rows.map((row) =>
        fieldAuditEntrySchema.parse({
          entryId: row.entry_id,
          jobId: row.job_id,
          action: row.action,
          actorRef: row.actor_ref,
          commandId: row.command_id,
          at: instant(row.at),
        }),
      )
    })
  }
}
