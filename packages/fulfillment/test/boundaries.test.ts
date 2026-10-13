import { describe, it, expect } from 'vitest'
import {
  jobSchema,
  jobLineSchema,
  findingSchema,
  custodyEventSchema,
  fulfillmentMutationSchema,
  CUSTODY_STAGE,
  FulfillmentError,
  type FulfillmentRepository,
  type FulfillmentReceipt,
} from '../src/contracts.js'
import { FulfillmentService } from '../src/service.js'

const JOB_ID = '550e8400-e29b-41d4-a716-446655440000'
const LINE_ID = '660e8400-e29b-41d4-a716-446655440000'
const FINDING_ID = '770e8400-e29b-41d4-a716-446655440000'
const CUSTODY_ID = '880e8400-e29b-41d4-a716-446655440000'
const RECONCILES_ID = '990e8400-e29b-41d4-a716-446655440000'

const fakeRepo: FulfillmentRepository = {
  execute: () => Promise.resolve({} as unknown as FulfillmentReceipt),
  getCommandStatus: () => Promise.resolve({ status: 'unknown' }),
  getAccess: () => Promise.resolve({ ownerId: 'u1', permissions: [], scope: 'all' as const }),
  listJobs: () => Promise.resolve([]),
  getJobDetail: () => Promise.resolve(null),
  listLines: () => Promise.resolve([]),
  listFindings: () => Promise.resolve([]),
  listCustody: () => Promise.resolve([]),
}

describe('fulfillment domain contracts and boundaries', () => {
  it('validates a dispatch job with bounded counters and per-client owner reference', () => {
    const job = jobSchema.parse({
      id: JOB_ID,
      tenantId: 'tenant-test',
      jobCode: 'SHP-2026-0042',
      kind: 'dispatch',
      title: 'Restock to Bandung DC',
      ownerRef: 'client-alpha-logistics',
      status: 'in_progress',
      consumedCount: 12,
      producedCount: 0,
      scrapCount: 0,
      unreconciledCount: 1,
      revision: 3,
      createdAt: '2026-10-12T10:00:00.000Z',
      updatedAt: '2026-10-12T11:00:00.000Z',
    })
    expect(job.kind).toBe('dispatch')
    expect(job.ownerRef).toBe('client-alpha-logistics')
    expect(job.unreconciledCount).toBe(1)
  })

  it('rejects negative counters and unknown job status', () => {
    expect(() =>
      jobSchema.parse({
        id: JOB_ID,
        tenantId: 'tenant-test',
        jobCode: 'SHP-2026-0042',
        kind: 'dispatch',
        title: 'Restock to Bandung DC',
        ownerRef: null,
        status: 'shipped',
        consumedCount: -4,
        producedCount: 0,
        scrapCount: 0,
        unreconciledCount: 0,
        revision: 1,
        createdAt: '2026-10-12T10:00:00.000Z',
        updatedAt: '2026-10-12T10:00:00.000Z',
      }),
    ).toThrow()
  })

  it('validates production material and scrap lines with positive integer quantities', () => {
    const line = jobLineSchema.parse({
      id: LINE_ID,
      tenantId: 'tenant-test',
      jobId: JOB_ID,
      direction: 'consume',
      itemRef: 'TEAK-PLANK-18',
      quantity: 18,
      disposition: 'normal',
      evidenceRef: 'BOM v2',
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:05:00.000Z',
    })
    expect(line.quantity).toBe(18)
    expect(line.disposition).toBe('normal')

    expect(() =>
      jobLineSchema.parse({
        id: LINE_ID,
        tenantId: 'tenant-test',
        jobId: JOB_ID,
        direction: 'consume',
        itemRef: 'TEAK-PLANK-18',
        quantity: 0,
        disposition: 'normal',
        evidenceRef: null,
        recordedBy: 'operator-1',
        recordedAt: '2026-10-12T10:05:00.000Z',
      }),
    ).toThrow()
  })

  it('validates delivery findings and reconciliation references', () => {
    const finding = findingSchema.parse({
      id: FINDING_ID,
      tenantId: 'tenant-test',
      jobId: JOB_ID,
      category: 'delivery',
      disposition: 'partial',
      quantity: 4,
      reason: 'Two cartons short on arrival',
      evidenceRef: 'POD photo 11',
      reconcilesId: null,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:30:00.000Z',
    })
    expect(finding.disposition).toBe('partial')

    const reconciliation = findingSchema.parse({
      id: CUSTODY_ID,
      tenantId: 'tenant-test',
      jobId: JOB_ID,
      category: 'delivery',
      disposition: 'accepted',
      quantity: null,
      reason: 'Short cartons rescheduled on next trip',
      evidenceRef: null,
      reconcilesId: RECONCILES_ID,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T11:00:00.000Z',
    })
    expect(reconciliation.reconcilesId).toBe(RECONCILES_ID)
  })

  it('keeps the custody chain order explicit and starts at received', () => {
    expect(CUSTODY_STAGE['received']).toBe(0)
    expect(CUSTODY_STAGE['dispatched']).toBe(4)
    expect(CUSTODY_STAGE['delivered']).toBe(5)
    expect(CUSTODY_STAGE['returned']).toBeUndefined()

    const custody = custodyEventSchema.parse({
      id: CUSTODY_ID,
      tenantId: 'tenant-test',
      jobId: JOB_ID,
      event: 'dispatched',
      fromRef: 'warehouse-bay-3',
      toRef: 'carrier-nusantara',
      evidenceRef: 'DO-77120',
      note: null,
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:40:00.000Z',
    })
    expect(custody.event).toBe('dispatched')
  })

  it('rejects malformed mutations through the fulfillment contract', () => {
    expect(() =>
      fulfillmentMutationSchema.parse({
        type: 'record_line',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        jobId: JOB_ID,
        direction: 'sideways',
        itemRef: 'TEAK-PLANK-18',
        quantity: 3,
      }),
    ).toThrow()
    expect(() =>
      fulfillmentMutationSchema.parse({
        type: 'change_job_status',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        jobId: JOB_ID,
        expectedRevision: 1,
        targetStatus: 'draft',
        reason: 'Backwards move',
      }),
    ).toThrow()
  })

  it('rejects direct service agent mutations through fulfillment service', async () => {
    const service = new FulfillmentService(fakeRepo)
    const mutation = fulfillmentMutationSchema.parse({
      type: 'create_job',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      jobCode: 'WO-2026-009',
      kind: 'production',
      title: 'Custom teak desk',
    })

    await expect(
      service.execute(
        'tenant-test',
        { kind: 'service', id: 'bot-auto', onBehalfOf: 'user-1' },
        mutation,
      ),
    ).rejects.toThrowError(FulfillmentError)
  })
})
