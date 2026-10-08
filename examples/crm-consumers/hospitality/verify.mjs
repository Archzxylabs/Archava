import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { CrmService, CRM_CONTACTS_MANIFEST, mutationSchema } from '@archava/crm'
import { CrmPipelineService, CRM_PIPELINE_MANIFEST } from '@archava/crm/pipeline'
import { PostgresCrmRepository, migrateCrm } from '@archava/crm-pg'
assert.equal(typeof CrmService, 'function')
assert.equal(typeof PostgresCrmRepository.create, 'function')
assert.equal(typeof migrateCrm, 'function')
assert.equal(CRM_CONTACTS_MANIFEST.id, 'crm_contacts')
assert.throws(() =>
  mutationSchema.parse({ requestId: 'invalid', command: { operation: 'send.email' } }),
)
const meta = JSON.parse(await readFile('dist/metafile.json', 'utf8'))
assert.ok(
  Object.keys(meta.inputs).every(
    (file) => !file.includes('/packages/') && !file.includes('crm-pg'),
  ),
)
assert.ok(
  Object.values(meta.outputs).every((file) =>
    file.imports.every((item) => !item.path.startsWith('node:')),
  ),
)
console.log('PASS independent CRM package declarations, browser build and public contracts')

assert.equal(typeof CrmPipelineService, 'function')
assert.equal(CRM_PIPELINE_MANIFEST.id, 'crm_sales_pipeline')
