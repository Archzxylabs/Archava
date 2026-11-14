# Ava business agent runtime

Ava connects a business goal to authorized context, a reviewed plan, the existing
Cockpit mission executor, and a receipt-backed result. Home submits the goal to
Desk; Desk restores personal conversations and pending goals from PostgreSQL.
The older CRM command shortcuts remain available as a separate narrow interface.
State reads return the 30 most recent personal threads and the 20 most recent
turns/goals in the selected thread; planner history uses the latest eight turns.
These are bounded working views, not an unlimited history search interface.

## Planning and execution

`POST /api/agent/turns` accepts a tenant, original request UUID, optional thread
and goal UUID, and an utterance. A registered action with typed values can also
be supplied from the native action editor. HTTP session, CSRF, tenant membership
and domain reads establish authority. Body/model role claims cannot supply it.
The host reads bounded context from the 19 mounted business areas and records
the membership/configuration fingerprint. Scheduling has its own route.

The optional Gemini GenerateContent planner uses `GEMINI_AGENT_MODEL` and
`GEMINI_AGENT_API_KEY` (fallback `GEMINI_API_KEY`). Select a model available to
the deployment account; startup never guesses one. The endpoint defaults to
Google; only that HTTPS host or owned loopback fixtures are allowed. The transport
uses the [official GenerateContent REST contract](https://ai.google.dev/api/generate-content)
and [structured output](https://ai.google.dev/gemini-api/docs/structured-output).
Provider requests require a current, explicitly reviewed Owner sharing policy
for that provider/account profile. Current messages must be selected. The host
projects only approved areas, data categories and company/workspace scope from
records the current user can read. Empty company selection never means all
companies. Raw conversation history, saved preference text and prior goal
objectives/outcome text and every historical action input/record ID are withheld.
Selected business facts can include registered tool/index metadata and per-step
completion flags requiring a matching approval, mission and verified checkpoint.
Fresh authorized reads supply business values and targets. Complete safe
historical memory remains separate work. Typed native
actions stay local. Without a usable sharing policy or configured
model, local planning offers grounded context/clarification, actual stock-driven
replenishment proposals and explicit action forms. It does not claim general
natural-language reasoning for all business tasks.

`GEMINI_AGENT_THINKING_LEVEL` optionally selects `minimal`, `low`, `medium` or
`high` for Gemini 3 text models that support that level. Leave it unset for
provider defaults and Gemini 2.5. The live browser profile uses
`gemini-3-flash-preview` with `minimal`; its default high reasoning exceeded the
interactive timeout on an adversarial approval request. These are explicit
inference settings, never an authority or approval setting.

Provider proposals use typed JSON input objects for each exact registered tool,
with generated request keys omitted. The decoding envelope describes each
registered field once with explicit nested types; the input retains the exact
contract for each tool. The host checks the selected tool's required fields
(including reviewed defaults), rejects extra fields and validates the domain
envelope. Shared decoding fields never grant another tool's inputs or authority.
Provider scalar types are checked before form-envelope construction; native
form coercion remains local. This is selected-contract validation, not a general
implementation of every JSON Schema keyword.
Counting bounds are enforced by host/domain validation. Missing-input questions
discard any accompanying draft actions and return clarification only.

One malformed binding, dependency or selected-contract shape can use the
remaining planning round for a format correction. The decoder narrows to
selected registered tools, using exact closed branches for small contracts;
larger selections keep the bounded shared-field grammar. Only installed public
contracts, bounded structural metadata and fixed guidance are sent; failed
proposal text, field values and arbitrary keys are withheld. Correction
and read expansion share the total three-call budget, including optional model
routing. At most one format correction is allowed. Current sharing and
authority are rechecked before and after every call. No correction executes or
approves work. See the [real-provider finding](../product/PRD_GEMINI_SCHEMA_FINDING_2026-11-09.md)
for observed rejected schemas, malformed bindings and validation limits.
The [historical context finding](../product/PRD_PRIOR_CONTEXT_FINDING_2026-11-09.md)
records the actual row-scope counterexample and the metadata-only correction.

Each provider-backed turn allows at most three total calls. A model routing call
consumes one slot; local routing leaves up to three planning rounds for distinct
search/detail reads and a final plan. Model input states the current round and
whether another read can be requested. A lookup requires a later planning slot;
an expansion adding no new query is refused. Limits remain six turns per actor/tenant
per minute, four concurrent transport calls per host, twenty seconds per call,
128 KiB total serialized input and 1 MiB response. Plans allow six context modules, 25 rows per
collection, 32 KiB per source, 12 steps and eight clarification questions.
Model output is untrusted: registered tools/fields only, no provider-only actions,
no invented UUID references to existing records, no mixed unanswered questions
and executable steps, and only earlier explicit dependencies. Native action
editors reuse the installed domain contracts. Domain handlers validate and
reauthorize the actual mutation regardless of planning checks.

Plans and their conversation response commit atomically. The plan hash binds
actor, tenant, goal, objective, context and inputs. Reusing an original request
with different content is refused. `GET /api/agent/state?requestId=...` distinguishes
an in-flight request, a committed response and a definitively uncommitted turn.
A failed planning attempt does not authorize business work. After an uncommitted
provider attempt, use a new request; the original provider budget is retained.
Provider failures expose safe categories and, where available, HTTP status;
upstream bodies, credentials and prompt values are not included in the error.
Model format errors may expose known contract names and bounded structural
facts, never rejected field values. Manual registered actions remain available.

`POST /api/agent/approve` requires explicit confirmation, the exact plan hash and
current write capability. It creates/reuses an existing Cockpit mission with
stable domain idempotency keys and its usual approval gates. L4/L5 independent
approval remains required. Approval of a plan is not evidence that it completed.
Only actual registered business receipts and committed mission checkpoints count
as completed steps. A lost checkpoint recovers the original domain key first.

Output bindings accept only IDs/revisions from earlier passed, verified business
receipts, with explicit dependencies. Prices, quantities and executable templates
cannot be bound. Source revisions/costs/stock are checked again before each new
effect. Changed facts stop the goal for a new review. Replanning creates an
immutable child goal, stops remaining old work and requires fresh approval.
Already completed receipts are retained; the model is instructed not to repeat
them. The operator must review a revised plan for business intent.

## Continuation and current access

The native server owns a worker that scans durable routing keys and serializes
execution by goal. Each drive is bounded to 24 approval/execution iterations.
The worker carries an internal task-local execution context linked to the
original active human session hash; it stores no bearer token and exposes no
HTTP header that creates this context. Logout, expiry, user version, membership,
configuration and domain grants remain effective. It does not refresh the
parent session's idle timeout. An expired/revoked session requires a human to
resume the reviewed goal with a current session; changed context requires a new
plan. Transient domain failures retry the original idempotency key.

`/api/agent/control` supports confirmed pause/resume/cancel. Conversational
control requests only open the current goal for review; quoted/negated text
cannot mutate it or authorize continuation. Use an explicit control with the
matching plan hash and confirmation to change work. It serializes with
execution and refuses to abandon a committed but unreconciled original effect.
Cancellation stops remaining work, not historical business records. A completed
goal keeps its receipt-backed result. Worker shutdown is joined by the native
server; durable queued work can resume in a fresh process.

Owner settings select business areas, categories and named companies through a
signed review with a ten-minute lifetime. Apply and restore recheck the live
provider and company access. Revocation stops future model rounds, including
lookups after a response already in flight; previously sent data cannot be
recalled. Provider retention/residency/training terms are not verified by this
runtime. This is field/scope projection, not semantic classification of arbitrary
user messages or free-text notes.

`owner-pg/004_ai_disclosure_boundary.sql` advances the immutable migration profile
and requires the matching boundary function at startup. Old distributions must
not serve a database after this profile upgrade. Rollback uses the separately
restored, pre-upgrade backup with its matching old binary.

`owner-pg/003_agent_runtime.sql` adds FORCE RLS conversations, preferences, goals,
turns, model attempts and work routing. Conversation reads are additionally
restricted to the human principal. Changed grants/configuration redact historical
derived business context. The worker-routing policy deliberately exposes only
`(tenant_id, goal_id)` when a transaction-local internal worker flag is set;
business/goal contents still require strict tenant RLS. The flag does not grant
business authority. The non-owner runtime role has only the declared table
grants. SQL changes remain append-only and registered in the manifest, server
and fixture.

## Voice and acceptance limits

“Plan from my spoken request” transfers the user's captured input transcription
into the same Desk turn, stops media and requires the same business review.
Model audio/output transcription cannot approve business work. This is an
explicit handoff, not an autonomous voice tool-call protocol.

Owned HTTP/WebRTC fixtures certify transport and host enforcement, not live-model
reasoning quality, transcription accuracy or provider account availability.
Public deployment remains separate. See the [implementation record](../product/AGENTIC_IMPLEMENTATION_WORK.md)
and the dated acceptance report linked there for the commands and outcomes
actually observed.

Live GenerateContent acceptance is explicitly selected through
`tools/e2e/run-agent-live.mjs`; see [the tool instructions](../../tools/README.md#live-gemini-acceptance).
It uses a private credential file only in memory, a chosen text model, Google's
canonical HTTPS endpoint, synthetic business data and an owned browser/database.
A legacy model restricted to realtime audio cannot serve this text planner.
Six natural-language turns cover procurement, follow-up, CRM, clarification,
approval enforcement and grounded reads. This bounded sample does not establish
quality on every business goal or live voice/transcription acceptance.

A new trade goal can explicitly select `companyId`. Follow-ups inherit the saved
company scope; conflicting input refuses. Trade context and later guard reads are
scoped, Organizations access joins the approval fingerprint, and company-specific
creation actions retain their reviewed company version. See [company trade](COMPANY_TRADE.md)
for the authority, legacy-scope and bounded-planner boundaries.

[Supplier document contracts](SUPPLIER_DOCUMENTS.md) are registered Purchasing actions for reviewed unposted claims and accepted-goods checks; model text cannot imply AP/GL posting or supplier payment.
# Bounded, authorized retrieval

The initial provider context contains at most 25 rows per collection and 32 KB per source. Collection coverage states the page size, included rows, whether more rows exist or may exist, and whether the collection is complete. An initial page is not a workspace total. The byte allowance applies to serialized UTF-8 data. If combined authorized pages exceed it, the host removes complete unselected rows from the largest collection and updates included counts, partial-collection coverage and `byteBudget`; it does not shorten record fields or increase the allowance. Selected-company records and exact lookup references are retained. Oversized required records or metadata still refuse explicitly instead of appearing as fully reviewed data.

During planning Ava can request up to eight distinct typed lookups within the
remaining total provider-call slots. A lookup names an available module/collection
plus either a literal search or an exact reference. It cannot provide a URL, SQL,
principal or permission. Exact references must be grounded in supplied or observed
records; their presence permits a scoped read, not access to the record. An
already grounded ID is rechecked through the authenticated endpoint and withheld
if current access is lost. Query text and historical identifiers are not reflected
into provider coverage. Included counts describe projected approved collections.

The response schema follows the same budget. While a later call remains and
fewer than eight reads were accepted, the schema offers the `lookups` field. On
the last call the budget allows it is removed, so the provider is constrained
to return a plan or questions rather than only told in `readExpansion` that no
read can follow. A provider that ignores its schema and still asks for a read
is refused with `agent_lookup_budget_exceeded` as before; no budget changed.

Before each later planning round, the host refreshes all authorized sources and
the accumulated typed lookups. Previously read queries may be refreshed along
with a new distinct query; a provider request consisting only of duplicate reads
is not served again: the turn ends with `agent_lookup_budget_exceeded` instead
of spending another planning slot on it. Membership/configuration changes stop
the turn, and row-level reassignment is reflected in the fresh reads. This avoids
resending an unrelated stale source while expanding another business area.

Review refreshes approved target IDs directly, including targets outside the first page, using current authority. Unrelated newer records do not invalidate a review merely by displacing its target from the initial page. A real target revision change still requires review again. Refresh is bounded to 64 target queries/rows and never calls the model. Planning results include lookup coverage; a page lookup does not establish a complete-filter aggregate or unlimited retrieval. Totals come from aggregate lookups (below), which are a separate kind of read with their own evidence. Lookup coverage distinguishes matching rows from included rows, including byte-limited results; current exact-ID guard refresh uses the same protected-record rule.

The database-backed scenarios in `crates/host/tests/agent_runtime/lookups.rs` cover an older product beyond 100 initial records, literal special-character search, an unrelated new row, actual target changes, forbidden company references, invalid model lookups and Support scope. A populated Unicode organization workspace beyond the byte limit also exercises bounded planning, an exact target outside that view, current review guards and one-effect replay. The held-out PRD evaluation remains separate acceptance work.

# Aggregate lookups

A page lookup returns bounded rows, so it cannot answer "how much do customers still owe us this month, by customer?" when there are more rows than the page. An aggregate lookup can: it is a count or a sum over every row of one collection that matches its filters, computed by one SQL statement in the adapters.

## What may be asked

The list is closed and is data: `agent.aggregates` in [`config/modules.v1.json`](../../config/modules.v1.json) (see [the module registry](../architecture/MODULE_REGISTRY.md)). It lives there because the registry already declares, per module, which collections the provider projection may read; an aggregate entry must name one of those collections. Each entry names its sum fields (a count is always offered), its group-by dimensions with the data category of their labels, its filters and the date they read.

| Module, collection | Sum of | Group by | Filters | Date |
| --- | --- | --- | --- | --- |
| `billing` `invoices` | `gross`, `outstanding`, `tax` | `status`, `customer`, `month` | `status`, `dateFrom`, `dateTo`, `companyId` | issue date |
| `commerce` `orders` | `total` | `state`, `customer`, `month` | `state`, `dateFrom`, `dateTo`, `companyId` | creation date |
| `purchasing` `orders` | `total` | `state`, `supplier`, `month` | `state`, `dateFrom`, `dateTo`, `companyId` | creation date |

The registry holds names only. The SQL behind each name is fixed in [`crates/adapters/src/erp/aggregates.rs`](../../crates/adapters/src/erp/aggregates.rs); a request is served only when the registry lists the name and that file implements it, and a unit test fails when the two differ. No model or request value becomes SQL text: filter values are bound parameters, the state filter accepts only the collection's own states, and dates must be real `YYYY-MM-DD` days.

The planner requests one as a lookup item, in the same `lookups` array and under the same budget as page lookups (eight distinct reads per turn, an identical read is not served twice, and the last call's schema offers no lookup of either kind):

```json
{"module":"billing","collection":"invoices","aggregate":{"measure":"sum","field":"outstanding","groupBy":"customer","filters":{"status":"issued","dateFrom":"2026-11-01","dateTo":"2026-11-30"}}}
```

`readExpansion.aggregates` in the planning input lists what this turn may ask (module, collection, `measures`, `sumFields`, `groupBy`, `filters`, `filterValues`, `dateField`), already reduced to what the sharing selection permits. The result arrives in `authorizedContext.aggregateResults` on the next planning round. Aggregates are recomputed on every later round, like page reads.

## Scope: the rows of the page read, all of them

The adapter resolves access with the resolver the collection's page read uses and applies the same predicates to the whole set:

- the module is enabled, and the caller holds its read permission (`billing.read`; `orders.read` in Commerce or Purchasing);
- the membership scope: `all`, or only rows the caller owns (an `assigned` Commerce scope narrows Purchasing too, as in the page read);
- company access: a row without a company, or a row of a company the caller can read through `companies.read` within the Organizations scope — the rule `company_trade::resolve` applies before a company page read;
- the tenant, by FORCE RLS and an explicit predicate; Commerce orders managed by Foodservice are excluded, as in the page read.

A page read shows one company (or the records without a company) at a time. An aggregate may span the companies the caller can read; the host narrows that to the sharing selection (below) and never widens it. A turn scoped to a company counts that company only, and a `companyId` filter naming another company is refused.

The statement runs under `statement_timeout='10s'`. A time-out produces a refusal and no figure.

## Money and units

A sum is never taken across currencies. Every sum is grouped by the record's currency first, and each currency is its own section with its unit stated: `whole IDR` for IDR records, `minor units of <CUR>` for any other currency ([trade currency](../architecture/MULTI_CURRENCY.md)), with the amount as the stored integer and as text from `trade_money::format_trade`. There is no cross-currency total. A count has one section without a currency.

## Bounds

At most 50 groups are returned per request, largest measure first. With several currencies the 50 are divided equally between the sections (25 each for two). The remaining groups of a section are folded into `other` with their number, their rows and their combined amount, so each section's total stays exact. More than 50 currencies is refused. `customer` groups by the recorded party or contact where the record has one and by the customer text as written otherwise; the label is the most recent name on those rows. A group whose dimension has no value (the issue month of an invoice that was never issued) has a `null` label.

## The result is evidence

```json
{
  "kind": "aggregate", "module": "billing", "collection": "invoices",
  "measure": {"kind": "sum", "field": "gross"}, "groupBy": "month",
  "filter": {"dateField": "issued", "dateFrom": null, "dateTo": null, "calendar": "UTC calendar dates, both bounds inclusive", "companyId": "c62b4c0f-…"},
  "scope": {"companies": "selected company", "recordsWithoutCompany": false, "ownRecordsOnly": false},
  "rowsCovered": 34, "complete": true, "incompleteReason": null,
  "observedAt": "2026-11-14T16:56:20Z", "groupLimit": 50,
  "sections": [{
    "currency": "USD", "unit": "minor units of USD", "rows": 34,
    "total": 1465075, "totalFormatted": "USD 14650.75", "groupCount": 2,
    "groups": [
      {"label": "2026-11", "rows": 30, "amount": 1257465, "formatted": "USD 12574.65"},
      {"label": "2026-10", "rows": 4, "amount": 207610, "formatted": "USD 2076.10"}
    ],
    "other": null
  }],
  "coverage": "Billing invoices: sum of gross by month over all 34 matching rows (no state or date filter); scope: records of the selected company; read at 2026-11-14T16:56:20Z."
}
```

`filter` is the filter as applied. Dates are UTC calendar dates with both bounds inclusive: there is no tenant time zone. `rowsCovered` is the whole filtered set within `scope`, and `complete` is `true` for every result that carries figures; anything that could not be computed completely is a refusal instead. `scope` says what "all" means here: which companies, whether records without a company count, and whether the caller sees only their own records. The reply to the person appends each result's `coverage` sentence under "Evidence (computed by Archava, not by the model)", so the answer states what it covers whatever the model wrote. The page-lookup evidence (`lookupResults`) is unchanged and stays under its own key.

## Refusals

A request outside the list, or one the access or the sharing selection does not permit, is answered to the planner as `{"kind":"aggregate","refused":true,"reason":…,"message":…}` in place of a result. It carries no figure, and echoes only registry vocabulary (a known module id and one of its collections), never a value from the request. Reasons: `aggregate_module_unavailable`, `aggregate_collection_not_listed`, `aggregate_measure_not_listed`, `aggregate_dimension_not_listed`, `aggregate_filter_not_listed`, `aggregate_filter_invalid`, `aggregate_module_not_shared`, `aggregate_data_class_not_shared` (with `dataClass`), `aggregate_company_out_of_scope`, `aggregate_company_not_shared`, `aggregate_company_not_readable`, `aggregate_not_permitted`, `aggregate_too_many_currencies`, `aggregate_timed_out`. A lookup that is structurally not an aggregate (an unknown member, or an aggregate combined with a search or an id) fails the turn with `invalid_agent_lookup`, like any malformed lookup.

## What reaches the provider

An aggregate discloses derived business data, so it follows the Owner sharing selection by the rule that governs record fields:

- the module must be shared;
- every part of the result must belong to a shared data category: a count, a state, a currency and a date are *Statuses, quantities and dates* (`business_facts`), required by every aggregate; a sum is *Prices, costs and financial amounts* (`financial_amounts`); group labels carry the category their dimension declares in the registry — customer and supplier names are *Names, titles and business codes* (`names`). A dimension whose category is not shared is refused; nothing is replaced by opaque labels;
- only approved companies count: the turn's company, or otherwise the approved companies the user can still read, plus records without a company (a turn without a company reaches the provider only when those are shared). Empty company selection never means all companies. No count of rows outside the selection is computed or sent.

The check happens before the statement runs, and again when the planning input is projected. A provider request therefore contains, for aggregates: the offer list in `readExpansion.aggregates`; and in `authorizedContext.aggregateResults` either the result object shown above or a refusal. The result's `filter.companyId` is replaced by `companyFilterApplied` unless *Record references* are shared. Nothing else is added: no row, no identifier of a grouped party, no label beyond the returned groups.

Two differences from page reads are deliberate and worth knowing. Page projection withholds any field whose name is not classified, which today includes an invoice's `customer`, `gross` and `outstanding`; the aggregate entries classify the corresponding sums and labels explicitly, so an owner who shares names and amounts for Billing now discloses invoice totals and customer names through aggregates that the page rows do not carry. And an aggregate can cover several approved companies in one result, where a page covers one.

## Limits

- Three collections; no join, no ratio, no average, no free date expression, no relative period ("this month" is the model's arithmetic from `observedAt`; the result states the dates actually applied).
- Dates are UTC calendar dates.
- Results are not trimmed to fit the 128 KiB planning input; a turn whose aggregates and records together exceed it is refused with `agent_context_too_large`.
- Local planning (no provider or no usable sharing selection) does not run aggregates.
- Owned fixtures prove the host's enforcement and the request bytes. Whether a live model chooses an aggregate for a totals question, and reports its coverage faithfully in its own words, is not established by these tests.

Tests: `crates/host/tests/agent_runtime/aggregates.rs` (exactness against direct SQL beyond a page and beyond 50 groups, two currencies, scope and RLS, the allowlist, the recorded provider requests under four sharing selections, the lookup budget), unit tests in `crates/adapters/src/erp/aggregates.rs`, `crates/host/src/agent/aggregate.rs` and `crates/host/src/agent/disclosure/tests.rs`.

# Amount units

A trade record's integer amounts are whole Rupiah when its currency is IDR and the currency's ISO minor unit otherwise ([trade currency](../architecture/MULTI_CURRENCY.md), rule 4). The planner is told, at two levels:

- every money field of the Commerce, Purchasing, supplier-document and Billing actions carries the unit in its description in the form catalogue (`config/operator-actions.v1.json`), and therefore in `actionContracts` and in the decoding schema;
- when a company is in scope, `authorizedContext.tradeCurrency` states that company's trade currency (its book's, IDR without a book), the unit (`whole_idr` or `minor`), the fraction digits and one worked example (`the integer 1999 is USD 19.99`). It is a business fact: it is sent only when *Statuses, quantities and dates* are shared. Without a company in scope there is no company currency to state; the general rule is in the instructions.

This reduces wrong proposals. It is not a guard: a proposal is reviewed by a person before it runs, and the domain still validates the amount it is given.

# Refused steps

When an approved step is refused by a business rule, the module answers 409, with `detail: {code, message, messageId}` when the refusal is catalogued ([refusal reasons](../architecture/REFUSAL_REASONS.md)). The goal stops in `needs_review` as before — no retry and no automatic replan — and now keeps the reason:

- `failure` is `Step <n> (<action label>) was refused: <catalogued English sentence> Nothing was changed by this step; review the plan before continuing.`;
- `failureDetail` on the goal is `{stepIndex, tool, action, code, message, messageId}` with the stable code and both catalogue sentences;
- the conversational report (`agent_report`) prints `failure` and carries the same object as `data.refusal`.

Only the catalogue's own sentences are stored. The runner takes the code from the module's answer and looks the entry up in the embedded catalogue (`refusals::for_code`); the answer's text is never copied, so a database message cannot reach the goal or the report. The step is named by its registered action label, not by the model-written title. An uncatalogued refusal keeps the fixed sentence ("The current domain rules refused this step. …") and has no `failureDetail`.

There is no column for the reason: it is stored in the goal's mission snapshot (`outcome.agentRefusal`) beside `failure`, moved to `failureDetail` when the goal is read, and no longer reported once a new approval clears the failure. Tests: `crates/host/tests/agent_runtime/failures.rs`.

