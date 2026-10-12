import { describe, it, expect } from 'vitest'
import {
  CapacityService,
  CapacityError,
  type CapacityRepository,
  type CapacityPrincipal,
} from '../src/index.js'

describe('CapacityService boundaries', () => {
  const dummyRepo: CapacityRepository = {
    access: () =>
      Promise.resolve({ permissions: ['capacity.read'], ownerId: 'alice', scope: 'all' }),
    listPools: () => Promise.resolve({ items: [] }),
    detailPool: () => Promise.reject(new CapacityError('not_found')),
    estimate: () =>
      Promise.resolve({
        poolId: 'a0000000-0000-4000-8000-000000000001',
        unit: 'hours',
        requiredUnits: 40,
        queuePosition: 0,
        estimatedStartAt: new Date().toISOString(),
        estimatedFinishAt: new Date().toISOString(),
        burnRatePerDay: 8,
        estimatedCalendarDays: 5,
      }),
    mutate: () =>
      Promise.resolve({
        commandId: 'a0000000-0000-4000-8000-000000000001',
        committedAt: new Date().toISOString(),
        revision: 1,
        status: 'committed',
      }),
    commandStatus: () => Promise.resolve({ status: 'unknown' }),
  }

  const service = new CapacityService(dummyRepo)

  it('rejects machine service mutation synchronously', () => {
    const machinePrincipal: CapacityPrincipal = {
      kind: 'service',
      id: 'bot-1',
      onBehalfOf: 'alice',
    }

    expect(() =>
      service.mutate('tenant-1', machinePrincipal, {
        type: 'create_pool',
        commandId: 'a0000000-0000-4000-8000-000000000002',
        code: 'DESIGN',
        name: 'Design Team',
        weeklyCapacity: 160,
      }),
    ).toThrowError(/Capacity machine writes are not registered/)
  })

  it('rejects machine service on commandStatus', () => {
    const machinePrincipal: CapacityPrincipal = {
      kind: 'service',
      id: 'bot-1',
      onBehalfOf: 'alice',
    }

    expect(() =>
      service.commandStatus('tenant-1', machinePrincipal, 'a0000000-0000-4000-8000-000000000002'),
    ).toThrowError(CapacityError)
  })

  it('validates human principal and passes mutation to repo', async () => {
    const humanPrincipal: CapacityPrincipal = {
      kind: 'human',
      id: 'alice',
    }

    const res = await service.mutate('tenant-1', humanPrincipal, {
      type: 'create_pool',
      commandId: 'a0000000-0000-4000-8000-000000000002',
      code: 'DESIGN',
      name: 'Design Team',
      weeklyCapacity: 160,
    })

    expect(res.status).toBe('committed')
  })
})
