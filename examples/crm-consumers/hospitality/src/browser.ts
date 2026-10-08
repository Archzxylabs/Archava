import { mountCrmDashboard } from '@archava/crm/dashboard'
import config from '../client.json'
const host = document.querySelector<HTMLElement>('#crm')
if (!host) throw new Error('CRM host missing')
let dashboard: ReturnType<typeof mountCrmDashboard> | null = null
async function signIn(principal: string) {
  const response = await fetch('/reference-session?principal=' + encodeURIComponent(principal), {
    method: 'POST',
  })
  if (!response.ok) throw new Error('Reference session refused')
  dashboard?.destroy()
  dashboard = mountCrmDashboard(host as HTMLElement, {
    apiBase: '/api/crm/' + config.tenantId,
    labels: { title: config.title, contacts: config.contacts, contact: config.contact },
  })
  await dashboard.ready
}
for (const principal of [config.principal, config.viewer].filter(
  (value): value is string => value !== null,
)) {
  const button = document.createElement('button')
  button.textContent = 'Use reference ' + principal
  button.dataset['crmSignin'] = principal
  button.addEventListener('click', () => {
    void signIn(principal)
  })
  document.querySelector('#sessions')?.append(button)
}
