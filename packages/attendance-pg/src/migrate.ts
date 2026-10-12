import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const ATTENDANCE_MIGRATIONS = ['001_attendance_sessions_rosters'] as const

export async function attendanceMigrationLedger() {
  return await Promise.all(
    ATTENDANCE_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateAttendance(pool: Pool): Promise<void> {
  const ledger = await attendanceMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_attendance_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_attendance')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_attendance.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_attendance.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified attendance migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_attendance.schema_migrations(id,checksum) VALUES($1,$2)',
        [item.id, item.checksum],
      )
    }
    await client.query('COMMIT')
  } catch (error) {
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}

export async function grantAttendanceRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid attendance runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_attendance TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_attendance.schema_migrations,archava_attendance.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_attendance.sessions,archava_attendance.rosters,archava_attendance.check_in_events,archava_attendance.commands,archava_attendance.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_attendance.lock_membership(text,text) TO "${role}"`,
  )
}
