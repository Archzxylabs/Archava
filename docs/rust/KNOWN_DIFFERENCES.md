# Known differences and corrected expectations

Frozen at R00, per PRD §23 G01/G02: corrected expected behaviour and explicit
contract differences are recorded _before_ the affected Rust slice is accepted.
A divergence is never silently folded into a pass rate.

## D01 — p1 pricing: 12 error-class-only differences

**Baseline:** TypeScript.
**Measured:** 350 fixtures, 338 matching outcomes, 12 differences — all of them
error-_class_ only (both engines reject the fixture; the error type differs).

**Corrected expectation:** the Rust engine's error classification is the
corrected one where it is more specific, but this is a _recorded contract
difference_, not a match. The compatibility boundary must map the Rust error
class back to the TypeScript one for existing consumers until that mapping is
versioned as a deliberate change. The paragraphs below are what that resolves to
on this tree: the shipping crate boundary needs no mapping because it kept the
contract, and the addon boundary has no existing consumer to map onto.

**Affected gate:** G02 Contracts.
**Status:** `locally_proven` for the disposition below, `specified` for the
contract. The disposition is now measured and the two halves of §1.1 item 2
resolve differently, so they are recorded separately rather than as one row.

**Which boundary the contract question is about (this checkpoint).** The two
candidates are the crate boundary, which is the shipping one, and the napi addon,
which is not. That split was measured, not assumed. `archava-native` appears
nowhere in `config/library-release.v1.json`, nowhere in `dist/library/artifacts.json`,
and a grep of `apps/`, `packages/`, `tools/` and `config/` for its exports
(`libarchava_native`, `quoteJson`, `ratesJson`) finds exactly one hit: its own
`.so`→`.node` copy step in `tools/rust/build-artifacts.mjs`. No product package
consumes it. Its only consumer is Consumer B of the two cold consumers
(`examples/rust-consumers/addon/host/verify.mjs`). So the question "must the
error contract be recovered for existing consumers?" has, on this tree, no
existing consumer to recover it for.

**The crate boundary preserves the contract and is stronger than the baseline.**
`archava_pricing::error::PricingError` carries the same message `engine.ts`'s
`PricingError` does, and `PricingErrorKind` exposes four variants —
`UnknownRegion`, `UnknownPresence`, `UnknownCapability`, `UnknownEnvironment`
— with a `kind()` accessor. That is a real discriminant a consumer can match on.
The TypeScript engine has no equivalent: its `PricingError` is a `name` and a
message, so a consumer can only distinguish failures by matching message text,
which `rust/crates/pricing/src/error.rs`'s header records plainly as "good
_evidence_" rather than a good API. Nothing is recovered here because nothing was
lost; the port kept the contract and added a discriminant.

**The addon boundary cannot carry the name, and that is structural.** napi
3.14.2's `JsError::into_value` calls `napi_create_error`, which takes a status
code and a message and has no name field — `napi::Error` stores status, reason,
cause and an optional ref and nothing else. Every `napi::Error` crossing into
JavaScript is therefore an `Error`, whatever the Rust side called it. This is why
D01 is a disposition rather than a fix: the capability does not exist at that
boundary, so there is no work in `archava-native` that would close it.

**What all 12 actually are, measured rather than asserted (this checkpoint).**
Running all 350 cases through the built addon: **all 12 are engine refusals whose
messages are character-identical to `engine.ts`'s, and not one is a serde
deserialization error.** The 12 are `absent-region`, `absent-presence`,
`absent-capability`, `absent-environment`, `invalid-presence`,
`invalid-capability`, `invalid-environment`, `invalid-all-three-check-order`,
`invalid-capability-and-environment`, `invalid-presence-empty-string`,
`invalid-presence-in-id-only` and `invalid-whitespace-presence`. Each carries
`code: 'GenericFailure'`, which is what distinguishes an engine refusal from a
request the boundary itself rejects: those carry `code: 'InvalidArg'` and a
`could not deserialize the request` message. That discrimination is the
replacement contract, and it is stronger than the `name` TypeScript lost, because
a `name` does not tell a caller whether the refusal came from the engine's policy
or from a malformed request.

**The disposition is proved, not described.** `tools/rust/test/error-contract.test.mjs`
(4 tests, wired into the fast gate's `node --test` step) drives the real built
addon against the real `config/pricing.v1.json` and the real `engine.ts`, loaded
through the same `tsx/esm/api` route `run-parity.mjs` uses, and asserts:
(1) each of the 12 messages survives the boundary character for character;
(2) `error.name` is `Error` and is _not_ `PricingError` — asserting the absence of
the capability, so a future improvement is not misreported as a regression and
someone cannot "fix" this entry by editing its prose; (3) all 12 carry
`GenericFailure` with no deserialization wording, so a boundary change that began
wrapping engine refusals would fail here before it reclassified the parity
evidence; and (4) the opposite direction — a request the boundary itself rejects
carries `InvalidArg` with `could not deserialize the request`. A check that only
asserted (3) would pass just as happily if every error carried that code.

**A first-draft fixture here was wrong, and is recorded because it is the same
shape as the bug `request.rs` documents.** The test's unknown-region case was
first hand-written as `region: 'MARS'`. That is not an engine refusal at all:
`region` is a typed `RegionCode` enum in Rust, so an out-of-variant string is
rejected by serde at the boundary (`InvalidArg`, "unknown variant `MARS`, expected
`GLOBAL` or `ID`"), while the baseline's untyped string reaches the engine and
produces its own message. Had that fixture shipped, the test would have asserted
a _difference_ where D01's twelve show _agreement_ — the exact failure direction
that makes a disposition look better than it is. The cases are now read by id from
`tools/rust/parity/cases/pricing.json` instead of being restated here, so there is
one definition of what was measured, and the file fails naming an id if the case
set moves.

**What is not closed.** The addon's contract remains `specified`: nothing
versioned maps these twelve onto the TypeScript error class for a consumer that
exists, and none does today. If a future release introduces one, this entry's
half-open status is the trigger to revisit it — not the count, which is already
pinned in both directions.

## D02 — p2 reference rates: 5 divergences, all baseline defects

**Baseline:** TypeScript, which is _wrong_ in these five cases.
**Measured:** 753 fixtures, 748 match, 5 diverge.

**Root cause:** the baseline reads object properties through a path where
`__proto__` is a real key. `roomsBuiltOf('__proto__')`,
`baseRateOf('__proto__')` and `nightlyRate('__proto__', …)` return
`Object.prototype`'s inherited members instead of "no such room", so the
baseline silently produces a value where the correct answer is absence.

**Corrected expectation:** the Rust behaviour is correct and the baseline is
buggy. These five are frozen as corrected expectations, and each ships with a
reproducer naming the exact key.

**Affected gate:** G02 Contracts, G04 Domain integrity.
**Status:** `locally_proven` for the corrected expectation, `specified` for the
contract. Both halves matter and they are different claims.

The _approval_ is now enforced and tested. The five live in the p2 harness's
`APPROVED` map, `compare.mjs` re-verifies each run that the port still emits
`null`, and `compare.test.mjs` — promoted into `tools/rust/parity/`, 13 tests,
all passing under `node --test` — asserts the mechanism in both directions: an
approval whose behaviour matches is a `corrected-expectation`, and an approval
whose behaviour _changed_ reverts to `diverge` and fails the run. That second
test is the one that makes the first meaningful; an approval that cannot fail is
not a check.

The _contract_ is still `specified`: nothing versioned maps these five onto the
TypeScript error class for existing consumers, which is D01's row and is not
advanced by this.

**Not a licence:** must not be dropped to raise a match rate. PRD §4 explicitly
forbids treating "the whole divergence set" as ignorable.

## D03 — Rust parity is a small fraction of the source, and only one slice is proven

**Measured (2026-10-15, `node tools/rust/count-lines.mjs --rule code`):**
2,844 Rust production lines against 103,068 TypeScript production lines — **2.76%**
of the source, both sides under one rule in one run. Counting every non-blank
line on both sides instead: 5,050 against 134,001.

This entry previously read "3,830 lines of Rust against 125,573 lines of
TypeScript". Neither number could be reproduced by any counting rule tried — the
TypeScript figure in particular resisted raw tracked lines, tracked-minus-tests,
non-blank-non-comment, at the working tree and at the baseline HEAD, which
returned 115,910 / 114,388 / 133,663 / 120,439 / 152,023 among others. A
denominator nothing reproduces makes every ratio computed from it decoration, so
both figures were replaced with ones a named tool derives on demand. The
replacement is smaller than the old claim, which is the direction this file
should err.

One crate (`rates`) now has a differential parity harness (`tools/rust/parity/`),
and `pricing` has one too since the R02 promotion — so the old sentence "one crate
has a parity harness" is also out of date and reads **two of seven** here.

**Consequence:** statements about "the Rust platform" cannot be made. The honest
claim is that two slices are `locally_proven` and the rest of the product is
`specified`. Any completion report must carry these denominators.
**Cold consumers, measured.** Two exist and both pass offline
(`pnpm rust:verify-consumers`, evidence `dist/rust-consumer-proof.json`). Two of
seven crates are packaged for consumption — `archava-pricing` and `archava-rates`.
The other five are not, and no percentage should be read off 2/7.

## D04 — `planning` and `schedule` still expose `placeholder()` at their entry points

**Observed at the pilot snapshot.** A caller invoking the documented entry point
receives a placeholder rather than a domain result.

**Status:** `specified` — these are not completed slices and must not be counted
as ported. R02 completes or explicitly retires them.

## D05 — A01 remains open: replay after revocation is not covered

Rechecked against current source, not inherited from the audit label. The
command ledger has no test asserting that a replayed command is refused _after_ its
grant is revoked; `crm-assistant/test/conversation.test.ts` covers
revocation in the assistant path only.

**Affected gate:** G03 Authority.
**Status:** `specified` — conformance test required before the affected Rust
slice is accepted.

## D06 — Archive identity drift (A07)

The archived 1.17.0 library and the current 1.17.0 candidate are different
artifacts (58 vs 60 artifacts; assembly 15 vs 16 at the time of that audit).
Both are recorded; neither is rewritten.

**Affected gate:** G09 Release identity.
**Status:** recorded, unreconciled by design.

## D07 — Compatibility bridges are mapped, not solved

The Python template backend, the Gemini HTTP/SSE adapter, and any
JavaScript-oriented vendor SDK remain compatibility surfaces. Each is named in
the inventory with an explicit disposition. A bridge that is not yet exercised
is listed as an exception rather than assumed working.

**Affected gate:** G10 Operations/retirement.

## D08 — The release catalogue does not verify migration ownership in reverse

**Found while measuring the inventory, not inherited from any audit.**

`packages/config/src/release-contracts.ts:88-95` validates one direction only:
every ledger listed in `databaseLedgers` must name a package that owns the
listed migration files (`Unowned database migration`). Nothing checks the other
direction — a package that ships migrations but is _not_ listed as a ledger
passes validation silently.

**Is it exploitable?** Not today, and the reason is worth recording rather than
assuming. Two packages exercise the gap right now: `@archava/act-storage` and
`@archava/act-confirmation-pg` own real schema, are packaged with checksums, and
are absent from `databaseLedgers`. They are absent deliberately — both are
public-schema legacy ports with zero `archava_*` references and no RLS, applied
only to a disposable fixture (`tools/act-integration/scripts/verify.mjs:219`).
So the 23-entry release ledger is correct, and `databaseLedgers: 23` in the audit
snapshot stands.

**Why it still matters for R00:** the direction that is unguarded is exactly the
direction a future ledger would drift in. A new `-pg` package whose schema is
forgotten from `databaseLedgers` would package and validate cleanly while its RLS
never reaches the release map. That is a silent authority gap, not a visible one.

**Corrected expectation:** the Rust host's release validation must reject in both
directions — an unowned registered ledger _and_ an unregistered migration owner —
with an explicit, reviewed exemption list rather than an implicit one.

**Affected gate:** G02 Contracts, G03 Authority, G09 Release identity.
**Status:** `specified` — required before any Rust slice is accepted.

## D09 — Axum's default JSON rejection reflects the caller's input

**Found at R01 while writing the G03 negative tests, in code that had not shipped.**

The G03 tests drive a hostile `CommandSubmission` through the real router. One of
them — `a_boundary_refusal_does_not_reflect_the_submission` — asserted the
refusal does not echo the rejected `tenant`. It failed, and not because the
authority gate leaked: the request never reached the gate. Axum's built-in
`Json` extractor rejects a malformed body _before_ the handler runs, and its
default rejection body echoes the offending value verbatim. A submission
carrying `"tenant": "../etc/passwd"` came back as:

```json
{ "tenantId": "tenantId must be a lowercase kebab-case slug, got \"../etc/passwd\"" }
```

**Why this is worse than a normal 400.** Three properties compound:

1. The route is unauthenticated — there is no session required to reach it.
2. The reflected value is attacker-controlled, so it is an oracle for how the
   field is parsed. Vary the input and the response's phrasing changes; that
   narrows the vocabulary an attacker has to guess.
3. It names the field. A caller that sends `role` and gets a `roleId` error has
   confirmed the schema, on a route whose entire purpose is to refuse.

The baseline's own rule is that a refusal must not confirm a tenant's existence
to a caller not entitled to it. A reflection of the tenant string is the same
class of disclosure arriving through a different door.

**Fix applied** (`rust/crates/host/src/router.rs`): a `rejection()` handler
replaces the default. The detail is logged server-side via `tracing::warn!`,
because an operator debugging a broken client needs it; the caller gets
`{"decision":"rejected","reason":"malformed_submission","message":"The
submission could not be processed."}` — a fixed string with no part of its own
input in it, and one deliberately identical across a bad tenant, a bad role and
a bad tier, so the response cannot be used to tell which field is wrong.

**What this cost, and what it bought.** The bug was found _only_ because the
test went through `router.oneshot` rather than calling `CommandAuthorizer`
directly. The authority policy was correct the whole time; the defect lived
entirely in the extraction layer above it. A policy-level test suite — which is
what "62 green tests" was before this — cannot see this class of bug at all.
That is a concrete, measured instance of why G03 says _real entrypoints_.

**Corrected expectation for R02:** every extraction path needs the same
treatment, not just `Json`. `Path`, `Query` and `Form` extractors have
equivalent default rejections. `Path` is the dangerous one — it reflects a URL
path segment, and the R02 routes are tenant-scoped by path. Any handler taking
`Path<TenantId>` must not let a rejected segment back out.

**Affected gate:** G03 Authority.
**Status:** `Json` fixed at R01. The `Path`/`Query`/`Form` follow-up turned out not
to be needed by R02, and is now guarded rather than left open by prose.

**Follow-up closed, and how.** R02 shipped without a handler that failed to
reflect — not because anyone wrote the handler, but because R02 never used the
extractor. The tenant still arrives in `CommandSubmission.tenant` (a `TenantId`,
so an invalid slug fails during deserialization, before any handler runs), and
the router declares four static routes with no path parameter anywhere:
`/healthz`, `/login`, `/contacts`, `/commands`. So there is no `Path`, `Query` or
`Form` extractor in the tree to leak through, and D09's concern about R02's
tenant-scoped paths is moot for R02.

That is a vacuous close, and the distinction is worth stating, because a vacuous
close is exactly what decays: the first tenant-scoped route someone adds
(`/tenants/{tenant}/contacts`) introduces the extractor, and the reflection comes
with it silently — the route would still work, and nothing would fail. This is
now guarded rather than merely documented:

- `rust/crates/host/tests/extractors.rs` reads the router's own source with
  `include_str!`. Two assertions hold that no `Path`, `Query` or `Form`
  extractor is wired without a refusal handler, matching the existing `Json`
  shape: fixed message, detail logged server-side, no part of the caller's input
  in the response. Both spellings are matched — the bare type in a handler
  signature (`Path<`) and the fully-qualified one (`extract::Path`) — because a
  grep-shaped guard that matches only the first is walked past by the second.
- It also asserts behaviorally that an unmatched path is **not** reflected —
  four hostile inputs (`../etc/passwd`, a tenant-slug path, a
  percent-encoded-dot path, a percent-encoded script tag) all return the
  router's own plain 404 with none of the path echoed back. That is the property
  a caller can observe today, independent of the source-reading assertion.
- The source-reading guard fails _loudly and on purpose_ — its message is the
  instruction for whoever adds the route.

The addon is counted separately from the gate it guards: it added 2 tests, and
`cargo clippy -- -D warnings` is clean. The workspace count is not repeated here
on purpose — the two assembly and capacity slices each landed afterwards, so any
number written into this line goes stale the moment another crate is promoted.
The authoritative count is the gate's own output.

## D10 — A promoted crate's tests reached outside its own tree

**Found when promoting `archava-pricing` at R02, on code that already passed
56/56 tests in the pilot.**

Two `#[cfg(test)]` modules in the pilot's pricing crate embedded their pricebook
with:

```rust
const PRICEBOOK: &str = include_str!("../../../fixtures/pricing.v1.json");
```

`include_str!` resolves relative to the _source file_, so that path climbs out of
`crates/pricing/src/` three levels to a `fixtures/` directory at the pilot root.
It compiled and passed there. In the product tree the same relative climb lands
on a directory that does not exist, and the crate fails to compile at all —
which is how it was found.

**The fix was not to copy the fixture in.** `config/pricing.v1.json` already
exists in the product tree and is git-tracked; it is byte-identical to the
pilot's copy, and its sha256 — `2499bc82220188dad4daff276f73159ddcff9233944169f
df8f0dcc3f14f7831` — matches the `pricebookSha256` recorded in the pilot's own
parity results. So the pilot's fixture was a duplicate of a tracked file, and
copying it forward would have created a second copy of the pricebook free to
drift from the first. The tests now `include_str!` the tracked file directly,
which means the parity hash in the results and the bytes under test are provably
the same object.

**Why this generalizes past one crate.** A crate whose tests read a path outside
its own directory is a crate that cannot be moved, vendored, or published — the
green test run is evidence about a directory layout, not about the crate. The
pilot's 56 passing tests were real; they were real _of the pilot tree_. The
re-verification ADR-R06 requires is what turned a passing suite into a broken
build, and that is precisely the failure mode the rule exists to catch: had the
crates been promoted by copying the pilot's result rather than re-running it,
this would have shipped as a build break.

**Corrected expectation for R02:** every promoted crate must compile and test
from its new location before any of the pilot's evidence is cited for it. Test
fixtures come from tracked product files, never from a copy made for the
experiment.

**Affected gate:** G02 Contracts, G09 Release identity.
**Status:** fixed at R02 promotion.

## D11 — Assembly planning is validated by two independent gates

**Baseline:** `packages/config/src/assembly.ts`.

**Measured:** the release catalogue's assembly contract and host settings are
checked in two places that do not share code, do not share an error class, and
do not share a caller. `planAssembly` validates the contract version against
`MODULE_CONTRACTS` and the host settings against its own recorded minimum; the
packaging path performs a second, differently-spelled check over the same
fields. Neither is a wrapper of the other and neither can be reached by making
the other pass.

**Corrected expectation:** the port reproduces both gates, in
`archava-assembly`. A future caller that wires only the planner into a pipeline
would be one gate short of the baseline, which is why the second is _not_
treated as redundant in the Rust crate even though it is the same field.

**Affected gate:** G02 Contracts, G05 Release identity.
**Status:** `locally_proven` — 31 integration tests, both gates exercised
independently.

**Why this is recorded rather than deduplicated:** the obvious refactor is to
let the planner own validation and have the packager trust it. That is a
behaviour _change_ to the product, made in the name of tidiness, and it is
exactly the kind of thing a port is not allowed to decide on the baseline's
behalf. The two gates are an enforced pair here so the next person does not
"clean up" one of them away.

## D12 — A plan-time prerequisite throw is provably dead code

**Baseline:** `planAssembly` throws when a selected module's declared
prerequisites are not all present in the same selection.

**Measured:** every path that can reach that throw has already been rejected
earlier by the precedence check, which refuses a module listed as a
prerequisite of a module that is _not_ selected before the prerequisite loop
runs. So the throw is unreachable with any input, and no test in
`packages/config/test/assembly.test.ts` reaches it either.

**Corrected expectation:** the port carries the throw, unreachable, and says so
in the code rather than omitting it. Deleting it would be a false claim that the
baseline has no such rule; keeping it without the note would be dead code that
reads as live.

**Affected gate:** G02 Contracts.
**Status:** `locally_proven` — reachability argued in the port, and the
argument is in `crates/assembly/src/plan.rs` where a future edit can break it.

**Not a licence to change the baseline:** the dead throw is a baseline property.
Removing it from the _product_ is a separate decision the owner has not asked
for, and this port is not the place to make it.

## D13 — The capacity math's `insufficient_capacity` carries one code where the baseline declares eight

**Baseline:** `packages/capacity/src/contracts.ts:3-19` declares `CapacityError`
with eight codes. `calculateSchedule` reaches exactly one of them.

**Measured:** `insufficient_capacity`, thrown twice — once at the entry
(`repository.ts:124`, when `weeklyCapacity / 5` is not positive) and once at the
exit (`repository.ts:209`, when the 1000-iteration loop gives up). The other
seven are thrown by the repository, service and API layers
(`repository.ts:70,297,319,340,405,436,440,477,501`, `service.ts:44,53`,
`tools.ts:29`), none of which is scheduling math.

**Corrected expectation:** `CapacityErrorKind` models one variant. When this
port grows a caller for those paths, the variants arrive with the code that
throws them — not before.

**Affected gate:** G02 Contracts.
**Status:** `locally_proven` — the single variant is `PartialEq` and its message
is its code, which is what `super(message ?? code)` makes the TypeScript
error's message.

**Not a licence to widen the enum early:** an enum member no code path
constructs is a claim the port decided something it did not decide. The other
seven are recorded here as _known and deliberately absent_, not as oversight.

## D14 — The capacity reference file is not byte-reproducible, by one case

**Baseline:** `packages/capacity-pg/src/repository.ts:117-231`, through
`calculateSchedule`.

**Measured:** of 37 fixtures, exactly one records `desired: null`. With no
desired start the baseline reads `new Date()`, so that case's recorded
timestamps are the moment the generator ran. The other 36 are fixed by their
fixtures and byte-stable across regenerations.

**Corrected expectation:** `rust/crates/capacity/tests/reference.json` is
_almost_ deterministic, and the one case that is not is named in the generator's
header, excluded from the comparison loop in `tests/capacity.rs`, and compared
by a test of its own that checks the fields which do not depend on the clock
plus replays the recorded instant as an explicit desired start. No tolerance is
used, because a tolerance would hide a real divergence in the other 36.

**Affected gate:** G02 Contracts, G03 Data integrity.
**Status:** `locally_proven`.

**Not a licence to fudge the rest:** the temptation is to add a tolerance to the
whole suite so the wall-clock case fits alongside the others. That would make 36
exact comparisons into 36 approximate ones to accommodate one imprecise
fixture, which is the wrong trade and is why the suite is split instead.

## D15 — The assistant's instants render microseconds from the DB clock; the baseline renders milliseconds from the process clock

**Found at R03 while porting the CRM conversation surface.**

**Baseline:** `apps/crm/src/host.ts` stamps `new Date().toISOString()`
(millisecond precision, process clock) for the assistant turn's `occurredAt`,
and `packages/crm/src/contracts.ts` renders section instants through JS `Date`
serialization.

**Port:** `rust/crates/host/src/assistant.rs` stamps every assistant instant
through the database clock -
`to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')` - which
gives microsecond precision exactly like the CRM command lane's
`CommandEnvelope.receivedAt`. Two consequences, both deliberate:

1. **A superset, not a divergence, on input:** the flow's instant parser
   accepts baseline-shaped millisecond strings (the form the replayed
   conversation bodies carry) in addition to its own microseconds.
2. **Clock authority, not convenience:** `occurredAt` comes from the serving
   transaction's `now()`, so two concurrent turns cannot disagree about whose
   worklist boundary is "now" merely because the processes' clocks skid. The
   baseline derives the same value from the process clock; where the two
   values are compared side by side they differ in fractional-second digits
   and origin, not in meaning.

**Planner lane:** the baseline picks `plannerFor(...)` and may select a
Gemini-backed planner; the R03 port serves `LocalCrmPlanner` only — the same
implementation the baseline falls back to when the provider lane is not
configured. This is a provider decision recorded in the module header, not a
CRM contract difference.

**Affected gate:** G03 Authority, G05 Assistant surface.
**Status:** `locally_proven` (the flow over the canned host and the HTTP suite
assert the recorded values end to end).

**Not a licence to widen precision elsewhere:** no other payload gains digits
"for consistency"; instants keep the precision their surface recorded until
that surface is itself ported.

