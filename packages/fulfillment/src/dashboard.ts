import { z } from 'zod'
import {
  fulfillmentAccessSchema,
  fulfillmentReceiptSchema,
  fulfillmentCommandStatusSchema,
  jobSchema,
  jobDetailSchema,
  type FulfillmentAccess,
  type FulfillmentMutation,
  type JobKind,
} from './contracts.js'

const JOB_STATUS_TARGETS = ['released', 'in_progress', 'completed', 'canceled'] as const

export function mountFulfillmentDashboard(
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
  let access: FulfillmentAccess | null = null
  let pending: FulfillmentMutation | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.fulfillmentStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.fulfillmentCreate = ''
  list.dataset.fulfillmentList = ''
  detail.dataset.fulfillmentDetail = ''
  recovery.dataset.fulfillmentRecovery = ''
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

  const createCode = field(createForm, 'jobCode', 'Job / Shipment Code', 'text', true)
  createCode.value = 'SHP-'
  const createKind = selectField(
    createForm,
    'kind',
    'Work Order Kind',
    ['dispatch', 'production'] as const,
    'dispatch',
  )
  const createTitle = field(createForm, 'title', 'Title', 'text', true)
  createTitle.value = 'Warehouse dispatch run'
  const createOwner = field(createForm, 'ownerRef', 'Owner / Client Reference', 'text', false)
  createOwner.value = 'client-alpha'

  const createSubmit = button('Create Job', async () => {
    const codeVal = createCode.value.trim()
    const titleVal = createTitle.value.trim()
    if (!codeVal || !titleVal) return

    const mutation: FulfillmentMutation = {
      type: 'create_job',
      commandId: crypto.randomUUID(),
      jobCode: codeVal,
      kind: createKind.value as JobKind,
      title: titleVal,
      ownerRef: createOwner.value.trim() || null,
    }

    await sendCommand(mutation)
    if (!uncertain) {
      createCode.value = 'SHP-'
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
      payload = fulfillmentCommandStatusSchema.parse(await res.json())
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

  const sendCommand = async (mutation: FulfillmentMutation) => {
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
      payload = fulfillmentReceiptSchema.parse(await res.json())
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
    access = fulfillmentAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('fulfillment.write')
  }

  const loadList = async () => {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/jobs?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load jobs: HTTP ${res.status}`)
    const items = z.array(jobSchema).parse(await res.json())
    for (const job of items) {
      const li = node('li')
      li.dataset.fulfillmentItem = job.id
      const title = node('strong', `${job.title} (${job.jobCode}) [${job.status.toUpperCase()}]`)
      const meta = node(
        'span',
        ` - Kind: ${job.kind} | Consumed: ${job.consumedCount} | Produced: ${job.producedCount}` +
          ` | Scrap: ${job.scrapCount} | Unreconciled: ${job.unreconciledCount}` +
          ` | Owner: ${job.ownerRef ?? 'internal'}`,
      )
      const viewBtn = button('View Job', async () => {
        await loadDetail(job.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
  }

  const loadDetail = async (id: string) => {
    detail.replaceChildren()
    const res = await transport(`${options.apiBase}/jobs/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load job detail: HTTP ${res.status}`)
    const record = jobDetailSchema.parse(await res.json())

    const h3 = node('h3', `${record.job.title} (${record.job.jobCode})`)
    detail.appendChild(h3)

    const dl = node('dl')
    const addProp = (term: string, val: string) => {
      dl.appendChild(node('dt', term))
      dl.appendChild(node('dd', val))
    }
    addProp('Status', record.job.status)
    addProp('Kind', record.job.kind)
    addProp('Owner / Client', record.job.ownerRef ?? 'internal')
    addProp('Consumed', String(record.job.consumedCount))
    addProp('Produced', String(record.job.producedCount))
    addProp('Scrap', String(record.job.scrapCount))
    addProp('Unreconciled findings', String(record.job.unreconciledCount))
    addProp('Revision', String(record.job.revision))
    detail.appendChild(dl)

    if (access?.permissions.includes('fulfillment.write')) {
      const statusForm = node('form')
      statusForm.dataset.fulfillmentStatusForm = ''
      statusForm.appendChild(node('h4', 'Job Lifecycle'))
      const targetSel = selectField(
        statusForm,
        'targetStatus',
        'Target Status',
        JOB_STATUS_TARGETS,
        'released',
      )
      const reasonField = field(statusForm, 'reason', 'Reason', 'text', true)
      reasonField.value = 'Operator advanced the job'

      const statusSubmit = button('Apply Job Status', async () => {
        const mutation: FulfillmentMutation = {
          type: 'change_job_status',
          commandId: crypto.randomUUID(),
          jobId: record.job.id,
          expectedRevision: record.job.revision,
          targetStatus: targetSel.value as (typeof JOB_STATUS_TARGETS)[number],
          reason: reasonField.value.trim(),
        }
        await sendCommand(mutation)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      statusForm.appendChild(statusSubmit)
      detail.appendChild(statusForm)

      const lineForm = node('form')
      lineForm.dataset.fulfillmentLine = ''
      lineForm.appendChild(node('h4', 'Record Material / Output Line'))
      const lineDirection = selectField(
        lineForm,
        'direction',
        'Direction',
        ['consume', 'produce'] as const,
        'consume',
      )
      const lineItem = field(lineForm, 'itemRef', 'Item Reference', 'text', true)
      lineItem.value = 'TEAK-PLANK-18'
      const lineQuantity = field(lineForm, 'quantity', 'Quantity', 'number', true)
      lineQuantity.value = '4'
      const lineDisposition = selectField(
        lineForm,
        'disposition',
        'Disposition',
        ['normal', 'scrap'] as const,
        'normal',
      )

      const lineSubmit = button('Record Line', async () => {
        const itemVal = lineItem.value.trim()
        if (!itemVal || !lineQuantity.value) return
        const mutation: FulfillmentMutation = {
          type: 'record_line',
          commandId: crypto.randomUUID(),
          jobId: record.job.id,
          direction: lineDirection.value as 'consume' | 'produce',
          itemRef: itemVal,
          quantity: Number(lineQuantity.value),
          disposition: lineDisposition.value as 'normal' | 'scrap',
          evidenceRef: null,
        }
        await sendCommand(mutation)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      lineForm.appendChild(lineSubmit)
      detail.appendChild(lineForm)

      const findingForm = node('form')
      findingForm.dataset.fulfillmentFinding = ''
      findingForm.appendChild(node('h4', 'Record Finding'))
      const findingCategory = selectField(
        findingForm,
        'category',
        'Category',
        ['condition', 'quantity', 'delivery'] as const,
        'delivery',
      )
      const findingDisposition = selectField(
        findingForm,
        'disposition',
        'Disposition',
        ['accepted', 'partial', 'rejected', 'lost', 'damaged'] as const,
        'partial',
      )
      const findingReason = field(findingForm, 'reason', 'Reason', 'text', true)
      findingReason.value = 'Carrier reported short delivery'
      const findingReconciles = field(
        findingForm,
        'reconcilesId',
        'Reconciles Finding UUID',
        'text',
        false,
      )

      const findingSubmit = button('Record Finding', async () => {
        const reasonVal = findingReason.value.trim()
        if (!reasonVal) return
        const mutation: FulfillmentMutation = {
          type: 'record_finding',
          commandId: crypto.randomUUID(),
          jobId: record.job.id,
          category: findingCategory.value as 'condition' | 'quantity' | 'delivery',
          disposition: findingDisposition.value as
            | 'accepted'
            | 'partial'
            | 'rejected'
            | 'lost'
            | 'damaged',
          quantity: null,
          reason: reasonVal,
          evidenceRef: null,
          reconcilesId: findingReconciles.value.trim() || null,
        }
        await sendCommand(mutation)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      findingForm.appendChild(findingSubmit)
      detail.appendChild(findingForm)

      const custodyForm = node('form')
      custodyForm.dataset.fulfillmentCustody = ''
      custodyForm.appendChild(node('h4', 'Record Custody Event'))
      const custodyEvent = selectField(
        custodyForm,
        'event',
        'Custody Event',
        ['received', 'stored', 'picked', 'packed', 'dispatched', 'delivered', 'returned'] as const,
        'received',
      )
      const custodyFrom = field(custodyForm, 'fromRef', 'From', 'text', false)
      custodyFrom.value = 'warehouse-bay-3'
      const custodyTo = field(custodyForm, 'toRef', 'To', 'text', false)
      custodyTo.value = 'carrier-nusantara'
      const custodyEvidence = field(custodyForm, 'evidenceRef', 'Evidence Ref', 'text', false)
      custodyEvidence.value = 'DO-77120'

      const custodySubmit = button('Record Custody', async () => {
        const mutation: FulfillmentMutation = {
          type: 'record_custody',
          commandId: crypto.randomUUID(),
          jobId: record.job.id,
          event: custodyEvent.value as
            | 'received'
            | 'stored'
            | 'picked'
            | 'packed'
            | 'dispatched'
            | 'delivered'
            | 'returned',
          fromRef: custodyFrom.value.trim() || null,
          toRef: custodyTo.value.trim() || null,
          evidenceRef: custodyEvidence.value.trim() || null,
          note: null,
        }
        await sendCommand(mutation)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      custodyForm.appendChild(custodySubmit)
      detail.appendChild(custodyForm)
    }

    const linesSection = node('div')
    linesSection.appendChild(node('h4', 'Material / Output Lines'))
    const linesList = node('ul')
    linesList.dataset.fulfillmentLines = ''
    for (const line of record.lines) {
      const li = node('li')
      li.dataset.fulfillmentLineItem = line.id
      li.textContent =
        `[${line.direction.toUpperCase()}/${line.disposition.toUpperCase()}] ` +
        `${line.itemRef} x${line.quantity} - ${line.recordedBy} @ ${line.recordedAt}`
      linesList.appendChild(li)
    }
    linesSection.appendChild(linesList)
    detail.appendChild(linesSection)

    const findingsSection = node('div')
    findingsSection.appendChild(node('h4', 'Findings & Reconciliation'))
    const findingsList = node('ul')
    findingsList.dataset.fulfillmentFindings = ''
    for (const finding of record.findings) {
      const li = node('li')
      li.dataset.fulfillmentFindingItem = finding.id
      li.textContent =
        `[${finding.category.toUpperCase()}/${finding.disposition.toUpperCase()}] ` +
        `${finding.reason}` +
        (finding.reconcilesId ? ` (reconciles ${finding.reconcilesId})` : '')
      findingsList.appendChild(li)
    }
    findingsSection.appendChild(findingsList)
    detail.appendChild(findingsSection)

    const custodySection = node('div')
    custodySection.appendChild(node('h4', 'Custody Chain'))
    const custodyList = node('ul')
    custodyList.dataset.fulfillmentCustodyList = ''
    for (const event of record.custody) {
      const li = node('li')
      li.dataset.fulfillmentCustodyItem = event.id
      li.textContent =
        `${event.event.toUpperCase()} from ${event.fromRef ?? '?'} to ${event.toRef ?? '?'}` +
        (event.evidenceRef ? ` [${event.evidenceRef}]` : '')
      custodyList.appendChild(li)
    }
    custodySection.appendChild(custodyList)
    detail.appendChild(custodySection)
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Fulfillment dashboard ready.'
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
