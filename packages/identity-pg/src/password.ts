import { randomBytes, scrypt, timingSafeEqual } from 'node:crypto'
import { z } from 'zod'

export const usernameSchema = z
  .email()
  .max(254)
  .transform((value) => value.toLowerCase())
export const passwordSchema = z.string().min(15).max(128)
export class IdentityError extends Error {
  constructor(readonly code: 'invalid_credentials' | 'rate_limited' | 'busy' | 'unauthenticated') {
    super(code)
    this.name = 'IdentityError'
  }
}
// OWASP's 32 MiB scrypt profile; bounded concurrency keeps memory predictable.
const options = { N: 32768, r: 8, p: 3, maxmem: 48 * 1024 * 1024 }
let active = 0
async function derive(password: string, salt: Buffer): Promise<Buffer> {
  if (active >= 2) throw new IdentityError('busy')
  active++
  try {
    return await new Promise<Buffer>((done, fail) => {
      scrypt(password, salt, 32, options, (error, key) => (error ? fail(error) : done(key)))
    })
  } finally {
    active--
  }
}
export async function hashPassword(raw: unknown): Promise<string> {
  const password = passwordSchema.parse(raw)
  const salt = randomBytes(16)
  const key = await derive(password, salt)
  return `scrypt$32768$8$3$${salt.toString('base64url')}$${key.toString('base64url')}`
}
/** Fixed parameters only: malformed/untrusted encodings cannot request costly work. */
export async function verifyPassword(password: string, encoded: string | null): Promise<boolean> {
  const match = /^scrypt\$32768\$8\$3\$([A-Za-z0-9_-]{22})\$([A-Za-z0-9_-]{43})$/.exec(
    encoded ?? '',
  )
  const salt = match ? Buffer.from(match[1] ?? '', 'base64url') : Buffer.alloc(16)
  const expected = match ? Buffer.from(match[2] ?? '', 'base64url') : Buffer.alloc(32)
  const actual = await derive(password, salt)
  return match !== null && expected.length === 32 && timingSafeEqual(expected, actual)
}
