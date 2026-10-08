import { createHmac, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  IdentityError,
  usernameSchema,
  passwordSchema,
  hashPassword,
  verifyPassword,
} from './password.js'
import { IDENTITY_MIGRATION, identityMigrationChecksum } from './migrate.js'

const tokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/)
export interface AuthenticatedSession {
  readonly principalId: string
  readonly username: string
  readonly csrf: string
}
export class PostgresIdentity {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}
  static async create(pool: Pool, key: string) {
    if (Buffer.byteLength(key) < 32)
      throw new Error('Identity secret must contain at least 32 random bytes')
    const role = await pool.query<{ unsafe: boolean }>(
      "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_identity'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
    )
    if (role.rows[0]?.unsafe !== false)
      throw new Error('Identity runtime requires a non-owner application role')
    const versions = await pool.query<{ id: string; checksum: string }>(
      'SELECT * FROM archava_identity.schema_migrations',
    )
    if (
      versions.rows.length !== 1 ||
      versions.rows[0]?.id !== IDENTITY_MIGRATION ||
      versions.rows[0]?.checksum !== (await identityMigrationChecksum())
    )
      throw new Error('Unsupported identity schema')
    return new PostgresIdentity(pool, key)
  }
  private digest(label: string, value: string) {
    return createHmac('sha256', this.key)
      .update(label + '\0' + value)
      .digest('hex')
  }
  private async transaction<T>(run: (client: PoolClient) => Promise<T>) {
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      const result = await run(client)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      throw error
    } finally {
      client.release()
    }
  }
  private async rate(username: string, source: string) {
    const denied = await this.transaction(async (client) => {
      let blocked = false
      // Consistent source-first lock order; every attempted login consumes budget.
      for (const [label, value, seconds, limit] of [
        ['source', source, 300, 60],
        ['account', username, 900, 5],
      ] as const) {
        const result = await client.query<{ attempts: number }>(
          `INSERT INTO archava_identity.login_buckets(key,attempts) VALUES($1,1) ON CONFLICT(key) DO UPDATE SET attempts=CASE WHEN login_buckets.window_start<now()-make_interval(secs=>$2) THEN 1 ELSE LEAST(login_buckets.attempts+1,1000) END,window_start=CASE WHEN login_buckets.window_start<now()-make_interval(secs=>$2) THEN now() ELSE login_buckets.window_start END RETURNING attempts`,
          [this.digest(label, value), seconds],
        )
        blocked ||= (result.rows[0]?.attempts ?? 1000) > limit
      }
      await client.query(
        "DELETE FROM archava_identity.login_buckets WHERE key IN (SELECT key FROM archava_identity.login_buckets WHERE window_start<now()-interval '1 day' LIMIT 100)",
      )
      await client.query(
        "DELETE FROM archava_identity.sessions WHERE token_hash IN (SELECT token_hash FROM archava_identity.sessions WHERE expires_at<now() OR last_seen_at<now()-interval '30 minutes' LIMIT 100)",
      )
      return blocked
    })
    if (denied) throw new IdentityError('rate_limited')
  }
  async login(
    rawUsername: unknown,
    rawPassword: unknown,
    source: string,
  ): Promise<{ token: string }> {
    const username = usernameSchema.parse(rawUsername)
    const password = z.string().max(128).parse(rawPassword)
    if (source.length > 128) throw new Error('Untrusted source identifier')
    await this.rate(username, source)
    const found = await this.pool.query<{ id: string; password_hash: string; active: boolean }>(
      'SELECT id,password_hash,active FROM archava_identity.users WHERE username=$1',
      [username],
    )
    const user = found.rows[0]
    if (!(await verifyPassword(password, user?.password_hash ?? null)) || !user?.active)
      throw new IdentityError('invalid_credentials')
    const token = randomBytes(32).toString('base64url')
    await this.transaction(async (client) => {
      const inserted = await client.query(
        'INSERT INTO archava_identity.sessions(token_hash,user_id,user_version) SELECT $1,id,version FROM archava_identity.users WHERE id=$2 AND active AND password_hash=$3 RETURNING user_id',
        [this.digest('session', token), user.id, user.password_hash],
      )
      if (inserted.rowCount !== 1) throw new IdentityError('invalid_credentials')
      await client.query(
        "INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'login')",
        [randomUUID(), user.id],
      )
      await client.query('DELETE FROM archava_identity.login_buckets WHERE key=$1', [
        this.digest('account', username),
      ])
    })
    return { token }
  }
  async authenticate(raw: unknown): Promise<AuthenticatedSession | null> {
    const token = tokenSchema.safeParse(raw)
    if (!token.success) return null
    const result = await this.pool.query<{ user_id: string; username: string }>(
      "UPDATE archava_identity.sessions s SET last_seen_at=now() FROM archava_identity.users u WHERE s.token_hash=$1 AND u.id=s.user_id AND u.active AND u.version=s.user_version AND s.expires_at>now() AND s.last_seen_at>now()-interval '30 minutes' RETURNING s.user_id,u.username",
      [this.digest('session', token.data)],
    )
    const row = result.rows[0]
    return row
      ? { principalId: row.user_id, username: row.username, csrf: this.digest('csrf', token.data) }
      : null
  }
  validCsrf(token: string, csrf: string | null): boolean {
    if (!tokenSchema.safeParse(token).success || !csrf || !/^[a-f0-9]{64}$/.test(csrf)) return false
    return timingSafeEqual(Buffer.from(this.digest('csrf', token), 'hex'), Buffer.from(csrf, 'hex'))
  }
  async logout(raw: unknown): Promise<void> {
    const token = tokenSchema.safeParse(raw)
    if (!token.success) return
    await this.transaction(async (client) => {
      const result = await client.query<{ user_id: string }>(
        'DELETE FROM archava_identity.sessions WHERE token_hash=$1 RETURNING user_id',
        [this.digest('session', token.data)],
      )
      if (result.rows[0])
        await client.query(
          "INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'logout')",
          [randomUUID(), result.rows[0].user_id],
        )
    })
  }
  async changePassword(token: string, current: string, next: unknown): Promise<void> {
    const session = await this.authenticate(token)
    if (!session) throw new IdentityError('unauthenticated')
    await this.rate(session.username, 'password-change-' + session.principalId)
    passwordSchema.parse(next)
    const result = await this.pool.query<{ password_hash: string }>(
      'SELECT password_hash FROM archava_identity.users WHERE id=$1',
      [session.principalId],
    )
    const previous = result.rows[0]?.password_hash
    if (!previous || !(await verifyPassword(current, previous)))
      throw new IdentityError('invalid_credentials')
    const hash = await hashPassword(next)
    await this.transaction(async (client) => {
      const updated = await client.query(
        'UPDATE archava_identity.users SET password_hash=$2,version=version+1 WHERE id=$1 AND active AND password_hash=$3 RETURNING id',
        [session.principalId, hash, previous],
      )
      if (updated.rowCount !== 1) throw new IdentityError('invalid_credentials')
      await client.query('DELETE FROM archava_identity.sessions WHERE user_id=$1', [
        session.principalId,
      ])
      await client.query(
        "INSERT INTO archava_identity.events(id,user_id,kind) VALUES($1,$2,'password_change')",
        [randomUUID(), session.principalId],
      )
    })
  }
}
