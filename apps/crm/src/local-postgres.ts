import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { home, privateDirectory, type RuntimeSettings } from './settings.js'

export const pgBinary = () => execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
export function pgTool(name: string, args: string[], env?: NodeJS.ProcessEnv) {
  return execFileSync(join(pgBinary(), name), args, {
    encoding: 'utf8',
    stdio: 'pipe',
    ...(env ? { env } : {}),
  })
}
export async function initializeLocalDatabase() {
  await privateDirectory(home, true)
  await privateDirectory(join(home, 'socket'), true)
  const data = join(home, 'data')
  if (!existsSync(join(data, 'PG_VERSION')))
    pgTool('initdb', ['-D', data, '-U', 'archava_operator', '--auth=trust'])
  await privateDirectory(data)
}
export async function startLocalDatabase(settings: RuntimeSettings) {
  await privateDirectory(join(home, 'data'))
  await privateDirectory(join(home, 'socket'))
  try {
    pgTool('pg_ctl', ['-D', join(home, 'data'), 'status'])
    return false
  } catch {
    // pg_ctl interprets -o through a shell; preserve an operator-owned path literally.
    const socket = "'" + join(home, 'socket').replace(/'/g, "'\\''") + "'"
    pgTool('pg_ctl', [
      '-D',
      join(home, 'data'),
      '-l',
      join(home, 'postgres.log'),
      '-w',
      'start',
      '-o',
      `-k ${socket} -p ${settings.databasePort} -c listen_addresses=''`,
    ])
  }
  return true
}
export async function withLocalDatabase<T>(settings: RuntimeSettings, run: () => Promise<T>) {
  const started = await startLocalDatabase(settings)
  try {
    return await run()
  } finally {
    if (started) stopLocalDatabase()
  }
}
export function stopLocalDatabase() {
  try {
    pgTool('pg_ctl', ['-D', join(home, 'data'), 'status'])
  } catch {
    return
  }
  pgTool('pg_ctl', ['-D', join(home, 'data'), '-m', 'fast', '-w', 'stop'])
}
export function databaseEnvironment(settings: RuntimeSettings) {
  return {
    ...Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('PG'))),
    PGHOST: join(home, 'socket'),
    PGPORT: String(settings.databasePort),
    PGDATABASE: settings.database,
    PGUSER: 'archava_operator',
  }
}
