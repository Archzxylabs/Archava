import { z } from 'zod'

/** Due-time/UUID keyset, retaining PostgreSQL microseconds across page boundaries. */
export const followupCursorSchema = z.object({ dueAt: z.iso.datetime(), id: z.uuid() }).strict()
export const followupBucketSchema = z.enum(['open', 'overdue', 'upcoming'])
export const followupQueueQuerySchema = z
  .object({
    bucket: followupBucketSchema.default('open'),
    asOf: z.iso.datetime().default(() => new Date().toISOString()),
    after: followupCursorSchema.optional(),
    limit: z.number().int().min(1).max(100).default(20),
    search: z.string().trim().max(120).default(''),
  })
  .strict()
export type FollowupQueueQuery = z.infer<typeof followupQueueQuerySchema>
export const followupQueueItemSchema = z
  .object({
    id: z.uuid(),
    contactId: z.uuid(),
    fullName: z.string().min(1).max(160),
    summary: z.string().min(1).max(2000),
    dueAt: z.iso.datetime(),
  })
  .strict()
export const followupQueueSchema = z
  .object({
    asOf: z.iso.datetime(),
    items: z.array(followupQueueItemSchema).max(100),
    next: followupCursorSchema.nullable(),
  })
  .strict()
export type FollowupQueue = z.infer<typeof followupQueueSchema>
