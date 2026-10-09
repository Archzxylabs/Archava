/**
 * Server-side confirmation for state-changing Act actions.
 *
 * A confirmation answers one question — *did this tenant, in this session, agree
 * to this action with exactly these inputs, inside this window, presenting a token
 * this service minted, the first time it was presented?* — and answers it with a
 * receipt or a refusal.
 *
 * It is provider-neutral on purpose. There is no LLM here and no policy decision
 * here: a confirmation is not a permission, and this package does not grant one.
 * It is also the last thing standing between a plausible-looking request and a
 * state change, which is why every guarantee it makes is written down in
 * `confirmation.ts` with a test beside it in `test/`.
 *
 * What a host gets from here:
 *
 * - `ConfirmationService`, to mint and to verify.
 * - The store port, to be implemented against a durable store — one whose
 *   `consume` is genuinely atomic, which is the part a single-process test cannot
 *   prove and a reviewer must check.
 * - `ConfirmationReceipt`, to log or correlate. It carries the digests, never the
 *   payload.
 */

export {
  ConfirmationService,
  CONFIRMATION_REFUSALS,
  DEFAULT_CONFIRMATION_TTL_MS,
  MAX_CONFIRMATION_TTL_MS,
  isChallengeToken,
  type ConfirmationChallenge,
  type ConfirmationChallengeIssue,
  type ConfirmationChallengeRequest,
  type ConfirmationPresentation,
  type ConfirmationReceipt,
  type ConfirmationRefusal,
  type ConfirmationServiceOptions,
  type ConfirmationVerification,
  type ConfirmationBindingDescription,
} from './confirmation.js'

export {
  CHALLENGE_TOKEN_BYTES,
  DIGEST_KEY_MIN_BYTES,
  ID_BYTES,
  canonicalize,
  defaultRandomBytes,
  digestBinding,
  digestInputs,
  digestReceipt,
  digestToken,
  digestsEqual,
  isDigestKey,
  keyedDigest,
  newChallengeToken,
  newId,
  type DigestKey,
  type RandomBytes,
  type ReceiptFields,
} from './digest.js'

export type {
  ConfirmationChallengeConsumeResult,
  ConfirmationChallengeConsumption,
  ConfirmationChallengeInsert,
  ConfirmationChallengeRecord,
  ConfirmationChallengeState,
  ConfirmationChallengeStore,
} from './store.js'
