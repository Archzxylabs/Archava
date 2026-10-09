import { createHmac, randomUUID } from 'node:crypto'
import { existsSync } from 'node:fs'
import { writeFile, rename, unlink } from 'node:fs/promises'
import { join } from 'node:path'
import { actionControlSchema, type ActionControl } from '@archava/crm-act/contracts'
import { CrmError } from '@archava/crm'
import { home, readPrivateJson, writePrivateJson, type RuntimeSettings } from './settings.js'

export const NATIVE_SERVICE_ID = 'archava-followup'
export const nativeControlPath = (settings: RuntimeSettings) =>
  join(home, 'native-actions-' + settings.database + '.json')
export const nativeConfirmationKey = (settings: RuntimeSettings) =>
  new Uint8Array(
    createHmac('sha256', settings.fingerprintKey)
      .update('archava/native-crm/confirmation-key/v1')
      .digest(),
  )
export async function readNativeControl(settings: RuntimeSettings): Promise<ActionControl> {
  try {
    return actionControlSchema.parse(await readPrivateJson(nativeControlPath(settings)))
  } catch {
    throw new CrmError('unavailable', 'Native action control could not be verified.')
  }
}
export async function initializeNativeControl(settings: RuntimeSettings) {
  const path = nativeControlPath(settings)
  if (!existsSync(path)) await writePrivateJson(path, { epoch: randomUUID(), held: false })
  return readNativeControl(settings)
}
export async function writeNativeControl(settings: RuntimeSettings, held: boolean, rotate = false) {
  const path = nativeControlPath(settings)
  const old = existsSync(path)
    ? await readNativeControl(settings)
    : { epoch: randomUUID(), held: true }
  const value = { epoch: rotate ? randomUUID() : old.epoch, held }
  const temporary = path + '.' + randomUUID() + '.tmp'
  try {
    await writeFile(temporary, JSON.stringify(value) + '\n', { mode: 0o600, flag: 'wx' })
    await rename(temporary, path)
  } finally {
    await unlink(temporary).catch(() => {})
  }
  return value
}
