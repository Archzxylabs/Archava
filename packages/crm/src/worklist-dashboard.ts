import { followupQueueSchema, type FollowupQueue } from './worklist-contracts.js'

export interface CrmWorklistOptions {
  readonly apiBase: string
  readonly transport?: typeof fetch
  readonly onContactSelected: (id: string) => Promise<void> | void
  readonly onFollowupSelected?: (id: string) => void
}

/** Shared read-only worklist. Opening a contact reuses the host's authorized detail flow. */
export function mountCrmWorklist(host: HTMLElement, options: CrmWorklistOptions) {
  const document = host.ownerDocument
  const send = options.transport ?? fetch
  const make = <T extends keyof HTMLElementTagNameMap>(tag: T, text = '') => {
    const element = document.createElement(tag)
    element.textContent = text
    return element
  }
  const bucket = make('select')
  bucket.dataset.crmWorkBucket = ''
  bucket.setAttribute('aria-label', 'Filter follow-up')
  for (const [value, label] of [
    ['open', 'Semua terbuka'],
    ['overdue', 'Terlambat'],
    ['upcoming', 'Mendatang'],
  ] as const) {
    const option = make('option', label)
    option.value = value
    bucket.append(option)
  }
  const search = make('input')
  search.maxLength = 120
  search.placeholder = 'Cari tugas atau kontak'
  search.setAttribute('aria-label', 'Cari tugas atau kontak')
  const form = make('form')
  form.dataset.crmWorkForm = ''
  const refresh = make('button', 'Perbarui daftar')
  refresh.type = 'submit'
  const next = make('button', 'Tugas berikutnya')
  next.type = 'button'
  next.hidden = true
  next.dataset.crmWorkNext = ''
  const state = make('p')
  state.setAttribute('role', 'status')
  state.dataset.crmWorkStatus = ''
  const list = make('ul')
  list.dataset.crmWorklist = ''
  form.append(bucket, search, refresh)
  host.replaceChildren(make('h2', 'Daftar kerja follow-up'), form, state, list, next)
  let disposed = false
  let busy = false
  let page: FollowupQueue | null = null
  let active: AbortController | null = null
  async function load(more = false) {
    if (disposed || busy) return
    busy = true
    refresh.disabled = true
    search.disabled = true
    bucket.disabled = true
    next.disabled = true
    list.replaceChildren()
    state.textContent = 'Memuat tugas…'
    active = new AbortController()
    try {
      const query = new URLSearchParams({ bucket: bucket.value, search: search.value, limit: '20' })
      if (more && page?.next) {
        query.set('asOf', page.asOf)
        query.set('afterDueAt', page.next.dueAt)
        query.set('afterId', page.next.id)
      }
      const response = await send(options.apiBase + '/followups?' + query.toString(), {
        credentials: 'same-origin',
        signal: active.signal,
      })
      if (disposed) return
      if (!response.ok)
        throw new Error(
          response.status === 403
            ? 'Anda tidak punya izin untuk daftar tugas ini.'
            : 'Daftar tugas belum bisa dimuat.',
        )
      const result = followupQueueSchema.parse(await response.json())
      if (disposed) return
      page = result
      for (const item of result.items) {
        const row = make('li')
        const status =
          new Date(item.dueAt).getTime() < new Date(result.asOf).getTime()
            ? 'Terlambat'
            : 'Mendatang'
        row.append(
          make('p', `${status} · ${new Date(item.dueAt).toLocaleString()} · ${item.summary}`),
        )
        const open = make('button', 'Buka kontak: ' + item.fullName)
        open.type = 'button'
        open.dataset.crmWorkContact = item.contactId
        open.addEventListener('click', () => {
          if (disposed || busy) return
          void Promise.resolve()
            .then(() => options.onContactSelected(item.contactId))
            .catch(() => {
              if (!disposed) state.textContent = 'Kontak belum bisa dibuka. Perbarui daftar.'
            })
        })
        row.append(open)
        if (options.onFollowupSelected) {
          const choose = make('button', 'Tinjau penyelesaian di Archava')
          choose.type = 'button'
          choose.dataset.crmWorkComplete = item.id
          choose.addEventListener('click', () => {
            if (!disposed && !busy) options.onFollowupSelected?.(item.id)
          })
          row.append(choose)
        }
        list.append(row)
      }
      state.textContent = result.items.length
        ? `${result.items.length} tugas pada halaman ini. Diperiksa ${new Date(result.asOf).toLocaleString()} (${Intl.DateTimeFormat().resolvedOptions().timeZone}).`
        : 'Tidak ada tugas yang cocok dalam data yang boleh Anda akses.'
      next.hidden = !result.next
    } catch {
      if (!disposed) {
        page = null
        next.hidden = true
        state.textContent =
          'Daftar tugas belum bisa dimuat atau izin Anda berubah. Perbarui untuk mencoba lagi.'
      }
    } finally {
      if (!disposed) {
        busy = false
        refresh.disabled = false
        search.disabled = false
        bucket.disabled = false
        next.disabled = false
      }
    }
  }
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    void load()
  })
  bucket.addEventListener('change', () => {
    void load()
  })
  search.addEventListener('input', () => {
    page = null
    next.hidden = true
  })
  next.addEventListener('click', () => {
    void load(true)
  })
  return {
    ready: load(),
    refresh: () => load(),
    destroy: () => {
      disposed = true
      active?.abort()
      page = null
      host.replaceChildren()
    },
  }
}
