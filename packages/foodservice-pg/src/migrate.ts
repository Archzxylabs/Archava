import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const FOOD_MIGRATIONS = ['001_tables_recipes_tickets'] as const
export async function foodserviceMigrationLedger() {
  return await Promise.all(
    FOOD_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migrateFoodService(pool: Pool): Promise<void> {
  const ledger = await foodserviceMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_foodservice_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_foodservice')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_foodservice.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_foodservice.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified foodservice migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_foodservice.schema_migrations(id,checksum) VALUES($1,$2)',
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
export async function grantFoodServiceRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid foodservice runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_foodservice TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_foodservice.schema_migrations,archava_foodservice.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_foodservice.tables,archava_foodservice.menu_items,archava_foodservice.tickets,archava_foodservice.commands,archava_foodservice.recipe_versions TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_foodservice.recipe_components,archava_foodservice.ticket_lines,archava_foodservice.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_foodservice.lock_membership(text,text) TO "${role}"`,
  )
}
