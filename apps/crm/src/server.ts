import { Pool } from 'pg'
import { setDefaultResultOrder } from 'node:dns'
import { open, readFile, unlink } from 'node:fs/promises'
import { join } from 'node:path'
import { CrmService } from '@archava/crm'
import { CrmPipelineService } from '@archava/crm/pipeline'
import { PostgresCrmRepository } from '@archava/crm-pg'
import { createNativeCrmVerifier, createPostgresNativeCrmHost } from '@archava/crm-act/postgres'
import { nativeConfirmationKey, readNativeControl, NATIVE_SERVICE_ID } from './native-actions.js'
import { PostgresIdentity } from '@archava/identity-pg'
import { home, loadSettings, connection } from './settings.js'
import { startLocalDatabase, stopLocalDatabase } from './local-postgres.js'
import { startCrmHost } from './host.js'
import { loadCrmPlanner } from './planner.js'
import { PostgresProviderAttemptStore } from '@archava/provider-runtime/postgres'

// This local host also runs on desktops without reliable IPv6 routing.
setDefaultResultOrder('ipv4first')

let release = () => {}
const stopped = new Promise<void>((done) => {
  release = done
})
process.on('SIGINT', () => release())
process.on('SIGTERM', () => release())
let pool: Pool | undefined
let host: Awaited<ReturnType<typeof startCrmHost>> | undefined
let ownsDatabase = false
let ownsLock = false
const lock = join(home, 'runtime.lock')
try {
  const settings = await loadSettings()
  try {
    const previous = Number(await readFile(lock, 'utf8'))
    if (!Number.isSafeInteger(previous) || previous <= 0) throw new Error('Invalid runtime lock')
    let alive = true
    try {
      process.kill(previous, 0)
    } catch {
      alive = false
    }
    if (alive) throw new Error('A CRM host is already running')
    await unlink(lock)
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error
  }
  const file = await open(lock, 'wx', 0o600)
  await file.writeFile(String(process.pid))
  await file.close()
  ownsLock = true
  ownsDatabase = await startLocalDatabase(settings)
  pool = new Pool(connection(settings))
  pool.on('error', () => process.stderr.write('CRM database connection unavailable.\n'))
  const nativeOptions = {
    serviceId: NATIVE_SERVICE_ID,
    confirmationKey: nativeConfirmationKey(settings),
    control: () => readNativeControl(settings),
  }
  const repository = await PostgresCrmRepository.create(
    pool,
    settings.fingerprintKey,
    createNativeCrmVerifier(nativeOptions),
  )
  const service = new CrmService(repository)
  const identity = await PostgresIdentity.create(pool, settings.identityKey)
  const planner = await loadCrmPlanner(await PostgresProviderAttemptStore.create(pool))
  const runtimePool = pool
  host = await startCrmHost(
    service,
    identity,
    pool,
    settings,
    join(import.meta.dirname, 'public'),
    new CrmPipelineService(repository),
    planner.select,
    planner.gemini ? 'gemini-synthetic' : 'local',
    (tenantId, humanId) =>
      createPostgresNativeCrmHost(runtimePool, {
        ...nativeOptions,
        tenantId,
        humanId,
        service,
        clock: Date.now,
      }),
  )
  process.stdout.write(`CRM ready: ${host.origin}\n`)
  await stopped
} catch {
  process.stderr.write(
    'CRM startup failed. Check setup, private settings, port and runtime lock.\n',
  )
  process.exitCode = 1
} finally {
  await host?.close()
  await pool?.end()
  if (ownsDatabase) stopLocalDatabase()
  if (ownsLock) await unlink(lock)
}
