import { z } from 'zod'
export const foodIdSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const foodPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: foodIdSchema }).strict(),
  z.object({ kind: z.literal('service'), id: foodIdSchema, onBehalfOf: foodIdSchema }).strict(),
])
export type FoodPrincipal = z.infer<typeof foodPrincipalSchema>
export const FOOD_PERMISSIONS = [
  'tables.read',
  'tables.write',
  'menu.read',
  'menu.write',
  'tickets.read',
  'tickets.write',
  'kitchen.write',
  'audit.read',
] as const
export type FoodPermission = (typeof FOOD_PERMISSIONS)[number]
export const foodMembershipSchema = z
  .object({
    principalId: foodIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(FOOD_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const foodAccessSchema = z
  .object({
    permissions: z.array(z.enum(FOOD_PERMISSIONS)),
    ownerId: foodIdSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type FoodAccess = z.infer<typeof foodAccessSchema>
const title = z.string().trim().min(1).max(160),
  reason = z.string().trim().min(1).max(2000),
  revision = z.number().int().positive().max(2147483647),
  price = z.number().int().min(0).max(1000000000)
export const recipeComponentsSchema = z
  .array(z.object({ productId: z.uuid(), quantity: z.number().int().min(1).max(1000000) }).strict())
  .min(1)
  .max(20)
  .superRefine((xs, ctx) => {
    if (new Set(xs.map((x) => x.productId)).size !== xs.length)
      ctx.addIssue({ code: 'custom', message: 'Recipe ingredients must be unique' })
  })
export const foodTableSchema = z
  .object({
    id: z.uuid(),
    tenantId: foodIdSchema,
    code: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,19}$/),
    title,
    seats: z.number().int().min(1).max(40),
    active: z.boolean(),
    occupied: z.boolean(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type FoodTable = z.infer<typeof foodTableSchema>
export const menuItemSchema = z
  .object({
    id: z.uuid(),
    tenantId: foodIdSchema,
    sku: z.string().min(1).max(40),
    title,
    price,
    currency: z.literal('IDR'),
    station: z.enum(['kitchen', 'bar']),
    active: z.boolean(),
    recipeVersion: revision,
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type MenuItem = z.infer<typeof menuItemSchema>
export const menuDetailSchema = z
  .object({ item: menuItemSchema, recipe: recipeComponentsSchema })
  .strict()
export type MenuDetail = z.infer<typeof menuDetailSchema>
export const FOOD_TICKET_STATES = [
  'draft',
  'queued',
  'preparing',
  'ready',
  'served',
  'closed',
  'cancelled',
] as const
export const ticketSchema = z
  .object({
    id: z.uuid(),
    tenantId: foodIdSchema,
    ownerId: foodIdSchema,
    orderId: z.uuid(),
    customerName: title,
    mode: z.enum(['dine_in', 'takeaway']),
    tableId: z.uuid().nullable(),
    guests: z.number().int().min(1).max(40),
    state: z.enum(FOOD_TICKET_STATES),
    stockExpired: z.boolean(),
    expiresAt: z.iso.datetime().nullable(),
    totalEstimate: z.number().int().min(0).max(500000000000000),
    currency: z.literal('IDR'),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type FoodTicket = z.infer<typeof ticketSchema>
export const ticketLineSchema = z
  .object({
    menuId: z.uuid(),
    orderLineId: z.uuid(),
    title,
    sku: z.string().min(1).max(40),
    quantity: z.number().int().min(1).max(100),
    price,
    station: z.enum(['kitchen', 'bar']),
    recipeVersion: revision,
  })
  .strict()
export const foodAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: foodIdSchema,
    operation: z.string().min(1).max(100),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const ticketDetailSchema = z
  .object({
    ticket: ticketSchema,
    lines: z.array(ticketLineSchema).min(1).max(20),
    audit: z.array(foodAuditSchema).max(100).nullable(),
  })
  .strict()
export type TicketDetail = z.infer<typeof ticketDetailSchema>
export const foodPageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    after: z.uuid().optional(),
    active: z.boolean().optional(),
  })
  .strict()
export type FoodPageQuery = z.infer<typeof foodPageSchema>
export const ticketPageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    after: z.uuid().optional(),
    state: z.enum(FOOD_TICKET_STATES).optional(),
  })
  .strict()
export type TicketPageQuery = z.infer<typeof ticketPageSchema>
export interface FoodPage<T> {
  items: readonly T[]
  next: string | null
  evaluatedAt: string
}
const target = { id: z.uuid(), expectedRevision: revision }
const ticketLines = z
  .array(z.object({ menuId: z.uuid(), quantity: z.number().int().min(1).max(100) }).strict())
  .min(1)
  .max(20)
  .superRefine((xs, ctx) => {
    if (new Set(xs.map((x) => x.menuId)).size !== xs.length)
      ctx.addIssue({ code: 'custom', message: 'Ticket menu items must be unique' })
  })
export const foodCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('table.create'),
      code: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,19}$/),
      title,
      seats: z.number().int().min(1).max(40),
    })
    .strict(),
  z
    .object({ operation: z.literal('table.set-active'), ...target, active: z.boolean(), reason })
    .strict(),
  z
    .object({
      operation: z.literal('menu.create'),
      sku: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,39}$/),
      title,
      price,
      station: z.enum(['kitchen', 'bar']).default('kitchen'),
      recipe: recipeComponentsSchema,
    })
    .strict(),
  z.object({ operation: z.literal('menu.set-price'), ...target, price, reason }).strict(),
  z
    .object({ operation: z.literal('menu.set-active'), ...target, active: z.boolean(), reason })
    .strict(),
  z
    .object({
      operation: z.literal('menu.replace-recipe'),
      ...target,
      recipe: recipeComponentsSchema,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('ticket.create'),
      customerName: title,
      mode: z.enum(['dine_in', 'takeaway']),
      tableId: z.uuid().optional(),
      guests: z.number().int().min(1).max(40).default(1),
      lines: ticketLines,
    })
    .strict(),
  z
    .object({
      operation: z.literal('ticket.queue'),
      ...target,
      holdSeconds: z.number().int().min(1).max(1800).default(900),
    })
    .strict(),
  z.object({ operation: z.literal('ticket.prepare'), ...target }).strict(),
  z.object({ operation: z.literal('ticket.ready'), ...target }).strict(),
  z.object({ operation: z.literal('ticket.serve'), ...target }).strict(),
  z.object({ operation: z.literal('ticket.close'), ...target }).strict(),
  z.object({ operation: z.literal('ticket.cancel'), ...target, reason }).strict(),
])
export const foodMutationSchema = z
  .object({ requestId: z.uuid(), command: foodCommandSchema })
  .strict()
  .superRefine((input, ctx) => {
    const c = input.command
    if (c.operation === 'ticket.create' && (c.mode === 'dine_in') !== (c.tableId !== undefined))
      ctx.addIssue({ code: 'custom', message: 'Dine-in alone requires a table' })
  })
export type FoodMutation = z.infer<typeof foodMutationSchema>
export type FoodCommand = FoodMutation['command']
export const foodReceiptSchema = z
  .object({
    requestId: z.uuid(),
    operation: z.string().min(1).max(100),
    id: z.uuid(),
    revision,
    replayed: z.boolean(),
  })
  .strict()
export type FoodReceipt = z.infer<typeof foodReceiptSchema>
export type FoodCommandStatus = { state: 'unknown' } | { state: 'committed'; receipt: FoodReceipt }
export class FoodError extends Error {
  constructor(readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable') {
    super(code)
    this.name = 'FoodError'
  }
}
export interface FoodRepository {
  access(tenant: string, principal: FoodPrincipal): Promise<FoodAccess>
  tables(
    tenant: string,
    principal: FoodPrincipal,
    query: FoodPageQuery,
  ): Promise<FoodPage<FoodTable>>
  menu(tenant: string, principal: FoodPrincipal, query: FoodPageQuery): Promise<FoodPage<MenuItem>>
  menuDetail(tenant: string, principal: FoodPrincipal, id: string): Promise<MenuDetail>
  tickets(
    tenant: string,
    principal: FoodPrincipal,
    query: TicketPageQuery,
  ): Promise<FoodPage<FoodTicket>>
  detail(tenant: string, principal: FoodPrincipal, id: string): Promise<TicketDetail>
  mutate(tenant: string, principal: FoodPrincipal, input: FoodMutation): Promise<FoodReceipt>
  commandStatus(tenant: string, principal: FoodPrincipal, id: string): Promise<FoodCommandStatus>
}
