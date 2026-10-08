import { GEMINI_CRM_MODEL } from '@archava/crm-assistant/server'
import { providerPolicySchema } from '@archava/provider-runtime'

/** One shared host budget across both business contexts; never an LLM input. */
export const CRM_PROVIDER_POLICY = providerPolicySchema.parse({
  bucketId: 'crm-gemini-pilot',
  providerId: 'google-gemini-crm',
  model: GEMINI_CRM_MODEL,
  dailyLimit: 30,
  minimumIntervalMs: 15000,
  leaseMs: 60000,
})
