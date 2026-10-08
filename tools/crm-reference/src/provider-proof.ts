import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import type { Pool } from 'pg'
import type { prepareConsumers } from './consumers.js'
import type { ProviderPolicy } from '@archava/provider-runtime'
type Consumers = Awaited<ReturnType<typeof prepareConsumers>>
function required<T>(items: readonly T[], index = 0): T {
  const item = items[index]
  assert.ok(item)
  return item
}
export async function verifyProviderRuntime(
  consumers: Consumers,
  owner: Pool,
  pool: Pool,
  check: (label: string) => void,
) {
  const modules = consumers.providerModules
  const pg = required(consumers.providerPgModules)
  await pg.migrateProviderRuntime(owner)
  await pg.migrateProviderRuntime(owner)
  await pg.grantProviderRuntime(owner, 'crm_app')
  await assert.rejects(() => pg.PostgresProviderAttemptStore.create(owner))
  const stores = await Promise.all(
    consumers.providerPgModules.map((module) => module.PostgresProviderAttemptStore.create(pool)),
  )
  const base: ProviderPolicy = {
    bucketId: 'shared-pilot',
    providerId: 'test-provider',
    model: 'test-model',
    dailyLimit: 3,
    minimumIntervalMs: 0,
    leaseMs: 60000,
  }
  let calls = 0
  let entered = () => {}
  let release = () => {}
  const started = new Promise<void>((done) => {
    entered = done
  })
  const pending = new Promise<void>((done) => {
    release = done
  })
  const firstId = randomUUID()
  const first = required(modules).runProvider({
    store: required(stores),
    policy: base,
    attemptId: firstId,
    invoke: async () => {
      calls++
      entered()
      await pending
      return 'private-result-fixture'
    },
    httpStatus: () => 200,
  })
  await started
  const contenders = await Promise.allSettled(
    Array.from({ length: 8 }, (_, index) =>
      required(modules, index % 2).runProvider({
        store: required(stores, index % 2),
        policy: base,
        attemptId: randomUUID(),
        invoke: () => {
          calls++
          return Promise.resolve('wrong')
        },
      }),
    ),
  )
  assert.ok(contenders.every((result) => result.status === 'rejected'))
  assert.equal(calls, 1)
  release()
  assert.equal(await first, 'private-result-fixture')
  const status = await pg.readProviderStatus(pool, base)
  assert.equal(status.attemptsToday, 1)
  assert.equal(status.recent[0]?.state, 'succeeded')
  assert.equal(status.recent[0]?.httpStatus, 200)
  assert.ok(!JSON.stringify(status).includes('private-result-fixture'))
  check(
    'two installed provider runtimes share one durable admission; eight overlapping calls invoke no duplicate provider',
  )
  const duplicate = await required(stores, 1).reserve(base, firstId)
  assert.ok(!duplicate.allowed && duplicate.code === 'duplicate_attempt')
  await assert.rejects(() => required(stores).reserve({ ...base, dailyLimit: 4 }, randomUUID()))
  assert.equal((await pool.query('SELECT 1 FROM archava_provider.attempts')).rowCount, 0)
  await assert.rejects(() => pool.query('DELETE FROM archava_provider.attempts'))
  await assert.rejects(() =>
    owner.query(
      "UPDATE archava_provider.attempts SET state='failed',failure_code='provider_failed' WHERE bucket_id=$1",
      [base.bucketId],
    ),
  )
  check(
    'policy mismatches, duplicate identities, unscoped reads/deletes and terminal outcome rewrites are refused',
  )
  await assert.rejects(() =>
    required(modules, 1).runProvider({
      store: required(stores, 1),
      policy: base,
      attemptId: randomUUID(),
      invoke: () => {
        calls++
        return Promise.reject(
          Object.assign(new Error('private-provider-fixture'), { code: 'rate_limited' }),
        )
      },
      httpStatus: () => 429,
    }),
  )
  const interruptedId = randomUUID()
  assert.deepEqual(await required(stores).reserve(base, interruptedId), { allowed: true })
  await owner.query(
    "UPDATE archava_provider.buckets SET lease_until=clock_timestamp()-interval '1 second' WHERE bucket_id=$1",
    [base.bucketId],
  )
  const exhausted = await required(stores, 1).reserve(base, randomUUID())
  assert.ok(!exhausted.allowed && exhausted.code === 'daily_limit')
  assert.equal(
    await required(stores).finish(base.bucketId, interruptedId, {
      state: 'succeeded',
      httpStatus: 200,
    }),
    false,
  )
  const final = await pg.readProviderStatus(pool, base)
  assert.equal(final.attemptsToday, 3)
  assert.equal(final.remaining, 0)
  assert.deepEqual(final.recent.map((item) => item.state).sort(), [
    'failed',
    'succeeded',
    'unknown',
  ])
  assert.equal(calls, 2)
  check(
    'failed/interrupted requests remain charged; expiration is committed even on denial and late results cannot overwrite unknown',
  )
  const paced = { ...base, bucketId: 'paced-pilot', dailyLimit: 2, minimumIntervalMs: 60000 }
  await required(modules).runProvider({
    store: required(stores),
    policy: paced,
    attemptId: randomUUID(),
    invoke: () => Promise.resolve('ok'),
  })
  const cooldown = await required(stores, 1).reserve(paced, randomUUID())
  assert.ok(!cooldown.allowed && cooldown.code === 'cooldown' && cooldown.retryAfterMs > 0)
  await owner.query(
    "UPDATE archava_provider.buckets SET usage_day=(clock_timestamp() AT TIME ZONE 'UTC')::date-1,used=2,last_started_at=clock_timestamp()-interval '2 minutes' WHERE bucket_id=$1",
    [paced.bucketId],
  )
  await required(modules, 1).runProvider({
    store: required(stores, 1),
    policy: paced,
    attemptId: randomUUID(),
    invoke: () => Promise.resolve('ok'),
  })
  assert.equal((await pg.readProviderStatus(pool, paced)).attemptsToday, 1)
  check('pacing spans independent consumers and only the authoritative UTC day resets daily usage')
  const rollback = { ...base, bucketId: 'rollback-pilot' }
  await owner.query(
    "CREATE FUNCTION public.provider_fail_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture'; END $$; CREATE TRIGGER provider_fail BEFORE INSERT ON archava_provider.attempts FOR EACH ROW EXECUTE FUNCTION public.provider_fail_insert()",
  )
  await assert.rejects(() =>
    required(modules).runProvider({
      store: required(stores),
      policy: rollback,
      attemptId: randomUUID(),
      invoke: () => {
        calls++
        return Promise.resolve('wrong')
      },
    }),
  )
  assert.equal(calls, 2)
  assert.equal(
    (
      await owner.query('SELECT 1 FROM archava_provider.buckets WHERE bucket_id=$1', [
        rollback.bucketId,
      ])
    ).rowCount,
    0,
  )
  await owner.query(
    'DROP TRIGGER provider_fail ON archava_provider.attempts; DROP FUNCTION public.provider_fail_insert()',
  )
  await pg.holdProviderAfterRestore(owner, base)
  const held = await required(stores).reserve(base, randomUUID())
  assert.ok(!held.allowed && held.code === 'restore_hold')
  assert.ok((await pg.readProviderStatus(pool, base)).restoreHoldUntil)
  await owner.query(
    "UPDATE archava_provider.buckets SET hold_until=clock_timestamp()-interval '1 second',usage_day=(clock_timestamp() AT TIME ZONE 'UTC')::date-1 WHERE bucket_id=$1",
    [base.bucketId],
  )
  await required(modules).runProvider({
    store: required(stores),
    policy: base,
    attemptId: randomUUID(),
    invoke: () => Promise.resolve('next day'),
  })
  const checksum = (
    await owner.query<{ checksum: string }>(
      'SELECT checksum FROM archava_provider.schema_migrations',
    )
  ).rows[0]?.checksum
  assert.ok(checksum)
  await owner.query("UPDATE archava_provider.schema_migrations SET checksum='changed'")
  await assert.rejects(() => pg.migrateProviderRuntime(owner))
  await assert.rejects(() => pg.PostgresProviderAttemptStore.create(pool))
  await owner.query('UPDATE archava_provider.schema_migrations SET checksum=$1', [checksum])
  check(
    'atomic reservation rollback, conservative restoration hold and migration checksum enforcement',
  )
  return { policy: base, used: 1 }
}
