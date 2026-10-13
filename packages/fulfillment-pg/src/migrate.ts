import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const FULFILLMENT_MIGRATIONS = ['001_fulfillment_jobs_evidence'] as const

export async function fulfillmentMigrationLedger() {
  return await Promise.all(
    FULFILLMENT_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateFulfillment(pool: Pool): Promise<void> {
  const ledger = await fulfillmentMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_fulfillment_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_fulfillment')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_fulfillment.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_fulfillment.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified fulfillment migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_fulfillment.schema_migrations(id,checksum) VALUES($1,$2)',
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

export async function grantFulfillmentRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid fulfillment runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_fulfillment TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_fulfillment.schema_migrations,archava_fulfillment.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_fulfillment.jobs,archava_fulfillment.commands TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT ON archava_fulfillment.job_lines,archava_fulfillment.findings,archava_fulfillment.custody_events,archava_fulfillment.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_fulfillment.lock_membership(text,text) TO "${role}"`,
  )
}
