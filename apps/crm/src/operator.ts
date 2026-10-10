import { randomBytes } from 'node:crypto'
import { migrateActHost, grantActHostRuntime } from '@archava/act-host-pg'
import {
  migrateNativeCrmAct,
  grantNativeCrmAct,
  nativeSqlClient,
  createPostgresNativeCrmHost,
} from '@archava/crm-act/postgres'
import {
  initializeNativeControl,
  writeNativeControl,
  readNativeControl,
  nativeConfirmationKey,
  NATIVE_SERVICE_ID,
} from './native-actions.js'
import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { lstat } from 'node:fs/promises'
import { Pool } from 'pg'
import { z } from 'zod'
import {
  migrateProviderRuntime,
  grantProviderRuntime,
  readProviderStatus,
  holdProviderAfterRestore,
} from '@archava/provider-runtime/postgres'
import { CRM_PROVIDER_POLICY } from './provider-policy.js'
import { CRM_PERMISSIONS, CrmService } from '@archava/crm'
import { migrateCrm, grantCrmRuntime, PostgresCrmRepository } from '@archava/crm-pg'
import {
  migrateIdentity,
  grantIdentityRuntime,
  provisionIdentity,
  resetIdentityPassword,
  usernameSchema,
  passwordSchema,
} from '@archava/identity-pg'
import {
  home,
  profilePath,
  settingsSchema,
  tenantConfig,
  databaseName,
  readPrivateJson,
  writePrivateJson,
  loadSettings,
  connection,
  type RuntimeSettings,
} from './settings.js'
import {
  initializeLocalDatabase,
  withLocalDatabase,
  pgTool,
  databaseEnvironment,
} from './local-postgres.js'
import { runCrmBackup, readBackupStatus, setBackupPolicy } from './backups.js'
import { verifyRuntimeRelease, verifyReleaseDatabase, ReleaseProfileError } from './release.js'

const credentialsSchema = z
  .object({
    username: usernameSchema,
    password: passwordSchema,
    role: z.enum(['owner', 'staff', 'viewer']),
    tenants: z.array(z.string()).min(1).max(20),
  })
  .strict()
const role = 'archava_crm_app'
async function grant(pool: Pool) {
  await grantCrmRuntime(pool, role)
  await grantIdentityRuntime(pool, role)
  await grantProviderRuntime(pool, role)
  await grantNativeCrmAct(pool, role)
  await grantActHostRuntime(nativeSqlClient(pool, 'operator', 'operator'), role)
}
async function migrateNative(pool: Pool) {
  await migrateActHost(nativeSqlClient(pool, 'operator', 'operator'))
  await migrateNativeCrmAct(pool)
}
async function enroll(pool: Pool, settings: RuntimeSettings, input: unknown) {
  const account = credentialsSchema.parse(input)
  if (account.tenants.some((tenant) => !settings.tenants.some((item) => item.id === tenant)))
    throw new Error('Unknown tenant in enrollment')
  const permissions =
    account.role === 'viewer'
      ? CRM_PERMISSIONS.filter((value) => value.endsWith('.read'))
      : CRM_PERMISSIONS.filter((value) => account.role === 'owner' || value !== 'audit.read')
  return await provisionIdentity(
    pool,
    account.username,
    account.password,
    async (client, identity) => {
      for (const tenant of account.tenants)
        await client.query(
          "INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,'human',$3,$4)",
          [
            tenant,
            identity.principalId,
            permissions,
            account.role === 'staff' ? 'assigned' : 'all',
          ],
        )
    },
  )
}
async function setup() {
  await initializeLocalDatabase()
  const fresh = !existsSync(profilePath)
  if (fresh) {
    await writePrivateJson(profilePath, {
      version: 1,
      database: 'archava_crm',
      databasePort: 20000 + Math.floor(Math.random() * 25000),
      appPort: 4175,
      fingerprintKey: randomBytes(32).toString('base64url'),
      identityKey: randomBytes(32).toString('base64url'),
      tenants: [
        {
          id: 'hotel-demo',
          title: 'Hospitality',
          contacts: 'Guests / inquiries',
          contact: 'Guest',
          pipelineLabels: {
            new: 'New inquiry',
            qualified: 'Qualified inquiry',
            proposal: 'Offer',
            won: 'Won inquiry',
            lost: 'Lost inquiry',
          },
        },
        {
          id: 'studio-demo',
          title: 'Professional services',
          contacts: 'Clients / inquiries',
          contact: 'Client',
          pipelineLabels: {
            new: 'New lead',
            qualified: 'Qualified lead',
            proposal: 'Proposal',
            won: 'Won opportunity',
            lost: 'Lost opportunity',
          },
        },
      ].map((value) => tenantConfig.parse(value)),
    })
  }
  const settings = await loadSettings()
  await withLocalDatabase(settings, async () => {
    const cluster = new Pool({ ...connection(settings, 'archava_operator'), database: 'postgres' })
    try {
      if (
        (await cluster.query('SELECT 1 FROM pg_database WHERE datname=$1', [settings.database]))
          .rowCount === 0
      )
        await cluster.query(`CREATE DATABASE "${settings.database}"`)
      if ((await cluster.query('SELECT 1 FROM pg_roles WHERE rolname=$1', [role])).rowCount === 0)
        await cluster.query(`CREATE ROLE "${role}" LOGIN NOSUPERUSER NOBYPASSRLS`)
    } finally {
      await cluster.end()
    }
    const owner = new Pool(connection(settings, 'archava_operator'))
    try {
      await migrateCrm(owner)
      await migrateIdentity(owner)
      await migrateProviderRuntime(owner)
      await migrateNative(owner)
      await grant(owner)
      for (const tenant of settings.tenants) {
        await owner.query(
          "INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,'service',$3,'all') ON CONFLICT DO NOTHING",
          [
            tenant.id,
            NATIVE_SERVICE_ID,
            ['contacts.read', 'activities.read', 'followups.read', 'followups.write'],
          ],
        )
        const member = (
          await owner.query<{ kind: string }>(
            'SELECT kind FROM archava_crm.memberships WHERE tenant_id=$1 AND principal_id=$2',
            [tenant.id, NATIVE_SERVICE_ID],
          )
        ).rows[0]
        if (member?.kind !== 'service') throw new Error('Native service identity conflict')
      }
      await initializeNativeControl(settings)
      if ((await owner.query('SELECT 1 FROM archava_identity.users LIMIT 1')).rowCount === 0) {
        const account = {
          username: 'owner@archava.local',
          password: randomBytes(24).toString('base64url'),
          role: 'owner' as const,
          tenants: settings.tenants.map((item) => item.id),
        }
        // Write the recovery credential before enrollment; a setup retry uses this same secret.
        const credentialPath = join(home, 'initial-owner.json')
        if (!existsSync(credentialPath)) await writePrivateJson(credentialPath, account)
        await enroll(owner, settings, await readPrivateJson(credentialPath))
      }
    } finally {
      await owner.end()
    }
  })
  process.stdout.write(
    `CRM initialized. Initial owner credential is in ${join(home, 'initial-owner.json')}; it is never printed.\n`,
  )
}
async function user(args: string[]) {
  const [mode, file] = args
  if (!['add', 'reset', 'recover', 'pipeline'].includes(mode ?? '') || !file)
    throw new Error('Expected user add|reset|recover|pipeline private-input.json')
  const settings = await loadSettings()
  const input = await readPrivateJson(resolve(file))
  await withLocalDatabase(settings, async () => {
    const owner = new Pool(connection(settings, 'archava_operator'))
    try {
      if (mode === 'add') await enroll(owner, settings, input)
      else if (mode === 'pipeline') {
        const account = z
          .object({
            username: usernameSchema,
            tenants: z.array(z.string()).min(1).max(20),
            access: z.enum(['read', 'write', 'none']),
          })
          .strict()
          .parse(input)
        if (account.tenants.some((tenant) => !settings.tenants.some((item) => item.id === tenant)))
          throw new Error('Unknown tenant')
        const client = await owner.connect()
        try {
          await client.query('BEGIN')
          const identity = (
            await client.query<{ id: string }>(
              'SELECT id FROM archava_identity.users WHERE username=$1 AND active FOR SHARE',
              [account.username],
            )
          ).rows[0]
          if (!identity) throw new Error('Active account required')
          for (const tenant of new Set(account.tenants)) {
            const member = (
              await client.query<{ permissions: string[] }>(
                "SELECT permissions FROM archava_crm.memberships WHERE tenant_id=$1 AND principal_id=$2 AND kind='human' AND active FOR UPDATE",
                [tenant, identity.id],
              )
            ).rows[0]
            if (!member || !member.permissions.includes('contacts.read'))
              throw new Error('Active contact membership required')
            const grants = member.permissions.filter((value) => !value.startsWith('pipeline.'))
            if (account.access !== 'none') grants.push('pipeline.read')
            if (account.access === 'write') grants.push('pipeline.write')
            await client.query(
              'UPDATE archava_crm.memberships SET permissions=$3 WHERE tenant_id=$1 AND principal_id=$2',
              [tenant, identity.id, grants],
            )
          }
          await client.query('COMMIT')
        } catch (error) {
          await client.query('ROLLBACK')
          throw error
        } finally {
          client.release()
        }
      } else {
        const account = z
          .object({ username: usernameSchema, password: passwordSchema })
          .strict()
          .parse(input)
        await resetIdentityPassword(owner, account.username, account.password, mode === 'recover')
      }
    } finally {
      await owner.end()
    }
  })
  process.stdout.write('Account operation completed; no credential was printed.\n')
}
async function backup(args: string[]) {
  const settings = await loadSettings()
  if (args.length > 1) throw new Error('Expected backup [private-output.dump]')
  const result = await runCrmBackup(settings, { destination: args[0] })
  if (result.outcome !== 'succeeded') throw new Error('Backup is already running')
  process.stdout.write(`Backup saved: ${result.file}\n`)
}
async function backupStatus() {
  const settings = await loadSettings()
  process.stdout.write(JSON.stringify(await readBackupStatus(settings)) + '\n')
}
async function backupPolicy(args: string[]) {
  const [mode, hours] = args
  if (
    !['enable', 'disable'].includes(mode ?? '') ||
    args.length > 2 ||
    (mode === 'disable' && hours !== undefined)
  )
    throw new Error('Expected backup-policy enable [hours] | disable')
  const settings = await loadSettings()
  const policy = await setBackupPolicy(
    settings,
    mode === 'enable',
    hours === undefined ? undefined : Number(hours),
  )
  process.stdout.write(
    JSON.stringify({
      enabled: policy.enabled,
      intervalHours: policy.intervalHours,
      appliesWhileHostRuns: true,
      automaticDeletion: false,
    }) + '\n',
  )
}
async function backupRun() {
  const settings = await loadSettings()
  process.stdout.write(
    JSON.stringify({ outcome: (await runCrmBackup(settings, { onlyIfDue: true })).outcome }) + '\n',
  )
}
async function restore(args: string[]) {
  const [rawBackup, rawTarget] = args
  if (!rawBackup || !rawTarget)
    throw new Error('Expected restore trusted-backup.dump new_database_name')
  const target = databaseName.parse(rawTarget)
  const settings = await loadSettings()
  if (target === settings.database) throw new Error('Restore requires a new database')
  const file = resolve(rawBackup)
  const stat = await lstat(file)
  if (
    !stat.isFile() ||
    stat.isSymbolicLink() ||
    (stat.mode & 0o077) !== 0 ||
    (process.getuid && stat.uid !== process.getuid())
  )
    throw new Error('Restore requires a private trusted archive')
  const snapshot = settingsSchema.parse(await readPrivateJson(file + '.runtime.json'))
  const recoveryProfile = join(home, 'restored-runtime.json')
  if (existsSync(recoveryProfile)) throw new Error('Recovery profile already exists; preserve it')
  await withLocalDatabase(settings, async () => {
    const cluster = new Pool({ ...connection(settings, 'archava_operator'), database: 'postgres' })
    try {
      await cluster.query(`CREATE DATABASE "${target}"`)
    } finally {
      await cluster.end()
    }
    pgTool(
      'pg_restore',
      [
        '--exit-on-error',
        '--single-transaction',
        '--no-owner',
        '--no-privileges',
        '--dbname',
        target,
        file,
      ],
      databaseEnvironment(settings),
    )
    const recovered = settingsSchema.parse({
      ...snapshot,
      database: target,
      databasePort: settings.databasePort,
      appPort: settings.appPort,
      identityKey: randomBytes(32).toString('base64url'),
    })
    const owner = new Pool(connection(recovered, 'archava_operator'))
    try {
      await migrateCrm(owner)
      await migrateIdentity(owner)
      await migrateProviderRuntime(owner)
      await holdProviderAfterRestore(owner, CRM_PROVIDER_POLICY)
      await writeNativeControl(recovered, true, true)
      await migrateNative(owner)
      await grant(owner)
      // Restoring an older backup must never resurrect a logged-out session.
      await owner.query('DELETE FROM archava_identity.sessions')
      await owner.query('DELETE FROM archava_identity.login_buckets')
      await owner.query('UPDATE archava_identity.users SET active=false,version=version+1')
    } finally {
      await owner.end()
    }
    await writePrivateJson(recoveryProfile, recovered)
  })
  process.stdout.write(
    'Restored into a new database. Use ARCHAVA_CRM_PROFILE=restored-runtime.json and recover selected accounts with new passwords; the original database is unchanged.\n',
  )
}
async function aiStatus() {
  const settings = await loadSettings()
  await withLocalDatabase(settings, async () => {
    const owner = new Pool(connection(settings, 'archava_operator'))
    try {
      process.stdout.write(
        JSON.stringify(await readProviderStatus(owner, CRM_PROVIDER_POLICY), null, 2) + '\n',
      )
    } finally {
      await owner.end()
    }
  })
}
async function resumeActions() {
  const settings = await loadSettings()
  await withLocalDatabase(settings, async () => {
    const owner = new Pool(connection(settings, 'archava_operator'))
    try {
      await migrateCrm(owner)
      await migrateNative(owner)
      await grant(owner)
      const active = (
        await owner.query(
          "SELECT 1 FROM archava_identity.users u JOIN archava_crm.memberships m ON m.principal_id=u.id WHERE u.active AND m.active AND m.kind='human' AND 'followups.write'=ANY(m.permissions) LIMIT 1",
        )
      ).rowCount
      if (!active) throw new Error('Recover an authorized account before resuming native actions')
      await writeNativeControl(settings, false, true)
    } finally {
      await owner.end()
    }
  })
  process.stdout.write('Native follow-up actions resumed with a fresh approval epoch.\n')
}
async function doctor() {
  const releaseProfile = await verifyRuntimeRelease()
  const settings = await loadSettings()
  let backups: { status: string } = { status: 'unavailable' }
  try {
    backups = await readBackupStatus(settings)
  } catch {
    /* Backup diagnostics must not print configuration or prevent database diagnostics. */
  }
  await withLocalDatabase(settings, async () => {
    const app = new Pool(connection(settings))
    try {
      await verifyReleaseDatabase(releaseProfile, app)
      const repository = await PostgresCrmRepository.create(app, settings.fingerprintKey)
      const control = await readNativeControl(settings)
      const tenant = settings.tenants[0]
      if (!tenant) throw new Error('Workspace configuration missing')
      await createPostgresNativeCrmHost(app, {
        tenantId: tenant.id,
        humanId: 'diagnostic',
        serviceId: NATIVE_SERVICE_ID,
        confirmationKey: nativeConfirmationKey(settings),
        control: () => readNativeControl(settings),
        service: new CrmService(repository),
        clock: Date.now,
      })
      process.stdout.write(
        JSON.stringify({
          status: 'ready',
          release: { id: releaseProfile.catalog.id, version: releaseProfile.catalog.version, verified: true },
          workspaces: settings.tenants.length,
          nativeActions: control.held ? 'held' : 'ready',
          externalActions: 'not configured',
          backups,
          credentialValuesPrinted: false,
        }) + '\n',
      )
    } finally {
      await app.end()
    }
  })
}
try {
  await verifyRuntimeRelease()
  const [mode, ...args] = process.argv.slice(2)
  if (mode === 'setup') await setup()
  else if (mode === 'user') await user(args)
  else if (mode === 'backup') await backup(args)
  else if (mode === 'backup-status') await backupStatus()
  else if (mode === 'backup-policy') await backupPolicy(args)
  else if (mode === 'backup-run') await backupRun()
  else if (mode === 'restore') await restore(args)
  else if (mode === 'ai-status') await aiStatus()
  else if (mode === 'resume-actions') await resumeActions()
  else if (mode === 'doctor') await doctor()
  else throw new Error('Unknown CRM operator command')
} catch (error) {
  if (error instanceof ReleaseProfileError) process.stderr.write(error.message + '\n')
  if (
    typeof error === 'object' &&
    error !== null &&
    'path' in error &&
    typeof error.path === 'string'
  )
    process.stderr.write(`Runtime file unavailable: ${JSON.stringify(error.path)}\n`)
  const code =
    error instanceof z.ZodError
      ? 'INVALID_CONFIG'
      : typeof error === 'object' &&
          error !== null &&
          'code' in error &&
          typeof error.code === 'string' &&
          /^[A-Z0-9_]{3,32}$/.test(error.code)
        ? error.code
        : error instanceof Error
          ? error.name
          : 'UNKNOWN'
  process.stderr.write(
    `CRM operator command failed (${code}). Check private configuration, command arguments and database state.\n`,
  )
  process.exitCode = 1
}
