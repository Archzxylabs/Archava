import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const BILLING_MIGRATIONS = ['001_billing_invoices_settlement'] as const

export async function billingMigrationLedger() {
  return await Promise.all(
    BILLING_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateBilling(pool: Pool): Promise<void> {
  const ledger = await billingMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_billing_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_billing')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_billing.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_billing.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified billing migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_billing.schema_migrations(id,checksum) VALUES($1,$2)',
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

export async function grantBillingRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid billing runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_billing TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_billing.schema_migrations,archava_billing.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_billing.invoices,archava_billing.commands TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_billing.invoice_lines,archava_billing.invoice_corrections,archava_billing.payment_attempts,archava_billing.payment_events,archava_billing.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_billing.lock_membership(text,text) TO "${role}"`,
  )
}
