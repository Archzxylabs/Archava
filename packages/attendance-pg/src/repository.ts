import { createHash, createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  AttendanceError,
  attendanceReceiptSchema,
  sessionSchema,
  rosterEntrySchema,
  checkInRecordSchema,
  sessionDetailSchema,
  type AttendanceRepository,
  type AttendancePrincipal,
  type AttendanceMutation,
  type AttendanceReceipt,
  type AttendanceAccess,
  type AttendanceCommandStatus,
  type AttendancePermission,
  type SessionPageQuery,
  type Session,
  type SessionDetail,
  type RosterEntry,
  type CheckInRecord,
} from '@archava/attendance'
import { attendanceMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

function instant(d: unknown): string {
  if (d instanceof Date) return d.toISOString()
  if (typeof d === 'string') return new Date(d).toISOString()
  return new Date().toISOString()
}

function nullable<T>(v: unknown): T | null {
  return v === null || v === undefined ? null : (v as T)
}

function grant(access: AttendanceAccess, permission: AttendancePermission): void {
  if (!access.permissions.includes(permission)) {
    throw new AttendanceError('forbidden', `Missing required permission: ${permission}`)
  }
}

export class PostgresAttendanceRepository implements AttendanceRepository {
  constructor(
    private readonly pool: Pool,
    private readonly key: string = '',
  ) {}

  static async create(pool: Pool, key: string): Promise<PostgresAttendanceRepository> {
    if (key.length < 32)
      throw new Error('Stable attendance fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_attendance'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Attendance requires a non-owner runtime')

    const expected = await attendanceMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_attendance.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Attendance schema migrations mismatch')

    return new PostgresAttendanceRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: AttendancePrincipal,
    work: (client: PoolClient, access: AttendanceAccess) => Promise<T>,
  ): Promise<T> {
    const client = await this.pool.connect()
    try {
      // 1. Verify connection is not superuser or schema owner
      const userRes = await client.query<{ usename: string; usesuper: boolean }>(
        'SELECT usename, usesuper FROM pg_user WHERE usename = current_user',
      )
      const currentUser = userRes.rows[0]
      if (currentUser?.usesuper) {
        throw new AttendanceError('forbidden', 'Superuser connections are prohibited')
      }

      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])

      // 2. Lock and verify membership
      const memRes = await client.query<Row>(
        'SELECT * FROM archava_attendance.lock_membership($1, $2)',
        [tenant, principal.id],
      )
      const mem = memRes.rows[0]
      if (!mem || !mem.active) {
        throw new AttendanceError('forbidden', 'Principal is inactive or not enrolled in tenant')
      }

      const access: AttendanceAccess = {
        ownerId: principal.id,
        permissions: (mem.permissions as AttendancePermission[]) ?? [],
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
    principal: AttendancePrincipal,
    mutation: AttendanceMutation,
  ): Promise<AttendanceReceipt> {
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = this.key
        ? createHmac('sha256', this.key).update(JSON.stringify(mutation)).digest('hex')
        : createHash('sha256').update(JSON.stringify(mutation)).digest('hex')

      // Check existing command for idempotent replay
      const existingCmd = (
        await client.query<Row>(
          'SELECT receipt, fingerprint FROM archava_attendance.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) {
          throw new AttendanceError('conflict', 'Command payload fingerprint mismatch on replay')
        }
        if (existingCmd.receipt) {
          return attendanceReceiptSchema.parse({
            ...existingCmd.receipt,
            idempotencyReplay: true,
          })
        }
      } else {
        await client.query(
          'INSERT INTO archava_attendance.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
          [tenant, mutation.commandId, access.ownerId, fingerprint],
        )
      }

      let receipt: AttendanceReceipt

      switch (mutation.type) {
        case 'create_session': {
          grant(access, 'attendance.write')
          const sessionId = randomUUID()
          const sessRow = (
            await client.query<Row>(
              `INSERT INTO archava_attendance.sessions(
                tenant_id, id, title, session_code, capacity, start_time,
                end_time, status, location, owner_id
              ) VALUES($1, $2, $3, $4, $5, $6, $7, 'scheduled', $8, $9)
              RETURNING *`,
              [
                tenant,
                sessionId,
                mutation.title,
                mutation.sessionCode,
                mutation.capacity ?? null,
                mutation.startTime,
                mutation.endTime,
                mutation.location ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!sessRow) throw new AttendanceError('not_found', 'Failed to create session')

          await client.query(
            'INSERT INTO archava_attendance.audit(tenant_id, request_id, actor_id, operation, session_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'create_session',
              sessionId,
              `Created session ${mutation.sessionCode}`,
            ],
          )

          receipt = attendanceReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'create_session',
            sessionId,
            status: 'scheduled',
            revision: 1,
            idempotencyReplay: false,
            executedAt: instant(sessRow.created_at),
          })
          break
        }

        case 'register_participant': {
          grant(access, 'attendance.write')
          const session = (
            await client.query<Row>(
              'SELECT * FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.sessionId],
            )
          ).rows[0]
          if (!session) throw new AttendanceError('not_found', 'Session not found')

          if (session.status === 'canceled' || session.status === 'completed') {
            throw new AttendanceError(
              'bad_request',
              'Cannot register for closed or canceled session',
            )
          }

          const capacity = session.capacity as number | null
          const enrolled = session.enrolled_count as number
          if (capacity !== null && enrolled >= capacity) {
            throw new AttendanceError('capacity_exceeded', 'Session capacity reached')
          }

          const rosterId = randomUUID()
          const nextEnrolled = enrolled + 1
          const nextRev = Number(session.revision) + 1

          await client.query(
            'UPDATE archava_attendance.sessions SET enrolled_count=$1, revision=$2, updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4',
            [nextEnrolled, nextRev, tenant, mutation.sessionId],
          )

          const rosterRow = (
            await client.query<Row>(
              `INSERT INTO archava_attendance.rosters(
                tenant_id, id, session_id, contact_id, role, status, ticket_code
              ) VALUES($1, $2, $3, $4, $5, 'registered', $6)
              RETURNING *`,
              [
                tenant,
                rosterId,
                mutation.sessionId,
                mutation.contactId,
                mutation.role,
                mutation.ticketCode,
              ],
            )
          ).rows[0]
          if (!rosterRow) throw new AttendanceError('not_found', 'Failed to register participant')

          await client.query(
            'INSERT INTO archava_attendance.audit(tenant_id, request_id, actor_id, operation, session_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'register_participant',
              mutation.sessionId,
              `Registered contact ${mutation.contactId} with ticket ${mutation.ticketCode}`,
            ],
          )

          receipt = attendanceReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'register_participant',
            sessionId: mutation.sessionId,
            rosterId,
            status: 'registered',
            revision: 1,
            idempotencyReplay: false,
            executedAt: instant(rosterRow.created_at),
          })
          break
        }

        case 'record_check_in': {
          grant(access, 'attendance.checkin')
          const session = (
            await client.query<Row>(
              'SELECT * FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.sessionId],
            )
          ).rows[0]
          if (!session) throw new AttendanceError('not_found', 'Session not found')

          if (session.status !== 'scheduled' && session.status !== 'in_progress') {
            throw new AttendanceError(
              'bad_request',
              'Check-in is only allowed for scheduled or active sessions',
            )
          }

          const roster = (
            await client.query<Row>(
              'SELECT * FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.rosterId],
            )
          ).rows[0]
          if (!roster) throw new AttendanceError('not_found', 'Roster entry not found')

          if (roster.status === 'attended') {
            throw new AttendanceError(
              'already_checked_in',
              'Participant is already checked in for this session',
            )
          }
          if (roster.status === 'canceled') {
            throw new AttendanceError('bad_request', 'Cannot check in a canceled registration')
          }

          // Idempotency key check
          const existingEvent = (
            await client.query<Row>(
              'SELECT id FROM archava_attendance.check_in_events WHERE tenant_id=$1 AND roster_id=$2 AND idempotency_key=$3',
              [tenant, mutation.rosterId, mutation.idempotencyKey],
            )
          ).rows[0]
          if (existingEvent) {
            throw new AttendanceError('already_checked_in', 'Duplicate check-in idempotency key')
          }

          const checkInId = randomUUID()
          const nextRosterRev = Number(roster.revision) + 1
          const nextAttended = Number(session.attended_count) + 1

          await client.query(
            `UPDATE archava_attendance.rosters
             SET status='attended', checked_in_at=clock_timestamp(), check_in_method=$1,
                 revision=$2, updated_at=clock_timestamp()
             WHERE tenant_id=$3 AND id=$4`,
            [mutation.method, nextRosterRev, tenant, mutation.rosterId],
          )

          await client.query(
            'UPDATE archava_attendance.sessions SET attended_count=$1, updated_at=clock_timestamp() WHERE tenant_id=$2 AND id=$3',
            [nextAttended, tenant, mutation.sessionId],
          )

          const checkInRow = (
            await client.query<Row>(
              `INSERT INTO archava_attendance.check_in_events(
                tenant_id, id, roster_id, session_id, contact_id, method,
                evidence_ref, idempotency_key, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)
              RETURNING *`,
              [
                tenant,
                checkInId,
                mutation.rosterId,
                mutation.sessionId,
                roster.contact_id,
                mutation.method,
                mutation.evidenceRef ?? null,
                mutation.idempotencyKey,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!checkInRow) throw new AttendanceError('not_found', 'Failed to record check-in event')

          await client.query(
            'INSERT INTO archava_attendance.audit(tenant_id, request_id, actor_id, operation, session_id, revision, reason) VALUES($1, $2, $3, $4, $5, 1, $6)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'record_check_in',
              mutation.sessionId,
              `Check-in recorded for ticket ${String(roster.ticket_code)} via ${mutation.method}`,
            ],
          )

          receipt = attendanceReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_check_in',
            sessionId: mutation.sessionId,
            rosterId: mutation.rosterId,
            checkInId,
            status: 'attended',
            revision: nextRosterRev,
            idempotencyReplay: false,
            executedAt: instant(checkInRow.recorded_at),
          })
          break
        }

        case 'update_roster_status': {
          grant(access, 'attendance.write')
          const roster = (
            await client.query<Row>(
              'SELECT * FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.rosterId],
            )
          ).rows[0]
          if (!roster) throw new AttendanceError('not_found', 'Roster entry not found')

          if (roster.revision !== mutation.expectedRevision) {
            throw new AttendanceError('conflict', 'Optimistic revision conflict on roster entry')
          }

          const nextRev = Number(roster.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_attendance.rosters
               SET status=$1, revision=$2, updated_at=clock_timestamp()
               WHERE tenant_id=$3 AND id=$4
               RETURNING *`,
              [mutation.targetStatus, nextRev, tenant, mutation.rosterId],
            )
          ).rows[0]
          if (!updated) throw new AttendanceError('not_found', 'Roster update failed')

          if (mutation.targetStatus === 'canceled' && roster.status !== 'canceled') {
            await client.query(
              'UPDATE archava_attendance.sessions SET enrolled_count = GREATEST(0, enrolled_count - 1), updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
              [tenant, roster.session_id],
            )
          }

          await client.query(
            'INSERT INTO archava_attendance.audit(tenant_id, request_id, actor_id, operation, session_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'update_roster_status',
              roster.session_id,
              nextRev,
              mutation.reason,
            ],
          )

          receipt = attendanceReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'update_roster_status',
            sessionId: roster.session_id as string,
            rosterId: mutation.rosterId,
            status: mutation.targetStatus,
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }

        case 'change_session_status': {
          grant(access, 'attendance.write')
          const session = (
            await client.query<Row>(
              'SELECT * FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
              [tenant, mutation.sessionId],
            )
          ).rows[0]
          if (!session) throw new AttendanceError('not_found', 'Session not found')

          if (session.revision !== mutation.expectedRevision) {
            throw new AttendanceError('conflict', 'Optimistic revision conflict on session')
          }

          const nextRev = Number(session.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_attendance.sessions
               SET status=$1, revision=$2, updated_at=clock_timestamp()
               WHERE tenant_id=$3 AND id=$4
               RETURNING *`,
              [mutation.targetStatus, nextRev, tenant, mutation.sessionId],
            )
          ).rows[0]
          if (!updated) throw new AttendanceError('not_found', 'Session update failed')

          if (mutation.targetStatus === 'canceled') {
            await client.query(
              `UPDATE archava_attendance.rosters
               SET status='canceled', updated_at=clock_timestamp()
               WHERE tenant_id=$1 AND session_id=$2 AND status != 'attended'`,
              [tenant, mutation.sessionId],
            )
          }

          await client.query(
            'INSERT INTO archava_attendance.audit(tenant_id, request_id, actor_id, operation, session_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
            [
              tenant,
              mutation.commandId,
              access.ownerId,
              'change_session_status',
              mutation.sessionId,
              nextRev,
              mutation.reason,
            ],
          )

          receipt = attendanceReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'change_session_status',
            sessionId: mutation.sessionId,
            status: mutation.targetStatus,
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }
      }

      await client.query(
        'UPDATE archava_attendance.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3',
        [receipt, tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: AttendancePrincipal,
    commandId: string,
  ): Promise<AttendanceCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_attendance.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'recorded',
        receipt: attendanceReceiptSchema.parse(row.receipt),
      }
    })
  }

  async getAccess(tenant: string, principal: AttendancePrincipal): Promise<AttendanceAccess> {
    return this.transaction(tenant, principal, (_client, access) => Promise.resolve(access))
  }

  async listSessions(
    tenant: string,
    principal: AttendancePrincipal,
    query: SessionPageQuery,
  ): Promise<readonly Session[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'attendance.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }

      params.push(query.limit ?? 50)
      const limitParam = `$${params.length}`
      params.push(query.offset ?? 0)
      const offsetParam = `$${params.length}`

      const res = await client.query<Row>(
        `SELECT * FROM archava_attendance.sessions
         WHERE ${conditions.join(' AND ')}
         ORDER BY start_time ASC, id ASC
         LIMIT ${limitParam} OFFSET ${offsetParam}`,
        params,
      )

      return res.rows.map((row) =>
        sessionSchema.parse({
          id: row.id,
          tenantId: row.tenant_id,
          title: row.title,
          sessionCode: row.session_code,
          capacity: nullable<number>(row.capacity),
          startTime: instant(row.start_time),
          endTime: instant(row.end_time),
          status: row.status,
          location: nullable<string>(row.location),
          enrolledCount: row.enrolled_count,
          attendedCount: row.attended_count,
          revision: row.revision,
          createdAt: instant(row.created_at),
          updatedAt: instant(row.updated_at),
        }),
      )
    })
  }

  async getSessionDetail(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<SessionDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'attendance.read')
      const sessRow = (
        await client.query<Row>(
          'SELECT * FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2',
          [tenant, sessionId],
        )
      ).rows[0]
      if (!sessRow) return null

      const session = sessionSchema.parse({
        id: sessRow.id,
        tenantId: sessRow.tenant_id,
        title: sessRow.title,
        sessionCode: sessRow.session_code,
        capacity: nullable<number>(sessRow.capacity),
        startTime: instant(sessRow.start_time),
        endTime: instant(sessRow.end_time),
        status: sessRow.status,
        location: nullable<string>(sessRow.location),
        enrolledCount: sessRow.enrolled_count,
        attendedCount: sessRow.attended_count,
        revision: sessRow.revision,
        createdAt: instant(sessRow.created_at),
        updatedAt: instant(sessRow.updated_at),
      })

      const rosterRows = (
        await client.query<Row>(
          'SELECT * FROM archava_attendance.rosters WHERE tenant_id=$1 AND session_id=$2 ORDER BY created_at ASC',
          [tenant, sessionId],
        )
      ).rows

      const roster = rosterRows.map((r) =>
        rosterEntrySchema.parse({
          id: r.id,
          tenantId: r.tenant_id,
          sessionId: r.session_id,
          contactId: r.contact_id,
          role: r.role,
          status: r.status,
          ticketCode: r.ticket_code,
          checkedInAt: r.checked_in_at ? instant(r.checked_in_at) : null,
          checkInMethod: nullable<string>(r.check_in_method),
          revision: r.revision,
          createdAt: instant(r.created_at),
          updatedAt: instant(r.updated_at),
        }),
      )

      const checkInRows = (
        await client.query<Row>(
          'SELECT * FROM archava_attendance.check_in_events WHERE tenant_id=$1 AND session_id=$2 ORDER BY recorded_at DESC',
          [tenant, sessionId],
        )
      ).rows

      const checkIns = checkInRows.map((c) =>
        checkInRecordSchema.parse({
          id: c.id,
          tenantId: c.tenant_id,
          rosterId: c.roster_id,
          sessionId: c.session_id,
          contactId: c.contact_id,
          method: c.method,
          evidenceRef: nullable<string>(c.evidence_ref),
          idempotencyKey: c.idempotency_key,
          recordedBy: c.recorded_by,
          recordedAt: instant(c.recorded_at),
        }),
      )

      return sessionDetailSchema.parse({
        session,
        roster,
        checkIns,
      })
    })
  }

  async listRoster(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly RosterEntry[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'attendance.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_attendance.rosters WHERE tenant_id=$1 AND session_id=$2 ORDER BY created_at ASC',
          [tenant, sessionId],
        )
      ).rows

      return rows.map((r) =>
        rosterEntrySchema.parse({
          id: r.id,
          tenantId: r.tenant_id,
          sessionId: r.session_id,
          contactId: r.contact_id,
          role: r.role,
          status: r.status,
          ticketCode: r.ticket_code,
          checkedInAt: r.checked_in_at ? instant(r.checked_in_at) : null,
          checkInMethod: nullable<string>(r.check_in_method),
          revision: r.revision,
          createdAt: instant(r.created_at),
          updatedAt: instant(r.updated_at),
        }),
      )
    })
  }

  async listCheckIns(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly CheckInRecord[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'attendance.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_attendance.check_in_events WHERE tenant_id=$1 AND session_id=$2 ORDER BY recorded_at DESC',
          [tenant, sessionId],
        )
      ).rows

      return rows.map((c) =>
        checkInRecordSchema.parse({
          id: c.id,
          tenantId: c.tenant_id,
          rosterId: c.roster_id,
          sessionId: c.session_id,
          contactId: c.contact_id,
          method: c.method,
          evidenceRef: nullable<string>(c.evidence_ref),
          idempotencyKey: c.idempotency_key,
          recordedBy: c.recorded_by,
          recordedAt: instant(c.recorded_at),
        }),
      )
    })
  }
}
