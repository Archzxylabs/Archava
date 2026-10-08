import { z } from 'zod'

export const budgetIdSchema = z.string().regex(/^[a-z][a-z0-9_-]{0,79}$/)
export const providerPolicySchema = z
  .object({
    bucketId: budgetIdSchema,
    providerId: z.string().regex(/^[a-z][a-z0-9_.-]{0,79}$/),
    model: z.string().regex(/^[a-z][a-z0-9_.-]{0,79}$/),
    dailyLimit: z.number().int().min(1).max(1000),
    minimumIntervalMs: z.number().int().min(0).max(60000),
    leaseMs: z.number().int().min(1000).max(120000),
  })
  .strict()
export type ProviderPolicy = z.infer<typeof providerPolicySchema>
export const failureCodeSchema = z.enum([
  'provider_failed',
  'timeout',
  'provider_unavailable',
  'invalid_response',
  'rate_limited',
  'plan_mismatch',
  'synthetic_only',
  'budget_exhausted',
  'busy',
  'interrupted',
])
export type ProviderFailureCode = z.infer<typeof failureCodeSchema>
export const completionSchema = z.discriminatedUnion('state', [
  z
    .object({
      state: z.literal('succeeded'),
      httpStatus: z.number().int().min(100).max(599).nullable(),
    })
    .strict(),
  z
    .object({
      state: z.literal('failed'),
      failureCode: failureCodeSchema,
      httpStatus: z.number().int().min(100).max(599).nullable(),
    })
    .strict(),
])
export type ProviderCompletion = z.infer<typeof completionSchema>
export type Admission =
  | { allowed: true }
  | {
      allowed: false
      code: 'daily_limit' | 'busy' | 'cooldown' | 'restore_hold' | 'duplicate_attempt'
      retryAfterMs: number
    }
export interface ProviderAttemptStore {
  reserve(policy: ProviderPolicy, attemptId: string): Promise<Admission>
  finish(bucketId: string, attemptId: string, completion: ProviderCompletion): Promise<boolean>
}
export class ProviderAdmissionError extends Error {
  constructor(
    readonly code: Exclude<Admission, { allowed: true }>['code'],
    readonly retryAfterMs: number,
  ) {
    super('Provider request is temporarily limited.')
    this.name = 'ProviderAdmissionError'
  }
}
export class ProviderRunError extends Error {
  constructor(readonly code: 'provider_failed' | 'outcome_unknown') {
    super('Provider result could not be verified.')
    this.name = 'ProviderRunError'
  }
}

/** No prompt/result/record/identity is persisted. The store receives safe metadata only. */
export async function runProvider<T>(options: {
  store: ProviderAttemptStore
  policy: ProviderPolicy
  attemptId: string
  invoke: () => Promise<T>
  httpStatus?: () => number | null
}): Promise<T> {
  const policy = providerPolicySchema.parse(options.policy)
  const attemptId = z.uuid().parse(options.attemptId)
  let admission: Admission
  try {
    admission = await options.store.reserve(policy, attemptId)
  } catch {
    throw new ProviderRunError('outcome_unknown')
  }
  if (!admission.allowed) throw new ProviderAdmissionError(admission.code, admission.retryAfterMs)
  const status = () => {
    try {
      const value = options.httpStatus?.() ?? null
      const parsed = z.number().int().min(100).max(599).nullable().safeParse(value)
      return parsed.success ? parsed.data : null
    } catch {
      return null
    }
  }
  let result: T
  try {
    result = await options.invoke()
  } catch (error) {
    const parsed = failureCodeSchema.safeParse(
      error && typeof error === 'object' && 'code' in error ? error.code : null,
    )
    try {
      const recorded = await options.store.finish(policy.bucketId, attemptId, {
        state: 'failed',
        failureCode: parsed.success ? parsed.data : 'provider_failed',
        httpStatus: status(),
      })
      if (!recorded) throw new ProviderRunError('outcome_unknown')
    } catch {
      throw new ProviderRunError('outcome_unknown')
    }
    throw new ProviderRunError('provider_failed')
  }
  try {
    if (
      !(await options.store.finish(policy.bucketId, attemptId, {
        state: 'succeeded',
        httpStatus: status(),
      }))
    )
      throw new ProviderRunError('outcome_unknown')
  } catch {
    throw new ProviderRunError('outcome_unknown')
  }
  return result
}
