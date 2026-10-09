import { describe, it, expect } from 'vitest'
import { ConfirmationService } from '../src/index.js'

const request = {
  tenantId: 'tenant-a',
  sessionId: 'session-a',
  actionId: 'email.send',
  inputs: { templateId: 'approved', to: 'synthetic-recipient@example.test' },
}
function fixture() {
  let writes = 0,
    clocks = 0
  const service = new ConfirmationService({
    digestKey: new Uint8Array(32).fill(29),
    clock: () => {
      clocks += 1
      return 1791536400000
    },
    store: {
      storeId: 'binding-test',
      issue: () => {
        writes += 1
        return true
      },
      consume: () => {
        throw new Error('Read-only description must not spend a challenge')
      },
    },
  })
  return { service, writes: () => writes, clocks: () => clocks }
}
describe('read-only binding descriptions', () => {
  it('matches actual issued binding without issuing consent, touching storage or reading a clock', async () => {
    const f = fixture(),
      description = f.service.describeBinding(request)
    expect(f.writes()).toBe(0)
    expect(f.clocks()).toBe(0)
    expect(description).not.toHaveProperty('token')
    expect(description).not.toHaveProperty('receiptDigest')
    const issued = await f.service.issue(request)
    if (issued.status !== 'issued') throw new Error('Missing fixture challenge')
    expect(description).toEqual({
      status: 'bound',
      inputDigest: issued.challenge.inputDigest,
      bindingDigest: issued.challenge.bindingDigest,
    })
    expect(f.writes()).toBe(1)
  })
  it('separates payloads and tenant/session/action bindings, while canonical key order agrees', () => {
    const f = fixture(),
      original = f.service.describeBinding(request)
    expect(
      f.service.describeBinding({
        ...request,
        inputs: {
          to: request.inputs.to,
          templateId: request.inputs.templateId,
        },
      }),
    ).toEqual(original)
    expect(
      f.service.describeBinding({
        ...request,
        inputs: { ...request.inputs, to: 'other@example.test' },
      }),
    ).not.toEqual(original)
    for (const patch of [{ tenantId: 'other' }, { sessionId: 'other' }, { actionId: 'other' }])
      expect(f.service.describeBinding({ ...request, ...patch })).not.toEqual(original)
    expect(f.writes()).toBe(0)
  })
  it('refuses unsupported inputs and keeps secrets out of serialization', () => {
    const f = fixture()
    expect(f.service.describeBinding({ ...request, inputs: { bad: undefined } })).toMatchObject({
      status: 'rejected',
    })
    expect(f.service.describeBinding({ ...request, tenantId: '' })).toMatchObject({
      status: 'rejected',
    })
    expect(JSON.stringify(f.service)).toBe('{}')
    expect(f.writes()).toBe(0)
  })
})
