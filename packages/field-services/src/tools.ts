import { z } from 'zod'
import { FieldServicesError } from './contracts.js'
import type { FieldServicesService } from './service.js'

export const FIELD_TOOLS = [
  'field.listJobs',
  'field.jobDetail',
  'field.createJob',
  'field.assignJob',
  'field.recordVisit',
  'field.appendWorkLine',
  'field.recordSignoff',
  'field.approveJob',
  'field.cancelJob',
  'field.audit',
] as const
export type FieldToolName = (typeof FIELD_TOOLS)[number]

const jobIdSchema = z.object({ jobId: z.string().trim().min(1).max(100) }).strict()

/**
 * Tool surface for an assistant. `field.dispatch` is a machine-held grant, so
 * the write tools are grouped separately from the reads: a principal that can
 * see the queue is not automatically one that can move work.
 */
export async function runFieldTool(
  service: FieldServicesService,
  tenant: string,
  principal: Parameters<FieldServicesService['listJobs']>[1],
  tool: string,
  input: unknown,
): Promise<unknown> {
  try {
    switch (tool) {
      case 'field.listJobs':
        return await service.listJobs(tenant, principal, input ?? {})
      case 'field.jobDetail': {
        const { jobId } = jobIdSchema.parse(input)
        return await service.detail(tenant, principal, jobId)
      }
      case 'field.createJob': {
        const { commandId, job } = z
          .object({
            commandId: z.string().trim().min(1).max(100),
            job: z
              .object({
                jobId: z.string().trim().min(1).max(100),
                customerRef: z.string().trim().min(1).max(100),
                siteRef: z.string().trim().min(1).max(100),
                summary: z.string().trim().min(1).max(500),
                priority: z.enum(['low', 'normal', 'high', 'urgent']).default('normal'),
                // Required: the scheduler-owned appointment this dispatch must not lose.
                appointmentRef: z.string().trim().min(1).max(100),
                assetRef: z.string().trim().min(1).max(100).nullable().default(null),
              })
              .strict(),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'create_job',
          commandId,
          job,
        })
      }
      case 'field.assignJob': {
        const { commandId, jobId, assignedPrincipal } = jobIdSchema
          .extend({
            commandId: z.string().trim().min(1).max(100),
            assignedPrincipal: z.string().trim().min(1).max(100),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'assign_job',
          commandId,
          jobId,
          assignedPrincipal,
        })
      }
      case 'field.recordVisit': {
        const { commandId, jobId, visit } = jobIdSchema
          .extend({
            commandId: z.string().trim().min(1).max(100),
            visit: z
              .object({
                visitId: z.string().trim().min(1).max(100),
                outcome: z.enum(['completed', 'partial', 'no_access']),
                note: z.string().trim().min(1).max(2000),
                technicianRef: z.string().trim().min(1).max(100),
              })
              .strict(),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'record_visit',
          commandId,
          jobId,
          visit,
        })
      }
      case 'field.appendWorkLine': {
        const { commandId, jobId, line } = jobIdSchema
          .extend({
            commandId: z.string().trim().min(1).max(100),
            line: z
              .object({
                lineId: z.string().trim().min(1).max(100),
                kind: z.enum(['labour', 'part']),
                description: z.string().trim().min(1).max(500),
                quantity: z.number().positive(),
                unit: z.enum(['hours', 'pieces']),
                partRef: z.string().trim().min(1).max(100).nullable(),
              })
              .strict(),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'append_work_line',
          commandId,
          jobId,
          line,
        })
      }
      case 'field.recordSignoff': {
        const { commandId, jobId, signoff } = jobIdSchema
          .extend({
            commandId: z.string().trim().min(1).max(100),
            signoff: z
              .object({
                signoffId: z.string().trim().min(1).max(100),
                signerRef: z.string().trim().min(1).max(100),
                accepted: z.boolean(),
                comment: z.string().trim().max(2000).nullable().default(null),
              })
              .strict(),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'record_signoff',
          commandId,
          jobId,
          signoff,
        })
      }
      case 'field.approveJob': {
        const { commandId, jobId } = jobIdSchema
          .extend({ commandId: z.string().trim().min(1).max(100) })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'approve_job',
          commandId,
          jobId,
        })
      }
      case 'field.cancelJob': {
        const { commandId, jobId, reason } = jobIdSchema
          .extend({
            commandId: z.string().trim().min(1).max(100),
            reason: z.string().trim().min(1).max(500),
          })
          .strict()
          .parse(input)
        return await service.mutate(tenant, principal, {
          action: 'cancel_job',
          commandId,
          jobId,
          reason,
        })
      }
      case 'field.audit': {
        const { jobId } = jobIdSchema.parse(input)
        return await service.audit(tenant, principal, jobId)
      }
      default:
        throw new FieldServicesError('not_found', `Unknown field tool: ${tool}`)
    }
  } catch (error) {
    if (error instanceof z.ZodError)
      throw new FieldServicesError('validation_failed', error.message)
    throw error
  }
}
