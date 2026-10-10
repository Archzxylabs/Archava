import { readFile, rename, unlink } from 'node:fs/promises'
import { join } from 'node:path'
import type { Pool } from 'pg'
import { FULL_MODULES, moduleSelectionSchema, type ModuleSelection } from '@archava/config/assembly'
import { home, loadSettings, profilePath, readPrivateJson, writePrivateJson } from './settings.js'
import { connection } from './settings.js'
import { withLocalDatabase } from './local-postgres.js'
import { Pool as PgPool } from 'pg'

export async function assertModuleLifecycle(pool: Pool, modules: ModuleSelection) {
  if (modules.nativeActions) return
  const result = await pool.query<{ unresolved: boolean }>(
    "SELECT EXISTS(SELECT 1 FROM archava_act_host.intents WHERE phase IN ('pending','executing','unknown')) AS unresolved",
  )
  if (result.rows[0]?.unresolved !== false)
    throw new Error('Unresolved native effects: reconcile or cancel before disabling nativeActions')
}
export async function setModules(file: string) {
  try {
    const pid = Number(await readFile(join(home, 'runtime.lock'), 'utf8'))
    if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error('Invalid runtime lock')
    let alive = true
    try {
      process.kill(pid, 0)
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ESRCH') throw error
      alive = false
    }
    if (alive) throw new Error('Stop the CRM host before changing modules')
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  }
  const modules = moduleSelectionSchema.parse(await readPrivateJson(file))
  const settings = await loadSettings()
  const previous = settings.modules ?? FULL_MODULES
  await withLocalDatabase(settings, async () => {
    const pool = new PgPool(connection(settings, 'archava_operator'))
    try {
      await assertModuleLifecycle(pool, modules)
    } finally {
      await pool.end()
    }
  })
  const temporary = profilePath + '.modules-next'
  await writePrivateJson(temporary, { ...settings, modules })
  try {
    await rename(temporary, profilePath)
  } finally {
    await unlink(temporary).catch(() => {})
  }
  return { previous, modules, dataPolicy: 'retained', restartRequired: true }
}
