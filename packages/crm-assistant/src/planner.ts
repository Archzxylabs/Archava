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
    const followups =
      /^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+follow[ -]?ups?(?:\s+(?:yang\s+)?(terlambat|overdue|mendatang|upcoming))?$/i.exec(
        text,
      )
    if (followups)
      return Promise.resolve({
        operation: 'followups.list',
        bucket: /^(terlambat|overdue)$/i.test(followups[1] ?? '')
          ? 'overdue'
          : /^(mendatang|upcoming)$/i.test(followups[1] ?? '')
            ? 'upcoming'
            : 'open',
      })
    if (
      /^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+(?:peluang|opportunities)$/i.test(
        text,
      )
    )
      return Promise.resolve({ operation: 'opportunities.list', search: '' })
    const opportunitySearch = /^(?:cari|search|find)\s+(?:peluang|opportunity)\s+(.+)$/i.exec(
      text,
    )?.[1]
    if (opportunitySearch && opportunitySearch.length <= 120)
      return Promise.resolve({ operation: 'opportunities.list', search: opportunitySearch })
    const opportunitySummary =
      /^(?:ringkas|ringkasan|summarize|summary)\s+(?:peluang|opportunity)(?:\s+(.+))?$/i.exec(text)
    if (opportunitySummary && (opportunitySummary[1]?.length ?? 0) <= 120)
      return Promise.resolve({
        operation: 'opportunity.summary',
        ...(opportunitySummary[1] ? { query: opportunitySummary[1] } : {}),
      })
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
