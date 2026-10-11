import { describe, it, expect } from 'vitest'
import { Client } from 'pg'
import { createRuntimePool, closeRuntimePool } from '../src/postgres-pool.js'
describe('owned database shutdown', () => {
  it('waits for actual removal even when pg-pool end resolves with zero listed clients', async () => {
    const pool = createRuntimePool({}),
      first = new Client(),
      second = new Client()
    // The public lifecycle can retain closing clients after totalCount reaches zero.
    pool.emit('connect', first)
    pool.emit('connect', second)
    expect(pool.totalCount).toBe(0)
    let stopped = false
    const closing = closeRuntimePool(pool).then(() => {
      stopped = true
    })
    await Promise.resolve()
    expect(stopped).toBe(false)
    pool.emit('remove', first)
    await Promise.resolve()
    expect(stopped).toBe(false)
    pool.emit('remove', second)
    await closing
    expect(stopped).toBe(true)
  })
})
