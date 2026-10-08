import { lstat, mkdir, readFile, writeFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { z } from 'zod'
import { tenantSchema } from '@archava/crm'

export const databaseName = z.string().regex(/^[a-z][a-z0-9_]{0,62}$/)
export const tenantConfig = z
  .object({
    id: tenantSchema,
    title: z.string().min(1).max(160),
    contacts: z.string().min(1).max(80),
    contact: z.string().min(1).max(80),
  })
  .strict()
export const settingsSchema = z
  .object({
    version: z.literal(1),
    database: databaseName,
    databasePort: z.number().int().min(20000).max(45000),
    appPort: z.number().int().min(0).max(65535),
    fingerprintKey: z.string().min(43).max(128),
    identityKey: z.string().min(43).max(128),
    publicOrigin: z.url().optional(),
    tenants: z
      .array(tenantConfig)
      .min(1)
      .max(20)
      .refine((items) => new Set(items.map((item) => item.id)).size === items.length),
  })
  .strict()
export type RuntimeSettings = z.infer<typeof settingsSchema>
export const home = resolve(process.env['ARCHAVA_CRM_HOME'] ?? '.local/crm')
export const profile = process.env['ARCHAVA_CRM_PROFILE'] ?? 'runtime.json'
if (!/^[a-z0-9_-]+\.json$/.test(profile)) throw new Error('Invalid CRM profile name')
export const profilePath = join(home, profile)
export async function privateDirectory(path: string, create = false) {
  if (create) await mkdir(path, { recursive: true, mode: 0o700 })
  const stat = await lstat(path)
  if (
    !stat.isDirectory() ||
    stat.isSymbolicLink() ||
    (stat.mode & 0o077) !== 0 ||
    (process.getuid && stat.uid !== process.getuid())
  )
    throw new Error('CRM state directory must be privately owned with mode 0700')
}
export async function readPrivateJson(path: string): Promise<unknown> {
  const stat = await lstat(path)
  if (
    !stat.isFile() ||
    stat.isSymbolicLink() ||
    (stat.mode & 0o077) !== 0 ||
    (process.getuid && stat.uid !== process.getuid()) ||
    stat.size > 65536
  )
    throw new Error('CRM private file must be owned, bounded and mode 0600')
  return JSON.parse(await readFile(path, 'utf8')) as unknown
}
export async function writePrivateJson(path: string, value: unknown) {
  await writeFile(path, JSON.stringify(value, null, 2) + '\n', { mode: 0o600, flag: 'wx' })
}
export function validateOrigin(raw: string | undefined) {
  if (!raw) return undefined
  const url = new URL(raw)
  if (url.origin !== raw || url.username || url.password || url.protocol !== 'https:')
    throw new Error('Public CRM origin must be an HTTPS origin')
  return raw
}
export async function loadSettings() {
  await privateDirectory(home)
  const settings = settingsSchema.parse(await readPrivateJson(profilePath))
  validateOrigin(settings.publicOrigin)
  return settings
}
export function connection(settings: RuntimeSettings, user = 'archava_crm_app') {
  return {
    host: join(home, 'socket'),
    port: settings.databasePort,
    database: settings.database,
    user,
    password: () => '',
    ssl: false,
    options: '-c row_security=on',
    sslnegotiation: 'postgres' as const,
    max: 8,
    connectionTimeoutMillis: 5000,
    statement_timeout: 10000,
  }
}
