import type { Pool } from 'pg'
import { createRuntimePool, closeRuntimePool } from './postgres-pool.js'
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
import { startLocalBackups } from './backups.js'
import { verifyRuntimeRelease, verifyReleaseDatabase, ReleaseProfileError } from './release.js'
import { assertModuleLifecycle } from './modules.js'
import { FULL_MODULES } from '@archava/config/assembly'
import { ProjectService } from '@archava/projects'
import { PostgresProjectRepository } from '@archava/projects-pg'
import { SchedulingService } from '@archava/scheduling'
import { PostgresSchedulingRepository } from '@archava/scheduling-pg'
import { AccommodationService } from '@archava/accommodation'
import { PostgresAccommodationRepository } from '@archava/accommodation-pg'
import { PurchasingService } from '@archava/purchasing'
import { PostgresPurchaseRepository } from '@archava/purchasing-pg'
import { SupportService } from '@archava/support'
import { PostgresSupportRepository } from '@archava/support-pg'
import { FoodService } from '@archava/foodservice'
import { PostgresFoodRepository } from '@archava/foodservice-pg'
import { CommerceService } from '@archava/commerce'
import { PostgresCommerceRepository } from '@archava/commerce-pg'
import { OrganizationService } from '@archava/organizations'
import { PostgresOrganizationRepository } from '@archava/organizations-pg'
import { ProposalService } from '@archava/proposals'
import { PostgresProposalRepository } from '@archava/proposals-pg'
import { CapacityService } from '@archava/capacity'
import { PostgresCapacityRepository } from '@archava/capacity-pg'
import { AssetService } from '@archava/assets'
import { PostgresAssetRepository } from '@archava/assets-pg'
import { SubscriptionService } from '@archava/subscriptions'
import { PostgresSubscriptionRepository } from '@archava/subscriptions-pg'
import { AttendanceService } from '@archava/attendance'
import { PostgresAttendanceRepository } from '@archava/attendance-pg'
import { FulfillmentService } from '@archava/fulfillment'
import { PostgresFulfillmentRepository } from '@archava/fulfillment-pg'

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
let backups: ReturnType<typeof startLocalBackups> | undefined
let ownsDatabase = false
let ownsLock = false
const lock = join(home, 'runtime.lock')
try {
  const releaseProfile = await verifyRuntimeRelease()
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
  if (!(settings.modules ?? FULL_MODULES).nativeActions) {
    const operator = createRuntimePool(connection(settings, 'archava_operator'))
    try {
      await assertModuleLifecycle(operator, settings.modules ?? FULL_MODULES)
    } finally {
      await closeRuntimePool(operator)
    }
  }
  pool = createRuntimePool(connection(settings))
  pool.on('error', () => process.stderr.write('CRM database connection unavailable.\n'))
  await verifyReleaseDatabase(releaseProfile, pool)
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
    (settings.modules ?? FULL_MODULES).projects
      ? new ProjectService(
          await PostgresProjectRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).scheduling
      ? new SchedulingService(
          await PostgresSchedulingRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).accommodation
      ? new AccommodationService(
          await PostgresAccommodationRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).commerce
      ? new CommerceService(
          await PostgresCommerceRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).foodservice
      ? new FoodService(await PostgresFoodRepository.create(runtimePool, settings.fingerprintKey))
      : undefined,
    (settings.modules ?? FULL_MODULES).support
      ? new SupportService(
          await PostgresSupportRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).purchasing
      ? new PurchasingService(
          await PostgresPurchaseRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).organizations
      ? new OrganizationService(
          await PostgresOrganizationRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).proposals
      ? new ProposalService(
          await PostgresProposalRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).capacity
      ? new CapacityService(
          await PostgresCapacityRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).assets
      ? new AssetService(await PostgresAssetRepository.create(runtimePool, settings.fingerprintKey))
      : undefined,
    (settings.modules ?? FULL_MODULES).subscriptions
      ? new SubscriptionService(
          await PostgresSubscriptionRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).attendance
      ? new AttendanceService(
          await PostgresAttendanceRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
    (settings.modules ?? FULL_MODULES).fulfillment
      ? new FulfillmentService(
          await PostgresFulfillmentRepository.create(runtimePool, settings.fingerprintKey),
        )
      : undefined,
  )
  process.stdout.write(`CRM ready: ${host.origin}\n`)
  backups = startLocalBackups(settings)
  await stopped
} catch (error) {
  if (error instanceof ReleaseProfileError) process.stderr.write(error.message + '\n')
  process.stderr.write(
    'CRM startup failed. Check setup, private settings, port and runtime lock.\n',
  )
  process.exitCode = 1
} finally {
  await host?.close()
  await backups?.stop()
  if (pool) await closeRuntimePool(pool)
  if (ownsDatabase) stopLocalDatabase()
  if (ownsLock) await unlink(lock)
}
