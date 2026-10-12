import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const ASSET_MIGRATIONS = ['001_assets_custody_maintenance'] as const

export async function assetMigrationLedger() {
  return await Promise.all(
    ASSET_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}

export async function migrateAssets(pool: Pool): Promise<void> {
  const ledger = await assetMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_assets_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_assets')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_assets.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_assets.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row: { id: string; checksum: string }, i: number) =>
          row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified assets migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_assets.schema_migrations(id,checksum) VALUES($1,$2)',
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

export async function grantAssetsRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid assets runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_assets TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_assets.schema_migrations,archava_assets.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_assets.assets,archava_assets.custody_events,archava_assets.maintenance_records,archava_assets.commands,archava_assets.audit TO "${role}"`,
  )
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_assets.lock_membership(text,text) TO "${role}"`,
  )
}
