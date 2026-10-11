import { z } from 'zod'
import {
  accommodationAccessSchema,
  propertySchema,
  roomTypeSchema,
  roomSchema,
  staySchema,
  stayDetailSchema,
  accommodationMutationSchema,
  accommodationReceiptSchema,
  accommodationCommandStatusSchema,
  type AccommodationAccess,
  type AccommodationMutation,
  type AccommodationCommand,
  type Property,
  type Room,
  type RoomType,
} from './contracts.js'

/** Reusable human-operated room-night panel. Identity and CSRF belong to the embedding host. */
export function mountAccommodationDashboard(
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
    access: AccommodationAccess | null = null,
    pending: AccommodationMutation | null = null,
    uncertain = false
  let selected: Property | null = null,
    selectedRoom: Room | null = null,
    selectedStay: string | null = null,
    propertyAfter: string | undefined,
    roomAfter: string | undefined,
    typeAfter: string | undefined,
    stayAfter: string | undefined
  let types: readonly RoomType[] = []
  const lifetime = new AbortController(),
    status = node('p'),
    recovery = node('div'),
    propertyList = node('ul'),
    typeList = node('ul'),
    roomList = node('ul'),
    stayList = node('ul'),
    roomDetail = node('section'),
    stayDetail = node('section'),
    propertyCreate = node('form'),
    typeCreate = node('form'),
    roomCreate = node('form'),
    stayCreate = node('form')
  status.setAttribute('role', 'status')
  status.dataset.accommodationStatus = ''
  propertyList.dataset.propertyList = ''
  roomList.dataset.hotelRoomList = ''
  stayList.dataset.stayList = ''
  roomDetail.dataset.hotelRoomDetail = ''
  stayDetail.dataset.stayDetail = ''
  propertyCreate.dataset.propertyCreate = ''
  typeCreate.dataset.roomTypeCreate = ''
  roomCreate.dataset.hotelRoomCreate = ''
  stayCreate.dataset.stayCreate = ''
  recovery.hidden = true
  for (const form of [propertyCreate, typeCreate, roomCreate, stayCreate]) form.hidden = true
  const field = (
    form: HTMLFormElement,
    name: string,
    label: string,
    type = 'text',
    value = '',
    maxLength = 160,
  ) => {
    const wrapper = node('label', label),
      input = node('input')
    input.name = name
    input.type = type
    input.value = value
    input.maxLength = maxLength
    input.required = true
    wrapper.append(input)
    form.append(wrapper)
    return input
  }
  const button = (text: string, work: () => Promise<void>) => {
    const b = node('button', text)
    b.type = 'button'
    b.addEventListener('click', () => {
      void perform(work)
    })
    return b
  }
  const submit = (form: HTMLFormElement, text: string, command: () => AccommodationCommand) => {
    const b = node('button', text)
    b.type = 'submit'
    form.append(b)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      void perform(() => mutate(command()))
    })
  }
  const propertyTitle = field(propertyCreate, 'title', 'Property name'),
    timezone = field(propertyCreate, 'timezone', 'IANA timezone', 'text', 'Asia/Jakarta', 100),
    checkIn = field(propertyCreate, 'checkInTime', 'Scheduled check-in', 'time', '15:00'),
    checkOut = field(propertyCreate, 'checkOutTime', 'Scheduled check-out', 'time', '11:00')
  submit(propertyCreate, 'Create property', () => ({
    operation: 'property.create',
    title: propertyTitle.value,
    timezone: timezone.value,
    checkInTime: checkIn.value,
    checkOutTime: checkOut.value,
  }))
  const typeTitle = field(typeCreate, 'title', 'Room type'),
    typeGuests = field(typeCreate, 'maxGuests', 'Maximum guests', 'number', '2'),
    rate = field(typeCreate, 'nightlyRate', 'Nightly rate, IDR', 'number', '0')
  typeGuests.min = '1'
  typeGuests.max = '16'
  rate.min = '0'
  rate.max = '1000000000'
  rate.step = '1'
  const propertyId = () => {
    if (!selected) throw new Error('Select a property.')
    return selected.id
  }
  submit(typeCreate, 'Create room type', () => ({
    operation: 'room-type.create',
    propertyId: propertyId(),
    title: typeTitle.value,
    maxGuests: Number(typeGuests.value),
    nightlyRate: Number(rate.value),
  }))
  const code = field(roomCreate, 'code', 'Room code', 'text', '', 40),
    typeSelect = node('select')
  typeSelect.name = 'roomTypeId'
  typeSelect.required = true
  const typeLabel = node('label', 'Room type')
  typeLabel.append(typeSelect)
  roomCreate.append(typeLabel)
  submit(roomCreate, 'Create room', () => ({
    operation: 'room.create',
    propertyId: propertyId(),
    roomTypeId: typeSelect.value,
    code: code.value,
  }))
  const guest = field(stayCreate, 'guestName', 'Guest name'),
    guests = field(stayCreate, 'guests', 'Guest count', 'number', '1'),
    arrival = field(stayCreate, 'arrival', 'Arrival date', 'date'),
    departure = field(stayCreate, 'departure', 'Departure date', 'date')
  guests.min = '1'
  guests.max = '16'
  submit(stayCreate, 'Hold room for 15 minutes', () => {
    if (!selectedRoom) throw new Error('Select a room.')
    return {
      operation: 'stay.hold',
      propertyId: propertyId(),
      roomId: selectedRoom.id,
      arrival: arrival.value,
      departure: departure.value,
      guestName: guest.value,
      guests: Number(guests.value),
      holdSeconds: 900,
    }
  })
  const propertyNext = button('Next properties', async () => {
      propertyAfter = propertyNext.dataset.after
      await loadProperties()
    }),
    typeNext = button('Next room types', async () => {
      typeAfter = typeNext.dataset.after
      await loadTypes()
    }),
    roomNext = button('Next rooms', async () => {
      roomAfter = roomNext.dataset.after
      await loadRooms()
    }),
    stayNext = button('Next stays', async () => {
      stayAfter = stayNext.dataset.after
      await loadStays()
    })
  for (const b of [propertyNext, typeNext, roomNext, stayNext]) b.hidden = true
  recovery.append(
    node('p', 'Result uncertain. Check or retry the same request before making another change.'),
    button('Check pending result', async () => {
      if (!pending) return
      const result = accommodationCommandStatusSchema.parse(
        await request('/commands/' + pending.requestId),
      )
      if (result.status === 'committed') {
        pending = null
        uncertain = false
        recovery.hidden = true
        await reload(result.receipt.propertyId)
        if (result.receipt.kind === 'stay') await showStay(result.receipt.id)
      } else
        status.textContent =
          'No committed receipt found yet. Retry the same request to resolve its outcome.'
    }),
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Accommodation'),
    node(
      'p',
      'Manage individual rooms, calendar nights and guest stays. Amounts are accommodation estimates; payment and tax records are separate.',
    ),
    status,
    recovery,
    propertyCreate,
    button('Refresh accommodation', async () => {
      propertyAfter = undefined
      roomAfter = undefined
      typeAfter = undefined
      stayAfter = undefined
      await reload(selected?.id)
      if (selectedStay) await showStay(selectedStay)
    }),
    propertyList,
    propertyNext,
    typeCreate,
    typeList,
    typeNext,
    roomCreate,
    roomList,
    roomNext,
    roomDetail,
    stayCreate,
    stayList,
    stayNext,
    stayDetail,
  )
  async function request(path: string, init?: RequestInit): Promise<unknown> {
    const timeout = new AbortController(),
      timer = setTimeout(() => timeout.abort(), 15000),
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
            ? 'The room, stay or required readiness changed. Refresh and check.'
            : response.status === 403
              ? 'Current permissions refuse this request.'
              : 'Accommodation request could not be completed.',
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
      receipt = accommodationReceiptSchema.parse(
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
    await reload(receipt.propertyId)
    if (receipt.kind === 'room') await showRoom(receipt.id)
    if (receipt.kind === 'stay') await showStay(receipt.id)
  }
  async function mutate(command: AccommodationCommand) {
    if (pending) throw new Error('Resolve the pending request first.')
    pending = accommodationMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    uncertain = false
    try {
      await sendPending()
    } finally {
      if (!disposed) recovery.hidden = pending === null
    }
  }
  const page = <T extends z.ZodType>(schema: T) =>
    z
      .object({
        items: z.array(schema).max(20),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
  const query = (after?: string) => {
    const q = new URLSearchParams({ propertyId: propertyId(), limit: '20' })
    if (after) q.set('after', after)
    return q.toString()
  }
  const next = (b: HTMLButtonElement, value: string | null) => {
    b.hidden = value === null
    b.dataset.after = value ?? ''
  }
  async function loadProperties() {
    const q = new URLSearchParams({ limit: '20' })
    if (propertyAfter) q.set('after', propertyAfter)
    const result = page(propertySchema).parse(await request('/properties?' + q.toString()))
    if (disposed) return
    propertyList.replaceChildren()
    for (const p of result.items) {
      const row = node('li')
      row.append(button(p.title + ' · ' + p.timezone, () => selectProperty(p.id)))
      propertyList.append(row)
    }
    next(propertyNext, result.next)
  }
  async function selectProperty(id: string) {
    selected = propertySchema.parse(await request('/properties/' + id))
    if (disposed) return
    selectedRoom = null
    selectedStay = null
    roomDetail.replaceChildren()
    stayDetail.replaceChildren()
    stayCreate.hidden = true
    roomAfter = undefined
    typeAfter = undefined
    stayAfter = undefined
    typeCreate.hidden = !access?.permissions.includes('rooms.write')
    roomCreate.hidden = !access?.permissions.includes('rooms.write')
    await loadTypes()
    await loadRooms()
    await loadStays()
  }
  async function loadTypes() {
    typeList.replaceChildren()
    typeSelect.replaceChildren()
    if (!access?.permissions.includes('rooms.read')) return
    const result = page(roomTypeSchema).parse(await request('/room-types?' + query(typeAfter)))
    if (disposed) return
    types = result.items
    for (const item of types) {
      typeList.append(
        node(
          'li',
          item.title + ' · max ' + item.maxGuests + ' guests · IDR ' + item.nightlyRate + '/night',
        ),
      )
      const option = node('option', item.title)
      option.value = item.id
      typeSelect.append(option)
    }
    next(typeNext, result.next)
  }
  async function loadRooms() {
    roomList.replaceChildren()
    if (!access?.permissions.includes('rooms.read')) return
    const result = page(roomSchema).parse(await request('/rooms?' + query(roomAfter)))
    if (disposed) return
    for (const item of result.items) {
      const row = node('li')
      row.append(
        button(
          item.code +
            ' · ' +
            item.housekeeping +
            (item.occupied ? ' · occupied' : '') +
            (item.active ? '' : ' · disabled'),
          () => showRoom(item.id),
        ),
      )
      roomList.append(row)
    }
    next(roomNext, result.next)
  }
  async function showRoom(id: string) {
    const found = roomSchema.parse(await request('/rooms/' + id))
    if (disposed) return
    if (found.propertyId !== selected?.id)
      throw new Error('Room no longer belongs to the selected property.')
    selectedRoom = found
    const r = found,
      pid = propertyId(),
      ref = { propertyId: pid, roomId: r.id, expectedRevision: r.revision }
    roomDetail.replaceChildren(
      node('h3', 'Room ' + r.code),
      node('p', 'Housekeeping: ' + r.housekeeping + ' · ' + (r.occupied ? 'occupied' : 'vacant')),
    )
    stayCreate.hidden =
      !r.active ||
      !access?.permissions.includes('stays.write') ||
      !access.permissions.includes('stays.read')
    const type = types.find((x) => x.id === r.roomTypeId)
    if (type) roomDetail.append(node('p', type.title + ' · IDR ' + type.nightlyRate + '/night'))
    if (!r.occupied && access?.permissions.includes('housekeeping.write')) {
      if (r.housekeeping === 'dirty')
        roomDetail.append(
          button('Mark room clean', () =>
            mutate({
              operation: 'room.housekeeping',
              ...ref,
              state: 'clean',
              reason: 'Operator completed cleaning',
            }),
          ),
        )
      else
        roomDetail.append(
          button('Mark room dirty', () =>
            mutate({
              operation: 'room.housekeeping',
              ...ref,
              state: 'dirty',
              reason: 'Operator requested cleaning',
            }),
          ),
        )
    }
    if (!r.occupied && r.housekeeping === 'clean' && access?.permissions.includes('rooms.inspect'))
      roomDetail.append(
        button('Inspect room', () =>
          mutate({
            operation: 'room.housekeeping',
            ...ref,
            state: 'inspected',
            reason: 'Operator inspected the cleaned room',
          }),
        ),
      )
    if (access?.permissions.includes('rooms.write'))
      roomDetail.append(
        button(r.active ? 'Disable room' : 'Enable room', () =>
          mutate({
            operation: 'room.set-active',
            ...ref,
            active: !r.active,
            reason: 'Operator changed room availability',
          }),
        ),
      )
  }
  async function loadStays() {
    stayList.replaceChildren()
    if (!access?.permissions.includes('stays.read')) return
    const result = page(staySchema).parse(await request('/stays?' + query(stayAfter)))
    if (disposed) return
    for (const item of result.items) {
      const row = node('li')
      row.append(
        button(
          item.guestName +
            ' · ' +
            item.arrival +
            ' → ' +
            item.departure +
            ' · ' +
            item.effectiveState +
            ' · IDR ' +
            item.totalEstimate,
          () => showStay(item.id),
        ),
      )
      stayList.append(row)
    }
    next(stayNext, result.next)
  }
  async function showStay(id: string) {
    const detail = stayDetailSchema.parse(await request('/stays/' + id))
    if (disposed) return
    selectedStay = id
    const item = detail.stay,
      ref = { propertyId: item.propertyId, id: item.id, expectedRevision: item.revision }
    await showRoom(item.roomId)
    if (disposed) return
    stayDetail.replaceChildren(
      node('h3', item.guestName),
      node('p', 'Stay: ' + item.effectiveState + ' · ' + item.arrival + ' → ' + item.departure),
      node('p', item.nights + ' nights · accommodation estimate IDR ' + item.totalEstimate),
    )
    const nights = node('ul')
    for (const night of detail.nights)
      nights.append(node('li', night.date + ' · IDR ' + night.rate))
    stayDetail.append(nights)
    if (!access?.permissions.includes('stays.write')) return
    if (item.effectiveState === 'held')
      stayDetail.append(button('Confirm stay', () => mutate({ operation: 'stay.confirm', ...ref })))
    if (item.state === 'confirmed')
      stayDetail.append(
        button('Check in guest', () => mutate({ operation: 'stay.check-in', ...ref })),
      )
    if (item.state === 'in_house')
      stayDetail.append(
        button('Check out guest', () => mutate({ operation: 'stay.check-out', ...ref })),
      )
    if (item.effectiveState === 'held' || item.state === 'confirmed') {
      const form = node('form'),
        reason = field(form, 'reason', 'Cancellation reason', 'text', '', 2000)
      submit(form, 'Cancel stay', () => ({
        operation: 'stay.cancel',
        ...ref,
        reason: reason.value,
      }))
      stayDetail.append(form)
    }
  }
  async function reload(id?: string) {
    const roomId = selected?.id === id ? selectedRoom?.id : undefined,
      stayId = selected?.id === id ? selectedStay : null
    await loadProperties()
    if (!disposed && id) {
      await selectProperty(id)
      if (roomId) await showRoom(roomId)
      if (stayId) await showStay(stayId)
    }
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
    access = accommodationAccessSchema.parse(await request('/access'))
    if (disposed) return
    propertyCreate.hidden = !access.permissions.includes('properties.write')
    await loadProperties()
  })
  return {
    ready,
    refresh: () => perform(() => reload(selected?.id)),
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      selected = null
      selectedRoom = null
      selectedStay = null
      access = null
      types = []
      host.replaceChildren()
    },
  }
}
