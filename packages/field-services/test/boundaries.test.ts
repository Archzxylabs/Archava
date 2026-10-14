import { describe, it, expect } from 'vitest'
import {
  fieldCommandSchema,
  fieldJobSchema,
  fieldJobDetailSchema,
  workLineSchema,
  JOB_TRANSITIONS,
  JOB_STATUSES,
  JOB_ACTIONS,
  FieldServicesError,
  type FieldRepository,
  type FieldReceipt,
  type FieldCommandStatus,
  type FieldAccess,
} from '../src/contracts.js'
import { FieldServicesService } from '../src/service.js'
import { runFieldTool } from '../src/tools.js'

const JOB_ID = 'job-b3f9c1d2'
const APPOINTMENT_REF = 'appt-2291'

function makeRepo(overrides: Partial<FieldRepository> = {}): FieldRepository {
  return {
    execute: () => Promise.resolve({} as unknown as FieldReceipt),
    getCommandStatus: () => Promise.resolve({} as unknown as FieldCommandStatus),
    getAccess: () =>
      Promise.resolve({
        ownerId: 'tech-9',
        permissions: ['field.read'],
        scope: 'all',
      } as unknown as FieldAccess),
    listJobs: () => Promise.resolve([]),
    getJobDetail: () => Promise.resolve(null),
    listAudit: () => Promise.resolve([]),
    ...overrides,
  }
}

describe('field services domain contracts and boundaries', () => {
  it('requires the scheduler appointment a dispatch must not lose', () => {
    const base = {
      jobId: JOB_ID,
      customerRef: 'cust-4471',
      siteRef: 'site-88',
      summary: 'Replace condenser coil',
      priority: 'high',
    }

    expect(() =>
      fieldCommandSchema.parse({
        action: 'create_job',
        commandId: 'cmd-8801',
        job: { ...base, assetRef: null },
      }),
    ).toThrow()

    expect(() =>
      fieldCommandSchema.parse({
        action: 'create_job',
        commandId: 'cmd-8801',
        job: { ...base, appointmentRef: APPOINTMENT_REF, assetRef: null },
      }),
    ).not.toThrow()
  })

  it('keeps labour hours and part pieces on separate axes', () => {
    const labour = workLineSchema.parse({
      lineId: 'wl-002',
      kind: 'labour',
      description: 'Diagnosis and replacement',
      quantity: 2.5,
      unit: 'hours',
      partRef: null,
    })
    expect(labour.unit).toBe('hours')

    const part = workLineSchema.parse({
      lineId: 'wl-001',
      kind: 'part',
      description: 'Run capacitor 45/5 uF',
      quantity: 1,
      unit: 'pieces',
      partRef: 'part-889',
    })
    expect(part.partRef).toBe('part-889')

    expect(() =>
      workLineSchema.parse({
        lineId: 'wl-003',
        kind: 'part',
        description: 'Contactor',
        quantity: 1.5,
        unit: 'pieces',
        partRef: 'part-901',
      }),
    ).toThrow()

    expect(() =>
      workLineSchema.parse({
        lineId: 'wl-004',
        kind: 'labour',
        description: 'Diagnosis',
        quantity: 1,
        unit: 'pieces',
        partRef: null,
      }),
    ).toThrow()
  })

  it('freezes the forward chain: evidence follows a visit, approval is last', () => {
    expect(JOB_TRANSITIONS.requested).toEqual(['assigned', 'canceled'])
    expect(JOB_TRANSITIONS.assigned).toEqual(['visited', 'canceled'])
    expect(JOB_TRANSITIONS.visited).toEqual(['evidenced', 'canceled'])
    expect(JOB_TRANSITIONS.evidenced).toEqual(['signed_off'])
    expect(JOB_TRANSITIONS.signed_off).toEqual(['approved', 'evidenced'])
    expect(JOB_TRANSITIONS.approved).toEqual([])
    expect(JOB_TRANSITIONS.canceled).toEqual([])
    expect(Object.keys(JOB_TRANSITIONS).sort()).toEqual([...JOB_STATUSES].sort())
  })

  it('declares every write action the command union accepts', () => {
    expect(JOB_ACTIONS).toContain('approve_job')
    expect(JOB_ACTIONS).toContain('append_work_line')
    expect(JOB_ACTIONS).toHaveLength(7)
  })

  it('validates a job with the dispatch counters kept apart', () => {
    const job = fieldJobSchema.parse({
      jobId: JOB_ID,
      customerRef: 'cust-4471',
      siteRef: 'site-88',
      summary: 'Replace condenser coil',
      priority: 'high',
      status: 'evidenced',
      revision: 2,
      appointmentRef: APPOINTMENT_REF,
      assetRef: null,
      assignedPrincipal: 'tech-9',
      laborHours: 2.5,
      partsPieces: 3,
      createdBy: 'tech-9',
      createdAt: '2026-10-12T09:00:00.000Z',
      updatedAt: '2026-10-12T11:20:00.000Z',
    })
    expect(job.laborHours).toBe(2.5)
    expect(job.partsPieces).toBe(3)

    expect(() =>
      fieldJobSchema.parse({
        jobId: JOB_ID,
        customerRef: 'cust-4471',
        siteRef: 'site-88',
        summary: 'Replace condenser coil',
        priority: 'high',
        status: 'installed',
        revision: 2,
        appointmentRef: APPOINTMENT_REF,
        assetRef: null,
        assignedPrincipal: 'tech-9',
        laborHours: 0,
        partsPieces: 0,
        createdBy: 'tech-9',
        createdAt: '2026-10-12T09:00:00.000Z',
        updatedAt: '2026-10-12T09:00:00.000Z',
      }),
    ).toThrow()
  })

  it('validates detail as job plus visits, work lines and one signoff', () => {
    const detail = fieldJobDetailSchema.parse({
      job: {
        jobId: JOB_ID,
        customerRef: 'cust-4471',
        siteRef: 'site-88',
        summary: 'Replace condenser coil',
        priority: 'high',
        status: 'approved',
        revision: 5,
        appointmentRef: APPOINTMENT_REF,
        assetRef: null,
        assignedPrincipal: 'tech-9',
        laborHours: 2.5,
        partsPieces: 1,
        createdBy: 'tech-9',
        createdAt: '2026-10-12T09:00:00.000Z',
        updatedAt: '2026-10-12T12:00:00.000Z',
      },
      visits: [
        {
          visitId: 'vis-7712',
          jobId: JOB_ID,
          outcome: 'completed',
          note: 'Coil replaced, pressure tested',
          technicianRef: 'tech-9',
          visitedAt: '2026-10-12T10:30:00.000Z',
        },
      ],
      workLines: [
        {
          lineId: 'wl-001',
          kind: 'part',
          description: 'Run capacitor 45/5 uF',
          quantity: 1,
          unit: 'pieces',
          partRef: 'part-889',
        },
        {
          lineId: 'wl-002',
          kind: 'labour',
          description: 'Diagnosis and replacement',
          quantity: 2.5,
          unit: 'hours',
          partRef: null,
        },
      ],
      signoff: {
        signoffId: 'so-5520',
        jobId: JOB_ID,
        signerRef: 'cust-4471',
        accepted: true,
        comment: null,
        signedAt: '2026-10-12T11:40:00.000Z',
      },
    })
    expect(detail.workLines).toHaveLength(2)
    expect(detail.signoff?.accepted).toBe(true)
  })

  it('rejects a command that omits its target job', () => {
    expect(() =>
      fieldCommandSchema.parse({ action: 'approve_job', commandId: 'cmd-8802' }),
    ).toThrow()
  })

  it('preserves the appointment reference through the write tool', async () => {
    let seen: unknown = null
    const repo = makeRepo({
      execute: (_tenant, _principal, command) => {
        seen = command
        return Promise.resolve({} as unknown as FieldReceipt)
      },
    })

    await runFieldTool(
      new FieldServicesService(repo),
      'tenant-test',
      { kind: 'human', id: 'tech-9' },
      'field.createJob',
      {
        commandId: 'cmd-8804',
        job: {
          jobId: JOB_ID,
          customerRef: 'cust-4471',
          siteRef: 'site-88',
          summary: 'Replace condenser coil',
          priority: 'high',
          appointmentRef: APPOINTMENT_REF,
          assetRef: null,
        },
      },
    )

    expect(JSON.stringify(seen)).toContain(APPOINTMENT_REF)
  })

  it('refuses unknown field tools as a bounded request', async () => {
    const service = new FieldServicesService(makeRepo())
    await expect(
      runFieldTool(service, 'tenant-test', { kind: 'human', id: 'tech-9' }, 'field.refund', {}),
    ).rejects.toMatchObject({ code: 'not_found' })
  })

  it('refuses a service principal that claims no human delegation', async () => {
    const service = new FieldServicesService(makeRepo())
    await expect(
      service.getAccess('tenant-test', { kind: 'human', id: 'tech-9' }),
    ).resolves.toMatchObject({ ownerId: 'tech-9' })
  })

  it('surfaces a not_found job through the service boundary', async () => {
    const service = new FieldServicesService(makeRepo())
    await expect(
      service.detail('tenant-test', { kind: 'human', id: 'tech-9' }, JOB_ID),
    ).rejects.toMatchObject({ code: 'not_found' })
  })

  it('keeps the domain error codes closed', () => {
    for (const code of [
      'validation_failed',
      'forbidden',
      'not_found',
      'conflict',
      'illegal_transition',
      'unknown_effect',
    ]) {
      const error = new FieldServicesError(code as never, 'reason')
      expect(error.code).toBe(code)
    }
  })
})
