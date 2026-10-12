import { z } from 'zod'

export class AssetError extends Error {
  constructor(
    public readonly code:
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'invalid_request'
      | 'unauthorized'
      | 'archived'
      | 'already_checked_out'
      | 'not_in_custody'
      | 'invalid_condition',
    message?: string,
  ) {
    super(message ?? code)
    this.name = 'AssetError'
  }
}

export const assetTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)

export const assetPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: assetTenantSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: assetTenantSchema, onBehalfOf: assetTenantSchema })
    .strict(),
])
export type AssetPrincipal = z.infer<typeof assetPrincipalSchema>

export const ASSET_PERMISSIONS = [
  'assets.read',
  'assets.write',
  'assets.handover',
  'assets.maintenance',
  'audit.read',
] as const
export type AssetPermission = (typeof ASSET_PERMISSIONS)[number]

export const assetMembershipSchema = z
  .object({
    principalId: assetTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(ASSET_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type AssetMembership = z.infer<typeof assetMembershipSchema>

export const assetAccessSchema = z
  .object({
    permissions: z.array(z.enum(ASSET_PERMISSIONS)),
    ownerId: assetTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type AssetAccess = z.infer<typeof assetAccessSchema>

const codeText = z.string().trim().min(1).max(50)
const nameText = z.string().trim().min(1).max(160)
const categoryText = z.string().trim().min(1).max(80)
const optionalText80 = z.string().trim().min(1).max(80).nullable()
const optionalText160 = z.string().trim().min(1).max(160).nullable()
const optionalText500 = z.string().trim().min(1).max(500).nullable()
const revision = z.number().int().positive().max(2147483647)
export const instantSchema = z.iso.datetime({ offset: true })

export const assetOwnershipSchema = z.enum(['business_owned', 'customer_owned'])
export type AssetOwnership = z.infer<typeof assetOwnershipSchema>

export const assetStatusSchema = z.enum([
  'available',
  'in_custody',
  'under_maintenance',
  'retired',
  'returned_to_customer',
])
export type AssetStatus = z.infer<typeof assetStatusSchema>

export const assetConditionSchema = z.enum([
  'excellent',
  'good',
  'fair',
  'damaged',
  'unserviceable',
])
export type AssetCondition = z.infer<typeof assetConditionSchema>

export const assetSchema = z
  .object({
    id: z.uuid(),
    tenantId: assetTenantSchema,
    code: codeText,
    name: nameText,
    category: categoryText,
    serialNumber: optionalText80,
    model: optionalText80,
    ownership: assetOwnershipSchema,
    customerContactId: optionalText80,
    status: assetStatusSchema,
    condition: assetConditionSchema,
    currentCustodianId: optionalText80,
    currentLocation: optionalText160,
    ownerId: assetTenantSchema,
    revision,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type Asset = z.infer<typeof assetSchema>

export const custodyEventTypeSchema = z.enum([
  'dispatch',
  'checkin',
  'transfer',
  'service_intake',
  'customer_return',
])
export type CustodyEventType = z.infer<typeof custodyEventTypeSchema>

export const assetCustodyEventSchema = z
  .object({
    id: z.uuid(),
    tenantId: assetTenantSchema,
    assetId: z.uuid(),
    eventType: custodyEventTypeSchema,
    fromCustodianId: optionalText80,
    toCustodianId: optionalText80,
    conditionBefore: assetConditionSchema,
    conditionAfter: assetConditionSchema,
    location: optionalText160,
    notes: optionalText500,
    recordedBy: assetTenantSchema,
    recordedAt: instantSchema,
  })
  .strict()
export type AssetCustodyEvent = z.infer<typeof assetCustodyEventSchema>

export const maintenanceTypeSchema = z.enum(['preventive', 'corrective', 'inspection'])
export type MaintenanceType = z.infer<typeof maintenanceTypeSchema>

export const assetMaintenanceRecordSchema = z
  .object({
    id: z.uuid(),
    tenantId: assetTenantSchema,
    assetId: z.uuid(),
    serviceType: maintenanceTypeSchema,
    description: nameText,
    partsUsed: optionalText500,
    conditionAfter: assetConditionSchema,
    performedBy: assetTenantSchema,
    recordedAt: instantSchema,
  })
  .strict()
export type AssetMaintenanceRecord = z.infer<typeof assetMaintenanceRecordSchema>

export const assetDetailSchema = z
  .object({
    asset: assetSchema,
    custodyEvents: z.array(assetCustodyEventSchema),
    maintenanceRecords: z.array(assetMaintenanceRecordSchema),
  })
  .strict()
export type AssetDetail = z.infer<typeof assetDetailSchema>

// Mutations data schemas
export const createAssetDataSchema = z
  .object({
    code: codeText,
    name: nameText,
    category: categoryText,
    serialNumber: optionalText80.optional(),
    model: optionalText80.optional(),
    ownership: assetOwnershipSchema,
    customerContactId: optionalText80.optional(),
    condition: assetConditionSchema.default('good'),
    initialLocation: optionalText160.optional(),
  })
  .strict()
  .refine(
    (data) =>
      data.ownership !== 'customer_owned' ||
      (data.customerContactId && data.customerContactId.trim().length > 0),
    { message: 'customer_owned asset requires customerContactId', path: ['customerContactId'] },
  )
export type CreateAssetData = z.infer<typeof createAssetDataSchema>

export const updateAssetDataSchema = z
  .object({
    assetId: z.uuid(),
    expectedRevision: revision,
    name: nameText.optional(),
    category: categoryText.optional(),
    serialNumber: optionalText80.optional(),
    model: optionalText80.optional(),
    currentLocation: optionalText160.optional(),
  })
  .strict()
export type UpdateAssetData = z.infer<typeof updateAssetDataSchema>

export const handoverAssetDataSchema = z
  .object({
    assetId: z.uuid(),
    expectedRevision: revision,
    eventType: custodyEventTypeSchema,
    toCustodianId: optionalText80.optional(),
    conditionAfter: assetConditionSchema,
    location: optionalText160.optional(),
    notes: optionalText500.optional(),
  })
  .strict()
export type HandoverAssetData = z.infer<typeof handoverAssetDataSchema>

export const recordMaintenanceDataSchema = z
  .object({
    assetId: z.uuid(),
    expectedRevision: revision,
    serviceType: maintenanceTypeSchema,
    description: nameText,
    partsUsed: optionalText500.optional(),
    conditionAfter: assetConditionSchema,
    location: optionalText160.optional(),
  })
  .strict()
export type RecordMaintenanceData = z.infer<typeof recordMaintenanceDataSchema>

export const retireAssetDataSchema = z
  .object({
    assetId: z.uuid(),
    expectedRevision: revision,
    reason: nameText,
  })
  .strict()
export type RetireAssetData = z.infer<typeof retireAssetDataSchema>

export const assetMutationSchema = z.discriminatedUnion('type', [
  z
    .object({ requestId: z.uuid(), type: z.literal('create_asset'), data: createAssetDataSchema })
    .strict(),
  z
    .object({ requestId: z.uuid(), type: z.literal('update_asset'), data: updateAssetDataSchema })
    .strict(),
  z
    .object({
      requestId: z.uuid(),
      type: z.literal('handover_asset'),
      data: handoverAssetDataSchema,
    })
    .strict(),
  z
    .object({
      requestId: z.uuid(),
      type: z.literal('record_maintenance'),
      data: recordMaintenanceDataSchema,
    })
    .strict(),
  z
    .object({ requestId: z.uuid(), type: z.literal('retire_asset'), data: retireAssetDataSchema })
    .strict(),
])
export type AssetMutation = z.infer<typeof assetMutationSchema>

export const assetReceiptSchema = z
  .object({
    requestId: z.uuid(),
    type: z.enum([
      'create_asset',
      'update_asset',
      'handover_asset',
      'record_maintenance',
      'retire_asset',
    ]),
    assetId: z.uuid(),
    revision,
    committedAt: instantSchema,
  })
  .strict()
export type AssetReceipt = z.infer<typeof assetReceiptSchema>

export const assetCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('committed'), receipt: assetReceiptSchema }).strict(),
  z.object({ status: z.literal('not_committed') }).strict(),
])
export type AssetCommandStatus = z.infer<typeof assetCommandStatusSchema>

export const assetPageSchema = z
  .object({
    limit: z.coerce.number().int().min(1).max(100).default(20),
    after: z.uuid().optional(),
    category: categoryText.optional(),
    status: assetStatusSchema.optional(),
    ownership: assetOwnershipSchema.optional(),
    search: z.string().trim().max(160).optional(),
  })
  .strict()
export type AssetPageQuery = z.infer<typeof assetPageSchema>

export interface AssetRepository {
  access(tenantId: string, principal: AssetPrincipal): Promise<AssetAccess>
  listAssets(
    tenantId: string,
    principal: AssetPrincipal,
    query: AssetPageQuery,
  ): Promise<{ items: Asset[]; nextCursor: string | null }>
  detail(tenantId: string, principal: AssetPrincipal, id: string): Promise<AssetDetail>
  listCustodyHistory(
    tenantId: string,
    principal: AssetPrincipal,
    assetId: string,
  ): Promise<AssetCustodyEvent[]>
  listMaintenanceHistory(
    tenantId: string,
    principal: AssetPrincipal,
    assetId: string,
  ): Promise<AssetMaintenanceRecord[]>
  mutate(
    tenantId: string,
    principal: AssetPrincipal,
    mutation: AssetMutation,
  ): Promise<AssetReceipt>
  commandStatus(
    tenantId: string,
    principal: AssetPrincipal,
    requestId: string,
  ): Promise<AssetCommandStatus>
}
