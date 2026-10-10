import { z } from 'zod'
export const PRESENCE_VERSION = '1.0.0' as const
export const PRESENCE_TOOLS = [
  'public.help',
  'crm.contacts.list',
  'crm.contact.summary',
  'crm.followups.list',
  'crm.overview',
  'crm.opportunities.list',
  'crm.followup.prepare',
  'crm.followup.complete.prepare',
] as const
const revision = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER)
export const presenceRequestSchema = z
  .object({
    version: z.literal(PRESENCE_VERSION),
    sessionId: z.uuid(),
    requestId: z.uuid(),
    pageRevision: revision,
    transport: z.enum(['chat', 'synthetic-voice-transcript']),
    utterance: z.string().trim().min(1).max(1000),
    requestedTool: z.enum(PRESENCE_TOOLS).optional(),
    selection: z
      .object({
        contactId: z.uuid().optional(),
        followupId: z.uuid().optional(),
        opportunityId: z.uuid().optional(),
      })
      .strict()
      .optional(),
  })
  .strict()
export const presenceContextSchema = z
  .object({
    version: z.literal(PRESENCE_VERSION),
    sessionId: z.uuid(),
    section: z.string().regex(/^[a-z][a-z0-9-]{0,63}$/),
    revision,
  })
  .strict()
export const presenceSessionSchema = z
  .object({
    version: z.literal(PRESENCE_VERSION),
    sessionId: z.uuid(),
    expiresAt: z.number().int().positive(),
    section: z.string(),
    revision,
    kind: z.enum(['guest', 'owner']),
  })
  .strict()
export const presenceAnswerSchema = z
  .object({
    status: z.enum(['ok', 'unavailable']),
    text: z.string().min(1).max(8000),
    source: z.enum(['public', 'crm', 'none']),
    mode: z.enum(['offline', 'provider-unavailable']),
    data: z.unknown().optional(),
  })
  .strict()
export const presenceResultSchema = presenceAnswerSchema
  .extend({
    version: z.literal(PRESENCE_VERSION),
    sessionId: z.uuid(),
    requestId: z.uuid(),
    pageRevision: revision,
    transport: z.enum(['chat', 'synthetic-voice-transcript']),
    allowedTools: z.array(z.enum(PRESENCE_TOOLS)).max(8),
  })
  .strict()
export const presenceErrorSchema = z
  .object({
    version: z.literal(PRESENCE_VERSION),
    error: z
      .object({
        code: z.enum([
          'invalid',
          'forbidden',
          'expired',
          'revision',
          'replay',
          'busy',
          'timeout',
          'unavailable',
        ]),
        message: z.string().max(160),
      })
      .strict(),
  })
  .strict()
export type PresenceRequest = z.infer<typeof presenceRequestSchema>
export type PresenceResult = z.infer<typeof presenceResultSchema>
export type PresenceAnswer = z.infer<typeof presenceAnswerSchema>
export type PresenceTool = (typeof PRESENCE_TOOLS)[number]
export type PresenceSession = z.infer<typeof presenceSessionSchema>
