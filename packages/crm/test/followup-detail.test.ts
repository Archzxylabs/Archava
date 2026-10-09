import { describe, it, expect, vi } from 'vitest'
import {
  CrmService,
  createCrmTools,
  type CrmRepository,
  type FollowupDetail,
  CRM_PERMISSIONS,
} from '../src/index.js'
const id = '10000000-0000-4000-8000-000000000001',
  contactId = '10000000-0000-4000-8000-000000000002'
const now = '2026-10-09T08:00:00.000Z'
const principal = { kind: 'human' as const, id: 'owner' }
function fixture() {
  const detail: FollowupDetail = {
    followup: {
      id,
      contactId,
      tenantId: 'hotel-demo',
      summary: 'Call',
      dueAt: now,
      status: 'open',
      revision: 1,
      createdAt: now,
      updatedAt: now,
    },
    contact: { id: contactId, fullName: 'Budi', revision: 2 },
  }
  const access = vi.fn(() =>
    Promise.resolve({ permissions: [...CRM_PERMISSIONS], ownerId: 'owner', scope: 'all' as const }),
  )
  const followupDetail = vi.fn(() => Promise.resolve(detail)),
    mutate = vi.fn()
  const refuse = () => Promise.reject(new Error('Unexpected storage'))
  const repository: CrmRepository = { access, followupDetail, mutate, list: refuse, detail: refuse }
  return { detail, access, followupDetail, mutate, service: new CrmService(repository), repository }
}
describe('selected follow-up preparation', () => {
  it('validates exact identity and linked contact, preserving old adapter compatibility', async () => {
    const f = fixture()
    await expect(f.service.followup('hotel-demo', principal, 'bad')).rejects.toThrow()
    expect(f.followupDetail).not.toHaveBeenCalled()
    f.followupDetail.mockResolvedValue({
      ...f.detail,
      followup: { ...f.detail.followup, id: contactId },
    })
    await expect(f.service.followup('hotel-demo', principal, id)).rejects.toMatchObject({
      code: 'unavailable',
    })
    f.followupDetail.mockResolvedValue({ ...f.detail, contact: { ...f.detail.contact, id } })
    await expect(f.service.followup('hotel-demo', principal, id)).rejects.toMatchObject({
      code: 'unavailable',
    })
    const unsupported = new CrmService({ ...f.repository, followupDetail: undefined })
    await expect(unsupported.followup('hotel-demo', principal, id)).rejects.toMatchObject({
      code: 'unavailable',
    })
  })
  it('requires write permission only to prepare a completion; it creates no mutation or consent', async () => {
    const f = fixture(),
      tools = createCrmTools(f.service, 'hotel-demo', principal)
    expect(await tools.invoke('crm.followup.complete.prepare', { id })).toEqual({
      task: f.detail,
      requiresApproval: true,
    })
    expect(f.mutate).not.toHaveBeenCalled()
    f.access.mockResolvedValue({
      permissions: ['contacts.read', 'followups.read'],
      ownerId: 'owner',
      scope: 'all',
    })
    f.followupDetail.mockClear()
    await expect(tools.invoke('crm.followup.complete.prepare', { id })).rejects.toMatchObject({
      code: 'forbidden',
    })
    expect(f.followupDetail).not.toHaveBeenCalled()
  })
  it('refuses a completed task or browser approval/ownership fields', async () => {
    const f = fixture(),
      tools = createCrmTools(f.service, 'hotel-demo', principal)
    f.followupDetail.mockResolvedValue({
      ...f.detail,
      followup: { ...f.detail.followup, status: 'completed' },
    })
    await expect(tools.invoke('crm.followup.complete.prepare', { id })).rejects.toMatchObject({
      code: 'conflict',
    })
    await expect(
      tools.invoke('crm.followup.complete.prepare', { id, confirmed: true }),
    ).rejects.toThrow()
    expect(f.mutate).not.toHaveBeenCalled()
  })
})
