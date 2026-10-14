import { z } from 'zod'
import {
  fieldAccessSchema,
  fieldReceiptSchema,
  fieldCommandStatusSchema,
  fieldJobSchema,
  fieldJobDetailSchema,
  type FieldAccess,
  type FieldCommand,
  type FieldCommandStatus,
} from './contracts.js'

const VISIT_OUTCOMES_UI = ['completed', 'partial', 'no_access'] as const
const WORK_LINE_KINDS = ['labour', 'part'] as const

/**
 * A recovery query has to name the job it is asking about, and creation is the
 * one command that does not carry a `jobId` yet: it is asking the server to
 * mint one. Reading it from the nested `job` keeps the same helper working for
 * every action in the union.
 */
const fieldJobIdOf = (command: FieldCommand): string | null =>
  command.action === 'create_job' ? command.job.jobId : command.jobId

export function mountFieldServicesDashboard(
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
  let access: FieldAccess | null = null
  let pending: FieldCommand | null = null
  let pendingJobId: string | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.fieldStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.fieldCreate = ''
  list.dataset.fieldList = ''
  detail.dataset.fieldDetail = ''
  recovery.dataset.fieldRecovery = ''
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

  const createSummary = field(createForm, 'summary', 'Work Summary', 'text', true)
  createSummary.value = 'Replace failed HVAC compressor'
  const createCustomer = field(createForm, 'customerRef', 'Customer Reference', 'text', false)
  createCustomer.value = 'customer-andi'
  const createSite = field(createForm, 'siteRef', 'Site Reference', 'text', false)
  createSite.value = 'site-jakarta-77'
  const createPriority = selectField(
    createForm,
    'priority',
    'Priority',
    ['low', 'normal', 'high', 'urgent'] as const,
    'normal',
  )
  const createAppointment = field(
    createForm,
    'appointmentRef',
    'Scheduler Appointment Ref (required)',
    'text',
    true,
  )
  createAppointment.value = 'APPT-'
  const createAsset = field(createForm, 'assetRef', 'Asset Ref (optional)', 'text', false)

  createForm.appendChild(
    button('Create Dispatch Job', async () => {
      const summaryVal = createSummary.value.trim()
      const appointmentVal = createAppointment.value.trim()
      if (!summaryVal || !appointmentVal) return
      await sendCommand({
        action: 'create_job',
        commandId: crypto.randomUUID(),
        job: {
          jobId: `job-${crypto.randomUUID().slice(0, 8)}`,
          customerRef: createCustomer.value.trim() || null,
          siteRef: createSite.value.trim(),
          summary: summaryVal,
          priority: createPriority.value as 'low' | 'normal' | 'high' | 'urgent',
          appointmentRef: appointmentVal,
          assetRef: createAsset.value.trim() || null,
        },
      } as FieldCommand)
      if (!uncertain) {
        await loadList()
      }
    }),
  )

  const loadAccess = async () => {
    const res = await transport(`${options.apiBase}/access`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Access check failed: HTTP ${res.status}`)
    access = fieldAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('field.write')
  }

  const loadList = async () => {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/jobs?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load jobs: HTTP ${res.status}`)
    const items = z.array(fieldJobSchema).parse(await res.json())
    for (const job of items) {
      const li = node('li')
      li.dataset.fieldItem = job.jobId
      const title = node('strong', `${job.jobId} [${job.status.toUpperCase()}]`)
      const meta = node(
        'span',
        ` - ${job.summary} | Site: ${job.siteRef} | Priority: ${job.priority}` +
          ` | Appointment: ${job.appointmentRef} | Labour: ${job.laborHours}h` +
          ` | Parts: ${job.partsPieces} pieces | Revision: ${job.revision}`,
      )
      const viewBtn = button('View Job', async () => {
        await loadDetail(job.jobId)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
  }

  const loadDetail = async (jobId: string) => {
    detail.replaceChildren()
    const res = await transport(`${options.apiBase}/jobs/${jobId}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load job detail: HTTP ${res.status}`)
    const record = fieldJobDetailSchema.parse(await res.json())
    const job = record.job

    detail.appendChild(node('h3', `${job.jobId} (${job.status})`))

    const dl = node('dl')
    const addProp = (term: string, val: string) => {
      dl.appendChild(node('dt', term))
      dl.appendChild(node('dd', val))
    }
    addProp('Status', job.status)
    addProp('Customer', job.customerRef)
    addProp('Site', job.siteRef)
    addProp('Assigned', job.assignedPrincipal ?? 'unassigned')
    addProp('Appointment', job.appointmentRef)
    addProp('Asset', job.assetRef ?? 'none')
    addProp('Labour (hours)', String(job.laborHours))
    addProp('Parts (pieces)', String(job.partsPieces))
    addProp('Revision', String(job.revision))
    detail.appendChild(dl)

    const visitsSection = node('div')
    visitsSection.appendChild(node('h4', 'Site Visits'))
    const visitsList = node('ul')
    visitsList.dataset.fieldVisitList = ''
    for (const visit of record.visits) {
      const li = node('li')
      li.dataset.fieldVisitItem = visit.visitId
      li.textContent =
        `${visit.outcome.toUpperCase()} by ${visit.technicianRef} - ${visit.note}` +
        ` (${visit.visitedAt})`
      visitsList.appendChild(li)
    }
    visitsSection.appendChild(visitsList)
    detail.appendChild(visitsSection)

    const workLinesSection = node('div')
    workLinesSection.appendChild(node('h4', 'Work Lines (hours vs pieces)'))
    const workLinesList = node('ul')
    workLinesList.dataset.fieldWorkLineList = ''
    for (const line of record.workLines) {
      const li = node('li')
      li.dataset.fieldLineItem = line.lineId
      li.textContent =
        `${line.description} - ${line.quantity} ${line.unit}` +
        (line.partRef ? ` (${line.partRef})` : '')
      workLinesList.appendChild(li)
    }
    workLinesSection.appendChild(workLinesList)
    detail.appendChild(workLinesSection)

    const signoffSection = node('div')
    signoffSection.appendChild(node('h4', 'Customer Signoff'))
    if (record.signoff) {
      signoffSection.appendChild(
        node(
          'p',
          `${record.signoff.accepted ? 'ACCEPTED' : 'REJECTED'} by ` +
            `${record.signoff.signerRef}` +
            (record.signoff.comment ? ` - ${record.signoff.comment}` : ''),
        ),
      )
    } else {
      signoffSection.appendChild(node('p', 'No signoff recorded yet.'))
    }
    detail.appendChild(signoffSection)

    if (access?.permissions.includes('field.dispatch')) {
      const assignForm = node('form')
      assignForm.dataset.fieldAssign = ''
      assignForm.appendChild(node('h4', 'Assign Technician'))
      const assignField = field(assignForm, 'assignedPrincipal', 'Technician Ref', 'text', true)
      assignField.value = 'tech-14'
      assignForm.appendChild(
        button('Assign', async () => {
          const assignedPrincipal = assignField.value.trim()
          if (!assignedPrincipal) return
          await sendCommand({
            action: 'assign_job',
            commandId: crypto.randomUUID(),
            jobId: job.jobId,
            assignedPrincipal,
          })
          if (!uncertain) {
            await loadDetail(jobId)
            await loadList()
          }
        }),
      )
      detail.appendChild(assignForm)

      const visitForm = node('form')
      visitForm.dataset.fieldVisit = ''
      visitForm.appendChild(node('h4', 'Record Site Visit'))
      const visitOutcome = selectField(
        visitForm,
        'outcome',
        'Outcome',
        VISIT_OUTCOMES_UI,
        'completed',
      )
      const visitTechnician = field(visitForm, 'technicianRef', 'Technician Ref', 'text', true)
      visitTechnician.value = 'tech-14'
      const visitNote = field(visitForm, 'note', 'Visit Note', 'text', true)
      visitNote.value = 'Site secured, compressor removed'
      visitForm.appendChild(
        button('Record Visit', async () => {
          const note = visitNote.value.trim()
          const technicianRef = visitTechnician.value.trim()
          if (!note || !technicianRef) return
          await sendCommand({
            action: 'record_visit',
            commandId: crypto.randomUUID(),
            jobId: job.jobId,
            visit: {
              visitId: crypto.randomUUID(),
              outcome: visitOutcome.value as 'completed' | 'partial' | 'no_access',
              note,
              technicianRef,
            },
          })
          if (!uncertain) {
            await loadDetail(jobId)
            await loadList()
          }
        }),
      )
      detail.appendChild(visitForm)

      const lineForm = node('form')
      lineForm.dataset.fieldWorkLine = ''
      lineForm.appendChild(node('h4', 'Append Work Line'))
      const lineDescription = field(lineForm, 'description', 'Description', 'text', true)
      lineDescription.value = 'Compressor replacement labour'
      const lineQuantity = field(lineForm, 'quantity', 'Quantity', 'number', true)
      lineQuantity.value = '2'
      const lineKind = selectField(
        lineForm,
        'kind',
        'Kind (hours for labour, pieces for parts)',
        WORK_LINE_KINDS,
        'labour',
      )
      const linePartRef = field(lineForm, 'partRef', 'Part Ref (parts only)', 'text', false)
      lineForm.appendChild(
        button('Append Line', async () => {
          const description = lineDescription.value.trim()
          const quantity = Number(lineQuantity.value)
          const kind = lineKind.value as 'labour' | 'part'
          if (!description || !(quantity > 0)) return
          await sendCommand({
            action: 'append_work_line',
            commandId: crypto.randomUUID(),
            jobId: job.jobId,
            line: {
              lineId: crypto.randomUUID(),
              kind,
              description,
              quantity,
              unit: kind === 'labour' ? 'hours' : 'pieces',
              partRef: kind === 'part' ? linePartRef.value.trim() || null : null,
            },
          })
          if (!uncertain) {
            await loadDetail(jobId)
            await loadList()
          }
        }),
      )
      detail.appendChild(lineForm)

      const signoffForm = node('form')
      signoffForm.dataset.fieldSignoff = ''
      signoffForm.appendChild(node('h4', 'Record Customer Signoff'))
      const signoffSigner = field(signoffForm, 'signerRef', 'Signer Ref', 'text', true)
      signoffSigner.value = 'customer-andi'
      const signoffComment = field(signoffForm, 'comment', 'Comment', 'text', false)
      signoffComment.value = 'Customer accepted completed work'
      signoffForm.appendChild(
        button('Record Signoff', async () => {
          const signerRef = signoffSigner.value.trim()
          if (!signerRef) return
          await sendCommand({
            action: 'record_signoff',
            commandId: crypto.randomUUID(),
            jobId: job.jobId,
            signoff: {
              signoffId: crypto.randomUUID(),
              signerRef,
              accepted: true,
              comment: signoffComment.value.trim() || null,
            },
          })
          if (!uncertain) {
            await loadDetail(jobId)
            await loadList()
          }
        }),
      )
      detail.appendChild(signoffForm)

      const cancelForm = node('form')
      cancelForm.dataset.fieldCancel = ''
      cancelForm.appendChild(node('h4', 'Cancel Job'))
      const cancelReason = field(cancelForm, 'reason', 'Reason', 'text', true)
      cancelReason.value = 'Customer canceled the visit'
      cancelForm.appendChild(
        button('Cancel Job', async () => {
          const reason = cancelReason.value.trim()
          if (!reason) return
          await sendCommand({
            action: 'cancel_job',
            commandId: crypto.randomUUID(),
            jobId: job.jobId,
            reason,
          })
          if (!uncertain) {
            await loadDetail(jobId)
            await loadList()
          }
        }),
      )
      detail.appendChild(cancelForm)
    }
  }

  const requeryBtn = button('Requery last command', async () => {
    if (!pending) return
    let res: Response
    try {
      res = await transport(`${options.apiBase}/commands/${pending.commandId}`, {
        headers: { accept: 'application/json' },
        signal: lifetime.signal,
      })
    } catch {
      // A recovery query that cannot reach the server leaves the outcome
      // unknown, which is a different state from a command that failed: the
      // pending mutation must stay pending so the operator can query again.
      status.textContent =
        'Network interrupted while executing recovery query; outcome still unknown'
      return
    }
    let payload: FieldCommandStatus
    try {
      payload = fieldCommandStatusSchema.parse(await res.json())
    } catch {
      status.textContent = 'Invalid receipt received during recovery; outcome remains unknown'
      return
    }
    if (payload.status !== 'accepted') {
      status.textContent = 'Command status is still unknown on the server'
      return
    }
    if (pendingJobId !== null && payload.jobId !== pendingJobId) {
      status.textContent = 'Recovery receipt job ID mismatch; pending command is still unresolved'
      return
    }
    uncertain = false
    pending = null
    pendingJobId = null
    renderRecovery()
    status.textContent = `Command recovered with recorded receipt (${payload.status}, revision ${payload.revision})`
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

  const sendCommand = async (mutation: FieldCommand) => {
    if (uncertain && pending?.commandId !== mutation.commandId) {
      status.textContent =
        'Uncertain command outcome; reconcile the pending command before submitting new requests.'
      return
    }
    // The pending UUID and unknown flag are set before dispatch so that a
    // truncated body, read failure or schema mismatch can never open a new write.
    pending = mutation
    pendingJobId = fieldJobIdOf(mutation)
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
      payload = fieldReceiptSchema.parse(await res.json())
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

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Field services dashboard ready.'
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
