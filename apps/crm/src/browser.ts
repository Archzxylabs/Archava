import { mountCrmDashboard } from '@archava/crm/dashboard'
import { mountCrmConversation } from '@archava/crm-assistant/panel'
import { z } from 'zod'

const sessionSchema = z.discriminatedUnion('authenticated', [
  z.object({ authenticated: z.literal(false) }).strict(),
  z
    .object({
      authenticated: z.literal(true),
      username: z.string(),
      csrf: z.string(),
      tenants: z.array(
        z
          .object({ id: z.string(), title: z.string(), contacts: z.string(), contact: z.string() })
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
let conversation: ReturnType<typeof mountCrmConversation> | null = null
let session: z.infer<typeof sessionSchema> = { authenticated: false }
function signedOut() {
  dashboard?.destroy()
  dashboard = null
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
  dashboard?.destroy()
  conversation?.destroy()
  conversation = null
  if (!selected) {
    host.textContent = 'No workspace access. Contact your administrator.'
    return
  }
  dashboard = mountCrmDashboard(host, {
    apiBase: '/api/crm/' + selected.id,
    labels: selected,
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
  conversation = mountCrmConversation(element<HTMLElement>('#conversation'), {
    apiBase: '/api/crm/' + selected.id + '/conversation',
    transport: async (input, init) => {
      const headers = new Headers(init?.headers)
      headers.set('x-archava-csrf', csrf)
      const response = await fetch(input, { ...init, headers })
      if (response.status === 401) signedOut()
      return response
    },
  })
  await dashboard.ready
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
