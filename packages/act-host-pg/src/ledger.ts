import { randomUUID } from 'node:crypto'
import {
  ActHostAttemptError,
  isAttemptReference,
  readDurableAttempt,
  type ActHostAttemptIdentity,
  type ActHostAttemptRecord,
  type AtomicActHostAttemptLedger,
  type DurableActHostAttemptRecord,
} from '@archava/act-host'
import type { SqlClient, SqlParameter } from '@archava/act-storage'
import { actHostMigration } from './migration.js'

const text = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= 256
function milliseconds(value: unknown): number {
  if (typeof value === 'string' && /^\d+$/.test(value)) value = Number(value)
  if (
    typeof value !== 'number' ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > 8640000000000000
  )
    throw new ActHostAttemptError('malformed')
  return value
}

export class PostgresActHostAttemptLedger implements AtomicActHostAttemptLedger {
  readonly ledgerId = 'postgres-act-host-intents@1'
  readonly #client: SqlClient
  readonly #scope: Readonly<{ tenantId: string; sessionId: string }>
  private constructor(client: SqlClient, scope: Readonly<{ tenantId: string; sessionId: string }>) {
    this.#client = client
    this.#scope = scope
  }
  static async create(client: SqlClient, scope: { tenantId: string; sessionId: string }) {
    try {
      if (!text(scope.tenantId) || !text(scope.sessionId))
        throw new ActHostAttemptError('malformed')
      const safe = (
        await client.run({
          sql: "SELECT NOT(rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_act_host'::regnamespace AND relowner=pg_roles.oid)) AS safe FROM pg_roles WHERE rolname=current_user",
          params: [],
        })
      )[0]
      if (safe?.safe !== true) throw new ActHostAttemptError('unavailable')
      const installed = await client.run({
        sql: 'SELECT id,checksum FROM archava_act_host.schema_migrations ORDER BY id',
        params: [],
      })
      const expected = await actHostMigration()
      if (
        installed.length !== 1 ||
        installed[0]?.id !== expected.id ||
        installed[0]?.checksum !== expected.checksum
      )
        throw new ActHostAttemptError('unavailable')
      const isolation = (
        await client.run({
          sql: "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_act_host'::regnamespace AND relrowsecurity AND relforcerowsecurity",
          params: [],
        })
      )[0]
      if (isolation?.count !== 3) throw new ActHostAttemptError('unavailable')
      return new PostgresActHostAttemptLedger(client, Object.freeze({ ...scope }))
    } catch (error) {
      if (error instanceof ActHostAttemptError) throw error
      throw new ActHostAttemptError('unavailable')
    }
  }
  #bound(record: ActHostAttemptIdentity) {
    if (
      record.tenantId !== this.#scope.tenantId ||
      record.sessionId !== this.#scope.sessionId ||
      !text(record.actionId) ||
      !/^[a-f0-9]{64}$/.test(record.inputDigest)
    )
      throw new ActHostAttemptError('transition_refused')
  }
  async #call(sql: string, params: readonly SqlParameter[]) {
    try {
      const rows = await this.#client.run({
        sql,
        params: [this.#scope.tenantId, this.#scope.sessionId, ...params],
      })
      if (rows.length !== 1 || !rows[0] || !Object.hasOwn(rows[0], 'result'))
        throw new ActHostAttemptError('malformed')
      return rows[0].result
    } catch (error) {
      if (error instanceof ActHostAttemptError) throw error
      if (error instanceof Error && error.message === 'host_attempt_transition_refused')
        throw new ActHostAttemptError('transition_refused')
      throw new ActHostAttemptError('unavailable')
    }
  }
  #record(value: unknown): DurableActHostAttemptRecord | null {
    if (value === null) return null
    if (typeof value !== 'object' || Array.isArray(value))
      throw new ActHostAttemptError('malformed')
    const row = value as Record<string, unknown>
    const record = readDurableAttempt({
      tenantId: row.tenant_id,
      sessionId: row.session_id,
      actionId: row.action_id,
      inputDigest: row.input_digest,
      intentId: row.intent_id,
      generation: row.generation,
      phase: row.phase,
      previousIntentId: row.previous_intent_id,
      occurredAt: new Date(milliseconds(row.occurred_at_ms)).toISOString(),
      challengeId: row.challenge_id,
      expiresAt: milliseconds(row.expires_at_ms),
    })
    this.#bound(record)
    return record
  }
  async open(record: ActHostAttemptRecord, previousIntentId?: string) {
    this.#bound(record)
    if (
      !isAttemptReference(record.challengeId) ||
      (previousIntentId !== undefined && !isAttemptReference(previousIntentId)) ||
      !Number.isSafeInteger(Date.parse(record.occurredAt)) ||
      new Date(record.occurredAt).toISOString() !== record.occurredAt
    )
      throw new ActHostAttemptError('malformed')
    const canonical = this.#record(
      await this.#call(
        'SELECT archava_act_host.open_attempt($1,$2,$3,$4,$5,$6,$7,$8,$9) AS result',
        [
          record.actionId,
          record.inputDigest,
          randomUUID(),
          milliseconds(Date.parse(record.occurredAt)),
          record.challengeId,
          milliseconds(record.expiresAt),
          previousIntentId ?? null,
        ],
      ),
    )
    if (
      !canonical ||
      canonical.actionId !== record.actionId ||
      canonical.inputDigest !== record.inputDigest ||
      canonical.challengeId !== record.challengeId ||
      canonical.expiresAt !== record.expiresAt
    )
      throw new ActHostAttemptError('malformed')
    return canonical
  }
  async save(record: ActHostAttemptRecord) {
    await this.open(record)
  }
  async find(identity: ActHostAttemptIdentity) {
    this.#bound(identity)
    const row = this.#record(
      await this.#call('SELECT archava_act_host.find_head($1,$2,$3,$4) AS result', [
        identity.actionId,
        identity.inputDigest,
      ]),
    )
    if (row && (row.actionId !== identity.actionId || row.inputDigest !== identity.inputDigest))
      throw new ActHostAttemptError('malformed')
    return row
  }
  async findByChallenge(challengeId: string) {
    return await this.#lookup(challengeId, 'challenge')
  }
  async findByIntent(intentId: string) {
    return await this.#lookup(intentId, 'intent')
  }
  async #lookup(ref: string, kind: 'challenge' | 'intent') {
    if (!isAttemptReference(ref)) return null
    const row = this.#record(
      await this.#call('SELECT archava_act_host.lookup_attempt($1,$2,$3,$4) AS result', [
        ref,
        kind,
      ]),
    )
    if (row && (kind === 'challenge' ? row.challengeId !== ref : row.intentId !== ref))
      throw new ActHostAttemptError('malformed')
    return row
  }
  async #transition(sql: string, params: readonly SqlParameter[]) {
    const result = await this.#call(sql, params)
    if (typeof result !== 'boolean') throw new ActHostAttemptError('malformed')
    return result
  }
  async claim(intentId: string, claimId: string) {
    if (!isAttemptReference(intentId) || !isAttemptReference(claimId))
      throw new ActHostAttemptError('malformed')
    return await this.#transition('SELECT archava_act_host.claim_attempt($1,$2,$3,$4) AS result', [
      intentId,
      claimId,
    ])
  }
  async settle(intentId: string, claimId: string, phase: 'succeeded' | 'unknown') {
    if (
      !isAttemptReference(intentId) ||
      !isAttemptReference(claimId) ||
      !['succeeded', 'unknown'].includes(phase)
    )
      throw new ActHostAttemptError('malformed')
    return await this.#transition(
      'SELECT archava_act_host.settle_attempt($1,$2,$3,$4,$5) AS result',
      [intentId, claimId, phase],
    )
  }
  async cancel(intentId: string) {
    if (!isAttemptReference(intentId)) throw new ActHostAttemptError('malformed')
    return await this.#transition('SELECT archava_act_host.cancel_attempt($1,$2,$3) AS result', [
      intentId,
    ])
  }
  async reconcileSuccess(intentId: string, proofDigest: string) {
    if (!isAttemptReference(intentId) || !/^[a-f0-9]{64}$/.test(proofDigest))
      throw new ActHostAttemptError('malformed')
    return await this.#transition(
      'SELECT archava_act_host.reconcile_attempt($1,$2,$3,$4) AS result',
      [intentId, proofDigest],
    )
  }
}
