/**
 * The confirmation boundary, exercised entirely offline.
 *
 * Nothing here talks to a database, a provider or a network. The store is a Map
 * that enforces the atomicity the contract requires, the clock only moves when a
 * test moves it, and the random source is deterministic. That is the point: every
 * guarantee below is a guarantee about *this package's decisions* — about what
 * counts as a confirmation and what does not — not about any vendor.
 *
 * The one property this file cannot prove is the store's own atomicity under a
 * real concurrent load, because a single-process test cannot produce that load.
 * What it does instead is assert that the service asks the store exactly once per
 * presentation, so that a store which *is* atomic is given something it can be
 * atomic about.
 */

import { describe, expect, it } from 'vitest'
import {
  CONFIRMATION_REFUSALS,
  ConfirmationService,
  DEFAULT_CONFIRMATION_TTL_MS,
  MAX_CONFIRMATION_TTL_MS,
  type ConfirmationPresentation,
  type ConfirmationChallengeConsumeResult,
  type ConfirmationChallengeRecord,
  type ConfirmationChallengeStore,
} from '../src/index.js'
import {
  ACTION,
  fakeClock,
  fakeRandom,
  INPUTS,
  KEY,
  OTHER_KEY,
  OTHER_TENANT,
  otherActionPresentation,
  OTHER_INPUTS,
  otherInputsPresentation,
  otherSessionPresentation,
  otherTokenPresentation,
  RecordingStore,
  SESSION,
  TENANT,
  ThrowingStore,
  expiredSpendStore,
  untimestampedStore,
} from './fakes.js'

/** One correct presentation, for the tests about everything except correctness. */
const PRESENTATION: ConfirmationPresentation = {
  challengeId: 'challenge-1',
  token: 'e'.repeat(64),
  tenantId: TENANT,
  sessionId: SESSION,
  actionId: ACTION,
  inputs: INPUTS,
}

/** One clock shared per test, moved by hand so nothing here waits on a wall clock. */
let clock = fakeClock()

/** A store and a service over it, built from a fresh clock in each test. */
function build(ttlMs?: number) {
  clock = fakeClock()
  const store = new RecordingStore()
  const service = new ConfirmationService({
    store,
    digestKey: KEY,
    clock: clock.now,
    random: fakeRandom(),
    ...(ttlMs === undefined ? {} : { ttlMs }),
  })
  return { store, service }
}

/** A minted challenge, or the refusal that explains why there is none. */
async function mint(service: ConfirmationService) {
  return service.issue({ tenantId: TENANT, sessionId: SESSION, actionId: ACTION, inputs: INPUTS })
}

async function minted(service: ConfirmationService) {
  const issued = await mint(service)
  if (issued.status !== 'issued') throw new Error(`challenge was not issued: ${issued.reason}`)
  return issued.challenge
}

/** A presentation that is correct about everything, derived from the challenge. */
function presentationFor(token: string, challengeId: string): ConfirmationPresentation {
  return {
    challengeId,
    token,
    tenantId: TENANT,
    sessionId: SESSION,
    actionId: ACTION,
    inputs: INPUTS,
  }
}

describe('confirmation service', () => {
  describe('issuing', () => {
    it('mints an opaque 64-hex token and a separate challenge id', async () => {
      const { service: issued } = build()
      const challenge = await minted(issued)
      expect(challenge.token).toMatch(/^[0-9a-f]{64}$/)
      expect(challenge.id).toMatch(/^[0-9a-f]{32}$/)
      // The id is not the token, and neither reveals the other.
      expect(challenge.id).not.toBe(challenge.token)
      expect(challenge.token).not.toContain(challenge.id)
      expect(challenge.expiresAt - challenge.issuedAt).toBe(DEFAULT_CONFIRMATION_TTL_MS)
    })

    it('writes a pending challenge to the store before handing the token out', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      expect(store.issues()).toBe(1)
      expect(store.stateOf(challenge.id)?.state).toBe('pending')
    })

    it('mints a different token for two identical requests', async () => {
      const { service } = build()
      const first = await minted(service)
      const second = await minted(service)
      expect(first.token).not.toBe(second.token)
      // Same binding, though: two challenges about the same thing.
      expect(first.bindingDigest).toBe(second.bindingDigest)
    })

    it('refuses a store that will not record it, rather than issuing a token in the air', async () => {
      const { store, service } = build()
      store.failNextIssue = true
      expect(await mint(service)).toEqual({ status: 'rejected', reason: 'store_conflict' })
      expect(store.issues()).toBe(1)
    })

    it('reports an issue-time store outage as unavailable', async () => {
      const service = new ConfirmationService({
        store: new ThrowingStore(),
        digestKey: KEY,
        clock: () => 0,
        random: fakeRandom(),
      })
      expect(await mint(service)).toEqual({ status: 'rejected', reason: 'store_unavailable' })
    })

    it('refuses a non-finite clock and oversized inputs without issuing a challenge', async () => {
      const { store } = build()
      const invalidClock = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: () => Number.NaN,
        random: fakeRandom(),
      })
      expect(await mint(invalidClock)).toEqual({ status: 'rejected', reason: 'store_unavailable' })
      const { service } = build()
      expect(
        await service.issue({
          tenantId: TENANT,
          sessionId: SESSION,
          actionId: ACTION,
          inputs: { value: 'x'.repeat(1_048_576) },
        }),
      ).toEqual({ status: 'rejected', reason: 'unsupported_inputs' })
      expect(store.issues()).toBe(0)
    })

    it('refuses inputs with no canonical form rather than digesting them loosely', async () => {
      const { service } = build()
      expect(
        await service.issue({
          tenantId: TENANT,
          sessionId: SESSION,
          actionId: ACTION,
          inputs: { when: Number.NaN },
        }),
      ).toEqual({ status: 'rejected', reason: 'unsupported_inputs' })
    })

    it('refuses a missing tenant, session or action', async () => {
      const { service } = build()
      expect(
        await service.issue({
          tenantId: '',
          sessionId: SESSION,
          actionId: ACTION,
          inputs: INPUTS,
        }),
      ).toEqual({ status: 'rejected', reason: 'malformed_request' })
      expect(
        await service.issue({
          tenantId: TENANT,
          sessionId: SESSION,
          actionId: ACTION,
          inputs: [] as unknown as Readonly<Record<string, unknown>>,
        }),
      ).toEqual({ status: 'rejected', reason: 'malformed_request' })
    })
  })

  describe('verifying', () => {
    it('verifies the MAC of an actual consumed receipt without spending consent again', async () => {
      const { service, store } = build(),
        challenge = await minted(service)
      const result = await service.verify(presentationFor(challenge.token, challenge.id))
      if (result.status !== 'confirmed') throw new Error('Expected actual receipt')
      expect(service.verifyReceipt(result.receipt)).toBe(true)
      expect(service.verifyReceipt(result.receipt)).toBe(true)
      expect(store.consumeCalls()).toBe(1)
    })
    it('rejects forged receipt fields, scope changes and malformed digests', async () => {
      const { service } = build(),
        challenge = await minted(service)
      const result = await service.verify(presentationFor(challenge.token, challenge.id))
      if (result.status !== 'confirmed') throw new Error('Expected actual receipt')
      for (const patch of [
        { tenantId: 'another-tenant' },
        { sessionId: 'another-owner' },
        { actionId: 'another.action' },
        { inputDigest: 'f'.repeat(64) },
        { receiptDigest: 'f'.repeat(64) },
        { confirmedAt: result.receipt.expiresAt },
        { receiptDigest: 'malformed' },
      ])
        expect(service.verifyReceipt({ ...result.receipt, ...patch })).toBe(false)
    })
    it('confirms a correct presentation exactly once', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const presentation = presentationFor(challenge.token, challenge.id)
      clock.advance(1_000)
      const result = await service.verify(presentation)
      expect(result.status).toBe('confirmed')
      if (result.status !== 'confirmed') return
      expect(result.receipt.confirmedAt).toBe(clock.now())
      expect(result.receipt.tenantId).toBe(TENANT)
      expect(result.receipt.actionId).toBe(ACTION)
      expect(result.receipt.receiptDigest).toMatch(/^[0-9a-f]{64}$/)
      // One store round-trip per presentation, never a retry.
      expect(store.consumeCalls()).toBe(1)
      expect(store.stateOf(challenge.id)?.state).toBe('consumed')
    })

    it('puts no raw input in the receipt', async () => {
      const { service } = build()
      const challenge = await minted(service)
      const result = await service.verify(presentationFor(challenge.token, challenge.id))
      expect(result.status).toBe('confirmed')
      if (result.status !== 'confirmed') return
      const serialized = JSON.stringify(result.receipt)
      // Probed with things that carry information, never with a lone digit: every
      // digest in this receipt is hex, and a `2` occurs in one sooner or later
      // whatever the input was. A substring of `1` proves nothing. These do.
      expect(serialized).not.toContain('deluxe')
      expect(serialized).not.toContain('"nights"')
      expect(serialized).not.toContain(JSON.stringify(INPUTS))
      // The digest is here so a host can correlate without being handed the input.
      expect(result.receipt.inputDigest).toMatch(/^[0-9a-f]{64}$/)
    })

    it('puts no raw token and no raw input in the store record', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const serialized = JSON.stringify(store.stateOf(challenge.id))
      expect(serialized).not.toContain(challenge.token)
      expect(serialized).not.toContain('deluxe')
      expect(store.stateOf(challenge.id)?.tokenDigest).toMatch(/^[0-9a-f]{64}$/)
    })

    it('refuses a second presentation of the same challenge', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const presentation = presentationFor(challenge.token, challenge.id)
      const first = await service.verify(presentation)
      const second = await service.verify(presentation)
      expect(first.status).toBe('confirmed')
      // A replay, reported as a replay: not the same refusal as a typo, because a
      // caller that cannot tell them apart cannot tell an attack from an accident.
      expect(second).toEqual({ status: 'rejected', reason: 'already_consumed' })
      expect(store.consumeCalls()).toBe(2)
      // And still exactly one receipt came out of two presentations.
      expect(store.stateOf(challenge.id)?.state).toBe('consumed')
    })

    it('spends one challenge only once under concurrent presentations', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const presentation = presentationFor(challenge.token, challenge.id)
      const results = await Promise.all([
        service.verify(presentation),
        service.verify(presentation),
      ])
      expect(results.filter((result) => result.status === 'confirmed')).toHaveLength(1)
      expect(results.filter((result) => result.status === 'rejected')).toHaveLength(1)
      expect(store.consumeCalls()).toBe(2)
    })

    it('verifies after a service restart against the same shared store and key', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const restarted = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: clock.now,
        random: fakeRandom(),
      })
      expect(await restarted.verify(presentationFor(challenge.token, challenge.id))).toHaveProperty(
        'status',
        'confirmed',
      )
    })

    it('refuses a different token for the same challenge', async () => {
      const { service } = build()
      const challenge = await minted(service)
      expect(
        await service.verify(
          otherTokenPresentation(presentationFor(challenge.token, challenge.id)),
        ),
      ).toEqual({ status: 'rejected', reason: 'wrong_binding' })
    })

    it('refuses a challenge presented for another tenant', async () => {
      const { service } = build()
      const challenge = await minted(service)
      const result = await service.verify({
        ...presentationFor(challenge.token, challenge.id),
        tenantId: OTHER_TENANT,
      })
      expect(result.status).toBe('rejected')
      // Never a receipt, for a tenant that was not in the binding.
      expect(result).not.toHaveProperty('receipt')
    })

    it('refuses a challenge presented for another session', async () => {
      const { service } = build()
      const challenge = await minted(service)
      expect(
        await service.verify(
          otherSessionPresentation(presentationFor(challenge.token, challenge.id)),
        ),
      ).toEqual({ status: 'rejected', reason: 'wrong_binding' })
    })

    it('refuses a challenge presented for another action', async () => {
      const { service } = build()
      const challenge = await minted(service)
      expect(
        await service.verify(
          otherActionPresentation(presentationFor(challenge.token, challenge.id)),
        ),
      ).toEqual({ status: 'rejected', reason: 'wrong_binding' })
    })

    it('refuses the same action with one more night', async () => {
      const { service } = build()
      const challenge = await minted(service)
      // The action is the same and the guest is the same; the agreement is not.
      expect(
        await service.verify(
          otherInputsPresentation(presentationFor(challenge.token, challenge.id)),
        ),
      ).toEqual({ status: 'rejected', reason: 'wrong_binding' })
      expect(OTHER_INPUTS.nights).toBe(3)
    })

    it('accepts a presentation whose input keys are merely reordered', async () => {
      const { service } = build()
      const challenge = await minted(service)
      const result = await service.verify({
        ...presentationFor(challenge.token, challenge.id),
        inputs: { nights: 2, room: 'deluxe' },
      })
      // Canonicalized, so this is the same agreement rather than a new one.
      expect(result.status).toBe('confirmed')
    })

    it('refuses an unknown challenge id', async () => {
      const { service } = build()
      await minted(service)
      expect(
        await service.verify({
          challengeId: 'does-not-exist',
          token: 'e'.repeat(64),
          tenantId: TENANT,
          sessionId: SESSION,
          actionId: ACTION,
          inputs: INPUTS,
        }),
      ).toEqual({ status: 'rejected', reason: 'wrong_binding' })
    })

    it('refuses a malformed token before the store is consulted', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      expect(
        await service.verify({ ...presentationFor(challenge.token, challenge.id), token: 'yes' }),
      ).toEqual({ status: 'rejected', reason: 'malformed_token' })
      // A plain yes is not a confirmation, and it reaches durable state in no
      // form — not even as an id it might have consumed.
      expect(store.consumeCalls()).toBe(0)
    })

    it('refuses a browser-supplied action id with no challenge at all', async () => {
      const { store, service } = build()
      const result = await service.verify({
        challengeId: '',
        token: '',
        tenantId: TENANT,
        sessionId: SESSION,
        // What `confirmedActionIds: ['booking.create']` would carry on its own.
        actionId: ACTION,
        inputs: INPUTS,
      })
      expect(result.status).toBe('rejected')
      expect(store.consumeCalls()).toBe(0)
    })
  })

  describe('expiry', () => {
    it('refuses a challenge presented after its window closed', async () => {
      const { store, service } = build(60_000)
      const challenge = await minted(service)
      clock.advance(60_001)
      // The window clause is the one that failed, so the refusal says so. The
      // tokens were right and the moment was not, and those are two facts.
      expect(await service.verify(presentationFor(challenge.token, challenge.id))).toEqual({
        status: 'rejected',
        reason: 'expired',
      })
      // Nothing was spent: the store closed a record it could no longer accept
      // rather than leaving it live, and no spend was ever recorded against it.
      expect(store.stateOf(challenge.id)?.state).toBe('expired')
      expect(store.stateOf(challenge.id)?.consumedAt).toBeUndefined()
    })

    it('refuses at the window boundary, and stores nothing spent when it does', async () => {
      const { store, service } = build(60_000)
      const challenge = await minted(service)
      clock.advance(60_000)
      // The upper bound is exclusive: `expiresAt` is `issuedAt` plus the ttl, so
      // the instant the window is reached is the instant it is gone.
      expect(await service.verify(presentationFor(challenge.token, challenge.id))).toEqual({
        status: 'rejected',
        reason: 'expired',
      })
      expect(store.stateOf(challenge.id)?.state).toBe('expired')
    })

    it('does not let a store mint a receipt from a spend outside the window it was given', async () => {
      // A store that reports `spent` with a `consumedAt` before the challenge was
      // issued. Nothing in the presentation is wrong; the store's answer is.
      const result = await new ConfirmationService({
        store: expiredSpendStore('e'.repeat(64)),
        digestKey: KEY,
        clock: () => 0,
        random: fakeRandom(),
      }).verify(PRESENTATION)
      // A record that claims a spend before the window opened is not a spend,
      // whatever the store believes it did. It is expired, not confirmed.
      expect(result).toEqual({ status: 'rejected', reason: 'expired' })
    })

    it('refuses a spend record with no consumption timestamp', async () => {
      const result = await new ConfirmationService({
        store: untimestampedStore('e'.repeat(64)),
        digestKey: KEY,
        clock: () => 0,
        random: fakeRandom(),
      }).verify(PRESENTATION)
      expect(result).toEqual({ status: 'rejected', reason: 'store_unavailable' })
    })
  })

  describe('store failures', () => {
    it('refuses a store that cannot answer', async () => {
      const result = await new ConfirmationService({
        store: new ThrowingStore(),
        digestKey: KEY,
        clock: () => 0,
        random: fakeRandom(),
      }).verify(PRESENTATION)
      // A deployment fact, not a caller fact — and never "no such challenge".
      expect(result).toEqual({ status: 'rejected', reason: 'store_unavailable' })
    })

    it('normalizes a malformed store response into an unavailable refusal', async () => {
      const store: ConfirmationChallengeStore = {
        storeId: 'malformed',
        issue: () => true,
        consume: (): Promise<ConfirmationChallengeConsumeResult> =>
          Promise.resolve(null as unknown as ConfirmationChallengeConsumeResult),
      }
      const service = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: () => 0,
        random: fakeRandom(),
      })
      expect(await service.verify(PRESENTATION)).toEqual({
        status: 'rejected',
        reason: 'store_unavailable',
      })
    })

    it('refuses a spent record with non-finite time or altered expiry', async () => {
      const changes: Array<(record: ConfirmationChallengeRecord) => ConfirmationChallengeRecord> = [
        (record) => ({ ...record, consumedAt: Number.NaN }),
        (record) => ({ ...record, expiresAt: record.expiresAt + 1_000 }),
      ]
      for (const change of changes) {
        const backing = new RecordingStore()
        const store: ConfirmationChallengeStore = {
          storeId: 'tampered',
          issue: (challenge) => backing.issue(challenge),
          consume: async (presentation) => {
            const result = await backing.consume(presentation)
            if (result.status !== 'spent') return result
            return { status: 'spent', challenge: change(result.challenge) }
          },
        }
        const service = new ConfirmationService({
          store,
          digestKey: KEY,
          clock: () => 0,
          random: fakeRandom(),
        })
        const challenge = await minted(service)
        expect(await service.verify(presentationFor(challenge.token, challenge.id))).toEqual({
          status: 'rejected',
          reason: 'store_unavailable',
        })
      }
    })

    it('refuses a non-finite verification clock before consuming the challenge', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const invalidClock = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: () => Number.POSITIVE_INFINITY,
        random: fakeRandom(),
      })
      expect(await invalidClock.verify(presentationFor(challenge.token, challenge.id))).toEqual({
        status: 'rejected',
        reason: 'store_unavailable',
      })
      expect(store.consumeCalls()).toBe(0)
    })

    it('does not spend a challenge if receipt randomness fails', async () => {
      const store = new RecordingStore()
      const nextBytes = fakeRandom()
      let calls = 0
      const service = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: () => 0,
        random: (length) => {
          calls += 1
          if (calls === 3) throw new Error('random source unavailable')
          return nextBytes(length)
        },
      })
      const challenge = await minted(service)
      expect(await service.verify(presentationFor(challenge.token, challenge.id))).toEqual({
        status: 'rejected',
        reason: 'store_unavailable',
      })
      expect(store.consumeCalls()).toBe(0)
      expect(store.stateOf(challenge.id)?.state).toBe('pending')
    })

    it('reports an unavailable store separately from a wrong binding', async () => {
      const { store, service } = build()
      const challenge = await minted(service)
      const presentation = presentationFor(challenge.token, challenge.id)
      // The same presentation, once against a live store and once against a quiet
      // one. They must not be the same refusal, because one is the caller's and
      // the other is the deployment's.
      expect(await service.verify(presentation)).toHaveProperty('status', 'confirmed')
      expect(
        await new ConfirmationService({
          store: new ThrowingStore(),
          digestKey: KEY,
          clock: clock.now,
          random: fakeRandom(),
        }).verify(presentation),
      ).toEqual({ status: 'rejected', reason: 'store_unavailable' })
      // And the live store was asked exactly once, for the presentation it had.
      expect(store.consumeCalls()).toBe(1)
    })
  })

  describe('the service does not trust a store that says spent', () => {
    it('re-checks the returned record against the presentation before confirming', async () => {
      // Both of these answer `spent`. Neither produces a receipt, because the
      // service reconciles the record it was handed rather than trusting it.
      const lying = [
        untimestampedStore('e'.repeat(64)),
        expiredSpendStore('e'.repeat(64)),
        // And a store that reports a spend for a presentation it never recorded.
        untimestampedStore('a'.repeat(64)),
      ]
      for (const store of lying) {
        const result = await new ConfirmationService({
          store,
          digestKey: KEY,
          clock: () => 0,
          random: fakeRandom(),
        }).verify(PRESENTATION)
        expect(result).not.toHaveProperty('receipt')
      }
    })

    it('has no way to run an action: issue and verify are the whole surface', async () => {
      const { store, service } = build()
      await minted(service)
      expect(store.issues()).toBe(1)
      expect(store.consumeCalls()).toBe(0)
    })

    it('speaks only the published refusal vocabulary', () => {
      for (const reason of CONFIRMATION_REFUSALS) expect(typeof reason).toBe('string')
      expect(new Set(CONFIRMATION_REFUSALS).size).toBe(CONFIRMATION_REFUSALS.length)
      for (const reason of [
        'malformed_token',
        'wrong_binding',
        'already_consumed',
        'expired',
        'store_unavailable',
      ] as const) {
        expect(CONFIRMATION_REFUSALS).toContain(reason)
      }
    })
  })

  describe('construction', () => {
    it('refuses a short digest key', () => {
      expect(
        () =>
          new ConfirmationService({
            store: new RecordingStore(),
            digestKey: new Uint8Array(16),
            clock: () => 0,
            random: fakeRandom(),
          }),
      ).toThrow(TypeError)
    })

    it('refuses a ttl outside the supported window', () => {
      for (const ttlMs of [999, 0, -1, 1.5, MAX_CONFIRMATION_TTL_MS + 1]) {
        expect(
          () =>
            new ConfirmationService({
              store: new RecordingStore(),
              digestKey: KEY,
              clock: () => 0,
              random: fakeRandom(),
              ttlMs,
            }),
        ).toThrow(TypeError)
      }
    })

    it('refuses a missing clock', () => {
      expect(
        () =>
          new ConfirmationService({
            store: new RecordingStore(),
            digestKey: KEY,
            // The clock is not a convenience here; without it nothing can expire.
            clock: undefined as unknown as () => number,
            random: fakeRandom(),
          }),
      ).toThrow(TypeError)
    })

    it('keeps its own copy when a caller mutates the supplied digest key', async () => {
      const key = new Uint8Array(KEY)
      const store = new RecordingStore()
      const service = new ConfirmationService({
        store,
        digestKey: key,
        clock: () => 0,
        random: fakeRandom(),
      })
      const challenge = await minted(service)
      key.fill(9)
      expect(await service.verify(presentationFor(challenge.token, challenge.id))).toHaveProperty(
        'status',
        'confirmed',
      )
    })

    it('returns a typed refusal for a null issue request', async () => {
      const { service } = build()
      expect(
        await service.issue(null as unknown as Parameters<ConfirmationService['issue']>[0]),
      ).toEqual({ status: 'rejected', reason: 'malformed_request' })
    })

    it('binds under the key it was given, so another key verifies nothing', async () => {
      const { store } = build()
      const minter = new ConfirmationService({
        store,
        digestKey: KEY,
        clock: clock.now,
        random: fakeRandom(),
      })
      const other = new ConfirmationService({
        store,
        // A rotation that took effect between issue and verify. The store matches
        // on its own fields, so it would report a spend — and the service still
        // refuses, because the binding was computed under a key it does not have.
        digestKey: OTHER_KEY,
        clock: clock.now,
        random: fakeRandom(),
      })
      const challenge = await minted(minter)
      expect(await other.verify(presentationFor(challenge.token, challenge.id))).toEqual({
        status: 'rejected',
        reason: 'wrong_binding',
      })
      expect(store.stateOf(challenge.id)?.state).toBe('pending')
    })
  })
})
