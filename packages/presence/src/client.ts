import { presenceRequestSchema, presenceResultSchema, type PresenceRequest } from './contracts.js'
export function createPresenceClient(origin: string, transport: typeof fetch = fetch) {
  const url = new URL(origin)
  if (url.origin !== origin || !['http:', 'https:'].includes(url.protocol))
    throw new Error('Invalid presence origin')
  return {
    async turn(input: PresenceRequest) {
      const request = presenceRequestSchema.parse(input)
      const response = await transport(origin + '/api/presence/turn', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(request),
      })
      if (!response.ok) throw new Error('Presence request unavailable (' + response.status + ')')
      const result = presenceResultSchema.parse(await response.json())
      if (
        result.requestId !== request.requestId ||
        result.sessionId !== request.sessionId ||
        result.pageRevision !== request.pageRevision ||
        result.transport !== request.transport
      )
        throw new Error('Presence response mismatch')
      return result
    },
  }
}
