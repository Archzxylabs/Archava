import { CrmService, type CrmRepository } from '@archava/crm'
import { PostgresCrmRepository } from '@archava/crm-pg'
export const createService = (repository: CrmRepository) => new CrmService(repository)
export const createRepository = PostgresCrmRepository.create
