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

Review refreshes approved target IDs directly, including targets outside the first page, using current authority. Unrelated newer records do not invalidate a review merely by displacing its target from the initial page. A real target revision change still requires review again. Refresh is bounded to 64 target queries/rows and never calls the model. Planning results include lookup coverage; they do not establish complete-filter aggregates or unlimited retrieval. Lookup coverage distinguishes matching rows from included rows, including byte-limited results; current exact-ID guard refresh uses the same protected-record rule.

The database-backed scenarios in `crates/host/tests/agent_runtime/lookups.rs` cover an older product beyond 100 initial records, literal special-character search, an unrelated new row, actual target changes, forbidden company references, invalid model lookups and Support scope. A populated Unicode organization workspace beyond the byte limit also exercises bounded planning, an exact target outside that view, current review guards and one-effect replay. Broader analytical aggregates and the held-out PRD evaluation remain separate acceptance work.

