import { CrmService, type CrmRepository } from '@archava/crm'
import { PostgresCrmRepository } from '@archava/crm-pg'
export const createService = (repository: CrmRepository) => new CrmService(repository)
export const createRepository = PostgresCrmRepository.create

import { CrmPipelineService, type CrmPipelineRepository } from '@archava/crm/pipeline'
export const createPipeline = (repository: CrmPipelineRepository) =>
  new CrmPipelineService(repository)
