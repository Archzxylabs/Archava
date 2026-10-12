import { describe, it, expect } from 'vitest'
import {
  sessionSchema,
  rosterEntrySchema,
  checkInRecordSchema,
  attendanceMutationSchema,
  AttendanceError,
  type AttendanceRepository,
  type AttendanceReceipt,
} from '../src/contracts.js'
import { AttendanceService } from '../src/service.js'

describe('attendance domain contracts and boundaries', () => {
  it('validates session model and enforces ISO datetime and code constraints', () => {
    const valid = sessionSchema.parse({
      id: '550e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      title: 'Fullstack Bootcamp Week 1',
      sessionCode: 'FS-BC-W1',
      capacity: 35,
      startTime: '2026-10-15T09:00:00.000Z',
      endTime: '2026-10-15T17:00:00.000Z',
      status: 'scheduled',
      location: 'Lab Room 3B',
      enrolledCount: 12,
      attendedCount: 0,
      revision: 1,
      createdAt: '2026-10-12T10:00:00.000Z',
      updatedAt: '2026-10-12T10:00:00.000Z',
    })
    expect(valid.sessionCode).toBe('FS-BC-W1')
    expect(valid.capacity).toBe(35)
  })

  it('rejects invalid session dates and negative capacity', () => {
    expect(() =>
      sessionSchema.parse({
        id: '550e8400-e29b-41d4-a716-446655440000',
        tenantId: 'tenant-test',
        title: 'Fullstack Bootcamp Week 1',
        sessionCode: 'FS-BC-W1',
        capacity: -5, // invalid
        startTime: 'not-a-date',
        endTime: 'not-a-date',
        status: 'scheduled',
        location: null,
        enrolledCount: 0,
        attendedCount: 0,
        revision: 1,
        createdAt: '2026-10-12T10:00:00.000Z',
        updatedAt: '2026-10-12T10:00:00.000Z',
      }),
    ).toThrow()
  })

  it('validates roster entry with attendee role and ticket code', () => {
    const roster = rosterEntrySchema.parse({
      id: '660e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      sessionId: '550e8400-e29b-41d4-a716-446655440000',
      contactId: '770e8400-e29b-41d4-a716-446655440000',
      role: 'attendee',
      status: 'confirmed',
      ticketCode: 'TCK-99201',
      checkedInAt: null,
      checkInMethod: null,
      revision: 1,
      createdAt: '2026-10-12T10:00:00.000Z',
      updatedAt: '2026-10-12T10:00:00.000Z',
    })
    expect(roster.role).toBe('attendee')
    expect(roster.ticketCode).toBe('TCK-99201')
  })

  it('validates check-in event idempotency key and method', () => {
    const checkIn = checkInRecordSchema.parse({
      id: '880e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      rosterId: '660e8400-e29b-41d4-a716-446655440000',
      sessionId: '550e8400-e29b-41d4-a716-446655440000',
      contactId: '770e8400-e29b-41d4-a716-446655440000',
      method: 'qr_scan',
      evidenceRef: 'Scanner gate A',
      idempotencyKey: 'scan-660e8400-01',
      recordedBy: 'operator-gate-1',
      recordedAt: '2026-10-15T08:55:00.000Z',
    })
    expect(checkIn.method).toBe('qr_scan')
    expect(checkIn.idempotencyKey).toBe('scan-660e8400-01')
  })

  it('rejects direct service agent mutations through attendance service', async () => {
    const fakeRepo: AttendanceRepository = {
      execute: () => Promise.resolve({} as unknown as AttendanceReceipt),
      getCommandStatus: () => Promise.resolve({ status: 'unknown' }),
      getAccess: () =>
        Promise.resolve({
          ownerId: 'u1',
          permissions: [],
          scope: 'all' as const,
        }),
      listSessions: () => Promise.resolve([]),
      getSessionDetail: () => Promise.resolve(null),
      listRoster: () => Promise.resolve([]),
      listCheckIns: () => Promise.resolve([]),
    }
    const service = new AttendanceService(fakeRepo)

    const mutation = attendanceMutationSchema.parse({
      type: 'create_session',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      title: 'AI Engineering Workshop',
      sessionCode: 'AI-ENG-01',
      capacity: 50,
      startTime: '2026-11-01T10:00:00.000Z',
      endTime: '2026-11-01T16:00:00.000Z',
    })

    await expect(
      service.execute(
        'tenant-test',
        { kind: 'service', id: 'bot-auto', onBehalfOf: 'user-1' },
        mutation,
      ),
    ).rejects.toThrowError(AttendanceError)
  })
})
