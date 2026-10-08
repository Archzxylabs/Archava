import { conversationReplySchema, type CrmConversationData } from './contracts.js'

/** Shared UI. It renders records as text and holds no credentials or durable conversation state. */
export function mountCrmConversation(
  host: HTMLElement,
  options: {
    apiBase: string
    transport?: typeof fetch
  },
) {
  let destroyed = false
  let busy = false
  let selected: { id: string; fullName: string } | null = null
  let active: AbortController | null = null
  const transport = options.transport ?? fetch
  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, text = '') => {
    const element = document.createElement(tag)
    element.textContent = text
    return element
  }
  const title = make('h2', 'Archava · CRM')
  const help = make(
    'p',
    'Coba “tampilkan kontak”, “cari Budi”, atau “ringkas Budi”. Draft follow-up belum disimpan atau dikirim.',
  )
  const selectedLabel = make('p', 'Belum ada kontak dipilih.')
  selectedLabel.dataset.archavaSelected = ''
  const clear = make('button', 'Hapus pilihan')
  clear.type = 'button'
  clear.addEventListener('click', () => {
    selected = null
    selectedLabel.textContent = 'Belum ada kontak dipilih.'
  })
  const log = make('div')
  log.dataset.archavaLog = ''
  log.setAttribute('aria-live', 'polite')
  const feedback = make('p')
  feedback.dataset.archavaStatus = ''
  feedback.setAttribute('role', 'status')
  const form = make('form')
  form.dataset.archavaForm = ''
  const label = make('label', 'Pesan untuk Archava')
  const message = make('input')
  message.name = 'utterance'
  message.maxLength = 1000
  message.required = true
  message.autocomplete = 'off'
  label.append(message)
  const details = make('details')
  details.append(make('summary', 'Detail draft follow-up'))
  const noteLabel = make('label', 'Pesan follow-up untuk ditinjau')
  const note = make('textarea')
  note.name = 'summary'
  note.maxLength = 2000
  noteLabel.append(note)
  const timeLabel = make('label', `Jadwal (${Intl.DateTimeFormat().resolvedOptions().timeZone})`)
  const due = make('input')
  due.type = 'datetime-local'
  due.name = 'dueAt'
  timeLabel.append(due)
  details.append(noteLabel, timeLabel)
  const submit = make('button', 'Tanya Archava')
  submit.type = 'submit'
  form.append(label, details, submit)
  host.replaceChildren(title, help, selectedLabel, clear, log, feedback, form)

  const render = (data: CrmConversationData | null, entry: HTMLElement) => {
    if (!data) return
    if (data.kind === 'contacts' || data.kind === 'choose') {
      const list = make('ul')
      for (const item of data.items) {
        const row = make('li')
        const button = make('button', item.fullName)
        button.type = 'button'
        button.dataset.archavaContact = item.id
        button.addEventListener('click', () => {
          if (destroyed || busy) return
          selected = { id: item.id, fullName: item.fullName }
          selectedLabel.textContent = 'Kontak dipilih: ' + item.fullName
          message.value = 'ringkas'
          message.focus()
        })
        row.append(button)
        list.append(row)
      }
      entry.append(list)
      if (data.next && data.kind === 'contacts') {
        const next = make('button', 'Halaman berikutnya')
        next.type = 'button'
        const utterance = message.value
        next.addEventListener('click', () => {
          void ask(utterance, data.next ?? undefined)
        })
        entry.append(next)
      } else if (data.next)
        entry.append(make('p', 'Masih ada hasil lain. Perjelas nama lewat “cari …”.'))
    }
    if (data.kind === 'summary') {
      const list = make('ul')
      for (const item of data.contact.activities)
        list.append(make('li', `${item.kind} · ${item.createdAt}: ${item.summary}`))
      for (const item of data.contact.followups)
        list.append(make('li', `${item.status} · ${item.dueAt}: ${item.summary}`))
      entry.append(
        list.childElementCount
          ? list
          : make('p', 'Tidak ada catatan atau follow-up yang dapat ditampilkan.'),
      )
    }
    if (data.kind === 'draft') {
      entry.append(
        make('p', data.draft.summary),
        make('p', 'Jadwal: ' + data.draft.dueAt),
        make('p', 'Status: draft · perlu persetujuan'),
      )
    }
  }
  async function ask(utterance: string, after?: string) {
    if (busy || destroyed) return
    feedback.textContent = ''
    const summary = note.value.trim()
    const date = due.value ? new Date(due.value) : null
    if ((summary || due.value) && (!summary || !date || !Number.isFinite(date.getTime()))) {
      feedback.textContent = 'Isi pesan dan jadwal draft secara lengkap.'
      return
    }
    busy = true
    submit.disabled = true
    message.disabled = true
    active = new AbortController()
    try {
      const response = await transport(options.apiBase, {
        method: 'POST',
        credentials: 'same-origin',
        signal: active.signal,
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          utterance,
          ...(selected ? { contactId: selected.id } : {}),
          ...(after ? { after } : {}),
          ...(summary && date ? { draft: { summary, dueAt: date.toISOString() } } : {}),
        }),
      })
      if (destroyed) return
      if (!response.ok)
        throw new Error(
          response.status === 403
            ? 'Anda tidak punya izin untuk permintaan ini.'
            : response.status === 401
              ? 'Sesi berakhir. Masuk lagi.'
              : 'Permintaan belum berhasil. Coba lagi.',
        )
      const result = conversationReplySchema.parse(await response.json())
      if (destroyed) return
      if (result.data?.kind === 'summary')
        selected = { id: result.data.contact.id, fullName: result.data.contact.fullName }
      else if (result.data?.kind === 'draft')
        selected = { id: result.data.draft.contactId, fullName: result.data.fullName }
      else if (result.data && ['choose', 'not_found', 'needs_contact'].includes(result.data.kind))
        selected = null
      selectedLabel.textContent = selected
        ? 'Kontak dipilih: ' + selected.fullName
        : 'Belum ada kontak dipilih.'
      const entry = make('article')
      entry.append(make('p', 'Anda: ' + utterance), make('p', result.text))
      if (result.source === 'crm') entry.append(make('small', 'Sumber: CRM'))
      render(result.data, entry)
      log.append(entry)
      while (log.childElementCount > 20) log.firstElementChild?.remove()
    } catch (error) {
      if (!destroyed)
        feedback.textContent = error instanceof Error ? error.message : 'Permintaan belum berhasil.'
    } finally {
      busy = false
      active = null
      if (!destroyed) {
        submit.disabled = false
        message.disabled = false
      }
    }
  }
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    void ask(message.value)
  })
  return {
    destroy() {
      destroyed = true
      active?.abort()
      selected = null
      host.replaceChildren()
    },
  }
}
