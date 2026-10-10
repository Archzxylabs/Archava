import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool } from 'pg'

export const PROJECT_MIGRATIONS = ['001_project_delivery'] as const
export async function projectMigrationLedger() {
  return await Promise.all(
    PROJECT_MIGRATIONS.map(async (id) => ({
      id,
      checksum: createHash('sha256')
        .update(await readFile(new URL(`../migrations/${id}.sql`, import.meta.url)))
        .digest('hex'),
    })),
  )
}
/** Explicit migration-owner connection only. Existing unrelated schemas are never changed. */
export async function migrateProjects(pool: Pool): Promise<void> {
  const ledger = await projectMigrationLedger()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_projects_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_projects')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_projects.schema_migrations(id text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_projects.schema_migrations ORDER BY id',
    )
    if (
      existing.rows.length > ledger.length ||
      existing.rows.some(
        (row, i) => row.id !== ledger[i]?.id || row.checksum !== ledger[i]?.checksum,
      )
    )
      throw new Error('Unsupported or modified project migration history')
    for (const item of ledger.slice(existing.rows.length)) {
      await client.query(
        await readFile(new URL(`../migrations/${item.id}.sql`, import.meta.url), 'utf8'),
      )
      await client.query(
        'INSERT INTO archava_projects.schema_migrations(id,checksum) VALUES($1,$2)',
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
/** Grants runtime privileges; deliberately never enrolls users or grants domain permissions. */
export async function grantProjectsRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid project runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_projects TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_projects.schema_migrations,archava_projects.memberships TO "${role}"`,
  )
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_projects.projects,archava_projects.tasks,archava_projects.milestones,archava_projects.commands TO "${role}"`,
  )
  await pool.query(`GRANT SELECT,INSERT ON archava_projects.audit TO "${role}"`)
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_projects.lock_membership(text,text) TO "${role}"`,
  )
}
