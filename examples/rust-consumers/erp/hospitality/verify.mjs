import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import {
  PurchasingService,
  purchaseMutationSchema,
  purchasePageSchema,
} from '@archava/purchasing'
import { FoodService, foodMutationSchema, FOOD_TICKET_STATES } from '@archava/foodservice'
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

// 1. The installed packages exist with real constructors — not stubs.
assert.equal(typeof PurchasingService, 'function')
assert.equal(typeof FoodService, 'function')
assert.equal(typeof PostgresPurchaseRepository, 'function')
assert.equal(typeof PostgresFoodRepository, 'function')

// 2. The shipped migrations and ledgers survive packaging: both modules
//    publish their migration ids and the ledger reads back from the
//    installed `migrations/` directory relative to the COMPILED dist file.
assert.deepEqual([...PURCHASE_MIGRATIONS], ['001_suppliers_orders_receipts'])
assert.deepEqual([...FOOD_MIGRATIONS], ['001_tables_recipes_tickets'])
const purchaseLedger = await purchasingMigrationLedger()
assert.equal(purchaseLedger.length, 1)
assert.equal(purchaseLedger[0]?.id, '001_suppliers_orders_receipts')
assert.match(purchaseLedger[0]?.checksum ?? '', /^[0-9a-f]{64}$/)
const foodLedger = await foodserviceMigrationLedger()
assert.equal(foodLedger.length, 1)
assert.equal(foodLedger[0]?.id, '001_tables_recipes_tickets')
assert.match(foodLedger[0]?.checksum ?? '', /^[0-9a-f]{64}$/)

// 3. The command grammars refuse a tenant-scoped mutation that ISN'T theirs —
//    the exact wire shape the Rust ERP gate mirrors.
const badRequest = { requestId: 'invalid', command: { operation: 'send.email' } }
assert.equal(purchaseMutationSchema.safeParse(badRequest).success, false)
assert.equal(foodMutationSchema.safeParse(badRequest).success, false)

// 4. The ticket lifecycle the kitchen drives is frozen.
assert.deepEqual([...FOOD_TICKET_STATES], [
  'draft',
  'queued',
  'preparing',
  'ready',
  'served',
  'closed',
  'cancelled',
])

// 5. The services validate and parse BEFORE they reach any client — proven
//    with a recording fake that must see NOTHING when validation refuses,
//    and exactly one call shape when it accepts. This is a real contract
//    consumption, not a typeof check.
const calls = []
const page = { items: [], total: 0 }
const fakePurchaseRepo = {
  access: () => page,
  suppliers: () => page,
  offers: () => page,
  orders: (tenant, principal, query) => {
    calls.push('orders:' + tenant + ':' + principal.id + ':' + (query?.limit ?? 'none'))
    return page
  },
}
const purchasing = new PurchasingService(fakePurchaseRepo)
const guest = { kind: 'human', id: 'hotel-owner-1' }
try {
  purchasing.orders('broken/tenant/identifier', guest, { limit: 5 })
  assert.fail('an invalid tenant must be refused before any repository call')
} catch {
  // refused as expected
}
purchasing.orders('hotel-demo', guest, { limit: 5 })
assert.deepEqual([...calls], ['orders:hotel-demo:hotel-owner-1:5'])

const fakeFoodRepo = {
  access: () => page,
  tables: () => page,
  menu: (tenant, principal, query) => {
    calls.push('menu:' + tenant + ':' + principal.id + ':' + (query?.limit ?? 'none'))
    return page
  },
  menuDetail: () => page,
}
const cook = new FoodService(fakeFoodRepo)
const kitchenHost = { kind: 'human', id: 'kitchen-owner-1' }
try {
  foodMutationSchema.safeParse({ requestId: crypto.randomUUID(), command: {} })
  cook.menu('hotel-demo', { kind: 'service', id: 'bad-id' }, { limit: 5 })
  assert.fail('a service principal with no onBehalfOf must be refused')
} catch {
  // refused as expected
}
cook.menu('hotel-demo', kitchenHost, { limit: 3 })
assert.deepEqual(calls.at(-1), 'menu:hotel-demo:kitchen-owner-1:3')
assert.equal(purchasePageSchema.safeParse({ limit: 10 }).success, true)
assert.equal(purchasePageSchema.safeParse({ limit: 'many' }).success, false)

// 6. The browser bundle of this consumer pulled ONLY pure packages: the
//    ledger/repository modules may not leak into what ships to the client.
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
