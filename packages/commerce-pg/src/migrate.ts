import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const COMMERCE_MIGRATIONS = [
  '001_catalog_orders_stock',
  '002_managed_menu_products',
  '003_purchase_receipts',
  // RA01's server-side authority records: explicit grant columns and bound,
  // single-use human approvals. Added with the Rust candidate, so the
  // TypeScript migrator and the Rust fixture stay two languages over ONE
  // migration history — if either side drifted, a database one migrates would
  // not be one the other is willing to serve.
  '004_bound_approvals',
] as const
export async function commerceMigrationLedger() {
  return await Promise.all(
    COMMERCE_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migrateCommerce(pool: Pool): Promise<void> {
  const ledger = await commerceMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_commerce_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_commerce')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_commerce.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_commerce.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified commerce migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_commerce.schema_migrations(id,checksum) VALUES($1,$2)',
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
export async function grantCommerceRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid commerce runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_commerce TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_commerce.schema_migrations,archava_commerce.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_commerce.products,archava_commerce.orders,archava_commerce.allocations,archava_commerce.commands TO "${role}"`,
  )
  await pool.query(`GRANT SELECT,UPDATE ON archava_commerce.stock_balances TO "${role}"`)
  await pool.query(`GRANT SELECT,INSERT,UPDATE,DELETE ON archava_commerce.order_lines TO "${role}"`)
  await pool.query(
    `GRANT SELECT,INSERT ON archava_commerce.movements,archava_commerce.returns,archava_commerce.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_commerce.lock_membership(text,text) TO "${role}"`,
  )
}
