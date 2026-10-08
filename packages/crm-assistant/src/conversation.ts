import { ActionPolicy } from '@archava/acl'
import type { BrainProvider } from '@archava/adapters'
import { runTurn } from '@archava/assistant'
import { contextGraphSchema } from '@archava/core'
import {
  CrmError,
  createCrmTools,
  principalSchema,
  tenantSchema,
  type CrmService,
  type CrmPrincipal,
} from '@archava/crm'
import {
  conversationInputSchema,
  conversationPlanSchema,
  conversationDataSchema,
  type CrmConversationPlanner,
  type CrmConversationData,
  type CrmConversationReply,
} from './contracts.js'

const HELP =
  'Coba “tampilkan kontak”, “cari Budi”, “ringkas Budi”, atau “siapkan follow-up Budi”. Follow-up hanya berupa draft untuk ditinjau; belum disimpan atau dikirim.'
function describe(data: CrmConversationData): string {
  switch (data.kind) {
    case 'help':
      return HELP
    case 'needs_contact':
      return 'Sebutkan nama kontak atau pilih kontak dari hasil pencarian.'
    case 'needs_draft':
      return 'Isi pesan dan jadwal follow-up, lalu kirim permintaan lagi. Jadwal tidak saya tentukan sendiri.'
    case 'not_found':
      return 'Kontak tidak ditemukan dalam data yang boleh Anda akses.'
    case 'choose':
      return 'Ada beberapa hasil yang mungkin cocok. Pilih kontak yang Anda maksud; saya belum menyiapkan draft.'
    case 'contacts':
      return data.items.length
        ? `${data.items.length} kontak pada halaman ini${data.next ? '; masih ada halaman berikutnya' : ''}.`
        : 'Belum ada kontak yang cocok dalam data yang boleh Anda akses.'
    case 'summary':
      return `Ringkasan ${data.contact.fullName}. Catatan dan follow-up di bawah berasal dari CRM; masing-masing menampilkan paling banyak 20 data yang diizinkan.`
    case 'draft':
      return `Draft follow-up untuk ${data.fullName} siap ditinjau. Belum disimpan sebagai tugas atau dikirim.`
  }
}

/** Host supplies authenticated identity. The planner never receives a service/repository or tool executor. */
export function createCrmConversation(service: CrmService, planner: CrmConversationPlanner) {
  return {
    async ask(
      context: { tenantId: string; principal: CrmPrincipal; turnId: string; occurredAt: string },
      raw: unknown,
    ): Promise<CrmConversationReply> {
      const tenant = tenantSchema.parse(context.tenantId)
      const principal = principalSchema.parse(context.principal)
      const input = conversationInputSchema.parse(raw)
      // Rechecked on every turn, including help; no session permission cache.
      const access = await service.access(tenant, principal)
      if (!access.permissions.includes('contacts.read'))
        throw new CrmError('forbidden', 'CRM reading is not permitted.')
      const planned = conversationPlanSchema.safeParse(await planner.plan(input.utterance))
      if (!planned.success)
        throw new CrmError('unavailable', 'Conversation plan could not be verified.')
      const plan = planned.data
      const tools = createCrmTools(service, tenant, principal)
      const resolve = async (): Promise<CrmConversationData> => {
        if (plan.operation === 'unsupported') return { kind: 'help' }
        if (plan.operation === 'contacts.list') {
          const page = await tools.invoke('crm.contacts.list', {
            search: plan.search,
            limit: 20,
            ...(input.after ? { after: input.after } : {}),
          })
          return conversationDataSchema.parse({ kind: 'contacts', ...page })
        }
        if (
          plan.operation === 'followup.prepare' &&
          !access.permissions.includes('followups.write')
        )
          throw new CrmError('forbidden', 'Follow-up preparation is not permitted.')
        let contactId = input.contactId
        // An explicit name always wins over a previous selection. Never silently follow up the old contact.
        if (plan.query) {
          const result = conversationDataSchema.parse({
            kind: 'contacts',
            ...(await tools.invoke('crm.contacts.list', { search: plan.query, limit: 20 })),
          })
          if (result.kind !== 'contacts') throw new Error('Invalid CRM page')
          if (!result.items.length) return { kind: 'not_found' }
          const exact = result.items.filter(
            (item) => item.fullName.toLocaleLowerCase() === plan.query?.toLocaleLowerCase(),
          )
          if (result.next || (exact.length !== 1 && result.items.length !== 1))
            return { kind: 'choose', items: result.items, next: result.next }
          contactId = (exact.length === 1 ? exact[0] : result.items[0])?.id
        }
        if (!contactId) return { kind: 'needs_contact' }
        const data = conversationDataSchema.parse({
          kind: 'summary',
          contact: await tools.invoke('crm.contact.summary', { id: contactId }),
        })
        if (data.kind !== 'summary') throw new Error('Invalid CRM summary')
        if (plan.operation === 'contact.summary') return data
        if (!input.draft) return { kind: 'needs_draft' }
        const draft = await tools.invoke('crm.followup.prepare', { contactId, ...input.draft })
        const prepared = conversationDataSchema.parse({
          kind: 'draft',
          fullName: data.contact.fullName,
          draft,
        })
        if (prepared.kind !== 'draft' || prepared.draft.contactRevision !== data.contact.revision)
          throw new CrmError(
            'conflict',
            'Contact changed while preparing the draft. Refresh and retry.',
          )
        return prepared
      }
      let toolFailure: unknown
      let failed = false
      const brain: BrainProvider = {
        providerId: planner.providerId,
        model: planner.model,
        health: { ready: true },
        reply: (turn) => {
          const parsed = conversationDataSchema.safeParse(turn.structuredTruth.crm)
          return Promise.resolve({
            text: parsed.success
              ? describe(parsed.data)
              : 'Data CRM belum bisa diverifikasi. Coba lagi nanti.',
            requestedActions: [],
            citations: [],
            deferToStructuredTruth: !parsed.success,
          })
        },
      }
      const outcome = await runTurn({
        tenantId: tenant,
        sessionId: context.turnId,
        occurredAt: context.occurredAt,
        utterance: input.utterance,
        brain,
        policy: new ActionPolicy(),
        capability: 'assist',
        role: 'archava_assistant',
        authoritativeSubjects: ['customer_or_order'],
        graph: contextGraphSchema.parse({
          tenantId: tenant,
          page: { route: '/crm', kind: 'crm', locale: 'id' },
          session: { authenticated: true },
          entities: [],
          availableActions: [],
          errors: [],
        }),
        knowledge: {
          retrieve: () =>
            Promise.resolve({
              plan: {
                need: 'structured_truth',
                subjects: ['customer_or_order'],
                query: input.utterance,
              },
              context: [],
              deferToStructuredTruth: true,
            }),
        },
        truth: {
          resolve: async () => {
            try {
              return { crm: await resolve() }
            } catch (error) {
              failed = true
              toolFailure = error
              throw error
            }
          },
        },
      })
      // runTurn intentionally masks truth failures. HTTP callers still need the correct authorization/error status.
      if (failed) throw toolFailure
      const data = conversationDataSchema.parse(outcome.structuredTruth.crm)
      return {
        text: outcome.text,
        source: ['contacts', 'summary', 'draft', 'choose', 'not_found'].includes(data.kind)
          ? 'crm'
          : 'none',
        data,
        planner: { providerId: planner.providerId, model: planner.model },
      }
    },
  }
}
