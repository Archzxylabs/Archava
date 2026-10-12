import { describe, it, expect } from 'vitest'
import {
  ProposalService,
  ProposalError,
  type ProposalRepository,
  type ProposalPrincipal,
} from '../src/index.js'

describe('ProposalService boundaries', () => {
  const dummyRepo: ProposalRepository = {
    access: () =>
      Promise.resolve({ permissions: ['proposals.read'], ownerId: 'alice', scope: 'all' }),
    list: () => Promise.resolve({ items: [] }),
    detail: () => Promise.reject(new ProposalError('not_found')),
    listVersions: () => Promise.resolve([]),
    listChangeRequests: () => Promise.resolve([]),
    mutate: () =>
      Promise.resolve({
        commandId: '00000000-0000-0000-0000-000000000001',
        committedAt: new Date().toISOString(),
        revision: 1,
        status: 'committed',
      }),
    commandStatus: () => Promise.resolve({ status: 'unknown' }),
  }

  const service = new ProposalService(dummyRepo)

  it('rejects machine service mutation synchronously', () => {
    const machinePrincipal: ProposalPrincipal = {
      kind: 'service',
      id: 'bot-1',
      onBehalfOf: 'alice',
    }

    expect(() =>
      service.mutate('tenant-1', machinePrincipal, {
        type: 'create_proposal',
        commandId: '00000000-0000-0000-0000-000000000002',
        title: 'New proposal',
        summary: 'Test summary',
        scopeItems: [{ title: 'D1', description: 'Desc' }],
      }),
    ).toThrowError(/Proposal machine writes are not registered/)
  })

  it('rejects machine service on commandStatus', () => {
    const machinePrincipal: ProposalPrincipal = {
      kind: 'service',
      id: 'bot-1',
      onBehalfOf: 'alice',
    }

    expect(() =>
      service.commandStatus('tenant-1', machinePrincipal, '00000000-0000-0000-0000-000000000002'),
    ).toThrowError(ProposalError)
  })

  it('validates human principal and passes mutation to repo', async () => {
    const humanPrincipal: ProposalPrincipal = {
      kind: 'human',
      id: 'alice',
    }

    const res = await service.mutate('tenant-1', humanPrincipal, {
      type: 'create_proposal',
      commandId: 'a0000000-0000-4000-8000-000000000002',
      title: 'New proposal',
      summary: 'Test summary',
      scopeItems: [
        {
          title: 'Milestone 1',
          description: 'Desc',
          estimatedDays: 10,
          estimatedValue: 1000,
          deliverableType: 'milestone',
        },
      ],
    })

    expect(res.status).toBe('committed')
  })
})
