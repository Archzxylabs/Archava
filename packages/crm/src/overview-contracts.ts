import { z } from 'zod'

const count = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER)
/** The host supplies the time and identity; callers cannot select a tenant scope or grant. */
export const crmOverviewQuerySchema = z.object({}).strict()
export const crmOverviewSchema = z
  .object({
    tenantId: z
      .string()
      .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
      .max(100),
    asOf: z.iso.datetime(),
    scope: z.enum(['all', 'assigned']),
    contacts: count,
    followups: z
      .object({ open: count, overdue: count, upcoming: count, completed: count })
      .strict()
      .refine((value) => value.open === value.overdue + value.upcoming)
      .nullable(),
    pipeline: z
      .object({
        total: count,
        active: count,
        new: count,
        qualified: count,
        proposal: count,
        won: count,
        lost: count,
      })
      .strict()
      .refine(
        (value) =>
          value.active === value.new + value.qualified + value.proposal &&
          value.total === value.active + value.won + value.lost,
      )
      .nullable(),
  })
  .strict()
export type CrmOverview = z.infer<typeof crmOverviewSchema>
