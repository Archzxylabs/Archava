import { z } from 'zod'
import { opportunityCommandSchema } from './pipeline-contracts.js'

export const tenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const idSchema = z.uuid()
const name = z.string().trim().min(1).max(160)
const summary = z.string().trim().min(1).max(2000)
const revision = z.number().int().positive()
const instant = z.iso.datetime()
const optionalText = z.string().trim().max(160).nullable()

export const principalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: tenantSchema }).strict(),
  z.object({ kind: z.literal('service'), id: tenantSchema, onBehalfOf: tenantSchema }).strict(),
])
/** Trusted host identity, never constructed from a browser role/actor field. */
export type CrmPrincipal = z.infer<typeof principalSchema>
export const CRM_PERMISSIONS = [
  'contacts.read',
  'contacts.write',
  'activities.read',
  'activities.write',
  'followups.read',
  'followups.write',
  'audit.read',
  'pipeline.read',
  'pipeline.write',
] as const
export type CrmPermission = (typeof CRM_PERMISSIONS)[number]
export const membershipSchema = z
  .object({
    principalId: tenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(CRM_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type CrmMembership = z.infer<typeof membershipSchema>
export interface CrmAccess {
  readonly permissions: readonly CrmPermission[]
  readonly ownerId: string
  readonly scope: 'all' | 'assigned'
}

export const contactSchema = z
  .object({
    id: idSchema,
    tenantId: tenantSchema,
    fullName: name,
    email: z.email().max(254).nullable(),
    organization: optionalText,
    ownerId: tenantSchema,
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type Contact = z.infer<typeof contactSchema>
export const activitySchema = z
  .object({
    id: idSchema,
    tenantId: tenantSchema,
    contactId: idSchema,
    kind: z.enum(['note', 'call', 'meeting']),
    summary,
    actorId: tenantSchema,
    createdAt: instant,
  })
  .strict()
export type Activity = z.infer<typeof activitySchema>
export const followupSchema = z
  .object({
    id: idSchema,
    tenantId: tenantSchema,
    contactId: idSchema,
    summary,
    dueAt: instant,
    status: z.enum(['open', 'completed']),
    revision,
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type FollowUp = z.infer<typeof followupSchema>

const contactFields = z
  .object({
    fullName: name,
    email: z.email().max(254).nullable().default(null),
    organization: optionalText.default(null),
  })
  .strict()
const contactCommandSchema = z.discriminatedUnion('operation', [
  z.object({ operation: z.literal('contact.create'), data: contactFields }).strict(),
  z
    .object({
      operation: z.literal('contact.update'),
      id: idSchema,
      expectedRevision: revision,
      data: z
        .object({
          fullName: name.optional(),
          email: z.email().max(254).nullable().optional(),
          organization: optionalText.optional(),
        })
        .strict()
        .refine((value) => Object.keys(value).length > 0, 'provide at least one changed field'),
    })
    .strict(),
  z
    .object({
      operation: z.literal('activity.add'),
      contactId: idSchema,
      contactRevision: revision,
      kind: z.enum(['note', 'call', 'meeting']),
      summary,
    })
    .strict(),
  z
    .object({
      operation: z.literal('followup.create'),
      contactId: idSchema,
      contactRevision: revision,
      summary,
      dueAt: instant,
    })
    .strict(),
  z
    .object({ operation: z.literal('followup.complete'), id: idSchema, expectedRevision: revision })
    .strict(),
])
export const commandSchema = z.union([contactCommandSchema, opportunityCommandSchema])
export type CrmCommand = z.infer<typeof commandSchema>
export const mutationSchema = z.object({ requestId: idSchema, command: commandSchema }).strict()
export type CrmMutation = z.infer<typeof mutationSchema>
export const mutationReceiptSchema = z
  .object({
    kind: z.enum(['contact', 'activity', 'followup', 'opportunity']),
    id: idSchema,
    revision,
  })
  .strict()
export type MutationReceipt = z.infer<typeof mutationReceiptSchema>
/** Absence is not proof of failure: the original transaction may still be running. */
export const commandStatusSchema = z.discriminatedUnion('status', [
  z
    .object({ status: z.literal('committed'), requestId: idSchema, receipt: mutationReceiptSchema })
    .strict(),
  z.object({ status: z.literal('unconfirmed'), requestId: idSchema }).strict(),
])
export type CrmCommandStatus = z.infer<typeof commandStatusSchema>
export const pageSchema = z
  .object({
    after: idSchema.optional(),
    limit: z.number().int().min(1).max(100).default(20),
    search: z.string().trim().max(120).default(''),
  })
  .strict()
export type CrmPage = z.infer<typeof pageSchema>
export interface ContactPage {
  readonly items: readonly Contact[]
  readonly next: string | null
}
export interface ContactDetail {
  readonly contact: Contact
  readonly activities: readonly Activity[]
  readonly followups: readonly FollowUp[]
}
export const followupDraftSchema = z
  .object({ contactId: idSchema, summary, dueAt: instant })
  .strict()
export interface CrmRepository {
  access(tenant: string, principal: CrmPrincipal): Promise<CrmAccess>
  list(tenant: string, principal: CrmPrincipal, page: CrmPage): Promise<ContactPage>
  detail(tenant: string, principal: CrmPrincipal, id: string): Promise<ContactDetail>
  mutate(tenant: string, principal: CrmPrincipal, mutation: CrmMutation): Promise<MutationReceipt>
  /** Server-only verified approval path. Ordinary machine writes remain forbidden. */
  mutateApproved?(
    tenant: string,
    principal: CrmPrincipal,
    mutation: CrmMutation,
    approval: unknown,
  ): Promise<MutationReceipt>
  approvedCommandStatus?(
    tenant: string,
    principal: CrmPrincipal,
    requestId: string,
  ): Promise<CrmCommandStatus>
  /** Optional read capability for existing adapters; never executes or retries a command. */
  commandStatus?(
    tenant: string,
    principal: CrmPrincipal,
    requestId: string,
  ): Promise<CrmCommandStatus>
}

export class CrmError extends Error {
  constructor(
    readonly code: 'unauthenticated' | 'forbidden' | 'not_found' | 'conflict' | 'unavailable',
    message: string,
  ) {
    super(message)
    this.name = 'CrmError'
  }
}

/** Config/installation metadata; it grants no authority and supplies no price. */
export const CRM_CONTACTS_MANIFEST = {
  id: 'crm_contacts',
  version: '0.4.0',
  status: 'implemented' as const,
  requires: [],
  permissions: CRM_PERMISSIONS.filter((permission) => !permission.startsWith('pipeline.')),
  persistence: { adapter: '@archava/crm-pg', migration: '001_crm_contacts' },
  apiVersion: '1.0.0',
  tools: ['crm.contacts.list', 'crm.contact.summary', 'crm.followup.prepare'],
}
