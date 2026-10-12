import { z } from 'zod'
import {
  assetAccessSchema,
  assetReceiptSchema,
  assetCommandStatusSchema,
  assetSchema,
  assetDetailSchema,
  type AssetAccess,
  type AssetMutation,
  type AssetDetail,
  type AssetCondition,
  type CustodyEventType,
  type MaintenanceType,
  type AssetOwnership,
} from './contracts.js'

export function mountAssetDashboard(
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
  let access: AssetAccess | null = null
  let pending: AssetMutation | null = null
  let uncertain = false
  let selected: AssetDetail | null = null
  let after: string | undefined

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const searchForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.assetStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.assetCreate = ''
  searchForm.dataset.assetSearch = ''
  list.dataset.assetList = ''
  detail.dataset.assetDetail = ''
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

  const createCode = field(createForm, 'code', 'Asset Code', 'text', true)
  const createName = field(createForm, 'name', 'Asset Name', 'text', true)
  const createCategory = field(createForm, 'category', 'Category', 'text', true)
  const createSerial = field(createForm, 'serialNumber', 'Serial Number')
  const createModel = field(createForm, 'model', 'Model')
  const createOwnership = selectField(
    createForm,
    'ownership',
    'Ownership',
    ['business_owned', 'customer_owned'] as const,
    'business_owned',
  )
  const createCustomer = field(createForm, 'customerContactId', 'Customer Contact ID')
  const createCondition = selectField(
    createForm,
    'condition',
    'Condition',
    ['excellent', 'good', 'fair', 'damaged', 'unserviceable'] as const,
    'good',
  )
  const createLocation = field(createForm, 'initialLocation', 'Initial Location')

  const createSubmit = button('Register Asset', async () => {
    if (uncertain) {
      status.textContent = 'Uncertain command outcome; reconcile before submitting new requests.'
      return
    }
    if (!createCode.value.trim() || !createName.value.trim() || !createCategory.value.trim()) return
    const ownership = createOwnership.value as AssetOwnership
    if (ownership === 'customer_owned' && !createCustomer.value.trim()) {
      status.textContent = 'Customer contact ID required for customer-owned assets.'
      return
    }
    const body: AssetMutation = {
      requestId: crypto.randomUUID(),
      type: 'create_asset',
      data: {
        code: createCode.value.trim(),
        name: createName.value.trim(),
        category: createCategory.value.trim(),
        serialNumber: createSerial.value.trim() || null,
        model: createModel.value.trim() || null,
        ownership,
        customerContactId: createCustomer.value.trim() || null,
        condition: createCondition.value as AssetCondition,
        initialLocation: createLocation.value.trim() || null,
      },
    }
    await sendCommand(body)
    if (uncertain) return
    createCode.value = ''
    createName.value = ''
    createCategory.value = ''
    createSerial.value = ''
    createModel.value = ''
    createCustomer.value = ''
    createLocation.value = ''
    await loadList()
  })
  createForm.appendChild(createSubmit)

  const searchInput = field(searchForm, 'search', 'Search Assets')
  const searchSubmit = button('Search', async () => {
    after = undefined
    await loadList()
  })
  searchForm.appendChild(searchSubmit)

  const checkPending = button('Check pending result', async () => {
    if (!pending) return
    try {
      const res = await transport(`${options.apiBase}/commands/${pending.requestId}`, {
        headers: { accept: 'application/json' },
        signal: lifetime.signal,
      })
      if (!res.ok) {
        status.textContent = `Check pending failed with HTTP ${res.status}.`
        return
      }
      const data = assetCommandStatusSchema.parse(await res.json())
      if (data.status === 'committed') {
        uncertain = false
        recovery.hidden = true
        pending = null
        status.textContent = 'Action confirmed.'
        await loadList()
      } else {
        status.textContent = 'Request not committed yet.'
      }
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : 'Check pending error'
    }
  })

  const retryPending = button('Retry same request', async () => {
    if (!pending) return
    const toRetry = pending
    await sendCommand(toRetry)
    if (!uncertain) {
      await loadList()
    }
  })

  recovery.dataset.assetRecovery = ''
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

  async function sendCommand(mutation: AssetMutation) {
    if (uncertain && pending?.requestId !== mutation.requestId) {
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
          const checkRes = await transport(`${options.apiBase}/commands/${mutation.requestId}`, {
            headers: { accept: 'application/json' },
            signal: lifetime.signal,
          })
          if (checkRes.ok) {
            const parsed = assetCommandStatusSchema.parse(await checkRes.json())
            if (parsed.status === 'committed') {
              uncertain = false
              recovery.hidden = true
              pending = null
              status.textContent = 'Action confirmed.'
              return
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
      const receipt = assetReceiptSchema.parse(await res.json())
      if (receipt.requestId !== mutation.requestId) {
        throw new Error('Command receipt ID mismatch')
      }
      uncertain = false
      recovery.hidden = true
      pending = null
      status.textContent = `Success (${receipt.type}).`
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
    if (!res.ok) {
      status.textContent = 'Failed to load access grants.'
      return
    }
    access = assetAccessSchema.parse(await res.json())
    const canWrite = access.permissions.includes('assets.write')
    createForm.hidden = !canWrite
  }

  async function loadList() {
    const params = new URLSearchParams()
    params.set('limit', '20')
    if (after) params.set('after', after)
    if (searchInput.value.trim()) params.set('search', searchInput.value.trim())
    const res = await transport(`${options.apiBase}/records?${params}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) {
      status.textContent = 'Failed to load assets list.'
      return
    }
    const data = z
      .object({ items: z.array(assetSchema), nextCursor: z.string().nullable() })
      .parse(await res.json())
    list.replaceChildren()
    for (const item of data.items) {
      const li = node('li')
      li.dataset.assetItem = item.id
      const title = node('strong', `[${item.code}] ${item.name} (${item.status})`)
      const meta = node(
        'span',
        ` — Condition: ${item.condition} | Loc: ${item.currentLocation ?? 'N/A'} | Owner: ${item.ownership}`,
      )
      const selectBtn = button('Select', async () => {
        await loadDetail(item.id)
      })
      li.appendChild(title)
      li.appendChild(meta)
      li.appendChild(selectBtn)
      list.appendChild(li)
    }
  }

  async function loadDetail(id: string) {
    const res = await transport(`${options.apiBase}/records/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) {
      status.textContent = 'Failed to load asset details.'
      return
    }
    selected = assetDetailSchema.parse(await res.json())
    renderDetail()
  }

  function renderDetail() {
    detail.replaceChildren()
    if (!selected) return
    const a = selected.asset
    const h3 = node('h3', `[${a.code}] ${a.name}`)
    detail.appendChild(h3)
    const p = node(
      'p',
      `Category: ${a.category} | Serial: ${a.serialNumber ?? 'N/A'} | Status: ${a.status} | Condition: ${a.condition} | Custodian: ${a.currentCustodianId ?? 'None'} | Location: ${a.currentLocation ?? 'N/A'} | Rev: ${a.revision}`,
    )
    detail.appendChild(p)

    // Handover Section
    const canHandover = access?.permissions.includes('assets.handover') ?? false
    if (canHandover && a.status !== 'retired' && a.status !== 'returned_to_customer') {
      const handoverSec = node('fieldset')
      handoverSec.appendChild(node('legend', 'Asset Handover / Custody Transfer'))
      const eventSelect = selectField(
        handoverSec,
        'eventType',
        'Event Type',
        ['dispatch', 'checkin', 'transfer', 'service_intake', 'customer_return'] as const,
        a.ownership === 'customer_owned' && a.status === 'under_maintenance'
          ? 'customer_return'
          : 'dispatch',
      )
      const toCustodianInput = field(handoverSec, 'toCustodian', 'New Custodian ID')
      const conditionSelect = selectField(
        handoverSec,
        'conditionAfter',
        'Condition After Inspection',
        ['excellent', 'good', 'fair', 'damaged', 'unserviceable'] as const,
        a.condition,
      )
      const locInput = field(handoverSec, 'location', 'New Location')
      const notesInput = field(handoverSec, 'notes', 'Inspection Notes')

      const handoverBtn = button('Execute Handover', async () => {
        if (uncertain) {
          status.textContent =
            'Uncertain command outcome; reconcile before submitting new requests.'
          return
        }
        const body: AssetMutation = {
          requestId: crypto.randomUUID(),
          type: 'handover_asset',
          data: {
            assetId: a.id,
            expectedRevision: a.revision,
            eventType: eventSelect.value as CustodyEventType,
            toCustodianId: toCustodianInput.value.trim() || null,
            conditionAfter: conditionSelect.value as AssetCondition,
            location: locInput.value.trim() || null,
            notes: notesInput.value.trim() || null,
          },
        }
        await sendCommand(body)
        if (uncertain) return
        await loadDetail(a.id)
        await loadList()
      })
      handoverSec.appendChild(handoverBtn)
      detail.appendChild(handoverSec)
    }

    // Maintenance Section
    const canMaintain = access?.permissions.includes('assets.maintenance') ?? false
    if (canMaintain && a.status !== 'retired') {
      const maintSec = node('fieldset')
      maintSec.appendChild(node('legend', 'Record Maintenance / Service'))
      const typeSelect = selectField(
        maintSec,
        'serviceType',
        'Service Type',
        ['preventive', 'corrective', 'inspection'] as const,
        'inspection',
      )
      const descInput = field(maintSec, 'description', 'Description', 'text', true)
      const partsInput = field(maintSec, 'partsUsed', 'Parts / Materials Used')
      const conditionSelect = selectField(
        maintSec,
        'conditionAfter',
        'Resulting Condition',
        ['excellent', 'good', 'fair', 'damaged', 'unserviceable'] as const,
        'good',
      )

      const maintBtn = button('Record Maintenance', async () => {
        if (uncertain) {
          status.textContent =
            'Uncertain command outcome; reconcile before submitting new requests.'
          return
        }
        if (!descInput.value.trim()) return
        const body: AssetMutation = {
          requestId: crypto.randomUUID(),
          type: 'record_maintenance',
          data: {
            assetId: a.id,
            expectedRevision: a.revision,
            serviceType: typeSelect.value as MaintenanceType,
            description: descInput.value.trim(),
            partsUsed: partsInput.value.trim() || null,
            conditionAfter: conditionSelect.value as AssetCondition,
          },
        }
        await sendCommand(body)
        if (uncertain) return
        await loadDetail(a.id)
        await loadList()
      })
      maintSec.appendChild(maintBtn)
      detail.appendChild(maintSec)
    }

    // Custody History List
    const custodyList = node('ul')
    custodyList.dataset.custodyHistory = ''
    for (const event of selected.custodyEvents) {
      const li = node(
        'li',
        `[${event.recordedAt}] ${event.eventType.toUpperCase()}: To ${event.toCustodianId ?? 'None'} (Cond: ${event.conditionBefore} -> ${event.conditionAfter}) — ${event.notes ?? ''}`,
      )
      custodyList.appendChild(li)
    }
    const custodySec = node('fieldset')
    custodySec.appendChild(node('legend', 'Custody & Handover History'))
    custodySec.appendChild(custodyList)
    detail.appendChild(custodySec)

    // Maintenance History List
    const maintList = node('ul')
    maintList.dataset.maintenanceHistory = ''
    for (const record of selected.maintenanceRecords) {
      const li = node(
        'li',
        `[${record.recordedAt}] ${record.serviceType.toUpperCase()}: ${record.description} (Parts: ${record.partsUsed ?? 'None'}) -> ${record.conditionAfter} by ${record.performedBy}`,
      )
      maintList.appendChild(li)
    }
    const maintSecHistory = node('fieldset')
    maintSecHistory.appendChild(node('legend', 'Maintenance History'))
    maintSecHistory.appendChild(maintList)
    detail.appendChild(maintSecHistory)
  }

  host.replaceChildren(status, recovery, createForm, searchForm, list, detail)

  const ready = perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Asset register ready.'
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
