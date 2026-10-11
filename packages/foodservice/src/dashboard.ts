import { z } from 'zod'
import {
  foodAccessSchema,
  foodMutationSchema,
  foodReceiptSchema,
  foodTableSchema,
  menuItemSchema,
  ticketSchema,
  ticketDetailSchema,
  recipeComponentsSchema,
  type FoodAccess,
  type FoodMutation,
  type FoodCommand,
  type FoodTable,
  type MenuItem,
  type TicketDetail,
} from './contracts.js'
/** Shared owner UI. The host supplies verified sessions/CSRF; no role is accepted from this panel. */
export function mountFoodDashboard(
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
    access: FoodAccess | null = null,
    pending: FoodMutation | null = null,
    uncertain = false,
    selected: TicketDetail | null = null,
    selectedMenu: MenuItem | null = null,
    tableAfter: string | undefined,
    menuAfter: string | undefined,
    ticketAfter: string | undefined
  const menus = new Map<string, MenuItem>(),
    tables = new Map<string, FoodTable>(),
    cart = new Map<string, number>(),
    recipe = new Map<string, number>()
  const status = node('p'),
    recovery = node('div'),
    tableCreate = node('form'),
    tableList = node('ul'),
    menuCreate = node('form'),
    menuList = node('ul'),
    menuDetail = node('section'),
    recipeList = node('ul'),
    cartPanel = node('section'),
    cartList = node('ul'),
    ticketCreate = node('form'),
    ticketList = node('ul'),
    detail = node('section')
  status.dataset.foodStatus = ''
  status.setAttribute('role', 'status')
  tableCreate.dataset.foodTableCreate = ''
  tableList.dataset.foodTables = ''
  menuCreate.dataset.foodMenuCreate = ''
  menuList.dataset.foodMenu = ''
  cartPanel.dataset.foodCart = ''
  ticketCreate.dataset.foodTicketCreate = ''
  ticketList.dataset.foodTickets = ''
  detail.dataset.foodDetail = ''
  recovery.hidden = true
  tableCreate.hidden = true
  menuCreate.hidden = true
  cartPanel.hidden = true
  ticketCreate.hidden = true
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
    value = '',
    max = 160,
  ) => {
    const l = node('label', label),
      i = node('input')
    i.name = name
    i.type = type
    i.value = value
    i.required = true
    i.maxLength = max
    l.append(i)
    form.append(l)
    return i
  }
  const select = (
    form: HTMLElement,
    name: string,
    label: string,
    choices: readonly string[] = [],
  ) => {
    const l = node('label', label),
      s = node('select')
    s.name = name
    for (const c of choices) {
      const o = node('option', c)
      o.value = c
      s.append(o)
    }
    l.append(s)
    form.append(l)
    return s
  }
  const integer = (i: HTMLInputElement, min: number, max: number) => {
    i.min = String(min)
    i.max = String(max)
    i.step = '1'
  }
  const submit = (f: HTMLFormElement, text: string, command: () => FoodCommand) => {
    const b = node('button', text)
    b.type = 'submit'
    f.append(b)
    f.addEventListener('submit', (e) => {
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
  const code = field(tableCreate, 'code', 'Table code', 'text', '', 20),
    tableTitle = field(tableCreate, 'title', 'Table name'),
    seats = field(tableCreate, 'seats', 'Seats', 'number', '2')
  integer(seats, 1, 40)
  submit(tableCreate, 'Create table', () => ({
    operation: 'table.create',
    code: code.value,
    title: tableTitle.value,
    seats: Number(seats.value),
  }))
  const sku = field(menuCreate, 'sku', 'Menu SKU', 'text', '', 40),
    title = field(menuCreate, 'title', 'Menu name'),
    price = field(menuCreate, 'price', 'Price in IDR', 'number', '0'),
    station = select(menuCreate, 'station', 'Preparation station', ['kitchen', 'bar'])
  integer(price, 0, 1000000000)
  const ingredient = select(menuCreate, 'ingredientId', 'Ingredient'),
    ingredientQty = field(
      menuCreate,
      'ingredientQuantity',
      'Quantity per portion in ingredient base units',
      'number',
      '1',
    )
  integer(ingredientQty, 1, 1000000)
  ingredientQty.required = false
  menuCreate.append(
    button('Add recipe ingredient', () => {
      const id = z.uuid().parse(ingredient.value),
        quantity = z.number().int().min(1).max(1000000).parse(Number(ingredientQty.value))
      if (!recipe.has(id) && recipe.size >= 20) throw new Error('At most 20 ingredients.')
      recipe.set(id, quantity)
      drawRecipe()
      return Promise.resolve()
    }),
    recipeList,
  )
  submit(menuCreate, 'Create menu item', () => ({
    operation: 'menu.create',
    sku: sku.value,
    title: title.value,
    price: Number(price.value),
    station: z.enum(['kitchen', 'bar']).parse(station.value),
    recipe: recipeInput(),
  }))
  const cartSelect = select(cartPanel, 'menuId', 'Menu item'),
    quantity = field(cartPanel, 'quantity', 'Portions', 'number', '1')
  integer(quantity, 1, 100)
  cartPanel.append(
    button('Add ticket line', () => {
      const id = z.uuid().parse(cartSelect.value),
        n = (cart.get(id) ?? 0) + Number(quantity.value)
      z.number().int().min(1).max(100).parse(n)
      if (!menus.get(id)?.active) throw new Error('Select an active menu item.')
      if (!cart.has(id) && cart.size >= 20) throw new Error('At most 20 menu items.')
      cart.set(id, n)
      drawCart()
      return Promise.resolve()
    }),
    cartList,
  )
  const customer = field(ticketCreate, 'customerName', 'Customer or ticket name'),
    mode = select(ticketCreate, 'mode', 'Service', ['dine_in', 'takeaway']),
    tableSelect = select(ticketCreate, 'tableId', 'Dining table'),
    guests = field(ticketCreate, 'guests', 'Guests', 'number', '1')
  integer(guests, 1, 40)
  mode.addEventListener('change', () => {
    tableSelect.disabled = mode.value === 'takeaway'
  })
  submit(ticketCreate, 'Open ticket', () => ({
    operation: 'ticket.create',
    customerName: customer.value,
    mode: z.enum(['dine_in', 'takeaway']).parse(mode.value),
    ...(mode.value === 'dine_in' ? { tableId: tableSelect.value } : {}),
    guests: Number(guests.value),
    lines: [...cart].map(([menuId, quantity]) => ({ menuId, quantity })),
  }))
  const tableNext = button('Next tables', async () => {
      tableAfter = tableNext.dataset.after
      await loadTables()
    }),
    menuNext = button('Next menu items', async () => {
      menuAfter = menuNext.dataset.after
      await loadMenu()
    }),
    ticketNext = button('Next tickets', async () => {
      ticketAfter = ticketNext.dataset.after
      await loadTickets()
    })
  tableNext.hidden = true
  menuNext.hidden = true
  ticketNext.hidden = true
  const receiptStatus = z.discriminatedUnion('state', [
    z.object({ state: z.literal('unknown') }).strict(),
    z.object({ state: z.literal('committed'), receipt: foodReceiptSchema }).strict(),
  ])
  recovery.append(
    node('p', 'Result uncertain. Resolve this request before another change.'),
    button('Check pending result', async () => {
      if (!pending) return
      const result = receiptStatus.parse(await request('/commands/' + pending.requestId))
      if (result.state === 'committed') {
        pending = null
        uncertain = false
        recovery.hidden = true
        await afterReceipt(result.receipt)
      } else
        status.textContent =
          'No committed receipt yet. Retry the same request to resolve its outcome.'
    }),
    button('Retry same request', sendPending),
  )
  host.replaceChildren(
    node('h2', 'Restaurant service'),
    node('p', 'Tables, menu recipes and kitchen tickets. Closing service records no payment.'),
    status,
    recovery,
    tableCreate,
    button('Refresh restaurant', reload),
    tableList,
    tableNext,
    menuCreate,
    menuList,
    menuNext,
    menuDetail,
    cartPanel,
    ticketCreate,
    ticketList,
    ticketNext,
    detail,
  )
  async function request(path: string, body?: unknown, base = options.apiBase) {
    const response = await transport(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: lifetime.signal,
    })
    if (!response.ok) {
      const error = new Error('Request refused (' + response.status + ').') as Error & {
        status: number
      }
      error.status = response.status
      throw error
    }
    return (await response.json()) as unknown
  }
  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    lock()
    try {
      await work()
    } catch (error) {
      if (!disposed)
        status.textContent = error instanceof Error ? error.message : 'Restaurant unavailable.'
    } finally {
      busy = false
      if (!disposed) lock()
    }
  }
  function lock() {
    for (const e of host.querySelectorAll<HTMLButtonElement>('button')) e.disabled = busy
    for (const e of host.querySelectorAll<HTMLInputElement>('input')) e.disabled = busy
  }
  function can(p: FoodAccess['permissions'][number]) {
    return access?.permissions.includes(p) ?? false
  }
  function admin(p: 'tables.write' | 'menu.write') {
    return can(p) && access?.scope === 'all'
  }
  function recipeInput() {
    return recipeComponentsSchema.parse(
      [...recipe].map(([productId, quantity]) => ({ productId, quantity })),
    )
  }
  function drawRecipe() {
    recipeList.replaceChildren()
    for (const [id, n] of recipe) {
      const text = Array.from(ingredient.options).find((o) => o.value === id)?.textContent ?? id,
        l = node('li', text + ' × ' + n)
      l.append(
        button('Remove ingredient', () => {
          recipe.delete(id)
          drawRecipe()
          return Promise.resolve()
        }),
      )
      recipeList.append(l)
    }
  }
  function drawCart() {
    cartList.replaceChildren()
    for (const [id, n] of cart) {
      const m = menus.get(id),
        l = node('li', (m?.title ?? id) + ' × ' + n + ' — preview ' + money((m?.price ?? 0) * n))
      l.append(
        button('Remove ticket line', () => {
          cart.delete(id)
          drawCart()
          return Promise.resolve()
        }),
      )
      cartList.append(l)
    }
  }
  function paging(next: HTMLButtonElement, after: string | null) {
    next.hidden = after === null
    next.dataset.after = after ?? ''
  }
  async function loadTables() {
    if (!can('tables.read')) return
    const page = z
      .object({
        items: z.array(foodTableSchema),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/tables' + (tableAfter ? '?after=' + tableAfter : '')))
    if (disposed) return
    tableList.replaceChildren()
    for (const b of page.items) {
      tables.set(b.id, b)
      const l = node(
        'li',
        b.title +
          ' (' +
          b.code +
          ') — ' +
          b.seats +
          ' seats — ' +
          (b.active ? 'active' : 'inactive') +
          ' — ' +
          (b.occupied ? 'occupied' : 'available'),
      )
      if (admin('tables.write'))
        l.append(
          button(b.active ? 'Disable table' : 'Enable table', () =>
            mutate({
              operation: 'table.set-active',
              id: b.id,
              expectedRevision: b.revision,
              active: !b.active,
              reason: 'Owner table availability update',
            }),
          ),
        )
      tableList.append(l)
    }
    paging(tableNext, page.next)
    tableSelect.replaceChildren()
    for (const b of tables.values())
      if (b.active && !b.occupied) {
        const o = node('option', b.title + ' — ' + b.seats + ' seats')
        o.value = b.id
        tableSelect.append(o)
      }
  }
  async function loadMenu() {
    if (!can('menu.read')) return
    const page = z
      .object({
        items: z.array(menuItemSchema),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/menu' + (menuAfter ? '?after=' + menuAfter : '')))
    if (disposed) return
    menuList.replaceChildren()
    for (const m of page.items) {
      menus.set(m.id, m)
      const l = node(
        'li',
        m.title +
          ' — ' +
          money(m.price) +
          ' — recipe v' +
          m.recipeVersion +
          ' — ' +
          (m.active ? 'active' : 'inactive'),
      )
      l.append(
        button('Open menu item', () => {
          selectedMenu = m
          drawMenu()
          return Promise.resolve()
        }),
      )
      menuList.append(l)
    }
    paging(menuNext, page.next)
    cartSelect.replaceChildren()
    for (const m of menus.values())
      if (m.active) {
        const o = node('option', m.title + ' — ' + money(m.price))
        o.value = m.id
        cartSelect.append(o)
      }
    drawCart()
  }
  function drawMenu() {
    menuDetail.replaceChildren()
    if (!selectedMenu) return
    const m = selectedMenu
    menuDetail.append(
      node('h3', m.title),
      node('p', m.sku + ' — ' + money(m.price) + ' — recipe v' + m.recipeVersion),
    )
    if (admin('menu.write')) {
      const form = node('form'),
        rate = field(form, 'price', 'New menu price', 'number', String(m.price)),
        reason = field(form, 'reason', 'Reason')
      integer(rate, 0, 1000000000)
      submit(form, 'Update menu price', () => ({
        operation: 'menu.set-price',
        id: m.id,
        expectedRevision: m.revision,
        price: Number(rate.value),
        reason: reason.value,
      }))
      menuDetail.append(
        form,
        button(m.active ? 'Disable menu item' : 'Enable menu item', () =>
          mutate({
            operation: 'menu.set-active',
            id: m.id,
            expectedRevision: m.revision,
            active: !m.active,
            reason: 'Owner menu availability update',
          }),
        ),
        button('Use recipe builder for new version', () =>
          mutate({
            operation: 'menu.replace-recipe',
            id: m.id,
            expectedRevision: m.revision,
            recipe: recipeInput(),
            reason: 'Owner replaces recipe for future tickets',
          }),
        ),
      )
    }
  }
  async function loadTickets() {
    if (!can('tickets.read')) return
    const page = z
      .object({
        items: z.array(ticketSchema),
        next: z.uuid().nullable(),
        evaluatedAt: z.iso.datetime(),
      })
      .strict()
      .parse(await request('/tickets' + (ticketAfter ? '?after=' + ticketAfter : '')))
    if (disposed) return
    ticketList.replaceChildren()
    for (const t of page.items) {
      const l = node(
        'li',
        t.customerName +
          ' — ' +
          t.state +
          (t.stockExpired ? ' (stock hold expired)' : '') +
          ' — ' +
          money(t.totalEstimate),
      )
      l.append(
        button('Open ticket', async () => {
          selected = ticketDetailSchema.parse(await request('/tickets/' + t.id))
          if (!disposed) drawDetail()
        }),
      )
      ticketList.append(l)
    }
    paging(ticketNext, page.next)
  }
  function drawDetail() {
    detail.replaceChildren()
    if (!selected) return
    const { ticket: t, lines } = selected
    detail.dataset.ticketId = t.id
    detail.append(
      node('h3', t.customerName),
      node('p', t.mode + ' — ' + t.state + ' — ' + money(t.totalEstimate)),
      node(
        'p',
        t.stockExpired
          ? 'Stock reservation expired. Cancel and open a new ticket.'
          : 'Service state is independent of payment.',
      ),
    )
    for (const l of lines)
      detail.append(
        node(
          'p',
          l.title +
            ' × ' +
            l.quantity +
            ' — recipe v' +
            l.recipeVersion +
            ' — ' +
            l.station +
            ' — ' +
            money(l.price * l.quantity),
        ),
      )
    const target = { id: t.id, expectedRevision: t.revision }
    if (t.state === 'draft' && can('tickets.write'))
      detail.append(
        button('Queue for kitchen', () =>
          mutate({ operation: 'ticket.queue', ...target, holdSeconds: 900 }),
        ),
      )
    if (t.state === 'queued' && !t.stockExpired && can('kitchen.write'))
      detail.append(
        button('Start preparation', () => mutate({ operation: 'ticket.prepare', ...target })),
      )
    if (t.state === 'preparing' && can('kitchen.write'))
      detail.append(button('Mark ready', () => mutate({ operation: 'ticket.ready', ...target })))
    if (t.state === 'ready' && can('tickets.write'))
      detail.append(button('Mark served', () => mutate({ operation: 'ticket.serve', ...target })))
    if (t.state === 'served' && can('tickets.write'))
      detail.append(button('Close service', () => mutate({ operation: 'ticket.close', ...target })))
    if (t.state !== 'closed' && t.state !== 'cancelled' && can('tickets.write')) {
      const form = node('form'),
        reason = field(
          form,
          'reason',
          t.state === 'draft' || t.state === 'queued'
            ? 'Cancellation reason'
            : 'Waste reason (consumed ingredients stay consumed)',
        )
      submit(form, 'Cancel ticket', () => ({
        operation: 'ticket.cancel',
        ...target,
        reason: reason.value,
      }))
      detail.append(form)
    }
  }
  async function loadIngredients() {
    if (!admin('menu.write')) return
    let after: string | undefined
    const list: unknown[] = []
    do {
      const page = z
        .object({
          items: z.array(
            z
              .object({
                id: z.uuid(),
                title: z.string(),
                unit: z.enum(['unit', 'gram', 'millilitre']),
                active: z.boolean(),
                trackStock: z.boolean(),
              })
              .passthrough(),
          ),
          next: z.uuid().nullable(),
          evaluatedAt: z.iso.datetime(),
        })
        .strict()
        .parse(
          await request(
            '/products?limit=100' + (after ? '&after=' + after : ''),
            undefined,
            options.commerceApiBase,
          ),
        )
      list.push(...page.items)
      after = page.next ?? undefined
      if (list.length > 1000)
        throw new Error(
          'Ingredient selector supports up to 1000 catalog items. Use the API for a larger catalog.',
        )
    } while (after && !disposed)
    if (disposed) return
    ingredient.replaceChildren()
    for (const raw of list) {
      const p = z
        .object({
          id: z.uuid(),
          title: z.string(),
          unit: z.string(),
          active: z.boolean(),
          trackStock: z.boolean(),
        })
        .passthrough()
        .parse(raw)
      if (p.active && p.trackStock) {
        const o = node('option', p.title + ' [' + p.unit + ']')
        o.value = p.id
        ingredient.append(o)
      }
    }
  }
  async function reload() {
    access = foodAccessSchema.parse(await request('/access'))
    if (disposed) return
    tableCreate.hidden = !admin('tables.write')
    menuCreate.hidden = !admin('menu.write')
    cartPanel.hidden = !can('tickets.write') || !can('menu.read')
    ticketCreate.hidden = !can('tickets.write') || !can('menu.read')
    await loadTables()
    await loadMenu()
    await loadIngredients()
    await loadTickets()
    if (selected) {
      selected = ticketDetailSchema.parse(await request('/tickets/' + selected.ticket.id))
      if (!disposed) drawDetail()
    }
    if (selectedMenu) {
      selectedMenu = menus.get(selectedMenu.id) ?? null
      drawMenu()
    }
    if (!pending) status.textContent = 'Restaurant ready.'
  }
  async function afterReceipt(receipt: z.infer<typeof foodReceiptSchema>) {
    if (disposed) return
    status.textContent = 'Saved.'
    if (receipt.operation === 'ticket.create') cart.clear()
    if (receipt.operation.startsWith('ticket.'))
      selected = ticketDetailSchema.parse(await request('/tickets/' + receipt.id))
    await reload()
  }
  async function mutate(command: FoodCommand) {
    if (pending) throw new Error('Resolve the pending request first.')
    pending = foodMutationSchema.parse({ requestId: crypto.randomUUID(), command })
    uncertain = false
    await sendPending()
  }
  async function sendPending() {
    if (!pending) return
    try {
      const receipt = foodReceiptSchema.parse(await request('/commands', pending))
      pending = null
      uncertain = false
      recovery.hidden = true
      await afterReceipt(receipt)
    } catch (error) {
      if (!pending) throw error
      const code =
        error && typeof error === 'object' && 'status' in error ? Number(error.status) : null
      if (code !== null && [400, 401, 403, 404, 409, 413].includes(code) && !uncertain) {
        pending = null
        recovery.hidden = true
      } else {
        uncertain = true
        recovery.hidden = false
      }
      throw error
    }
  }
  void perform(reload)
  return {
    refresh: () => perform(reload),
    destroy() {
      disposed = true
      lifetime.abort()
      pending = null
      selected = null
      selectedMenu = null
      menus.clear()
      tables.clear()
      cart.clear()
      recipe.clear()
      host.replaceChildren()
    },
  }
}
