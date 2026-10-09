import { randomUUID } from 'node:crypto'
import { describe, it, expect } from 'vitest'
import { createTabFollowupRecovery } from '../src/recovery.js'

function storage() {
  const values = new Map<string, string>()
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value)
    },
    removeItem: (key: string) => {
      values.delete(key)
    },
  }
}
describe('tab recovery identity', () => {
  it('survives recreation using only a UUID and isolates accounts/workspaces', () => {
    const tab = storage()
    const scope = JSON.stringify(['owner@example.test', 'hotel-demo'])
    const requestId = randomUUID()
    createTabFollowupRecovery(tab, scope).remember(requestId)
    expect(createTabFollowupRecovery(tab, scope).load()).toBe(requestId)
    expect(
      createTabFollowupRecovery(tab, JSON.stringify(['staff@example.test', 'hotel-demo'])).load(),
    ).toBeNull()
    expect(
      createTabFollowupRecovery(tab, JSON.stringify(['owner@example.test', 'studio-demo'])).load(),
    ).toBeNull()
    expect([...tab.values.values()]).toEqual([requestId])
  })
  it('never overwrites or clears an unresolved different request', () => {
    const tab = storage()
    const recovery = createTabFollowupRecovery(tab, 'scope')
    const id = randomUUID()
    recovery.remember(id)
    recovery.remember(id)
    expect(() => recovery.remember(randomUUID())).toThrow(/unresolved/)
    recovery.forget(randomUUID())
    expect(recovery.load()).toBe(id)
    recovery.forget(id)
    expect(recovery.load()).toBeNull()
  })
  it('fails closed on corrupted, unavailable or silently failing browser storage', () => {
    const tab = storage()
    tab.setItem('archava.crm.followup-recovery.v1:scope', '{"summary":"private draft"}')
    expect(() => createTabFollowupRecovery(tab, 'scope').load()).toThrow()
    const unavailable = createTabFollowupRecovery(
      {
        ...tab,
        getItem: () => {
          throw new Error('storage denied')
        },
      },
      'scope',
    )
    expect(() => unavailable.load()).toThrow()
    expect(() => unavailable.remember(randomUUID())).toThrow()
    const silent = createTabFollowupRecovery({ ...storage(), setItem: () => {} }, 'scope')
    expect(() => silent.remember(randomUUID())).toThrow(/verified/)
    expect(() => createTabFollowupRecovery(tab, '').remember(randomUUID())).toThrow()
  })
})
