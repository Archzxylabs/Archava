import { z } from 'zod'

export class OrganizationError extends Error {
  constructor(
    public readonly code:
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'invalid_request'
      | 'unauthorized'
      | 'archived'
      | 'primary_conflict',
    message?: string,
  ) {
    super(message ?? code)
    this.name = 'OrganizationError'
  }
}

export const orgTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)

export const orgPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: orgTenantSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: orgTenantSchema, onBehalfOf: orgTenantSchema })
    .strict(),
])
export type OrganizationPrincipal = z.infer<typeof orgPrincipalSchema>

export const ORGANIZATION_PERMISSIONS = [
  'organizations.read',
  'organizations.write',
  'affiliations.read',
  'affiliations.write',
  'audit.read',
] as const
export type OrganizationPermission = (typeof ORGANIZATION_PERMISSIONS)[number]

export const organizationMembershipSchema = z
  .object({
    principalId: orgTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(ORGANIZATION_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type OrganizationMembership = z.infer<typeof organizationMembershipSchema>

export const organizationAccessSchema = z
  .object({
    permissions: z.array(z.enum(ORGANIZATION_PERMISSIONS)),
    ownerId: orgTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type OrganizationAccess = z.infer<typeof organizationAccessSchema>

const name = z.string().trim().min(1).max(160)
const optionalText160 = z.string().trim().min(1).max(160).nullable()
const optionalText50 = z.string().trim().min(1).max(50).nullable()
const optionalText100 = z.string().trim().min(1).max(100).nullable()
const optionalText200 = z.string().trim().min(1).max(200).nullable()
const optionalText500 = z.string().trim().min(1).max(500).nullable()
const reasonText = z.string().trim().min(1).max(2000)
const roleText = z.string().trim().min(1).max(80)
const revision = z.number().int().positive().max(2147483647)
export const instantSchema = z.iso.datetime({ offset: true })

export const organizationStateSchema = z.enum(['active', 'archived'])
export type OrganizationState = z.infer<typeof organizationStateSchema>

export const organizationSchema = z
  .object({
    id: z.uuid(),
    tenantId: orgTenantSchema,
    name,
    legalName: optionalText160,
    taxId: optionalText50,
    website: optionalText200,
    industry: optionalText100,
    billingAddress: optionalText500,
    state: organizationStateSchema,
    ownerId: orgTenantSchema,
    revision,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type Organization = z.infer<typeof organizationSchema>

export const affiliationStatusSchema = z.enum(['active', 'inactive'])
export type AffiliationStatus = z.infer<typeof affiliationStatusSchema>

export const affiliationSchema = z
  .object({
    id: z.uuid(),
    tenantId: orgTenantSchema,
    organizationId: z.uuid(),
    contactId: z.uuid(),
    role: roleText,
    title: optionalText100,
    department: z.string().trim().min(1).max(80).nullable(),
    isPrimary: z.boolean(),
    status: affiliationStatusSchema,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type Affiliation = z.infer<typeof affiliationSchema>

export const organizationDetailSchema = z
  .object({
    organization: organizationSchema,
    affiliations: z.array(affiliationSchema),
  })
  .strict()
export type OrganizationDetail = z.infer<typeof organizationDetailSchema>

export const createOrganizationDataSchema = z
  .object({
    name,
    legalName: optionalText160.optional(),
    taxId: optionalText50.optional(),
    website: optionalText200.optional(),
    industry: optionalText100.optional(),
    billingAddress: optionalText500.optional(),
  })
  .strict()

export const updateOrganizationDataSchema = z
  .object({
    id: z.uuid(),
    revision,
    name: name.optional(),
    legalName: optionalText160.optional(),
    taxId: optionalText50.optional(),
    website: optionalText200.optional(),
    industry: optionalText100.optional(),
    billingAddress: optionalText500.optional(),
  })
  .strict()

export const archiveOrganizationDataSchema = z
  .object({
    id: z.uuid(),
    revision,
    reason: reasonText,
  })
  .strict()

export const createAffiliationDataSchema = z
  .object({
    organizationId: z.uuid(),
    contactId: z.uuid(),
    role: roleText,
    title: optionalText100.optional(),
    department: z.string().trim().min(1).max(80).nullable().optional(),
    isPrimary: z.boolean().optional(),
  })
  .strict()

export const updateAffiliationDataSchema = z
  .object({
    id: z.uuid(),
    organizationId: z.uuid(),
    role: roleText.optional(),
    title: optionalText100.optional(),
    department: z.string().trim().min(1).max(80).nullable().optional(),
    isPrimary: z.boolean().optional(),
    status: affiliationStatusSchema.optional(),
  })
  .strict()

export const removeAffiliationDataSchema = z
  .object({
    id: z.uuid(),
    organizationId: z.uuid(),
    reason: reasonText,
  })
  .strict()

export const organizationMutationSchema = z.discriminatedUnion('type', [
  z.object({
    requestId: z.uuid(),
    type: z.literal('create_organization'),
    data: createOrganizationDataSchema,
  }).strict(),
  z.object({
    requestId: z.uuid(),
    type: z.literal('update_organization'),
    data: updateOrganizationDataSchema,
  }).strict(),
  z.object({
    requestId: z.uuid(),
    type: z.literal('archive_organization'),
    data: archiveOrganizationDataSchema,
  }).strict(),
  z.object({
    requestId: z.uuid(),
    type: z.literal('create_affiliation'),
    data: createAffiliationDataSchema,
  }).strict(),
  z.object({
    requestId: z.uuid(),
    type: z.literal('update_affiliation'),
    data: updateAffiliationDataSchema,
  }).strict(),
  z.object({
    requestId: z.uuid(),
    type: z.literal('remove_affiliation'),
    data: removeAffiliationDataSchema,
  }).strict(),
])
export type OrganizationMutation = z.infer<typeof organizationMutationSchema>

export const organizationReceiptSchema = z
  .object({
    requestId: z.uuid(),
    type: z.enum([
      'create_organization',
      'update_organization',
      'archive_organization',
      'create_affiliation',
      'update_affiliation',
      'remove_affiliation',
    ]),
    organizationId: z.uuid(),
    affiliationId: z.uuid().nullable(),
    revision: revision,
    committedAt: instantSchema,
  })
  .strict()
export type OrganizationReceipt = z.infer<typeof organizationReceiptSchema>

export const organizationCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('committed'), receipt: organizationReceiptSchema }).strict(),
  z.object({ status: z.literal('not_committed') }).strict(),
])
export type OrganizationCommandStatus = z.infer<typeof organizationCommandStatusSchema>

export const organizationPageSchema = z
  .object({
    limit: z.coerce.number().int().min(1).max(100).default(20),
    after: z.uuid().optional(),
    state: organizationStateSchema.optional(),
    search: z.string().trim().max(160).optional(),
  })
  .strict()
export type OrganizationPageQuery = z.infer<typeof organizationPageSchema>

export const affiliationPageSchema = z
  .object({
    limit: z.coerce.number().int().min(1).max(100).default(50),
    after: z.uuid().optional(),
    status: affiliationStatusSchema.optional(),
    role: z.string().trim().max(80).optional(),
  })
  .strict()
export type AffiliationPageQuery = z.infer<typeof affiliationPageSchema>

export interface OrganizationRepository {
  access(tenantId: string, principal: OrganizationPrincipal): Promise<OrganizationAccess>
  listOrganizations(
    tenantId: string,
    principal: OrganizationPrincipal,
    query: OrganizationPageQuery,
  ): Promise<{ items: Organization[]; nextCursor: string | null }>
  detail(
    tenantId: string,
    principal: OrganizationPrincipal,
    id: string,
  ): Promise<OrganizationDetail>
  listAffiliations(
    tenantId: string,
    principal: OrganizationPrincipal,
    organizationId: string,
    query: AffiliationPageQuery,
  ): Promise<{ items: Affiliation[]; nextCursor: string | null }>
  mutate(
    tenantId: string,
    principal: OrganizationPrincipal,
    mutation: OrganizationMutation,
  ): Promise<OrganizationReceipt>
  commandStatus(
    tenantId: string,
    principal: OrganizationPrincipal,
    requestId: string,
  ): Promise<OrganizationCommandStatus>
}
