-- Act attempt records: the durable half of the booking and email replay
-- boundaries in packages/act-booking and packages/act-email.
--
-- One row is one attempt to perform one side effect, addressed by the identity
-- the boundary already computes: tenant, action kind, idempotency key. The row
-- is what makes "at most once" true across processes, restarts and replays.
--
-- ## What is not here, deliberately
--
-- No recipient, no customer reference, no slot, no notes, no template body, no
-- provider payload, no provider credential and no HMAC key. The pipeline's raw
-- idempotency key includes canonical action inputs and may contain visitor data;
-- the adapter stores a tenant/action-bound HMAC of it in idempotency_key instead.
-- `fingerprint` is a separate keyed digest of the payload. Neither raw input
-- nor raw idempotency key reaches this table or the SQL client's parameters.
-- Both HMAC secrets must remain stable across processes and restarts for the
-- lifetime of these rows. Rotation requires a deliberate data migration; a new
-- key without migrating old identities would make old attempts look unclaimed.
--
-- ## The one statement the schema exists to make
--
-- A unique constraint on (tenant_id, action_kind, idempotency_key) is what
-- makes `claim` atomic. It is NOT DEFERRABLE, which is PostgreSQL's default and
-- is stated here so nobody "improves" it into DEFERRABLE: a deferrable
-- constraint is checked at COMMIT, which would let two concurrent transactions
-- both believe they inserted the first copy. Uniqueness must be checked
-- immediately whenever a row is inserted.
--
-- That constraint is also the whole reason a race cannot produce a double side
-- effect. Two processes racing to claim the same key: exactly one INSERT
-- succeeds; the other hits the unique index and, with ON CONFLICT DO NOTHING,
-- reports zero rows rather than an error.
--
-- ## Why one table rather than two
--
-- `booking` and `email` attempts have identical storage shapes: the same
-- composite identity, the same fingerprint, the same four-state machine, one
-- nullable provider identifier. `action_kind` separates them. Two tables would
-- duplicate every constraint and every query in this package while adding a
-- cross-kind migration hazard — and would still need `action_kind` anyway the
-- moment a third action appears. One table, one constraint, `action_kind` as a
-- discriminator.
--
-- ## Retention
--
-- These rows are idempotency evidence. A record deleted early makes a replay
-- that arrives after the deletion look like a first attempt, which is precisely
-- the double-booking / double-send this table exists to prevent. So:
--
--   * An `in_flight` / `prepared` row is never deleted by any process; it is
--     the only trace of an attempt whose outcome nobody knows.
--   * Terminal rows are retained for at least the replay lifetime the owning
--     boundary guarantees to its callers, which is a deployment decision and
--     not one this package can make. Delete only after it has certainly
--     outlived every client that could still be retrying with that key.
--   * If a client reuses idempotency keys without a guaranteed lifetime, these
--     rows must live for that entire reuse window and beyond, or the guarantee
--     is void.
--
-- A `DELETE` migration against this table is a change to the at-most-once
-- guarantee, not storage housekeeping. Nothing in this package deletes rows.

CREATE TABLE IF NOT EXISTS act_attempts (
    -- The composite identity. All three together are the primary key, which is
    -- why (tenant_id, action_kind, idempotency_key) is the uniqueness arbiter
    -- the claim relies on.
    tenant_id    text        NOT NULL,
    action_kind  text        NOT NULL,
    idempotency_key text     NOT NULL, -- HMAC-SHA256 hex, never the pipeline's raw key

    -- A keyed digest of the payload. Hex, fixed length. Not a secret: it is a
    -- MAC, and it only answers "is this the same payload that claimed this
    -- key". Nothing here identifies a person, a slot or a message.
    fingerprint  text        NOT NULL,

    -- 'booking': in_flight | confirmed | rejected | unknown
    -- 'email':   prepared  | accepted  | rejected | unknown
    -- The boundary interprets the state; the store only records what it is told
    -- and refuses to hand back a row whose state it cannot name.
    state        text        NOT NULL,

    -- The opaque identifier the authoritative system issued, and only that.
    -- booking_reference or provider_message_id — never two in one row, because
    -- action_kind decides which one this is.
    provider_id  text,

    -- Kept for forensics and retention policy only. Nothing branches on them.
    claimed_at   timestamptz NOT NULL DEFAULT now(),
    settled_at   timestamptz,

    -- One storage of one fact, for one tenant, under one key.
    --
    -- NOT DEFERRABLE is the default and is spelled out because the alternative
    -- is a silent loss of the atomic claim: see the header. The accompanying
    -- unique B-tree index is the arbiter for INSERT ... ON CONFLICT, and an
    -- arbiter must be a NOT DEFERRABLE constraint or unique index.
    CONSTRAINT act_attempts_identity_pkey
        PRIMARY KEY (tenant_id, action_kind, idempotency_key),

    -- A record may only carry a provider identifier when it is in a state that
    -- has one. This is a backstop against a bug writing a fabricated reference
    -- into a row, not the primary defence; the adapter also validates on read.
    CONSTRAINT act_attempts_provider_id_shape
        CHECK (
            (provider_id IS NULL AND state IN ('in_flight', 'prepared', 'rejected', 'unknown'))
            OR
            (provider_id IS NOT NULL AND state IN ('confirmed', 'accepted'))
        )
);

-- The primary key above already creates this index. These two are for the
-- retention sweep and the operator's questions ("what is still in flight for
-- this tenant"), which scan by tenant and state rather than by key.
CREATE INDEX IF NOT EXISTS act_attempts_tenant_state_idx
    ON act_attempts (tenant_id, action_kind, state);
