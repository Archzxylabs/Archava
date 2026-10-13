import { z } from 'zod'
import type { ModuleSelection } from '@archava/config/assembly'
import { CAPABILITY_IDS } from './catalogue.js'

export const kitIdSchema = z.string().regex(/^K(?:0[1-9]|[1-3][0-9]|40)$/)
export const capabilityIdSchema = z.enum(CAPABILITY_IDS)
const text = z.string().trim().min(1).max(1200)
export const kitManifestSchema = z
  .object({
    id: kitIdSchema,
    slug: z.string().regex(/^[a-z]+(?:-[a-z]+)*$/),
    label: text,
    group: z.enum(['general_business', 'specialist']),
    sectorCodes: z
      .array(z.string().regex(/^[A-T]$/))
      .min(1)
      .max(4),
    capabilities: z.array(capabilityIdSchema).min(1).max(20),
    domainRequirement: z.string().regex(/^domain\.[a-z]+(?:-[a-z]+)*$/),
    workflow: text,
    invariants: z.array(text).min(2).max(12),
    extensionContracts: z.array(text).min(1).max(8),
    evidenceDepth: z.enum(['workflow_evidence', 'partly_documented', 'taxonomy_only']),
    sourceIds: z
      .array(z.string().regex(/^S\d\d$/))
      .min(1)
      .max(8),
    implementationRoute: z.enum([
      'evaluate_native_composition_or_existing_system',
      'evaluate_authoritative_adapter_first',
    ]),
    legacyTemplateMatches: z.array(z.string().regex(/^[a-z_]+$/)).max(4),
    status: z.literal('candidate'),
    specification: z
      .object({
        recordOwner: text,
        configurationPolicy: text,
        apiContract: text,
        eventContract: text,
        uiContract: text,
        permissionPolicy: text,
        actionPolicy: text,
        migrationPolicy: text,
        healthPolicy: text,
        failurePolicy: text,
      })
      .strict(),
  })
  .strict()
export type KitManifest = z.infer<typeof kitManifestSchema>

export const kitRequestSchema = z
  .object({
    schemaVersion: z.literal('1.0.0'),
    catalogueVersion: z.literal('1.0.0'),
    profile: z.string().regex(/^\d+\.\d+\.\d+$/),
    selections: z
      .array(
        z
          .object({
            id: kitIdSchema,
            scope: z.enum([
              'operations',
              'crm_inquiry',
              'project_tracking',
              'resource_reservations',
              'accommodation_stays',
              'commerce_orders',
              'foodservice_tickets',
              'support_cases',
              'purchase_receipts',
              'organization_accounts',
              'proposals_scope',
              'delivery_capacity',
              'asset_register',
              'subscriptions_entitlements',
              'attendance_roster',
              'fulfillment_dispatch',
              'billing_invoicing',
            ]),
          })
          .strict(),
      )
      .min(1)
      .max(8)
      .refine(
        (items) => new Set(items.map((item) => item.id)).size === items.length,
        'Duplicate kit selection',
      ),
    requirements: z
      .array(z.string().regex(/^[a-z][a-z0-9_.-]{0,99}$/))
      .max(64)
      .default([]),
    channels: z
      .array(z.enum(['web', 'pos', 'mobile', 'marketplace', 'api']))
      .min(1)
      .max(5)
      .default(['web']),
    assistant: z.boolean().default(false),
    pipeline: z.boolean().default(false),
    nativeActions: z.boolean().default(false),
  })
  .strict()
  .refine(
    (request) => !request.nativeActions || request.assistant,
    'nativeActions requires assistant',
  )
export type KitRequest = z.infer<typeof kitRequestSchema>
export type RequirementDisposition =
  | 'implemented_local_contract'
  | 'needs_configuration'
  | 'needs_implementation'
  | 'needs_domain_review'
  | 'unsupported'
export interface KitRequirement {
  readonly id: string
  readonly requiredBy: readonly string[]
  readonly disposition: RequirementDisposition
  readonly reason: string
  readonly packages: readonly { name: string; version: string }[]
}
export interface KitPlan {
  readonly schemaVersion: '1.0.0'
  readonly catalogueVersion: '1.0.0'
  readonly profile: string
  readonly selections: readonly {
    id: string
    scope:
      | 'operations'
      | 'crm_inquiry'
      | 'project_tracking'
      | 'resource_reservations'
      | 'accommodation_stays'
      | 'commerce_orders'
      | 'foodservice_tickets'
      | 'support_cases'
      | 'purchase_receipts'
      | 'organization_accounts'
      | 'proposals_scope'
      | 'delivery_capacity'
      | 'asset_register'
      | 'subscriptions_entitlements'
      | 'attendance_roster'
      | 'fulfillment_dispatch'
      | 'billing_invoicing'
  }[]
  readonly requirements: readonly KitRequirement[]
  readonly blockers: readonly KitRequirement[]
  readonly readyToAssemble: boolean
  readonly assembly: ModuleSelection | null
  readonly operationalKitsAccepted: readonly string[]
  readonly authority: 'configuration-only'
  readonly pricing: 'manual-review-required-for-new-kit-scope'
}
export class KitPlanningError extends Error {
  readonly code: string
  constructor(code: string) {
    super(`Kit planning refused: ${code}`)
    this.code = code
  }
}
