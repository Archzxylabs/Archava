import { describe, it, expect, vi } from 'vitest'
import type { Pool } from 'pg'
import type { CrmService } from '@archava/crm'
import type { PostgresIdentity } from '@archava/identity-pg'
import { cookieToken, sessionCookie, createHostHandler } from '../src/host.js'
import type { RuntimeSettings } from '../src/settings.js'

const origin = 'http://127.0.0.1:4175'
const token = 'a'.repeat(43)
const settings: RuntimeSettings = {
  version: 1,
  database: 'test',
  databasePort: 23000,
  appPort: 4175,
  fingerprintKey: 'x'.repeat(43),
  identityKey: 'y'.repeat(43),
  tenants: [],
}
function fixture(session: unknown = null) {
  const authenticate = vi.fn(() => Promise.resolve(session))
  const login = vi.fn()
  const api = createHostHandler(
    {} as CrmService,
    { authenticate, login, validCsrf: () => false } as unknown as PostgresIdentity,
    {} as Pool,
    settings,
    origin,
  )
  return { api, authenticate, login }
}
describe('authenticated CRM host boundary', () => {
  it('refuses ambiguous cookies and carries secure cookie properties', () => {
    expect(
      cookieToken(
        `archava_local_session=${token}; archava_local_session=${token}`,
        'archava_local_session',
      ),
    ).toBeNull()
    expect(
      cookieToken(`unrelated=x; archava_local_session=${token}`, 'archava_local_session'),
    ).toBe(token)
    expect(sessionCookie('__Host-archava_session', token, true)).toContain('; Secure')
    expect(sessionCookie('__Host-archava_session', token, true)).toContain(
      'HttpOnly; SameSite=Strict; Path=/',
    )
    expect(sessionCookie('__Host-archava_session', token, true, true)).toContain('Max-Age=0')
  })
  it('refuses login CSRF before invoking any credential work', async () => {
    const { api, login } = fixture()
    const response = await api(
      new Request(origin + '/api/login', {
        method: 'POST',
        headers: { origin: 'https://attacker.test' },
        body: '{}',
      }),
      'local',
    )
    expect(response.status).toBe(403)
    expect(login).not.toHaveBeenCalled()
  })
  it('requires cookie authentication rather than caller-supplied bearer identity', async () => {
    const { api, authenticate } = fixture()
    const response = await api(
      new Request(origin + '/api/crm/hotel-demo/contacts', {
        headers: { authorization: 'Bearer ' + token },
      }),
      'local',
    )
    expect(response.status).toBe(401)
    expect(authenticate).toHaveBeenCalledWith(null)
  })
  it('blocks an authenticated mutation without its session-bound CSRF token', async () => {
    const { api } = fixture({
      principalId: 'human-valid',
      username: 'owner@example.test',
      csrf: 'b'.repeat(64),
    })
    const response = await api(
      new Request(origin + '/api/crm/hotel-demo/commands', {
        method: 'POST',
        headers: { origin, cookie: 'archava_local_session=' + token },
        body: '{}',
      }),
      'local',
    )
    expect(response.status).toBe(403)
  })
  it('rejects role/actor injection in the login contract before password verification', async () => {
    const { api, login } = fixture()
    const response = await api(
      new Request(origin + '/api/login', {
        method: 'POST',
        headers: { origin, 'content-type': 'application/json' },
        body: JSON.stringify({
          username: 'owner@example.test',
          password: 'long-enough passphrase',
          role: 'owner',
        }),
      }),
      'local',
    )
    expect(response.status).toBe(400)
    expect(login).not.toHaveBeenCalled()
  })
})
