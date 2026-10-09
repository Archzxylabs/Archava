import { conversationReplySchema, type CrmConversationData } from './contracts.js'
import { GEMINI_PILOT_CASES } from './pilot.js'
import { commandStatusSchema, mutationReceiptSchema, idSchema } from '@archava/crm/contracts'
import type { FollowupRecoveryStore } from './recovery.js'
import { nativeReviewSchema, nativeExecutionSchema } from '@archava/crm-act/contracts'

/** Shared UI. It renders records as text and holds no credentials or durable conversation state. */
export function mountCrmConversation(
  host: HTMLElement,
  options: {
    apiBase: string
    commandsBase?: string
    approvalBase?: string
    pilotEnabled?: boolean
    recovery?: FollowupRecoveryStore
    transport?: typeof fetch
    onFollowupCompleted?: () => void
  },
) {
  let destroyed = false
  let busy = false
  let selected: { id: string; fullName: string } | null = null
  let opportunity: { id: string; title: string } | null = null
  let followupId: string | null = null
  let active: AbortController | null = null
  const transport = options.transport ?? fetch
  let pendingRequestId: string | null = null
  let recoveryUnavailable = false
  let recoveryActive: AbortController | null = null
  try {
    const pending = options.recovery?.load() ?? null
    pendingRequestId = pending === null ? null : idSchema.parse(pending)
  } catch {
    recoveryUnavailable = true
  }
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
    followupId = null
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
  const recoveryBox = make('div')
  recoveryBox.dataset.archavaRecovery = ''
  const recoveryState = make('p')
  recoveryState.setAttribute('role', 'status')
  const checkSave = make('button', 'Cek hasil penyimpanan')
  checkSave.type = 'button'
  checkSave.dataset.archavaRecover = ''
  recoveryBox.append(recoveryState, checkSave)
  const cancelSave = make('button', 'Batalkan penyimpanan yang belum dijalankan')
  cancelSave.type = 'button'
  cancelSave.dataset.archavaCancel = ''
  if (options.approvalBase) recoveryBox.append(cancelSave)
  if (options.recovery) log.before(recoveryBox)

  function syncRecovery() {
    if (recoveryUnavailable && !recoveryState.textContent?.includes('ditahan'))
      recoveryState.textContent =
        (recoveryState.textContent ? recoveryState.textContent + ' ' : '') +
        'Pemulihan tab belum bisa diperbarui; penyimpanan baru ditahan.'
    recoveryBox.hidden = !pendingRequestId && !recoveryUnavailable && !recoveryState.textContent
    checkSave.hidden = !pendingRequestId
    checkSave.disabled = busy || recoveryActive !== null || recoveryUnavailable
    cancelSave.hidden = !options.approvalBase || !pendingRequestId
    cancelSave.disabled = busy || recoveryActive !== null || recoveryUnavailable
    for (const button of host.querySelectorAll<HTMLButtonElement>('[data-archava-save]'))
      button.disabled =
        busy ||
        recoveryActive !== null ||
        recoveryUnavailable ||
        button.dataset.archavaFinal === 'true' ||
        button.dataset.archavaUncertain === 'true' ||
        (pendingRequestId !== null && pendingRequestId !== button.dataset.archavaRequestId)
  }
  function forgetKnown(requestId: string) {
    try {
      options.recovery?.forget(requestId)
      if (pendingRequestId === requestId) pendingRequestId = null
    } catch {
      recoveryUnavailable = true
    }
  }
  async function recoverSave(allowBusy = false) {
    const requestId = pendingRequestId
    if (
      !requestId ||
      !options.commandsBase ||
      recoveryActive ||
      (busy && !allowBusy) ||
      destroyed ||
      recoveryUnavailable
    )
      return
    recoveryActive = new AbortController()
    recoveryState.textContent = 'Memeriksa hasil penyimpanan sebelumnya…'
    syncRecovery()
    try {
      const response = await transport(options.commandsBase + '/' + requestId, {
        credentials: 'same-origin',
        signal: recoveryActive.signal,
      })
      if (destroyed) return
      if (!response.ok) throw new Error('Receipt could not be verified')
      const result = commandStatusSchema.parse(await response.json())
      if (destroyed) return
      if (result.requestId !== requestId) throw new Error('Receipt does not match')
      if (result.status === 'committed') {
        if (result.receipt.kind !== 'followup') throw new Error('Receipt does not match')
        const known = host.querySelector<HTMLButtonElement>(
          `[data-archava-request-id="${requestId}"]`,
        )
        if (
          known?.dataset.archavaFollowupId &&
          (result.receipt.id !== known.dataset.archavaFollowupId ||
            String(result.receipt.revision) !== known.dataset.archavaResultRevision)
        )
          throw new Error('Completion receipt does not match the reviewed task')
        forgetKnown(requestId)
        recoveryState.textContent =
          'Penyimpanan sebelumnya terkonfirmasi: follow-up tersimpan di CRM.'
        const button = host.querySelector<HTMLButtonElement>(
          `[data-archava-request-id="${requestId}"]`,
        )
        if (button) {
          button.dataset.archavaFinal = 'true'
          button.textContent = button.dataset.archavaAction === 'complete' ? 'Selesai' : 'Tersimpan'
          const state = button.parentElement?.querySelector<HTMLElement>(
            '[data-archava-save-state]',
          )
          if (state)
            state.textContent =
              button.dataset.archavaAction === 'complete'
                ? 'Tugas ditandai selesai di CRM.'
                : 'Follow-up tersimpan di CRM.'
          if (button.dataset.archavaAction === 'complete') {
            followupId = null
            options.onFollowupCompleted?.()
          }
        }
      } else {
        recoveryState.textContent =
          'Hasil penyimpanan belum terkonfirmasi. Cek lagi sebelum membuat penyimpanan baru.'
      }
    } catch {
      if (!destroyed)
        recoveryState.textContent =
          'Hasil belum bisa diverifikasi. Cek lagi saat koneksi dan akses tersedia.'
    } finally {
      recoveryActive = null
      if (!destroyed) syncRecovery()
    }
  }
  checkSave.addEventListener('click', () => {
    void recoverSave()
  })
  cancelSave.addEventListener('click', () => {
    const id = pendingRequestId
    if (!id || !options.approvalBase || busy || recoveryActive || recoveryUnavailable || destroyed)
      return
    void (async () => {
      busy = true
      syncRecovery()
      try {
        const response = await transport(options.approvalBase + '/' + id + '/cancel', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'content-type': 'application/json' },
          body: '{}',
        })
        if (destroyed) return
        const result: unknown = await response.json()
        if (
          !response.ok ||
          typeof result !== 'object' ||
          result === null ||
          !('cancelled' in result) ||
          result.cancelled !== true
        )
          throw new Error('Cannot cancel a started action')
        forgetKnown(id)
        const button = host.querySelector<HTMLButtonElement>(`[data-archava-request-id="${id}"]`)
        if (button) {
          button.dataset.archavaFinal = 'true'
          button.textContent = 'Dibatalkan'
        }
        recoveryState.textContent =
          'Penyimpanan dibatalkan sebelum dijalankan. Buat dan tinjau draft baru.'
      } catch {
        if (!destroyed)
          recoveryState.textContent =
            'Penyimpanan belum dapat dibatalkan. Cek hasilnya; tindakan yang sudah dimulai tidak boleh diulang.'
      } finally {
        busy = false
        if (!destroyed) syncRecovery()
      }
    })()
  })
  if (recoveryUnavailable)
    recoveryState.textContent =
      'Pemulihan penyimpanan tidak tersedia di tab ini. Penyimpanan baru ditahan.'
  else if (pendingRequestId)
    recoveryState.textContent = 'Ada penyimpanan sebelumnya yang belum terkonfirmasi.'
  syncRecovery()
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
    if (data.kind === 'completion_draft') {
      entry.append(
        make('p', data.task.followup.summary),
        make('p', 'Jadwal: ' + data.task.followup.dueAt),
      )
      if (options.approvalBase) renderNativeApproval(entry, data)
      else entry.append(make('p', 'Approval penyelesaian belum tersedia di host ini.'))
    }
    if (data.kind === 'followups') {
      const list = make('ul')
      list.dataset.archavaFollowups = ''
      for (const item of data.items) {
        const row = make('li', `${new Date(item.dueAt).toLocaleString()} · ${item.summary} · `)
        const button = make('button', item.fullName)
        button.type = 'button'
        button.dataset.archavaContact = item.contactId
        button.addEventListener('click', () => {
          if (destroyed || busy) return
          followupId = null
          selected = { id: item.contactId, fullName: item.fullName }
          opportunity = null
          opportunityLabel.textContent = 'Belum ada peluang dipilih.'
          selectedLabel.textContent = 'Kontak dipilih: ' + item.fullName
          message.value = 'ringkas'
          message.focus()
        })
        row.append(button)
        const choose = make('button', 'Pilih tugas untuk Archava')
        choose.type = 'button'
        choose.dataset.archavaFollowup = item.id
        choose.addEventListener('click', () => selectFollowup(item.id))
        row.append(choose)
        list.append(row)
      }
      entry.append(list)
      if (data.next) {
        const cursor = data.next
        const next = make('button', 'Halaman follow-up berikutnya')
        next.type = 'button'
        const utterance = message.value
        next.addEventListener('click', () => {
          void ask(utterance, undefined, { after: cursor, asOf: data.asOf })
        })
        entry.append(next)
      }
    }
    if (data.kind === 'contacts' || data.kind === 'choose') {
      const list = make('ul')
      for (const item of data.items) {
        const row = make('li')
        const button = make('button', item.fullName)
        button.type = 'button'
        button.dataset.archavaContact = item.id
        button.addEventListener('click', () => {
          if (destroyed || busy) return
          followupId = null
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
          followupId = null
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
      if (options.approvalBase) {
        renderNativeApproval(entry, data)
        return
      }
      const commandsBase = options.commandsBase
      if (commandsBase) {
        const save = make('button', 'Simpan follow-up yang ditinjau')
        save.type = 'button'
        save.dataset.archavaSave = ''
        const requestId = crypto.randomUUID()
        save.dataset.archavaRequestId = requestId
        let saved = false
        let revisionChanged = false
        const state = make('p', 'Simpan sebagai tugas follow-up di CRM.')
        state.dataset.archavaSaveState = ''
        save.addEventListener('click', () => {
          if (
            destroyed ||
            busy ||
            saved ||
            revisionChanged ||
            recoveryUnavailable ||
            recoveryActive ||
            (pendingRequestId !== null && pendingRequestId !== requestId)
          )
            return
          void (async () => {
            const wasPending = pendingRequestId === requestId
            if (options.recovery) {
              try {
                options.recovery.remember(requestId)
                pendingRequestId = requestId
                recoveryState.textContent = 'Ada penyimpanan yang belum terkonfirmasi.'
              } catch {
                recoveryUnavailable = true
                recoveryState.textContent =
                  'Pemulihan penyimpanan tidak tersedia di tab ini. Penyimpanan baru ditahan.'
                syncRecovery()
                return
              }
            }
            busy = true
            save.disabled = true
            submit.disabled = true
            message.disabled = true
            active = new AbortController()
            syncRecovery()
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
                // A first, explicit validation/revision refusal did not commit.
                // An earlier uncertain response must instead stay unresolved.
                if (!wasPending && [400, 409].includes(response.status)) forgetKnown(requestId)
                throw new Error(
                  response.status === 409
                    ? 'Kontak berubah. Buat ulang draft setelah meninjau data terbaru.'
                    : 'Belum berhasil menyimpan. Percobaan ulang memakai permintaan yang sama.',
                )
              }
              const receipt = mutationReceiptSchema.parse(await response.json())
              if (receipt.kind !== 'followup') throw new Error('Unexpected receipt')
              if (destroyed) return
              saved = true
              save.dataset.archavaFinal = 'true'
              forgetKnown(requestId)
              recoveryState.textContent = ''
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
                if (revisionChanged) save.dataset.archavaFinal = 'true'
                syncRecovery()
                submit.disabled = false
                message.disabled = false
              }
            }
          })()
        })
        entry.append(save, state)
        syncRecovery()
      }
    }
  }

  function renderNativeApproval(
    entry: HTMLElement,
    data: Extract<CrmConversationData, { kind: 'draft' | 'completion_draft' }>,
  ) {
    const completion = data.kind === 'completion_draft'
    const base = options.approvalBase
      ? options.approvalBase + (completion ? '/complete' : '')
      : undefined
    if (!base) return
    const inputs =
      data.kind === 'completion_draft'
        ? {
            followupId: data.task.followup.id,
            expectedRevision: data.task.followup.revision,
          }
        : {
            contactId: data.draft.contactId,
            contactRevision: data.draft.contactRevision,
            summary: data.draft.summary,
            dueAt: data.draft.dueAt,
          }
    const button = make('button', completion ? 'Tinjau penyelesaian' : 'Tinjau penyimpanan'),
      state = make(
        'p',
        completion
          ? 'Tugas masih terbuka; tinjau sebelum menandainya selesai.'
          : 'Review dulu; follow-up belum disimpan.',
      )
    button.type = 'button'
    button.dataset.archavaSave = ''
    button.dataset.archavaAction = completion ? 'complete' : 'create'
    if (data.kind === 'completion_draft') {
      button.dataset.archavaFollowupId = data.task.followup.id
      button.dataset.archavaResultRevision = String(data.task.followup.revision + 1)
    }
    state.dataset.archavaSaveState = ''
    let review: ReturnType<typeof nativeReviewSchema.parse> | null = null
    entry.append(button, state)
    button.addEventListener('click', () => {
      if (
        destroyed ||
        busy ||
        recoveryUnavailable ||
        recoveryActive ||
        button.dataset.archavaFinal === 'true' ||
        button.dataset.archavaUncertain === 'true' ||
        pendingRequestId !== null
      )
        return
      void (async () => {
        busy = true
        submit.disabled = true
        message.disabled = true
        active = new AbortController()
        syncRecovery()
        let presenting = false
        try {
          if (!review) {
            state.textContent = 'Memeriksa draft dan izin…'
            const response = await transport(base + '/review', {
              method: 'POST',
              credentials: 'same-origin',
              signal: active.signal,
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify(inputs),
            })
            if (destroyed) return
            if (response.status === 409) {
              button.dataset.archavaFinal = 'true'
              button.textContent = 'Buat draft baru'
              state.textContent = completion
                ? 'Tugas berubah atau sudah selesai. Pilih dan tinjau ulang data terbaru.'
                : 'Kontak berubah. Buat ulang draft setelah meninjau data terbaru.'
              return
            }
            if (!response.ok) throw new Error('Review refused')
            review = nativeReviewSchema.parse(await response.json())
            button.dataset.archavaRequestId = review.intentId
            button.textContent = completion ? 'Konfirmasi selesai' : 'Konfirmasi penyimpanan'
            state.textContent = completion
              ? 'Konfirmasi untuk menandai hanya tugas ini selesai di CRM.'
              : 'Draft siap. Konfirmasi untuk membuat satu tugas follow-up di CRM.'
            return
          }
          if (Date.now() >= review.challenge.expiresAt) {
            review = null
            button.textContent = 'Tinjau ulang'
            state.textContent = 'Persetujuan kedaluwarsa. Tinjau draft lagi.'
            return
          }
          if (!options.recovery) throw new Error('Native approval needs recovery storage')
          try {
            options.recovery.remember(review.intentId)
          } catch {
            recoveryUnavailable = true
            recoveryState.textContent =
              'Pemulihan penyimpanan tidak tersedia di tab ini. Penyimpanan baru ditahan.'
            return
          }
          pendingRequestId = review.intentId
          recoveryState.textContent = 'Ada penyimpanan yang belum terkonfirmasi.'
          presenting = true
          button.dataset.archavaUncertain = 'true'
          state.textContent = completion
            ? 'Menandai tugas yang disetujui selesai…'
            : 'Menyimpan follow-up yang disetujui…'
          syncRecovery()
          const response = await transport(base + '/present', {
            method: 'POST',
            credentials: 'same-origin',
            signal: active.signal,
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({
              inputs,
              confirmation: {
                challengeId: review.challenge.challengeId,
                token: review.challenge.token,
              },
            }),
          })
          if (destroyed) return
          if (!response.ok) throw new Error('Approval refused')
          const result = nativeExecutionSchema.parse(await response.json())
          if (result.actionId !== (completion ? 'crm.followup.complete' : 'crm.followup.create'))
            throw new Error('Action does not match')
          if (result.execution !== 'succeeded') throw new Error('Outcome is uncertain')
          await recoverSave(true)
        } catch {
          if (!destroyed) {
            if (presenting)
              state.textContent =
                'Hasil belum terkonfirmasi. Gunakan “Cek hasil penyimpanan”; jangan membuat penyimpanan baru.'
            else {
              review = null
              button.textContent = 'Tinjau ulang'
              state.textContent =
                'Draft belum dapat disetujui. Periksa akses dan data kontak terbaru.'
            }
          }
        } finally {
          busy = false
          active = null
          if (!destroyed) {
            submit.disabled = false
            message.disabled = false
            syncRecovery()
            if (presenting) void recoverSave()
          }
        }
      })()
    })
  }
  async function ask(
    utterance: string,
    after?: string,
    followups?: { after: { dueAt: string; id: string }; asOf: string },
  ) {
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
          ...(followupId ? { followupId } : {}),
          ...(after ? { after } : {}),
          ...(followups ? { followupAfter: followups.after, followupAsOf: followups.asOf } : {}),
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
      if (
        result.data &&
        [
          'followups',
          'contacts',
          'summary',
          'draft',
          'choose',
          'opportunities',
          'opportunity',
          'choose_opportunity',
        ].includes(result.data.kind)
      )
        followupId = null
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
      selectedLabel.textContent = followupId
        ? 'Tugas follow-up dipilih untuk ditinjau.'
        : selected
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
      syncRecovery()
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
        syncRecovery()
      }
    }
  }
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    void ask(message.value)
  })
  function selectFollowup(id: string) {
    if (destroyed || busy) return
    followupId = idSchema.parse(id)
    selected = null
    opportunity = null
    note.value = ''
    due.value = ''
    selectedLabel.textContent = 'Tugas follow-up dipilih untuk ditinjau.'
    opportunityLabel.textContent = 'Belum ada peluang dipilih.'
    message.value = 'tinjau penyelesaian follow-up'
    message.focus()
  }
  void recoverSave()
  return {
    selectFollowup,
    destroy() {
      destroyed = true
      active?.abort()
      recoveryActive?.abort()
      selected = null
      opportunity = null
      followupId = null
      host.replaceChildren()
    },
  }
}
