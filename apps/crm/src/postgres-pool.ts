import { Pool, type PoolClient, type PoolConfig } from 'pg'
const liveClients = new WeakMap<Pool, Set<PoolClient>>()
/** Track clients until actual socket removal, including idle clients already closing. */
export function createRuntimePool(config: PoolConfig) {
  const pool = new Pool(config),
    clients = new Set<PoolClient>()
  liveClients.set(pool, clients)
  pool.on('connect', (client) => clients.add(client))
  pool.on('remove', (client) => clients.delete(client))
  return pool
}
/** pg-pool end() can resolve before remove events. Owned database shutdown must wait. */
export async function closeRuntimePool(pool: Pool) {
  const clients = liveClients.get(pool)
  if (!clients || clients.size === 0) {
    await pool.end()
    return
  }
  const removed = new Promise<void>((resolve) => {
    const onRemove = () => {
      if (clients.size === 0) {
        pool.off('remove', onRemove)
        resolve()
      }
    }
    pool.on('remove', onRemove)
  })
  await pool.end()
  await removed
  liveClients.delete(pool)
}
