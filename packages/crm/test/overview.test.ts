import { describe, it, expect, vi } from 'vitest'
import {
  CrmService,
  createCrmTools,
  createCrmApi,
  crmOverviewSchema,
  type CrmRepository,
  type CrmOverview,
} from '../src/index.js'

const asOf = '2026-10-09T08:00:00.000Z',
  principal = { kind: 'human' as const, id: 'owner' }
const snapshot: CrmOverview = {
  tenantId: 'hotel-demo',
  asOf,
  scope: 'assigned',
  contacts: 120,
  followups: { open: 42, overdue: 12, upcoming: 30, completed: 8 },
  pipeline: { total: 25, active: 18, new: 10, qualified: 5, proposal: 3, won: 4, lost: 3 },
}
function fixture() {
  const refuse = () => Promise.reject(new Error('Unexpected storage')),
    overview = vi.fn(() => Promise.resolve(snapshot)),
    mutate = vi.fn()
  const repository: CrmRepository = {
    overview,
    access: refuse,
    list: refuse,
    detail: refuse,
    mutate,
  }
  return { repository, overview, mutate, service: new CrmService(repository, () => asOf) }
}
describe('authorized CRM aggregate boundary', () => {
  it('rejects caller identity/time/SQL fields and unauthenticated reads before storage', async () => {
    const f = fixture()
    for (const input of [
      { tenantId: 'studio-demo' },
      { scope: 'all' },
      { asOf },
      { sql: 'SELECT *' },
      { permissions: ['pipeline.read'] },
      { limit: 20 },
    ])
      await expect(f.service.overview('hotel-demo', principal, input)).rejects.toThrow()
    await expect(f.service.overview('hotel-demo', null)).rejects.toMatchObject({
      code: 'unauthenticated',
    })
    expect(f.overview).not.toHaveBeenCalled()
  })
  it('binds aggregate tenant and due boundary, rejecting inconsistent and unsafe counts', async () => {
    const f = fixture()
    for (const output of [
      { ...snapshot, tenantId: 'studio-demo' },
      { ...snapshot, asOf: '2026-10-10T00:00:00.000Z' },
      { ...snapshot, contacts: -1 },
      { ...snapshot, contacts: Number.MAX_SAFE_INTEGER + 1 },
      { ...snapshot, followups: { open: 42, overdue: 13, upcoming: 30, completed: 8 } },
      {
        ...snapshot,
        pipeline: { total: 26, active: 18, new: 10, qualified: 5, proposal: 3, won: 4, lost: 3 },
      },
    ]) {
      f.overview.mockResolvedValue(output)
      await expect(f.service.overview('hotel-demo', principal)).rejects.toThrow()
    }
    expect(
      crmOverviewSchema.safeParse({ ...snapshot, email: 'private@example.test' }).success,
    ).toBe(false)
    expect(f.mutate).not.toHaveBeenCalled()
  })
  it('preserves unavailable sections distinctly from a permitted zero and old adapters', async () => {
    const f = fixture()
    f.overview.mockResolvedValue({ ...snapshot, followups: null, pipeline: null })
    expect((await f.service.overview('hotel-demo', principal)).followups).toBeNull()
    await expect(
      new CrmService({ ...f.repository, overview: undefined }).overview('hotel-demo', principal),
    ).rejects.toMatchObject({ code: 'unavailable' })
  })
  it('reads through the same empty-input tool with no write', async () => {
    const f = fixture()
    expect(
      await createCrmTools(f.service, 'hotel-demo', principal).invoke('crm.overview', {}),
    ).toEqual(snapshot)
    expect(f.overview).toHaveBeenCalledWith('hotel-demo', principal, asOf)
    expect(f.mutate).not.toHaveBeenCalled()
  })
  it('exposes authenticated no-store GET, refusing caller query controls', async () => {
    const f = fixture(),
      api = createCrmApi(f.service, () => Promise.resolve(principal))
    const response = await api(new Request('https://crm.test/api/crm/hotel-demo/overview'))
    expect(response.status).toBe(200)
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(await response.json()).toEqual(snapshot)
    f.overview.mockClear()
    expect(
      (await api(new Request('https://crm.test/api/crm/hotel-demo/overview?scope=all'))).status,
    ).toBe(400)
    expect(f.overview).not.toHaveBeenCalled()
  })
})
