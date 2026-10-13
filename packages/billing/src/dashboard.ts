import { z } from 'zod'
import {
  billingAccessSchema,
  billingReceiptSchema,
  billingCommandStatusSchema,
  invoiceSchema,
  invoiceDetailSchema,
  INVOICE_STATUSES,
  PAYMENT_OUTCOMES,
  currencySchema,
} from './contracts.js'
import type { BillingMutation, InvoiceDetail } from './contracts.js'

/**
 * The billing dashboard.
 *
 * It shows what is owed and what has been proven, and it offers exactly the
 * writes a person may make. There is deliberately no control that marks an
 * invoice paid: an amount is only ever settled by a provider event, and this
 * surface's job in that story is to record an *attempt* — visibly, as an
 * attempt — and to display the settlement that arrived from elsewhere.
 */
export function mountBillingDashboard(
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
  let access: z.infer<typeof billingAccessSchema> | null = null
  let pending: BillingMutation | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.billingStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.billingCreate = ''
  list.dataset.billingList = ''
  detail.dataset.billingDetail = ''
  recovery.dataset.billingRecovery = ''
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

  const money = (minor: number) => `${(minor / 100).toFixed(2)}`

  const createCode = field(createForm, 'invoiceCode', 'Invoice Code', 'text', true)
  createCode.value = 'INV-'
  const createCustomer = field(createForm, 'customerRef', 'Customer Reference', 'text', true)
  const createCurrency = field(createForm, 'currency', 'Currency (ISO)', 'text', true)
  createCurrency.value = 'IDR'
  const createConsumer = field(createForm, 'consumerRef', 'Consumer Record Owner', 'text', true)
  createConsumer.value = 'clinics'
  const lineOneDescription = field(
    createForm,
    'line1Description',
    'Line 1 Description',
    'text',
    true,
  )
  const lineOneQuantity = field(createForm, 'line1Quantity', 'Line 1 Quantity', 'number', true)
  lineOneQuantity.value = '1'
  const lineOneUnit = field(
    createForm,
    'line1UnitMinor',
    'Line 1 Unit (minor units)',
    'number',
    true,
  )
  const lineTwoDescription = field(createForm, 'line2Description', 'Line 2 Description (optional)')
  const lineTwoQuantity = field(createForm, 'line2Quantity', 'Line 2 Quantity', 'number')
  const lineTwoUnit = field(createForm, 'line2UnitMinor', 'Line 2 Unit (minor units)', 'number')

  const createSubmit = button('Create Invoice', async () => {
    const lines = [
      {
        description: lineOneDescription.value.trim(),
        quantity: Number(lineOneQuantity.value),
        unitMinor: Number(lineOneUnit.value),
      },
    ]
    if (lineTwoDescription.value.trim()) {
      lines.push({
        description: lineTwoDescription.value.trim(),
        quantity: Number(lineTwoQuantity.value),
        unitMinor: Number(lineTwoUnit.value),
      })
    }
    await sendCommand({
      type: 'create_invoice',
      commandId: crypto.randomUUID(),
      invoiceCode: createCode.value.trim(),
      customerRef: createCustomer.value.trim(),
      currency: currencySchema.parse(createCurrency.value.trim().toUpperCase()),
      consumerRef: createConsumer.value.trim(),
      lines,
    })
    if (!uncertain) {
      createCode.value = 'INV-'
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
      payload = billingCommandStatusSchema.parse(await res.json())
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
    status.textContent = `Command committed: ${payload.receipt.action}`
    await loadList()
  })

  const retryBtn = button('Retry same request', async () => {
    if (!pending) return
    await sendCommand(pending)
  })

  const renderRecovery = () => {
    recovery.replaceChildren()
    recovery.appendChild(
      node('strong', 'Operation in an uncertain state. A previous write may have been recorded.'),
    )
    recovery.appendChild(requeryBtn)
    recovery.appendChild(retryBtn)
    recovery.appendChild(
      node(
        'p',
        'Do not create a new request until this one is reconciled. Retrying the same command ' +
          'UUID is always safe: an already-recorded command replays its original receipt.',
      ),
    )
    recovery.hidden = !uncertain
  }

  const sendCommand = async (mutation: BillingMutation) => {
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
      payload = billingReceiptSchema.parse(await res.json())
    } catch {
      renderRecovery()
      status.textContent =
        'Invalid or truncated command receipt. Reconcile or retry before proceeding'
      return
    }

    if (payload.commandId !== mutation.commandId) {
      renderRecovery()
      status.textContent = 'Command receipt ID mismatch. Pending command remains unresolved'
      return
    }

    pending = null
    uncertain = false
    renderRecovery()
    status.textContent = `Command committed: ${payload.action}`
    if (payload.invoiceId) await loadDetail(payload.invoiceId)
    await loadList()
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
    access = billingAccessSchema.parse(await res.json())
    const write = access.permissions.includes('billing.write')
    const settle = access.permissions.includes('billing.settle')
    createForm.hidden = !write
    host.dataset.canSettle = settle ? 'true' : 'false'
  }

  const loadList = async () => {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/invoices?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load invoices: HTTP ${res.status}`)
    const items = z.array(invoiceSchema).parse(await res.json())
    for (const invoice of items) {
      const li = node('li')
      li.dataset.billingItem = invoice.id
      const title = node(
        'strong',
        `${invoice.invoiceCode} — ${invoice.customerRef} [${invoice.status.toUpperCase()}]`,
      )
      const meta = node(
        'span',
        ` - ${invoice.currency} gross ${money(invoice.grossMinor)}` +
          ` | corrections ${money(invoice.correctionMinor)}` +
          ` | settled ${money(invoice.settledMinor)}` +
          ` | outstanding ${money(invoice.outstandingMinor)}`,
      )
      const viewBtn = button('View Invoice', async () => {
        await loadDetail(invoice.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
    if (!items.length) list.appendChild(node('li', 'No invoices yet.'))
  }

  const loadDetail = async (id: string) => {
    const res = await transport(`${options.apiBase}/invoices/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load invoice: HTTP ${res.status}`)
    const record = invoiceDetailSchema.parse(await res.json())
    renderDetail(record)
  }

  const renderDetail = (record: InvoiceDetail) => {
    const invoice = record.invoice
    detail.replaceChildren()
    detail.appendChild(node('h3', `${invoice.invoiceCode} — ${invoice.status.toUpperCase()}`))
    const dl = node('dl')
    const addProp = (term: string, value: string) => {
      dl.appendChild(node('dt', term))
      dl.appendChild(node('dd', value))
    }
    addProp('Customer', invoice.customerRef)
    addProp('Consumer Record Owner', invoice.consumerRef)
    addProp('Currency', invoice.currency)
    addProp('Gross total (minor units)', String(invoice.grossMinor))
    addProp('Corrections (minor units)', String(invoice.correctionMinor))
    addProp('Settled (minor units)', String(invoice.settledMinor))
    addProp('Outstanding (minor units)', String(invoice.outstandingMinor))
    addProp('Revision', String(invoice.revision))
    detail.appendChild(dl)

    const linesSection = node('div')
    linesSection.appendChild(node('h4', 'Priced snapshot'))
    const linesList = node('ul')
    linesList.dataset.billingLines = ''
    for (const line of record.lines) {
      const li = node('li')
      li.dataset.billingLineItem = line.id
      li.textContent =
        `${line.description} x${line.quantity} @ ${money(line.unitMinor)} ` +
        `= ${money(line.lineTotalMinor)} — ${line.recordedBy}`
      linesList.appendChild(li)
    }
    linesSection.appendChild(linesList)
    detail.appendChild(linesSection)

    const correctionSection = node('div')
    correctionSection.appendChild(node('h4', 'Corrections (appended, never edited)'))
    const correctionList = node('ul')
    correctionList.dataset.billingCorrections = ''
    for (const correction of record.corrections) {
      const li = node('li')
      li.dataset.billingCorrectionItem = correction.id
      li.textContent =
        `${correction.amountMinor > 0 ? 'debit' : 'credit'} ` +
        `${money(Math.abs(correction.amountMinor))} — ${correction.reason}` +
        (correction.reversesId ? ` (reverses ${correction.reversesId})` : '')
      correctionList.appendChild(li)
    }
    if (!record.corrections.length) correctionList.appendChild(node('li', 'None.'))
    correctionSection.appendChild(correctionList)
    detail.appendChild(correctionSection)

    const attemptSection = node('div')
    attemptSection.appendChild(node('h4', 'Payment attempts (intent only — never a settlement)'))
    const attemptList = node('ul')
    attemptList.dataset.billingAttempts = ''
    for (const attempt of record.attempts) {
      const li = node('li')
      li.dataset.billingAttemptItem = attempt.id
      li.textContent =
        `${money(attempt.amountMinor)} ${attempt.currency} attempted` +
        (attempt.providerRef ? ` via ${attempt.providerRef}` : '') +
        ' — no result recorded from this request'
      attemptList.appendChild(li)
    }
    if (!record.attempts.length) attemptList.appendChild(node('li', 'None.'))
    attemptSection.appendChild(attemptList)
    detail.appendChild(attemptSection)

    const eventSection = node('div')
    eventSection.appendChild(node('h4', 'Provider events (the only settlement evidence)'))
    const eventList = node('ul')
    eventList.dataset.billingEvents = ''
    for (const event of record.events) {
      const li = node('li')
      li.dataset.billingEventItem = event.id
      li.textContent =
        `${event.outcome.toUpperCase()} ${money(event.amountMinor)} ${event.currency} ` +
        `from ${event.providerId} [${event.providerEventId}] verified ${event.verifiedAt}`
      eventList.appendChild(li)
    }
    if (!record.events.length) eventList.appendChild(node('li', 'None.'))
    eventSection.appendChild(eventList)
    detail.appendChild(eventSection)

    const canWrite = !!access?.permissions.includes('billing.write')
    const canSettle = !!access?.permissions.includes('billing.settle')

    const issueForm = node('form')
    issueForm.dataset.billingIssueForm = ''
    issueForm.appendChild(node('h4', 'Issue invoice (freezes the priced snapshot)'))
    const issueReason = field(issueForm, 'reason', 'Reason', 'text', true)
    issueReason.value = 'Work approved and billed'
    issueForm.appendChild(
      button('Issue Invoice', async () => {
        await sendCommand({
          type: 'issue_invoice',
          commandId: crypto.randomUUID(),
          invoiceId: invoice.id,
          expectedRevision: invoice.revision,
          reason: issueReason.value.trim(),
        })
      }),
    )
    issueForm.hidden = !(canSettle && invoice.status === 'draft')
    detail.appendChild(issueForm)

    const correctionForm = node('form')
    correctionForm.dataset.billingCorrectionForm = ''
    correctionForm.appendChild(node('h4', 'Append correction (negative = credit note)'))
    const correctionAmount = field(
      correctionForm,
      'amountMinor',
      'Amount (minor units, signed)',
      'number',
      true,
    )
    const correctionReason = field(correctionForm, 'reason', 'Reason', 'text', true)
    correctionForm.appendChild(
      button('Append Correction', async () => {
        await sendCommand({
          type: 'append_correction',
          commandId: crypto.randomUUID(),
          invoiceId: invoice.id,
          amountMinor: Number(correctionAmount.value),
          reason: correctionReason.value.trim(),
        })
      }),
    )
    correctionForm.hidden = !(canSettle && invoice.status === 'issued')
    detail.appendChild(correctionForm)

    const voidForm = node('form')
    voidForm.dataset.billingVoidForm = ''
    voidForm.appendChild(node('h4', 'Void invoice (refused while anything is owed)'))
    const voidReason = field(voidForm, 'reason', 'Reason', 'text', true)
    voidForm.appendChild(
      button('Void Invoice', async () => {
        await sendCommand({
          type: 'void_invoice',
          commandId: crypto.randomUUID(),
          invoiceId: invoice.id,
          expectedRevision: invoice.revision,
          reason: voidReason.value.trim(),
        })
      }),
    )
    voidForm.hidden = !(canSettle && (invoice.status === 'draft' || invoice.status === 'issued'))
    detail.appendChild(voidForm)

    const attemptForm = node('form')
    attemptForm.dataset.billingAttemptForm = ''
    attemptForm.appendChild(node('h4', 'Record payment attempt (records intent, settles nothing)'))
    const attemptAmount = field(attemptForm, 'amountMinor', 'Amount (minor units)', 'number', true)
    attemptAmount.value = String(invoice.outstandingMinor)
    const attemptProvider = field(attemptForm, 'providerRef', 'Provider Reference')
    attemptForm.appendChild(
      button('Record Payment Attempt', async () => {
        await sendCommand({
          type: 'record_payment_attempt',
          commandId: crypto.randomUUID(),
          invoiceId: invoice.id,
          amountMinor: Number(attemptAmount.value),
          providerRef: attemptProvider.value.trim() || null,
        })
      }),
    )
    attemptForm.hidden = !canWrite
    detail.appendChild(attemptForm)

    const eventForm = node('form')
    eventForm.dataset.billingEventForm = ''
    eventForm.appendChild(
      node(
        'h4',
        'Record provider event (only a correctly signed notification settles this invoice)',
      ),
    )
    const eventProvider = field(eventForm, 'providerId', 'Provider ID', 'text', true)
    eventProvider.value = 'acquirer-demo'
    const eventEventId = field(eventForm, 'providerEventId', 'Provider Event ID', 'text', true)
    const eventOutcome = selectField(eventForm, 'outcome', 'Outcome', PAYMENT_OUTCOMES, 'succeeded')
    const eventAmount = field(eventForm, 'amountMinor', 'Amount (minor units)', 'number', true)
    eventAmount.value = String(invoice.outstandingMinor)
    const eventSignature = field(
      eventForm,
      'signature',
      'Provider Signature (64 hex)',
      'text',
      true,
    )
    eventForm.appendChild(
      button('Record Provider Event', async () => {
        await sendCommand({
          type: 'record_provider_event',
          commandId: crypto.randomUUID(),
          invoiceId: invoice.id,
          providerId: eventProvider.value.trim(),
          providerEventId: eventEventId.value.trim(),
          outcome: eventOutcome.value as (typeof PAYMENT_OUTCOMES)[number],
          amountMinor: Number(eventAmount.value),
          currency: invoice.currency,
          signature: eventSignature.value.trim().toLowerCase(),
        })
      }),
    )
    eventForm.hidden = !canWrite
    detail.appendChild(eventForm)

    detail.appendChild(node('p', `Statuses observed: ${INVOICE_STATUSES.join(' → ')}.`))
  }

  const load = async () => {
    await loadAccess()
    await loadList()
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)
  const ready = perform(load)

  return {
    ready,
    destroy() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
      delete host.dataset.billingStatus
    },
  }
}
