import { describe, expect, it } from 'vitest'
import { randomUUID } from 'node:crypto'
import {
  commandSchema,
  principalSchema,
  pageSchema,
  mutationSchema,
  CrmService,
  CrmError,
  createCrmApi,
  type CrmRepository,
  type CrmCommandStatus,
  commandStatusSchema,
} from '../src/index.js'

describe('CRM contracts', () => {
  it('keeps omitted contact changes absent instead of clearing email/organization', () => {
    const command = commandSchema.parse({
      operation: 'contact.update',
      id: randomUUID(),
      expectedRevision: 1,
      data: { fullName: ' New name ' },
    })
    expect(command).toMatchObject({ data: { fullName: 'New name' } })
    if (command.operation !== 'contact.update') throw new Error('Wrong fixture')
    expect(Object.keys(command.data)).toEqual(['fullName'])
    expect(() => commandSchema.parse({ ...command, data: {} })).toThrow()
  })
  it('permits explicit null clears, refuses extra ownership/permission fields', () => {
    expect(
      commandSchema.parse({
        operation: 'contact.update',
        id: randomUUID(),
        expectedRevision: 1,
        data: { email: null },
      }),
    ).toMatchObject({ data: { email: null } })
    expect(() =>
      commandSchema.parse({
        operation: 'contact.create',
        data: { fullName: 'Contact', ownerId: 'other-person' },
      }),
    ).toThrow()
    expect(() => principalSchema.parse({ kind: 'human', id: 'owner', role: 'owner' })).toThrow()
    expect(() => principalSchema.parse({ kind: 'service', id: 'bot' })).toThrow()
    expect(() =>
      mutationSchema.parse({
        requestId: randomUUID(),
        command: { operation: 'contact.delete', id: randomUUID() },
      }),
    ).toThrow()
  })
  it('bounds every public field and refuses invalid contact/task dates', () => {
    expect(() => pageSchema.parse({ limit: 101 })).toThrow()
    expect(() => pageSchema.parse({ after: 'garbage' })).toThrow()
    expect(() => pageSchema.parse({ search: 'x'.repeat(121) })).toThrow()
    expect(() =>
      commandSchema.parse({ operation: 'contact.create', data: { fullName: 'x'.repeat(161) } }),
    ).toThrow()
    expect(() =>
      commandSchema.parse({
        operation: 'contact.create',
        data: { fullName: 'Name', email: 'not-email' },
      }),
    ).toThrow()
    expect(() =>
      commandSchema.parse({
        operation: 'followup.create',
        contactId: randomUUID(),
        contactRevision: 1,
        summary: 'Task',
        dueAt: 'tomorrow',
      }),
    ).toThrow()
  })
})

/** A trap proves refused input cannot reach persistence. Real SQL cases live in crm:verify. */
const trapped = (): CrmRepository => {
  const refuse = () => Promise.reject(new Error('Persistence must not be reached'))
  return { access: refuse, list: refuse, detail: refuse, mutate: refuse }
}
describe('service and HTTP boundary', () => {
  const service = new CrmService(trapped())
  it('refuses unauthenticated access and machine writes before touching storage', async () => {
    await expect(service.list('hotel-demo', null)).rejects.toThrow(CrmError)
    await expect(
      service.mutate('hotel-demo', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, {}),
    ).rejects.toThrow(/Act/)
  })
  it('rejects malformed input before storage', async () => {
    await expect(
      service.mutate(
        'hotel-demo',
        { kind: 'human', id: 'owner' },
        { requestId: 'wrong', command: {} },
      ),
    ).rejects.toThrow()
  })
  it('does not authenticate browser body identities or silently expose database errors', async () => {
    const denied = createCrmApi(service, () => Promise.resolve(null))
    expect((await denied(new Request('https://crm.test/api/crm/hotel-demo/contacts'))).status).toBe(
      401,
    )
    const handler = createCrmApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
    const result = await handler(new Request('https://crm.test/api/crm/hotel-demo/contacts'))
    expect(result.status).toBe(503)
    expect(JSON.stringify(await result.json())).not.toContain('Persistence')
  })
  it('rejects foreign origins and missing cookie origins before writes', async () => {
    const handler = createCrmApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
    for (const headers of [
      { origin: 'https://other.test' },
      { cookie: 'session=fixture' },
    ] as HeadersInit[]) {
      const response = await handler(
        new Request('https://crm.test/api/crm/hotel-demo/commands', {
          method: 'POST',
          headers,
          body: '{}',
        }),
      )
      expect(response.status).toBe(403)
    }
  })
  it('rejects large streamed bodies and invalid JSON without persistence', async () => {
    const handler = createCrmApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
    for (const [body, status] of [
      [' '.repeat(17_000), 413],
      ['not json', 400],
    ] as const) {
      const response = await handler(
        new Request('https://crm.test/api/crm/hotel-demo/commands', { method: 'POST', body }),
      )
      expect(response.status).toBe(status)
    }
  })
})

describe('command recovery boundary', () => {
  const id = randomUUID()
  const principal = { kind: 'human' as const, id: 'owner' }
  it('requires a human, validated request id and an explicitly supported adapter', async () => {
    const unsupported = new CrmService(trapped())
    await expect(unsupported.commandStatus('hotel-demo', null, id)).rejects.toMatchObject({
      code: 'unauthenticated',
    })
    await expect(
      unsupported.commandStatus(
        'hotel-demo',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        id,
      ),
    ).rejects.toMatchObject({ code: 'forbidden' })
    await expect(unsupported.commandStatus('hotel-demo', principal, 'invalid')).rejects.toThrow()
    await expect(unsupported.commandStatus('hotel-demo', principal, id)).rejects.toMatchObject({
      code: 'unavailable',
    })
  })
  it('rejects a mismatched receipt or unbounded output instead of claiming success', async () => {
    const mismatch = new CrmService({
      ...trapped(),
      commandStatus: () => Promise.resolve({ status: 'unconfirmed', requestId: randomUUID() }),
    })
    await expect(mismatch.commandStatus('hotel-demo', principal, id)).rejects.toMatchObject({
      code: 'unavailable',
    })
    expect(() =>
      commandStatusSchema.parse({ status: 'unconfirmed', requestId: id, rawInputs: 'private' }),
    ).toThrow()
  })
  it('reads a receipt with no mutation and masks storage errors at HTTP', async () => {
    let calls = 0
    const receipt = { kind: 'followup' as const, id: randomUUID(), revision: 1 }
    const repository = {
      ...trapped(),
      commandStatus: () => {
        calls += 1
        return Promise.resolve({ status: 'committed' as const, requestId: id, receipt })
      },
    }
    const handler = createCrmApi(new CrmService(repository), () => Promise.resolve(principal))
    const response = await handler(
      new Request('https://crm.test/api/crm/hotel-demo/commands/' + id),
    )
    expect(response.status).toBe(200)
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(await response.json()).toEqual({ status: 'committed', requestId: id, receipt })
    expect(calls).toBe(1)
    const broken = createCrmApi(
      new CrmService({
        ...trapped(),
        commandStatus: (): Promise<CrmCommandStatus> =>
          Promise.reject(new Error('private database details')),
      }),
      () => Promise.resolve(principal),
    )
    const failure = await broken(new Request('https://crm.test/api/crm/hotel-demo/commands/' + id))
    expect(failure.status).toBe(503)
    expect(JSON.stringify(await failure.json())).not.toContain('private database')
  })
})
