import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const PURCHASE_MIGRATIONS = ['001_suppliers_orders_receipts'] as const
export async function purchasingMigrationLedger() {
  return await Promise.all(
    PURCHASE_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migratePurchasing(pool: Pool): Promise<void> {
  const ledger = await purchasingMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_purchasing_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_purchasing')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_purchasing.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_purchasing.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified purchasing migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_purchasing.schema_migrations(id,checksum) VALUES($1,$2)',
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
export async function grantPurchasingRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid purchasing runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_purchasing TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_purchasing.schema_migrations,archava_purchasing.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_purchasing.suppliers,archava_purchasing.offers,archava_purchasing.orders,archava_purchasing.lines,archava_purchasing.commands TO "${role}"`,
  )
  await pool.query(`GRANT DELETE ON archava_purchasing.lines TO "${role}"`)
  await pool.query(
    `GRANT SELECT,INSERT ON archava_purchasing.receipts,archava_purchasing.receipt_lines,archava_purchasing.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_purchasing.lock_membership(text,text) TO "${role}"`,
  )
}
