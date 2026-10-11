import { mountCrmDashboard } from '@archava/crm/dashboard'
import { mountCrmWorklist } from '@archava/crm/worklist-dashboard'
import { mountCrmOverview } from '@archava/crm/overview-dashboard'
import { mountPipelineDashboard } from '@archava/crm/pipeline-dashboard'
import { pipelineLabelsSchema } from '@archava/crm/pipeline-contracts'
import { mountCrmConversation } from '@archava/crm-assistant/panel'
import { createTabFollowupRecovery } from '@archava/crm-assistant/recovery'
import { z } from 'zod'
import { moduleSelectionSchema, FULL_MODULES } from '@archava/config/assembly'
import { mountProjectDashboard } from '@archava/projects/dashboard'
import { mountSchedulingDashboard } from '@archava/scheduling/dashboard'
import { mountAccommodationDashboard } from '@archava/accommodation/dashboard'
import { mountSupportDashboard } from '@archava/support/dashboard'
import { mountFoodDashboard } from '@archava/foodservice/dashboard'
import { mountCommerceDashboard } from '@archava/commerce/dashboard'

const sessionSchema = z.discriminatedUnion('authenticated', [
  z.object({ authenticated: z.literal(false) }).strict(),
  z
    .object({
      authenticated: z.literal(true),
      username: z.string(),
      csrf: z.string(),
      plannerMode: z.enum(['local', 'gemini-synthetic']).optional(),
      nativeActions: z.boolean().optional(),
      modules: moduleSelectionSchema.optional(),
      tenants: z.array(
        z
          .object({
            id: z.string(),
            title: z.string(),
            contacts: z.string(),
            contact: z.string(),
            pipelineLabels: pipelineLabelsSchema.optional(),
          })
          .strict(),
      ),
    })
    .strict(),
])
const element = <T extends HTMLElement>(selector: string) => {
  const value = document.querySelector<T>(selector)
  if (!value) throw new Error('Missing application element')
  return value
}
const login = element<HTMLFormElement>('#login')
const password = element<HTMLFormElement>('#password')
const workspace = element<HTMLElement>('#workspace')
const host = element<HTMLElement>('#crm')
const tenants = element<HTMLSelectElement>('#tenants')
const status = element<HTMLElement>('#status')
let csrf = ''
let dashboard: ReturnType<typeof mountCrmDashboard> | null = null
let worklist: ReturnType<typeof mountCrmWorklist> | null = null
let overview: ReturnType<typeof mountCrmOverview> | null = null
let pipeline: ReturnType<typeof mountPipelineDashboard> | null = null
let conversation: ReturnType<typeof mountCrmConversation> | null = null
let projects: ReturnType<typeof mountProjectDashboard> | null = null
let scheduling: ReturnType<typeof mountSchedulingDashboard> | null = null
let accommodation: ReturnType<typeof mountAccommodationDashboard> | null = null
let support: ReturnType<typeof mountSupportDashboard> | null = null
let foodservice: ReturnType<typeof mountFoodDashboard> | null = null
let commerce: ReturnType<typeof mountCommerceDashboard> | null = null
let session: z.infer<typeof sessionSchema> = { authenticated: false }
function signedOut() {
  support?.destroy()
  support = null
  foodservice?.destroy()
  foodservice = null
  commerce?.destroy()
  commerce = null
  accommodation?.destroy()
  accommodation = null
  scheduling?.destroy()
  scheduling = null
  projects?.destroy()
  projects = null
  overview?.destroy()
  overview = null
  worklist?.destroy()
  worklist = null
  dashboard?.destroy()
  dashboard = null
  pipeline?.destroy()
  pipeline = null
  conversation?.destroy()
  conversation = null
  csrf = ''
  session = { authenticated: false }
  workspace.hidden = true
  login.hidden = false
  password.reset()
}
async function send(path: string, body?: unknown) {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...(body === undefined
      ? {}
      : {
          method: 'POST',
          headers: {
            'content-type': 'application/json',
            ...(csrf ? { 'x-archava-csrf': csrf } : {}),
          },
          body: JSON.stringify(body),
        }),
  })
  if (!response.ok) {
    if (response.status === 401 && path !== '/api/login') signedOut()
    throw new Error(
      response.status === 429
        ? 'Too many attempts. Try again later.'
        : response.status === 401
          ? 'Sign-in details could not be verified.'
          : 'Request could not be completed.',
    )
  }
  return (await response.json()) as unknown
}
async function mount() {
  if (!session.authenticated) return
  const selected = session.tenants.find((item) => item.id === tenants.value)
  support?.destroy()
  support = null
  foodservice?.destroy()
  foodservice = null
  commerce?.destroy()
  commerce = null
  accommodation?.destroy()
  accommodation = null
  scheduling?.destroy()
  scheduling = null
  projects?.destroy()
  projects = null
  overview?.destroy()
  overview = null
  worklist?.destroy()
  worklist = null
  dashboard?.destroy()
  pipeline?.destroy()
  pipeline = null
  conversation?.destroy()
  conversation = null
  if (!selected) {
    host.textContent = 'No workspace access. Contact your administrator.'
    return
  }
  const modules = session.modules ?? FULL_MODULES
  for (const [id, enabled] of [
    ['pipeline', modules.pipeline],
    ['conversation', modules.assistant],
    ['overview', modules.pipeline],
    ['projects', modules.projects ?? false],
    ['scheduling', modules.scheduling ?? false],
    ['accommodation', modules.accommodation ?? false],
    ['commerce', modules.commerce ?? false],
    ['foodservice', modules.foodservice ?? false],
    ['support', modules.support ?? false],
  ] as const) {
    const node = element<HTMLElement>('#' + id)
    node.hidden = !enabled
    if (!enabled) node.replaceChildren()
  }
  dashboard = mountCrmDashboard(host, {
    apiBase: '/api/crm/' + selected.id,
    labels: selected,
    onContactSelected: (contact) => pipeline?.selectContact(contact),
    transport: async (input, init) => {
      const headers = new Headers(init?.headers)
      if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
      const response = await fetch(input, { ...init, headers })
      if (response.status === 401) {
        signedOut()
        status.textContent = 'Your session ended. Sign in again.'
      }
      return response
    },
  })
  if (modules.projects)
    projects = mountProjectDashboard(element<HTMLElement>('#projects'), {
      apiBase: '/api/projects/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.support)
    support = mountSupportDashboard(element<HTMLElement>('#support'), {
      apiBase: '/api/support/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.foodservice)
    foodservice = mountFoodDashboard(element<HTMLElement>('#foodservice'), {
      apiBase: '/api/foodservice/' + selected.id,
      commerceApiBase: '/api/commerce/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.commerce)
    commerce = mountCommerceDashboard(element<HTMLElement>('#commerce'), {
      apiBase: '/api/commerce/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.accommodation)
    accommodation = mountAccommodationDashboard(element<HTMLElement>('#accommodation'), {
      apiBase: '/api/accommodation/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.scheduling)
    scheduling = mountSchedulingDashboard(element<HTMLElement>('#scheduling'), {
      apiBase: '/api/scheduling/' + selected.id,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.pipeline)
    pipeline = mountPipelineDashboard(element<HTMLElement>('#pipeline'), {
      apiBase: '/api/crm/' + selected.id,
      labels: selected.pipelineLabels,
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        if (init?.method === 'POST') headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        return response
      },
    })
  if (modules.assistant)
    conversation = mountCrmConversation(element<HTMLElement>('#conversation'), {
      apiBase: '/api/crm/' + selected.id + '/conversation',
      commandsBase: '/api/crm/' + selected.id + '/commands',
      approvalBase: session.nativeActions
        ? '/api/crm/' + selected.id + '/actions/followup'
        : undefined,
      pilotEnabled: session.plannerMode === 'gemini-synthetic',
      pipelineLabels: selected.pipelineLabels,
      onFollowupCompleted: () => {
        void worklist?.refresh()
        void overview?.refresh()
      },
      recovery: createTabFollowupRecovery(
        {
          getItem: (key) => sessionStorage.getItem(key),
          setItem: (key, value) => sessionStorage.setItem(key, value),
          removeItem: (key) => sessionStorage.removeItem(key),
        },
        JSON.stringify([session.username, selected.id]),
      ),
      transport: async (input, init) => {
        const headers = new Headers(init?.headers)
        headers.set('x-archava-csrf', csrf)
        const response = await fetch(input, { ...init, headers })
        if (response.status === 401) signedOut()
        if (response.status === 429)
          throw new Error(
            'Batas permintaan AI tercapai. Coba lagi nanti atau gunakan perintah lokal.',
          )
        return response
      },
    })
  const currentDashboard = dashboard
  const currentConversation = conversation
  worklist = mountCrmWorklist(element<HTMLElement>('#worklist'), {
    apiBase: '/api/crm/' + selected.id,
    onContactSelected: (id) => currentDashboard.selectContact(id),
    onFollowupSelected: session.nativeActions
      ? (id) => currentConversation?.selectFollowup(id)
      : undefined,
    transport: async (input, init) => {
      const response = await fetch(input, init)
      if (response.status === 401) signedOut()
      return response
    },
  })
  if (modules.pipeline)
    overview = mountCrmOverview(element<HTMLElement>('#overview'), {
      apiBase: '/api/crm/' + selected.id,
      tenantId: selected.id,
      pipelineLabels: selected.pipelineLabels,
      transport: async (input, init) => {
        const response = await fetch(input, init)
        if (response.status === 401) signedOut()
        return response
      },
    })
  await Promise.all([dashboard.ready, pipeline?.ready, worklist.ready, overview?.ready])
}
async function refreshSession() {
  session = sessionSchema.parse(await send('/api/session'))
  if (!session.authenticated) {
    signedOut()
    return
  }
  csrf = session.csrf
  element<HTMLElement>('#account').textContent = session.username
  element<HTMLInputElement>('#password [name=username]').value = session.username
  login.hidden = true
  workspace.hidden = false
  tenants.replaceChildren()
  for (const tenant of session.tenants) {
    const option = document.createElement('option')
    option.value = tenant.id
    option.textContent = tenant.title
    tenants.append(option)
  }
  await mount()
}
async function perform(operation: () => Promise<void>) {
  status.textContent = 'Working…'
  try {
    await operation()
    status.textContent = ''
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : 'Request could not be completed.'
  }
}
login.addEventListener('submit', (event) => {
  event.preventDefault()
  void perform(async () => {
    const data = new FormData(login)
    await send('/api/login', { username: data.get('username'), password: data.get('password') })
    login.reset()
    await refreshSession()
  })
})
tenants.addEventListener('change', () => {
  void perform(mount)
})
element<HTMLButtonElement>('#logout').addEventListener('click', () => {
  void perform(async () => {
    await send('/api/logout', {})
    signedOut()
  })
})
password.addEventListener('submit', (event) => {
  event.preventDefault()
  void perform(async () => {
    const data = new FormData(password)
    await send('/api/password', { current: data.get('current'), next: data.get('next') })
    signedOut()
  })
})
void perform(refreshSession)
