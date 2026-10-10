import { describe, it, expect, vi } from 'vitest'
import { randomUUID } from 'node:crypto'
import { PresenceGateway, type PresenceIdentity, type PresenceExecutor } from '../src/index.js'
const guest: PresenceIdentity = {
  kind: 'guest',
  binding: 'server-issued-a',
  tenantId: 'hotel-demo',
}
const owner: PresenceIdentity = {
  kind: 'owner',
  binding: 'verified-owner-session',
  tenantId: 'hotel-demo',
  principalId: 'human-owner',
}
const request = (sessionId: string, transport = 'chat') => ({
  version: '1.0.0',
  sessionId,
  requestId: randomUUID(),
  pageRevision: 0,
  transport,
  utterance: 'Help',
})
const answer = () =>
  Promise.resolve({
    status: 'ok' as const,
    text: 'Offline help',
    source: 'public' as const,
    mode: 'offline' as const,
  })
describe('shared presence policy boundary', () => {
  it('uses the same implementation for chat and synthetic voice with exact request identities and bounded tools', async () => {
    const execute = vi.fn(answer)
    const gateway = new PresenceGateway({
      project: () => Promise.resolve(['public.help']),
      answer: execute,
    })
    const session = gateway.issue(guest, ['top', 'services'])
    const chat = await gateway.turn(request(session.sessionId), guest)
    const voice = await gateway.turn(
      request(session.sessionId, 'synthetic-voice-transcript'),
      guest,
    )
    expect(chat.text).toBe(voice.text)
    expect(execute).toHaveBeenCalledTimes(2)
    expect(chat.allowedTools).toEqual(['public.help'])
    expect(
      gateway.context(
        { version: '1.0.0', sessionId: session.sessionId, section: 'services', revision: 1 },
        guest,
      ),
    ).toEqual({ section: 'services', revision: 1 })
    await expect(gateway.turn(request(session.sessionId), guest)).rejects.toThrow('revision')
  })
  it('refuses caller authority, extra fields, malformed schema, private tools and forged guest bindings before execution', async () => {
    const execute = vi.fn(answer),
      gateway = new PresenceGateway({
        project: () => Promise.resolve(['public.help']),
        answer: execute,
      })
    const session = gateway.issue(guest, ['top'])
    for (const input of [
      { ...request(session.sessionId), role: 'owner' },
      { ...request(session.sessionId), grants: ['*'] },
      { ...request(session.sessionId), tenantId: 'other' },
      { ...request(session.sessionId), pageRevision: true },
    ])
      await expect(gateway.turn(input, guest)).rejects.toThrow('invalid')
    await expect(
      gateway.turn({ ...request(session.sessionId), requestedTool: 'crm.contacts.list' }, guest),
    ).rejects.toThrow('forbidden')
    await expect(
      gateway.turn(request(session.sessionId), { ...guest, binding: 'forged' }),
    ).rejects.toThrow('forbidden')
    await expect(
      gateway.turn(request(session.sessionId), { ...guest, tenantId: 'other' }),
    ).rejects.toThrow('forbidden')
    await expect(
      gateway.turn(
        { ...request(session.sessionId), selection: { contactId: randomUUID() } },
        guest,
      ),
    ).rejects.toThrow('forbidden')
    expect(execute).not.toHaveBeenCalled()
  })
  it('refuses replay, expired sessions, unknown sections and stale/context-injected revisions', async () => {
    let now = 1000
    const gateway = new PresenceGateway(
      { project: () => Promise.resolve(['public.help']), answer },
      () => now,
    )
    const session = gateway.issue(guest, ['top'], 1),
      input = request(session.sessionId)
    await gateway.turn(input, guest)
    await expect(gateway.turn(input, guest)).rejects.toThrow('replay')
    for (const packet of [
      { version: '1.0.0', sessionId: session.sessionId, section: 'private', revision: 1 },
      { version: '1.0.0', sessionId: session.sessionId, section: 'top', revision: 0 },
      {
        version: '1.0.0',
        sessionId: session.sessionId,
        section: 'top',
        revision: 1,
        prompt: 'override',
      },
    ])
      expect(() => gateway.context(packet, guest)).toThrow()
    now = 2000
    await expect(gateway.turn(request(session.sessionId), guest)).rejects.toThrow('expired')
  })
  it('rechecks owner projection and refuses other owner sessions and cross-tenant scope', async () => {
    let permitted = true
    const executor: PresenceExecutor = {
      project: () => Promise.resolve(permitted ? ['crm.contacts.list'] : []),
      answer: () =>
        Promise.resolve({ status: 'ok', text: 'Verified read', source: 'crm', mode: 'offline' }),
    }
    const gateway = new PresenceGateway(executor),
      session = gateway.issue(owner, ['crm'])
    await expect(
      gateway.turn({ ...request(session.sessionId), requestedTool: 'crm.contacts.list' }, owner),
    ).resolves.toMatchObject({ source: 'crm' })
    permitted = false
    await expect(
      gateway.turn({ ...request(session.sessionId), requestedTool: 'crm.contacts.list' }, owner),
    ).rejects.toThrow('forbidden')
    await expect(
      gateway.turn(request(session.sessionId), { ...owner, principalId: 'human-other' }),
    ).rejects.toThrow('forbidden')
    await expect(
      gateway.turn(request(session.sessionId), { ...owner, tenantId: 'other' }),
    ).rejects.toThrow('forbidden')
  })
  it('times out honestly, aborts the executor and retains a busy lane until late work settles', async () => {
    let signal: AbortSignal | undefined
    const gateway = new PresenceGateway(
      {
        project: () => Promise.resolve(['public.help']),
        answer: (_request, ctx) => {
          signal = ctx.signal
          return new Promise(() => {})
        },
      },
      Date.now,
      10,
    )
    const session = gateway.issue(guest, ['top'])
    await expect(gateway.turn(request(session.sessionId), guest)).rejects.toThrow('timeout')
    expect(signal?.aborted).toBe(true)
    await expect(gateway.turn(request(session.sessionId), guest)).rejects.toThrow('busy')
  })
  it('reports unavailable without fabricated data or success and refuses unsafe public projection', async () => {
    const unavailable = new PresenceGateway({
      project: () => Promise.resolve(['public.help']),
      answer: () =>
        Promise.resolve({
          status: 'unavailable',
          text: 'Provider is unavailable. No action was performed.',
          source: 'none',
          mode: 'provider-unavailable',
        }),
    })
    const session = unavailable.issue(guest, ['top'])
    await expect(unavailable.turn(request(session.sessionId), guest)).resolves.toMatchObject({
      status: 'unavailable',
      source: 'none',
      mode: 'provider-unavailable',
    })
    const unsafe = new PresenceGateway({
      project: () => Promise.resolve(['crm.contacts.list']),
      answer,
    })
    await expect(
      unsafe.turn(request(unsafe.issue(guest, ['top']).sessionId), guest),
    ).rejects.toThrow('forbidden')
  })
})
