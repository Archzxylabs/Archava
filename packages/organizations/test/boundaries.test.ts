import { describe, it, expect } from 'vitest'
import {
  organizationSchema,
  affiliationSchema,
  OrganizationError,
  type OrganizationRepository,
  type OrganizationPrincipal,
  type OrganizationMutation,
} from '../src/contracts.js'
import { OrganizationService } from '../src/service.js'

describe('organization contracts and service boundaries', () => {
  it('validates organization schema strictly', () => {
    const valid = {
      id: 'a0000000-0000-4000-8000-000000000001',
      tenantId: 'test-tenant',
      name: 'Acme Corp',
      legalName: 'Acme International PT',
      taxId: '01.234.567.8-901.000',
      website: 'https://acme.corp',
      industry: 'Manufacturing',
      billingAddress: 'Jakarta, Indonesia',
      state: 'active',
      ownerId: 'owner-1',
      revision: 1,
      createdAt: '2026-10-11T12:00:00.000Z',
      updatedAt: '2026-10-11T12:00:00.000Z',
    }
    expect(organizationSchema.parse(valid)).toEqual(valid)

    expect(() =>
      organizationSchema.parse({
        ...valid,
        name: '',
      }),
    ).toThrow()
  })

  it('validates affiliation schema', () => {
    const valid = {
      id: 'a0000000-0000-4000-8000-000000000002',
      tenantId: 'test-tenant',
      organizationId: 'a0000000-0000-4000-8000-000000000001',
      contactId: 'a0000000-0000-4000-8000-000000000003',
      role: 'procurement',
      title: 'Procurement Director',
      department: 'Purchasing',
      isPrimary: true,
      status: 'active',
      createdAt: '2026-10-11T12:00:00.000Z',
      updatedAt: '2026-10-11T12:00:00.000Z',
    }
    expect(affiliationSchema.parse(valid)).toEqual(valid)
  })

  it('denies machine service mutations', async () => {
    const mockRepo: OrganizationRepository = {
      access: async () => ({
        permissions: ['organizations.read', 'organizations.write'],
        ownerId: 'owner-1',
        scope: 'all',
      }),
      listOrganizations: async () => ({ items: [], nextCursor: null }),
      detail: async () => {
        throw new Error('not implemented')
      },
      listAffiliations: async () => ({ items: [], nextCursor: null }),
      mutate: async () => {
        throw new Error('should not be called')
      },
      commandStatus: async () => ({ status: 'not_committed' }),
    }
    const service = new OrganizationService(mockRepo)

    const machinePrincipal: OrganizationPrincipal = {
      kind: 'service',
      id: 'agent-bot',
      onBehalfOf: 'owner-1',
    }
    const mutation: OrganizationMutation = {
      requestId: 'a0000000-0000-4000-8000-000000000010',
      type: 'create_organization',
      data: { name: 'Bot Corp' },
    }

    expect(() => service.mutate('test-tenant', machinePrincipal, mutation)).toThrow(
      OrganizationError,
    )
  })
})
