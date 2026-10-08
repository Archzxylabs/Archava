import { Pool } from 'pg'
import { createCluster } from './cluster.js'
import { prepareConsumers } from './consumers.js'
import { startReferenceServer } from './server.js'
import { referenceShutdown } from './shutdown.js'

const shutdown = referenceShutdown()
const consumers = await prepareConsumers()
const cluster = await createCluster(consumers.postgres, consumers.crm.CRM_PERMISSIONS).catch(
  async (error: unknown) => {
    await consumers.close()
    throw error
  },
)
const pool = new Pool(cluster.application)
let server: Awaited<ReturnType<typeof startReferenceServer>> | null = null
try {
  const service = new consumers.crm.CrmService(
    await consumers.postgres.PostgresCrmRepository.create(pool, cluster.fingerprintKey),
  )
  server = await startReferenceServer(
    service,
    consumers.publicDirectory,
    consumers.crm.createCrmApi,
  )
  process.stdout.write(
    `Reference CRM: ${server.origin}/hospitality/ and ${server.origin}/professional-services/\nFictional local sessions; disposable PostgreSQL data is removed on exit.\n`,
  )
  await shutdown.wait()
} finally {
  await server?.close()
  await pool.end()
  await cluster.close()
  await consumers.close()
}
