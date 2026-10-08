import { createHash, randomUUID } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool, PoolClient } from 'pg'
import { hashPassword, usernameSchema } from './password.js'

export const IDENTITY_MIGRATION = '001_identity'
const sqlFile = new URL('../migrations/001_identity.sql', import.meta.url)
export async function identityMigrationChecksum() {
  return createHash('sha256')
    .update(await readFile(sqlFile))
    .digest('hex')
}
export async function migrateIdentity(pool: Pool): Promise<void> {
  const sql = await readFile(sqlFile, 'utf8')
  const checksum = createHash('sha256').update(sql).digest('hex')
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_identity_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_identity')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_identity.schema_migrations (id text PRIMARY KEY,checksum text NOT NULL)',
    )
    const existing = await client.query<{ id: string; checksum: string }>(
      'SELECT * FROM archava_identity.schema_migrations',
    )
    if (existing.rows.some((row) => row.id !== IDENTITY_MIGRATION || row.checksum !== checksum))
      throw new Error('Unsupported identity migration history')
    if (existing.rows.length === 0) {
      await client.query(sql)
      await client.query('INSERT INTO archava_identity.schema_migrations VALUES($1,$2)', [
        IDENTITY_MIGRATION,
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
export async function grantIdentityRuntime(pool: Pool, role: string): Promise<void> {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid identity runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_identity TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_identity.users,archava_identity.schema_migrations TO "${role}"`,
  )
  await pool.query(`GRANT UPDATE(password_hash,version) ON archava_identity.users TO "${role}"`)
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE,DELETE ON archava_identity.sessions,archava_identity.login_buckets TO "${role}"`,
  )
  await pool.query(`GRANT SELECT,INSERT ON archava_identity.events TO "${role}"`)
}
/** Operator-only enrollment. Browser sessions never grant provisioning privileges. */
export async function provisionIdentity(
  pool: Pool,
  rawUsername: unknown,
  password: unknown,
  onProvision?: (
    client: PoolClient,
    identity: { principalId: string; username: string },
  ) => Promise<void>,
) {
  const username = usernameSchema.parse(rawUsername)
  const hash = await hashPassword(password)
  const id = 'human-' + randomUUID()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query(
      'INSERT INTO archava_identity.users(id,username,password_hash) VALUES($1,$2,$3)',
      [id, username, hash],
    )
    await onProvision?.(client, { principalId: id, username })
    await client.query(
      "INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'provision')",
      [randomUUID(), id],
    )
    await client.query('COMMIT')
    return { principalId: id, username }
  } catch (error) {
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}
export async function resetIdentityPassword(
  pool: Pool,
  rawUsername: unknown,
  password: unknown,
  activate = false,
) {
  const username = usernameSchema.parse(rawUsername)
  const hash = await hashPassword(password)
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    const result = await client.query<{ id: string }>(
      'UPDATE archava_identity.users SET password_hash=$2,version=version+1,active=CASE WHEN $3 THEN true ELSE active END WHERE username=$1 RETURNING id',
      [username, hash, activate],
    )
    const id = result.rows[0]?.id
    if (!id) throw new Error('Account not found')
    await client.query('DELETE FROM archava_identity.sessions WHERE user_id=$1', [id])
    await client.query(
      "INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'password_reset')",
      [randomUUID(), id],
    )
    await client.query('COMMIT')
  } catch (error) {
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}
