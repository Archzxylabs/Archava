import { describe, it, expect, vi } from 'vitest'
import {
  CrmService,
  createCrmApi,
  followupQueueQuerySchema,
  type CrmRepository,
} from '../src/index.js'

const principal = { kind: 'human' as const, id: 'owner' }
const asOf = '2026-10-09T08:00:00.000Z'
const id = '10000000-0000-4000-8000-000000000001'
const trapped = (): CrmRepository => {
  const refuse = () => Promise.reject(new Error('Storage should not be reached'))
  return { access: refuse, list: refuse, detail: refuse, mutate: refuse }
}
describe('follow-up worklist boundary', () => {
  it('requires a complete bounded keyset and refuses authority fields before storage', async () => {
    const listFollowups = vi.fn()
    const service = new CrmService({ ...trapped(), listFollowups })
    for (const input of [
      { limit: 101 },
      { bucket: 'completed' },
      { asOf: 'tomorrow' },
      { after: { id } },
      { after: { dueAt: asOf } },
      { ownerId: 'other' },
      { search: 'x'.repeat(121) },
    ])
      await expect(service.listFollowups('hotel-demo', principal, input)).rejects.toThrow()
    await expect(service.listFollowups('hotel-demo', null)).rejects.toMatchObject({
      code: 'unauthenticated',
    })
    expect(listFollowups).not.toHaveBeenCalled()
    expect(
      followupQueueQuerySchema.parse({ asOf, after: { id, dueAt: '2026-10-09T08:00:00.123456Z' } })
        .after?.dueAt,
    ).toContain('123456')
  })
  it('keeps old adapters compatible and rejects unverified or excessive output', async () => {
    await expect(
      new CrmService(trapped()).listFollowups('hotel-demo', principal),
    ).rejects.toMatchObject({ code: 'unavailable' })
    const item = { id, contactId: id, fullName: 'Budi', summary: 'Call', dueAt: asOf }
    for (const output of [
      { asOf: '2026-10-10T08:00:00.000Z', items: [], next: null },
      { asOf, items: [item, item], next: null },
      { asOf, items: [{ ...item, email: 'private@example.test' }], next: null },
    ]) {
      const service = new CrmService({ ...trapped(), listFollowups: () => Promise.resolve(output) })
      await expect(
        service.listFollowups('hotel-demo', principal, { asOf, limit: 1 }),
      ).rejects.toThrow()
    }
  })
  it('projects a no-store read endpoint and refuses incomplete HTTP cursors', async () => {
    const listFollowups = vi.fn(() => Promise.resolve({ asOf, items: [], next: null }))
    const api = createCrmApi(new CrmService({ ...trapped(), listFollowups }), () =>
      Promise.resolve(principal),
    )
    const response = await api(
      new Request('https://crm.test/api/crm/hotel-demo/followups?asOf=' + asOf + '&bucket=overdue'),
    )
    expect(response.status).toBe(200)
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(await response.json()).toEqual({ asOf, items: [], next: null })
    listFollowups.mockClear()
    expect(
      (await api(new Request('https://crm.test/api/crm/hotel-demo/followups?afterId=' + id)))
        .status,
    ).toBe(400)
    expect(listFollowups).not.toHaveBeenCalled()
  })
})
