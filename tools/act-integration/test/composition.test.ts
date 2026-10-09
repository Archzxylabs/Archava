import { describe, it, expect } from 'vitest'
import { DurableActHost } from '@archava/act-host'
import { actFixture } from '../src/fixture.js'

async function issue(
  f: ReturnType<typeof actFixture>,
  actionId = 'booking.create',
  inputs: Readonly<Record<string, unknown>> = f.bookingInput,
) {
  const envelope = { utterance: 'Synthetic request', action: { actionId, inputs } }
  const reviewed = await f.host.review({ envelope, graph: f.graph })
  if (reviewed.status !== 'challenge_issued' || !reviewed.intentId)
    throw new Error('Challenge not issued')
  const presentation = {
    graph: f.graph,
    envelope: {
      ...envelope,
      confirmation: {
        challengeId: reviewed.challenge.challengeId,
        token: reviewed.challenge.token,
      },
    },
  }
  return { reviewed: { ...reviewed, intentId: reviewed.intentId }, presentation }
}
describe('public Act packages composed end to end, offline', () => {
  it('recovers an unknown email only through a positive read bound to the original validated inputs', async () => {
    const f = actFixture()
    f.unknownMail()
    const flow = await issue(f, 'email.send', f.emailInput)
    await f.host.present(flow.presentation)
    const id = flow.reviewed.intentId,
      count = f.challengeCount()
    expect((await f.host.reconcile(id, f.graph)).phase).toBe('unknown')
    expect(f.statusCalls).toHaveLength(0)
    expect((await f.host.reconcile(id, f.graph, f.emailInput)).phase).toBe('unknown')
    expect(f.statusCalls).toHaveLength(1)
    f.confirmMailAcceptance()
    await expect(
      f.host.reconcile(id, f.graph, {
        ...f.emailInput,
        to: 'another-recipient@example.test',
      }),
    ).rejects.toMatchObject({ code: 'visitor_challenge_mismatch' })
    expect(f.statusCalls).toHaveLength(1)
    const restarted = new DurableActHost(f.options)
    expect((await restarted.reconcile(id, f.graph, f.emailInput)).phase).toBe('succeeded')
    expect(f.statusCalls).toHaveLength(2)
    expect(f.mailCalls).toHaveLength(1)
    expect(f.executionCalls).toHaveLength(1)
    expect(f.challengeCount()).toBe(count)
    expect([...f.emailRows.values()][0]?.state).toBe('accepted')
    const retry = await issue(f, 'email.send', f.emailInput)
    expect((await restarted.present(retry.presentation)).execution).toBe('succeeded')
    expect(f.mailCalls).toHaveLength(1)
  })
  it('recovery of a missing attempt or wrong tenant cannot claim or perform a side effect', async () => {
    const f = actFixture()
    for (const action of ['booking.create', 'email.send']) {
      const request = {
        tenantId: f.options.tenantId,
        sessionId: f.options.sessionId,
        action,
        inputs: action === 'email.send' ? f.emailInput : f.bookingInput,
        idempotencyKey: 'never-issued',
      }
      expect((await f.executor.recover(request)).status).toBe('failed')
      expect((await f.executor.recover({ ...request, tenantId: 'foreign' })).status).toBe('failed')
    }
    expect(f.bookingRows.size + f.emailRows.size).toBe(0)
    expect(f.bookingCalls.length + f.mailCalls.length + f.statusCalls.length).toBe(0)
  })
  it('recovery refuses an outbox fingerprint conflict before consulting the provider', async () => {
    const f = actFixture()
    f.unknownMail()
    const flow = await issue(f, 'email.send', f.emailInput)
    await f.host.present(flow.presentation)
    const row = required([...f.emailRows.entries()][0])
    f.emailRows.set(row[0], { ...row[1], fingerprint: 'f'.repeat(64) })
    f.confirmMailAcceptance()
    expect((await f.host.reconcile(flow.reviewed.intentId, f.graph, f.emailInput)).phase).toBe(
      'unknown',
    )
    expect(f.statusCalls).toHaveLength(0)
    expect(f.mailCalls).toHaveLength(1)
  })
  it('runs real policy/consent/validation/turn/composition/replay for a booking, with safe output', async () => {
    const f = actFixture(),
      flow = await issue(f)
    expect((await f.host.present(flow.presentation)).execution).toBe('succeeded')
    expect(f.bookingCalls).toHaveLength(1)
    expect([...f.bookingRows.values()][0]).toMatchObject({
      state: 'confirmed',
      bookingReference: 'authoritative_booking_123',
    })
    const retry = await issue(f)
    expect(retry.reviewed.intentId).toBe(flow.reviewed.intentId)
    const result = await f.host.present(retry.presentation)
    expect(result.execution).toBe('succeeded')
    expect(f.bookingCalls).toHaveLength(1)
    expect(JSON.stringify(result)).not.toContain(flow.reviewed.challenge.token)
    expect(JSON.stringify(result)).not.toContain(required(f.executionCalls[0]).idempotencyKey)
  })
  it('runs the real email outbox and Postmark gateway through a mocked transport; acceptance is not delivery', async () => {
    const f = actFixture(),
      flow = await issue(f, 'email.send', f.emailInput)
    const result = await f.host.present(flow.presentation)
    expect(result.execution).toBe('succeeded')
    expect(f.mailCalls).toHaveLength(1)
    expect([...f.emailRows.values()][0]).toMatchObject({
      state: 'accepted',
      providerMessageId: '00000000-0000-4000-8000-000000000001',
    })
    const wire = JSON.parse(required(f.mailCalls[0]).body) as { Metadata: Record<string, string> }
    expect(JSON.stringify(wire.Metadata)).not.toContain(
      required(f.executionCalls[0]).idempotencyKey,
    )
    expect(JSON.stringify(result)).not.toContain(f.emailInput.to)
    expect(JSON.stringify(result)).not.toContain('delivered')
  })
  it('Assist and cross-tenant/session cannot reach an authoritative gateway', async () => {
    const assist = actFixture('assist-example', 'assist')
    await expect(issue(assist)).rejects.toMatchObject({ code: 'visitor_action_not_permitted' })
    expect(assist.bookingCalls).toHaveLength(0)
    const f = actFixture(),
      flow = await issue(f)
    const other = new DurableActHost({ ...f.options, sessionId: 'another-session' })
    await expect(other.present(flow.presentation)).rejects.toMatchObject({
      code: 'visitor_challenge_mismatch',
    })
    await expect(
      f.host.present({ ...flow.presentation, graph: actFixture('foreign-tenant').graph }),
    ).rejects.toThrow()
    expect(f.bookingCalls).toHaveLength(0)
  })
  it('changed payload, replayed token and expired consent are refused without additional writes', async () => {
    const f = actFixture(),
      flow = await issue(f)
    await expect(
      f.host.present({
        ...flow.presentation,
        envelope: {
          ...flow.presentation.envelope,
          action: {
            actionId: 'booking.create',
            inputs: { ...f.bookingInput, customer: { customerRef: 'other-customer' } },
          },
        },
      }),
    ).rejects.toThrow()
    await f.host.present(flow.presentation)
    await expect(f.host.present(flow.presentation)).rejects.toMatchObject({
      code: 'visitor_challenge_already_spent',
    })
    const expired = await issue(f, 'email.send', f.emailInput)
    f.advance(300001)
    await expect(f.host.present(expired.presentation)).rejects.toMatchObject({
      code: 'visitor_challenge_expired',
    })
    expect(f.bookingCalls).toHaveLength(1)
    expect(f.mailCalls).toHaveLength(0)
  })
  it('an unknown booking/mail remains unknown on fresh challenges and recreated hosts, with no blind provider repeat', async () => {
    for (const action of ['booking.create', 'email.send']) {
      const f = actFixture()
      if (action === 'booking.create') f.unknownBooking()
      else f.unknownMail()
      const inputs = action === 'booking.create' ? f.bookingInput : f.emailInput
      const flow = await issue(f, action, inputs)
      expect((await f.host.present(flow.presentation)).errorCode).toBe('provider_outcome_unknown')
      const retry = await issue(f, action, inputs)
      expect(retry.reviewed.occurredAt).toBe(flow.reviewed.occurredAt)
      const restarted = new DurableActHost(f.options)
      expect((await restarted.present(retry.presentation)).errorCode).toBe(
        'provider_outcome_unknown',
      )
      expect(f.bookingCalls.length + f.mailCalls.length).toBe(1)
    }
  })
  it('template approval stays authoritative even with a verified consent', async () => {
    const f = actFixture(),
      flow = await issue(f, 'email.send', f.emailInput)
    f.revokeTemplate()
    expect((await f.host.present(flow.presentation)).execution).toBe('failed')
    expect(f.mailCalls).toHaveLength(0)
  })
  it('direct cross-tenant dispatcher requests are refused below the gates too', async () => {
    const f = actFixture()
    const outcome = await f.executor.execute({
      tenantId: 'wrong-tenant',
      sessionId: 'wrong-session',
      action: 'booking.create',
      inputs: f.bookingInput,
      idempotencyKey: 'synthetic-key',
    })
    expect(outcome.status).toBe('failed')
    expect(f.bookingCalls).toHaveLength(0)
  })
})

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error('Missing fixture result')
  return value
}
