import { join } from 'node:path'
import { LocalCrmPlanner, type CrmConversationPlanner } from '@archava/crm-assistant'
import { GeminiCrmPlanner, geminiPlannerConfigSchema } from '@archava/crm-assistant/server'
import { geminiPilotCase } from '@archava/crm-assistant/pilot'
import { home, readPrivateJson } from './settings.js'

export async function loadCrmPlanner() {
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
  const gemini = new GeminiCrmPlanner({ apiKey: config.apiKey, projectTier: config.projectTier })
  // Arbitrary names/user text stay local. Provider failures do not silently fall back.
  return {
    select: (utterance: string): CrmConversationPlanner =>
      geminiPilotCase(utterance) ? gemini : local,
    gemini,
  }
}
