import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const SUPPORT_MIGRATIONS = ['001_cases_notes_resolution'] as const
export async function supportMigrationLedger() {
  return await Promise.all(
    SUPPORT_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migrateSupport(pool: Pool): Promise<void> {
  const ledger = await supportMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_support_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_support')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_support.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_support.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified support migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_support.schema_migrations(id,checksum) VALUES($1,$2)',
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
/** Grants runtime SQL privileges without enrolling users or giving domain authority. */
export async function grantSupportRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid support runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_support TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_support.schema_migrations,archava_support.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_support.cases,archava_support.commands TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_support.notes,archava_support.transitions,archava_support.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_support.lock_membership(text,text) TO "${role}"`,
  )
}
