import { z } from 'zod'
import {
  schedulingAccessSchema,
  schedulingResourceSchema,
  reservationSchema,
  schedulingMutationSchema,
  schedulingReceiptSchema,
  schedulingCommandStatusSchema,
  type SchedulingAccess,
  type SchedulingMutation,
  type SchedulingCommand,
  type SchedulingResource,
  type Reservation,
} from './contracts.js'

/** Human-operated UI with explicit instants. The embedding consumer supplies authenticated transport. */
export function mountSchedulingDashboard(
  host: HTMLElement,
  options: { apiBase: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument,
    transport = options.transport ?? fetch
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const value = document.createElement(tag)
    value.textContent = text
    return value
  }
  let disposed = false,
    busy = false,
    access: SchedulingAccess | null = null,
    pending: SchedulingMutation | null = null,
    uncertain = false
  let selected: SchedulingResource | null = null,
    resourceAfter: string | undefined,
    reservationAfter: string | undefined
  const lifetime = new AbortController(),
    status = node('p'),
    recovery = node('div'),
    resources = node('ul'),
    reservations = node('ul'),
    create = node('form'),
    booking = node('form'),
    details = node('section')
  status.setAttribute('role', 'status')
  status.dataset.schedulingStatus = ''
  create.dataset.resourceCreate = ''
  booking.dataset.reservationCreate = ''
  resources.dataset.resourceList = ''
  reservations.dataset.reservationList = ''
  recovery.hidden = true
  create.hidden = true
  booking.hidden = true
  const field = (form: HTMLFormElement, name: string, label: string, maxLength = 160) => {
    const wrapper = node('label', label),
      value = node('input')
    value.name = name
    value.required = true
    value.maxLength = maxLength
    wrapper.append(value)
    form.append(wrapper)
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
  const submit = (form: HTMLFormElement, text: string, command: () => SchedulingCommand) => {
    const value = node('button', text)
    value.type = 'submit'
    form.append(value)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      void perform(() => mutate(command()))
    })
  }
  const title = field(create, 'title', 'Resource name'),
    timezone = field(create, 'timezone', 'IANA timezone', 100)
  timezone.value = 'Asia/Jakarta'
  const kind = node('select')
  kind.name = 'kind'
  for (const label of ['staff', 'asset', 'room', 'space']) {
    const option = node('option', label)
    option.value = label
    kind.append(option)
  }
  const kindLabel = node('label', 'Resource type')
  kindLabel.append(kind)
  create.append(kindLabel)
  submit(create, 'Create resource', () => ({
    operation: 'resource.create',
    title: title.value,
    timezone: timezone.value,
    kind: z.enum(['staff', 'asset', 'room', 'space']).parse(kind.value),
  }))
  const bookingTitle = field(booking, 'title', 'Reservation title'),
    start = field(booking, 'startsAt', 'Start, ISO time with UTC offset', 40),
    end = field(booking, 'endsAt', 'End, ISO time with UTC offset', 40)
  start.placeholder = '2026-10-12T10:00:00+07:00'
  end.placeholder = '2026-10-12T11:00:00+07:00'
  submit(booking, 'Hold slot for 15 minutes', () => {
    if (!selected) throw new Error('Select a resource.')
    return {
      operation: 'reservation.hold',
      resourceId: selected.id,
      title: bookingTitle.value,
      startsAt: start.value,
      endsAt: end.value,
      holdSeconds: 900,
    }
  })
  const resourceNext = button('Next resources', async () => {
    resourceAfter = resourceNext.dataset.after
    await loadResources()
  })
  resourceNext.hidden = true
  const reservationNext = button('Next reservations', async () => {
    reservationAfter = reservationNext.dataset.after
    await loadReservations()
  })
  reservationNext.hidden = true
  const resolve = button('Check pending result', async () => {
    if (!pending) return
    const result = schedulingCommandStatusSchema.parse(
      await request('/commands/' + pending.requestId),
    )
    if (result.status === 'committed') {
      pending = null
      uncertain = false
      recovery.hidden = true
      await reload(result.receipt.resourceId)
    } else
      status.textContent =
        'No committed receipt found yet. Retry the same request to resolve its outcome.'
  })
  recovery.append(
    node('p', 'Result uncertain. Check or retry the same request before making another change.'),
    resolve,
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Resource reservations'),
    node(
      'p',
      'One resource per slot. Enter times with an explicit offset, for example +07:00. Confirm a hold before it expires.',
    ),
    status,
    recovery,
    create,
    button('Refresh reservations', async () => {
      resourceAfter = undefined
      reservationAfter = undefined
      await reload(selected?.id)
    }),
    resources,
    resourceNext,
    details,
    booking,
    reservations,
    reservationNext,
  )
  async function request(path: string, init?: RequestInit): Promise<unknown> {
    const timeout = new AbortController(),
      timer = setTimeout(() => timeout.abort(), 15_000),
      cancel = () => timeout.abort()
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
          !uncertain
        ) {
          pending = null
          recovery.hidden = true
        }
        throw new Error(
          response.status === 409
            ? 'The slot is unavailable or the reservation changed. Refresh and check.'
            : response.status === 403
              ? 'Current permissions refuse this request.'
              : 'Reservation request could not be completed.',
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
      receipt = schedulingReceiptSchema.parse(
        await request('/commands', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(pending),
        }),
      )
    } catch (error) {
      if (pending) uncertain = true
      throw error
    }
    if (disposed) return
    pending = null
    uncertain = false
    recovery.hidden = true
    booking.reset()
    await reload(receipt.resourceId)
  }
  async function mutate(command: SchedulingCommand) {
    if (pending) throw new Error('Resolve the pending request first.')
    pending = schedulingMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    uncertain = false
    try {
      await sendPending()
    } finally {
      if (!disposed) recovery.hidden = pending === null
    }
  }
  async function loadResources() {
    const query = new URLSearchParams({ limit: '20' })
    if (resourceAfter) query.set('after', resourceAfter)
    const page = z
      .object({
        items: z.array(schedulingResourceSchema).max(20),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/resources?' + query.toString()))
    if (disposed) return
    resources.replaceChildren()
    for (const item of page.items) {
      const row = node('li')
      row.append(
        button(item.title + ' · ' + item.timezone + (item.active ? '' : ' · disabled'), () =>
          select(item.id),
        ),
      )
      resources.append(row)
    }
    resourceNext.hidden = page.next === null
    resourceNext.dataset.after = page.next ?? ''
  }
  async function select(id: string) {
    selected = schedulingResourceSchema.parse(await request('/resources/' + id))
    if (disposed) return
    reservationAfter = undefined
    details.replaceChildren(node('h3', selected.title))
    booking.hidden =
      !selected.active ||
      !access?.permissions.includes('reservations.write') ||
      !access.permissions.includes('reservations.read')
    if (access?.permissions.includes('resources.write')) {
      const current = selected
      details.append(
        button(current.active ? 'Disable resource' : 'Enable resource', () =>
          mutate({
            operation: 'resource.set-active',
            resourceId: current.id,
            expectedRevision: current.revision,
            active: !current.active,
            reason: 'Owner changed availability',
          }),
        ),
      )
    }
    await loadReservations()
  }
  const display = (value: string) =>
    selected
      ? new Intl.DateTimeFormat('en-GB', {
          timeZone: selected.timezone,
          dateStyle: 'medium',
          timeStyle: 'short',
        }).format(new Date(value))
      : value
  function controls(item: Reservation) {
    const section = node('section')
    if (
      !access?.permissions.includes('reservations.write') ||
      ['expired', 'cancelled'].includes(item.effectiveState) ||
      (item.kind === 'block' && !access.permissions.includes('resources.write'))
    )
      return section
    const ref = { resourceId: item.resourceId, id: item.id, expectedRevision: item.revision }
    if (item.effectiveState === 'held')
      section.append(
        button('Confirm reservation', () => mutate({ operation: 'reservation.confirm', ...ref })),
      )
    const cancelForm = node('form'),
      cancellation = field(cancelForm, 'reason', 'Cancellation reason', 2000)
    submit(cancelForm, 'Cancel reservation', () => ({
      operation: 'reservation.cancel',
      ...ref,
      reason: cancellation.value,
    }))
    section.append(cancelForm)
    const move = node('form'),
      from = field(move, 'startsAt', 'New start, ISO time with UTC offset', 40),
      to = field(move, 'endsAt', 'New end, ISO time with UTC offset', 40),
      explanation = field(move, 'reason', 'Move reason', 2000)
    from.value = item.startsAt
    to.value = item.endsAt
    submit(move, 'Move reservation', () => ({
      operation: 'reservation.move',
      ...ref,
      startsAt: from.value,
      endsAt: to.value,
      reason: explanation.value,
    }))
    section.append(move)
    return section
  }
  async function loadReservations() {
    reservations.replaceChildren()
    reservationNext.hidden = true
    if (!selected || !access?.permissions.includes('reservations.read')) return
    const query = new URLSearchParams({ resourceId: selected.id, limit: '20' })
    if (reservationAfter) query.set('after', reservationAfter)
    const page = z
      .object({
        items: z.array(reservationSchema).max(20),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/reservations?' + query.toString()))
    if (disposed) return
    for (const item of page.items) {
      const row = node(
        'li',
        item.title +
          ' · ' +
          item.effectiveState +
          ' · ' +
          display(item.startsAt) +
          ' – ' +
          display(item.endsAt),
      )
      if (item.effectiveState === 'held') row.append(node('p', 'Hold expires ' + item.expiresAt))
      row.dataset.reservationId = item.id
      row.append(controls(item))
      reservations.append(row)
    }
    reservationNext.hidden = page.next === null
    reservationNext.dataset.after = page.next ?? ''
  }
  async function reload(id?: string) {
    await loadResources()
    if (!disposed && id) await select(id)
  }
  async function perform(work: () => Promise<void>) {
    if (busy || disposed) return
    busy = true
    status.textContent = 'Working…'
    try {
      await work()
      if (!disposed && !pending) status.textContent = ''
    } catch (error) {
      if (!disposed) status.textContent = error instanceof Error ? error.message : 'Request failed.'
    } finally {
      busy = false
      if (!disposed) recovery.hidden = pending === null
    }
  }
  const ready = perform(async () => {
    access = schedulingAccessSchema.parse(await request('/access'))
    if (disposed) return
    create.hidden = !access.permissions.includes('resources.write')
    await loadResources()
  })
  return {
    ready,
    refresh: () => perform(() => reload(selected?.id)),
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      selected = null
      access = null
      host.replaceChildren()
    },
  }
}
