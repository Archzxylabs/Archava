import { readFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import { CrmError } from '@archava/crm'
import type { CrmApprovedMutationVerifier } from '@archava/crm-pg'
import { PostgresActHostAttemptLedger } from '@archava/act-host-pg'
import {
  PostgresChallengeStore,
  lookupChallengeStatement,
  readChallengeRow,
} from '@archava/act-confirmation-pg'
import { ConfirmationService, type ConfirmationReceipt } from '@archava/act-confirmation'
import type { SqlClient } from '@archava/act-storage'
import {
  NativeCrmActHost,
  nativeSessionId,
  NATIVE_FOLLOWUP_ACTION,
  type NativeCrmActOptions,
} from './native-host.js'
import { actionControlSchema } from './contracts.js'

const migrationId = '001_native_confirmations'
export async function nativeCrmMigration() {
  const sql = await readFile(
    new URL('../migrations/001_native_confirmations.sql', import.meta.url),
    'utf8',
  )
  return { id: migrationId, sql, checksum: createHash('sha256').update(sql).digest('hex') }
}
export async function migrateNativeCrmAct(pool: Pool) {
  const migration = await nativeCrmMigration(),
    client = await pool.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT pg_advisory_xact_lock(hashtext('archava_native_crm_act'))")
    const installed = (
      await client.query<{ exists: boolean }>(
        "SELECT to_regclass('archava_crm_act.schema_migrations') IS NOT NULL AS exists",
      )
    ).rows[0]?.exists
    if (installed) {
      const rows = (
        await client.query<{ id: string; checksum: string }>(
          'SELECT id,checksum FROM archava_crm_act.schema_migrations ORDER BY id',
        )
      ).rows
      if (
        rows.length !== 1 ||
        rows[0]?.id !== migration.id ||
        rows[0]?.checksum !== migration.checksum
      )
        throw new Error('Unsupported native action migration ledger')
    } else {
      await client.query(migration.sql)
      await client.query('INSERT INTO archava_crm_act.schema_migrations VALUES($1,$2)', [
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
export async function grantNativeCrmAct(pool: Pool, role: string) {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid native action runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_crm_act TO "${role}"`)
  await pool.query(`GRANT SELECT ON archava_crm_act.schema_migrations TO "${role}"`)
  await pool.query(
    `GRANT SELECT,INSERT,UPDATE ON archava_crm_act.confirmation_challenges TO "${role}"`,
  )
}
function transactionClient(client: PoolClient, tenant: string, session: string): SqlClient {
  return {
    storeId: 'native-crm-transaction',
    capabilities: { transactions: true },
    run: async (statement) => {
      await client.query(
        "SELECT set_config('archava.tenant_id',$1,true),set_config('archava.act_session',$2,true),set_config('search_path','archava_crm_act,pg_catalog',true)",
        [tenant, session],
      )
      return (await client.query<Record<string, unknown>>(statement.sql, [...statement.params]))
        .rows
    },
  }
}
export function nativeSqlClient(pool: Pool, tenant: string, session: string): SqlClient {
  return {
    storeId: 'native-crm-postgres',
    capabilities: { transactions: true },
    run: async (statement) => {
      const client = await pool.connect()
      try {
        await client.query('BEGIN')
        const rows = await transactionClient(client, tenant, session).run(statement)
        await client.query('COMMIT')
        return rows
      } catch (error) {
        await client.query('ROLLBACK')
        throw error
      } finally {
        client.release()
      }
    },
  }
}
const receiptSchema = z
  .object({
    receiptId: z.string().regex(/^[a-f0-9]{32}$/),
    challengeId: z.string().regex(/^[a-f0-9]{32}$/),
    tenantId: z.string().min(1).max(100),
    sessionId: z.string().min(1).max(256),
    actionId: z.literal(NATIVE_FOLLOWUP_ACTION),
    inputDigest: z.string().regex(/^[a-f0-9]{64}$/),
    bindingDigest: z.string().regex(/^[a-f0-9]{64}$/),
    receiptDigest: z.string().regex(/^[a-f0-9]{64}$/),
    issuedAt: z.number().int().nonnegative(),
    expiresAt: z.number().int().positive(),
    confirmedAt: z.number().int().nonnegative(),
  })
  .strict()
const approvalSchema = z.object({ intentId: z.uuid(), receipt: receiptSchema }).strict()
export function createNativeCrmVerifier(
  options: Pick<NativeCrmActOptions, 'serviceId' | 'confirmationKey' | 'control'>,
): CrmApprovedMutationVerifier {
  const key = Uint8Array.from(options.confirmationKey)
  if (key.byteLength < 32)
    throw new TypeError('Native approval requires a stable server key of at least 32 bytes.')
  return {
    verify: async (client, tenant, principal, mutation, raw) => {
      const parsed = approvalSchema.safeParse(raw)
      if (
        !parsed.success ||
        principal.kind !== 'service' ||
        principal.id !== options.serviceId ||
        mutation.command.operation !== 'followup.create' ||
        mutation.requestId !== parsed.data.intentId
      )
        return false
      const control = actionControlSchema.parse(await options.control())
      if (control.held) return false
      const session = nativeSessionId(
          key,
          tenant,
          principal.onBehalfOf,
          principal.id,
          control.epoch,
        ),
        receipt: ConfirmationReceipt = parsed.data.receipt
      if (
        receipt.tenantId !== tenant ||
        receipt.sessionId !== session ||
        receipt.confirmedAt > Date.now() ||
        receipt.expiresAt <= Date.now()
      )
        return false
      const sql = transactionClient(client, tenant, session)
      const confirmations = new ConfirmationService({
        store: new PostgresChallengeStore(sql),
        digestKey: key,
        clock: Date.now,
      })
      const inputs = {
        contactId: mutation.command.contactId,
        contactRevision: mutation.command.contactRevision,
        summary: mutation.command.summary,
        dueAt: mutation.command.dueAt,
      }
      const description = confirmations.describeBinding({
        tenantId: tenant,
        sessionId: session,
        actionId: NATIVE_FOLLOWUP_ACTION,
        inputs,
      })
      if (
        description.status !== 'bound' ||
        description.inputDigest !== receipt.inputDigest ||
        description.bindingDigest !== receipt.bindingDigest ||
        !confirmations.verifyReceipt(receipt)
      )
        return false
      const statement = lookupChallengeStatement(receipt.challengeId)
      const rows = await sql.run({ ...statement, sql: statement.sql + ' FOR SHARE' })
      if (rows.length !== 1 || !rows[0]) return false
      const row = readChallengeRow(rows[0], receipt.challengeId)
      if (
        row.status !== 'record' ||
        row.challenge.state !== 'consumed' ||
        row.challenge.tenantId !== tenant ||
        row.challenge.sessionId !== session ||
        row.challenge.actionId !== NATIVE_FOLLOWUP_ACTION ||
        row.challenge.bindingDigest !== receipt.bindingDigest ||
        row.challenge.issuedAt !== receipt.issuedAt ||
        row.challenge.expiresAt !== receipt.expiresAt ||
        row.challenge.consumedAt !== receipt.confirmedAt
      )
        return false
      const ledger = await PostgresActHostAttemptLedger.create(sql, {
        tenantId: tenant,
        sessionId: session,
      })
      return ledger.verifyExecution(parsed.data.intentId, receipt.challengeId, receipt.inputDigest)
    },
  }
}
export async function createPostgresNativeCrmHost(
  pool: Pool,
  options: Omit<NativeCrmActOptions, 'attempts' | 'confirmations'>,
) {
  const role = (
    await pool.query<{ safe: boolean }>(
      "SELECT NOT(rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_crm_act'::regnamespace AND relowner=pg_roles.oid)) AS safe FROM pg_roles WHERE rolname=current_user",
    )
  ).rows[0]
  const ledger = (
    await pool.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_crm_act.schema_migrations ORDER BY id',
    )
  ).rows
  const migration = await nativeCrmMigration()
  const isolation = (
    await pool.query<{ count: number }>(
      "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_crm_act'::regnamespace AND relrowsecurity AND relforcerowsecurity",
    )
  ).rows[0]
  if (
    role?.safe !== true ||
    ledger.length !== 1 ||
    ledger[0]?.id !== migration.id ||
    ledger[0]?.checksum !== migration.checksum ||
    isolation?.count !== 1
  )
    throw new CrmError('unavailable', 'Native action storage is not ready.')
  await PostgresActHostAttemptLedger.create(
    nativeSqlClient(pool, options.tenantId, 'native-startup'),
    { tenantId: options.tenantId, sessionId: 'native-startup' },
  )
  return new NativeCrmActHost({
    ...options,
    attempts: (sessionId) =>
      PostgresActHostAttemptLedger.create(nativeSqlClient(pool, options.tenantId, sessionId), {
        tenantId: options.tenantId,
        sessionId,
      }),
    confirmations: (sessionId) =>
      Promise.resolve(
        new ConfirmationService({
          store: new PostgresChallengeStore(nativeSqlClient(pool, options.tenantId, sessionId)),
          digestKey: options.confirmationKey,
          clock: options.clock,
        }),
      ),
  })
}
