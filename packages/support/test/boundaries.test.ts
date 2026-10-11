import { describe, it, expect, vi } from 'vitest'
import {
  SupportService,
  SupportError,
  SUPPORT_PERMISSIONS,
  supportMutationSchema,
  createSupportApi,
  createSupportTools,
  type SupportRepository,
} from '../src/index.js'
const id = '10000000-0000-4000-8000-000000000001',
  human = { kind: 'human' as const, id: 'owner' },
  bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' },
  input = (command: unknown) => ({ requestId: id, command })
function repository() {
  const unused = () => Promise.reject(new Error('PRIVATE database details'))
  return {
    access: () =>
      Promise.resolve({
        permissions: [...SUPPORT_PERMISSIONS],
        ownerId: 'owner',
        scope: 'all' as const,
      }),
    assignees: unused,
    cases: unused,
    detail: unused,
    mutate: vi.fn(unused),
    commandStatus: vi.fn(unused),
  } satisfies SupportRepository
}
describe('support execution boundaries', () => {
  it('refuses caller ownership, assignments, state, delivery, SLA and permission claims before persistence', () => {
    const repo = repository(),
      service = new SupportService(repo),
      create = {
        operation: 'case.create',
        requesterName: 'Customer',
        subject: 'Issue',
        description: 'Investigation required',
      }
    for (const extra of [
      { ownerId: 'another' },
      { assigneeId: 'staff' },
      { state: 'resolved' },
      { sent: true },
      { slaFulfilled: true },
      { permissions: SUPPORT_PERMISSIONS },
      { paid: true },
    ])
      expect(() => service.mutate('support', human, input({ ...create, ...extra }))).toThrow()
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('requires bounded resolution/findings and explicit reasons for reopening, assignment and target changes', () => {
    for (const operation of ['case.reopen', 'case.assign', 'case.cancel', 'case.set-target'])
      expect(
        supportMutationSchema.safeParse(input({ operation, id, expectedRevision: 1 })).success,
      ).toBe(false)
    for (const body of ['', 'x'.repeat(4001)])
      expect(
        supportMutationSchema.safeParse(
          input({ operation: 'note.add', id, expectedRevision: 1, body }),
        ).success,
      ).toBe(false)
    expect(
      supportMutationSchema.safeParse(
        input({ operation: 'case.resolve', id, expectedRevision: 1, resolution: '' }),
      ).success,
    ).toBe(false)
  })
  it('rejects ambiguous local times, nonfinite targets and invalid revisions while preserving explicit offset targets', () => {
    const create = {
      operation: 'case.create',
      requesterName: 'Customer',
      subject: 'Issue',
      description: 'Investigate',
    }
    for (const dueAt of [
      '2026-10-12T12:00:00',
      'infinity',
      '1999-12-31T00:00:00Z',
      '2100-01-01T00:00:00Z',
    ])
      expect(supportMutationSchema.safeParse(input({ ...create, dueAt })).success).toBe(false)
    expect(
      supportMutationSchema.safeParse(input({ ...create, dueAt: '2026-10-12T12:00:00+07:00' }))
        .success,
    ).toBe(true)
    expect(
      supportMutationSchema.safeParse(
        input({ operation: 'case.start', id, expectedRevision: 1.5, reason: 'Investigate' }),
      ).success,
    ).toBe(false)
  })
  it('read-only machine tools cannot mutate, query assignment directory or recover execution receipts', async () => {
    const repo = repository(),
      service = new SupportService(repo),
      tools = createSupportTools(service, 'support', bot)
    expect(() =>
      service.mutate(
        'support',
        bot,
        input({ operation: 'case.close', id, expectedRevision: 1, reason: 'Close' }),
      ),
    ).toThrow(SupportError)
    expect(() => service.commandStatus('support', bot, id)).toThrow(SupportError)
    expect(() => service.assignees('support', bot)).toThrow(SupportError)
    await expect(tools.invoke('support.case.resolve', { id })).rejects.toMatchObject({
      code: 'not_found',
    })
    await expect(
      tools.invoke('support.detail', { id, scope: 'all', tenant: 'another' }),
    ).rejects.toThrow()
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('API requires identity/origin, rejects repeated or unknown queries and enforces streamed byte limits', async () => {
    const service = new SupportService(repository()),
      api = createSupportApi(service, () => Promise.resolve(human)),
      url = 'http://localhost/api/support/support/'
    expect(
      (await createSupportApi(service, () => Promise.resolve(null))(new Request(url + 'access')))
        .status,
    ).toBe(401)
    for (const suffix of [
      'cases?scope=all',
      'cases?limit=1&limit=2',
      'cases?overdue=1',
      'assignees?permissions=all',
    ])
      expect((await api(new Request(url + suffix))).status).toBe(400)
    for (const headers of [
      new Headers({ cookie: 'session=token' }),
      new Headers({ origin: 'http://attacker.example' }),
    ])
      expect(
        (await api(new Request(url + 'commands', { method: 'POST', headers, body: '{}' }))).status,
      ).toBe(403)
    expect(
      (await api(new Request(url + 'commands', { method: 'POST', body: ' '.repeat(17000) })))
        .status,
    ).toBe(413)
  })
  it('dependency failures remain opaque rather than returning database record or credential details', async () => {
    const response = await createSupportApi(new SupportService(repository()), () =>
      Promise.resolve(human),
    )(new Request('http://localhost/api/support/support/cases'))
    expect(response.status).toBe(503)
    expect(await response.json()).toEqual({ error: 'unavailable' })
  })
})
