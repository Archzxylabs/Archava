import { z } from 'zod'
import {
  commerceAccessSchema,
  commerceMutationSchema,
  commerceReceiptSchema,
  commerceCommandStatusSchema,
  productSchema,
  stockRecordSchema,
  orderSchema,
  orderDetailSchema,
  orderLinesInputSchema,
  type CommerceAccess,
  type CommerceMutation,
  type CommerceCommand,
  type Product,
  type OrderDetail,
} from './contracts.js'

/** Host supplies session and CSRF. Every stock/order effect is a human command validated by the backend. */
export function mountCommerceDashboard(
  host: HTMLElement,
  options: { apiBase: string; transport?: typeof fetch },
) {
  const document = host.ownerDocument,
    transport = options.transport ?? fetch,
    lifetime = new AbortController()
  const node = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const e = document.createElement(tag)
    e.textContent = text
    return e
  }
  let disposed = false,
    busy = false,
    access: CommerceAccess | null = null,
    pending: CommerceMutation | null = null,
    uncertain = false
  let selectedProduct: Product | null = null,
    selectedOrder: OrderDetail | null = null,
    productAfter: string | undefined,
    orderAfter: string | undefined
  const catalog = new Map<string, Product>(),
    cart = new Map<string, number>()
  const status = node('p'),
    recovery = node('div'),
    productCreate = node('form'),
    productList = node('ul'),
    productDetail = node('section'),
    cartPanel = node('section'),
    cartList = node('ul'),
    orderCreate = node('form'),
    orderList = node('ul'),
    orderDetail = node('section')
  status.dataset.commerceStatus = ''
  status.setAttribute('role', 'status')
  productCreate.dataset.commerceProductCreate = ''
  productList.dataset.commerceProductList = ''
  productDetail.dataset.commerceProductDetail = ''
  cartPanel.dataset.commerceCart = ''
  orderCreate.dataset.commerceOrderCreate = ''
  orderList.dataset.commerceOrderList = ''
  orderDetail.dataset.commerceOrderDetail = ''
  recovery.hidden = true
  productCreate.hidden = true
  cartPanel.hidden = true
  orderCreate.hidden = true
  const field = (
    form: HTMLElement,
    name: string,
    label: string,
    type = 'text',
    value = '',
    maximum = 160,
  ) => {
    const wrapper = node('label', label),
      input = node('input')
    input.name = name
    input.type = type
    input.value = value
    input.required = true
    input.maxLength = maximum
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
  const submit = (form: HTMLFormElement, text: string, command: () => CommerceCommand) => {
    const b = node('button', text)
    b.type = 'submit'
    form.append(b)
    form.addEventListener('submit', (e) => {
      e.preventDefault()
      void perform(() => mutate(command()))
    })
  }
  const money = (n: number) =>
    new Intl.NumberFormat('id-ID', {
      style: 'currency',
      currency: 'IDR',
      maximumFractionDigits: 0,
    }).format(n)
  const sku = field(productCreate, 'sku', 'SKU', 'text', '', 40),
    title = field(productCreate, 'title', 'Product name'),
    price = field(productCreate, 'price', 'Price in IDR per base unit', 'number', '0')
  price.min = '0'
  price.max = '1000000000'
  price.step = '1'
  const unit = node('select')
  unit.name = 'unit'
  for (const name of ['unit', 'gram', 'millilitre']) {
    const o = node('option', name)
    o.value = name
    unit.append(o)
  }
  const unitLabel = node('label', 'Base unit')
  unitLabel.append(unit)
  productCreate.append(unitLabel)
  const track = field(productCreate, 'trackStock', 'Track physical stock', 'checkbox')
  track.required = false
  track.checked = true
  submit(productCreate, 'Create product', () => ({
    operation: 'product.create',
    sku: sku.value,
    title: title.value,
    price: Number(price.value),
    trackStock: track.checked,
    unit: z.enum(['unit', 'gram', 'millilitre']).parse(unit.value),
  }))
  const cartSelect = node('select')
  cartSelect.name = 'productId'
  const cartLabel = node('label', 'Product')
  cartLabel.append(cartSelect)
  cartPanel.append(node('h3', 'Order lines'), cartLabel)
  const quantity = field(cartPanel, 'quantity', 'Quantity in base units', 'number', '1')
  quantity.min = '1'
  quantity.max = '10000'
  quantity.step = '1'
  cartPanel.append(
    button('Add order line', () => {
      const id = z.uuid().parse(cartSelect.value),
        qty = z.number().int().min(1).max(10000).parse(Number(quantity.value))
      if (!catalog.has(id)) throw new Error('Select a visible product.')
      const total = (cart.get(id) ?? 0) + qty
      z.number().int().min(1).max(10000).parse(total)
      if (!cart.has(id) && cart.size >= 50) throw new Error('At most 50 products per order.')
      cart.set(id, total)
      drawCart()
      return Promise.resolve()
    }),
    cartList,
  )
  const customer = field(orderCreate, 'customerName', 'Customer or order name')
  submit(orderCreate, 'Create draft order', () => ({
    operation: 'order.create',
    customerName: customer.value,
    lines: orderLinesInputSchema.parse(
      [...cart].map(([productId, quantity]) => ({ productId, quantity })),
    ),
  }))
  const productNext = button('Next products', async () => {
      productAfter = productNext.dataset.after
      await loadProducts()
    }),
    orderNext = button('Next orders', async () => {
      orderAfter = orderNext.dataset.after
      await loadOrders()
    })
  productNext.hidden = true
  orderNext.hidden = true
  recovery.append(
    node('p', 'Result uncertain. Check or retry the same request before another change.'),
    button('Check pending result', async () => {
      if (!pending) return
      const result = commerceCommandStatusSchema.parse(
        await request('/commands/' + pending.requestId),
      )
      if (result.status === 'committed') {
        pending = null
        uncertain = false
        recovery.hidden = true
        await afterReceipt(result.receipt)
      } else
        status.textContent =
          'No committed receipt found yet. Retry the same request to resolve its outcome.'
    }),
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Commerce'),
    node(
      'p',
      'Catalog, physical stock and order fulfillment. Fulfillment and returns do not record payment or refunds.',
    ),
    status,
    recovery,
    productCreate,
    button('Refresh commerce', reload),
    productList,
    productNext,
    productDetail,
    cartPanel,
    orderCreate,
    orderList,
    orderNext,
    orderDetail,
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
            ? 'The stock, order or revision changed. Refresh and check.'
            : response.status === 403
              ? 'Current permissions refuse this request.'
              : 'Commerce request could not be completed.',
        )
      }
      return await response.json()
    } finally {
      clearTimeout(timer)
      lifetime.signal.removeEventListener('abort', cancel)
    }
  }
  async function afterReceipt(receipt: z.infer<typeof commerceReceiptSchema>) {
    if (disposed) return
    if (receipt.kind === 'order') {
      cart.clear()
      drawCart()
    }
    await reload()
    if (receipt.kind === 'order') await showOrder(receipt.id)
    else await showProduct(receipt.id)
  }
  async function sendPending() {
    if (!pending || disposed) return
    let receipt
    try {
      receipt = commerceReceiptSchema.parse(
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
    await afterReceipt(receipt)
  }
  async function mutate(command: CommerceCommand) {
    if (pending) throw new Error('Resolve the pending request first.')
    pending = commerceMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    uncertain = false
    try {
      await sendPending()
    } finally {
      if (!disposed) recovery.hidden = pending === null
    }
  }
  function drawCart() {
    cartList.replaceChildren()
    for (const [id, qty] of cart) {
      const row = node('li', (catalog.get(id)?.title ?? id) + ' × ' + qty)
      row.append(
        button('Remove line', () => {
          cart.delete(id)
          drawCart()
          return Promise.resolve()
        }),
      )
      cartList.append(row)
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
  function query(after?: string) {
    const q = new URLSearchParams({ limit: '20' })
    if (after) q.set('after', after)
    return q.toString()
  }
  async function loadProducts() {
    if (!access?.permissions.includes('catalog.read')) {
      productList.replaceChildren()
      return
    }
    const result = page(productSchema).parse(await request('/products?' + query(productAfter)))
    if (disposed) return
    productList.replaceChildren()
    for (const p of result.items) {
      catalog.set(p.id, p)
      const row = node('li')
      row.append(
        button(
          p.sku +
            ' · ' +
            p.title +
            ' · ' +
            money(p.price) +
            '/' +
            p.unit +
            (p.active ? '' : ' · inactive'),
          () => showProduct(p.id),
        ),
      )
      productList.append(row)
    }
    productNext.hidden = result.next === null
    productNext.dataset.after = result.next ?? ''
    cartSelect.replaceChildren()
    for (const p of catalog.values()) {
      if (!p.active) continue
      const o = node('option', p.sku + ' · ' + p.title)
      o.value = p.id
      cartSelect.append(o)
    }
  }
  async function loadOrders() {
    if (!access?.permissions.includes('orders.read')) {
      orderList.replaceChildren()
      return
    }
    const result = page(orderSchema).parse(await request('/orders?' + query(orderAfter)))
    if (disposed) return
    orderList.replaceChildren()
    for (const o of result.items) {
      const row = node('li')
      row.append(
        button(o.customerName + ' · ' + o.effectiveState + ' · ' + money(o.totalEstimate), () =>
          showOrder(o.id),
        ),
      )
      orderList.append(row)
    }
    orderNext.hidden = result.next === null
    orderNext.dataset.after = result.next ?? ''
  }
  async function showProduct(id: string) {
    const p = productSchema.parse(await request('/products/' + id)),
      stock = access?.permissions.includes('stock.read')
        ? stockRecordSchema.parse(await request('/stock/' + id))
        : null
    if (disposed) return
    selectedProduct = p
    catalog.set(p.id, p)
    productDetail.replaceChildren(
      node('h3', p.title),
      node(
        'p',
        p.sku +
          ' · ' +
          money(p.price) +
          '/' +
          p.unit +
          (p.trackStock ? ' · physical stock' : ' · stock not tracked'),
      ),
    )
    if (stock)
      productDetail.append(
        node(
          'p',
          'On hand: ' +
            stock.onHand +
            ' · Reserved: ' +
            stock.reserved +
            ' · Available: ' +
            stock.available +
            ' · ' +
            stock.evaluatedAt,
        ),
      )
    if (access?.scope === 'all' && access.permissions.includes('catalog.write')) {
      const form = node('form')
      form.dataset.commercePrice = ''
      const rate = field(form, 'price', 'New unit price', 'number', String(p.price)),
        why = field(form, 'reason', 'Reason', 'text', '', 2000)
      rate.min = '0'
      rate.max = '1000000000'
      rate.step = '1'
      submit(form, 'Update price', () => ({
        operation: 'product.set-price',
        id: p.id,
        expectedRevision: p.revision,
        price: Number(rate.value),
        reason: why.value,
      }))
      productDetail.append(form)
      productDetail.append(
        button(p.active ? 'Disable product' : 'Enable product', () =>
          mutate({
            operation: 'product.set-active',
            id: p.id,
            expectedRevision: p.revision,
            active: !p.active,
            reason: p.active ? 'Owner disabled product' : 'Owner enabled product',
          }),
        ),
      )
    }
    if (
      stock &&
      p.trackStock &&
      access?.scope === 'all' &&
      access.permissions.includes('stock.write')
    ) {
      const form = node('form')
      form.dataset.commerceStockAdjust = ''
      const delta = field(form, 'quantity', 'Stock adjustment (+received / -removed)', 'number'),
        why = field(form, 'reason', 'Evidence or reason', 'text', '', 2000)
      delta.min = '-1000000000'
      delta.max = '1000000000'
      delta.step = '1'
      submit(form, 'Record stock adjustment', () => ({
        operation: 'stock.adjust',
        productId: p.id,
        expectedRevision: stock.revision,
        quantity: Number(delta.value),
        reason: why.value,
      }))
      productDetail.append(form)
    }
  }
  async function showOrder(id: string) {
    const detail = orderDetailSchema.parse(await request('/orders/' + id))
    if (disposed) return
    selectedOrder = detail
    const o = detail.order
    orderDetail.replaceChildren(
      node('h3', o.customerName),
      node(
        'p',
        o.effectiveState +
          ' · ' +
          money(o.totalEstimate) +
          ' · Revision ' +
          o.revision +
          (o.expiresAt ? ' · Hold expires ' + o.expiresAt : ''),
      ),
    )
    const lines = node('ul')
    for (const l of detail.lines)
      lines.append(
        node(
          'li',
          l.sku +
            ' · ' +
            l.title +
            ' × ' +
            l.quantity +
            ' · ' +
            money(l.price) +
            '/' +
            l.unit +
            ' · Returned ' +
            l.returnedQuantity,
        ),
      )
    orderDetail.append(lines)
    if (o.managedBy) {
      orderDetail.append(node('p', 'Kitchen workflow controls this order.'))
      return
    }
    if (!access?.permissions.includes('orders.write')) return
    const ref = { id: o.id, expectedRevision: o.revision }
    if (o.state === 'draft') {
      orderDetail.append(
        button('Reserve stock', () =>
          mutate({ operation: 'order.reserve', ...ref, holdSeconds: 900 }),
        ),
      )
      const form = node('form')
      form.dataset.commerceReplace = ''
      const why = field(form, 'reason', 'Reason for replacing draft lines', 'text', '', 2000)
      submit(form, 'Replace draft lines from cart', () => ({
        operation: 'order.replace-lines',
        ...ref,
        lines: orderLinesInputSchema.parse(
          [...cart].map(([productId, quantity]) => ({ productId, quantity })),
        ),
        reason: why.value,
      }))
      orderDetail.append(form)
    }
    if (o.effectiveState === 'reserved')
      orderDetail.append(
        button('Fulfill order', () => mutate({ operation: 'order.fulfill', ...ref })),
      )
    if (o.state === 'draft' || o.state === 'reserved') {
      const form = node('form')
      form.dataset.commerceCancel = ''
      const why = field(form, 'reason', 'Cancellation reason', 'text', '', 2000)
      submit(form, 'Cancel order', () => ({ operation: 'order.cancel', ...ref, reason: why.value }))
      orderDetail.append(form)
    }
    if (o.state === 'fulfilled' && access.permissions.includes('returns.write')) {
      const form = node('form')
      form.dataset.commerceReturn = ''
      const inputs: {
        lineId: string
        quantity: HTMLInputElement
        disposition: HTMLSelectElement
      }[] = []
      for (const l of detail.lines) {
        if (l.returnedQuantity === l.quantity) continue
        const input = field(form, l.id, l.title + ' quantity received', 'number', '0')
        input.min = '0'
        input.max = String(l.quantity - l.returnedQuantity)
        input.step = '1'
        const disposition = node('select')
        disposition.name = 'disposition-' + l.id
        for (const state of access.permissions.includes('stock.write')
          ? ['restock', 'discard']
          : ['discard']) {
          const option = node(
            'option',
            state === 'restock' ? 'Received in sellable condition' : 'Received but discarded',
          )
          option.value = state
          disposition.append(option)
        }
        form.append(disposition)
        inputs.push({ lineId: l.id, quantity: input, disposition })
      }
      const why = field(form, 'reason', 'Return reason', 'text', '', 2000)
      submit(form, 'Record received return', () => ({
        operation: 'order.return',
        ...ref,
        lines: inputs
          .filter((x) => Number(x.quantity.value) > 0)
          .map((x) => ({
            lineId: x.lineId,
            quantity: Number(x.quantity.value),
            disposition: z.enum(['restock', 'discard']).parse(x.disposition.value),
          })),
        reason: why.value,
      }))
      orderDetail.append(form)
    }
  }
  async function reload() {
    const fresh = commerceAccessSchema.parse(await request('/access'))
    if (disposed) return
    access = fresh
    productCreate.hidden = fresh.scope !== 'all' || !fresh.permissions.includes('catalog.write')
    cartPanel.hidden =
      !fresh.permissions.includes('orders.write') || !fresh.permissions.includes('catalog.read')
    orderCreate.hidden = cartPanel.hidden
    await loadProducts()
    await loadOrders()
    if (selectedProduct && fresh.permissions.includes('catalog.read'))
      await showProduct(selectedProduct.id)
    if (selectedOrder && fresh.permissions.includes('orders.read'))
      await showOrder(selectedOrder.order.id)
  }
  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    status.textContent = 'Working…'
    try {
      await work()
      if (!disposed && !pending) status.textContent = ''
    } catch (error) {
      if (!disposed) status.textContent = error instanceof Error ? error.message : 'Request failed.'
    } finally {
      busy = false
    }
  }
  const ready = perform(reload)
  return {
    ready,
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      uncertain = false
      access = null
      selectedProduct = null
      selectedOrder = null
      catalog.clear()
      cart.clear()
      host.replaceChildren()
    },
  }
}
