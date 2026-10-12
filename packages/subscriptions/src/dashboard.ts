import { z } from 'zod'
import {
  subscriptionAccessSchema,
  subscriptionReceiptSchema,
  subscriptionCommandStatusSchema,
  subscriptionSchema,
  subscriptionDetailSchema,
  type SubscriptionAccess,
  type SubscriptionMutation,
  type BillingInterval,
  type EntitlementType,
} from './contracts.js'

export function mountSubscriptionDashboard(
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
  let access: SubscriptionAccess | null = null
  let pending: SubscriptionMutation | null = null
  let uncertain = false

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.subscriptionStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.subscriptionCreate = ''
  list.dataset.subscriptionList = ''
  detail.dataset.subscriptionDetail = ''
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

  const createContact = field(createForm, 'contactId', 'Contact ID', 'text', true)
  const createPlanCode = field(createForm, 'planCode', 'Plan Code', 'text', true)
  const createPlanName = field(createForm, 'planName', 'Plan Name', 'text', true)
  const createInterval = selectField(
    createForm,
    'billingInterval',
    'Billing Interval',
    ['monthly', 'yearly', 'quarterly', 'weekly'] as const,
    'monthly',
  )
  const createPrice = field(createForm, 'unitPrice', 'Unit Price', 'number', true)
  createPrice.value = '100000'
  const createCurrency = field(createForm, 'currency', 'Currency', 'text', true)
  createCurrency.value = 'IDR'
  const createQuantity = field(createForm, 'quantity', 'Seats/Quantity', 'number', true)
  createQuantity.value = '1'

  // Initial entitlement inputs
  const createFeatureCode = field(createForm, 'featureCode', 'Initial Feature Code')
  const createFeatureName = field(createForm, 'featureName', 'Initial Feature Name')
  const createFeatureType = selectField(
    createForm,
    'entitlementType',
    'Entitlement Type',
    ['boolean_access', 'pass_bundle', 'metered', 'seat_count'] as const,
    'boolean_access',
  )
  const createFeatureQty = field(createForm, 'featureQty', 'Feature Quota', 'number')

  const createSubmit = button('Create Subscription', async () => {
    if (uncertain) {
      status.textContent = 'Uncertain command outcome; reconcile before submitting new requests.'
      return
    }
    if (!createContact.value.trim() || !createPlanCode.value.trim() || !createPlanName.value.trim())
      return

    const initialEntitlements: Array<{
      featureCode: string
      featureName: string
      entitlementType: 'boolean_access' | 'pass_bundle' | 'metered' | 'seat_count'
      allocatedQuantity?: number | null
    }> = []

    if (createFeatureCode.value.trim()) {
      initialEntitlements.push({
        featureCode: createFeatureCode.value.trim(),
        featureName: createFeatureName.value.trim() || createFeatureCode.value.trim(),
        entitlementType: createFeatureType.value as EntitlementType,
        allocatedQuantity: createFeatureQty.value ? Number(createFeatureQty.value) : null,
      })
    }

    const commandId = crypto.randomUUID()
    const mutation: SubscriptionMutation = {
      type: 'create_subscription',
      commandId,
      contactId: createContact.value.trim(),
      planCode: createPlanCode.value.trim(),
      planName: createPlanName.value.trim(),
      billingInterval: createInterval.value as BillingInterval,
      unitPrice: Number(createPrice.value),
      currency: createCurrency.value.trim().toUpperCase(),
      quantity: Number(createQuantity.value) || 1,
      initialEntitlements,
    }

    await sendCommand(mutation)
    if (!uncertain) {
      createPlanCode.value = ''
      createPlanName.value = ''
      createFeatureCode.value = ''
      createFeatureName.value = ''
      createFeatureQty.value = ''
      await loadList()
    }
  })
  createForm.appendChild(createSubmit)

  // Recovery controls
  const checkPending = button('Check pending result', async () => {
    if (!pending) return
    const res = await transport(`${options.apiBase}/commands/${pending.commandId}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) {
      status.textContent = `Pending check failed: HTTP ${res.status}`
      return
    }
    const data = subscriptionCommandStatusSchema.parse(await res.json())
    if (data.status === 'recorded') {
      uncertain = false
      recovery.hidden = true
      pending = null
      status.textContent = 'Action confirmed.'
      await loadList()
    } else {
      status.textContent = 'Pending command was not recorded by server.'
    }
  })

  const retryPending = button('Retry same request', async () => {
    if (!pending) return
    await sendCommand(pending)
    if (!uncertain) {
      await loadList()
    }
  })

  recovery.dataset.subscriptionRecovery = ''
  recovery.appendChild(node('span', 'Recovery mode: previous request outcome is uncertain.'))
  recovery.appendChild(checkPending)
  recovery.appendChild(retryPending)

  async function perform(work: () => Promise<void>) {
    if (busy || disposed) return
    busy = true
    host.dataset.busy = 'true'
    status.textContent = 'Working...'
    try {
      await work()
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : 'Operation failed'
    } finally {
      busy = false
      delete host.dataset.busy
    }
  }

  async function sendCommand(mutation: SubscriptionMutation) {
    if (uncertain && pending?.commandId !== mutation.commandId) {
      throw new Error('Uncertain command outcome; reconcile before submitting new requests.')
    }
    pending = mutation
    uncertain = true
    recovery.hidden = false
    let res: Response
    try {
      res = await transport(`${options.apiBase}/commands`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', accept: 'application/json' },
        body: JSON.stringify(mutation),
        signal: lifetime.signal,
      })
    } catch {
      uncertain = true
      recovery.hidden = false
      status.textContent = 'Network interruption. Reconcile or retry before proceeding.'
      return
    }
    if (!res.ok) {
      if (res.status === 409) {
        try {
          const checkRes = await transport(`${options.apiBase}/commands/${mutation.commandId}`, {
            headers: { accept: 'application/json' },
            signal: lifetime.signal,
          })
          if (checkRes.ok) {
            const parsed = subscriptionCommandStatusSchema.parse(await checkRes.json())
            if (parsed.status === 'recorded') {
              uncertain = false
              recovery.hidden = true
              pending = null
              status.textContent = 'Action confirmed.'
              return parsed.receipt
            }
          }
        } catch {
          // ignore
        }
      }
      let code = `HTTP ${res.status}`
      try {
        const body = (await res.json()) as { error?: string }
        if (body.error) code = body.error
      } catch {
        void 0
      }
      if (res.status === 400) {
        pending = null
        uncertain = false
        recovery.hidden = true
      } else {
        uncertain = true
        recovery.hidden = false
      }
      status.textContent = `Command failed: ${code}`
      return
    }
    try {
      const receipt = subscriptionReceiptSchema.parse(await res.json())
      if (receipt.commandId !== mutation.commandId) {
        throw new Error('Command receipt ID mismatch')
      }
      uncertain = false
      recovery.hidden = true
      pending = null
      status.textContent = `Success (${receipt.action}).`
      return receipt
    } catch {
      uncertain = true
      recovery.hidden = false
      status.textContent =
        'Invalid or truncated command receipt. Reconcile or retry before proceeding.'
      return
    }
  }

  async function loadAccess() {
    const res = await transport(`${options.apiBase}/access`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Access check failed: HTTP ${res.status}`)
    access = subscriptionAccessSchema.parse(await res.json())
    const canWrite = access.permissions.includes('subscriptions.write')
    createForm.hidden = !canWrite
  }

  async function loadList() {
    list.replaceChildren()
    const res = await transport(`${options.apiBase}/records?limit=50`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load subscriptions: HTTP ${res.status}`)
    const items = z.array(subscriptionSchema).parse(await res.json())
    for (const sub of items) {
      const li = node('li')
      li.dataset.subscriptionItem = sub.id
      const title = node(
        'strong',
        `${sub.planName} (${sub.planCode}) [${sub.status.toUpperCase()}]`,
      )
      const meta = node(
        'span',
        ` - Qty: ${sub.quantity} | ${sub.unitPrice.toLocaleString()} ${sub.currency}/${sub.billingInterval}`,
      )
      const viewBtn = button('View', async () => {
        await loadDetail(sub.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(viewBtn)
      list.appendChild(li)
    }
  }

  async function loadDetail(id: string) {
    detail.replaceChildren()
    const res = await transport(`${options.apiBase}/records/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Failed to load details: HTTP ${res.status}`)
    const record = subscriptionDetailSchema.parse(await res.json())

    const h = node('h3', `Subscription: ${record.subscription.planName}`)
    detail.appendChild(h)

    const info = node('div')
    info.appendChild(node('p', `Status: ${record.subscription.status}`))
    info.appendChild(node('p', `Plan: ${record.subscription.planCode}`))
    info.appendChild(node('p', `Billing: ${record.subscription.billingInterval}`))
    info.appendChild(
      node(
        'p',
        `Current Period: ${record.subscription.currentPeriodStart ?? 'None'} to ${record.subscription.currentPeriodEnd ?? 'None'}`,
      ),
    )
    info.appendChild(
      node('p', `Payment Reference: ${record.subscription.paymentReference ?? 'None'}`),
    )
    detail.appendChild(info)

    // Entitlements list
    const entSec = node('section')
    entSec.appendChild(node('h4', 'Entitlements & Quotas'))
    const entList = node('ul')
    for (const ent of record.entitlements) {
      const li = node('li')
      li.dataset.entitlementItem = ent.id
      const quotaStr =
        ent.allocatedQuantity !== null
          ? `${ent.consumedQuantity} / ${ent.allocatedQuantity}`
          : 'Unlimited'
      li.textContent = `${ent.featureName} (${ent.featureCode}) [${ent.status}] - Used: ${quotaStr}`

      if (access?.permissions.includes('subscriptions.usage') && ent.status === 'active') {
        const useBtn = button('Record 1 Usage', async () => {
          if (uncertain) return
          const cmdId = crypto.randomUUID()
          const mut: SubscriptionMutation = {
            type: 'consume_usage',
            commandId: cmdId,
            entitlementId: ent.id,
            units: 1,
            idempotencyKey: `dash-use-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
            evidenceRef: 'Dashboard check-in / consumption',
          }
          await sendCommand(mut)
          if (!uncertain) await loadDetail(id)
        })
        li.appendChild(useBtn)
      }
      entList.appendChild(li)
    }
    entSec.appendChild(entList)
    detail.appendChild(entSec)

    // Authoritative payment confirmation form (if permissions allow)
    if (access?.permissions.includes('subscriptions.payment')) {
      const payForm = node('form')
      payForm.dataset.subscriptionPayment = ''
      payForm.appendChild(node('h4', 'Record Authoritative Payment Confirmation'))
      const payRef = field(payForm, 'paymentRef', 'Payment Reference', 'text', true)
      payRef.value = `INV-${Date.now()}`
      const payAmount = field(payForm, 'amount', 'Amount Paid', 'number', true)
      payAmount.value = String(record.subscription.unitPrice * record.subscription.quantity)

      const paySubmit = button('Confirm Payment & Activate', async () => {
        if (uncertain) return
        if (!payRef.value.trim()) return
        const now = new Date()
        const end = new Date(now.getTime() + 30 * 86400000) // +30 days
        const cmdId = crypto.randomUUID()
        const mut: SubscriptionMutation = {
          type: 'record_payment',
          commandId: cmdId,
          subscriptionId: record.subscription.id,
          paymentReference: payRef.value.trim(),
          amountPaid: Number(payAmount.value),
          periodStart: now.toISOString(),
          periodEnd: end.toISOString(),
        }
        await sendCommand(mut)
        if (!uncertain) await loadDetail(id)
      })
      payForm.appendChild(paySubmit)
      detail.appendChild(payForm)
    }

    // Status change (cancel/pause/active)
    if (access?.permissions.includes('subscriptions.write')) {
      const statusForm = node('form')
      statusForm.dataset.subscriptionStatusChange = ''
      statusForm.appendChild(node('h4', 'Change Lifecycle Status'))
      const targetSel = selectField(
        statusForm,
        'targetStatus',
        'Target Status',
        ['active', 'paused', 'canceled'] as const,
        'canceled',
      )
      const reasonField = field(statusForm, 'reason', 'Reason', 'text', true)
      reasonField.value = 'User requested via dashboard'

      const statusSubmit = button('Apply Status Change', async () => {
        if (uncertain) return
        const cmdId = crypto.randomUUID()
        const mut: SubscriptionMutation = {
          type: 'change_status',
          commandId: cmdId,
          subscriptionId: record.subscription.id,
          expectedRevision: record.subscription.revision,
          targetStatus: targetSel.value as 'active' | 'paused' | 'canceled',
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
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Subscriptions dashboard ready.'
  })

  return {
    ready,
    destroy() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
    dispose() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
