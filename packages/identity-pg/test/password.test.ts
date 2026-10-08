import { describe, it, expect } from 'vitest'
import { hashPassword, verifyPassword, passwordSchema, usernameSchema } from '../src/index.js'

describe('identity credential boundaries', () => {
  it('uses independent salts and preserves spaces/unicode rather than silently truncating', async () => {
    const password = '  correct café passphrase  '
    const first = await hashPassword(password)
    const second = await hashPassword(password)
    expect(first).not.toBe(second)
    expect(await verifyPassword(password, first)).toBe(true)
    expect(await verifyPassword(password.trim(), first)).toBe(false)
  })
  it('requires long bounded passwords and normalizes a valid account name', () => {
    expect(passwordSchema.safeParse('short').success).toBe(false)
    expect(passwordSchema.safeParse('x'.repeat(129)).success).toBe(false)
    expect(usernameSchema.parse('Owner@Example.test')).toBe('owner@example.test')
  })
  it('refuses imported encodings that request unsupported work factors', async () => {
    expect(
      await verifyPassword(
        'valid-looking passphrase',
        'scrypt$1073741824$8$1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
      ),
    ).toBe(false)
    expect(await verifyPassword('valid-looking passphrase', null)).toBe(false)
  })
  it('bounds concurrent password hashing memory even for valid requests', async () => {
    const attempts = await Promise.allSettled(
      Array.from({ length: 3 }, () => hashPassword('a sufficiently long passphrase')),
    )
    expect(attempts.filter((result) => result.status === 'fulfilled')).toHaveLength(2)
    const rejected = attempts.find((result) => result.status === 'rejected')
    expect(rejected?.status === 'rejected' && rejected.reason).toMatchObject({ code: 'busy' })
  })
})
