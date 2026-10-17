import { mountFoodDashboard } from '@archava/foodservice/dashboard'
import { mountPurchasingDashboard } from '@archava/purchasing/dashboard'
import config from '../client.json'

const foodHost = document.querySelector<HTMLElement>('#food')
if (!foodHost) throw new Error('Food host missing')
const purchHost = document.querySelector<HTMLElement>('#purchasing')
if (!purchHost) throw new Error('Purchasing host missing')

// This consumer mounts the SAME two dashboards in the OPPOSITE order and
// against its own tenant — the pairing must hold for any vertical, not only
// for the hospitality configuration the other consumer exercises.
mountFoodDashboard(foodHost, {
  apiBase: '/api/foodservice/' + config.tenantId,
  commerceApiBase: '/api/commerce/' + config.tenantId,
})
mountPurchasingDashboard(purchHost, {
  apiBase: '/api/purchasing/' + config.tenantId,
  commerceApiBase: '/api/commerce/' + config.tenantId,
})
