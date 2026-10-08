import { mountCrmDashboard } from '@archava/crm/dashboard'
import { mountPipelineDashboard } from '@archava/crm/pipeline-dashboard'
import config from '../client.json'
const host = document.querySelector<HTMLElement>('#crm')
if (!host) throw new Error('CRM host missing')
let dashboard: ReturnType<typeof mountCrmDashboard> | null = null
let pipeline: ReturnType<typeof mountPipelineDashboard> | null = null
async function signIn(principal: string) {
  const response = await fetch('/reference-session?principal=' + encodeURIComponent(principal), {
    method: 'POST',
  })
  if (!response.ok) throw new Error('Reference session refused')
  dashboard?.destroy()
  pipeline?.destroy()
  dashboard = mountCrmDashboard(host as HTMLElement, {
    apiBase: '/api/crm/' + config.tenantId,
    labels: { title: config.title, contacts: config.contacts, contact: config.contact },
    onContactSelected: (contact) => pipeline?.selectContact(contact),
  })
  const pipelineHost = document.querySelector<HTMLElement>('#pipeline')
  if (!pipelineHost) throw new Error('Pipeline host missing')
  pipeline = mountPipelineDashboard(pipelineHost, {
    apiBase: '/api/crm/' + config.tenantId,
    labels: config.pipelineLabels,
  })
  await Promise.all([dashboard.ready, pipeline.ready])
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
