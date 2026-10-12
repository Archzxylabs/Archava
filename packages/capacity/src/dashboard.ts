import {
  capacityAccessSchema,
  commandReceiptSchema,
  capacityPoolSchema,
  capacityEstimateSchema,
  type CapacityAccess,
  type CapacityMutation,
  type CapacityPoolDetail,
} from './contracts.js'

export function mountCapacityDashboard(
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
  let access: CapacityAccess | null = null
  let pending: CapacityMutation | null = null
  let uncertain = false
  let selectedPool: CapacityPoolDetail | null = null

  const status = node('p')
  const recovery = node('div')
  const createForm = node('form')
  const poolList = node('ul')
  const detail = node('section')

  status.dataset.capacityStatus = ''
  status.setAttribute('role', 'status')
  createForm.dataset.capacityCreate = ''
  poolList.dataset.capacityPoolList = ''
  poolList.dataset.capacityPools = ''
  detail.dataset.capacityDetail = ''
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

  const codeInput = field(createForm, 'code', 'Pool Code (e.g. DESIGN)', 'text', true)
  const nameInput = field(createForm, 'name', 'Team / Pool Name', 'text', true)
  const capacityInput = field(
    createForm,
    'weeklyCapacity',
    'Weekly Capacity (Hours)',
    'number',
    true,
  )
  capacityInput.value = '160'
  const maxConcurrencyInput = field(createForm, 'maxConcurrency', 'Max Concurrency', 'number', true)
  maxConcurrencyInput.value = '3'

  const createSubmit = button('Create Capacity Pool', async () => {
    if (uncertain) {
      setStatus('Uncertain command outcome; reconcile before submitting new requests.')
      return
    }
    if (!codeInput.value.trim() || !nameInput.value.trim()) return
    const body: CapacityMutation = {
      type: 'create_pool',
      commandId: crypto.randomUUID(),
      code: codeInput.value.trim().toUpperCase(),
      name: nameInput.value.trim(),
      unit: 'hours',
      weeklyCapacity: Number(capacityInput.value) || 40,
      maxConcurrentJobs: Number(maxConcurrencyInput.value) || 1,
    }
    await sendCommand(body)
    if (uncertain) return
    codeInput.value = ''
    nameInput.value = ''
    await loadPools()
  })
  createForm.appendChild(createSubmit)
  createForm.addEventListener('submit', (e) => {
    e.preventDefault()
    createSubmit.click()
  })

  async function checkPendingOutcome() {
    if (!pending) return
    try {
      const checkRes = await transport(`${options.apiBase}/commands/${pending.commandId}`, {
        headers: { accept: 'application/json' },
        signal: lifetime.signal,
      })
      if (checkRes.ok) {
        const st = (await checkRes.json()) as { status: string }
        if (st.status === 'committed') {
          pending = null
          uncertain = false
          recovery.hidden = true
          setStatus('Action confirmed.')
          await loadPools()
          return
        }
      }
      setStatus('Command not yet committed.')
    } catch {
      setStatus('Cannot verify command status with server. Retry connection.')
    }
  }

  async function retryPendingAction() {
    if (!pending) return
    await sendCommand(pending)
    await loadPools()
  }

  const retryButton = button('Retry Unknown Request', retryPendingAction)
  const retrySameButton = button('Retry same request', retryPendingAction)
  const checkPendingButton = button('Check pending result', checkPendingOutcome)
  const dismissButton = button('Dismiss Pending Request', checkPendingOutcome)

  recovery.appendChild(node('p', 'Connection lost during command. Reconciling server state...'))
  recovery.appendChild(checkPendingButton)
  recovery.appendChild(retryButton)
  recovery.appendChild(retrySameButton)
  recovery.appendChild(dismissButton)

  host.appendChild(status)
  host.appendChild(recovery)
  host.appendChild(createForm)
  host.appendChild(poolList)
  host.appendChild(detail)

  function setStatus(msg: string) {
    status.textContent = msg
  }

  async function perform(work: () => Promise<void>) {
    if (disposed || busy) return
    busy = true
    host.dataset.busy = 'true'
    setStatus('Working...')
    try {
      await work()
    } catch (err) {
      setStatus(`Error: ${err instanceof Error ? err.message : 'failed'}`)
    } finally {
      busy = false
      delete host.dataset.busy
    }
  }

  async function sendCommand(mutation: CapacityMutation) {
    if (uncertain && pending?.commandId !== mutation.commandId) {
      throw new Error('Uncertain command outcome; reconcile before submitting new requests.')
    }
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
      try {
        const checkRes = await transport(`${options.apiBase}/commands/${mutation.commandId}`, {
          signal: lifetime.signal,
        })
        if (checkRes.ok) {
          const st = (await checkRes.json()) as { status: string }
          if (st.status === 'committed') {
            uncertain = false
            recovery.hidden = true
            pending = null
            setStatus(`Confirmed committed command ${mutation.commandId}.`)
            return
          }
        }
      } catch {
        // ignore
      }
      pending = null
      uncertain = false
      recovery.hidden = true
      setStatus('Conflict: revision mismatch or conflicting request.')
      return
    }
    if (!response.ok) {
      if (response.status === 400) {
        pending = null
        uncertain = false
        recovery.hidden = true
        setStatus(`Command rejected: HTTP ${response.status}`)
        return
      }
      uncertain = true
      recovery.hidden = false
      setStatus(`Command rejected: HTTP ${response.status}`)
      return
    }

    const receipt = commandReceiptSchema.parse(await response.json())
    pending = null
    uncertain = false
    recovery.hidden = true
    setStatus(`Committed command ${receipt.commandId}. Revision ${receipt.revision}.`)
  }

  async function loadAccess() {
    const res = await transport(`${options.apiBase}/access`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load capacity access')
    access = capacityAccessSchema.parse(await res.json())
    createForm.hidden = !access.permissions.includes('capacity.write')
  }

  async function loadPools() {
    const res = await transport(`${options.apiBase}/pools`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load pools')
    const data = (await res.json()) as { items: unknown[] }
    poolList.replaceChildren()
    for (const item of data.items ?? []) {
      const pool = capacityPoolSchema.parse(item)
      const li = node('li')
      const link = node(
        'button',
        `${pool.name} [${pool.code}] (${pool.weeklyCapacity} ${pool.unit}/wk)`,
      )
      link.type = 'button'
      link.addEventListener('click', () => {
        void perform(() => loadDetail(pool.id))
      })
      li.appendChild(link)
      poolList.appendChild(li)
    }
  }

  async function loadDetail(poolId: string) {
    const res = await transport(`${options.apiBase}/pools/${poolId}`, { signal: lifetime.signal })
    if (!res.ok) throw new Error('Failed to load pool detail')
    selectedPool = (await res.json()) as CapacityPoolDetail
    renderDetail()
  }

  function renderDetail() {
    detail.replaceChildren()
    if (!selectedPool) return
    const p = selectedPool.pool
    detail.appendChild(node('h3', `${p.name} (${p.code})`))
    detail.appendChild(
      node(
        'p',
        `Weekly Capacity: ${p.weeklyCapacity} ${p.unit} | Max Concurrency: ${p.maxConcurrentJobs} | State: ${p.state}`,
      ),
    )
    detail.appendChild(node('p', `Current Queued Load: ${selectedPool.currentLoadHours} ${p.unit}`))

    // Estimate calculation form
    const estBox = node('div')
    estBox.appendChild(node('h4', 'Estimate Timeline for New Workload'))
    const reqHours = field(estBox, 'reqUnits', 'Required Units', 'number', true)
    reqHours.value = '60'
    const estResult = node('p')
    const estBtn = button('Calculate Deterministic Timeline', async () => {
      const res = await transport(`${options.apiBase}/estimate`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          poolId: p.id,
          requiredUnits: Number(reqHours.value) || 1,
        }),
        signal: lifetime.signal,
      })
      if (!res.ok) {
        estResult.textContent = `Estimation failed: HTTP ${res.status}`
        return
      }
      const est = capacityEstimateSchema.parse(await res.json())
      estResult.textContent = `Queue Pos: #${est.queuePosition} | Earliest Start: ${est.estimatedStartAt} | Finish: ${est.estimatedFinishAt} (${est.estimatedCalendarDays} days)`
    })
    estBox.appendChild(estBtn)
    estBox.appendChild(estResult)
    detail.appendChild(estBox)

    if (access?.permissions.includes('capacity.allocate') && p.state === 'active') {
      const commitBox = node('form')
      commitBox.dataset.capacityCommit = ''
      commitBox.appendChild(node('h4', 'Commit Workload Allocation'))
      const workIdInput = field(commitBox, 'workloadId', 'Workload ID', 'text', true)
      workIdInput.value = 'WORK-' + Math.floor(Math.random() * 1000)
      const workTitleInput = field(commitBox, 'workloadTitle', 'Workload Title', 'text', true)
      workTitleInput.value = 'Sprint Task Delivery'
      const workUnitsInput = field(commitBox, 'workUnits', 'Required Units', 'number', true)
      workUnitsInput.value = '40'
      const commitBtn = button('Commit Allocation', async () => {
        if (!workIdInput.value.trim() || !workTitleInput.value.trim()) return
        await sendCommand({
          type: 'commit_allocation',
          commandId: crypto.randomUUID(),
          poolId: p.id,
          workloadId: workIdInput.value.trim(),
          workloadTitle: workTitleInput.value.trim(),
          requiredUnits: Number(workUnitsInput.value) || 1,
        })
        await loadDetail(p.id)
        await loadPools()
      })
      commitBox.appendChild(commitBtn)
      commitBox.addEventListener('submit', (e) => {
        e.preventDefault()
        commitBtn.click()
      })
      detail.appendChild(commitBox)
    }

    // Active allocations
    const allocBox = node('div')
    allocBox.appendChild(node('h4', 'Active Workload Queue'))
    const ul = node('ul')
    for (const alloc of selectedPool.activeAllocations) {
      const li = node(
        'li',
        `${alloc.workloadTitle} - Remaining: ${alloc.remainingUnits}/${alloc.requiredUnits} ${p.unit} [${alloc.state}] (Scheduled: ${alloc.scheduledStartAt} -> ${alloc.scheduledFinishAt})`,
      )
      if (
        alloc.state !== 'completed' &&
        alloc.state !== 'cancelled' &&
        access?.permissions.includes('capacity.allocate')
      ) {
        const progressBtn = button('Record Progress', async () => {
          await sendCommand({
            type: 'update_allocation_progress',
            commandId: crypto.randomUUID(),
            allocationId: alloc.id,
            expectedRevision: alloc.revision,
            remainingUnits: Math.max(0, alloc.remainingUnits - 10),
          })
          await loadDetail(p.id)
        })
        li.appendChild(progressBtn)

        const completeBtn = button('Complete Workload', async () => {
          await sendCommand({
            type: 'complete_allocation',
            commandId: crypto.randomUUID(),
            allocationId: alloc.id,
            expectedRevision: alloc.revision,
          })
          await loadDetail(p.id)
        })
        li.appendChild(completeBtn)
      }
      ul.appendChild(li)
    }
    allocBox.appendChild(ul)
    detail.appendChild(allocBox)
  }

  void perform(async () => {
    await loadAccess()
    await loadPools()
    status.textContent = 'Capacity ready.'
  })

  return {
    destroy() {
      disposed = true
      lifetime.abort()
      host.replaceChildren()
    },
  }
}
