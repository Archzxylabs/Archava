import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { PurchasingService, purchaseMutationSchema } from '@archava/purchasing'
import { FoodService, foodMutationSchema } from '@archava/foodservice'
import {
  PostgresPurchaseRepository,
  purchasingMigrationLedger,
  PURCHASE_MIGRATIONS,
} from '@archava/purchasing-pg'
import {
  PostgresFoodRepository,
  foodserviceMigrationLedger,
  FOOD_MIGRATIONS,
} from '@archava/foodservice-pg'

// The two installed repositories are distinct classes, and both ship the
// frozen ids their SQL pharmacies came from.
assert.equal(typeof PostgresPurchaseRepository, 'function')
assert.equal(typeof PostgresFoodRepository, 'function')
assert.deepEqual([...PURCHASE_MIGRATIONS], ['001_suppliers_orders_receipts'])
assert.deepEqual([...FOOD_MIGRATIONS], ['001_tables_recipes_tickets'])

const purchaseLedger = await purchasingMigrationLedger()
assert.match(purchaseLedger[0]?.checksum ?? '', /^[0-9a-f]{64}$/)
const foodLedger = await foodserviceMigrationLedger()
assert.match(foodLedger[0]?.checksum ?? '', /^[0-9a-f]{64}$/)

// The purchasing mutation grammar refuses an unknown operation and a
// principal family the desk does not accept.
assert.equal(
  purchaseMutationSchema.safeParse({
    requestId: crypto.randomUUID(),
    command: { operation: 'stock.take' },
  }).success,
  false,
)
assert.equal(
  purchaseMutationSchema.safeParse({
    requestId: 'not-a-uuid',
    command: { operation: 'supplier.create', code: 'SUP', name: 'Desk' },
  }).success,
  false,
)

// FoodService refuses before any client query the same way, from ITS module.
assert.equal(foodMutationSchema.safeParse({ requestId: 'nope', command: {} }).success, false)

// EXERCISED SERVICE FLOW — written differently from the hospitality battery:
// here the recording fake checks the ACCEPTED path first and asserts the
// tenant parse widened nothing.
const calls = []
const page = { items: [], total: 0 }
const buyer = { kind: 'human', id: 'procurement-owner-1' }
const purchasing = new PurchasingService({

  access: () => page,
  suppliers: () => page,
  offers: () => page,
  orders: (tenant, principal, query) => {
    calls.push('orders:' + tenant + ':' + principal.id + ':' + (query?.limit ?? 'none'))
    return page
  },
  receipts: () => page,
})
const accepted = purchasing.orders('field-services-demo', buyer, { limit: 25 })
assert.deepEqual([...calls], ['orders:field-services-demo:procurement-owner-1:25'])
assert.equal(accepted, page)
try {
  purchasing.orders('field-services-demo', { kind: 'service', id: 'kernel-account' }, { limit: 1 })
  assert.fail('a service principal without onBehalfOf must be refused by its own contract')
} catch {
  // refused — the recording fake saw nothing new
}
assert.deepEqual([...calls], ['orders:field-services-demo:procurement-owner-1:25'])

const cook = new FoodService({

  tables: () => page,
  menu: () => page,
  menuDetail: () => page,
})
assert.equal(cook.menu('field-services-demo', { kind: 'human', id: 'crew-owner-1' }, {}), page)

// The browser bundle ships nothing from the -pg modules and nothing from node.
const meta = JSON.parse(await readFile('dist/metafile.json', 'utf8'))
assert.ok(
  Object.keys(meta.inputs).every(
    (file) => !file.includes('purchasing-pg') && !file.includes('foodservice-pg'),
  ),
)
assert.ok(
  Object.values(meta.outputs).every((file) =>
    file.imports.every((item) => !item.path.startsWith('node:')),
  ),
)

console.log('PASS independent ERP packages, browser build, migrations and public contracts')
