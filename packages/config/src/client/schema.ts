import { z } from 'zod'

/**
 * Client Configuration Model.
 *
 * One instance describes exactly one client deployment: who they are, which
 * template they start from, and every axis override. Everything is explicit —
 * an unknown or invalid value must fail validation loudly rather than fall
 * back to a default, because every field here feeds pricing, capabilities,
 * and permission decisions.
 */

const nonEmptyString = z.string().min(1)
const slug = z.string().regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, 'must be a lowercase kebab-case slug')

export const presenceSchema = z.enum(['chat', 'voice', 'human'])
export const capabilitySchema = z.enum(['assist', 'act', 'transact', 'enterprise'])
export const environmentSchema = z.enum([
  'existing_site',
  'landing',
  'business',
  'commerce_booking',
  'custom_web_app',
  'enterprise_system',
])
export const regionSchema = z.enum(['GLOBAL', 'ID'])
export const integrationComplexitySchema = z.enum(['standard', 'custom', 'advanced', 'enterprise'])
export const supportTierSchema = z.enum(['standard', 'priority', 'dedicated_sla'])
export const designAddonSchema = z.enum([
  'custom_visual_direction',
  'advanced_motion_3d',
  'premium_avatar_provider',
])

export const integrationSchema = z.object({
  name: nonEmptyString,
  complexity: integrationComplexitySchema,
  /** Requested in this scope. V1 enabled is not connection or health evidence. */
  enabled: z.boolean().default(true),
  /** Free-text note on credentials/account ownership. Never a secret value. */
  note: z.string().optional(),
})

export const moduleToggleSchema = z.object({
  name: nonEmptyString,
  enabled: z.boolean(),
})

export const brandingSchema = z.object({
  businessName: nonEmptyString,
  tagline: z.string().optional(),
  /** Palette + typography only. No arbitrary CSS is accepted. */
  theme: z.object({
    accent: z.string().regex(/^#[0-9a-fA-F]{6}$/, 'must be a #rrggbb hex colour'),
    surface: z.string().regex(/^#[0-9a-fA-F]{6}$/),
    ink: z.string().regex(/^#[0-9a-fA-F]{6}$/),
    radius: z.enum(['soft', 'rounded', 'sharp']),
    fontFamily: z.string().min(1),
  }),
  /** Absolute or relative URL to a logo asset. Not inlined data. */
  logo: z.string().optional(),
})

export const languageSchema = z.object({
  code: nonEmptyString,
  label: nonEmptyString,
  /** Fallback order when a message is not localised in this language. */
  fallback: z.string().optional(),
})

export const knowledgeSourceSchema = z.object({
  id: slug,
  kind: z.enum(['faq', 'documentation', 'policy', 'product', 'service', 'marketing']),
  title: nonEmptyString,
  /** Structured content is authored locally, never scraped from arbitrary URLs. */
  content: z.string().min(1),
  /** Optional source URL for attribution only — never fetched by the runtime. */
  sourceUrl: z.string().optional(),
  updatedAt: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, 'must be an ISO YYYY-MM-DD date'),
})

export const entitySchema = z.object({
  id: slug,
  kind: z.enum(['product', 'service', 'resource', 'offer', 'location']),
  name: nonEmptyString,
  summary: nonEmptyString,
  /** Structured, live-answerable attributes. These are business truth, not
   *  retrieved marketing copy. */
  attributes: z.record(z.string(), z.union([z.string(), z.number(), z.boolean()])).default({}),
  visible: z.boolean().default(true),
})

export const allowedOriginSchema = z
  .string()
  .refine(
    (value) => value === '*' || /^https?:\/\//.test(value),
    'must be an absolute origin (or "*")',
  )

/* ------------------------------------------------- Decision Intelligence -- */

/**
 * The stable Decision Layer task ids.
 *
 * The Decision Layer is a bounded, advisory judgement seam beside the
 * deterministic turn: it never grants permission, never confirms an action and
 * never proves a fact. The ids are the vocabulary the tenant configuration, the
 * task catalogue and the offline evaluation corpus all spell the same way. They
 * are declared here rather than imported so this package stays usable on its
 * own while the catalogue package is built beside it.
 */
export const DECISION_TASK_IDS = [
  'intent_classification',
  'knowledge_routing',
  'clarification',
  'handoff_recommendation',
  'evidence_sufficiency',
] as const

/** How far a tenant lets the Decision Layer go. Default is `off`. */
export const DECISION_MODES = ['off', 'shadow', 'assist'] as const

/** One task's policy: whether it runs at all, and what confidence it must clear. */
export const decisionTaskPolicySchema = z.object({
  enabled: z.boolean(),
  /** Inclusive lower bound in [0, 1] on the provider's reported confidence. */
  minConfidence: z.number().min(0).max(1),
})

/** Per-task policies. An unknown task id is rejected, never ignored. */
export const decisionTasksSchema = z
  .object({
    intent_classification: decisionTaskPolicySchema.optional(),
    knowledge_routing: decisionTaskPolicySchema.optional(),
    clarification: decisionTaskPolicySchema.optional(),
    handoff_recommendation: decisionTaskPolicySchema.optional(),
    evidence_sufficiency: decisionTaskPolicySchema.optional(),
  })
  .strict()

/**
 * Optional Decision Intelligence configuration. Absent by default, so every
 * existing tenant config keeps its Foundation V1.1 behaviour exactly: no
 * provider is asked, and nothing may disagree with the deterministic turn. An
 * explicit `mode: 'off'` is that same promise written down.
 */
export const decisionConfigSchema = z
  .object({
    mode: z.enum(DECISION_MODES),
    /**
     * Provider id, resolved by the host against the providers it registered.
     * Deliberately not an enum: provider identity belongs to the adapter that
     * implements it, and an id this schema has never heard of must resolve to
     * the deterministic baseline rather than fail the whole config.
     */
    provider: slug.optional(),
    /** Per-task policy. A task left out is a task that is not consulted. */
    tasks: decisionTasksSchema.default({}),
  })
  .strict()
  .refine(
    (decision) => decision.mode === 'off' || decision.provider !== undefined,
    'a provider is required unless mode is "off"',
  )

export const clientConfigSchema = z
  .object({
    schema_version: z.string().regex(/^\d+\.\d+\.\d+$/),
    tenantId: slug,
    environment: environmentSchema,
    presence: presenceSchema,
    capability: capabilitySchema,
    region: regionSchema,
    template: nonEmptyString,
    branding: brandingSchema,
    languages: z.array(languageSchema).min(1),
    integrations: z.array(integrationSchema).default([]),
    modules: z.array(moduleToggleSchema).default([]),
    entities: z.array(entitySchema).default([]),
    knowledgeSources: z.array(knowledgeSourceSchema).default([]),
    support: supportTierSchema.default('standard'),
    designAddons: z.array(designAddonSchema).default([]),
    /** Origins allowed to embed the Archava SDK on this tenant. */
    allowedOrigins: z.array(allowedOriginSchema).default([]),
    /** Locales the reference demo exposes. */
    primaryLanguage: nonEmptyString,
    updatedAt: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
    /** Marks a config as reference/demo data, never a real business. */
    isReferenceImplementation: z.boolean().default(false),
    /**
     * Optional Decision Intelligence policy. Absent means Foundation V1.1:
     * the deterministic turn alone decides, and no provider is consulted.
     */
    decision: decisionConfigSchema.optional(),
  })
  .strict()

export type ClientConfig = z.infer<typeof clientConfigSchema>
export type Integration = z.infer<typeof integrationSchema>
export type ModuleToggle = z.infer<typeof moduleToggleSchema>
export type Entity = z.infer<typeof entitySchema>
export type KnowledgeSource = z.infer<typeof knowledgeSourceSchema>
export type Language = z.infer<typeof languageSchema>
export type Branding = z.infer<typeof brandingSchema>
export type Presence = z.infer<typeof presenceSchema>
export type Capability = z.infer<typeof capabilitySchema>
export type Environment = z.infer<typeof environmentSchema>
export type Region = z.infer<typeof regionSchema>
export type DecisionMode = z.infer<typeof DECISION_MODES>
export type DecisionTaskId = (typeof DECISION_TASK_IDS)[number]
export type DecisionTaskPolicy = z.infer<typeof decisionTaskPolicySchema>
export type DecisionTasks = z.infer<typeof decisionTasksSchema>
export type DecisionConfig = z.infer<typeof decisionConfigSchema>

/** Validate a client config, returning either the parsed value or clear issues. */
export function parseClientConfig(raw: unknown): ClientConfig {
  return clientConfigSchema.parse(raw)
}

export function safeParseClientConfig(
  raw: unknown,
): { success: true; data: ClientConfig } | { success: false; issues: string[] } {
  const result = clientConfigSchema.safeParse(raw)
  if (result.success) return { success: true, data: result.data }
  return {
    success: false,
    issues: result.error.issues.map(
      (issue) => `${issue.path.join('.') || '(root)'}: ${issue.message}`,
    ),
  }
}
