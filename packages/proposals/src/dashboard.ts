import {
  proposalAccessSchema,
  commandReceiptSchema,
  proposalSchema,
  type ProposalAccess,
  type ProposalMutation,
  type ProposalDetail,
} from './contracts.js'

export function mountProposalsDashboard(
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
  let access: ProposalAccess | null = null
  let pending: ProposalMutation | null = null
  let uncertain = false
  let selected: ProposalDetail | null = null

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const list = node('ul')
  const detail = node('section')

  status.dataset.proposalStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.proposalCreate = ''
  list.dataset.proposalList = ''
  detail.dataset.proposalDetail = ''
  recovery.hidden = true
  createForm.hidden = true

  const button = (text: string, work: () => Promise<void> | void) => {
    const b = node('button', text)
    b.type = 'button'
    b.addEventListener('click', () => {
      void perform(async () => {
        await work()
      })
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

  const createTitle = field(createForm, 'title', 'Proposal Title', 'text', true)
  const createSummary = field(createForm, 'summary', 'Summary', 'text', true)
  const createCurrency = field(createForm, 'currency', 'Currency (3 letters)', 'text', true)
  createCurrency.value = 'IDR'
  const createItemTitle = field(createForm, 'itemTitle', 'Initial Deliverable Title', 'text', true)
  const createItemDesc = field(createForm, 'itemDesc', 'Initial Deliverable Desc', 'text', true)
  const createItemVal = field(createForm, 'itemValue', 'Estimated Value', 'number', true)
  createItemVal.value = '1000000'

  const createSubmit = button('Create Proposal', async () => {
    if (!createTitle.value.trim() || !createSummary.value.trim()) return
    const body: ProposalMutation = {
      type: 'create_proposal',
      commandId: crypto.randomUUID(),
      title: createTitle.value.trim(),
      summary: createSummary.value.trim(),
      currency: createCurrency.value.trim().toUpperCase() || 'IDR',
      organizationId: null,
      contactId: null,
      leadId: null,
      scopeItems: [
        {
          title: createItemTitle.value.trim() || 'Milestone 1',
          description: createItemDesc.value.trim() || 'Initial scope deliverable',
          estimatedDays: 14,
          estimatedValue: Number(createItemVal.value) || 0,
          deliverableType: 'milestone',
        },
      ],
    }
    await sendCommand(body)
    createTitle.value = ''
    createSummary.value = ''
    await loadList()
  })
  createForm.appendChild(createSubmit)

  const retryButton = button('Retry Unknown Request', async () => {
    if (!pending) return
    uncertain = false
    recovery.hidden = true
    await sendCommand(pending)
  })

  const dismissButton = button('Dismiss Pending Request', () => {
    pending = null
    uncertain = false
    recovery.hidden = true
    setStatus('Pending request dismissed.')
  })
  recovery.appendChild(node('p', 'Connection lost during command. Reconciling server state...'))
  recovery.appendChild(retryButton)
  recovery.appendChild(dismissButton)

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(list)
  host.appendChild(detail)

  function setStatus(msg: string) {
    status.textContent = msg
  }

  async function perform(work: () => Promise<void>) {
    if (disposed || busy || uncertain) return
    busy = true
    setStatus('Working...')
    try {
      await work()
    } catch (err) {
      setStatus(`Error: ${err instanceof Error ? err.message : 'failed'}`)
    } finally {
      busy = false
    }
  }

  async function sendCommand(mutation: ProposalMutation) {
    pending = mutation
    const url = `${options.apiBase}/commands`
    let response: Response
    try {
      response = await transport(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(mutation),
        signal: lifetime.signal,
      })
    } catch {
      uncertain = true
      recovery.hidden = false
      setStatus('Network interruption. Check command status before proceeding.')
      return
    }

    if (response.status === 409) {
      pending = null
      setStatus('Conflict: revision mismatch or conflicting request.')
      return
    }
    if (!response.ok) {
      pending = null
      setStatus(`Command rejected: HTTP ${response.status}`)
      return
    }

    const receipt = commandReceiptSchema.parse(await response.json())
    pending = null
    setStatus(`Committed command ${receipt.commandId}. Revision ${receipt.revision}.`)
  }

  async function loadAccess() {
    const res = await transport(`${options.apiBase}/access`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load proposal access')
    access = proposalAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('proposals.write')
  }

  async function loadList() {
    const res = await transport(`${options.apiBase}/records`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load proposals')
    const data = (await res.json()) as { items: unknown[] }
    list.replaceChildren()
    for (const item of data.items ?? []) {
      const prop = proposalSchema.parse(item)
      const li = node('li')
      const link = node('button', `${prop.title} [${prop.state}] v${prop.currentScopeVersion}`)
      link.type = 'button'
      link.addEventListener('click', () => {
        void perform(() => loadDetail(prop.id))
      })
      li.appendChild(link)
      list.appendChild(li)
    }
  }

  async function loadDetail(id: string) {
    const res = await transport(`${options.apiBase}/records/${id}`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load proposal detail')
    selected = (await res.json()) as ProposalDetail
    renderDetail()
  }

  function renderDetail() {
    detail.replaceChildren()
    if (!selected) return
    const p = selected.proposal
    detail.appendChild(node('h3', p.title))
    detail.appendChild(
      node(
        'p',
        `State: ${p.state} | Current Scope Version: ${p.currentScopeVersion} | Revision: ${p.revision}`,
      ),
    )
    detail.appendChild(node('p', `Summary: ${p.summary}`))
    detail.appendChild(node('p', `Total Value: ${p.totalEstimatedValue} ${p.currency}`))

    if (p.state === 'draft' && access?.permissions.includes('proposals.write')) {
      const submitBtn = button('Submit Proposal for Client Review', async () => {
        await sendCommand({
          type: 'submit_proposal',
          commandId: crypto.randomUUID(),
          proposalId: p.id,
          expectedRevision: p.revision,
        })
        await loadDetail(p.id)
        await loadList()
      })
      detail.appendChild(submitBtn)
    }

    if (p.state === 'submitted' && access?.permissions.includes('proposals.decide')) {
      const acceptBtn = button('Accept Agreed Scope (Freeze Baseline v1)', async () => {
        await sendCommand({
          type: 'accept_proposal',
          commandId: crypto.randomUUID(),
          proposalId: p.id,
          expectedRevision: p.revision,
          linkedProjectId: null,
        })
        await loadDetail(p.id)
        await loadList()
      })
      detail.appendChild(acceptBtn)
    }

    const itemsSection = node('div')
    itemsSection.appendChild(node('h4', 'Deliverables / Scope Items'))
    const ul = node('ul')
    for (const item of selected.currentItems) {
      const li = node(
        'li',
        `${item.title} (${item.deliverableType}) - ${item.estimatedValue} ${p.currency} [${item.estimatedDays ?? 0} days]`,
      )
      ul.appendChild(li)
    }
    itemsSection.appendChild(ul)
    detail.appendChild(itemsSection)

    const versionsSection = node('div')
    versionsSection.appendChild(node('h4', 'Scope Version History'))
    for (const ver of selected.versions) {
      const vCard = node('div')
      vCard.appendChild(
        node(
          'p',
          `Version ${ver.scopeVersion} confirmed by ${ver.confirmedBy} at ${ver.confirmedAt}: ${ver.summary} (Total: ${ver.totalValue} ${p.currency}, ${ver.totalDays} days)`,
        ),
      )
      versionsSection.appendChild(vCard)
    }
    detail.appendChild(versionsSection)

    const crSection = node('div')
    crSection.appendChild(node('h4', 'Change Requests / Scope Amendments'))
    for (const cr of selected.changeRequests) {
      const crCard = node('div')
      crCard.appendChild(
        node(
          'p',
          `CR-${cr.changeNumber}: ${cr.title} [${cr.state}] - Reason: ${cr.reason} (Delta: ${cr.costDeltaValue} ${p.currency}, ${cr.timeDeltaDays} days)`,
        ),
      )
      if (cr.state === 'submitted' && access?.permissions.includes('changerequests.decide')) {
        const approveBtn = button(
          `Approve CR-${cr.changeNumber} (Advance Scope Version)`,
          async () => {
            await sendCommand({
              type: 'approve_change_request',
              commandId: crypto.randomUUID(),
              changeRequestId: cr.id,
              expectedRevision: cr.revision,
            })
            await loadDetail(p.id)
            await loadList()
          },
        )
        crCard.appendChild(approveBtn)
      }
      crSection.appendChild(crCard)
    }
    detail.appendChild(crSection)
  }

  void perform(async () => {
    await loadAccess()
    await loadList()
    status.textContent = 'Proposals ready.'
  })

  return {
    destroy() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
