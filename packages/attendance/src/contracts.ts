import { z } from 'zod'

export const attendanceTenantSchema = z.string().trim().min(1).max(100)

export const attendancePrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: attendanceTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: attendanceTenantSchema,
      onBehalfOf: attendanceTenantSchema,
    })
    .strict(),
])
export type AttendancePrincipal = z.infer<typeof attendancePrincipalSchema>

export const ATTENDANCE_PERMISSIONS = [
  'attendance.read',
  'attendance.write',
  'attendance.checkin',
  'audit.read',
] as const
export type AttendancePermission = (typeof ATTENDANCE_PERMISSIONS)[number]

export const attendanceMembershipSchema = z
  .object({
    principalId: attendanceTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(ATTENDANCE_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type AttendanceMembership = z.infer<typeof attendanceMembershipSchema>

export const attendanceAccessSchema = z
  .object({
    permissions: z.array(z.enum(ATTENDANCE_PERMISSIONS)),
    ownerId: attendanceTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type AttendanceAccess = z.infer<typeof attendanceAccessSchema>

export const SESSION_STATUSES = [
  'draft',
  'scheduled',
  'in_progress',
  'completed',
  'canceled',
] as const
export type SessionStatus = (typeof SESSION_STATUSES)[number]

export const ROSTER_ROLES = ['attendee', 'instructor', 'speaker', 'staff'] as const
export type RosterRole = (typeof ROSTER_ROLES)[number]

export const ROSTER_STATUSES = [
  'registered',
  'waitlisted',
  'confirmed',
  'attended',
  'canceled',
  'no_show',
] as const
export type RosterStatus = (typeof ROSTER_STATUSES)[number]

export const CHECK_IN_METHODS = ['qr_scan', 'manual_operator', 'kiosk'] as const
export type CheckInMethod = (typeof CHECK_IN_METHODS)[number]

export const sessionSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: attendanceTenantSchema,
    title: z.string().trim().min(1).max(200),
    sessionCode: z.string().trim().min(1).max(50),
    capacity: z.number().int().positive().nullable(),
    startTime: z.string().datetime(),
    endTime: z.string().datetime(),
    status: z.enum(SESSION_STATUSES),
    location: z.string().trim().max(300).nullable(),
    enrolledCount: z.number().int().nonnegative().default(0),
    attendedCount: z.number().int().nonnegative().default(0),
    revision: z.number().int().positive().default(1),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type Session = z.infer<typeof sessionSchema>

export const rosterEntrySchema = z
  .object({
    id: z.string().uuid(),
    tenantId: attendanceTenantSchema,
    sessionId: z.string().uuid(),
    contactId: z.string().uuid(),
    role: z.enum(ROSTER_ROLES),
    status: z.enum(ROSTER_STATUSES),
    ticketCode: z.string().trim().min(1).max(100),
    checkedInAt: z.string().datetime().nullable(),
    checkInMethod: z.enum(CHECK_IN_METHODS).nullable(),
    revision: z.number().int().positive().default(1),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type RosterEntry = z.infer<typeof rosterEntrySchema>

export const checkInRecordSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: attendanceTenantSchema,
    rosterId: z.string().uuid(),
    sessionId: z.string().uuid(),
    contactId: z.string().uuid(),
    method: z.enum(CHECK_IN_METHODS),
    evidenceRef: z.string().trim().max(300).nullable(),
    idempotencyKey: z.string().trim().min(1).max(100),
    recordedBy: attendanceTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type CheckInRecord = z.infer<typeof checkInRecordSchema>

export const sessionDetailSchema = z
  .object({
    session: sessionSchema,
    roster: z.array(rosterEntrySchema),
    checkIns: z.array(checkInRecordSchema),
  })
  .strict()
export type SessionDetail = z.infer<typeof sessionDetailSchema>

export const attendanceMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_session'),
      commandId: z.string().uuid(),
      title: z.string().trim().min(1).max(200),
      sessionCode: z.string().trim().min(1).max(50),
      capacity: z.number().int().positive().nullable().optional(),
      startTime: z.string().datetime(),
      endTime: z.string().datetime(),
      location: z.string().trim().max(300).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('register_participant'),
      commandId: z.string().uuid(),
      sessionId: z.string().uuid(),
      contactId: z.string().uuid(),
      role: z.enum(ROSTER_ROLES).default('attendee'),
      ticketCode: z.string().trim().min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_check_in'),
      commandId: z.string().uuid(),
      sessionId: z.string().uuid(),
      rosterId: z.string().uuid(),
      method: z.enum(CHECK_IN_METHODS),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
      idempotencyKey: z.string().trim().min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('update_roster_status'),
      commandId: z.string().uuid(),
      rosterId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      targetStatus: z.enum(['confirmed', 'canceled', 'no_show', 'waitlisted']),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
  z
    .object({
      type: z.literal('change_session_status'),
      commandId: z.string().uuid(),
      sessionId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      targetStatus: z.enum(['scheduled', 'in_progress', 'completed', 'canceled']),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
])
export type AttendanceMutation = z.infer<typeof attendanceMutationSchema>

export const attendanceReceiptSchema = z
  .object({
    commandId: z.string().uuid(),
    tenantId: attendanceTenantSchema,
    action: z.string().min(1),
    sessionId: z.string().uuid().nullable().optional(),
    rosterId: z.string().uuid().nullable().optional(),
    checkInId: z.string().uuid().nullable().optional(),
    status: z.string().min(1),
    revision: z.number().int().nonnegative().nullable().optional(),
    idempotencyReplay: z.boolean().default(false),
    executedAt: z.string().datetime(),
  })
  .strict()
export type AttendanceReceipt = z.infer<typeof attendanceReceiptSchema>

export const attendanceCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('recorded'), receipt: attendanceReceiptSchema }).strict(),
])
export type AttendanceCommandStatus = z.infer<typeof attendanceCommandStatusSchema>

export const sessionPageQuerySchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    offset: z.number().int().nonnegative().default(0),
    status: z.enum(SESSION_STATUSES).optional(),
  })
  .strict()
export type SessionPageQuery = z.infer<typeof sessionPageQuerySchema>

export interface AttendanceRepository {
  execute(
    tenant: string,
    principal: AttendancePrincipal,
    mutation: AttendanceMutation,
  ): Promise<AttendanceReceipt>
  getCommandStatus(
    tenant: string,
    principal: AttendancePrincipal,
    commandId: string,
  ): Promise<AttendanceCommandStatus>
  getAccess(tenant: string, principal: AttendancePrincipal): Promise<AttendanceAccess>
  listSessions(
    tenant: string,
    principal: AttendancePrincipal,
    query: SessionPageQuery,
  ): Promise<readonly Session[]>
  getSessionDetail(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<SessionDetail | null>
  listRoster(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly RosterEntry[]>
  listCheckIns(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly CheckInRecord[]>
}

export type AttendanceErrorCode =
  | 'forbidden'
  | 'unauthorized'
  | 'not_found'
  | 'conflict'
  | 'bad_request'
  | 'capacity_exceeded'
  | 'already_checked_in'

export class AttendanceError extends Error {
  constructor(
    public readonly code: AttendanceErrorCode,
    message?: string,
  ) {
    super(message ?? `Attendance error: ${code}`)
    this.name = 'AttendanceError'
  }
}
