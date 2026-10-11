import { z } from 'zod'
import {
  supportAccessSchema,
  supportMutationSchema,
  supportReceiptSchema,
  supportCommandStatusSchema,
  caseSchema,
  caseDetailSchema,
  assigneeSchema,
  type SupportAccess,
  type SupportMutation,
  type CaseDetail,
} from './contracts.js'
/** Shared human case UI. Host transport supplies identity and CSRF; notes are internal records. */
export function mountSupportDashboard(
  host: HTMLElement,
  options: { apiBase: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument,
    transport = options.transport ?? fetch,
    lifetime = new AbortController(),
    node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
      const n = document.createElement(tag)
      n.textContent = text
      return n
    }
  let disposed = false,
    busy = false,
    access: SupportAccess | null = null,
    pending: SupportMutation | null = null,
    uncertain = false,
    selected: CaseDetail | null = null,
    after: string | undefined
  const status = node('p'),
    recovery = node('div'),
    create = node('form'),
    filters = node('form'),
    list = node('ul'),
    detail = node('section'),
    assignees = new Map<string, string>()
  status.dataset.supportStatus = ''
  status.setAttribute('role', 'status')
  create.dataset.supportCreate = ''
  list.dataset.supportList = ''
  detail.dataset.supportDetail = ''
  filters.dataset.supportFilters = ''
  recovery.hidden = true
  create.hidden = true
  const button = (text: string, work: () => Promise<void>) => {
    const b = node('button', text)
    b.type = 'button'
    b.addEventListener('click', () => {
      void perform(work)
    })
    return b
  }
  const field = (
    form: HTMLElement,
    name: string,
    label: string,
    type = 'text',
    value = '',
    required = true,
    max = 160,
  ) => {
    const l = node('label', label),
      i = node('input')
    i.name = name
    i.type = type
    i.value = value
    i.required = required
    i.maxLength = max
    l.append(i)
    form.append(l)
    return i
  }
  const textarea = (form: HTMLElement, name: string, label: string) => {
    const l = node('label', label),
      i = node('textarea')
    i.name = name
    i.required = true
    i.maxLength = 4000
    l.append(i)
    form.append(l)
    return i
  }
  const select = (form: HTMLElement, name: string, label: string, choices: readonly string[]) => {
    const l = node('label', label),
      s = node('select')
    s.name = name
    for (const value of choices) {
      const o = node('option', value || 'All')
      o.value = value
      s.append(o)
    }
    l.append(s)
    form.append(l)
    return s
  }
  const submit = (f: HTMLFormElement, text: string, command: () => unknown) => {
    const b = node('button', text)
    b.type = 'submit'
    f.append(b)
    f.addEventListener('submit', (e) => {
      e.preventDefault()
      void perform(() => mutate(command()))
    })
  }
  const target = (input: HTMLInputElement) =>
    input.value ? new Date(input.value).toISOString() : null
  const requester = field(create, 'requesterName', 'Requester'),
    subject = field(create, 'subject', 'Issue'),
    description = textarea(create, 'description', 'Description'),
    priority = select(create, 'priority', 'Priority', ['normal', 'low', 'high']),
    due = field(create, 'dueAt', 'Internal due target', 'datetime-local', '', false)
  submit(create, 'Create case', () => ({
    operation: 'case.create',
    requesterName: requester.value,
    subject: subject.value,
    description: description.value,
    priority: priority.value,
    dueAt: target(due),
  }))
  const stateFilter = select(filters, 'state', 'State', [
      '',
      'open',
      'in_progress',
      'waiting',
      'resolved',
      'closed',
      'cancelled',
    ]),
    priorityFilter = select(filters, 'priority', 'Priority', ['', 'low', 'normal', 'high']),
    dueFilter = select(filters, 'overdue', 'Due target', ['', 'true', 'false'])
  const filterSubmit = node('button', 'Apply filters')
  filterSubmit.type = 'submit'
  filters.append(filterSubmit)
  filters.addEventListener('submit', (e) => {
    e.preventDefault()
    after = undefined
    void perform(loadCases)
  })
  const next = button('Next cases', async () => {
    after = next.dataset.after
    await loadCases()
  })
  next.hidden = true
  recovery.append(
    node('p', 'Result uncertain. Resolve this request before another change.'),
    button('Check pending result', async () => {
      if (!pending) return
      const result = supportCommandStatusSchema.parse(
        await request('/commands/' + pending.requestId),
      )
      if (result.status === 'committed') {
        pending = null
        uncertain = false
        recovery.hidden = true
        await afterReceipt(result.receipt)
      } else
        status.textContent =
          'No committed receipt yet. Retry the same request to resolve its outcome.'
    }),
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Support cases'),
    node('p', 'Investigation, assignment and resolution history. Notes stay internal.'),
    status,
    recovery,
    create,
    button('Refresh support cases', reload),
    filters,
    list,
    next,
    detail,
  )
  async function request(path: string, body?: unknown) {
    const response = await transport(options.apiBase + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: lifetime.signal,
    })
    if (!response.ok) {
      const error = new Error('Request refused (' + response.status + ').') as Error & {
        status: number
      }
      error.status = response.status
      throw error
    }
    return (await response.json()) as unknown
  }
  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    lock()
    try {
      await work()
    } catch (error) {
      if (!disposed)
        status.textContent = error instanceof Error ? error.message : 'Support unavailable.'
    } finally {
      busy = false
      if (!disposed) lock()
    }
  }
  function lock() {
    for (const b of host.querySelectorAll<HTMLButtonElement>('button')) b.disabled = busy
  }
  function can(p: SupportAccess['permissions'][number]) {
    return access?.permissions.includes(p) ?? false
  }
  async function loadAssignees() {
    assignees.clear()
    if (!can('cases.read') || !can('cases.assign') || access?.scope !== 'all') return
    const page = z
      .object({ items: z.array(assigneeSchema), next: z.string().nullable() })
      .strict()
      .parse(await request('/assignees?limit=100'))
    if (disposed) return
    for (const p of page.items) assignees.set(p.id, p.displayName)
  }
  async function loadCases() {
    if (!can('cases.read')) {
      list.replaceChildren()
      detail.replaceChildren()
      selected = null
      return
    }
    const q = new URLSearchParams()
    q.set('limit', '50')
    if (after) q.set('after', after)
    if (stateFilter.value) q.set('state', stateFilter.value)
    if (priorityFilter.value) q.set('priority', priorityFilter.value)
    if (dueFilter.value) q.set('overdue', dueFilter.value)
    const page = z
      .object({
        items: z.array(caseSchema),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/cases?' + q.toString()))
    if (disposed) return
    list.replaceChildren()
    for (const c of page.items) {
      const l = node(
        'li',
        c.subject + ' — ' + c.state + ' — ' + c.priority + (c.overdue ? ' — overdue' : ''),
      )
      l.append(
        button('Open case', async () => {
          selected = caseDetailSchema.parse(await request('/cases/' + c.id))
          if (!disposed) drawDetail()
        }),
      )
      list.append(l)
    }
    next.hidden = page.next === null
    next.dataset.after = page.next ?? ''
  }
  function drawDetail() {
    detail.replaceChildren()
    if (!selected) return
    const { record: c, notes, transitions } = selected
    detail.dataset.caseId = c.id
    detail.append(
      node('h3', c.subject),
      node('p', c.requesterName + ' — ' + c.state + ' — cycle ' + c.cycle),
      node('p', c.description),
      node(
        'p',
        'Assigned: ' + (c.assigneeId ? (assignees.get(c.assigneeId) ?? 'Operator') : 'Unassigned'),
      ),
      node(
        'p',
        c.dueAt
          ? 'Internal due target: ' +
              new Date(c.dueAt).toLocaleString() +
              (c.overdue ? ' (overdue)' : '')
          : 'No internal due target',
      ),
    )
    if (c.resolution) detail.append(node('p', 'Resolution: ' + c.resolution))
    const t = { id: c.id, expectedRevision: c.revision }
    if (can('cases.write')) {
      const actions = node('section'),
        reason = field(actions, 'reason', 'Reason', 'text', '', false, 2000)
      if (c.state === 'open')
        actions.append(
          button('Start investigation', () =>
            mutate({ operation: 'case.start', ...t, reason: 'Operator starts investigation' }),
          ),
        )
      if (c.state === 'in_progress')
        actions.append(
          button('Wait for information', () =>
            mutate({ operation: 'case.wait', ...t, reason: reason.value }),
          ),
        )
      if (c.state === 'waiting')
        actions.append(
          button('Resume investigation', () =>
            mutate({ operation: 'case.resume', ...t, reason: 'Operator resumes investigation' }),
          ),
        )
      if (c.state === 'in_progress' || c.state === 'waiting') {
        const f = node('form'),
          solution = textarea(f, 'resolution', 'Resolution')
        submit(f, 'Resolve case', () => ({
          operation: 'case.resolve',
          ...t,
          resolution: solution.value,
        }))
        actions.append(f)
      }
      if (c.state === 'resolved')
        actions.append(
          button('Close case', () =>
            mutate({
              operation: 'case.close',
              ...t,
              reason: 'Operator closes the resolved investigation',
            }),
          ),
        )
      if (c.state === 'resolved' || c.state === 'closed') {
        const f = node('form'),
          why = field(f, 'reason', 'Reopen reason', 'text', '', true, 2000),
          newDue = field(f, 'dueAt', 'New internal due target', 'datetime-local', '', false)
        submit(f, 'Reopen case', () => ({
          operation: 'case.reopen',
          ...t,
          reason: why.value,
          dueAt: target(newDue),
        }))
        actions.append(f)
      }
      if (['open', 'in_progress', 'waiting'].includes(c.state))
        actions.append(
          button('Cancel case', () =>
            mutate({ operation: 'case.cancel', ...t, reason: reason.value }),
          ),
        )
      if (c.state !== 'closed' && c.state !== 'cancelled') {
        const p = select(actions, 'priority', 'New priority', ['normal', 'low', 'high'])
        p.value = c.priority
        const d = field(actions, 'dueAt', 'New internal due target', 'datetime-local', '', false)
        actions.append(
          button('Update priority', () =>
            mutate({
              operation: 'case.set-priority',
              ...t,
              priority: p.value,
              reason: reason.value,
            }),
          ),
          button('Update due target', () =>
            mutate({ operation: 'case.set-target', ...t, dueAt: target(d), reason: reason.value }),
          ),
        )
        if (can('cases.assign') && access?.scope === 'all') {
          const l = node('label', 'Assign operator'),
            s = node('select')
          s.name = 'assigneeId'
          const none = node('option', 'Unassigned')
          none.value = ''
          s.append(none)
          for (const [id, name] of assignees) {
            const o = node('option', name)
            o.value = id
            s.append(o)
          }
          s.value = c.assigneeId ?? ''
          l.append(s)
          actions.append(
            l,
            button('Update assignment', () =>
              mutate({
                operation: 'case.assign',
                ...t,
                assigneeId: s.value || null,
                reason: reason.value,
              }),
            ),
          )
        }
      }
      detail.append(actions)
    }
    detail.append(node('h4', 'Internal notes'))
    if (notes === null) detail.append(node('p', 'Notes unavailable with current permissions.'))
    else for (const n of notes) detail.append(node('p', n.body))
    if (selected.notesHasMore) detail.append(node('p', 'Showing the latest 100 notes.'))
    if (can('notes.write')) {
      const f = node('form'),
        text = textarea(f, 'body', 'Internal finding')
      f.dataset.supportNote = ''
      submit(f, 'Add internal note', () => ({ operation: 'note.add', ...t, body: text.value }))
      detail.append(f)
    }
    if (transitions !== null) {
      detail.append(node('h4', 'Investigation history'))
      for (const e of transitions) {
        detail.append(
          node(
            'p',
            e.operation +
              ' — ' +
              (e.from ?? 'new') +
              ' → ' +
              e.to +
              ' — cycle ' +
              e.cycle +
              (e.reason ? ' — ' + e.reason : ''),
          ),
        )
        if (e.previousResolution)
          detail.append(node('p', 'Previous resolution: ' + e.previousResolution))
        if (e.resolution) detail.append(node('p', 'Resolution: ' + e.resolution))
      }
    }
  }
  async function reload() {
    try {
      access = supportAccessSchema.parse(await request('/access'))
    } catch (error) {
      access = null
      selected = null
      assignees.clear()
      list.replaceChildren()
      detail.replaceChildren()
      create.hidden = true
      throw error
    }
    if (disposed) return
    create.hidden = !can('cases.read') || !can('cases.write')
    await loadAssignees()
    await loadCases()
    if (selected) {
      try {
        selected = caseDetailSchema.parse(await request('/cases/' + selected.record.id))
        if (!disposed) drawDetail()
      } catch (error) {
        selected = null
        detail.replaceChildren()
        throw error
      }
    }
    if (!pending) status.textContent = 'Support ready.'
  }
  async function afterReceipt(receipt: z.infer<typeof supportReceiptSchema>) {
    if (disposed) return
    selected = caseDetailSchema.parse(await request('/cases/' + receipt.id))
    await reload()
  }
  async function mutate(command: unknown) {
    if (pending) throw new Error('Resolve the pending request first.')
    pending = supportMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    uncertain = false
    await sendPending()
  }
  async function sendPending() {
    if (!pending) return
    try {
      const receipt = supportReceiptSchema.parse(await request('/commands', pending))
      pending = null
      uncertain = false
      recovery.hidden = true
      await afterReceipt(receipt)
    } catch (error) {
      if (!pending) throw error
      const code =
        error && typeof error === 'object' && 'status' in error ? Number(error.status) : null
      if (code !== null && [400, 401, 403, 404, 409, 413].includes(code) && !uncertain) {
        pending = null
        recovery.hidden = true
      } else {
        uncertain = true
        recovery.hidden = false
      }
      throw error
    }
  }
  void perform(reload)
  return {
    refresh: () => perform(reload),
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      selected = null
      access = null
      assignees.clear()
      host.replaceChildren()
    },
  }
}
