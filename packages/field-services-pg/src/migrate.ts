import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const FIELD_MIGRATIONS = ['001_field_services_dispatch_visits'] as const

export async function fieldMigrationLedger() {
  return await Promise.all(
    FIELD_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateFieldServices(pool: Pool): Promise<void> {
  const ledger = await fieldMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_field_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_field')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_field.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_field.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified field services migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query('INSERT INTO archava_field.schema_migrations(id,checksum) VALUES($1,$2)', [
        item.id,
        item.checksum,
      ])
    }
    await client.query('COMMIT')
  } catch (error) {
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}

export async function grantFieldServicesRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid field runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_field TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_field.schema_migrations,archava_field.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_field.jobs,archava_field.commands TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_field.visits,archava_field.work_lines,archava_field.signoffs,archava_field.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_field.lock_membership(text,text) TO "${role}"`,
  )
}
