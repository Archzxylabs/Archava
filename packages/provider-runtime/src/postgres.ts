import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  budgetIdSchema,
  providerPolicySchema,
  completionSchema,
  type ProviderPolicy,
  type ProviderAttemptStore,
  type ProviderCompletion,
  type Admission,
} from './index.js'

const MIGRATION = '001_provider_attempts'
async function migration() {
  const sql = await readFile(
    new URL('../migrations/001_provider_attempts.sql', import.meta.url),
    'utf8',
  )
  return { sql, checksum: createHash('sha256').update(sql).digest('hex') }
}
export async function migrateProviderRuntime(pool: Pool) {
  const { sql, checksum } = await migration()
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_provider_migrations'))")
    await client.query('CREATE SCHEMA IF NOT EXISTS archava_provider')
    await client.query(
      'CREATE TABLE IF NOT EXISTS archava_provider.schema_migrations(id text PRIMARY KEY,checksum text NOT NULL)',
    )
    const rows = (
      await client.query<{ id: string; checksum: string }>(
        'SELECT * FROM archava_provider.schema_migrations',
      )
    ).rows
    if (rows.some((row) => row.id !== MIGRATION || row.checksum !== checksum))
      throw new Error('Unsupported provider schema')
    if (!rows.length) {
      await client.query(sql)
      await client.query('INSERT INTO archava_provider.schema_migrations VALUES($1,$2)', [
        MIGRATION,
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
export async function grantProviderRuntime(pool: Pool, role: string) {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid provider runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_provider TO "${role}"`)
  await pool.query(`GRANT SELECT ON archava_provider.schema_migrations TO "${role}"`)
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_provider.buckets,archava_provider.attempts TO "${role}"`,
  )
}
type Bucket = {
  bucket_id: string
  provider_id: string
  model: string
  daily_limit: number
  minimum_interval_ms: number
  lease_ms: number
  usage_day: Date
  used: number
  last_started_at: Date | null
  active_attempt: string | null
  lease_until: Date | null
  hold_until: Date | null
}
async function transaction<T>(
  pool: Pool,
  bucketId: string,
  run: (client: PoolClient) => Promise<T>,
): Promise<T> {
  const client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.provider_bucket',$1,true)", [
      budgetIdSchema.parse(bucketId),
    ])
    const value = await run(client)
    await client.query('COMMIT')
    return value
  } catch (error) {
    await client.query('ROLLBACK')
    throw error
  } finally {
    client.release()
  }
}
async function time(client: PoolClient): Promise<Date> {
  const row = (await client.query<{ now: Date }>('SELECT clock_timestamp() AS now')).rows[0]
  if (!row) throw new Error('Provider clock unavailable')
  return row.now
}
async function seed(client: PoolClient, policy: ProviderPolicy) {
  await client.query(
    `INSERT INTO archava_provider.buckets
    (bucket_id,provider_id,model,daily_limit,minimum_interval_ms,lease_ms,usage_day)
    VALUES($1,$2,$3,$4,$5,$6,(clock_timestamp() AT TIME ZONE 'UTC')::date) ON CONFLICT DO NOTHING`,
    [
      policy.bucketId,
      policy.providerId,
      policy.model,
      policy.dailyLimit,
      policy.minimumIntervalMs,
      policy.leaseMs,
    ],
  )
  const row = (
    await client.query<Bucket>(
      'SELECT * FROM archava_provider.buckets WHERE bucket_id=$1 FOR UPDATE',
      [policy.bucketId],
    )
  ).rows[0]
  if (
    !row ||
    row.provider_id !== policy.providerId ||
    row.model !== policy.model ||
    row.daily_limit !== policy.dailyLimit ||
    row.minimum_interval_ms !== policy.minimumIntervalMs ||
    row.lease_ms !== policy.leaseMs
  )
    throw new Error('Provider policy mismatch')
  return row
}
async function reap(client: PoolClient, bucket: Bucket, now: Date) {
  if (!bucket.active_attempt || !bucket.lease_until || bucket.lease_until.getTime() > now.getTime())
    return
  await client.query(
    `UPDATE archava_provider.attempts SET state='unknown',failure_code='interrupted',
    finished_at=$3,elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM ($3::timestamptz-started_at))*1000))::integer
    WHERE bucket_id=$1 AND attempt_id=$2 AND state='pending'`,
    [bucket.bucket_id, bucket.active_attempt, now],
  )
  await client.query(
    'UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL WHERE bucket_id=$1',
    [bucket.bucket_id],
  )
  bucket.active_attempt = null
  bucket.lease_until = null
}
export class PostgresProviderAttemptStore implements ProviderAttemptStore {
  private constructor(private readonly pool: Pool) {}
  static async create(pool: Pool) {
    const role = (
      await pool.query<{ unsafe: boolean }>(`SELECT (rolsuper OR rolbypassrls OR EXISTS
      (SELECT 1 FROM pg_class WHERE relnamespace='archava_provider'::regnamespace AND relowner=pg_roles.oid)) AS unsafe
      FROM pg_roles WHERE rolname=current_user`)
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Provider runtime requires a non-owner role')
    const rows = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT * FROM archava_provider.schema_migrations',
      )
    ).rows
    const expected = await migration()
    if (rows.length !== 1 || rows[0]?.id !== MIGRATION || rows[0]?.checksum !== expected.checksum)
      throw new Error('Unsupported provider schema')
    const rls = (
      await pool.query<{ n: number }>(
        "SELECT count(*)::integer AS n FROM pg_class WHERE relnamespace='archava_provider'::regnamespace AND relrowsecurity AND relforcerowsecurity",
      )
    ).rows[0]
    if (rls?.n !== 2) throw new Error('Provider scope policies must be forced')
    return new PostgresProviderAttemptStore(pool)
  }
  reserve(raw: ProviderPolicy, rawId: string): Promise<Admission> {
    const policy = providerPolicySchema.parse(raw)
    const id = z.uuid().parse(rawId)
    return transaction(this.pool, policy.bucketId, async (client): Promise<Admission> => {
      const bucket = await seed(client, policy)
      const now = await time(client)
      await reap(client, bucket, now)
      const nextDay = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1)
      if (
        (
          await client.query(
            'SELECT 1 FROM archava_provider.attempts WHERE bucket_id=$1 AND attempt_id=$2',
            [policy.bucketId, id],
          )
        ).rowCount
      )
        return { allowed: false, code: 'duplicate_attempt', retryAfterMs: 0 }
      if (bucket.hold_until && bucket.hold_until.getTime() > now.getTime())
        return {
          allowed: false,
          code: 'restore_hold',
          retryAfterMs: bucket.hold_until.getTime() - now.getTime(),
        }
      if (bucket.active_attempt && bucket.lease_until)
        return {
          allowed: false,
          code: 'busy',
          retryAfterMs: bucket.lease_until.getTime() - now.getTime(),
        }
      // Compare dates inside PostgreSQL: pg's date parser follows the process timezone.
      const current = (
        await client.query<{ same: boolean }>(
          "SELECT usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date) AS same FROM archava_provider.buckets WHERE bucket_id=$1",
          [policy.bucketId, now],
        )
      ).rows[0]
      if (!current?.same) {
        await client.query(
          "UPDATE archava_provider.buckets SET usage_day=($2::timestamptz AT TIME ZONE 'UTC')::date,used=0,hold_until=NULL WHERE bucket_id=$1",
          [policy.bucketId, now],
        )
        bucket.used = 0
      }
      if (bucket.used >= policy.dailyLimit)
        return { allowed: false, code: 'daily_limit', retryAfterMs: nextDay - now.getTime() }
      const remaining = bucket.last_started_at
        ? policy.minimumIntervalMs - (now.getTime() - bucket.last_started_at.getTime())
        : 0
      if (remaining > 0) return { allowed: false, code: 'cooldown', retryAfterMs: remaining }
      await client.query(
        "INSERT INTO archava_provider.attempts(bucket_id,attempt_id,started_at,state) VALUES($1,$2,$3,'pending')",
        [policy.bucketId, id, now],
      )
      await client.query(
        'UPDATE archava_provider.buckets SET used=used+1,last_started_at=$2,active_attempt=$3,lease_until=$4 WHERE bucket_id=$1',
        [policy.bucketId, now, id, new Date(now.getTime() + policy.leaseMs)],
      )
      return { allowed: true }
    })
  }
  finish(rawBucket: string, rawId: string, raw: ProviderCompletion): Promise<boolean> {
    const bucketId = budgetIdSchema.parse(rawBucket)
    const id = z.uuid().parse(rawId)
    const completion = completionSchema.parse(raw)
    return transaction(this.pool, bucketId, async (client) => {
      const bucket = (
        await client.query<Bucket>(
          'SELECT * FROM archava_provider.buckets WHERE bucket_id=$1 FOR UPDATE',
          [bucketId],
        )
      ).rows[0]
      if (!bucket) return false
      const now = await time(client)
      await reap(client, bucket, now)
      if (bucket.active_attempt !== id) return false
      const changed = await client.query(
        `UPDATE archava_provider.attempts SET state=$3,http_status=$4,failure_code=$5,finished_at=$6,
        elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM ($6::timestamptz-started_at))*1000))::integer
        WHERE bucket_id=$1 AND attempt_id=$2 AND state='pending'`,
        [
          bucketId,
          id,
          completion.state,
          completion.httpStatus,
          completion.state === 'failed' ? completion.failureCode : null,
          now,
        ],
      )
      if (changed.rowCount !== 1) return false
      await client.query(
        'UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL WHERE bucket_id=$1',
        [bucketId],
      )
      return true
    })
  }
}

/** Operator recovery freezes this UTC day's allowance rather than trusting stale backup counters. */
export async function holdProviderAfterRestore(pool: Pool, raw: ProviderPolicy) {
  const policy = providerPolicySchema.parse(raw)
  await transaction(pool, policy.bucketId, async (client) => {
    await seed(client, policy)
    await client.query(
      `UPDATE archava_provider.attempts SET state='unknown',failure_code='interrupted',finished_at=clock_timestamp(),
      elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM (clock_timestamp()-started_at))*1000))::integer
      WHERE bucket_id=$1 AND state='pending'`,
      [policy.bucketId],
    )
    await client.query(
      `UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL,
      hold_until=((clock_timestamp() AT TIME ZONE 'UTC')::date+1)::timestamp AT TIME ZONE 'UTC' WHERE bucket_id=$1`,
      [policy.bucketId],
    )
  })
}
export async function readProviderStatus(pool: Pool, raw: ProviderPolicy) {
  const policy = providerPolicySchema.parse(raw)
  return transaction(pool, policy.bucketId, async (client) => {
    const bucket = await seed(client, policy)
    const now = await time(client)
    await reap(client, bucket, now)
    const today =
      (
        await client.query<{ used: number }>(
          "SELECT CASE WHEN usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date) THEN used ELSE 0 END AS used FROM archava_provider.buckets WHERE bucket_id=$1",
          [policy.bucketId, now],
        )
      ).rows[0]?.used ?? 0
    const held = !!bucket.hold_until && bucket.hold_until.getTime() > now.getTime()
    const recent = (
      await client.query<{
        attempt_id: string
        state: string
        started_at: Date
        finished_at: Date | null
        elapsed_ms: number | null
        failure_code: string | null
        http_status: number | null
      }>(
        'SELECT * FROM archava_provider.attempts WHERE bucket_id=$1 ORDER BY started_at DESC,attempt_id LIMIT 20',
        [policy.bucketId],
      )
    ).rows.map((row) => ({
      requestId: row.attempt_id,
      state: row.state,
      startedAt: row.started_at.toISOString(),
      finishedAt: row.finished_at?.toISOString() ?? null,
      elapsedMs: row.elapsed_ms,
      failureCode: row.failure_code,
      httpStatus: row.http_status,
    }))
    return {
      bucketId: policy.bucketId,
      providerId: policy.providerId,
      model: policy.model,
      utcDay: now.toISOString().slice(0, 10),
      dailyLimit: policy.dailyLimit,
      attemptsToday: today,
      remaining: held ? 0 : Math.max(0, policy.dailyLimit - today),
      restoreHoldUntil: held ? bucket.hold_until?.toISOString() : null,
      activeRequestId: bucket.active_attempt,
      recent,
    }
  })
}
