import { z } from 'zod'
import { idSchema, commandStatusSchema } from '@archava/crm/contracts'

export const followupActionInputsSchema = z
  .object({
    contactId: idSchema,
    contactRevision: z.number().int().positive(),
    summary: z.string().trim().min(1).max(2000),
    dueAt: z.iso.datetime(),
  })
  .strict()
export type FollowupActionInputs = z.infer<typeof followupActionInputsSchema>
export const nativeReviewSchema = z
  .object({
    intentId: idSchema,
    challenge: z
      .object({
        challengeId: z.string().regex(/^[a-f0-9]{32}$/),
        token: z.string().regex(/^[a-f0-9]{64}$/),
        expiresAt: z.number().int().positive(),
      })
      .strict(),
  })
  .strict()
export const nativePresentSchema = z
  .object({
    inputs: followupActionInputsSchema,
    confirmation: nativeReviewSchema.shape.challenge.pick({ challengeId: true, token: true }),
  })
  .strict()
export const nativeStatusSchema = commandStatusSchema
export const nativeExecutionSchema = z
  .object({
    actionId: z.literal('crm.followup.create'),
    execution: z.enum(['not_attempted', 'succeeded', 'failed']),
    text: z.string().max(2000),
    errorCode: z.enum(['provider_outcome_unknown', 'action_not_completed']).optional(),
  })
  .strict()
export const actionControlSchema = z.object({ epoch: idSchema, held: z.boolean() }).strict()
export type ActionControl = z.infer<typeof actionControlSchema>
