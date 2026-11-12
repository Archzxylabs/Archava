/** Current business boundaries, read from the module registry. This inventory does not grant product acceptance. */
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const registry = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../config/modules.v1.json'), 'utf8'))

export const modules = registry.modules.map(({ id, label, source, suite, readPath, uiPath, control }) => ({ id, label, domain: source, suite, readPath, uiPath, control }))
