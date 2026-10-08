import type { CrmConversationPlanner, CrmConversationPlan } from './contracts.js'

/** Small offline vocabulary, explicitly separate from a live language model. */
export class LocalCrmPlanner implements CrmConversationPlanner {
  readonly providerId = 'archava-crm-local'
  readonly model = 'deterministic-commands-v1'
  plan(utterance: string): Promise<CrmConversationPlan> {
    const text = utterance
      .trim()
      .replace(/[?!.]+$/, '')
      .trim()
    if (
      /^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+(?:kontak|contacts)$/i.test(
        text,
      )
    )
      return Promise.resolve({ operation: 'contacts.list', search: '' })
    const search = /^(?:cari|search|find)(?:\s+(?:kontak|contact))?\s+(.+)$/i.exec(text)?.[1]
    if (search && search.length <= 120)
      return Promise.resolve({ operation: 'contacts.list', search })
    const summary =
      /^(?:ringkas|ringkasan|summarize|summary)(?:\s+(?:kontak|contact))?(?:\s+(.+))?$/i.exec(text)
    const followup =
      /^(?:siapkan|prepare|draft)\s+follow[ -]?up(?:\s+(?:untuk|for))?(?:\s+(.+))?$/i.exec(text)
    const match = summary ?? followup
    if (match && (match[1]?.length ?? 0) <= 120)
      return Promise.resolve({
        operation: summary ? 'contact.summary' : 'followup.prepare',
        ...(match[1] ? { query: match[1] } : {}),
      })
    return Promise.resolve({ operation: 'unsupported' })
  }
}
