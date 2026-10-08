import { describe, expect, it } from 'vitest'
import { parseClientConfig, type ClientConfig } from '@archava/config'
import { foldContextEvents, seedContextGraph } from '@archava/core'
import { SdkConfigError, SdkConsentError, createSession, type PageShape } from '../src/index.js'

it('refuses a config from another tenant before starting a session', () => {
  expect(() => createSession({ tenantId: 'retail-demo', config: configFor('hotel-demo') })).toThrow(
    SdkConfigError,
  )
})

/**
 * What the session is *for*, in the two promises a bug report depends on:
 *
 * 1. **Replay identity.** The events it logs, folded through the same reducer
 *    the server uses, reproduce the graph it hands out. Without that, "the
 *    assistant saw a stale section" is unfalsifiable.
 * 2. **A gate that fails closed.** Consent is checked in one place, so there is
 *    no call site a page can sneak context through.
 *
 * The config fixture goes through the real parser, as in
 * `packages/core/test/context-graph.test.ts`: a cast-in-shape config would let
 * these tests keep passing after the schema moved.
 */
function configFor(tenantId = 'client-xyz'): ClientConfig {
  return parseClientConfig({
    schema_version: '1.0.0',
    tenantId,
    environment: 'commerce_booking',
    presence: 'chat',
    capability: 'transact',
    region: 'ID',
    template: 'hospitality',
    branding: {
      businessName: 'Acme Hotels',
      theme: {
        accent: '#123456',
        surface: '#ffffff',
        ink: '#101010',
        radius: 'rounded',
        fontFamily: 'Inter',
      },
    },
    languages: [{ code: 'id', label: 'Bahasa Indonesia' }],
    primaryLanguage: 'id',
    updatedAt: '2026-04-01',
  })
}

function page(overrides: Partial<PageShape> = {}): PageShape {
  return {
    path: '/rooms',
    kind: 'catalog',
    locale: 'id',
    entities: [{ id: 'room-1', name: 'Deluxe Suite', kind: 'room' }],
    actions: [{ name: 'add_to_cart', enabled: true }],
    errors: [],
    ...overrides,
  }
}

const ROUTE = '/rooms'

describe('createSession and the seed it starts from', () => {
  it('refuses a blank tenant, because an unscoped read has nowhere to be scoped to', () => {
    for (const tenantId of ['', '   ']) {
      expect(() => createSession({ tenantId })).toThrow(SdkConfigError)
    }
  })

  it('seeds the tenant, route, and language the host declared', () => {
    const session = createSession({ tenantId: 'client-xyz', config: configFor(), route: ROUTE })

    expect(session.tenantId).toBe('client-xyz')
    expect(session.graph()).toMatchObject({
      tenantId: 'client-xyz',
      page: { route: ROUTE, kind: 'home', locale: 'id' },
    })
  })

  it('degenerates to an empty page when the host declared no config', () => {
    const session = createSession({ tenantId: 'client-xyz' })

    // The embedding page may be rendered before the config arrives; a loader is
    // not a reason to invent a tenant or a language.
    expect(session.graph()).toMatchObject({
      tenantId: 'client-xyz',
      page: { route: '/', kind: 'home', locale: 'en' },
      comparison: { entityIds: [] },
      entities: [],
      availableActions: [],
      errors: [],
    })
  })
})

describe('the consent gate', () => {
  it('refuses both doors when page context was never granted', () => {
    const refused = createSession({ tenantId: 'client-xyz', consent: { page_context: false } })

    expect(() =>
      refused.observe([{ type: 'page/route', route: '/checkout', kind: 'checkout' }]),
    ).toThrow(SdkConsentError)
    expect(() => refused.readPage(page())).toThrow(SdkConsentError)

    // A refusal leaves no trace: the history is what a replay would apply, and
    // nothing was lawfully learned, so nothing may be in it.
    expect(refused.history()).toEqual([])
    expect(refused.graph()).toMatchObject({ page: { route: '/', kind: 'home' }, entities: [] })
  })

  it('opens the door the moment the domain is granted', () => {
    const session = createSession({ tenantId: 'client-xyz' })

    expect(() => session.readPage(page())).toThrow(SdkConsentError)
    session.grant('page_context')

    expect(session.readPage(page())).not.toEqual([])
    expect(session.graph().entities).toEqual([{ id: 'room-1', name: 'Deluxe Suite', kind: 'room' }])
  })

  it('closes on revoke, and keeps what it lawfully held while granted', () => {
    const session = createSession({ tenantId: 'client-xyz', consent: { page_context: true } })
    session.readPage(page())

    const held = session.graph()
    const logged = session.history()

    session.revoke('page_context')

    // The next turn is the first that must not observe. Erasing the graph and
    // the log here would rewrite history after the fact, which is the one thing
    // a history must never do.
    expect(session.consent().page_context).toBe(false)
    expect(() => session.readPage(page({ path: '/checkout', kind: 'checkout' }))).toThrow(
      SdkConsentError,
    )
    expect(session.graph()).toEqual(held)
    expect(session.history()).toEqual(logged)
  })
})

describe('replay identity', () => {
  it('reproduces the graph it handed out from the log it kept', () => {
    const session = createSession({
      tenantId: 'client-xyz',
      config: configFor(),
      route: ROUTE,
      consent: { page_context: true },
      sensitiveFields: ['nationality'],
    })

    session.readPage(
      page({
        section: 'Deluxe Suites',
        comparison: { entityIds: ['room-1', 'room-2'], metric: 'price' },
        form: {
          id: 'booking',
          completedFields: ['email', 'password', 'nationality'],
          pendingFields: ['notes'],
          maskedFields: ['password'],
        },
      }),
    )
    session.readPage(
      page({
        entities: [{ id: 'room-2', name: 'Standard', kind: 'room' }],
        actions: [{ name: 'pay', enabled: false }],
        activePanel: 'filters',
      }),
    )

    const seed = seedContextGraph(configFor(), ROUTE)
    const replayed = foldContextEvents(seed, session.history())

    // The assertion that makes a support answer possible: the graph the host
    // was handed at the time, and the graph built later from the log, agree.
    expect(replayed).toEqual(session.graph())
  })

  it('hands out a copy, so a log a replay depends on cannot be edited in place', () => {
    const session = createSession({ tenantId: 'client-xyz', consent: { page_context: true } })
    session.readPage(page())

    const handed = session.history()
    session.readPage(page({ path: '/checkout', kind: 'checkout' }))

    // The first read's events stay with the first read; if the same array were
    // handed over twice, it would grow as the session did.
    expect(handed.some((event) => event.type === 'page/route' && event.route === '/checkout')).toBe(
      false,
    )
    expect(session.history().length).toBeGreaterThan(handed.length)
  })
})

describe('masking at the host boundary', () => {
  it('withholds a tenant-declared field the SDK would not have caught by name', () => {
    const session = createSession({
      tenantId: 'client-xyz',
      consent: { page_context: true },
      sensitiveFields: ['nationality'],
    })
    session.readPage(
      page({
        form: {
          id: 'booking',
          completedFields: ['email', 'password', 'nationality'],
          pendingFields: ['notes'],
          maskedFields: ['password'],
        },
      }),
    )

    // The graph the assistant receives carries the union, not the intersection:
    // a host that forgot this field still gets it withheld.
    expect(session.maskedFields()).toEqual(['password', 'nationality'])
    expect(session.graph().form?.maskedFields).toEqual(['password', 'nationality'])
    expect(session.graph().form?.completedFields).toContain('nationality')
  })

  it('notices each withheld field once, however often it is read', () => {
    const session = createSession({ tenantId: 'client-xyz', consent: { page_context: true } })
    session.readPage(
      page({
        form: {
          id: 'booking',
          completedFields: ['password'],
          pendingFields: [],
          maskedFields: ['password'],
        },
      }),
    )

    expect(session.notices()).toEqual(['Field "password" was withheld from the provider (§16).'])
    expect(session.notices()).toEqual(session.notices())
  })
})

describe('destroy', () => {
  it('is idempotent, and reports its own state', () => {
    const session = createSession({ tenantId: 'client-xyz' })

    expect(session.destroyed()).toBe(false)
    session.destroy()
    session.destroy()

    expect(session.destroyed()).toBe(true)
  })
})
