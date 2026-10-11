import { createHash, randomUUID } from 'node:crypto'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { createReadStream } from 'node:fs'
import { link, lstat, open, rename, unlink } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { createRuntimePool, closeRuntimePool } from './postgres-pool.js'
import { z } from 'zod'
import {
  home,
  privateDirectory,
  readPrivateJson,
  writePrivateJson,
  settingsSchema,
  connection,
  type RuntimeSettings,
} from './settings.js'
import { pgBinary, databaseEnvironment, withLocalDatabase } from './local-postgres.js'

const execute = promisify(execFile)
const policySchema = z
  .object({
    version: z.literal(1),
    enabled: z.boolean(),
    intervalHours: z.number().int().min(1).max(168),
  })
  .strict()
const successSchema = z
  .object({
    file: z.string().min(1).max(4096),
    completedAt: z.iso.datetime(),
    bytes: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
    archiveDigest: z.string().regex(/^[a-f0-9]{64}$/),
    configurationDigest: z.string().regex(/^[a-f0-9]{64}$/),
  })
  .strict()
const stateSchema = z
  .object({
    version: z.literal(1),
    lastAttemptAt: z.iso.datetime(),
    lastOutcome: z.enum(['succeeded', 'failed']),
    lastSuccess: successSchema.nullable(),
  })
  .strict()
const digest = (value: string) => createHash('sha256').update(value).digest('hex')
const policyPath = (settings: RuntimeSettings) =>
  join(home, 'backup-policy-' + settings.database + '.json')
const statePath = (settings: RuntimeSettings) =>
  join(home, 'backup-state-' + settings.database + '.json')
const missing = (error: unknown) =>
  error instanceof Error && 'code' in error && error.code === 'ENOENT'
async function readPolicy(settings: RuntimeSettings) {
  try {
    return policySchema.parse(await readPrivateJson(policyPath(settings)))
  } catch (error) {
    if (missing(error)) return policySchema.parse({ version: 1, enabled: false, intervalHours: 24 })
    throw error
  }
}
async function readState(settings: RuntimeSettings) {
  try {
    return stateSchema.parse(await readPrivateJson(statePath(settings)))
  } catch (error) {
    if (missing(error)) return null
    throw error
  }
}
async function replacePrivateJson(path: string, value: unknown) {
  try {
    await readPrivateJson(path)
  } catch (error) {
    if (!missing(error)) throw error
  }
  const temporary = path + '.' + randomUUID() + '.partial'
  try {
    await writePrivateJson(temporary, value)
    const file = await open(temporary, 'r')
    try {
      await file.sync()
    } finally {
      await file.close()
    }
    await rename(temporary, path)
  } finally {
    await unlink(temporary).catch(() => {})
  }
}
export async function setBackupPolicy(settings: RuntimeSettings, enabled: boolean, hours?: number) {
  const previous = await readPolicy(settings)
  const policy = policySchema.parse({
    ...previous,
    enabled,
    intervalHours: hours ?? previous.intervalHours,
  })
  await replacePrivateJson(policyPath(settings), policy)
  return policy
}
async function fileDigest(path: string) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(path)) {
    if (!Buffer.isBuffer(chunk)) throw new Error('Invalid archive stream')
    hash.update(chunk)
  }
  return hash.digest('hex')
}
async function ownedArchive(path: string) {
  await privateDirectory(dirname(path))
  const info = await lstat(path)
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    (info.mode & 0o077) !== 0 ||
    (process.getuid && info.uid !== process.getuid())
  )
    throw new Error('Private archive required')
  return info
}
async function checkSuccess(settings: RuntimeSettings, success: z.infer<typeof successSchema>) {
  if (success.configurationDigest !== digest(JSON.stringify(settings))) return false
  const info = await ownedArchive(success.file)
  const metadata = settingsSchema.parse(await readPrivateJson(success.file + '.runtime.json'))
  return (
    info.size === success.bytes &&
    digest(JSON.stringify(metadata)) === success.configurationDigest &&
    (await fileDigest(success.file)) === success.archiveDigest
  )
}
export async function readBackupStatus(settings: RuntimeSettings, now = Date.now()) {
  const policy = await readPolicy(settings),
    state = await readState(settings)
  let archiveIntact = false
  if (state?.lastSuccess) {
    try {
      archiveIntact = await checkSuccess(settings, state.lastSuccess)
    } catch {
      /* Report unavailable material without its private path or payload. */
    }
  }
  const lastSuccessAt = state?.lastSuccess?.completedAt ?? null,
    completed = lastSuccessAt ? Date.parse(lastSuccessAt) : null,
    fresh =
      archiveIntact &&
      completed !== null &&
      now >= completed &&
      now - completed < policy.intervalHours * 3_600_000
  return {
    enabled: policy.enabled,
    intervalHours: policy.intervalHours,
    status: !policy.enabled
      ? 'disabled'
      : state?.lastOutcome === 'failed'
        ? 'failed'
        : fresh
          ? 'fresh'
          : lastSuccessAt
            ? 'stale'
            : 'missing',
    archiveIntact,
    lastSuccessAt,
    lastAttemptAt: state?.lastAttemptAt ?? null,
    lastAttemptOutcome: state?.lastOutcome ?? null,
    nextDueAt:
      completed === null
        ? null
        : new Date(completed + policy.intervalHours * 3_600_000).toISOString(),
    archiveBytes: archiveIntact ? (state?.lastSuccess?.bytes ?? null) : null,
    automaticDeletion: false,
  }
}
async function pgCommand(
  name: 'pg_dump' | 'pg_restore',
  args: string[],
  settings: RuntimeSettings,
  signal?: AbortSignal,
) {
  await execute(join(pgBinary(), name), args, {
    env: databaseEnvironment(settings),
    timeout: 60_000,
    maxBuffer: 1_048_576,
    ...(signal ? { signal } : {}),
  })
}
async function capture(settings: RuntimeSettings, file: string, signal?: AbortSignal) {
  await privateDirectory(dirname(file))
  for (const target of [file, file + '.runtime.json']) {
    try {
      await lstat(target)
      throw new Error('Backup target exists')
    } catch (error) {
      if (!missing(error)) throw error
    }
  }
  const stage = file + '.' + randomUUID() + '.partial',
    metadata = stage + '.runtime.json'
  let publishedMetadata = false,
    publishedArchive = false
  try {
    const handle = await open(stage, 'wx', 0o600)
    await handle.close()
    await pgCommand(
      'pg_dump',
      ['--format=custom', '--no-owner', '--no-privileges', '--file', stage],
      settings,
      signal,
    )
    await pgCommand('pg_restore', ['--list', stage], settings, signal)
    const info = await ownedArchive(stage)
    if (info.size < 5) throw new Error('Empty backup refused')
    await writePrivateJson(metadata, settings)
    for (const path of [stage, metadata]) {
      const handle = await open(path, 'r')
      try {
        await handle.sync()
      } finally {
        await handle.close()
      }
    }
    signal?.throwIfAborted()
    await link(metadata, file + '.runtime.json')
    publishedMetadata = true
    await link(stage, file)
    publishedArchive = true
    return successSchema.parse({
      file,
      completedAt: new Date().toISOString(),
      bytes: info.size,
      archiveDigest: await fileDigest(file),
      configurationDigest: digest(JSON.stringify(settings)),
    })
  } catch (error) {
    if (publishedMetadata && !publishedArchive) await unlink(file + '.runtime.json').catch(() => {})
    throw error
  } finally {
    await unlink(stage).catch(() => {})
    await unlink(metadata).catch(() => {})
  }
}
/** One advisory-lock owner captures/publishes at a time. No retention deletion or domain write. */
export async function runCrmBackup(
  settings: RuntimeSettings,
  options: { onlyIfDue?: boolean; destination?: string; signal?: AbortSignal } = {},
) {
  return withLocalDatabase(settings, async () => {
    const pool = createRuntimePool({ ...connection(settings, 'archava_operator'), max: 1 })
    const client = await pool.connect().catch(async (error: unknown) => {
      await closeRuntimePool(pool)
      throw error
    })
    client.on('error', () => {})
    let locked = false,
      stateLoaded = false,
      previous: Awaited<ReturnType<typeof readState>> = null
    const lockName = 'archava/local-backup/v1/' + settings.database
    try {
      locked =
        (
          await client.query<{ acquired: boolean }>(
            'SELECT pg_try_advisory_lock(hashtextextended($1,0)) AS acquired',
            [lockName],
          )
        ).rows[0]?.acquired === true
      if (!locked) return { outcome: 'busy' as const }
      previous = await readState(settings)
      stateLoaded = true
      if (options.onlyIfDue) {
        const status = await readBackupStatus(settings)
        if (!status.enabled) return { outcome: 'disabled' as const }
        if (status.status === 'fresh') return { outcome: 'not_due' as const }
      }
      const directory = join(home, 'backups')
      await privateDirectory(directory, true)
      const destination = options.destination
        ? resolve(options.destination)
        : join(directory, `crm-${Date.now()}-${randomUUID()}.dump`)
      const success = await capture(settings, destination, options.signal)
      await client.query('SELECT 1')
      await replacePrivateJson(statePath(settings), {
        version: 1,
        lastAttemptAt: success.completedAt,
        lastOutcome: 'succeeded',
        lastSuccess: success,
      })
      return { outcome: 'succeeded' as const, file: success.file }
    } catch {
      if (stateLoaded)
        await replacePrivateJson(statePath(settings), {
          version: 1,
          lastAttemptAt: new Date().toISOString(),
          lastOutcome: 'failed',
          lastSuccess: previous?.lastSuccess ?? null,
        }).catch(() => {})
      throw new Error('Local backup failed')
    } finally {
      if (locked)
        await client
          .query('SELECT pg_advisory_unlock(hashtextextended($1,0))', [lockName])
          .catch(() => {})
      client.release()
      await closeRuntimePool(pool)
    }
  })
}
export function startBackupLoop(
  run: (signal: AbortSignal) => Promise<unknown>,
  onFailure: () => void,
  pollMs = 60_000,
) {
  let stopped = false,
    active: Promise<void> | null = null
  const controller = new AbortController()
  const tick = () => {
    if (stopped || active) return active ?? Promise.resolve()
    active = Promise.resolve()
      .then(() => {
        if (!stopped) return run(controller.signal)
      })
      .then(
        () => {},
        () => {
          if (!stopped) {
            try {
              onFailure()
            } catch {
              /* A diagnostic sink cannot crash the host. */
            }
          }
        },
      )
      .finally(() => {
        active = null
      })
    return active
  }
  const ready = tick()
  const timer = setInterval(() => {
    void tick()
  }, pollMs)
  timer.unref()
  return {
    ready,
    async stop() {
      stopped = true
      clearInterval(timer)
      controller.abort()
      await active
    },
  }
}
export function startLocalBackups(settings: RuntimeSettings) {
  return startBackupLoop(
    async (signal) => {
      if (!(await readPolicy(settings)).enabled) return
      await runCrmBackup(settings, { onlyIfDue: true, signal })
    },
    () =>
      process.stderr.write(
        'CRM automatic backup unavailable. Run crm:backup-status to inspect local backup status.\n',
      ),
  )
}
