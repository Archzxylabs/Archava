import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const LAUNDRY_MIGRATIONS = ['001_laundry_orders_processing'] as const

export async function laundryMigrationLedger() {
  return await Promise.all(
    LAUNDRY_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateLaundry(pool: Pool): Promise<void> {
  const ledger = await laundryMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_laundry_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_laundry')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_laundry.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_laundry.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified laundry migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_laundry.schema_migrations(id,checksum) VALUES($1,$2)',
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

export async function grantLaundryRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid laundry runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_laundry TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_laundry.schema_migrations,archava_laundry.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_laundry.orders,archava_laundry.commands TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_laundry.order_lines,archava_laundry.process_events,archava_laundry.findings,archava_laundry.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_laundry.lock_membership(text,text) TO "${role}"`,
  )
}
