import { z } from 'zod'

export const schedulingIdSchema = z.string().regex(/^[a-z0-9][a-z0-9-]{0,99}$/)
export const schedulingPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: schedulingIdSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: schedulingIdSchema, onBehalfOf: schedulingIdSchema })
    .strict(),
])
export type SchedulingPrincipal = z.infer<typeof schedulingPrincipalSchema>
export const SCHEDULING_PERMISSIONS = [
  'resources.read',
  'resources.write',
  'reservations.read',
  'reservations.write',
  'audit.read',
] as const
export type SchedulingPermission = (typeof SCHEDULING_PERMISSIONS)[number]
export const schedulingMembershipSchema = z
  .object({
    principalId: schedulingIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(SCHEDULING_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const schedulingAccessSchema = z
  .object({
    ownerId: schedulingIdSchema,
    permissions: z.array(z.enum(SCHEDULING_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type SchedulingAccess = z.infer<typeof schedulingAccessSchema>
const title = z.string().trim().min(1).max(160)
const reason = z.string().trim().min(1).max(2000)
const revision = z.number().int().positive().max(2_147_483_647)
/** Offsets must be explicit. Ambiguous wall-clock input and precision beyond milliseconds are refused. */
export const schedulingInstantSchema = z.iso
  .datetime({ offset: true })
  .refine(
    (value) =>
      /^(20\d\d|2100)-.+T\d\d:\d\d:\d\d(?:\.\d{1,3})?(?:Z|[+-]\d\d:\d\d)$/.test(value) &&
      Number.isFinite(Date.parse(value)),
    'Explicit instant in 2000–2100 required',
  )
  .transform((value) => new Date(value).toISOString())
export const schedulingTimezoneSchema = z
  .string()
  .min(1)
  .max(100)
  .refine((value) => {
    try {
      new Intl.DateTimeFormat('en', { timeZone: value })
      return true
    } catch {
      return false
    }
  }, 'Supported IANA timezone required')
const interval = { startsAt: schedulingInstantSchema, endsAt: schedulingInstantSchema }
const validInterval = (value: { startsAt: string; endsAt: string }) => {
  const length = Date.parse(value.endsAt) - Date.parse(value.startsAt)
  return length > 0 && length <= 31 * 86_400_000
}
export const schedulingIntervalSchema = z
  .object(interval)
  .strict()
  .refine(validInterval, 'Interval must be positive and at most 31 days')
export const schedulingResourceSchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    ownerId: schedulingIdSchema,
    title,
    kind: z.enum(['staff', 'asset', 'room', 'space']),
    timezone: schedulingTimezoneSchema,
    active: z.boolean(),
    capacity: z.literal(1),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type SchedulingResource = z.infer<typeof schedulingResourceSchema>
export const reservationSchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    resourceId: z.uuid(),
    ownerId: schedulingIdSchema,
    title,
    kind: z.enum(['booking', 'block']),
    state: z.enum(['held', 'confirmed', 'cancelled']),
    effectiveState: z.enum(['held', 'confirmed', 'cancelled', 'expired']),
    ...interval,
    expiresAt: z.iso.datetime().nullable(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type Reservation = z.infer<typeof reservationSchema>
const reservationRef = { resourceId: z.uuid(), id: z.uuid(), expectedRevision: revision }
export const schedulingCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('resource.create'),
      title,
      kind: z.enum(['staff', 'asset', 'room', 'space']),
      timezone: schedulingTimezoneSchema,
    })
    .strict(),
  z
    .object({
      operation: z.literal('resource.set-active'),
      resourceId: z.uuid(),
      expectedRevision: revision,
      active: z.boolean(),
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('reservation.hold'),
      resourceId: z.uuid(),
      title,
      ...interval,
      holdSeconds: z.number().int().min(1).max(1800).default(900),
    })
    .strict()
    .refine(validInterval),
  z
    .object({
      operation: z.literal('reservation.block'),
      resourceId: z.uuid(),
      title,
      ...interval,
      reason,
    })
    .strict()
    .refine(validInterval),
  z.object({ operation: z.literal('reservation.confirm'), ...reservationRef }).strict(),
  z
    .object({ operation: z.literal('reservation.move'), ...reservationRef, ...interval, reason })
    .strict()
    .refine(validInterval),
  z.object({ operation: z.literal('reservation.cancel'), ...reservationRef, reason }).strict(),
])
export type SchedulingCommand = z.infer<typeof schedulingCommandSchema>
export const schedulingMutationSchema = z
  .object({ requestId: z.uuid(), command: schedulingCommandSchema })
  .strict()
export type SchedulingMutation = z.infer<typeof schedulingMutationSchema>
export const schedulingReceiptSchema = z
  .object({
    kind: z.enum(['resource', 'reservation']),
    id: z.uuid(),
    resourceId: z.uuid(),
    revision,
  })
  .strict()
export type SchedulingReceipt = z.infer<typeof schedulingReceiptSchema>
export const schedulingCommandStatusSchema = z.discriminatedUnion('status', [
  z
    .object({
      status: z.literal('committed'),
      requestId: z.uuid(),
      receipt: schedulingReceiptSchema,
    })
    .strict(),
  z.object({ status: z.literal('not_committed'), requestId: z.uuid() }).strict(),
])
export type SchedulingCommandStatus = z.infer<typeof schedulingCommandStatusSchema>
const paging = { limit: z.number().int().min(1).max(100).default(20), after: z.uuid().optional() }
export const resourcePageSchema = z.object({ ...paging, active: z.boolean().optional() }).strict()
export type ResourcePageQuery = z.infer<typeof resourcePageSchema>
export const reservationPageSchema = z.object({ ...paging, resourceId: z.uuid() }).strict()
export type ReservationPageQuery = z.infer<typeof reservationPageSchema>
export const availabilityQuerySchema = z
  .object({ resourceId: z.uuid(), ...interval })
  .strict()
  .refine(validInterval)
export type AvailabilityQuery = z.infer<typeof availabilityQuerySchema>
export const availabilitySchema = z
  .object({
    resourceId: z.uuid(),
    ...interval,
    available: z.boolean(),
    evaluatedAt: z.iso.datetime(),
    authority: z.literal('snapshot-only'),
  })
  .strict()
export type Availability = z.infer<typeof availabilitySchema>
export const schedulingAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: schedulingIdSchema,
    operation: z.string(),
    resourceId: z.uuid(),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const reservationDetailSchema = z
  .object({
    reservation: reservationSchema,
    evaluatedAt: z.iso.datetime(),
    audit: z.array(schedulingAuditSchema).max(50).nullable(),
    auditHasMore: z.boolean(),
  })
  .strict()
export type ReservationDetail = z.infer<typeof reservationDetailSchema>
export interface SchedulingPage<T> {
  readonly items: readonly T[]
  readonly next: string | null
  readonly evaluatedAt: string
}
export interface SchedulingRepository {
  access(tenant: string, principal: SchedulingPrincipal): Promise<SchedulingAccess>
  resources(
    tenant: string,
    principal: SchedulingPrincipal,
    query: ResourcePageQuery,
  ): Promise<SchedulingPage<SchedulingResource>>
  resource(tenant: string, principal: SchedulingPrincipal, id: string): Promise<SchedulingResource>
  reservations(
    tenant: string,
    principal: SchedulingPrincipal,
    query: ReservationPageQuery,
  ): Promise<SchedulingPage<Reservation>>
  detail(tenant: string, principal: SchedulingPrincipal, id: string): Promise<ReservationDetail>
  availability(
    tenant: string,
    principal: SchedulingPrincipal,
    query: AvailabilityQuery,
  ): Promise<Availability>
  mutate(
    tenant: string,
    principal: SchedulingPrincipal,
    mutation: SchedulingMutation,
  ): Promise<SchedulingReceipt>
  commandStatus(
    tenant: string,
    principal: SchedulingPrincipal,
    id: string,
  ): Promise<SchedulingCommandStatus>
}
export class SchedulingError extends Error {
  readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable'
  constructor(code: SchedulingError['code']) {
    super('Scheduling request refused.')
    this.code = code
  }
}
export const SCHEDULING_MANIFEST = {
  id: 'appointment_scheduling',
  version: '0.1.0',
  apiVersion: '1.0.0',
  capacity: 1,
  owns: ['resources', 'reservations', 'commands', 'audit'],
  permissions: SCHEDULING_PERMISSIONS,
  sourceOfTruth: 'domain-repository',
  scope:
    'exclusive timed reservations; no recurring availability, pooled capacity, room-night pricing, payment or customer confirmation',
} as const
