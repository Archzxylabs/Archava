import { describe, it, expect } from 'vitest'
import {
  orderSchema,
  orderLineSchema,
  findingSchema,
  processEventSchema,
  laundryMutationSchema,
  PROCESS_STAGE,
  MACHINE_EVENTS,
  LaundryError,
  type LaundryRepository,
  type LaundryReceipt,
} from '../src/contracts.js'
import { LaundryService } from '../src/service.js'
import { executeLaundryTool } from '../src/tools.js'

const ORDER_ID = '550e8400-e29b-41d4-a716-446655440000'
const LINE_ID = '660e8400-e29b-41d4-a716-446655440000'
const FINDING_ID = '770e8400-e29b-41d4-a716-446655440000'
const PROCESS_ID = '880e8400-e29b-41d4-a716-446655440000'
const RECONCILES_ID = '990e8400-e29b-41d4-a716-446655440000'

const fakeRepo: LaundryRepository = {
  execute: () => Promise.resolve({} as unknown as LaundryReceipt),
  getCommandStatus: () => Promise.resolve({ status: 'unknown' }),
  getAccess: () => Promise.resolve({ ownerId: 'u1', permissions: [], scope: 'all' as const }),
  listOrders: () => Promise.resolve([]),
  getOrderDetail: () => Promise.resolve(null),
  listLines: () => Promise.resolve([]),
  listProcess: () => Promise.resolve([]),
  listFindings: () => Promise.resolve([]),
}

describe('laundry domain contracts and boundaries', () => {
  it('validates a priced weight order with frozen totals and customer reference', () => {
    const order = orderSchema.parse({
      id: ORDER_ID,
      tenantId: 'tenant-test',
      orderCode: 'LDR-2026-0042',
      kind: 'weight',
      customerRef: 'customer-andi',
      status: 'processing',
      currency: 'IDR',
      totalMinor: 36000,
      itemCount: 0,
      weightGrams: 3000,
      unreconciledCount: 1,
      revision: 2,
      createdAt: '2026-10-12T10:00:00.000Z',
      updatedAt: '2026-10-12T10:00:00.000Z',
    })
    expect(order.kind).toBe('weight')
    expect(order.totalMinor).toBe(36000)
    expect(order.unreconciledCount).toBe(1)
  })

  it('rejects negative counters, unknown status and malformed currency', () => {
    expect(() =>
      orderSchema.parse({
        id: ORDER_ID,
        tenantId: 'tenant-test',
        orderCode: 'LDR-2026-0042',
        kind: 'weight',
        customerRef: null,
        status: 'shipped',
        currency: 'IDR',
        totalMinor: -1,
        revision: 1,
        createdAt: '2026-10-12T10:00:00.000Z',
        updatedAt: '2026-10-12T10:00:00.000Z',
      }),
    ).toThrow()
    expect(() =>
      orderSchema.parse({
        id: ORDER_ID,
        tenantId: 'tenant-test',
        orderCode: 'LDR-2026-0043',
        kind: 'weight',
        customerRef: null,
        status: 'received',
        currency: 'idr',
        totalMinor: 0,
        revision: 1,
        createdAt: '2026-10-12T10:00:00.000Z',
        updatedAt: '2026-10-12T10:00:00.000Z',
      }),
    ).toThrow()
  })

  it('validates priced order lines with integer units and minor currency prices', () => {
    const line = orderLineSchema.parse({
      id: LINE_ID,
      tenantId: 'tenant-test',
      orderId: ORDER_ID,
      itemRef: 'wash-fold-kg',
      quantity: 3000,
      unit: 'gram',
      unitPriceMinor: 12,
      lineTotalMinor: 36000,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:05:00.000Z',
    })
    expect(line.unit).toBe('gram')
    expect(line.lineTotalMinor).toBe(36000)

    expect(() =>
      orderLineSchema.parse({
        id: LINE_ID,
        tenantId: 'tenant-test',
        orderId: ORDER_ID,
        itemRef: 'wash-fold-kg',
        quantity: 0,
        unit: 'gram',
        unitPriceMinor: 12,
        lineTotalMinor: 0,
        recordedBy: 'operator-1',
        recordedAt: '2026-10-12T10:05:00.000Z',
      }),
    ).toThrow()
    expect(() =>
      orderLineSchema.parse({
        id: LINE_ID,
        tenantId: 'tenant-test',
        orderId: ORDER_ID,
        itemRef: 'wash-fold-kg',
        quantity: 3000,
        unit: 'kilogram',
        unitPriceMinor: 12,
        lineTotalMinor: 36000,
        recordedBy: 'operator-1',
        recordedAt: '2026-10-12T10:05:00.000Z',
      }),
    ).toThrow()
  })

  it('keeps the processing chain order explicit and separates machine events', () => {
    expect(PROCESS_STAGE['intake']).toBe(0)
    expect(PROCESS_STAGE['quality_check']).toBe(5)
    expect(PROCESS_STAGE['handover']).toBe(7)
    expect(MACHINE_EVENTS.has('wash')).toBe(true)
    expect(MACHINE_EVENTS.has('intake')).toBe(false)
    expect(MACHINE_EVENTS.has('handover')).toBe(false)

    const record = processEventSchema.parse({
      id: PROCESS_ID,
      tenantId: 'tenant-test',
      orderId: ORDER_ID,
      event: 'wash',
      note: 'Kiloan cycle 1',
      evidenceRef: 'TAG-0001',
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:40:00.000Z',
    })
    expect(record.event).toBe('wash')
  })

  it('validates quality findings and one-shot reconciliation references', () => {
    const finding = findingSchema.parse({
      id: FINDING_ID,
      tenantId: 'tenant-test',
      orderId: ORDER_ID,
      category: 'stain',
      disposition: 'partial',
      quantity: 2,
      reason: 'Ink stain remains after wash',
      evidenceRef: 'photo-11',
      reconcilesId: null,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:30:00.000Z',
    })
    expect(finding.disposition).toBe('partial')

    const reconciliation = findingSchema.parse({
      id: RECONCILES_ID,
      tenantId: 'tenant-test',
      orderId: ORDER_ID,
      category: 'stain',
      disposition: 'accepted',
      quantity: null,
      reason: 'Stain removed on re-press',
      evidenceRef: null,
      reconcilesId: FINDING_ID,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T11:00:00.000Z',
    })
    expect(reconciliation.reconcilesId).toBe(FINDING_ID)
  })

  it('rejects malformed mutations through the laundry contract', () => {
    expect(() =>
      laundryMutationSchema.parse({
        type: 'record_process',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        orderId: ORDER_ID,
        event: 'teleport',
      }),
    ).toThrow()
    expect(() =>
      laundryMutationSchema.parse({
        type: 'change_order_status',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        orderId: ORDER_ID,
        expectedRevision: 1,
        targetStatus: 'received',
        reason: 'Backwards move',
      }),
    ).toThrow()
    expect(() =>
      laundryMutationSchema.parse({
        type: 'create_order',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        orderCode: 'LDR-2026-0044',
        kind: 'weight',
        currency: 'IDR',
        lines: [],
      }),
    ).toThrow()
  })

  it('rejects direct service agent mutations through laundry service', async () => {
    const service = new LaundryService(fakeRepo)
    const mutation = laundryMutationSchema.parse({
      type: 'create_order',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      orderCode: 'LDR-2026-0045',
      kind: 'item',
      currency: 'IDR',
      lines: [{ itemRef: 'press-shirt', quantity: 3, unit: 'piece', unitPriceMinor: 9000 }],
    })

    await expect(
      service.execute(
        'tenant-test',
        { kind: 'service', id: 'bot-auto', onBehalfOf: 'user-1' },
        mutation,
      ),
    ).rejects.toThrowError(LaundryError)
  })

  it('refuses unknown private tools', async () => {
    const service = new LaundryService(fakeRepo)
    await expect(
      executeLaundryTool(service, 'tenant-test', { kind: 'human', id: 'u1' }, 'laundry.refund', {}),
    ).rejects.toMatchObject({ code: 'not_found' })
  })

  it('refuses malformed tool input as a bounded bad request', async () => {
    const service = new LaundryService(fakeRepo)
    await expect(
      executeLaundryTool(
        service,
        'tenant-test',
        { kind: 'human', id: 'u1' },
        'laundry.orderDetail',
        {
          orderId: 'not-a-uuid',
        },
      ),
    ).rejects.toMatchObject({ code: 'bad_request' })
  })
})
