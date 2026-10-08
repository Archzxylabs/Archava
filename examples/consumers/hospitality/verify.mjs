import assert from 'node:assert/strict'
import { config, ask, allowedActions } from './dist/main.js'
import { seedContextGraph, TenantScopeError } from '@archava/core'
import { formatMoney } from '@archava/chat'
import {
  parseClientConfig,
  buildDeploymentPlan,
  UnknownCurrencyError as PureCurrencyError,
} from '@archava/config/pure'
import { UnknownCurrencyError } from '@archava/config'
import { ClientConfigResolver } from '@archava/config/client'
import { TemplateRegistry } from '@archava/config/templates'
import { parsePricebook } from '@archava/config/pricing'
import { readFile } from 'node:fs/promises'

assert.equal(seedContextGraph(config).tenantId, config.tenantId)
const result = await ask()
assert.equal(result.tenantId, config.tenantId)
assert.equal(result.basis, 'retrieval')
assert.ok(result.text.includes(config.knowledgeSources[0].content))
assert.deepEqual(result.permittedActionIds, allowedActions)
assert.ok(result.actions.every((item) => item.policy !== 'allowed'))
assert.equal(result.actions.length, 1)
assert.ok(result.citations.every((item) => item.sourceId.startsWith(config.tenantId)))
await assert.rejects(() => ask('other-tenant'), TenantScopeError)
assert.equal(parseClientConfig({ ...config, integrations: [] }).integrations.length, 0)
assert.equal(UnknownCurrencyError, PureCurrencyError)
assert.throws(() => formatMoney(100, 'INVALID', 'en'), UnknownCurrencyError)
assert.equal(typeof ClientConfigResolver, 'function')
assert.equal(typeof TemplateRegistry, 'function')
assert.equal(typeof parsePricebook, 'function')
assert.equal(typeof buildDeploymentPlan, 'function')
assert.ok(formatMoney(12300, 'IDR', 'id-ID').includes('12.300'))
const metafile = JSON.parse(await readFile('dist/browser-metafile.json', 'utf8'))
assert.ok(
  Object.keys(metafile.inputs).every(
    (file) => !file.includes('/packages/') && !file.includes('src/client/resolver'),
  ),
)
assert.ok(
  Object.values(metafile.outputs).every((file) =>
    file.imports.every((item) => !item.path.startsWith('node:')),
  ),
)
console.log(
  JSON.stringify({
    tenantId: config.tenantId,
    brand: config.branding.businessName,
    allowedActions,
    text: result.text,
    crossTenant: 'denied',
    unsafeAction: 'denied',
    browser: 'built-without-alias',
  }),
)
