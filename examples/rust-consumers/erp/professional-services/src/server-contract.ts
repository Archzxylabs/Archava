import { PurchasingService, type PurchasePrincipal, type PurchaseRepository } from '@archava/purchasing'
import { FoodService, type FoodPrincipal, type FoodRepository } from '@archava/foodservice'
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

/** The procurement-focused principal this vertical works as. */
export const fieldPrincipal: PurchasePrincipal = { kind: 'human', id: 'procurement-owner-1' }
export const crewPrincipal: FoodPrincipal = { kind: 'human', id: 'crew-owner-1' }

export const createPurchaseService = (repository: PurchaseRepository) =>
  new PurchasingService(repository)
export const createFoodService = (repository: FoodRepository) => new FoodService(repository)

/** The ledgers the two -pg modules freeze; the battery checks their identity
 * through the installed packages, not through any in-tree copy. */
export const purchaseLedger = { ids: PURCHASE_MIGRATIONS, read: purchasingMigrationLedger }
export const foodLedger = { ids: FOOD_MIGRATIONS, read: foodserviceMigrationLedger }
export const purchaseRepoClass = PostgresPurchaseRepository
export const foodRepoClass = PostgresFoodRepository
