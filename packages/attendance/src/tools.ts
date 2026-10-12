import { z } from 'zod'
import { AttendanceError, sessionPageQuerySchema, type AttendancePrincipal } from './contracts.js'
import type { AttendanceService } from './service.js'

export async function executeAttendanceTool(
  service: AttendanceService,
  tenant: string,
  principal: AttendancePrincipal,
  tool: string,
  input: unknown,
): Promise<unknown> {
  try {
    switch (tool) {
      case 'attendance.listSessions': {
        const query = sessionPageQuerySchema.parse(input ?? {})
        return await service.listSessions(tenant, principal, query)
      }
      case 'attendance.sessionDetail': {
        const { sessionId } = z.object({ sessionId: z.string().uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, sessionId)
      }
      case 'attendance.roster': {
        const { sessionId } = z.object({ sessionId: z.string().uuid() }).strict().parse(input)
        return await service.roster(tenant, principal, sessionId)
      }
      case 'attendance.checkIns': {
        const { sessionId } = z.object({ sessionId: z.string().uuid() }).strict().parse(input)
        return await service.checkIns(tenant, principal, sessionId)
      }
      default:
        throw new AttendanceError('not_found', `Unknown attendance tool: ${tool}`)
    }
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new AttendanceError('bad_request', error.message)
    }
    throw error
  }
}
