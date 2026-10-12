import { z } from 'zod'
import {
  attendanceAccessSchema,
  attendanceReceiptSchema,
  attendanceCommandStatusSchema,
  sessionSchema,
  sessionDetailSchema,
  type AttendanceAccess,
  type AttendanceMutation,
  type RosterRole,
  type CheckInMethod,
} from './contracts.js'

export function mountAttendanceDashboard(
  host: HTMLElement,
  options: { apiBase: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument
  const transport = options.transport ?? fetch
  const lifetime = new AbortController()
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const n = document.createElement(tag)
    n.textContent = text
    return n
  }

  let disposed = false
  let busy = false
  let access: AttendanceAccess | null = null
  let pending: AttendanceMutation | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.attendanceStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.attendanceCreate = ''
  list.dataset.attendanceList = ''
  detail.dataset.attendanceDetail = ''
  recovery.dataset.attendanceRecovery = ''
  recovery.hidden = true
  createForm.hidden = true

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
    required = false,
  ) => {
    const labelEl = node('label', label)
    const input = document.createElement('input')
    input.type = type
    input.name = name
    input.required = required
    labelEl.appendChild(input)
    form.appendChild(labelEl)
    return input
  }

  const selectField = <T extends string>(
    form: HTMLElement,
    name: string,
    label: string,
    optionsList: readonly T[],
    defaultValue?: T,
  ) => {
    const labelEl = node('label', label)
    const select = document.createElement('select')
    select.name = name
    for (const opt of optionsList) {
      const option = document.createElement('option')
      option.value = opt
      option.textContent = opt
      if (opt === defaultValue) option.selected = true
      select.appendChild(option)
    }
    labelEl.appendChild(select)
    form.appendChild(labelEl)
    return select
  }

  const createTitle = field(createForm, 'title', 'Session Title', 'text', true)
  const createCode = field(createForm, 'sessionCode', 'Session Code', 'text', true)
  const createCapacity = field(createForm, 'capacity', 'Max Capacity (seats)', 'number', false)
  createCapacity.value = '30'
  const createLocation = field(createForm, 'location', 'Location / Venue', 'text', false)
  createLocation.value = 'Main Auditorium'

  const now = new Date()
  const startDefault = new Date(now.getTime() + 86400000).toISOString()
  const endDefault = new Date(now.getTime() + 90000000).toISOString()
  const createStart = field(createForm, 'startTime', 'Start Time (ISO)', 'text', true)
  createStart.value = startDefault
  const createEnd = field(createForm, 'endTime', 'End Time (ISO)', 'text', true)
  createEnd.value = endDefault

  const createSubmit = button('Create Session', async () => {
    const titleVal = createTitle.value.trim()
    const codeVal = createCode.value.trim()
    if (!titleVal || !codeVal) return

    const commandId = crypto.randomUUID()
    const mutation: AttendanceMutation = {
      type: 'create_session',
      commandId,
      title: titleVal,
      sessionCode: codeVal,
      capacity: createCapacity.value ? Number(createCapacity.value) : null,
      startTime: createStart.value.trim(),
      endTime: createEnd.value.trim(),
      location: createLocation.value.trim() || null,
    }

    await sendCommand(mutation)
    if (!uncertain) {
      createTitle.value = ''
      createCode.value = ''
      await loadList()
    }
  })
  createForm.appendChild(createSubmit)

  const requeryBtn = button('Re-query status', async () => {
    if (!pending) return
    let res: Response
    try {
      res = await transport(`${options.apiBase}/commands/${pending.commandId}`, {
        headers: { accept: 'application/json' },
        signal: lifetime.signal,
      })
    } catch {
      status.textContent =
        'Network interrupted while executing recovery query; outcome still unknown'
      return
    }
    if (!res.ok) {
      status.textContent = `Recovery query failed: HTTP ${res.status}`
      return
    }
    let payload
    try {
      payload = attendanceCommandStatusSchema.parse(await res.json())
    } catch {
      status.textContent = 'Invalid receipt received during recovery; outcome remains unknown'
      return
    }
    if (payload.status !== 'recorded') {
      status.textContent = 'Command status is still unknown on the server'
      return
    }
    if (payload.receipt.commandId !== pending.commandId) {
      status.textContent =
        'Recovery receipt command ID mismatch; pending command is still unresolved'
      return
    }
    uncertain = false
    pending = null
    renderRecovery()
    status.textContent = `Command recovered with recorded receipt (${payload.receipt.status})`
    await loadList()
  })

  const retryBtn = button('Retry same request', async () => {
    if (!pending) return
    await sendCommand(pending)
    if (!uncertain) {
      await loadList()
    }
  })

  recovery.appendChild(
    node('strong', 'Operation in an uncertain state. A previous write may have been recorded.'),
  )
  recovery.appendChild(requeryBtn)
  recovery.appendChild(retryBtn)

  const renderRecovery = () => {
    recovery.hidden = !uncertain
  }

  const sendCommand = async (mutation: AttendanceMutation) => {
    if (uncertain && pending?.commandId !== mutation.commandId) {
      status.textContent =
        'Uncertain command outcome; reconcile the pending command before submitting new requests.'
      return
    }
    pending = mutation
    uncertain = true
    renderRecovery()

    let res: Response
    try {
      res = await transport(`${options.apiBase}/commands`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', accept: 'application/json' },
        body: JSON.stringify(mutation),
        signal: lifetime.signal,
      })
    } catch {
      status.textContent = 'Network interrupted while executing command; verify before retrying'
      return
    }

    if (!res.ok) {
      if (res.status === 400) {
        pending = null
        uncertain = false
      }
      renderRecovery()
      status.textContent = `Command failed: HTTP ${res.status}`
      return
    }

    let payload
    try {
      payload = attendanceReceiptSchema.parse(await res.json())
    } catch {
      renderRecovery()
      status.textContent =
        'Invalid or truncated command receipt. Reconcile or retry before proceeding.'
      return
    }
    if (payload.commandId !== mutation.commandId) {
      renderRecovery()
      status.textContent =
        'Invalid or truncated command receipt. Reconcile or retry before proceeding.'
      return
    }

    uncertain = false
    pending = null
    renderRecovery()
    status.textContent = `Command committed: ${payload.action} (Status: ${payload.status})`
  }

  const perform = async (work: () => Promise<void>) => {
    if (busy || disposed) return
    busy = true
    host.dataset.busy = 'true'
    status.textContent = 'Working...'
    try {
      await work()
    } catch (error) {
      if (!uncertain) {
        status.textContent = (error as Error).message
      }
    } finally {
      busy = false
      delete host.dataset.busy
    }
  }

  const loadAccess = async () => {
    const res = await transport(`${options.apiBase}/access`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Access check failed: HTTP ${res.status}`)
    access = attendanceAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('attendance.write')
  }

  const loadList = async () => {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/sessions?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load sessions: HTTP ${res.status}`)
    const items = z.array(sessionSchema).parse(await res.json())
    for (const sess of items) {
      const li = node('li')
      li.dataset.attendanceItem = sess.id
      const title = node(
        'strong',
        `${sess.title} (${sess.sessionCode}) [${sess.status.toUpperCase()}]`,
      )
      const capText =
        sess.capacity !== null ? `${sess.enrolledCount}/${sess.capacity}` : `${sess.enrolledCount}`
      const meta = node(
        'span',
        ` - Enrolled: ${capText} | Attended: ${sess.attendedCount} | Loc: ${sess.location ?? 'TBD'}`,
      )
      const viewBtn = button('View Session', async () => {
        await loadDetail(sess.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
  }

  const loadDetail = async (id: string) => {
    detail.replaceChildren()
    const res = await transport(`${options.apiBase}/sessions/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load session detail: HTTP ${res.status}`)
    const record = sessionDetailSchema.parse(await res.json())

    const h3 = node('h3', `${record.session.title} (${record.session.sessionCode})`)
    detail.appendChild(h3)

    const dl = node('dl')
    const addProp = (term: string, val: string) => {
      dl.appendChild(node('dt', term))
      dl.appendChild(node('dd', val))
    }
    addProp('Status', record.session.status)
    addProp(
      'Capacity',
      record.session.capacity !== null ? String(record.session.capacity) : 'Unlimited',
    )
    addProp('Enrolled', String(record.session.enrolledCount))
    addProp('Attended', String(record.session.attendedCount))
    addProp('Start Time', record.session.startTime)
    addProp('End Time', record.session.endTime)
    addProp('Location', record.session.location ?? 'None')
    addProp('Revision', String(record.session.revision))
    detail.appendChild(dl)

    // Status change controls
    if (access?.permissions.includes('attendance.write')) {
      const statusForm = node('form')
      statusForm.appendChild(node('h4', 'Session Status Lifecycle'))
      const targetSel = selectField(
        statusForm,
        'targetStatus',
        'Target Status',
        ['scheduled', 'in_progress', 'completed', 'canceled'] as const,
        'in_progress',
      )
      const reasonField = field(statusForm, 'reason', 'Reason', 'text', true)
      reasonField.value = 'Operator updated session state'

      const statusSubmit = button('Apply Session Status', async () => {
        const cmdId = crypto.randomUUID()
        const mut: AttendanceMutation = {
          type: 'change_session_status',
          commandId: cmdId,
          sessionId: record.session.id,
          expectedRevision: record.session.revision,
          targetStatus: targetSel.value as 'scheduled' | 'in_progress' | 'completed' | 'canceled',
          reason: reasonField.value.trim(),
        }
        await sendCommand(mut)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      statusForm.appendChild(statusSubmit)
      detail.appendChild(statusForm)
    }

    // Participant Registration Form
    if (access?.permissions.includes('attendance.write')) {
      const regForm = node('form')
      regForm.dataset.attendanceRegister = ''
      regForm.appendChild(node('h4', 'Register Participant'))
      const regContact = field(regForm, 'contactId', 'Contact UUID', 'text', true)
      const regRole = selectField(
        regForm,
        'role',
        'Role',
        ['attendee', 'instructor', 'speaker', 'staff'] as const,
        'attendee',
      )
      const regTicket = field(regForm, 'ticketCode', 'Ticket / Badge Code', 'text', true)
      regTicket.value = `TCK-${Math.floor(100000 + Math.random() * 900000)}`

      const regSubmit = button('Register', async () => {
        const cid = regContact.value.trim()
        const tcode = regTicket.value.trim()
        if (!cid || !tcode) return

        const cmdId = crypto.randomUUID()
        const mut: AttendanceMutation = {
          type: 'register_participant',
          commandId: cmdId,
          sessionId: record.session.id,
          contactId: cid,
          role: regRole.value as RosterRole,
          ticketCode: tcode,
        }
        await sendCommand(mut)
        if (!uncertain) {
          regContact.value = ''
          regTicket.value = `TCK-${Math.floor(100000 + Math.random() * 900000)}`
          await loadDetail(id)
          await loadList()
        }
      })
      regForm.appendChild(regSubmit)
      detail.appendChild(regForm)
    }

    // Roster Table & Check-in Controls
    const rosterSection = node('div')
    rosterSection.appendChild(node('h4', 'Roster & Check-in Control'))
    const rosterList = node('ul')
    rosterList.dataset.attendanceRoster = ''

    for (const entry of record.roster) {
      const li = node('li')
      li.dataset.rosterItem = entry.id
      const info = node(
        'span',
        `[${entry.role.toUpperCase()}] Contact: ${entry.contactId} | Ticket: ${entry.ticketCode} | Status: ${entry.status.toUpperCase()}`,
      )
      li.appendChild(info)

      if (
        entry.status !== 'attended' &&
        entry.status !== 'canceled' &&
        access?.permissions.includes('attendance.checkin')
      ) {
        const checkInBtn = button('Check In (Scan)', async () => {
          const cmdId = crypto.randomUUID()
          const mut: AttendanceMutation = {
            type: 'record_check_in',
            commandId: cmdId,
            sessionId: record.session.id,
            rosterId: entry.id,
            method: 'qr_scan' as CheckInMethod,
            evidenceRef: 'Terminal Scanner #01',
            idempotencyKey: `scan-${entry.id}-${Date.now()}`,
          }
          await sendCommand(mut)
          if (!uncertain) {
            await loadDetail(id)
            await loadList()
          }
        })
        checkInBtn.dataset.attendanceCheckin = ''
        li.appendChild(checkInBtn)
      }

      rosterList.appendChild(li)
    }
    rosterSection.appendChild(rosterList)
    detail.appendChild(rosterSection)
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Attendance dashboard ready.'
  })

  return {
    ready,
    destroy() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
