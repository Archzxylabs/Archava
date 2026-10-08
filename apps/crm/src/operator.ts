import { randomBytes } from 'node:crypto'
import { existsSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import { chmod, lstat, writeFile } from 'node:fs/promises'
import { Pool } from 'pg'
import { z } from 'zod'
import { CRM_PERMISSIONS } from '@archava/crm'
import { migrateCrm, grantCrmRuntime } from '@archava/crm-pg'
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
  privateDirectory,
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
      await grant(owner)
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
  const directory = join(home, 'backups')
  await privateDirectory(directory, true)
  const file = args[0] ? resolve(args[0]) : join(directory, `crm-${Date.now()}.dump`)
  await privateDirectory(dirname(file))
  if (existsSync(file) || existsSync(file + '.runtime.json'))
    throw new Error('Backup target already exists')
  await writeFile(file, '', { mode: 0o600, flag: 'wx' })
  await withLocalDatabase(settings, async () => {
    pgTool(
      'pg_dump',
      ['--format=custom', '--no-owner', '--no-privileges', '--file', file],
      databaseEnvironment(settings),
    )
    await chmod(file, 0o600)
    await writePrivateJson(file + '.runtime.json', settings)
  })
  process.stdout.write(`Backup saved: ${file}\n`)
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
try {
  const [mode, ...args] = process.argv.slice(2)
  if (mode === 'setup') await setup()
  else if (mode === 'user') await user(args)
  else if (mode === 'backup') await backup(args)
  else if (mode === 'restore') await restore(args)
  else throw new Error('Unknown CRM operator command')
} catch (error) {
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
