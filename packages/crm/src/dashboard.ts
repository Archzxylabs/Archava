import type { Contact, ContactDetail, CrmAccess, CrmCommand, ContactPage } from './contracts.js'

export interface CrmDashboardOptions {
  readonly apiBase: string
  readonly labels: { readonly title: string; readonly contacts: string; readonly contact: string }
  /** Same-origin cookie session by default. A host may inject its authenticated transport. */
  readonly transport?: typeof fetch
}

/** One dashboard component, configured by the host. Every write goes through the API. */
export function mountCrmDashboard(host: HTMLElement, options: CrmDashboardOptions) {
  const document = host.ownerDocument
  const send = options.transport ?? fetch
  host.replaceChildren()
  const node = <T extends keyof HTMLElementTagNameMap>(tag: T, text = '') => {
    const element = document.createElement(tag)
    element.textContent = text
    return element
  }
  const message = node('p')
  message.setAttribute('role', 'status')
  message.dataset['crmStatus'] = ''
  const title = node('h1', options.labels.title)
  const search = node('input')
  search.placeholder = 'Search contacts'
  search.setAttribute('aria-label', 'Search contacts')
  const refresh = node('button', 'Search')
  refresh.type = 'button'
  const list = node('ul')
  list.dataset['crmContacts'] = ''
  const next = node('button', 'Next page')
  next.type = 'button'
  next.hidden = true
  const detail = node('section')
  detail.dataset['crmDetail'] = ''
  const createForm = node('form')
  createForm.dataset['crmCreate'] = ''
  createForm.hidden = true
  const createName = node('input')
  createName.name = 'fullName'
  createName.required = true
  createName.maxLength = 160
  createName.placeholder = options.labels.contact + ' name'
  createName.setAttribute('aria-label', options.labels.contact + ' name')
  const createEmail = node('input')
  createEmail.name = 'email'
  createEmail.type = 'email'
  createEmail.maxLength = 254
  createEmail.placeholder = 'Email (optional)'
  createEmail.setAttribute('aria-label', 'Email')
  const createButton = node('button', 'Add ' + options.labels.contact)
  createButton.type = 'submit'
  createForm.append(createName, createEmail, createButton)
  host.append(
    title,
    message,
    node('h2', options.labels.contacts),
    search,
    refresh,
    createForm,
    list,
    next,
    detail,
  )
  let access: CrmAccess
  let selected: Contact | null = null
  let after: string | undefined
  let busy = false
  let disposed = false
  // Keep the same command identity if its response is lost. Changed input is a new intent.
  let pending: { body: string; requestId: string } | null = null

  async function request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await send(options.apiBase + path, { credentials: 'same-origin', ...init })
    const payload: unknown = await response.json()
    if (!response.ok) {
      const error = payload as { error?: { message?: string } }
      throw new Error(error.error?.message ?? 'CRM request failed.')
    }
    return payload as T
  }
  async function command(value: CrmCommand) {
    const body = JSON.stringify(value)
    if (!pending || pending.body !== body) pending = { body, requestId: crypto.randomUUID() }
    const result = await request<{ id: string; kind: string }>('/commands', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ requestId: pending.requestId, command: value }),
    })
    pending = null
    return result
  }
  async function perform(work: () => Promise<void>) {
    if (busy || disposed) return
    busy = true
    message.textContent = 'Working…'
    try {
      await work()
      message.textContent = 'Saved.'
    } catch (error) {
      message.textContent = error instanceof Error ? error.message : 'CRM unavailable.'
    } finally {
      busy = false
    }
  }
  async function show(id: string) {
    const result = await request<ContactDetail>('/contacts/' + encodeURIComponent(id))
    selected = result.contact
    detail.replaceChildren(
      node('h2', result.contact.fullName),
      node('p', result.contact.email ?? 'No email recorded'),
    )
    if (access.permissions.includes('contacts.write')) {
      const form = node('form')
      form.dataset['crmEdit'] = ''
      const name = node('input')
      name.name = 'fullName'
      name.value = result.contact.fullName
      name.required = true
      name.maxLength = 160
      name.setAttribute('aria-label', 'Edit contact name')
      const save = node('button', 'Save contact')
      save.type = 'submit'
      form.append(name, save)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        void perform(async () => {
          await command({
            operation: 'contact.update',
            id,
            expectedRevision: result.contact.revision,
            data: { fullName: name.value },
          })
          await load()
          await show(id)
        })
      })
      detail.append(form)
    }
    detail.append(node('h3', 'Recent activity (up to 20)'))
    const activities = node('ul')
    for (const activity of result.activities)
      activities.append(node('li', activity.kind + ': ' + activity.summary))
    detail.append(activities)
    if (access.permissions.includes('activities.write')) {
      const form = node('form')
      form.dataset['crmActivity'] = ''
      const input = node('textarea')
      input.name = 'summary'
      input.required = true
      input.maxLength = 2000
      input.setAttribute('aria-label', 'Activity note')
      const save = node('button', 'Add note')
      save.type = 'submit'
      form.append(input, save)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        void perform(async () => {
          await command({
            operation: 'activity.add',
            contactId: id,
            contactRevision: result.contact.revision,
            kind: 'note',
            summary: input.value,
          })
          await show(id)
        })
      })
      detail.append(form)
    }
    detail.append(node('h3', 'Follow-ups (up to 20)'))
    for (const task of result.followups) {
      const row = node('p', `${task.summary} · ${task.dueAt} · ${task.status}`)
      if (task.status === 'open' && access.permissions.includes('followups.write')) {
        const complete = node('button', 'Complete')
        complete.type = 'button'
        complete.dataset['crmComplete'] = task.id
        complete.addEventListener('click', () => {
          void perform(async () => {
            await command({
              operation: 'followup.complete',
              id: task.id,
              expectedRevision: task.revision,
            })
            await show(id)
          })
        })
        row.append(complete)
      }
      detail.append(row)
    }
    if (access.permissions.includes('followups.write')) {
      const form = node('form')
      form.dataset['crmFollowup'] = ''
      const summary = node('input')
      summary.name = 'summary'
      summary.required = true
      summary.maxLength = 2000
      summary.placeholder = 'Follow-up task'
      summary.setAttribute('aria-label', 'Follow-up task')
      const due = node('input')
      due.name = 'dueAt'
      due.type = 'datetime-local'
      due.required = true
      due.setAttribute('aria-label', 'Follow-up due date')
      const save = node('button', 'Add follow-up')
      save.type = 'submit'
      form.append(summary, due, save)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        void perform(async () => {
          await command({
            operation: 'followup.create',
            contactId: id,
            contactRevision: result.contact.revision,
            summary: summary.value,
            dueAt: new Date(due.value).toISOString(),
          })
          await show(id)
        })
      })
      detail.append(form)
    }
  }
  async function load(cursor?: string) {
    const query = new URLSearchParams({ search: search.value, limit: '20' })
    if (cursor) query.set('after', cursor)
    const page = await request<ContactPage>('/contacts?' + query.toString())
    list.replaceChildren()
    for (const contact of page.items) {
      const row = node('li')
      const open = node('button', contact.fullName)
      open.type = 'button'
      open.dataset['crmContact'] = contact.id
      open.addEventListener('click', () => {
        void perform(() => show(contact.id))
      })
      row.append(open)
      list.append(row)
    }
    after = page.next ?? undefined
    next.hidden = !after
  }
  createForm.addEventListener('submit', (event) => {
    event.preventDefault()
    void perform(async () => {
      const receipt = await command({
        operation: 'contact.create',
        data: { fullName: createName.value, email: createEmail.value || null, organization: null },
      })
      createForm.reset()
      await load()
      await show(receipt.id)
    })
  })
  refresh.addEventListener('click', () => {
    void perform(() => load())
  })
  next.addEventListener('click', () => {
    void perform(() => load(after))
  })
  const ready = perform(async () => {
    access = await request<CrmAccess>('/access')
    createForm.hidden = !access.permissions.includes('contacts.write')
    await load()
    message.textContent = ''
  })
  return {
    ready,
    refresh: () =>
      perform(async () => {
        await load()
        if (selected) await show(selected.id)
      }),
    destroy: () => {
      disposed = true
      host.replaceChildren()
    },
  }
}
