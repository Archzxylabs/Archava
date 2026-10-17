import {
  PurchasingService,
  type PurchaseRepository,
  type PurchasePrincipal,
  type PurchasePageQuery,
} from '@archava/purchasing'
import {
  FoodService,
  type FoodRepository,
  type FoodPrincipal,
  type FoodPageQuery,
} from '@archava/foodservice'
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

/** The exact principals the hospitality desk works as; the modules themselves
 * re-validate these shapes on every call their services make. */
export const hotelPrincipal: PurchasePrincipal = { kind: 'human', id: 'hotel-owner-1' }
export const kitchenPrincipal: FoodPrincipal = { kind: 'human', id: 'kitchen-owner-1' }

export const createPurchaseService = (repository: PurchaseRepository) =>
  new PurchasingService(repository)
export const createFoodService = (repository: FoodRepository) => new FoodService(repository)

/** The page queries the battery feeds through each module's own parse layer. */
export const pageQ: PurchasePageQuery = { limit: 10 }
export const foodPage: FoodPageQuery = { limit: 5 }

/** Frozen names the installed packages must still carry, read as values so
 * typecheck refuses broken exports without running anything. */
export const purchaseLedger = { ids: PURCHASE_MIGRATIONS, read: purchasingMigrationLedger }
export const foodLedger = { ids: FOOD_MIGRATIONS, read: foodserviceMigrationLedger }
export const purchaseRepoClass = PostgresPurchaseRepository
export const foodRepoClass = PostgresFoodRepository
