import { z } from 'zod'

export const conversationInputSchema = z
  .object({
    utterance: z.string().trim().min(1).max(1000),
    contactId: z.uuid().optional(),
    after: z.uuid().optional(),
    draft: z
      .object({
        summary: z.string().trim().min(1).max(2000),
        dueAt: z.iso.datetime(),
      })
      .strict()
      .optional(),
  })
  .strict()
export type CrmConversationInput = z.infer<typeof conversationInputSchema>

/** Provider output is a request. No tenant, identity, permissions, SQL or write tool exists here. */
export const conversationPlanSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('contacts.list'),
      search: z.string().trim().max(120).default(''),
    })
    .strict(),
  z
    .object({
      operation: z.literal('contact.summary'),
      query: z.string().trim().min(1).max(120).optional(),
    })
    .strict(),
  z
    .object({
      operation: z.literal('followup.prepare'),
      query: z.string().trim().min(1).max(120).optional(),
    })
    .strict(),
  z.object({ operation: z.literal('unsupported') }).strict(),
])
export type CrmConversationPlan = z.infer<typeof conversationPlanSchema>
export interface CrmConversationPlanner {
  readonly providerId: string
  readonly model: string
  /** Only the user's bounded text. No private record is sent to a planner. */
  plan(utterance: string): Promise<unknown>
}

const itemSchema = z
  .object({ id: z.uuid(), fullName: z.string().max(160), revision: z.number().int().positive() })
  .strict()
export const conversationDataSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('help') }).strict(),
  z.object({ kind: z.literal('needs_contact') }).strict(),
  z.object({ kind: z.literal('needs_draft') }).strict(),
  z.object({ kind: z.literal('not_found') }).strict(),
  z
    .object({
      kind: z.literal('contacts'),
      items: z.array(itemSchema).max(20),
      next: z.uuid().nullable(),
    })
    .strict(),
  z
    .object({
      kind: z.literal('choose'),
      items: z.array(itemSchema).max(20),
      next: z.uuid().nullable(),
    })
    .strict(),
  z
    .object({
      kind: z.literal('summary'),
      contact: z
        .object({
          id: z.uuid(),
          fullName: z.string().max(160),
          revision: z.number().int().positive(),
          activities: z
            .array(
              z
                .object({
                  kind: z.enum(['note', 'call', 'meeting']),
                  summary: z.string().max(2000),
                  createdAt: z.iso.datetime(),
                })
                .strict(),
            )
            .max(20),
          followups: z
            .array(
              z
                .object({
                  id: z.uuid(),
                  summary: z.string().max(2000),
                  dueAt: z.iso.datetime(),
                  status: z.enum(['open', 'completed']),
                })
                .strict(),
            )
            .max(20),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      kind: z.literal('draft'),
      fullName: z.string().max(160),
      draft: z
        .object({
          contactId: z.uuid(),
          summary: z.string().max(2000),
          dueAt: z.iso.datetime(),
          tenantId: z.string().max(100),
          contactRevision: z.number().int().positive(),
          status: z.literal('draft'),
          requiresApproval: z.literal(true),
        })
        .strict(),
    })
    .strict(),
])
export type CrmConversationData = z.infer<typeof conversationDataSchema>
export const conversationReplySchema = z
  .object({
    text: z.string(),
    source: z.enum(['crm', 'none']),
    data: conversationDataSchema.nullable(),
    planner: z.object({ providerId: z.string(), model: z.string() }).strict(),
  })
  .strict()
export type CrmConversationReply = z.infer<typeof conversationReplySchema>
