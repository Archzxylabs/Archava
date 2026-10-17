import { mountPurchasingDashboard } from '@archava/purchasing/dashboard'
import { mountFoodDashboard } from '@archava/foodservice/dashboard'
import config from '../client.json'

const purchHost = document.querySelector<HTMLElement>('#purchasing')
if (!purchHost) throw new Error('Purchasing host missing')
const foodHost = document.querySelector<HTMLElement>('#food')
if (!foodHost) throw new Error('Food host missing')

mountPurchasingDashboard(purchHost, {
  // . The host supplies the real pool and the session; these base paths name
  // the two ERP ledgers the dashboard drives.
  apiBase: '/api/purchasing/' + config.tenantId,
  commerceApiBase: '/api/commerce/' + config.tenantId,
})
mountFoodDashboard(foodHost, {
  apiBase: '/api/foodservice/' + config.tenantId,
  commerceApiBase: '/api/commerce/' + config.tenantId,
})
