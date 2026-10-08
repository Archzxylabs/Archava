/**
 * The session: the graph a single visitor's page accumulates.
 *
 * PRD §16 says the SDK "should maintain a normalized context graph", and the
 * graph in `@archava/core` is a pure reducer over events. That split matters, so
 * this module owns only the stateful half:
 *
 * - It holds the seed and the event history, and applies every event through the
 *   same `reduceContextGraph` the server uses. A graph assembled here and a graph
 *   replayed from the same history are identical, which is what makes a bug report
 *   answerable.
 * - It never invents a tenant. The tenant comes from `init`, because every read
 *   that follows is scoped to it (§23).
 * - It never collects without consent. Asking is cheap; asking *after* the fact is
 *   not consent.
 */

import type { ClientConfig } from '@archava/config'
import type { ContextEvent, ContextGraph } from '@archava/core'
import { reduceContextGraph, seedContextGraph } from '@archava/core'
import {
  grantConsent,
  parseConsent,
  revokeConsent,
  type ConsentDomain,
  type ConsentState,
} from './consent.js'
import { SdkConfigError, SdkConsentError } from './errors.js'
import { isValueBearing, reconcilePage, type PageShape } from './page.js'

/** What the SDK needs in order to start: an identity, and nothing secret. */
export interface SdkInit {
  /** The tenant this page belongs to. Not secret — it scopes, it does not authorise. */
  readonly tenantId: string
  /** The resolved client config, when the host has one. */
  readonly config?: ClientConfig
  /** The route to start on. Defaults to `/`. */
  readonly route?: string
  /** The visitor's recorded consent, by domain. */
  readonly consent?: Partial<Record<ConsentDomain, boolean>>
  /** Extra field names the tenant considers sensitive, beyond the defaults. */
  readonly sensitiveFields?: readonly string[]
}

/** What a host gets back from `init`: a handle that answers for this page. */
export interface SdkSession {
  readonly tenantId: string
  /** The graph as it stands. */
  graph(): ContextGraph
  /** The events that produced the graph, in the order they were applied. */
  history(): readonly ContextEvent[]
  /** Apply events. Returns the graph after the fold. */
  observe(events: readonly ContextEvent[]): ContextGraph
  /** Apply a page read. Returns only the events that described a change. */
  readPage(page: PageShape): readonly ContextEvent[]
  /** The consent currently in force. */
  consent(): ConsentState
  /** Grant one domain. */
  grant(domain: ConsentDomain): ConsentState
  /** Withdraw one domain. Takes effect on the next turn, not retroactively. */
  revoke(domain: ConsentDomain): ConsentState
  /** Names this session withholds from a provider. */
  maskedFields(): readonly string[]
  /** One notice per field withheld, for the §16 masking log. */
  notices(): readonly string[]
  /** Stop observing. Idempotent. */
  destroy(): void
  /** True once `destroy` has been called. */
  destroyed(): boolean
}

/**
 * Create a session.
 *
 * `init` refuses two things. A blank tenant is one: an unscoped session has no
 * route to a corpus and no answer to "whose page is this?" (§23). A refused
 * tenant is the other, and it throws rather than degrading, because the two
 * available degradations — collect nothing, or collect without permission — are
 * respectively useless and unsafe.
 */
export function createSession(init: SdkInit): SdkSession {
  const tenantId = typeof init.tenantId === 'string' ? init.tenantId.trim() : ''
  if (tenantId.length === 0) {
    throw new SdkConfigError(
      'Archava.init requires a tenantId; a page with no tenant has no scope.',
    )
  }

  if (init.config !== undefined && init.config.tenantId !== tenantId) {
    throw new SdkConfigError('Session tenantId must match the supplied client configuration.')
  }

  let consent = parseConsent(init.consent)
  const sensitive = [...(init.sensitiveFields ?? [])]
  const notices: string[] = []

  let graph: ContextGraph =
    init.config === undefined
      ? seedless(tenantId, init.route ?? '/')
      : seedContextGraph(init.config, init.route)

  let lastPage: PageShape | null = null
  let destroyed = false

  /** The log a replay needs: every event applied, in the order it was applied. */
  const log: ContextEvent[] = []

  /**
   * The only door through which context enters.
   *
   * Consent is checked here rather than at each call site, because page context
   * is what the events *are* — a door per call site is a door that gets missed.
   */
  function observe(events: readonly ContextEvent[]): ContextGraph {
    if (consent.page_context !== true) {
      throw new SdkConsentError(
        'Page context was refused, so no context event may be observed. Grant `page_context` first.',
      )
    }
    for (const event of events) {
      log.push(event)
      graph = reduceContextGraph(graph, event)
    }
    return project(graph, sensitive)
  }

  return {
    tenantId,

    graph(): ContextGraph {
      return project(graph, sensitive)
    },

    history(): readonly ContextEvent[] {
      // A copy, so a caller cannot edit a log a replay depends on.
      return [...log]
    },

    observe,

    readPage(page: PageShape): readonly ContextEvent[] {
      const changed = reconcilePage(lastPage, page)
      observe(changed)
      // Only now is the page remembered. A refused read must leave no trace of
      // the page it describes, or the first read after a grant would diff
      // against a page it never lawfully learned.
      lastPage = page
      return changed
    },

    consent(): ConsentState {
      return { ...consent }
    },

    grant(domain: ConsentDomain): ConsentState {
      consent = grantConsent(consent, domain)
      return { ...consent }
    },

    revoke(domain: ConsentDomain): ConsentState {
      consent = revokeConsent(consent, domain)
      // Withdrawal is prospective only: the graph keeps what it lawfully held
      // while the domain was granted, and the next turn is the first that must
      // not use it. Erasing history retroactively would make the event log lie.
      return { ...consent }
    },

    maskedFields(): readonly string[] {
      return withheld(graph, sensitive)
    },

    notices(): readonly string[] {
      // A notice is emitted once per field the first time it is withheld, so a
      // session that masks the same field on ten turns logs it once.
      for (const name of withheld(graph, sensitive)) {
        const notice = `Field "${name}" was withheld from the provider (§16).`
        if (notices.includes(notice) === false) notices.push(notice)
      }
      return [...notices]
    },

    destroy(): void {
      destroyed = true
    },

    destroyed(): boolean {
      return destroyed
    },
  }
}

/** A seed for a host with no config: identity and locale only. */
function seedless(tenantId: string, route: string): ContextGraph {
  return {
    tenantId,
    page: { route, kind: 'home', locale: 'en' },
    entities: [],
    comparison: { entityIds: [] },
    session: { authenticated: false },
    availableActions: [],
    errors: [],
  }
}

/**
 * The names this session will not send to a provider.
 *
 * The union of three sources, because the failure mode of missing one is
 * asymmetric: a field that reaches the model is a disclosure the visitor cannot
 * take back, while a field masked twice costs nothing. The sources are the
 * graph's own field names, the tenant's declared list, and the SDK's defaults.
 */
function withheld(graph: ContextGraph, sensitive: readonly string[]): readonly string[] {
  const form = graph.form
  const named = form === undefined ? [] : [...form.completedFields, ...form.pendingFields]
  return [...new Set([...named, ...sensitive])].filter(
    (name) => isValueBearing(name) || sensitive.includes(name),
  )
}

/**
 * The graph as the host should receive it.
 *
 * The reducer's shape has nowhere to put a value, so this is a projection of the
 * tenant's declared sensitive names onto the graph's field-name lists — the same
 * belt-and-braces the server applies at its own boundary (§16).
 */
function project(graph: ContextGraph, sensitive: readonly string[]): ContextGraph {
  if (sensitive.length === 0) return graph
  const form = graph.form
  if (form === undefined) return graph
  const masked = [...new Set([...form.maskedFields, ...withheld(graph, sensitive)])]
  if (masked.every((name) => form.maskedFields.includes(name))) return graph
  return {
    ...graph,
    form: { ...form, maskedFields: masked },
  }
}
