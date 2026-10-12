import {
  AttendanceError,
  attendanceTenantSchema,
  attendancePrincipalSchema,
  attendanceMutationSchema,
  sessionPageQuerySchema,
  type AttendanceRepository,
  type AttendancePrincipal,
  type AttendanceMutation,
  type AttendanceReceipt,
  type AttendanceAccess,
  type AttendanceCommandStatus,
  type SessionPageQuery,
  type Session,
  type SessionDetail,
  type RosterEntry,
  type CheckInRecord,
} from './contracts.js'

export class AttendanceService {
  constructor(private readonly repository: AttendanceRepository) {}

  private assertHumanOperator(principal: AttendancePrincipal) {
    if (principal.kind !== 'human') {
      throw new AttendanceError(
        'forbidden',
        'Automated service agents cannot record attendance mutations directly',
      )
    }
  }

  async mutate(
    tenant: string,
    principal: AttendancePrincipal,
    mutation: AttendanceMutation,
  ): Promise<AttendanceReceipt> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    const validMutation = attendanceMutationSchema.parse(mutation)
    this.assertHumanOperator(validPrincipal)
    return this.repository.execute(validTenant, validPrincipal, validMutation)
  }

  async execute(
    tenant: string,
    principal: AttendancePrincipal,
    mutation: AttendanceMutation,
  ): Promise<AttendanceReceipt> {
    return this.mutate(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: AttendancePrincipal,
    commandId: string,
  ): Promise<AttendanceCommandStatus> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(validTenant, validPrincipal, commandId)
  }

  async getAccess(tenant: string, principal: AttendancePrincipal): Promise<AttendanceAccess> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    return this.repository.getAccess(validTenant, validPrincipal)
  }

  async listSessions(
    tenant: string,
    principal: AttendancePrincipal,
    query: Partial<SessionPageQuery> = {},
  ): Promise<readonly Session[]> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    const parsedQuery = sessionPageQuerySchema.parse(query)
    return this.repository.listSessions(validTenant, validPrincipal, parsedQuery)
  }

  async detail(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<SessionDetail> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    const detail = await this.repository.getSessionDetail(validTenant, validPrincipal, sessionId)
    if (!detail) {
      throw new AttendanceError('not_found', `Session ${sessionId} not found`)
    }
    return detail
  }

  async roster(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly RosterEntry[]> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    return this.repository.listRoster(validTenant, validPrincipal, sessionId)
  }

  async checkIns(
    tenant: string,
    principal: AttendancePrincipal,
    sessionId: string,
  ): Promise<readonly CheckInRecord[]> {
    const validTenant = attendanceTenantSchema.parse(tenant)
    const validPrincipal = attendancePrincipalSchema.parse(principal)
    return this.repository.listCheckIns(validTenant, validPrincipal, sessionId)
  }
}
