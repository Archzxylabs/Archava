import { conversationReplySchema, type CrmConversationData } from './contracts.js'
import { z } from 'zod'
import { GEMINI_PILOT_CASES } from './pilot.js'

/** Shared UI. It renders records as text and holds no credentials or durable conversation state. */
export function mountCrmConversation(
  host: HTMLElement,
  options: {
    apiBase: string
    commandsBase?: string
    pilotEnabled?: boolean
    transport?: typeof fetch
  },
) {
  let destroyed = false
  let busy = false
  let selected: { id: string; fullName: string } | null = null
  let opportunity: { id: string; title: string } | null = null
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
    'Coba “tampilkan kontak”, “cari Budi”, “ringkas Budi”, atau “tampilkan peluang”. Tinjau draft follow-up sebelum menyimpannya ke CRM.',
  )
  const selectedLabel = make('p', 'Belum ada kontak dipilih.')
  selectedLabel.dataset.archavaSelected = ''
  const opportunityLabel = make('p', 'Belum ada peluang dipilih.')
  opportunityLabel.dataset.archavaOpportunitySelected = ''
  const clear = make('button', 'Hapus pilihan')
  clear.type = 'button'
  clear.addEventListener('click', () => {
    selected = null
    opportunity = null
    selectedLabel.textContent = 'Belum ada kontak dipilih.'
    opportunityLabel.textContent = 'Belum ada peluang dipilih.'
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
  host.replaceChildren(title, help, selectedLabel, opportunityLabel, clear, log, feedback, form)
  if (options.pilotEnabled) {
    const examples = make('div')
    examples.append(make('p', 'Pilot Gemini: pilih contoh permintaan. Teks bebas diproses lokal.'))
    for (const item of GEMINI_PILOT_CASES.filter((item) => item.example)) {
      const button = make('button', item.utterance)
      button.type = 'button'
      button.dataset.archavaExample = item.id
      button.addEventListener('click', () => {
        if (!destroyed && !busy) {
          message.value = item.utterance
          message.focus()
        }
      })
      examples.append(button)
    }
    form.before(examples)
  }

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
          opportunity = null
          opportunityLabel.textContent = 'Belum ada peluang dipilih.'
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
    if (data.kind === 'opportunities' || data.kind === 'choose_opportunity') {
      const list = make('ul')
      for (const item of data.items) {
        const row = make('li')
        const button = make('button', `${item.title} · ${item.stage}`)
        button.type = 'button'
        button.dataset.archavaOpportunity = item.id
        button.addEventListener('click', () => {
          if (destroyed || busy) return
          opportunity = { id: item.id, title: item.title }
          selected = null
          selectedLabel.textContent = 'Belum ada kontak dipilih.'
          opportunityLabel.textContent = 'Peluang dipilih: ' + item.title
          message.value = 'ringkas peluang'
          message.focus()
        })
        row.append(button)
        list.append(row)
      }
      entry.append(list)
      if (data.next && data.kind === 'opportunities') {
        const next = make('button', 'Halaman peluang berikutnya')
        next.type = 'button'
        const utterance = message.value
        next.addEventListener('click', () => {
          void ask(utterance, data.next ?? undefined)
        })
        entry.append(next)
      } else if (data.next) entry.append(make('p', 'Masih ada hasil lain. Perjelas judul peluang.'))
    }
    if (data.kind === 'opportunity') {
      const history = make('ul')
      for (const item of data.opportunity.transitions)
        history.append(
          make(
            'li',
            `${item.occurredAt} · ${item.from ?? 'Created'} → ${item.to}${item.reason ? ': ' + item.reason : ''}`,
          ),
        )
      entry.append(history)
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
      const commandsBase = options.commandsBase
      if (commandsBase) {
        const save = make('button', 'Simpan follow-up yang ditinjau')
        save.type = 'button'
        save.dataset.archavaSave = ''
        const requestId = crypto.randomUUID()
        let saved = false
        let revisionChanged = false
        const state = make('p', 'Simpan sebagai tugas follow-up di CRM.')
        save.addEventListener('click', () => {
          if (destroyed || busy || saved) return
          void (async () => {
            busy = true
            save.disabled = true
            submit.disabled = true
            message.disabled = true
            active = new AbortController()
            state.textContent = 'Menyimpan…'
            try {
              const response = await transport(commandsBase, {
                method: 'POST',
                credentials: 'same-origin',
                signal: active.signal,
                headers: { 'content-type': 'application/json' },
                body: JSON.stringify({
                  requestId,
                  command: {
                    operation: 'followup.create',
                    contactId: data.draft.contactId,
                    contactRevision: data.draft.contactRevision,
                    summary: data.draft.summary,
                    dueAt: data.draft.dueAt,
                  },
                }),
              })
              if (destroyed) return
              if (!response.ok) {
                revisionChanged = response.status === 409
                throw new Error(
                  response.status === 409
                    ? 'Kontak berubah. Buat ulang draft setelah meninjau data terbaru.'
                    : 'Belum berhasil menyimpan. Percobaan ulang memakai permintaan yang sama.',
                )
              }
              z.object({
                kind: z.literal('followup'),
                id: z.uuid(),
                revision: z.number().int().positive(),
              })
                .strict()
                .parse(await response.json())
              if (destroyed) return
              saved = true
              state.textContent = 'Follow-up tersimpan di CRM.'
              save.textContent = 'Tersimpan'
            } catch {
              if (!destroyed)
                state.textContent = revisionChanged
                  ? 'Kontak berubah. Buat ulang draft setelah meninjau data terbaru.'
                  : 'Belum berhasil menyimpan. Tinjau status atau ulangi permintaan yang sama.'
            } finally {
              busy = false
              active = null
              if (!destroyed) {
                save.disabled = saved || revisionChanged
                submit.disabled = false
                message.disabled = false
              }
            }
          })()
        })
        entry.append(save, state)
      }
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
          ...(opportunity ? { opportunityId: opportunity.id } : {}),
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
      if (
        result.data &&
        [
          'opportunities',
          'choose_opportunity',
          'opportunity',
          'needs_opportunity',
          'opportunity_not_found',
        ].includes(result.data.kind)
      )
        selected = null
      if (result.data?.kind === 'opportunity')
        opportunity = { id: result.data.opportunity.id, title: result.data.opportunity.title }
      else if (
        result.data &&
        [
          'contacts',
          'choose',
          'summary',
          'draft',
          'choose_opportunity',
          'opportunity_not_found',
          'needs_opportunity',
        ].includes(result.data.kind)
      )
        opportunity = null
      selectedLabel.textContent = selected
        ? 'Kontak dipilih: ' + selected.fullName
        : 'Belum ada kontak dipilih.'
      opportunityLabel.textContent = opportunity
        ? 'Peluang dipilih: ' + opportunity.title
        : 'Belum ada peluang dipilih.'
      const entry = make('article')
      entry.append(make('p', 'Anda: ' + utterance), make('p', result.text))
      if (result.source === 'crm') entry.append(make('small', 'Sumber: CRM'))
      entry.append(
        make(
          'small',
          result.planner.providerId === 'google-gemini-crm'
            ? 'Pemahaman: model Gemini · pilot'
            : 'Pemahaman: perintah lokal',
        ),
      )
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
      opportunity = null
      host.replaceChildren()
    },
  }
}
