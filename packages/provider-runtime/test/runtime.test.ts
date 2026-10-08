import { describe, it, expect, vi } from 'vitest'
import { randomUUID } from 'node:crypto'
import { runProvider, type ProviderAttemptStore, type Admission } from '../src/index.js'
const policy = {
  bucketId: 'test-budget',
  providerId: 'test-provider',
  model: 'test-model',
  dailyLimit: 30,
  minimumIntervalMs: 15000,
  leaseMs: 60000,
}
function fixture(admission: Admission = { allowed: true }, recorded = true) {
  const store = {
    reserve: vi.fn(() => Promise.resolve(admission)),
    finish: vi.fn(() => Promise.resolve(recorded)),
  } satisfies ProviderAttemptStore
  return { store, options: { store, policy, attemptId: randomUUID() } }
}
describe('provider runtime admission and metadata boundary', () => {
  it.each(['daily_limit', 'busy', 'cooldown', 'restore_hold', 'duplicate_attempt'] as const)(
    'refuses %s before invoking a provider',
    async (code) => {
      const { options, store } = fixture({ allowed: false, code, retryAfterMs: 12000 })
      const invoke = vi.fn(() => Promise.resolve('result'))
      await expect(runProvider({ ...options, invoke })).rejects.toMatchObject({
        code,
        retryAfterMs: 12000,
      })
      expect(invoke).not.toHaveBeenCalled()
      expect(store.finish).not.toHaveBeenCalled()
    },
  )
  it('returns the result only after durable completion and never passes its contents to the ledger', async () => {
    const { options, store } = fixture()
    const result = { privateRecord: 'confidential-fixture', apiKey: 'secret-fixture' }
    expect(
      await runProvider({
        ...options,
        invoke: () => Promise.resolve(result),
        httpStatus: () => 200,
      }),
    ).toBe(result)
    expect(store.finish).toHaveBeenCalledWith(policy.bucketId, options.attemptId, {
      state: 'succeeded',
      httpStatus: 200,
    })
    expect(JSON.stringify(vi.mocked(store.reserve).mock.calls)).not.toContain(
      'confidential-fixture',
    )
    expect(JSON.stringify(vi.mocked(store.finish).mock.calls)).not.toContain('secret-fixture')
  })
  it('masks arbitrary provider errors and records only an allowed code', async () => {
    const { options, store } = fixture()
    await expect(
      runProvider({ ...options, invoke: () => Promise.reject(new Error('private-key-fixture')) }),
    ).rejects.toMatchObject({
      message: 'Provider result could not be verified.',
      code: 'provider_failed',
    })
    expect(store.finish).toHaveBeenCalledWith(policy.bucketId, options.attemptId, {
      state: 'failed',
      failureCode: 'provider_failed',
      httpStatus: null,
    })
  })
  it('records known timeout/HTTP metadata without a response body', async () => {
    const { options, store } = fixture()
    await expect(
      runProvider({
        ...options,
        invoke: () => Promise.reject(Object.assign(new Error('confidential'), { code: 'timeout' })),
        httpStatus: () => 429,
      }),
    ).rejects.toMatchObject({ code: 'provider_failed' })
    expect(store.finish).toHaveBeenCalledWith(policy.bucketId, options.attemptId, {
      state: 'failed',
      failureCode: 'timeout',
      httpStatus: 429,
    })
  })
  it('does not invoke after ambiguous reservation and does not release unrecorded results', async () => {
    const { options, store } = fixture()
    vi.mocked(store.reserve).mockRejectedValueOnce(new Error('private-db-fixture'))
    const invoke = vi.fn(() => Promise.resolve('result'))
    await expect(runProvider({ ...options, invoke })).rejects.toMatchObject({
      code: 'outcome_unknown',
    })
    expect(invoke).not.toHaveBeenCalled()
    vi.mocked(store.finish).mockRejectedValueOnce(new Error('private-db-fixture'))
    await expect(runProvider({ ...options, invoke })).rejects.toMatchObject({
      code: 'outcome_unknown',
    })
  })
  it('rejects completion of an expired lease without returning a model plan', async () => {
    const { options } = fixture({ allowed: true }, false)
    await expect(
      runProvider({ ...options, invoke: () => Promise.resolve('late result') }),
    ).rejects.toMatchObject({ code: 'outcome_unknown' })
  })
  it('rejects forged policy/attempt fields before reserving', async () => {
    const { options, store } = fixture()
    await expect(
      runProvider({ ...options, attemptId: 'forged', invoke: () => Promise.resolve('result') }),
    ).rejects.toThrow()
    await expect(
      runProvider({
        ...options,
        policy: { ...policy, dailyLimit: 0 },
        invoke: () => Promise.resolve('result'),
      }),
    ).rejects.toThrow()
    expect(store.reserve).not.toHaveBeenCalled()
  })
})
