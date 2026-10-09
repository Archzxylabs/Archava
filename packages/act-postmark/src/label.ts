import { createHmac } from 'node:crypto'

/** Preserve the existing v2 wire label byte-for-byte for send and lookup. */
export function postmarkAttemptLabel(key: Uint8Array, tenantId: string, idempotencyKey: string) {
  return createHmac('sha256', key)
    .update(JSON.stringify(['archava-postmark-attempt-v2', tenantId, idempotencyKey]))
    .digest('base64url')
}
