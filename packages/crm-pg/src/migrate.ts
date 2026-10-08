import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const CRM_MIGRATION = '001_crm_contacts'

export async function crmMigrationChecksum(): Promise<string> {
  const sql = await readFile(new URL('../migrations/001_crm_contacts.sql', import.meta.url), 'utf8')
  return createHash('sha256').update(sql).digest('hex')
}

/** Run with a migration-owner connection; never with the application's login. */
export async function migrateCrm(pool: Pool): Promise<void> {
  const sql = await readFile(new URL('../migrations/001_crm_contacts.sql', import.meta.url), 'utf8')
  const checksum = createHash('sha256').update(sql).digest('hex')
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_crm_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_crm')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_crm.schema_migrations (id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_crm.schema_migrations',
    )
    if (existing.rows.some((row) => row.id !== CRM_MIGRATION || row.checksum !== checksum))
      throw new Error('Unsupported or modified CRM migration history')
    if (existing.rows.length === 0) {
      await client.query(sql)
      await client.query('INSERT INTO archava_crm.schema_migrations(id,checksum) VALUES($1,$2)', [
        CRM_MIGRATION,
        checksum,
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
  await pool.query(`GRANT EXECUTE ON FUNCTION archava_crm.lock_membership(text,text) TO "${role}"`)
}
