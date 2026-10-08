import { randomUUID } from 'node:crypto'
import { expect, it } from 'vitest'
import { vi } from 'vitest'
import {
  CrmPipelineService,
  createPipelineTools,
  opportunityMutationSchema,
  pipelineLabelsSchema,
} from '../src/pipeline.js'
import type { CrmPipelineRepository } from '../src/pipeline.js'

it('requires current contact/revision intent, rejects injected ownership and loss without reason', () => {
  const command = {
    operation: 'opportunity.create',
    contactId: randomUUID(),
    contactRevision: 1,
    title: 'Inquiry',
  }
  for (const bad of [
    { ...command, ownerId: 'other' },
    { ...command, contactRevision: 0 },
    { ...command, tenantId: 'other' },
  ])
    expect(() =>
      opportunityMutationSchema.parse({ requestId: randomUUID(), command: bad }),
    ).toThrow()
  expect(() =>
    opportunityMutationSchema.parse({
      requestId: randomUUID(),
      command: {
        operation: 'opportunity.move',
        id: randomUUID(),
        expectedRevision: 1,
        stage: 'lost',
      },
    }),
  ).toThrow()
  expect(() =>
    pipelineLabelsSchema.parse({
      new: 'New',
      qualified: 'Qualified',
      proposal: 'Proposal',
      won: 'Won',
      lost: 'Lost',
      paid: 'Paid',
    }),
  ).toThrow()
})

it('refuses machine mutations and unknown tools before persistence', async () => {
  const mutate = vi.fn()
  const refuse = () => Promise.reject(new Error('Unexpected persistence'))
  const repository: CrmPipelineRepository = {
    access: refuse,
    list: refuse,
    detail: refuse,
    mutate,
    listOpportunities: refuse,
    opportunityDetail: refuse,
  }
  const service = new CrmPipelineService(repository)
  const principal = { kind: 'service', id: 'bot', onBehalfOf: 'owner' } as const
  await expect(
    service.mutate('hotel-demo', principal, {
      requestId: randomUUID(),
      command: {
        operation: 'opportunity.create',
        contactId: randomUUID(),
        contactRevision: 1,
        title: 'Inquiry',
      },
    }),
  ).rejects.toThrow(/Act/)
  await expect(
    createPipelineTools(service, 'hotel-demo', principal).invoke('crm.opportunity.move', {}),
  ).rejects.toThrow(/Unsupported/)
  expect(mutate).not.toHaveBeenCalled()
})
