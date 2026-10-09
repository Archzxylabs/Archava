import { crmOverviewSchema, type CrmOverview } from './overview-contracts.js'
import { DEFAULT_PIPELINE_LABELS, pipelineLabelsSchema } from './pipeline-contracts.js'

/** Shared presentation for the dashboard and Archava; no records or write controls. */
export function renderCrmOverview(
  host: HTMLElement,
  raw: CrmOverview,
  pipelineLabels: unknown = DEFAULT_PIPELINE_LABELS,
) {
  const snapshot = crmOverviewSchema.parse(raw),
    labels = pipelineLabelsSchema.parse(pipelineLabels),
    document = host.ownerDocument
  const make = <T extends keyof HTMLElementTagNameMap>(tag: T, text = '') => {
    const node = document.createElement(tag)
    node.textContent = text
    return node
  }
  const numbers = make('dl')
  numbers.dataset.crmOverviewCounts = ''
  const metric = (key: string, label: string, value: number) => {
    const number = make('dd', String(value))
    number.dataset.crmMetric = key
    numbers.append(make('dt', label), number)
  }
  metric('contacts', 'Kontak', snapshot.contacts)
  if (snapshot.followups) {
    metric('followups.open', 'Follow-up terbuka', snapshot.followups.open)
    metric('followups.overdue', 'Follow-up terlambat', snapshot.followups.overdue)
    metric('followups.upcoming', 'Follow-up mendatang', snapshot.followups.upcoming)
    metric('followups.completed', 'Follow-up selesai', snapshot.followups.completed)
  }
  if (snapshot.pipeline) {
    metric('pipeline.active', 'Peluang aktif', snapshot.pipeline.active)
    for (const stage of ['new', 'qualified', 'proposal', 'won', 'lost'] as const)
      metric('pipeline.' + stage, labels[stage], snapshot.pipeline[stage])
  }
  host.replaceChildren(
    make(
      'p',
      snapshot.scope === 'assigned'
        ? 'Ringkasan tugas dan kontak yang ditugaskan kepada Anda.'
        : 'Ringkasan workspace yang boleh Anda akses.',
    ),
    make(
      'p',
      'Diperiksa ' +
        new Date(snapshot.asOf).toLocaleString() +
        ' (' +
        Intl.DateTimeFormat().resolvedOptions().timeZone +
        ').',
    ),
    numbers,
  )
  if (!snapshot.followups)
    host.append(make('p', 'Ringkasan follow-up tidak tersedia untuk akses Anda.'))
  if (!snapshot.pipeline)
    host.append(make('p', 'Ringkasan peluang tidak tersedia untuk akses Anda.'))
}

export function mountCrmOverview(
  host: HTMLElement,
  options: {
    apiBase: string
    tenantId: string
    pipelineLabels?: unknown
    transport?: typeof fetch
  },
) {
  const document = host.ownerDocument,
    send = options.transport ?? fetch,
    title = document.createElement('h2'),
    refresh = document.createElement('button'),
    state = document.createElement('p'),
    content = document.createElement('div')
  title.textContent = 'Kondisi CRM'
  refresh.type = 'button'
  refresh.textContent = 'Perbarui ringkasan'
  refresh.dataset.crmOverviewRefresh = ''
  state.setAttribute('role', 'status')
  state.dataset.crmOverviewStatus = ''
  host.replaceChildren(title, refresh, state, content)
  let disposed = false,
    busy = false,
    active: AbortController | null = null
  async function load() {
    if (disposed || busy) return
    busy = true
    refresh.disabled = true
    content.replaceChildren()
    state.textContent = 'Memuat ringkasan…'
    active = new AbortController()
    try {
      const response = await send(options.apiBase + '/overview', {
        credentials: 'same-origin',
        signal: active.signal,
      })
      if (disposed) return
      if (!response.ok) throw new Error('Overview unavailable')
      const snapshot = crmOverviewSchema.parse(await response.json())
      if (disposed) return
      if (snapshot.tenantId !== options.tenantId) throw new Error('Workspace does not match')
      renderCrmOverview(content, snapshot, options.pipelineLabels)
      state.textContent = ''
    } catch {
      if (!disposed) {
        content.replaceChildren()
        state.textContent =
          'Ringkasan belum bisa dimuat atau izin Anda berubah. Perbarui untuk mencoba lagi.'
      }
    } finally {
      if (!disposed) {
        busy = false
        refresh.disabled = false
      }
    }
  }
  refresh.addEventListener('click', () => {
    void load()
  })
  return {
    ready: load(),
    refresh: load,
    destroy() {
      disposed = true
      active?.abort()
      host.replaceChildren()
    },
  }
}
