import { describe, it, expect } from 'vitest'
import {
  assetSchema,
  assetCustodyEventSchema,
  assetMaintenanceRecordSchema,
  createAssetDataSchema,
  AssetError,
  type AssetRepository,
  type AssetPrincipal,
  type AssetMutation,
} from '../src/contracts.js'
import { AssetService } from '../src/service.js'

describe('asset contracts and service boundaries', () => {
  it('validates asset schema strictly and enforces customer-owned contact id', () => {
    const validBusinessAsset = {
      id: 'a0000000-0000-4000-8000-000000000001',
      tenantId: 'test-tenant',
      code: 'AST-CAT-320',
      name: 'Excavator Caterpillar 320',
      category: 'heavy_equipment',
      serialNumber: 'CAT320-998822',
      model: '320 GC',
      ownership: 'business_owned' as const,
      customerContactId: null,
      status: 'available' as const,
      condition: 'excellent' as const,
      currentCustodianId: null,
      currentLocation: 'Jakarta Yard',
      ownerId: 'owner-1',
      revision: 1,
      createdAt: '2026-10-12T12:00:00.000Z',
      updatedAt: '2026-10-12T12:00:00.000Z',
    }
    expect(assetSchema.parse(validBusinessAsset)).toEqual(validBusinessAsset)

    // customer_owned asset requires customerContactId
    expect(() =>
      createAssetDataSchema.parse({
        code: 'CUST-LAPTOP-01',
        name: 'MacBook Pro 16',
        category: 'electronics',
        ownership: 'customer_owned',
        customerContactId: '',
      }),
    ).toThrow()
  })

  it('validates custody event and maintenance record schemas', () => {
    const validCustody = {
      id: 'a0000000-0000-4000-8000-000000000002',
      tenantId: 'test-tenant',
      assetId: 'a0000000-0000-4000-8000-000000000001',
      eventType: 'dispatch' as const,
      fromCustodianId: null,
      toCustodianId: 'custodian-operator-1',
      conditionBefore: 'excellent' as const,
      conditionAfter: 'good' as const,
      location: 'Site Sudirman',
      notes: 'Dispatched for 3 days rental',
      recordedBy: 'staff-1',
      recordedAt: '2026-10-12T12:00:00.000Z',
    }
    expect(assetCustodyEventSchema.parse(validCustody)).toEqual(validCustody)

    const validMaint = {
      id: 'a0000000-0000-4000-8000-000000000003',
      tenantId: 'test-tenant',
      assetId: 'a0000000-0000-4000-8000-000000000001',
      serviceType: 'preventive' as const,
      description: 'Hydraulic fluid replacement and track tension check',
      partsUsed: 'Hydraulic Oil 20L, Filter Element',
      conditionAfter: 'excellent' as const,
      performedBy: 'technician-budi',
      recordedAt: '2026-10-12T12:00:00.000Z',
    }
    expect(assetMaintenanceRecordSchema.parse(validMaint)).toEqual(validMaint)
  })

  it('denies machine service mutations', () => {
    const mockRepo: AssetRepository = {
      access: () =>
        Promise.resolve({
          permissions: ['assets.read', 'assets.write'],
          ownerId: 'owner-1',
          scope: 'all',
        }),
      listAssets: () => Promise.resolve({ items: [], nextCursor: null }),
      detail: () => Promise.reject(new AssetError('not_found')),
      listCustodyHistory: () => Promise.resolve([]),
      listMaintenanceHistory: () => Promise.resolve([]),
      mutate: () =>
        Promise.resolve({
          requestId: 'b0000000-0000-4000-8000-000000000001',
          type: 'create_asset',
          assetId: 'a0000000-0000-4000-8000-000000000001',
          revision: 1,
          committedAt: '2026-10-12T12:00:00.000Z',
        }),
      commandStatus: () => Promise.resolve({ status: 'not_committed' }),
    }

    const service = new AssetService(mockRepo)
    const machinePrincipal: AssetPrincipal = {
      kind: 'service',
      id: 'svc-bot',
      onBehalfOf: 'user-1',
    }

    const mutation: AssetMutation = {
      requestId: 'b0000000-0000-4000-8000-000000000001',
      type: 'create_asset',
      data: {
        code: 'AST-TEST-01',
        name: 'Test Rig',
        category: 'tools',
        ownership: 'business_owned',
        condition: 'good',
      },
    }

    expect(() => service.mutate('test-tenant', machinePrincipal, mutation)).toThrow(AssetError)
  })
})
