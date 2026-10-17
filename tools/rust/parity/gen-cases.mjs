/**
 * The P1 fixture generator.
 *
 * Cases are synthetic: every tier name, count, add-on and forecast comes from
 * the public pricebook or is deliberately invalid. No owner data, no real
 * quotes, nothing read from `.local` or any environment file.
 *
 * ## How the set is built
 *
 * Three groups, each answering a different question:
 *
 * 1. **Hand-written boundary cases** — the shapes where the two engines are
 *    most likely to diverge for a reason that matters: a `custom: true` tier
 *    that also states a price, a count of zero versus one, an absent optional
 *    versus an explicit default. These are one per claim, so each has a name
 *    and a note saying what it claims.
 * 2. **The exhaustive cross-product** — every presence × capability ×
 *    environment combination in both regions. The pricebook has 3×4×7×2 = 168
 *    combos, small enough to enumerate completely, which is the part
 *    generation is genuinely good at: it cannot forget a tier, and it cannot
 *    test one that no longer exists.
 * 3. **Seeded combinations** — deterministic pseudo-random requests from a
 *    fixed seed, so a rerun produces the identical set. Randomness here is for
 *    coverage breadth; a non-reproducible case file would make the parity
 *    verdict unverifiable, because the digests in the evidence manifest would
 *    be meaningless.
 *
 * ## Why the counts include zero, one, and fractions
 *
 * `standardIntegrations: 0` skips a branch entirely — no line is pushed at
 * all — while `1` pushes a line whose total is zero. A fractional count
 * reaches an `f64` multiplication an integer-only fixture never would. The set
 * therefore includes `0`, `1`, `2`, `0.5`, and every capability's inclusion
 * allowance boundary.
 *
 * ## Why each case names only the fields it needs
 *
 * `QuoteRequestInit`'s defaults are part of what is under test, so pre-filling
 * them would hide exactly the behaviour being checked.
 */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Node globals this generator touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

/**
 * The pricebook, read so the case set enumerates its tiers.
 *
 * `../../../config/pricing.v1.json` is the tracked product file — the same bytes
 * `archava-pricing`'s tests embed via `include_str!` and the same object whose
 * sha256 the pilot recorded as `pricebookSha256`. Reading the tracked file rather
 * than a local copy is what keeps the case set provably about the pricebook the
 * product ships; this path previously named `../fixtures/`, a pilot-local copy
 * that does not exist here (D10's failure mode at the generator).
 */
const PRICEBOOK = JSON.parse(
  readFileSync(fileURLToPath(new URL('../../../config/pricing.v1.json', import.meta.url)), 'utf8'),
)

const REGIONS = ['GLOBAL', 'ID']
const PRESENCES = Object.keys(PRICEBOOK.regions.GLOBAL.presence)
const CAPABILITIES = Object.keys(PRICEBOOK.regions.GLOBAL.capability)
const ENVIRONMENTS = Object.keys(PRICEBOOK.regions.GLOBAL.environment)
const INTEGRATIONS = Object.keys(PRICEBOOK.regions.GLOBAL.integration)
const ADDONS = Object.keys(PRICEBOOK.regions.GLOBAL.design_addons)
const SUPPORTS = Object.keys(PRICEBOOK.regions.GLOBAL.support_addons)

/** Every case, in generation order — which is also the comparison order. */
const cases = []

/**
 * Add one case, rejecting a duplicate id.
 *
 * A duplicate id would make two different fixtures compare under one name, and
 * a diff that names the wrong fixture is worse than no diff.
 *
 * @param {string} id
 * @param {object} request a `QuoteRequestInit`
 * @param {string} note the claim this case makes
 */
function add(id, request, note) {
  if (cases.some((existing) => existing.id === id)) {
    throw new Error(`duplicate fixture id: ${id}`)
  }
  cases.push({ id, note, request })
}

// ---------------------------------------------------------------------------
// 1. Boundary cases, one per claim
// ---------------------------------------------------------------------------

/**
 * `existing_site` is `{setup: 0, monthly: 0}` — classified `fixed`, but with a
 * zero setup, so it is the one tier whose classification and its contribution
 * to the bundle disagree. It anchors the "does nothing join the bundle" cases.
 */
add(
  'boundary-minimal-existing-site',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
  },
  'the smallest quote: one priced component, no discount, no integrations',
)

add(
  'boundary-existing-site-id',
  {
    region: 'ID',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
  },
  'the same request in the ID region — the two pricebooks must never cross',
)

/**
 * The `custom_web_app` shape that started the whole investigation.
 *
 * Its raw JSON is `{setup_from: 8000, monthly: 0, custom: true}`, and zod's
 * union parsing strips the `custom` key on the way through, so `classifyLine`
 * sees only `setup_from` and `monthly` and classifies the line `from`. The
 * port reproduces that behaviour, not what the raw file spells.
 */
add(
  'boundary-custom-web-app-global',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'custom_web_app',
  },
  'custom_web_app classifies as `from`, not `custom`, because zod strips the flag',
)

add(
  'boundary-custom-web-app-id',
  {
    region: 'ID',
    presence: 'chat',
    capability: 'assist',
    environment: 'custom_web_app',
  },
  'the same stripped-flag line in ID',
)

add(
  'boundary-enterprise-capability',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'enterprise',
    environment: 'existing_site',
  },
  'the `enterprise` capability is `{custom: true}` with no other key',
)

add(
  'boundary-enterprise-environment',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'enterprise_system',
  },
  'the `enterprise_system` environment is `{custom: true}`',
)

/**
 * Zero versus one, everywhere the distinction is observable.
 *
 * A port that collapses `0` into "absent", or the reverse, produces a
 * different quote than the baseline on one of these.
 */
add(
  'boundary-integrations-zero',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    standardIntegrations: 0,
  },
  'a zero count pushes no line at all',
)

add(
  'boundary-integrations-fractional',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    standardIntegrations: 0.5,
  },
  'a fractional count reaches an f64 multiplication',
)

add(
  'boundary-integrations-negative',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    standardIntegrations: -5,
  },
  '`Math.max(0, -5)` clamps the count to zero before anything reads it',
)

/**
 * The capability inclusion allowance at its exact boundary.
 *
 * `transact` includes 2 standard integrations, so 2 requested means zero
 * chargeable and 3 means one chargeable — where an off-by-one in
 * `chargeableStandard` shows up.
 */
for (const count of [0, 1, 2, 3, 4]) {
  add(
    `boundary-allowance-transact-${count}`,
    {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'transact',
      environment: 'existing_site',
      standardIntegrations: count,
    },
    `transact includes 2 standard integrations; ${count} requested`,
  )
}

/** Absent versus explicit, for every field that has a default. */
add(
  'boundary-support-absent',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
  },
  "support omitted — `toRequest` defaults it to 'standard'",
)

add(
  'boundary-support-explicit-standard',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    support: 'standard',
  },
  'support stated explicitly; must equal the absent case',
)

add(
  'boundary-prepay-absent',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
  },
  'annualPrepay omitted, so annualPlatformOption is null',
)

add(
  'boundary-prepay-false',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    annualPrepay: false,
  },
  'prepay stated false; must equal the absent case',
)

add(
  'boundary-prepay-true',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    annualPrepay: true,
  },
  'prepay stated true, so the annual option is a number',
)

/**
 * `usageForecast` — the field deliberately given no default.
 *
 * Absent means no estimate; `0` means an estimate of zero. Both compute the
 * same overage, so only a port that kept them distinguishable passes, and one
 * that flattened them cannot be caught except here.
 */
add(
  'boundary-forecast-absent',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
  },
  'usageForecast omitted',
)

add(
  'boundary-forecast-zero',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    usageForecast: 0,
  },
  'usageForecast stated as zero',
)

add(
  'boundary-forecast-at-allowance',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    usageForecast: 1000,
  },
  'exactly at the chat allowance of 1000 — overage must be zero, not epsilon',
)

add(
  'boundary-forecast-just-above',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    usageForecast: 1001,
  },
  'one active conversation over the allowance',
)

add(
  'boundary-forecast-fractional',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    usageForecast: 1000.5,
  },
  'a fractional forecast, so the overage is a fractional f64',
)

/** The half-open rounding boundary, on both sides of 25. */
add(
  'boundary-rounding-exact-boundary',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'act',
    environment: 'landing',
  },
  'a total landing exactly on a 25 boundary',
)

add(
  'boundary-rounding-below-boundary',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'act',
    environment: 'business',
  },
  'a total just under a boundary',
)

/** Design add-ons in all three classifications. */
for (const addon of ADDONS) {
  add(
    `boundary-addon-${addon}`,
    {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'assist',
      environment: 'existing_site',
      designAddons: [addon],
    },
    `design add-on ${addon} alone`,
  )
}

add(
  'boundary-addon-duplicated',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    designAddons: ['premium_avatar_provider', 'premium_avatar_provider'],
  },
  'a duplicated custom add-on — the driver dedupes, the add-on list does not',
)

add(
  'boundary-addon-both-priced',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    designAddons: ['custom_visual_direction', 'advanced_motion_3d'],
  },
  'one fixed plus one `from` add-on together',
)

add(
  'boundary-addon-unknown',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    designAddons: ['a_hologram'],
  },
  'an add-on the pricebook does not carry — scoped, not rejected',
)

/**
 * All four integration complexities together.
 *
 * The baseline pushes them in the order the request's fields are read, not the
 * order the pricebook lists them, so this is an ordering claim separate from
 * each complexity's arithmetic.
 */
add(
  'boundary-all-integration-kinds',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'transact',
    environment: 'existing_site',
    standardIntegrations: 1,
    customIntegrations: 1,
    advancedIntegrations: 1,
    enterpriseIntegrations: 1,
  },
  "all four complexities in one request, in the request's field order",
)

add(
  'boundary-enterprise-integration-only',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    enterpriseIntegrations: 1,
  },
  'an enterprise integration alone forces a custom quote',
)

add(
  'boundary-enterprise-integration-fractional',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'assist',
    environment: 'existing_site',
    enterpriseIntegrations: 2.5,
  },
  'a fractional enterprise count reaches the driver message',
)

/**
 * Driver ordering, with several drivers firing at once.
 *
 * Their order in the output is the engine's code order, not the request's, so
 * this is a claim of its own separate from each driver individually.
 */
add(
  'boundary-many-drivers-at-once',
  {
    region: 'GLOBAL',
    presence: 'chat',
    capability: 'enterprise',
    environment: 'custom_web_app',
    standardIntegrations: 1,
    customIntegrations: 1,
    advancedIntegrations: 1,
    enterpriseIntegrations: 1,
    designAddons: ['premium_avatar_provider', 'advanced_motion_3d'],
    support: 'dedicated_sla',
    usageForecast: 5000,
    annualPrepay: true,
  },
  'every driver firing at once, in the order the engine emits them',
)

// ---------------------------------------------------------------------------
// 2. The exhaustive cross-product
// ---------------------------------------------------------------------------

/**
 * Every tier triple in both regions — 168 combinations, all reachable.
 *
 * Generation wins over hand-writing here precisely because it cannot forget a
 * tier and cannot test one the pricebook no longer carries.
 */
for (const region of REGIONS) {
  for (const presence of PRESENCES) {
    for (const capability of CAPABILITIES) {
      for (const environment of ENVIRONMENTS) {
        add(
          `cross-${region}-${presence}-${capability}-${environment}`,
          { region, presence, capability, environment },
          `exhaustive: ${region}/${presence}/${capability}/${environment}`,
        )
      }
    }
  }
}

/** Every support tier against a fixed-priced triple, in both regions. */
for (const region of REGIONS) {
  for (const support of SUPPORTS) {
    add(
      `support-${region}-${support}`,
      {
        region,
        presence: 'chat',
        capability: 'transact',
        environment: 'existing_site',
        support,
      },
      `support tier ${support} in ${region}`,
    )
  }
}

/**
 * Every integration complexity at each count, in both regions.
 *
 * The counts straddle the inclusion allowances: with `transact` including 2,
 * 0/1/2/3 covers absent, below, at, and above.
 */
for (const region of REGIONS) {
  for (const complexity of INTEGRATIONS) {
    for (const count of [0, 1, 2, 3]) {
      add(
        `integration-${region}-${complexity}-${count}`,
        {
          region,
          presence: 'chat',
          capability: 'transact',
          environment: 'existing_site',
          [`${complexity}Integrations`]: count,
        },
        `${complexity} integrations, count ${count}`,
      )
    }
  }
}

// ---------------------------------------------------------------------------
// 3. Seeded combination coverage
// ---------------------------------------------------------------------------

/**
 * A deterministic generator, so the case file is reproducible.
 *
 * `mulberry32`, counter-based. Seeded rather than `Math.random` because the
 * parity verdict must be checkable: the same seed must produce the same cases
 * on every machine and every run, or the digests in the evidence manifest mean
 * nothing.
 *
 * @param {number} seed
 * @returns {() => number} floats in [0, 1)
 */
function mulberry32(seed) {
  let state = seed
  return () => {
    state = (state + 0x6d2b79f5) | 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const random = mulberry32(0x5eed)

/**
 * Pick one element from a list.
 *
 * @template T
 * @param {readonly T[]} list
 * @returns {T}
 */
function pick(list) {
  return list[Math.floor(random() * list.length)]
}

/**
 * Pick a count straddling the interesting values — mostly small integers,
 * occasionally zero, occasionally fractional, so combinations exercise the
 * branch structure rather than only the arithmetic.
 *
 * @returns {number}
 */
function pickCount() {
  return pick([0, 0, 1, 1, 2, 3, 4, 7, 0.5, 1.5, 12.25])
}

/** A forecast from the interesting set, or nothing at all. */
function pickForecast() {
  return pick([undefined, 0, 500, 999, 1000, 1001, 5000, 250_000])
}

/**
 * Build one seeded case.
 *
 * @param {number} index
 */
function seededCase(index) {
  const request = {
    region: pick(REGIONS),
    presence: pick(PRESENCES),
    capability: pick(CAPABILITIES),
    environment: pick(ENVIRONMENTS),
  }

  // Integration counts are assigned by field, so *which* field carries a
  // nonzero count is itself part of the variation.
  if (random() < 0.6) request.standardIntegrations = pickCount()
  if (random() < 0.3) request.customIntegrations = pickCount()
  if (random() < 0.3) request.advancedIntegrations = pickCount()
  if (random() < 0.15) request.enterpriseIntegrations = pickCount()

  const addonCount = pick([0, 0, 1, 1, 2])
  if (addonCount > 0) {
    const addons = []
    for (let i = 0; i < addonCount; i += 1) {
      const candidate = pick(ADDONS)
      // Duplicates are allowed deliberately: `unresolvedScopeDrivers` is a
      // `Set` and the add-on list is not, so the pair tests the difference.
      if (!addons.includes(candidate) || random() < 0.2) addons.push(candidate)
    }
    if (addons.length > 0) request.designAddons = addons
  }

  const support = pick(SUPPORTS)
  if (support !== 'standard' || random() < 0.2) request.support = support

  const forecast = pickForecast()
  if (forecast !== undefined) request.usageForecast = forecast
  if (random() < 0.3) request.annualPrepay = random() < 0.5

  add(`seeded-${String(index).padStart(3, '0')}`, request, 'seeded combination')
}

for (let index = 0; index < 120; index += 1) seededCase(index)

// ---------------------------------------------------------------------------
// 4. The invalid cases
// ---------------------------------------------------------------------------

/**
 * Requests that must fail, and fail in a specific order.
 *
 * The four `PricingError` messages are part of the observable contract, and a
 * request bad in several ways reports only the first check that fires. These
 * pin that order down.
 */
const INVALID = [
  {
    id: 'invalid-presence',
    request: {
      region: 'GLOBAL',
      presence: 'telepathy',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'unknown presence',
  },
  {
    id: 'invalid-capability',
    request: {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'teleport',
      environment: 'existing_site',
    },
    why: 'unknown capability',
  },
  {
    id: 'invalid-environment',
    request: {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'assist',
      environment: 'the_moon',
    },
    why: 'unknown environment',
  },
  {
    id: 'invalid-all-three-check-order',
    request: {
      region: 'GLOBAL',
      presence: 'bad',
      capability: 'bad',
      environment: 'bad',
    },
    why: 'presence is checked first, so a request bad three ways names it',
  },
  {
    id: 'invalid-capability-and-environment',
    request: {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'bad',
      environment: 'bad',
    },
    why: 'capability is checked before environment',
  },
  {
    id: 'invalid-presence-empty-string',
    request: {
      region: 'GLOBAL',
      presence: '',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'the empty string is a name, and it matches nothing',
  },
  {
    id: 'invalid-presence-in-id-only',
    request: {
      region: 'GLOBAL',
      presence: 'empat',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'a tier name from the other region is still unknown in this one',
  },
  {
    id: 'invalid-whitespace-presence',
    request: {
      region: 'GLOBAL',
      presence: ' chat ',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'no trimming anywhere in the lookup',
  },
]

/**
 * Requests with a tier name *omitted entirely*, rather than named wrongly.
 *
 * These exist because they are the one shape the whole table above cannot
 * reach, and the pilot's most severe defect lived exactly here. The first port
 * of `toRequest` treated the four tier names as required and called
 * `.expect()`, so a request missing one aborted the **host process** at the
 * napi boundary — `fatal runtime error: failed to initiate panic`, exit 134,
 * core dumped — where the baseline threw a catchable `PricingError`. The
 * `INVALID` table cannot catch this: every entry in it names all four tiers,
 * so none of them takes the absent path.
 *
 * What the baseline does with each omitted name is *not* the same thing. Three
 * of them reach an object-lookup miss and throw the tier's own error with the
 * literal string `undefined`, because `selected.presence[undefined]` coerces
 * the key to `"undefined"`. The fourth — `region` — misses the *regions* table
 * first and throws `Unknown region "undefined". No quote produced.`, a
 * different sentence. A port that produced one message for all four would match
 * on three cases and diverge on the fourth, which is why each is a separate
 * case rather than one.
 *
 * These are also a liveness check on the run itself: a port that reverted to
 * panicking would abort the parity process rather than diverge, so the run
 * would stop instead of reporting.
 */
const ABSENT_TIERS = [
  {
    id: 'absent-region',
    request: {
      presence: 'chat',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'the regions table is checked before any tier, so this throws the region message',
  },
  {
    id: 'absent-presence',
    request: {
      region: 'GLOBAL',
      capability: 'assist',
      environment: 'existing_site',
    },
    why: 'a missing presence reaches the lookup as the literal "undefined"',
  },
  {
    id: 'absent-capability',
    request: {
      region: 'GLOBAL',
      presence: 'chat',
      environment: 'existing_site',
    },
    why: 'capability is checked after presence, so presence must be valid here',
  },
  {
    id: 'absent-environment',
    request: {
      region: 'GLOBAL',
      presence: 'chat',
      capability: 'assist',
    },
    why: 'the last of the three Step A checks',
  },
]
for (const { id, request, why } of ABSENT_TIERS) add(id, request, why)

for (const { id, request, why } of INVALID) add(id, request, why)

// ---------------------------------------------------------------------------
// Emit
// ---------------------------------------------------------------------------

/**
 * The directory the generated case file lands in.
 *
 * Gen-cases lives alongside the other harness scripts in `node/`, but the
 * cases themselves are generated data, not source. Keeping them one level down
 * means a `git status` shows a dirty directory rather than thousands of lines
 * in the harness's own folder, and the run script reads this exact path.
 */
const CASES_DIR = fileURLToPath(new URL('./cases/', import.meta.url))

/**
 * The declared path of the generated case file.
 *
 * `run-parity.mjs` reads this same path, so the two must agree by construction
 * rather than by copy-paste. If they ever diverge the run fails loudly with an
 * ENOENT rather than silently comparing against a stale file.
 */
const CASES_FILE = `${CASES_DIR}pricing.json`

const payload = {
  slice: 'pricing',
  generatedBy: 'gen-cases.mjs',
  // Names the pricebook both engines are run against, so a reader of the case
  // file can find the object the verdict is about without hunting. It records the
  // tracked product path, not this generator's own resolution: the field is
  // provenance, and a path relative to the generator would stop being meaningful
  // the moment the file is read from anywhere else — which is the whole failure
  // mode of the `../fixtures/` string this replaces.
  pricebook: 'config/pricing.v1.json',
  // Counted, so a future edit that changes the set is visible rather than
  // silently altering what the parity verdict covers.
  count: cases.length,
  cases,
}

// Recursive, so the directory is created if this is a fresh checkout — a
// generator that only works on a second run is worse than useless, it looks
// like it succeeded.
mkdirSync(CASES_DIR, { recursive: true })
writeFileSync(CASES_FILE, `${JSON.stringify(payload, null, 2)}\n`)

process.stdout.write(`wrote ${cases.length} pricing fixtures to ${CASES_FILE}\n`)
