import { z } from 'zod'
import {
  projectSchema,
  projectDetailSchema,
  projectAccessSchema,
  projectReceiptSchema,
  projectCommandStatusSchema,
  projectMutationSchema,
  projectStateSchema,
  PROJECT_TRANSITIONS,
  TASK_TRANSITIONS,
  type ProjectCommand,
  type ProjectAccess,
  type ProjectMutation,
} from './contracts.js'

/** Reusable human-operated delivery UI. The host owns session and CSRF transport. */
export function mountProjectDashboard(
  host: HTMLElement,
  options: { apiBase: string; title?: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument,
    transport = options.transport ?? fetch
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const value = document.createElement(tag)
    value.textContent = text
    return value
  }
  const button = (text: string, work: () => Promise<void>) => {
    const value = node('button', text)
    value.type = 'button'
    value.addEventListener('click', () => {
      void perform(work)
    })
    return value
  }
  const field = (
    form: HTMLFormElement,
    name: string,
    label: string,
    multiline = false,
    required = true,
  ) => {
    const wrapper = node('label', label),
      value = multiline ? node('textarea') : node('input')
    value.name = name
    value.required = required
    value.maxLength = multiline ? 2000 : 160
    wrapper.append(value)
    form.append(wrapper)
    return value
  }
  const submit = (form: HTMLFormElement, text: string, command: () => ProjectCommand) => {
    const value = node('button', text)
    value.type = 'submit'
    form.append(value)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      void perform(() => mutate(command()))
    })
  }
  let disposed = false,
    busy = false,
    after: string | undefined
  let access: ProjectAccess | null = null,
    pending: ProjectMutation | null = null,
    selected: string | null = null
  let pendingUncertain = false
  const lifetime = new AbortController()
  const status = node('p')
  status.setAttribute('role', 'status')
  status.dataset.projectStatus = ''
  const recovery = node('div')
  recovery.hidden = true
  const list = node('ul'),
    detail = node('section'),
    create = node('form')
  list.dataset.projectList = ''
  detail.dataset.projectDetail = ''
  create.dataset.projectCreate = ''
  create.hidden = true
  const title = field(create, 'title', 'Project title'),
    scope = field(create, 'scope', 'Agreed internal scope', true)
  submit(create, 'Create project', () => ({
    operation: 'project.create',
    title: title.value,
    scopeSummary: scope.value,
    dueAt: null,
  }))
  const filter = node('input')
  filter.maxLength = 120
  filter.setAttribute('aria-label', 'Search projects')
  const refresh = button('Refresh projects', async () => {
    after = undefined
    await load()
    if (selected) await show(selected)
  })
  const next = button('Next projects', async () => {
    after = next.dataset.after
    await load()
  })
  next.hidden = true
  const recover = button('Check pending result', async () => {
    if (!pending) return
    const result = projectCommandStatusSchema.parse(await request('/commands/' + pending.requestId))
    if (result.status === 'committed') {
      pending = null
      pendingUncertain = false
      recovery.hidden = true
      await load()
      await show(result.receipt.projectId)
    } else
      status.textContent =
        'No committed receipt found yet. Retry the same request to resolve its outcome.'
  })
  const retry = button('Retry same request', async () => {
    if (pending) await sendPending()
  })
  recovery.append(
    node(
      'p',
      'The last request has an uncertain result. Check or retry it before making another change.',
    ),
    recover,
    retry,
  )
  host.replaceChildren(
    node('h2', options.title ?? 'Project delivery'),
    node(
      'p',
      'Track internal scope, tasks and milestones. Client agreements and payments have separate records.',
    ),
    status,
    recovery,
    create,
    filter,
    refresh,
    list,
    next,
    detail,
  )
  async function request(path: string, init?: RequestInit): Promise<unknown> {
    const timeout = new AbortController(),
      timer = setTimeout(() => timeout.abort(), 15_000)
    const cancel = () => timeout.abort()
    lifetime.signal.addEventListener('abort', cancel, { once: true })
    try {
      const response = await transport(options.apiBase + path, {
        credentials: 'same-origin',
        ...init,
        signal: timeout.signal,
      })
      if (!response.ok) {
        if (
          init?.method === 'POST' &&
          response.status >= 400 &&
          response.status < 500 &&
          !pendingUncertain
        ) {
          pending = null
          recovery.hidden = true
        }
        throw new Error(
          response.status === 409
            ? 'The project changed or its required work is incomplete. Refresh and check its details.'
            : response.status === 403
              ? 'Your current permissions do not allow this request.'
              : 'Project request could not be completed.',
        )
      }
      return await response.json()
    } finally {
      clearTimeout(timer)
      lifetime.signal.removeEventListener('abort', cancel)
    }
  }
  async function sendPending() {
    if (!pending || disposed) return
    let receipt
    try {
      receipt = projectReceiptSchema.parse(
        await request('/commands', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(pending),
        }),
      )
    } catch (error) {
      if (pending) pendingUncertain = true
      throw error
    }
    if (disposed) return
    pending = null
    pendingUncertain = false
    recovery.hidden = true
    create.reset()
    await load()
    await show(receipt.projectId)
  }
  async function mutate(command: ProjectCommand) {
    if (pending) throw new Error('Resolve the pending request before starting another change.')
    pending = projectMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    pendingUncertain = false
    try {
      await sendPending()
    } finally {
      if (!disposed) recovery.hidden = pending === null
    }
  }
  async function load() {
    const query = new URLSearchParams({ limit: '20', search: filter.value })
    if (after) query.set('after', after)
    const page = z
      .object({ items: z.array(projectSchema).max(20), next: z.uuid().nullable() })
      .strict()
      .parse(await request('/records?' + query.toString()))
    if (disposed) return
    list.replaceChildren()
    for (const item of page.items) {
      const row = node('li')
      const open = button(item.title + ' · ' + item.state, () => show(item.id))
      open.dataset.projectRecord = item.id
      row.append(open)
      list.append(row)
    }
    next.hidden = page.next === null
    next.dataset.after = page.next ?? ''
  }
  async function show(id: string) {
    const result = projectDetailSchema.parse(await request('/records/' + encodeURIComponent(id)))
    if (disposed) return
    selected = id
    const record = result.project,
      writable = !['delivered', 'closed', 'cancelled'].includes(record.state)
    const can = (permission: ProjectAccess['permissions'][number]) =>
      access?.permissions.includes(permission) ?? false
    const reference = { projectId: id, projectRevision: record.revision }
    detail.replaceChildren(
      node('h3', record.title),
      node('p', 'Status: ' + record.state),
      node('p', 'Scope: ' + record.scopeSummary),
    )
    if (record.scopeConfirmedAt)
      detail.append(node('p', 'Internal scope confirmed: ' + record.scopeConfirmedAt))
    if (record.dueAt) detail.append(node('p', 'Due: ' + record.dueAt))
    if (can('projects.write') && PROJECT_TRANSITIONS[record.state].length) {
      const form = node('form'),
        target = node('select')
      form.dataset.projectTransition = ''
      target.setAttribute('aria-label', 'Project status')
      for (const state of PROJECT_TRANSITIONS[record.state]) {
        const value = node('option', state)
        value.value = state
        target.append(value)
      }
      form.append(target)
      const reason = field(form, 'reason', 'Reason (required for cancellation)', true, false)
      target.addEventListener('change', () => {
        reason.required = target.value === 'cancelled'
      })
      reason.required = target.value === 'cancelled'
      submit(form, 'Update project', () => ({
        operation: 'project.transition',
        ...reference,
        state: projectStateSchema.parse(target.value),
        ...(reason.value.trim() ? { reason: reason.value } : {}),
      }))
      detail.append(form)
    }
    detail.append(node('h4', 'Milestones'))
    if (result.milestones === null)
      detail.append(node('p', 'Milestones are unavailable with your permissions.'))
    for (const item of result.milestones ?? []) {
      const section = node('div')
      section.append(
        node('p', item.title + ' · ' + item.state + (item.required ? ' · required' : '')),
      )
      if (item.evidence) section.append(node('p', item.evidence))
      if (writable && can('milestones.write') && item.state === 'open') {
        const form = node('form'),
          evidence = field(form, 'evidence', 'Completion evidence', true)
        form.dataset.projectMilestoneComplete = item.id
        submit(form, 'Complete milestone', () => ({
          operation: 'milestone.complete',
          ...reference,
          id: item.id,
          expectedRevision: item.revision,
          evidence: evidence.value,
        }))
        section.append(form)
      }
      detail.append(section)
    }
    if (writable && can('milestones.write')) {
      const form = node('form'),
        name = field(form, 'title', 'New required milestone')
      form.dataset.projectMilestoneCreate = ''
      submit(form, 'Add milestone', () => ({
        operation: 'milestone.create',
        ...reference,
        title: name.value,
        required: true,
        dueAt: null,
      }))
      detail.append(form)
    }
    detail.append(node('h4', 'Tasks'))
    if (result.tasks === null)
      detail.append(node('p', 'Tasks are unavailable with your permissions.'))
    for (const item of result.tasks ?? []) {
      const section = node('div')
      section.append(
        node('p', item.title + ' · ' + item.state + (item.required ? ' · required' : '')),
      )
      if (writable && can('tasks.write')) {
        const form = node('form'),
          target = node('select')
        form.dataset.projectTaskTransition = item.id
        target.setAttribute('aria-label', 'Task status')
        for (const state of TASK_TRANSITIONS[item.state]) {
          const option = node('option', state)
          option.value = state
          target.append(option)
        }
        form.append(target)
        const reason = field(form, 'reason', 'Reason (required for cancellation)', true, false)
        target.addEventListener('change', () => {
          reason.required = target.value === 'cancelled'
        })
        reason.required = target.value === 'cancelled'
        submit(form, 'Update task', () => ({
          operation: 'task.transition',
          ...reference,
          id: item.id,
          expectedRevision: item.revision,
          state: target.value as 'todo' | 'doing' | 'done' | 'cancelled',
          ...(reason.value.trim() ? { reason: reason.value } : {}),
        }))
        section.append(form)
      }
      detail.append(section)
    }
    if (writable && can('tasks.write')) {
      const form = node('form'),
        name = field(form, 'title', 'New required task'),
        parent = node('select')
      form.dataset.projectTaskCreate = ''
      parent.setAttribute('aria-label', 'Task milestone')
      const unlinked = node('option', 'No milestone')
      unlinked.value = ''
      parent.append(unlinked)
      for (const item of result.milestones ?? []) {
        if (item.state !== 'open') continue
        const option = node('option', item.title)
        option.value = item.id
        parent.append(option)
      }
      form.append(parent)
      submit(form, 'Add task', () => ({
        operation: 'task.create',
        ...reference,
        title: name.value,
        milestoneId: parent.value || null,
        required: true,
        dueAt: null,
      }))
      detail.append(form)
    }
    const history = node('ol')
    for (const item of result.audit ?? [])
      history.append(
        node(
          'li',
          item.occurredAt +
            ' · ' +
            item.operation +
            ' · ' +
            item.actorId +
            (item.reason ? ' · ' + item.reason : ''),
        ),
      )
    detail.append(
      node('h4', 'Latest project changes'),
      result.audit === null ? node('p', 'Audit history requires permission.') : history,
    )
    if (result.auditHasMore)
      detail.append(
        node('p', 'Showing the latest 50 changes. Earlier audit records remain stored.'),
      )
  }
  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    status.textContent = 'Working…'
    try {
      await work()
      if (!disposed && !pending) status.textContent = 'Up to date.'
    } catch (error) {
      if (!disposed)
        status.textContent = error instanceof Error ? error.message : 'Project request failed.'
    } finally {
      busy = false
      if (!disposed) recovery.hidden = pending === null
    }
  }
  void perform(async () => {
    access = projectAccessSchema.parse(await request('/access'))
    if (disposed) return
    if (!access.permissions.includes('projects.read')) {
      status.textContent = 'Project access requires a grant from your administrator.'
      return
    }
    create.hidden = !access.permissions.includes('projects.write')
    await load()
  })
  return {
    refresh: () =>
      perform(async () => {
        await load()
        if (selected) await show(selected)
      }),
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      access = null
      host.replaceChildren()
    },
  }
}
