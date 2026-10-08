import { describe, it, expect, vi } from 'vitest'
import { GeminiCrmPlanner, GEMINI_CRM_MODEL, geminiPlannerConfigSchema } from '../src/server.js'
import { GEMINI_PILOT_CASES } from '../src/pilot.js'

const key = 'fixture-gemini-key-marker-000000'
const utterance = 'Bisa tampilkan daftar kontak CRM?'
const wire = { operation: 'contacts.list', search: '', query: null, stage: null }
function response(plan: unknown = wire, finishReason = 'STOP') {
  return new Response(
    JSON.stringify({
      candidates: [{ finishReason, content: { parts: [{ text: JSON.stringify(plan) }] } }],
    }),
  )
}
function fixture(transport: typeof fetch, maxRequests = 30, timeoutMs = 1000) {
  return new GeminiCrmPlanner({
    apiKey: key,
    projectTier: 'free',
    transport,
    maxRequests,
    timeoutMs,
    minimumIntervalMs: 0,
  })
}
describe('Gemini synthetic-only server boundary', () => {
  it('uses one fixed HTTPS endpoint, header-only credentials and no provider tools', async () => {
    const transport = vi.fn<typeof fetch>(() => Promise.resolve(response()))
    const planner = fixture(transport)
    expect(await planner.plan(utterance)).toEqual({ operation: 'contacts.list', search: '' })
    const [url, init] = transport.mock.calls[0] ?? []
    expect(url).toBe(
      `https://generativelanguage.googleapis.com/v1beta/models/${GEMINI_CRM_MODEL}:generateContent`,
    )
    expect(init?.redirect).toBe('error')
    expect(new Headers(init?.headers).get('x-goog-api-key')).toBe(key)
    if (typeof url !== 'string' || typeof init?.body !== 'string')
      throw new Error('Expected JSON request')
    expect(init.body).not.toContain(key)
    expect(url).not.toContain(key)
    const body = JSON.parse(init.body) as Record<string, unknown>
    expect(body).not.toHaveProperty('tools')
    expect(JSON.stringify(planner)).not.toContain(key)
    expect(JSON.stringify(planner.snapshot())).not.toContain(key)
  })
  it('keeps arbitrary personal text and unknown fields away from the free provider', async () => {
    const transport = vi.fn<typeof fetch>()
    const planner = fixture(transport)
    await expect(planner.plan('Ringkas customer pribadi@example.test')).rejects.toMatchObject({
      code: 'synthetic_only',
    })
    expect(transport).not.toHaveBeenCalled()
    expect(() =>
      geminiPlannerConfigSchema.parse({
        provider: 'gemini',
        model: GEMINI_CRM_MODEL,
        projectTier: 'paid',
        enabled: false,
        apiKey: '',
      }),
    ).toThrow()
    expect(() =>
      geminiPlannerConfigSchema.parse({
        provider: 'gemini',
        model: GEMINI_CRM_MODEL,
        projectTier: 'free',
        enabled: true,
        apiKey: '',
      }),
    ).toThrow()
  })
  it.each(GEMINI_PILOT_CASES)('accepts only the declared safe plan for $id', async (item) => {
    const expected = item.expected
    const result = {
      operation: expected.operation,
      search: 'search' in expected ? expected.search : null,
      query: 'query' in expected ? (expected.query ?? null) : null,
      stage: 'stage' in expected ? (expected.stage ?? null) : null,
    }
    const planner = fixture(() => Promise.resolve(response(result)))
    expect(await planner.plan(item.utterance)).toEqual(expected)
  })
  it.each([
    { ...wire, operation: 'followup.create' },
    { ...wire, tenantId: 'other' },
    { ...wire, role: 'owner' },
    { ...wire, operation: 'contact.summary', search: null, query: 'Invented person' },
    { ...wire, stage: 'won' },
  ])('rejects forged or mismatched plans before returning a tool request', async (plan) => {
    await expect(
      fixture(() => Promise.resolve(response(plan)))
        .plan(utterance)
        .catch((error: unknown) =>
          error && typeof error === 'object' && 'code' in error ? String(error.code) : 'unknown',
        ),
    ).resolves.toMatch(/invalid_response|plan_mismatch/)
  })
  it('rejects partial, oversized, malformed and multiple candidate responses without retry', async () => {
    for (const reply of [
      response(wire, 'MAX_TOKENS'),
      new Response('{'),
      new Response('x'.repeat(32769)),
      new Response(JSON.stringify({ candidates: [] })),
    ]) {
      const transport = vi.fn<typeof fetch>(() => Promise.resolve(reply))
      await expect(fixture(transport).plan(utterance)).rejects.toMatchObject({
        code: 'invalid_response',
      })
      expect(transport).toHaveBeenCalledTimes(1)
    }
  })
  it('bounds simultaneous calls, timeouts and total request budget', async () => {
    const hanging = vi.fn<typeof fetch>(() => new Promise<Response>(() => {}))
    const planner = fixture(hanging, 1, 10)
    const pending = planner.plan(utterance)
    await expect(planner.plan(utterance)).rejects.toMatchObject({ code: 'busy' })
    await expect(pending).rejects.toMatchObject({ code: 'timeout' })
    await expect(planner.plan(utterance)).rejects.toMatchObject({ code: 'budget_exhausted' })
    expect(hanging).toHaveBeenCalledTimes(1)
  })
  it('does not retry quota failures or disclose credential-bearing transport errors', async () => {
    const quota = vi.fn<typeof fetch>(() =>
      Promise.resolve(new Response('private-provider-body', { status: 429 })),
    )
    await expect(fixture(quota).plan(utterance)).rejects.toMatchObject({ code: 'rate_limited' })
    expect(quota).toHaveBeenCalledTimes(1)
    const failed = fixture(() => Promise.reject(new Error(key)))
    try {
      await failed.plan(utterance)
    } catch (error) {
      expect(String(error)).not.toContain(key)
    }
  })
  it('drops a late provider body after timeout and never counts it as a successful plan', async () => {
    let controller: ReadableStreamDefaultController<Uint8Array> | undefined
    const stream = new ReadableStream<Uint8Array>({
      start(value) {
        controller = value
      },
    })
    const planner = fixture(() => Promise.resolve(new Response(stream)), 1, 10)
    const pending = planner.plan(utterance)
    await expect(pending).rejects.toMatchObject({ code: 'timeout' })
    controller?.close()
    await new Promise((done) => setTimeout(done, 0))
    expect(planner.snapshot()).toMatchObject({
      requests: 1,
      successes: 0,
      active: false,
      lastFailure: 'timeout',
    })
  })
})
