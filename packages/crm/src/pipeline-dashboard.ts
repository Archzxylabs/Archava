import type { Contact, CrmAccess } from './contracts.js'
import {
  opportunitySchema,
  transitionSchema,
  pipelineLabelsSchema,
  DEFAULT_PIPELINE_LABELS,
  PIPELINE_STAGES,
  PIPELINE_TRANSITIONS,
  type Opportunity,
  type OpportunityCommand,
  type OpportunityDetail,
  type OpportunityPage,
} from './pipeline-contracts.js'

/** One reusable human-operated pipeline. Session/CSRF transport belongs to its host. */
export function mountPipelineDashboard(
  host: HTMLElement,
  options: {
    apiBase: string
    title?: string
    labels?: unknown
    transport?: typeof fetch
  },
) {
  const document = host.ownerDocument
  const labels = pipelineLabelsSchema.parse(options.labels ?? DEFAULT_PIPELINE_LABELS)
  const send = options.transport ?? fetch
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const item = document.createElement(tag)
    item.textContent = text
    return item
  }
  let disposed = false
  let busy = false
  let canWrite = false
  let selectedContact: Contact | null = null
  let selectedOpportunity: Opportunity | null = null
  let after: string | undefined
  let active: AbortController | null = null
  let pending: { body: string; requestId: string } | null = null
  const message = node('p')
  message.setAttribute('role', 'status')
  message.dataset.pipelineStatus = ''
  const selector = node('select')
  selector.setAttribute('aria-label', 'Opportunity stage')
  selector.dataset.pipelineFilter = ''
  const all = node('option', 'All stages')
  all.value = ''
  selector.append(all)
  for (const stage of PIPELINE_STAGES) {
    const option = node('option', labels[stage])
    option.value = stage
    selector.append(option)
  }
  const list = node('ul')
  list.dataset.pipelineList = ''
  const next = node('button', 'Next opportunities')
  next.type = 'button'
  next.hidden = true
  const refresh = node('button', 'Refresh opportunities')
  refresh.type = 'button'
  const contactLabel = node('p', 'Select a contact above to create an opportunity.')
  contactLabel.dataset.pipelineContact = ''
  const form = node('form')
  form.dataset.pipelineCreate = ''
  form.hidden = true
  const title = node('input')
  title.name = 'title'
  title.required = true
  title.maxLength = 160
  title.setAttribute('aria-label', 'Opportunity title')
  title.placeholder = 'Opportunity title'
  const create = node('button', 'Create opportunity')
  create.type = 'submit'
  create.disabled = true
  form.append(contactLabel, title, create)
  const detail = node('section')
  detail.dataset.pipelineDetail = ''
  host.replaceChildren(
    node('h2', options.title ?? 'Sales pipeline'),
    node('p', 'Won records a sales outcome. Booking and payment keep their own status.'),
    message,
    selector,
    refresh,
    form,
    list,
    next,
    detail,
  )
  async function request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await send(options.apiBase + path, {
      credentials: 'same-origin',
      signal: active?.signal,
      ...init,
    })
    const payload: unknown = await response.json()
    if (!response.ok) {
      const error = payload as { error?: { message?: string } }
      throw new Error(error.error?.message ?? 'Pipeline request failed.')
    }
    return payload as T
  }
  async function mutate(command: OpportunityCommand) {
    const body = JSON.stringify(command)
    if (!pending || pending.body !== body) pending = { body, requestId: crypto.randomUUID() }
    const receipt = await request<{ id: string }>('/commands', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ requestId: pending.requestId, command }),
    })
    pending = null
    return receipt
  }
  async function show(id: string) {
    const response = await request<OpportunityDetail>('/opportunities/' + encodeURIComponent(id))
    if (disposed) return
    const record = opportunitySchema.parse(response.opportunity)
    const history = response.transitions.map((item) => transitionSchema.parse(item))
    selectedOpportunity = record
    detail.replaceChildren(node('h3', record.title), node('p', 'Stage: ' + labels[record.stage]))
    if (record.lossReason) detail.append(node('p', 'Loss reason: ' + record.lossReason))
    if (canWrite && PIPELINE_TRANSITIONS[record.stage].length) {
      const move = node('form')
      move.dataset.pipelineMove = ''
      const target = node('select')
      target.name = 'stage'
      target.setAttribute('aria-label', 'Move opportunity to stage')
      for (const stage of PIPELINE_TRANSITIONS[record.stage]) {
        const option = node('option', labels[stage])
        option.value = stage
        target.append(option)
      }
      const reason = node('textarea')
      reason.name = 'reason'
      reason.maxLength = 1000
      reason.placeholder = 'Reason (required for Lost)'
      reason.setAttribute('aria-label', 'Stage transition reason')
      const moveButton = node('button', 'Move stage')
      moveButton.type = 'submit'
      move.append(target, reason, moveButton)
      target.addEventListener('change', () => {
        reason.required = target.value === 'lost'
      })
      reason.required = target.value === 'lost'
      move.addEventListener('submit', (event) => {
        event.preventDefault()
        void perform(async () => {
          const stage = PIPELINE_STAGES.find((stage) => stage === target.value)
          if (!stage || !selectedOpportunity) return
          const result = await mutate({
            operation: 'opportunity.move',
            id: record.id,
            expectedRevision: record.revision,
            stage,
            ...(reason.value.trim() ? { reason: reason.value.trim() } : {}),
          })
          await load()
          await show(result.id)
        })
      })
      detail.append(move)
    }
    const timeline = node('ol')
    for (const item of history)
      timeline.append(
        node(
          'li',
          `${item.occurredAt} · ${item.from ? labels[item.from] : 'Created'} → ${labels[item.to]}${item.reason ? ' · ' + item.reason : ''}`,
        ),
      )
    detail.append(node('h4', 'Latest 20 stage changes'), timeline)
  }
  async function load() {
    const params = new URLSearchParams({ limit: '20' })
    if (selector.value) params.set('stage', selector.value)
    if (after) params.set('after', after)
    const result = await request<OpportunityPage>('/opportunities?' + params.toString())
    if (disposed) return
    list.replaceChildren()
    for (const item of result.items) {
      const record = opportunitySchema.parse(item)
      const row = node('li')
      const button = node('button', record.title + ' · ' + labels[record.stage])
      button.type = 'button'
      button.dataset.pipelineOpportunity = record.id
      button.addEventListener('click', () => {
        void perform(() => show(record.id))
      })
      row.append(button)
      list.append(row)
    }
    next.hidden = result.next === null
    next.dataset.after = result.next ?? ''
  }
  async function perform(work: () => Promise<void>) {
    if (busy || disposed) return
    busy = true
    active = new AbortController()
    message.textContent = 'Working…'
    create.disabled = true
    try {
      await work()
      if (!disposed) message.textContent = 'Ready.'
    } catch (error) {
      if (!disposed)
        message.textContent = error instanceof Error ? error.message : 'Pipeline unavailable.'
    } finally {
      busy = false
      active = null
      if (!disposed) create.disabled = !canWrite || !selectedContact
    }
  }
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    void perform(async () => {
      if (!selectedContact) return
      const receipt = await mutate({
        operation: 'opportunity.create',
        contactId: selectedContact.id,
        contactRevision: selectedContact.revision,
        title: title.value.trim(),
      })
      title.value = ''
      after = undefined
      await load()
      await show(receipt.id)
    })
  })
  selector.addEventListener('change', () => {
    void perform(async () => {
      after = undefined
      await load()
    })
  })
  refresh.addEventListener('click', () => {
    void perform(async () => {
      after = undefined
      await load()
      if (selectedOpportunity) await show(selectedOpportunity.id)
    })
  })
  next.addEventListener('click', () => {
    void perform(async () => {
      after = next.dataset.after
      await load()
    })
  })
  const ready = perform(async () => {
    const access = await request<CrmAccess>('/access')
    if (disposed) return
    if (
      !access.permissions.includes('pipeline.read') ||
      !access.permissions.includes('contacts.read')
    ) {
      host.replaceChildren()
      return
    }
    canWrite = access.permissions.includes('pipeline.write')
    form.hidden = !canWrite
    await load()
    message.textContent = ''
  })
  return {
    ready,
    selectContact(contact: Contact) {
      if (disposed) return
      selectedContact = contact
      contactLabel.textContent = 'Contact: ' + contact.fullName
      create.disabled = !canWrite || busy
    },
    destroy() {
      disposed = true
      active?.abort()
      selectedContact = null
      selectedOpportunity = null
      pending = null
      host.replaceChildren()
    },
  }
}
