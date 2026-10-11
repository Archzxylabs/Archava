import { z } from 'zod'
import {
  schedulingIdSchema,
  schedulingPrincipalSchema,
  schedulingTimezoneSchema,
} from '@archava/scheduling'
import {
  calendarDateSchema,
  calendarTimeSchema,
  calendarNights,
} from '@archava/scheduling/calendar'

export {
  schedulingIdSchema as accommodationIdSchema,
  schedulingPrincipalSchema as accommodationPrincipalSchema,
}
export type AccommodationPrincipal = z.infer<typeof schedulingPrincipalSchema>
export const ACCOMMODATION_PERMISSIONS = [
  'properties.read',
  'properties.write',
  'rooms.read',
  'rooms.write',
  'housekeeping.write',
  'rooms.inspect',
  'stays.read',
  'stays.write',
  'audit.read',
] as const
export type AccommodationPermission = (typeof ACCOMMODATION_PERMISSIONS)[number]
export const accommodationMembershipSchema = z
  .object({
    principalId: schedulingIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(ACCOMMODATION_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const accommodationAccessSchema = z
  .object({
    ownerId: schedulingIdSchema,
    permissions: z.array(z.enum(ACCOMMODATION_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type AccommodationAccess = z.infer<typeof accommodationAccessSchema>
const title = z.string().trim().min(1).max(160),
  reason = z.string().trim().min(1).max(2000),
  revision = z.number().int().positive().max(2_147_483_647)
const rate = z.number().int().min(0).max(1_000_000_000),
  instant = z.iso.datetime()
export const propertySchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    ownerId: schedulingIdSchema,
    title,
    timezone: schedulingTimezoneSchema,
    checkInTime: calendarTimeSchema,
    checkOutTime: calendarTimeSchema,
    currency: z.literal('IDR'),
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type Property = z.infer<typeof propertySchema>
export const roomTypeSchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    propertyId: z.uuid(),
    title,
    maxGuests: z.number().int().min(1).max(16),
    nightlyRate: rate,
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type RoomType = z.infer<typeof roomTypeSchema>
export const housekeepingSchema = z.enum(['dirty', 'clean', 'inspected'])
export const roomSchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    propertyId: z.uuid(),
    roomTypeId: z.uuid(),
    code: z.string().trim().min(1).max(40),
    active: z.boolean(),
    housekeeping: housekeepingSchema,
    occupied: z.boolean(),
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type Room = z.infer<typeof roomSchema>
export const stayStateSchema = z.enum(['held', 'confirmed', 'in_house', 'checked_out', 'cancelled'])
export const staySchema = z
  .object({
    id: z.uuid(),
    tenantId: schedulingIdSchema,
    propertyId: z.uuid(),
    roomId: z.uuid(),
    ownerId: schedulingIdSchema,
    guestName: title,
    guests: z.number().int().min(1).max(16),
    arrival: calendarDateSchema,
    departure: calendarDateSchema,
    nights: z.number().int().min(1).max(31),
    nightlyRate: rate,
    rateRevision: revision,
    totalEstimate: z.number().int().min(0).max(31_000_000_000),
    currency: z.literal('IDR'),
    state: stayStateSchema,
    effectiveState: z.enum([
      'held',
      'expired',
      'confirmed',
      'in_house',
      'checked_out',
      'cancelled',
    ]),
    startsAt: instant,
    endsAt: instant,
    expiresAt: instant,
    checkedInAt: instant.nullable(),
    checkedOutAt: instant.nullable(),
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type Stay = z.infer<typeof staySchema>
const dates = { arrival: calendarDateSchema, departure: calendarDateSchema }
const validNights = (x: { arrival: string; departure: string }) => {
  try {
    calendarNights(x.arrival, x.departure)
    return true
  } catch {
    return false
  }
}
const propertyRef = { propertyId: z.uuid() }
const roomRef = { ...propertyRef, roomId: z.uuid(), expectedRevision: revision }
const stayRef = { ...propertyRef, id: z.uuid(), expectedRevision: revision }
export const accommodationCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('property.create'),
      title,
      timezone: schedulingTimezoneSchema,
      checkInTime: calendarTimeSchema.default('15:00'),
      checkOutTime: calendarTimeSchema.default('11:00'),
    })
    .strict()
    .refine((x) => x.checkOutTime <= x.checkInTime, 'Checkout must not be after next arrival time'),
  z
    .object({
      operation: z.literal('room-type.create'),
      ...propertyRef,
      title,
      maxGuests: z.number().int().min(1).max(16),
      nightlyRate: rate,
    })
    .strict(),
  z
    .object({
      operation: z.literal('room-type.set-rate'),
      ...propertyRef,
      id: z.uuid(),
      expectedRevision: revision,
      nightlyRate: rate,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('room.create'),
      ...propertyRef,
      roomTypeId: z.uuid(),
      code: z.string().trim().min(1).max(40),
    })
    .strict(),
  z
    .object({ operation: z.literal('room.set-active'), ...roomRef, active: z.boolean(), reason })
    .strict(),
  z
    .object({
      operation: z.literal('room.housekeeping'),
      ...roomRef,
      state: housekeepingSchema,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('stay.hold'),
      ...propertyRef,
      roomId: z.uuid(),
      ...dates,
      guestName: title,
      guests: z.number().int().min(1).max(16),
      holdSeconds: z.number().int().min(1).max(1800).default(900),
    })
    .strict()
    .refine(validNights, 'Stay must have 1–31 calendar nights'),
  z.object({ operation: z.literal('stay.confirm'), ...stayRef }).strict(),
  z.object({ operation: z.literal('stay.check-in'), ...stayRef }).strict(),
  z.object({ operation: z.literal('stay.check-out'), ...stayRef }).strict(),
  z.object({ operation: z.literal('stay.cancel'), ...stayRef, reason }).strict(),
])
export type AccommodationCommand = z.infer<typeof accommodationCommandSchema>
export const accommodationMutationSchema = z
  .object({ requestId: z.uuid(), command: accommodationCommandSchema })
  .strict()
export type AccommodationMutation = z.infer<typeof accommodationMutationSchema>
export const accommodationReceiptSchema = z
  .object({
    kind: z.enum(['property', 'room-type', 'room', 'stay']),
    id: z.uuid(),
    propertyId: z.uuid(),
    revision,
  })
  .strict()
export type AccommodationReceipt = z.infer<typeof accommodationReceiptSchema>
export const accommodationCommandStatusSchema = z.discriminatedUnion('status', [
  z
    .object({
      status: z.literal('committed'),
      requestId: z.uuid(),
      receipt: accommodationReceiptSchema,
    })
    .strict(),
  z.object({ status: z.literal('not_committed'), requestId: z.uuid() }).strict(),
])
export type AccommodationCommandStatus = z.infer<typeof accommodationCommandStatusSchema>
const paging = { limit: z.number().int().min(1).max(100).default(20), after: z.uuid().optional() }
export const propertyPageSchema = z.object(paging).strict()
export type PropertyPageQuery = z.infer<typeof propertyPageSchema>
export const accommodationPageSchema = z.object({ ...paging, ...propertyRef }).strict()
export type AccommodationPageQuery = z.infer<typeof accommodationPageSchema>
export const roomAvailabilityQuerySchema = z
  .object({
    ...propertyRef,
    roomId: z.uuid(),
    ...dates,
    guests: z.number().int().min(1).max(16).default(1),
  })
  .strict()
  .refine(validNights)
export type RoomAvailabilityQuery = z.infer<typeof roomAvailabilityQuerySchema>
export const roomAvailabilitySchema = z
  .object({
    ...propertyRef,
    roomId: z.uuid(),
    ...dates,
    available: z.boolean(),
    nights: z.number().int().min(1).max(31),
    nightlyRate: rate,
    totalEstimate: z.number().int().min(0).max(31_000_000_000),
    currency: z.literal('IDR'),
    housekeeping: housekeepingSchema,
    occupied: z.boolean(),
    evaluatedAt: instant,
    authority: z.literal('snapshot-only'),
  })
  .strict()
export type RoomAvailability = z.infer<typeof roomAvailabilitySchema>
export const stayNightSchema = z
  .object({ date: calendarDateSchema, rate, rateRevision: revision })
  .strict()
export const accommodationAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: schedulingIdSchema,
    operation: z.string(),
    propertyId: z.uuid(),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: instant,
  })
  .strict()
export const stayDetailSchema = z
  .object({
    stay: staySchema,
    property: propertySchema,
    room: roomSchema,
    nights: z.array(stayNightSchema).min(1).max(31),
    audit: z.array(accommodationAuditSchema).max(50).nullable(),
    auditHasMore: z.boolean(),
    evaluatedAt: instant,
  })
  .strict()
export type StayDetail = z.infer<typeof stayDetailSchema>
export interface AccommodationPage<T> {
  readonly items: readonly T[]
  readonly next: string | null
  readonly evaluatedAt: string
}
export interface AccommodationRepository {
  access(tenant: string, principal: AccommodationPrincipal): Promise<AccommodationAccess>
  properties(
    tenant: string,
    principal: AccommodationPrincipal,
    query: PropertyPageQuery,
  ): Promise<AccommodationPage<Property>>
  property(tenant: string, principal: AccommodationPrincipal, id: string): Promise<Property>
  roomTypes(
    tenant: string,
    principal: AccommodationPrincipal,
    query: AccommodationPageQuery,
  ): Promise<AccommodationPage<RoomType>>
  rooms(
    tenant: string,
    principal: AccommodationPrincipal,
    query: AccommodationPageQuery,
  ): Promise<AccommodationPage<Room>>
  room(tenant: string, principal: AccommodationPrincipal, id: string): Promise<Room>
  stays(
    tenant: string,
    principal: AccommodationPrincipal,
    query: AccommodationPageQuery,
  ): Promise<AccommodationPage<Stay>>
  detail(tenant: string, principal: AccommodationPrincipal, id: string): Promise<StayDetail>
  availability(
    tenant: string,
    principal: AccommodationPrincipal,
    query: RoomAvailabilityQuery,
  ): Promise<RoomAvailability>
  mutate(
    tenant: string,
    principal: AccommodationPrincipal,
    mutation: AccommodationMutation,
  ): Promise<AccommodationReceipt>
  commandStatus(
    tenant: string,
    principal: AccommodationPrincipal,
    id: string,
  ): Promise<AccommodationCommandStatus>
}
export class AccommodationError extends Error {
  readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable'
  constructor(code: AccommodationError['code']) {
    super('Accommodation request refused.')
    this.code = code
  }
}
export const ACCOMMODATION_MANIFEST = {
  id: 'accommodation_stays',
  version: '0.1.0',
  apiVersion: '1.0.0',
  requires: ['scheduling:0.2.0'],
  owns: ['properties', 'room_types', 'rooms', 'stays', 'stay_nights', 'commands', 'audit'],
  permissions: ACCOMMODATION_PERMISSIONS,
  sourceOfTruth: 'domain-repository',
  scope:
    'individual room-night stays, IDR rate snapshots and vacant-room housekeeping; no payments, taxes, channel manager or full PMS acceptance',
} as const
