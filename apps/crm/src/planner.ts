import { join } from 'node:path'
import { LocalCrmPlanner, type CrmConversationPlanner } from '@archava/crm-assistant'
import { GeminiCrmPlanner, geminiPlannerConfigSchema } from '@archava/crm-assistant/server'
import { geminiPilotCase } from '@archava/crm-assistant/pilot'
import { home, readPrivateJson } from './settings.js'
import { runProvider, type ProviderAttemptStore } from '@archava/provider-runtime'
import { CRM_PROVIDER_POLICY } from './provider-policy.js'

export async function loadCrmPlanner(store: ProviderAttemptStore) {
  const local = new LocalCrmPlanner()
  const path = join(home, 'gemini-planner.json')
  let raw: unknown
  try {
    raw = await readPrivateJson(path)
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error
    return { select: (_utterance: string): CrmConversationPlanner => local, gemini: null }
  }
  const config = geminiPlannerConfigSchema.parse(raw)
  if (!config.enabled)
    return { select: (_utterance: string): CrmConversationPlanner => local, gemini: null }
  // Arbitrary names/user text stay local. Provider failures do not silently fall back.
  return {
    select: (utterance: string, attemptId: string): CrmConversationPlanner => {
      if (!geminiPilotCase(utterance)) return local
      // Durable admission owns concurrency/daily pacing; one fresh bounded transport per attempt.
      const gemini = new GeminiCrmPlanner({
        apiKey: config.apiKey,
        projectTier: config.projectTier,
        maxRequests: 1,
        minimumIntervalMs: 0,
      })
      return {
        providerId: gemini.providerId,
        model: gemini.model,
        plan: (text: string) =>
          runProvider({
            store,
            policy: CRM_PROVIDER_POLICY,
            attemptId,
            invoke: () => gemini.plan(text),
            httpStatus: () => gemini.snapshot().lastHttpStatus,
          }),
      }
    },
    gemini: true,
  }
}
