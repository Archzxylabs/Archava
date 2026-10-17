/**
 * The parity comparator, as its own module.
 *
 * It exists separately from `run-parity.mjs` for one reason: the comparator is
 * the thing that decides the pilot's verdict, so it has to be testable in
 * isolation. A harness whose comparison logic has never been shown to fail is
 * not an assertion — it is a pattern match that happens to agree with whatever
 * it is fed. `compare.test.mjs` proves this one catches the differences it
 * claims to catch.
 *
 * ## The verdict vocabulary
 *
 * Four outcomes, not two, because not every difference means the same thing:
 *
 * - `match` — byte-identical canonical output. The engines agree.
 * - `diverge` — the pricing results themselves differ. This is the failure the
 *   pilot exists to find.
 * - `error-message-only` — both engines rejected the request, but the message
 *   text differs. A real bug in the port, or a genuinely different message,
 *   and either way it is reported.
 * - `error-class-only` — both engines rejected the request with the *same*
 *   message, but the error's own name differs. This is the napi boundary's
 *   type mapping, not the pricing logic; it is real and observable, but it is
 *   not what the port is being judged on, so it is reported separately rather
 *   than folded into the pass rate or hidden.
 * - `corrected-expectation` — the engines diverge, the divergence is already
 *   recorded in `KNOWN_DIFFERENCES.md` with a stated reason, and the caller has
 *   supplied the approved expectation for this fixture. Reported in its own
 *   bucket, never merged into `match`, and still required to *be* the approved
 *   behaviour — see `approvedExpectation`.
 *
 * That last verdict is the one this comparator did not always have, and adding
 * it was a decision rather than a cleanup. The p2 harness exits non-zero on any
 * `diverge`, which is correct — but five `__proto__` fixtures diverge where the
 * baseline is provably wrong and the port is right (D02), so the run could never
 * be green. There are exactly two honest resolutions: drop the fixtures, or
 * record them as approved. Dropping them is the failure mode the harness's own
 * header warns about ("a parity harness that quietly drops the case it knows
 * will differ is not measuring parity"), so the mechanism below records them —
 * and records them *conditionally*, so an approved divergence that later changes
 * behaviour fails instead of being waved through.
 */

/** A window of context around an offset, for the divergence report. */
const CONTEXT = 60

/**
 * The offset of the first differing character between two strings.
 *
 * When one string is a prefix of the other, this returns the shorter length,
 * which is exactly where a truncated output differs.
 *
 * @param {string} left
 * @param {string} right
 * @returns {number}
 */
export function firstDifference(left, right) {
  let index = 0
  while (index < left.length && index < right.length && left[index] === right[index]) {
    index += 1
  }
  return index
}

/**
 * A window of context around an offset.
 *
 * @param {string} text
 * @param {number} index
 * @returns {string}
 */
function contextAround(text, index) {
  return text.slice(Math.max(0, index - CONTEXT), index + CONTEXT)
}

/**
 * Compare one fixture across both engines.
 *
 * @param {string} fixtureId used only to label the report
 * @param {string} tsText canonical TS output
 * @param {string} rustText canonical Rust output
 * @param {{rust: string, reason: string}} [approved] an approved corrected
 *   expectation for this fixture, as recorded in `KNOWN_DIFFERENCES.md`. Only
 *   consulted when the outputs actually differ; a fixture that matches needs no
 *   approval, and approving one that matches would be meaningless.
 * @returns {{verdict: string, message?: string}}
 */
export function compare(fixtureId, tsText, rustText, approved) {
  if (tsText === rustText) return { verdict: 'match' }

  const index = firstDifference(tsText, rustText)
  const detail =
    `${fixtureId}: canonical output diverges at offset ${index}\n` +
    `  TS:   ...${contextAround(tsText, index)}...\n` +
    `  Rust: ...${contextAround(rustText, index)}...`

  // Both sides throwing is a different situation from one side throwing. When
  // both threw, the interesting question is *why* they threw, so the two error
  // lines are parsed rather than compared as opaque text.
  //
  // The `s` flag, not `.*`, so a message spanning several lines still matches
  // whole — the baseline's own messages are one line, but the serde
  // deserialization errors this boundary also emits can wrap.
  const tsError = /^ERROR:(PricingError): ([\s\S]*)$/.exec(tsText)
  const rustError = /^ERROR:(Error): ([\s\S]*)$/.exec(rustText)

  if (tsError && rustError) {
    if (tsError[2] === rustError[2]) return { verdict: 'error-class-only', message: detail }
    return { verdict: 'error-message-only', message: detail }
  }

  // A divergence with an approved corrected expectation is still a divergence;
  // it is one with a reviewed reason. The approval is *checked*, not assumed:
  // if the port's output no longer matches the behaviour that was approved, this
  // stays a `diverge` and fails the run. An approval that stopped being verified
  // would be indistinguishable from an approval that was never earned, and the
  // whole point of recording the expectation is that the port keeps meeting it.
  if (approved) {
    if (rustText === approved.rust) {
      return {
        verdict: 'corrected-expectation',
        message: `${detail}\n  approved: ${approved.reason}`,
      }
    }
    return {
      verdict: 'diverge',
      message:
        `${detail}\n  an approved corrected expectation exists for this fixture but the ` +
        `port no longer produces it:\n    approved: ${approved.rust}\n    reason:   ${approved.reason}`,
    }
  }

  return { verdict: 'diverge', message: detail }
}
