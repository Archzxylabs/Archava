import { z } from 'zod'
import {
  purchaseAccessSchema,
  purchaseMutationSchema,
  purchaseReceiptSchema,
  purchaseStatusSchema,
  supplierSchema,
  offerSchema,
  purchaseOrderSchema,
  purchaseDetailSchema,
  type PurchaseAccess,
  type PurchaseMutation,
  type PurchaseReceipt,
  type PurchaseDetail,
  type Supplier,
  type SupplierOffer,
} from './contracts.js'
/** Human procurement panel. Host supplies credential/session/CSRF; receipts describe physical goods. */
export function mountPurchasingDashboard(
  host: HTMLElement,
  options: { apiBase: string; commerceApiBase: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument,
    transport = options.transport ?? fetch,
    lifetime = new AbortController()
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const n = document.createElement(tag)
    n.textContent = text
    return n
  }
  let disposed = false,
    busy = false,
    access: PurchaseAccess | null = null,
    pending: PurchaseMutation | null = null,
    uncertain = false,
    selected: PurchaseDetail | null = null,
    after: string | undefined
  let suppliers: Supplier[] = [],
    offers: SupplierOffer[] = []
  const status = node('p'),
    recovery = node('div'),
    supplierForm = node('form'),
    offerForm = node('form'),
    intake = node('form'),
    supplierList = node('ul'),
    offerList = node('ul'),
    orders = node('ul'),
    detail = node('section')
  status.dataset.purchasingStatus = ''
  status.setAttribute('role', 'status')
  recovery.hidden = true
  supplierForm.hidden = true
  offerForm.hidden = true
  intake.hidden = true
  supplierForm.dataset.purchasingSupplier = ''
  offerForm.dataset.purchasingOffer = ''
  intake.dataset.purchasingCreate = ''
  detail.dataset.purchasingDetail = ''
  orders.dataset.purchasingList = ''
  const button = (text: string, work: () => Promise<void>) => {
    const b = node('button', text)
    b.type = 'button'
    b.addEventListener('click', () => {
      void perform(work)
    })
    return b
  }
  const field = (
    f: HTMLElement,
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
    if (type === 'number') {
      i.min = '0'
      i.step = '1'
    }
    l.append(i)
    f.append(l)
    return i
  }
  const select = (f: HTMLElement, name: string, label: string) => {
    const l = node('label', label),
      s = node('select')
    s.name = name
    l.append(s)
    f.append(l)
    return s
  }
  const option = (s: HTMLSelectElement, value: string, label: string) => {
    const o = node('option', label)
    o.value = value
    s.append(o)
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
  const supplierCode = field(supplierForm, 'code', 'Supplier code'),
    supplierName = field(supplierForm, 'name', 'Supplier name')
  submit(supplierForm, 'Create supplier', () => ({
    operation: 'supplier.create',
    code: supplierCode.value,
    name: supplierName.value,
  }))
  const offerSupplier = select(offerForm, 'supplierId', 'Supplier'),
    product = select(offerForm, 'productId', 'Stock product'),
    cost = field(offerForm, 'unitCost', 'Supplier cost per base unit (IDR)', 'number', '0')
  submit(offerForm, 'Create supplier offer', () => ({
    operation: 'offer.create',
    supplierId: offerSupplier.value,
    productId: product.value,
    unitCost: Number(cost.value),
  }))
  const orderSupplier = select(intake, 'supplierId', 'Order supplier'),
    cart = node('div'),
    cartRows: { s: HTMLSelectElement; q: HTMLInputElement }[] = []
  const fillOffers = (s: HTMLSelectElement) => {
    s.replaceChildren()
    for (const o of offers.filter((x) => x.active && x.supplierId === orderSupplier.value))
      option(s, o.id, o.sku + ' — ' + o.title + ' — ' + o.unitCost + ' IDR/' + o.unit)
  }
  function addLine() {
    if (cartRows.length >= 50) throw new Error('Use at most 50 lines.')
    const row = node('div'),
      s = select(row, 'offerId', 'Supplier offer'),
      q = field(row, 'quantity', 'Quantity in base units', 'number', '1')
    q.min = '1'
    q.max = '1000000'
    fillOffers(s)
    const item = { s, q }
    cartRows.push(item)
    row.append(
      button('Remove line', () => {
        cartRows.splice(cartRows.indexOf(item), 1)
        row.remove()
        return Promise.resolve()
      }),
    )
    cart.append(row)
  }
  orderSupplier.addEventListener('change', () => {
    for (const r of cartRows) fillOffers(r.s)
  })
  intake.append(
    cart,
    button('Add purchase line', () => {
      addLine()
      return Promise.resolve()
    }),
  )
  submit(intake, 'Create purchase draft', () => ({
    operation: 'order.create',
    supplierId: orderSupplier.value,
    lines: cartRows.map((r) => ({ offerId: r.s.value, quantity: Number(r.q.value) })),
  }))
  const filters = node('form'),
    state = select(filters, 'state', 'Order state')
  for (const x of ['', 'draft', 'confirmed', 'partial', 'received', 'cancelled', 'short_closed'])
    option(state, x, x || 'All')
  const apply = node('button', 'Apply order filter')
  apply.type = 'submit'
  filters.append(apply)
  filters.addEventListener('submit', (e) => {
    e.preventDefault()
    after = undefined
    void perform(loadOrders)
  })
  const next = button('Next orders', async () => {
    after = next.dataset.after
    await loadOrders()
  })
  next.hidden = true
  recovery.append(
    node('p', 'A previous Save has an unresolved outcome. Keep its request ID until recovered.'),
    button('Check saved request', async () => {
      if (!pending) return
      const r = purchaseStatusSchema.parse(await request('/commands/' + pending.requestId))
      if (disposed) return
      if (r.state === 'committed') {
        pending = null
        uncertain = false
        recovery.hidden = true
        await afterReceipt(r.receipt)
      } else status.textContent = 'No committed receipt yet. Retry the same request.'
    }),
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Purchasing'),
    node(
      'p',
      'Supplier quotes, purchase orders and physical receipts. Accepted goods enter shared stock; rejected goods remain separate findings.',
    ),
    status,
    recovery,
    supplierForm,
    supplierList,
    offerForm,
    offerList,
    intake,
    button('Refresh purchasing', reload),
    filters,
    orders,
    next,
    detail,
  )
  async function request(path: string, body?: unknown, base = options.apiBase) {
    const r = await transport(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: lifetime.signal,
    })
    if (!r.ok) {
      const e = new Error('Request refused (' + r.status + ').') as Error & { status: number }
      e.status = r.status
      throw e
    }
    return (await r.json()) as unknown
  }
  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    lock()
    try {
      await work()
    } catch (e) {
      if (!disposed) status.textContent = e instanceof Error ? e.message : 'Purchasing unavailable.'
    } finally {
      busy = false
      if (!disposed) lock()
    }
  }
  function lock() {
    for (const b of host.querySelectorAll<HTMLButtonElement>('button')) b.disabled = busy
  }
  function can(p: PurchaseAccess['permissions'][number]) {
    return access?.permissions.includes(p) ?? false
  }
  async function collect<T>(
    path: string,
    schema: z.ZodType<T>,
    base = options.apiBase,
  ): Promise<T[]> {
    const items: T[] = []
    let cursor: string | null = null
    do {
      const q = new URLSearchParams({ limit: '100' })
      if (cursor) q.set('after', cursor)
      const page = z
        .object({ items: z.array(schema), next: z.uuid().nullable() })
        .passthrough()
        .parse(await request(path + '?' + q.toString(), undefined, base))
      items.push(...page.items)
      cursor = page.next
      if (items.length >= 1000 && cursor)
        throw new Error('Selector exceeds 1000 entries; use the paginated API.')
    } while (cursor)
    return items
  }
  async function loadCatalog() {
    suppliers = can('suppliers.read') ? await collect('/suppliers', supplierSchema) : []
    offers = can('offers.read') ? await collect('/offers', offerSchema) : []
    if (disposed) return
    supplierList.replaceChildren()
    offerList.replaceChildren()
    for (const s of suppliers) {
      const li = node('li', s.code + ' — ' + s.name + ' — ' + (s.active ? 'active' : 'inactive'))
      if (can('suppliers.write') && access?.scope === 'all')
        li.append(
          button(s.active ? 'Disable supplier' : 'Enable supplier', () =>
            mutate({
              operation: 'supplier.set-active',
              id: s.id,
              expectedRevision: s.revision,
              active: !s.active,
              reason: 'Operator changes supplier availability',
            }),
          ),
        )
      supplierList.append(li)
    }
    for (const o of offers) {
      const li = node(
        'li',
        (suppliers.find((x) => x.id === o.supplierId)?.name ?? 'Supplier') +
          ' — ' +
          o.sku +
          ' — ' +
          o.unitCost +
          ' IDR/' +
          o.unit +
          ' — ' +
          (o.active ? 'active' : 'inactive'),
      )
      if (can('offers.write') && access?.scope === 'all') {
        const f = node('form'),
          v = field(f, 'unitCost', 'New supplier unit cost', 'number', String(o.unitCost))
        submit(f, 'Update supplier cost', () => ({
          operation: 'offer.set-cost',
          id: o.id,
          expectedRevision: o.revision,
          unitCost: Number(v.value),
          reason: 'Operator updates supplier offer',
        }))
        li.append(
          f,
          button(o.active ? 'Disable offer' : 'Enable offer', () =>
            mutate({
              operation: 'offer.set-active',
              id: o.id,
              expectedRevision: o.revision,
              active: !o.active,
              reason: 'Operator changes offer availability',
            }),
          ),
        )
      }
      offerList.append(li)
    }
    const keep = orderSupplier.value
    offerSupplier.replaceChildren()
    orderSupplier.replaceChildren()
    for (const s of suppliers.filter((x) => x.active)) {
      option(offerSupplier, s.id, s.code + ' — ' + s.name)
      option(orderSupplier, s.id, s.code + ' — ' + s.name)
    }
    if (suppliers.some((x) => x.id === keep && x.active)) orderSupplier.value = keep
    for (const r of cartRows) fillOffers(r.s)
    if (!cartRows.length && offers.some((x) => x.active)) addLine()
    if (can('offers.write') && access?.scope === 'all') {
      const ps = await collect(
        '/products',
        z
          .object({
            id: z.uuid(),
            sku: z.string(),
            title: z.string(),
            unit: z.string(),
            active: z.boolean(),
            trackStock: z.boolean(),
          })
          .passthrough(),
        options.commerceApiBase,
      )
      if (disposed) return
      product.replaceChildren()
      for (const p of ps.filter((x) => x.active && x.trackStock))
        option(product, p.id, p.sku + ' — ' + p.title + ' (' + p.unit + ')')
    }
  }
  async function loadOrders() {
    if (!can('orders.read')) {
      orders.replaceChildren()
      detail.replaceChildren()
      selected = null
      return
    }
    const q = new URLSearchParams({ limit: '50' })
    if (after) q.set('after', after)
    if (state.value) q.set('state', state.value)
    const page = z
      .object({ items: z.array(purchaseOrderSchema), next: z.uuid().nullable() })
      .passthrough()
      .parse(await request('/orders?' + q.toString()))
    if (disposed) return
    orders.replaceChildren()
    for (const o of page.items) {
      const li = node(
        'li',
        o.supplierName + ' — ' + o.state + ' — ' + o.totalEstimate + ' IDR estimate',
      )
      li.append(
        button('Open purchase', async () => {
          selected = purchaseDetailSchema.parse(await request('/orders/' + o.id))
          if (!disposed) drawDetail()
        }),
      )
      orders.append(li)
    }
    next.hidden = page.next === null
    next.dataset.after = page.next ?? ''
  }
  function drawDetail() {
    detail.replaceChildren()
    if (!selected) return
    const { order: o, lines, receipts, audit } = selected
    detail.dataset.orderId = o.id
    detail.append(
      node('h3', o.supplierCode + ' — ' + o.supplierName),
      node('p', o.state + ' — ' + o.totalEstimate + ' IDR estimate'),
    )
    for (const l of lines)
      detail.append(
        node(
          'p',
          l.sku +
            ' — ' +
            l.quantity +
            ' ' +
            l.unit +
            ' × ' +
            l.unitCost +
            ' IDR; accepted ' +
            l.acceptedQuantity +
            ', rejected ' +
            l.rejectedQuantity,
        ),
      )
    if (can('orders.write')) {
      const t = { id: o.id, expectedRevision: o.revision },
        actions = node('section'),
        reason = field(actions, 'reason', 'Reason', 'text', '', false, 2000)
      if (o.state === 'draft') {
        actions.append(
          button('Confirm purchase', () =>
            mutate({
              operation: 'order.confirm',
              ...t,
              reason: 'Operator confirms the reviewed supplier quote',
            }),
          ),
          button('Refresh draft quote', () =>
            mutate({
              operation: 'order.replace-lines',
              ...t,
              reason: 'Operator refreshes draft from current supplier offers',
              lines: lines.map((l) => ({ offerId: l.offerId, quantity: l.quantity })),
            }),
          ),
        )
      }
      if (o.state === 'draft' || o.state === 'confirmed')
        actions.append(
          button('Cancel purchase', () =>
            mutate({ operation: 'order.cancel', ...t, reason: reason.value }),
          ),
        )
      if (o.state === 'confirmed' || o.state === 'partial') {
        actions.append(
          button('Short-close purchase', () =>
            mutate({ operation: 'order.short-close', ...t, reason: reason.value }),
          ),
        )
        if (can('receipts.write')) {
          const f = node('form'),
            rows: { id: string; a: HTMLInputElement; r: HTMLInputElement }[] = []
          f.dataset.purchasingReceive = ''
          for (const l of lines.filter(
            (l) => l.acceptedQuantity + l.rejectedQuantity < l.quantity,
          )) {
            const group = node('div'),
              remaining = l.quantity - l.acceptedQuantity - l.rejectedQuantity
            group.append(node('h4', l.sku + ' — remaining ' + remaining + ' ' + l.unit))
            const a = field(group, 'accepted-' + l.id, 'Accepted', 'number', '0'),
              r = field(group, 'rejected-' + l.id, 'Rejected', 'number', '0')
            a.max = String(remaining)
            r.max = String(remaining)
            rows.push({ id: l.id, a, r })
            f.append(group)
          }
          const why = field(
            f,
            'reason',
            'Physical receipt reference or finding',
            'text',
            '',
            true,
            2000,
          )
          submit(f, 'Record goods receipt', () => ({
            operation: 'order.receive',
            ...t,
            reason: why.value,
            lines: rows
              .map((l) => ({
                lineId: l.id,
                accepted: Number(l.a.value),
                rejected: Number(l.r.value),
              }))
              .filter((l) => l.accepted + l.rejected > 0),
          }))
          actions.append(f)
        }
      }
      detail.append(actions)
    }
    detail.append(node('h4', 'Physical receipt findings'))
    if (receipts === null)
      detail.append(node('p', 'Receipt findings unavailable with current permissions.'))
    else
      for (const r of receipts)
        detail.append(
          node('p', r.accepted + ' accepted / ' + r.rejected + ' rejected — ' + r.reason),
        )
    if (selected.receiptsHasMore)
      detail.append(node('p', 'Showing the latest 100 receipt findings.'))
    if (audit !== null) {
      detail.append(node('h4', 'Purchase history'))
      for (const a of audit)
        detail.append(
          node('p', a.operation + ' — revision ' + a.revision + (a.reason ? ' — ' + a.reason : '')),
        )
      if (selected.auditHasMore) detail.append(node('p', 'Showing the latest 100 history entries.'))
    }
  }
  async function afterReceipt(r: PurchaseReceipt) {
    status.textContent = 'Saved ' + r.operation + '.'
    if (r.operation.startsWith('order.'))
      selected = purchaseDetailSchema.parse(await request('/orders/' + r.id))
    await reload()
  }
  async function sendPending() {
    if (!pending) return
    const saved = pending
    try {
      const r = purchaseReceiptSchema.parse(await request('/commands', saved))
      if (disposed) return
      pending = null
      uncertain = false
      recovery.hidden = true
      await afterReceipt(r)
    } catch (e) {
      if (disposed) return
      const code = e && typeof e === 'object' && 'status' in e ? e.status : undefined
      if (code === 400 || code === 409) {
        pending = null
        uncertain = false
        recovery.hidden = true
        if (selected)
          selected = purchaseDetailSchema.parse(await request('/orders/' + selected.order.id))
        drawDetail()
      } else {
        uncertain = true
        recovery.hidden = false
        status.textContent = 'Save outcome unresolved. Check or retry the same request.'
      }
      throw e
    }
  }
  async function mutate(c: unknown) {
    if (uncertain) throw new Error('Recover the previous Save before starting another.')
    pending = purchaseMutationSchema.parse({ requestId: crypto.randomUUID(), command: c })
    await sendPending()
  }
  async function reload() {
    access = purchaseAccessSchema.parse(await request('/access'))
    if (disposed) return
    supplierForm.hidden =
      !can('suppliers.read') || !can('suppliers.write') || access.scope !== 'all'
    offerForm.hidden =
      !can('suppliers.read') ||
      !can('offers.read') ||
      !can('offers.write') ||
      access.scope !== 'all'
    intake.hidden =
      !can('orders.read') || !can('orders.write') || !can('offers.read') || !can('suppliers.read')
    await loadCatalog()
    await loadOrders()
    if (selected) {
      selected = purchaseDetailSchema.parse(await request('/orders/' + selected.order.id))
      if (!disposed) drawDetail()
    }
    if (!status.textContent) status.textContent = 'Purchasing ready.'
  }
  void perform(reload)
  return {
    reload: () => perform(reload),
    destroy() {
      disposed = true
      pending = null
      selected = null
      suppliers = []
      offers = []
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
