import { z } from 'zod'
import {
  laundryAccessSchema,
  laundryReceiptSchema,
  laundryCommandStatusSchema,
  orderSchema,
  orderDetailSchema,
  type LaundryAccess,
  type LaundryMutation,
  type OrderKind,
} from './contracts.js'

const ORDER_STATUS_TARGETS = ['processing', 'ready', 'returned', 'canceled'] as const
const PROCESS_EVENTS_UI = [
  'intake',
  'tag',
  'wash',
  'dry',
  'press',
  'quality_check',
  'ready',
  'handover',
] as const

export function mountLaundryDashboard(
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
  let access: LaundryAccess | null = null
  let pending: LaundryMutation | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.laundryStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.laundryCreate = ''
  list.dataset.laundryList = ''
  detail.dataset.laundryDetail = ''
  recovery.dataset.laundryRecovery = ''
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

  const createCode = field(createForm, 'orderCode', 'Order Code', 'text', true)
  createCode.value = 'LDR-'
  const createKind = selectField(
    createForm,
    'kind',
    'Order Kind',
    ['weight', 'item'] as const,
    'weight',
  )
  const createCustomer = field(createForm, 'customerRef', 'Customer Reference', 'text', false)
  createCustomer.value = 'customer-andi'
  const createCurrency = field(createForm, 'currency', 'Currency', 'text', true)
  createCurrency.value = 'IDR'
  const lineItem = field(createForm, 'itemRef', 'Service / Garment Item', 'text', true)
  lineItem.value = 'wash-fold-kg'
  const lineQuantity = field(createForm, 'quantity', 'Quantity', 'number', true)
  lineQuantity.value = '3000'
  const lineUnit = selectField(createForm, 'unit', 'Unit', ['piece', 'gram'] as const, 'gram')
  const linePrice = field(createForm, 'unitPriceMinor', 'Unit Price (minor)', 'number', true)
  linePrice.value = '12'

  const createSubmit = button('Create Order', async () => {
    const codeVal = createCode.value.trim()
    const itemVal = lineItem.value.trim()
    if (!codeVal || !itemVal || !lineQuantity.value || !linePrice.value) return

    const mutation: LaundryMutation = {
      type: 'create_order',
      commandId: crypto.randomUUID(),
      orderCode: codeVal,
      kind: createKind.value as OrderKind,
      customerRef: createCustomer.value.trim() || null,
      currency: createCurrency.value.trim().toUpperCase() || 'IDR',
      lines: [
        {
          itemRef: itemVal,
          quantity: Number(lineQuantity.value),
          unit: lineUnit.value as 'piece' | 'gram',
          unitPriceMinor: Number(linePrice.value),
        },
      ],
    }

    await sendCommand(mutation)
    if (!uncertain) {
      createCode.value = 'LDR-'
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
      payload = laundryCommandStatusSchema.parse(await res.json())
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

  const sendCommand = async (mutation: LaundryMutation) => {
    if (uncertain && pending?.commandId !== mutation.commandId) {
      status.textContent =
        'Uncertain command outcome; reconcile the pending command before submitting new requests.'
      return
    }
    // The pending UUID and unknown flag are set before dispatch so that a
    // truncated body, read failure or schema mismatch can never open a new write.
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
      payload = laundryReceiptSchema.parse(await res.json())
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
    access = laundryAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('laundry.write')
  }

  const loadList = async () => {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/orders?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load orders: HTTP ${res.status}`)
    const items = z.array(orderSchema).parse(await res.json())
    for (const order of items) {
      const li = node('li')
      li.dataset.laundryItem = order.id
      const title = node(
        'strong',
        `${order.orderCode} (${order.kind}) [${order.status.toUpperCase()}]`,
      )
      const meta = node(
        'span',
        ` - Total: ${order.totalMinor} ${order.currency} | Pieces: ${order.itemCount}` +
          ` | Grams: ${order.weightGrams} | Unreconciled: ${order.unreconciledCount}` +
          ` | Customer: ${order.customerRef ?? 'walk-in'}`,
      )
      const viewBtn = button('View Order', async () => {
        await loadDetail(order.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
  }

  const loadDetail = async (id: string) => {
    detail.replaceChildren()
    const res = await transport(`${options.apiBase}/orders/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load order detail: HTTP ${res.status}`)
    const record = orderDetailSchema.parse(await res.json())

    const h3 = node('h3', `${record.order.orderCode} (${record.order.kind})`)
    detail.appendChild(h3)

    const dl = node('dl')
    const addProp = (term: string, val: string) => {
      dl.appendChild(node('dt', term))
      dl.appendChild(node('dd', val))
    }
    addProp('Status', record.order.status)
    addProp('Kind', record.order.kind)
    addProp('Customer', record.order.customerRef ?? 'walk-in')
    addProp('Total (minor)', `${record.order.totalMinor} ${record.order.currency}`)
    addProp('Pieces', String(record.order.itemCount))
    addProp('Grams', String(record.order.weightGrams))
    addProp('Unreconciled findings', String(record.order.unreconciledCount))
    addProp('Revision', String(record.order.revision))
    detail.appendChild(dl)

    if (access?.permissions.includes('laundry.write')) {
      const statusForm = node('form')
      statusForm.dataset.laundryStatusForm = ''
      statusForm.appendChild(node('h4', 'Order Lifecycle'))
      const targetSel = selectField(
        statusForm,
        'targetStatus',
        'Target Status',
        ORDER_STATUS_TARGETS,
        'processing',
      )
      const reasonField = field(statusForm, 'reason', 'Reason', 'text', true)
      reasonField.value = 'Operator advanced the order'

      const statusSubmit = button('Apply Order Status', async () => {
        const mutation: LaundryMutation = {
          type: 'change_order_status',
          commandId: crypto.randomUUID(),
          orderId: record.order.id,
          expectedRevision: record.order.revision,
          targetStatus: targetSel.value as (typeof ORDER_STATUS_TARGETS)[number],
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

      const processForm = node('form')
      processForm.dataset.laundryProcess = ''
      processForm.appendChild(node('h4', 'Record Process Step'))
      const eventSel = selectField(
        processForm,
        'event',
        'Process Event',
        PROCESS_EVENTS_UI,
        'intake',
      )
      const processNote = field(processForm, 'note', 'Note', 'text', false)
      processNote.value = 'Intake counted at counter'
      const processEvidence = field(processForm, 'evidenceRef', 'Evidence Ref', 'text', false)
      processEvidence.value = 'TAG-0001'

      const processSubmit = button('Record Process Step', async () => {
        const mutation: LaundryMutation = {
          type: 'record_process',
          commandId: crypto.randomUUID(),
          orderId: record.order.id,
          event: eventSel.value as (typeof PROCESS_EVENTS_UI)[number],
          note: processNote.value.trim() || null,
          evidenceRef: processEvidence.value.trim() || null,
        }
        await sendCommand(mutation)
        if (!uncertain) {
          await loadDetail(id)
          await loadList()
        }
      })
      processForm.appendChild(processSubmit)
      detail.appendChild(processForm)

      const findingForm = node('form')
      findingForm.dataset.laundryFinding = ''
      findingForm.appendChild(node('h4', 'Record Quality Finding'))
      const findingCategory = selectField(
        findingForm,
        'category',
        'Category',
        ['stain', 'damage', 'missing', 'count'] as const,
        'stain',
      )
      const findingDisposition = selectField(
        findingForm,
        'disposition',
        'Disposition',
        ['accepted', 'partial', 'rejected'] as const,
        'partial',
      )
      const findingReason = field(findingForm, 'reason', 'Reason', 'text', true)
      findingReason.value = 'Ink stain remains after wash'
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
        const mutation: LaundryMutation = {
          type: 'record_finding',
          commandId: crypto.randomUUID(),
          orderId: record.order.id,
          category: findingCategory.value as 'stain' | 'damage' | 'missing' | 'count',
          disposition: findingDisposition.value as 'accepted' | 'partial' | 'rejected',
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
    }

    const linesSection = node('div')
    linesSection.appendChild(node('h4', 'Priced Lines (frozen at creation)'))
    const linesList = node('ul')
    linesList.dataset.laundryLines = ''
    for (const line of record.lines) {
      const li = node('li')
      li.dataset.laundryLineItem = line.id
      li.textContent =
        `${line.itemRef} x${line.quantity} ${line.unit} @ ${line.unitPriceMinor}` +
        ` = ${line.lineTotalMinor} (${line.recordedBy})`
      linesList.appendChild(li)
    }
    linesSection.appendChild(linesList)
    detail.appendChild(linesSection)

    const processSection = node('div')
    processSection.appendChild(node('h4', 'Processing Chain'))
    const processList = node('ul')
    processList.dataset.laundryProcessList = ''
    for (const event of record.process) {
      const li = node('li')
      li.dataset.laundryProcessItem = event.id
      li.textContent =
        `${event.event.toUpperCase()}${event.evidenceRef ? ` [${event.evidenceRef}]` : ''}` +
        (event.note ? ` - ${event.note}` : '')
      processList.appendChild(li)
    }
    processSection.appendChild(processList)
    detail.appendChild(processSection)

    const findingsSection = node('div')
    findingsSection.appendChild(node('h4', 'Quality Findings & Reconciliation'))
    const findingsList = node('ul')
    findingsList.dataset.laundryFindings = ''
    for (const finding of record.findings) {
      const li = node('li')
      li.dataset.laundryFindingItem = finding.id
      li.textContent =
        `[${finding.category.toUpperCase()}/${finding.disposition.toUpperCase()}] ${finding.reason}` +
        (finding.reconcilesId ? ` (reconciles ${finding.reconcilesId})` : '')
      findingsList.appendChild(li)
    }
    findingsSection.appendChild(findingsList)
    detail.appendChild(findingsSection)
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Laundry dashboard ready.'
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
