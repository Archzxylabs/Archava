-- ## Confirmation challenges: the durable half of one-time consent
--
-- A confirmation is a promise that a person, in a session, agreed to one
-- specific action. The promise is only worth what the record of it is worth, so
-- the record lives here rather than in the process that issued it: a restart, a
-- second replica, or a retried request must all reach the same verdict.
--
-- The port this implements is `ConfirmationChallengeStore` in
-- `@archava/act-confirmation`. Nothing in that port, and nothing here, decides
-- whether the action is allowed. That is `ActionPolicy`'s job, upstream, with
-- the action already bound. This table answers one narrow question and nothing
-- else: has this exact challenge been spent, and was it still spendable.

-- ## What the table stores
--
-- Six identifiers and three instants. The identifiers are the ones the caller
-- already holds — challenge id, tenant, session, action — plus the two digests
-- the port hands over. The instants are epoch milliseconds:
--
--   issued_at    when the challenge was minted
--   expires_at   when it stops being spendable (issued_at + ttl, by the service)
--   consumed_at  when this row transitioned to consumed, NULL until then
--
-- There is no column for anything a person typed, for the action's payload, for
-- the resulting object, or for anything about the browser or the mail that
-- carried the challenge.

-- ## What is not here, deliberately
--
-- `token_digest` and `binding_digest` are HMAC-SHA-256 hex digests computed by
-- `@archava/act-confirmation`. They are the only secret-shaped things in this
-- table, and they are digests precisely so that a dump of it does not hand
-- anyone a usable confirmation. What must never be added to this table:
--
--   * the confirmation token itself, or any prefix of it that could be
--     brute-forced back to it — a digest is one-way, a stored token is not;
--   * the HMAC key, or anything from which it could be derived;
--   * raw action input, entity resolution output, or provider payloads. A
--     challenge identifies an action; it does not need to remember its
--     arguments to decide that it has been spent;
--   * browser identifiers, IP addresses, or user agents. They are not part of
--     the binding and storing them turns a consent record into a tracking
--     record;
--   * tenant PII of any kind. Tenant, session and action are opaque ids as far
--     as this table is concerned.
--
-- A column proposed for this table that names a person, a message body, or a
-- credential is a column that belongs somewhere else.

-- ## The one statement the schema exists to make
--
-- `confirmation_challenges_pkey` is a primary key on the challenge id alone, and
-- primary keys are `NOT DEFERRABLE` — so is a `UNIQUE` constraint, and
-- `INITIALLY IMMEDIATE` is the default check time for both. That is the whole
-- guarantee. Two processes issuing the same id cannot both succeed: the second
-- `INSERT ... ON CONFLICT (id) DO NOTHING` inserts nothing and returns no row,
-- and the store reports the id as taken rather than throwing.
--
-- Making this constraint `DEFERRABLE`, or moving to a targetless
-- `ON CONFLICT DO NOTHING`, would put both halves of the guarantee at once. A
-- deferrable constraint "cannot be used as a conflict arbiter in an
-- `INSERT ... ON CONFLICT`", so the insert would raise a unique violation on a
-- second connection committing the same id — and a targetless `DO NOTHING`
-- would swallow a unique violation from *any* constraint, turning a
-- misconfiguration into a silent "inserted". The store names its conflict
-- target for that reason: a violation that is not the one it asked for is an
-- error, and an error is a refusal.

-- ## Why epoch milliseconds and not timestamptz
--
-- The service computes `issuedAt` and `expiresAt` from its own clock and passes
-- both to the store, and it passes the presentation time into `consume`. The
-- store compares the supplied time against the stored expiry, so the instant
-- under test has to be the caller's instant exactly. Converting through
-- `timestamptz` would mean `to_timestamp($1 / 1000.0)` — a division that lands
-- in a binary floating-point value, where 1770000000123 does not round-trip.
--
-- `bigint` holds the millisecond count exactly and makes the comparison an
-- integer comparison. `pg` returns `bigint` as a string unless a type parser is
-- installed, so the store's row reader accepts `number | string` and rejects
-- anything else — a row that cannot be read without guessing is a refusal, not
-- a row to repair.

-- ## What the constraints refuse to let a row say
--
-- `confirmation_challenges_state_known` limits `state` to the four states in the
-- port. A fifth value cannot be written, so a reader never has to guess what a
-- state it has not seen means.
--
-- `confirmation_challenges_consumed_is_stamped` ties `consumed_at` to the
-- `consumed` state in both directions: a consumed row always carries the instant
-- it was spent, and a pending, expired or revoked row never claims one. The
-- service re-checks this on the way out (`reconcile` requires a `consumedAt`
-- inside `[issuedAt, expiresAt)`), but it should not have to be able to fail.
--
-- `confirmation_challenges_window_positive` requires `expires_at > issued_at`.
-- The service already refuses a non-positive TTL; this keeps a hand-written row
-- from creating a challenge that was born expired.

CREATE TABLE IF NOT EXISTS confirmation_challenges (
  id             text   NOT NULL,
  tenant_id      text   NOT NULL,
  session_id     text   NOT NULL,
  action_id      text   NOT NULL,
  token_digest   text   NOT NULL,
  binding_digest text   NOT NULL,
  state          text   NOT NULL,
  issued_at      bigint NOT NULL,
  expires_at     bigint NOT NULL,
  consumed_at    bigint,

  CONSTRAINT confirmation_challenges_pkey PRIMARY KEY (id),

  CONSTRAINT confirmation_challenges_state_known
    CHECK (state IN ('pending', 'consumed', 'expired', 'revoked')),

  CONSTRAINT confirmation_challenges_consumed_is_stamped
    CHECK ((state = 'consumed') = (consumed_at IS NOT NULL)),

  CONSTRAINT confirmation_challenges_window_positive
    CHECK (expires_at > issued_at)
);

-- ## One index, and what it is for
--
-- Both statements the store runs find their row by the primary key. This index
-- exists for the only other question anyone asks of this table: which rows are
-- past their expiry, which is what a retention sweep needs and what nothing in
-- this package performs.

CREATE INDEX IF NOT EXISTS confirmation_challenges_state_expiry_idx
  ON confirmation_challenges (state, expires_at);

-- ## Retention
--
-- Nothing in this package deletes a row. There is no `DELETE` migration here,
-- and there should not be one in this file: a challenge that has vanished looks
-- exactly like a challenge that never existed, and both of those are refusals.
--
-- The consequence is worth stating plainly, because it is the opposite of the
-- attempt stores' reason for keeping rows. A deleted challenge does not create a
-- double confirmation — every path that finds no row answers "unknown", and the
-- service refuses it. Retention here is therefore a diagnostics commitment, not
-- a safety one: it buys the ability to answer *why* a confirmation was refused,
-- and to answer "already used" rather than "unknown challenge" for a spend that
-- someone repeats days later.
--
-- What a deployment should hold, and why:
--
--   * consumed rows must outlive the window in which an operator wants the
--     distinction between "already consumed" and "no such challenge". Once it is
--     gone, a replayed confirmation is indistinguishable from a forged id.
--   * pending rows must outlive `expires_at`, and the margins around it, by at
--     least the clock skew between the issuing process and whatever enforces
--     expiry — otherwise a challenge can become unspendable before it is
--     identifiable as expired.
--   * rows are never deleted while pending, whatever their age. "Expired" is a
--     state the store reports, not a state a sweeper asserts on the row's
--     behalf; deleting the row takes the report away.
--
-- A `DELETE` against this table is a change to what the service can tell
-- somebody about a confirmation they just used, not storage housekeeping.
