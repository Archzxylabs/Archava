import rawConfig from '../client.json' with { type: 'json' }
import { parseClientConfig } from '@archava/config/pure'
import { ActionPolicy } from '@archava/acl'
import { createSession } from '@archava/sdk'
import { runTurn } from '@archava/assistant'
import { KnowledgeStore, retrieveContext } from '@archava/knowledge'
import type { BrainProvider } from '@archava/adapters'

export const config = parseClientConfig(rawConfig)
export const allowedActions = ['ui.highlight']
const store = new KnowledgeStore()
for (const source of config.knowledgeSources) store.ingest({ ...source, tenantId: config.tenantId })

/** Deterministic offline fixture brain; public ports only, no provider calls. */
const brain: BrainProvider = {
  providerId: 'offline-fixture',
  model: 'fixture',
  health: { ready: true },
  async reply(turn) {
    return {
      text: turn.grounding.map((item) => item.text).join(' '),
      citations: turn.grounding.map((item) => ({
        sourceId: item.sourceId,
        sourceTitle: item.sourceTitle,
      })),
      // Deliberately adversarial: the shared policy must refuse this request.
      requestedActions: [{ actionId: 'admin.config.update', inputs: {} }],
      deferToStructuredTruth: false,
    }
  },
}

export async function ask(tenantId = config.tenantId) {
  const session = createSession({
    tenantId: config.tenantId,
    config,
    consent: { page_context: true },
  })
  session.observe([
    { type: 'action/set', actions: allowedActions.map((name) => ({ name, enabled: true })) },
  ])
  try {
    return await runTurn({
      tenantId,
      sessionId: 'offline-reference',
      utterance: 'Tell me about your services',
      occurredAt: '2026-10-08T00:00:00.000Z',
      graph: session.graph(),
      capability: config.capability,
      role: 'archava_assistant',
      brain,
      policy: new ActionPolicy(),
      knowledge: {
        async retrieve(request) {
          return retrieveContext(request, store)
        },
      },
    })
  } finally {
    session.destroy()
  }
}
