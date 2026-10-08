import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const CRM_MIGRATION = '001_crm_contacts'
export const CRM_MIGRATIONS = [CRM_MIGRATION, '002_sales_pipeline'] as const

export async function crmMigrationChecksum(id: string = CRM_MIGRATION): Promise<string> {
  if (!CRM_MIGRATIONS.some((migration) => migration === id))
    throw new Error('Unknown CRM migration')
  const sql = await readFile(new URL(`../migrations/${id}.sql`, import.meta.url), 'utf8')
  return createHash('sha256').update(sql).digest('hex')
}
export async function crmMigrationLedger() {
  return await Promise.all(
    CRM_MIGRATIONS.map(async (id) => ({ id, checksum: await crmMigrationChecksum(id) })),
  )
}

/** Run with a migration-owner connection; never with the application's login. */
export async function migrateCrm(pool: Pool): Promise<void> {
  const migrations = await Promise.all(
    CRM_MIGRATIONS.map(async (id) => {
      const sql = await readFile(new URL(`../migrations/${id}.sql`, import.meta.url), 'utf8')
      return { id, sql, checksum: createHash('sha256').update(sql).digest('hex') }
    }),
  )
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_crm_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_crm')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_crm.schema_migrations (id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_crm.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > migrations.length ||
      existing.rows.some(
        (row, index) =>
          row.id !== migrations[index]?.id || row.checksum !== migrations[index]?.checksum,
      )
    )
      throw new Error('Unsupported or modified CRM migration history')
    for (const migration of migrations.slice(existing.rows.length)) {
      await client.query(migration.sql)
      await client.query('INSERT INTO archava_crm.schema_migrations(id,checksum) VALUES($1,$2)', [
        migration.id,
        migration.checksum,
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

/** Provision privileges, not identities. The host owns role and membership management. */
export async function grantCrmRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid runtime role name')
  // Identifiers cannot be SQL parameters; this name is operator-owned and validated above.
  await pool.query(`GRANT USAGE ON SCHEMA archava_crm TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_crm.schema_migrations, archava_crm.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_crm.contacts,archava_crm.followups,archava_crm.commands TO "${role}"`,
  )
  await pool.query(`GRANT SELECT,INSERT ON archava_crm.activities,archava_crm.audit TO "${role}"`)
  await pool.query(`GRANT SELECT,INSERT,UPDATE ON archava_crm.opportunities TO "${role}"`)
  await pool.query(`GRANT SELECT,INSERT ON archava_crm.opportunity_transitions TO "${role}"`)
  await pool.query(`GRANT EXECUTE ON FUNCTION archava_crm.lock_membership(text,text) TO "${role}"`)
}
