import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'
import { grantOccupancyRuntime } from '@archava/scheduling-pg/occupancy'

export const ACCOMMODATION_MIGRATIONS = ['001_room_night_stays'] as const
export async function accommodationMigrationLedger() {
  return await Promise.all(
    ACCOMMODATION_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migrateAccommodation(pool: Pool): Promise<void> {
  const ledger = await accommodationMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_accommodation_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_accommodation')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_accommodation.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_accommodation.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified accommodation migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_accommodation.schema_migrations(id,checksum) VALUES($1,$2)',
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
/** Grants runtime privileges; deliberately never enrolls users or grants domain permissions. */
export async function grantAccommodationRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid accommodation runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_accommodation TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_accommodation.schema_migrations,archava_accommodation.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_accommodation.properties,archava_accommodation.stay_nights TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_accommodation.room_types,archava_accommodation.rooms,archava_accommodation.stays,archava_accommodation.commands TO "${role}"`,
  )
  await pool.query(`GRANT SELECT,INSERT ON archava_accommodation.audit TO "${role}"`)
  await grantOccupancyRuntime(pool, role)
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_accommodation.lock_membership(text,text) TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_accommodation.lock_property(text,uuid) TO "${role}"`,
  )
}
