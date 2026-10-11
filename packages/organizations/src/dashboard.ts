import { z } from 'zod'
import {
  organizationAccessSchema,
  organizationReceiptSchema,
  organizationCommandStatusSchema,
  organizationSchema,
  organizationDetailSchema,
  type OrganizationAccess,
  type OrganizationMutation,
  type OrganizationDetail,
} from './contracts.js'

export function mountOrganizationDashboard(
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
  let access: OrganizationAccess | null = null
  let pending: OrganizationMutation | null = null
  let uncertain = false
  let selected: OrganizationDetail | null = null
  let after: string | undefined

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const searchForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.orgStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.orgCreate = ''
  searchForm.dataset.orgSearch = ''
  list.dataset.orgList = ''
  detail.dataset.orgDetail = ''
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

  const createName = field(createForm, 'name', 'Organization Name', 'text', true)
  const createLegal = field(createForm, 'legalName', 'Legal Name')
  const createTax = field(createForm, 'taxId', 'Tax ID')
  const createIndustry = field(createForm, 'industry', 'Industry')
  const createSubmit = button('Create Organization', async () => {
    if (!createName.value.trim()) return
    const body: OrganizationMutation = {
      requestId: crypto.randomUUID(),
      type: 'create_organization',
      data: {
        name: createName.value.trim(),
        legalName: createLegal.value.trim() || null,
        taxId: createTax.value.trim() || null,
        industry: createIndustry.value.trim() || null,
      },
    }
    await sendCommand(body)
    createName.value = ''
    createLegal.value = ''
    createTax.value = ''
    createIndustry.value = ''
    await loadList()
  })
  createForm.appendChild(createSubmit)

  const searchInput = field(searchForm, 'query', 'Search')
  searchForm.appendChild(
    button('Search', async () => {
      after = undefined
      await loadList()
    }),
  )

  const checkPending = button('Check pending result', async () => {
    if (!pending) return
    const res = await transport(
      `${options.apiBase}/commands/${pending.requestId}`,
      { headers: { accept: 'application/json' }, signal: lifetime.signal },
    )
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const parsed = organizationCommandStatusSchema.parse(await res.json())
    if (parsed.status === 'committed') {
      uncertain = false
      recovery.hidden = true
      pending = null
      status.textContent = 'Action confirmed.'
      await loadList()
    } else {
      status.textContent = 'Command not yet committed.'
    }
  })

  const retryPending = button('Retry same request', async () => {
    if (!pending) return
    uncertain = false
    await sendCommand(pending)
    await loadList()
  })

  recovery.appendChild(node('p', 'Uncertain command outcome; inspect or retry.'))
  recovery.appendChild(checkPending)
  recovery.appendChild(retryPending)

  async function perform(work: () => Promise<void>) {
    if (busy || disposed) return
    busy = true
    status.textContent = 'Working...'
    try {
      await work()
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : 'Operation failed'
    } finally {
      busy = false
    }
  }

  async function sendCommand(mutation: OrganizationMutation) {
    pending = mutation
    uncertain = true
    recovery.hidden = false
    const res = await transport(`${options.apiBase}/commands`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'application/json' },
      body: JSON.stringify(mutation),
      signal: lifetime.signal,
    })
    if (!res.ok) {
      let code = `HTTP ${res.status}`
      try {
        const body = (await res.json()) as { error?: string }
        if (body.error) code = body.error
      } catch {}
      throw new Error(`Command failed: ${code}`)
    }
    const receipt = organizationReceiptSchema.parse(await res.json())
    uncertain = false
    recovery.hidden = !uncertain
    pending = null
    status.textContent = `Success (${receipt.type}).`
    return receipt
  }

  async function loadAccess() {
    const res = await transport(`${options.apiBase}/access`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Access load failed: ${res.status}`)
    access = organizationAccessSchema.parse(await res.json())
    const canWrite = access.permissions.includes('organizations.write')
    createForm.hidden = !canWrite
  }

  async function loadList() {
    const params = new URLSearchParams()
    params.set('limit', '20')
    if (after) params.set('after', after)
    const q = searchInput.value.trim()
    if (q) params.set('search', q)

    const res = await transport(`${options.apiBase}/records?${params.toString()}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`List load failed: ${res.status}`)
    const pageSchema = z.object({
      items: z.array(organizationSchema),
      nextCursor: z.string().nullable(),
    })
    const data = pageSchema.parse(await res.json())

    list.replaceChildren()
    for (const org of data.items) {
      const li = node('li')
      li.dataset.orgId = org.id
      li.textContent = `${org.name} [${org.state}] (rev: ${org.revision})`
      li.appendChild(
        button('View', async () => {
          await loadDetail(org.id)
        }),
      )
      list.appendChild(li)
    }
  }

  async function loadDetail(id: string) {
    const res = await transport(`${options.apiBase}/records/${id}`, {
      headers: { accept: 'application/json' },
      signal: lifetime.signal,
    })
    if (!res.ok) throw new Error(`Detail load failed: ${res.status}`)
    selected = organizationDetailSchema.parse(await res.json())
    renderDetail()
  }

  function renderDetail() {
    detail.replaceChildren()
    if (!selected) return

    const org = selected.organization
    detail.appendChild(node('h3', org.name))
    detail.appendChild(node('p', `State: ${org.state} | Industry: ${org.industry ?? '-'}`))
    if (org.taxId) detail.appendChild(node('p', `Tax ID: ${org.taxId}`))

    const canWrite = access?.permissions.includes('organizations.write') ?? false
    if (canWrite && org.state === 'active') {
      detail.appendChild(
        button('Archive', async () => {
          await sendCommand({
            requestId: crypto.randomUUID(),
            type: 'archive_organization',
            data: { id: org.id, revision: org.revision, reason: 'Manual archive from dashboard' },
          })
          await loadDetail(org.id)
          await loadList()
        }),
      )
    }

    const affSection = node('div')
    affSection.appendChild(node('h4', 'Affiliated Contacts'))
    const affList = node('ul')
    for (const aff of selected.affiliations) {
      const li = node('li')
      li.textContent = `Contact: ${aff.contactId} | Role: ${aff.role} ${aff.isPrimary ? '(Primary)' : ''} [${aff.status}]`
      if (canWrite && aff.status === 'active') {
        li.appendChild(
          button('Remove', async () => {
            await sendCommand({
              requestId: crypto.randomUUID(),
              type: 'remove_affiliation',
              data: { id: aff.id, organizationId: org.id, reason: 'Removed affiliation' },
            })
            await loadDetail(org.id)
          }),
        )
      }
      affList.appendChild(li)
    }
    affSection.appendChild(affList)

    if (canWrite && org.state === 'active') {
      const addAffForm = node('form')
      const contactInput = field(addAffForm, 'contactId', 'Contact ID (UUID)', 'text', true)
      const roleInput = field(addAffForm, 'role', 'Role', 'text', true)
      const primaryInput = field(addAffForm, 'primary', 'Is Primary (yes/no)')
      addAffForm.appendChild(
        button('Add Affiliation', async () => {
          if (!contactInput.value.trim() || !roleInput.value.trim()) return
          await sendCommand({
            requestId: crypto.randomUUID(),
            type: 'create_affiliation',
            data: {
              organizationId: org.id,
              contactId: contactInput.value.trim(),
              role: roleInput.value.trim(),
              isPrimary: primaryInput.value.trim().toLowerCase() === 'yes',
            },
          })
          await loadDetail(org.id)
        }),
      )
      affSection.appendChild(addAffForm)
    }

    detail.appendChild(affSection)
  }

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(searchForm)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  void perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Organizations ready.'
  })

  return {
    dispose() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
