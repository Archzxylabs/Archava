# Owner acceptance test map (OWN-T01..T10)

Which executed tests cover each clause of the owner acceptance criteria in [OWNER_CONFIGURATION_PRD.md](OWNER_CONFIGURATION_PRD.md) §9, and which clauses no test covers yet. A test counts for a clause only when its body asserts it. This is a coverage map, not an acceptance record: no criterion is accepted here, and statuses live in [PRD_INHERITED_TRACEABILITY.v1.json](PRD_INHERITED_TRACEABILITY.v1.json).

## Tests added after the analysis (2026-11-13)

The analysis below was written by reading test bodies before these tests existed; where it says NOT COVERED for one of these clauses, this table supersedes it.

| Test | Closes |
| --- | --- |
| `crates/host/tests/own.rs::disabling_a_required_module_is_blocked_and_nothing_is_written` | T05/T07: missing-dependency block over HTTP on the installed catalogue (plan, chat, `plan_blocked` apply, zero rows), for every requirement the module registry declares |
| `crates/host/tests/own.rs::a_never_granted_or_revoked_principal_is_refused_on_every_owner_route` | T04: never-granted and revoked principals on config, history, status, plan, chat, apply, restore preview and restore |
| `crates/host/tests/own.rs::an_unauthenticated_caller_is_refused_on_every_owner_route` | T04: missing and forged bearer on the same eight routes |
| `crates/host/tests/own.rs::restore_keeps_historical_labels_traceable_and_only_appends` | T08: per-revision labels, request id, plan hash and outcome in `/own/history` after a restore; earlier revisions, receipts and audit rows byte-identical; grants, CRM memberships and contacts unchanged |
| `crates/host/tests/own.rs::two_businesses_share_one_release_and_differ_only_by_owner_configuration` | T01: two tenants on one router and catalogue with different applied configurations; separate revisions, labels and history; the same CRM command refused for one and served for the other; neither owner reaches the other tenant |
| `crates/host/tests/own.rs::the_preview_states_changes_readiness_and_cost_and_writes_nothing` | T03/T05: changes, per-module readiness and cost, catalogue and pricing identity on the installed catalogue; previewing writes nothing; an unknown module is `unknown_module`, cannot be applied and writes nothing |
| `crates/host/tests/product_workspaces.rs` (toggle loop) | T07: each of the thirteen workspace modules refuses its read and its command route with `module_disabled` when disabled and records no command row, an unrelated module keeps answering, and both return after enabling; the five ledger modules without a workspace projection (commerce, purchasing, billing, support, accounting) refuse their command route the same way |
| `crates/host/tests/own.rs::chat_and_dashboard_agree_on_business_sentences_and_blocked_plans` | T02: a business sentence that enables and disables modules, a `mandatory` block and a `missing_dependency` block each give the identical plan, base configuration, catalogue and pricing identity through chat and through the dashboard request; neither writes |
| browser `OWNER-native-rendered-preview` (`tools/e2e/owner-configuration.mjs`) | T03: the review section renders exactly the host plan's changes and effects, its plan hash and base revision; the preview writes no revision |
| browser `OWNER-native-blocked-plan-is-shown-and-never-applied` | T05: a `missing_dependency` plan renders its reason and detail with confirmation disabled; sending the blocked plan anyway is `plan_blocked`; no revision or receipt is written |
| browser `OWNER-native-two-tab-stale-plan-refused` | T06: a plan reviewed in one tab is refused with `stale_base` after a second tab commits; the stale plan writes no revision and changes no module |
| browser `OWNER-native-non-owner-refused` (`tools/e2e/verify.mjs`) | T04: a signed-in principal without a management grant sees the refusal at `/app/owner` with no modules, preview or history; plan, chat and history answer `missing_grant` |
| browser `OWNER-native-preview-and-apply-send-no-provider-request` (`tools/e2e/ai-disclosure.mjs`) | T03 f: the owned provider endpoint receives no request while the owner previews, reviews and confirms the AI-sharing configuration |

Still missing after these: a revoked owner in the browser (T04 l; the never-granted case is covered), a read-only owner (T04 j), and the command-status routes of disabled modules.

---


Read-only analysis, 2026-11-13, branch `feat/registry-owner-vocabulary`. Nothing was built or run; every claim below is from reading test bodies.

## Inventory actually found

| Short name | File | Count | Notes |
| --- | --- | --- | --- |
| `own.rs` | `crates/host/tests/own.rs` | 17 `#[tokio::test]` | HTTP + PostgreSQL, one tenant (`acme-retail`) |
| `own/disclosure.rs` | `crates/host/tests/own/disclosure.rs` | 5 | included by `own.rs` via `#[path]`; AI-disclosure approval |
| `engine.rs` | `crates/own/tests/engine.rs` | 21 `#[test]` | pure engine, **synthetic catalogue** (`core/restaurant/laundry/events/reports/ai-desk`), no HTTP/PG |
| host unit | `crates/host/src/own.rs` `mod tests` (line 1534) | 3 | chat parsing / registry aliases / catalogue requirements |
| slice unit | `crates/slice/src/owner.rs` `mod tests` (line 689) | 2 | pending-marker reuse (`reuse_or_mint`) |
| `product_workspaces.rs` | `crates/host/tests/product_workspaces.rs` | 1 test, owner part at lines 132-216 | `thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants` |
| `assistant.rs` | `crates/host/tests/assistant.rs:1288` | 1 | `disabling_the_pipeline_module_removes_the_tool_and_zeroes_the_counts` |
| accounting | `crates/host/tests/erp_accounting/{stock,invoice,purchase}_sources.rs` | 3 tests touch `/own/plan` | policy-pinned modules report `blocked.reason == "mandatory"` |
| browser | `tools/e2e/owner-configuration.mjs` (70 lines) | 4 `OWNER-*` passes | called from `tools/e2e/verify.mjs:387` |
| browser | `tools/e2e/verify.mjs:382-386` | `F05-owner-confirmation` | preview → confirm → "committed revision" |
| browser | `tools/e2e/ai-disclosure.mjs` | 3 `AI-native-*` passes | owner AI-sharing review/apply/revoke |
| browser | `tools/e2e/invoice-posting.mjs:95-97` | `INVOICEPOST-native-source-close-and-owner-dependency` | API call from the page, `blocked.reason == 'mandatory'` |
| browser (opt-in) | `tools/e2e/product.mjs:125-136` | `OWN-bounded-chat-parity`, `OWN-business-discovery`, `OWN-business-modules`, `OWN-chat-ui` | only when `ARCHAVA_E2E_PRODUCT=1`; `observe()` records `met`/`gap` and **does not assert**, so these are not gate evidence |

The brief said 29 integration tests in `own.rs`; I count 17 + 5 = **22**. No other file is pulled in by `#[path]`.

Conventions below: `own.rs::name` = `crates/host/tests/own.rs`; `engine.rs::name` = `crates/own/tests/engine.rs`. "Engine only" means the clause is proven against the synthetic catalogue and not against the real host catalogue, HTTP or PostgreSQL.

---

## OWN-T01 — two consumers, identical core, config only through UI/Archava

| Clause | Evidence |
| --- | --- |
| a. Two business-model consumers run on the identical released CRM/core | **NOT COVERED.** Every owner test uses one tenant with one configuration. `own/disclosure.rs::owner_disclosure_review_cannot_cross_tenants_or_survive_management_revocation` plans in a second tenant (`other-business`) but only to prove an approval cannot cross; it asserts nothing about two configurations coexisting. Missing (Rust integration): two tenants on one router apply different module sets and labels; each tenant's `/own/config`, `/crm/records` labels and module gate reflect only its own config. |
| b. No source fork | NOT COVERED as a test, and probably not testable beyond (a) — one binary serving both is the proof. UNSURE whether the "cold public-contract fixtures" named in `PRD_INHERITED_TRACEABILITY.v1.json` still exist; I found no test file matching them (grep for `consumer` in `crates/*/tests` and `tools/` gives nothing relevant). |
| c. Owner changes supported config through the UI | Browser `OWNER-native-disable-lost-response-recovery` (`owner-configuration.mjs:22-44`): clicks `owner-toggle-crm-pipeline`, `owner-preview`, `owner-confirm` and asserts the module is disabled in `/own/config`. `OWNER-native-chat-manual-plan-identity` (`:49-56`): fills `owner-chat-input`, confirms, asserts `config.terminology.pipelineLabels.new` persisted. One tenant only. |
| d. Relabel changes the caption, not the canonical key | `engine.rs::t01_relabelling_changes_the_label_but_not_the_canonical_stage_key` (engine only). `own.rs::crm_presentation_uses_current_labels_without_granting_owner_access`: `/crm/records` returns `pipelineLabels.new == "Discovery <&>"`, `configurationRevision == 1`, and `contact` unchanged. Browser `owner-configuration.mjs:57-62`: the `record-move-stage` option with `value == 'new'` renders the configured label as plain text (this assertion sits between two passes; it fails the gate but is reported under `OWNER-native-label-restore-preserves-history`). |
| e. Only *supported* config can be changed | `own.rs::the_chat_plan_equals_the_dashboard_plan_and_applies`: `"rename invented to Bid"` → 400 `unrecognized_request`. `own.rs::a_foreign_tenant_role_or_approval_is_refused_without_writing`: an unknown body field → 422. NOT COVERED: an invented stage key through the dashboard body (`/own/plan` with `pipelineLabels: {"invented": ...}`) — Rust integration. |

## OWN-T02 — chat and dashboard give an equivalent validated plan with authoritative identity

| Clause | Evidence |
| --- | --- |
| a. Same request via chat and dashboard → equal plan | `own.rs::the_chat_plan_equals_the_dashboard_plan_and_applies`: `chat["plan"] == dashboard["plan"]` for `disable crm-pipeline` and for `rename qualified to Bid`. Browser `OWNER-native-chat-manual-plan-identity` (`:49-53`): the `/own/chat` response captured from the real chat UI deep-equals a `/own/plan` fetch for the same relabel. Note the "manual" side in the browser is an API call from the page, not the dashboard controls. |
| b. Equal base / config / catalogue / pricing identity | Same test: `baseConfig`, `catalogueVersion`, `pricingVersion`, `plan.baseRevision` equal. Browser: same four keys deep-equal (`:53`). `own.rs::a_confirmation_from_a_previous_catalogue_or_pricing_refuses` asserts `plan.catalogueVersion == CATALOGUE_VERSION` and `plan.pricingVersion == "included@1"`. |
| c. The plan is validated and actionable | Same chat test: the chat plan hash applies through `/own/apply` → revision 1. Determinism: `engine.rs::t02_the_same_typed_request_has_the_same_plan_identity`, `a2_plan_identity_does_not_depend_on_request_order` (engine only). |
| d. Equivalence for a module **enable** / business-sentence request | **NOT COVERED.** `product_workspaces.rs:159-170` asserts only the parsed `request.enable/disable` of "Bisnis saya hotel, tambahkan laundry tanpa restaurant"; it never compares with a dashboard plan. Host unit `a_business_sentence_enables_the_named_modules_and_what_they_need` asserts the parse only. Missing (Rust integration): chat sentence → post its `request` to `/own/plan` → plans equal. |
| e. Equivalence when the plan is blocked | NOT COVERED. Missing (Rust integration): `disable crm-contacts` by chat and by dashboard give the same `blocked`. |

## OWN-T03 — preview shows changes / dependencies / readiness / cost; no hidden side effect

| Clause | Evidence |
| --- | --- |
| a. Actual changes | `own.rs::the_chat_plan_equals_the_dashboard_plan_and_applies`: `plan.changes[0].kind == "disable"`. `own.rs::restore_removes_the_labels_the_target_snapshot_never_had`: restore preview contains `removeStage` for `new`. Browser `:25` asserts the **API** plan's changes name `crm-pipeline`. **NOT COVERED:** the rendered `owner-changes` list in the DOM (browser gate). |
| b. Dependencies | `engine.rs::t05_a_missing_dependency_is_an_explicit_block_and_never_a_partial_apply`: `blocked.detail` contains the required module (engine only). **NOT COVERED** over HTTP or in the DOM (`owner-blocked`). |
| c. Readiness and cost classification | `engine.rs::t03_the_plan_carries_readiness_and_cost_for_every_change` (engine only). Browser `:26-27`: the `owner-module-crm-pipeline` list item text includes the `readiness` and `cost` strings from `/own/config` — that is the inspect list, not the preview's `owner-effects`. **NOT COVERED:** `plan.effects` readiness/cost for an enable on the real catalogue over HTTP; `owner-effects` in the DOM. |
| d. No hidden activation / payment / provider effect in the preview document | `own.rs::the_chat_plan_equals_the_dashboard_plan_and_applies`: the preview's key set is asserted closed (8 keys) and `payment`, `provider`, `activation` are absent. Browser `:28`: `plan.effects.length == 0` for the optional disable. |
| e. Preview writes nothing | Browser `:54`: revision unchanged after a chat preview. `own.rs` chat test: `config_revisions == 1` after several further plans and refused chats (indirect). No test asserts `apply_receipts`/`config_audit` unchanged after a plan — small gap. |
| f. No provider call during preview/apply | **NOT COVERED.** `own/disclosure.rs` points the planner at `127.0.0.1:1`, which would fail rather than be observed. `ai-disclosure.mjs` counts `fixture.requests` only around the agent goal and after revoke (`:59-75`), not around the owner preview/confirm. Missing (browser gate, a two-line addition): `fixture.requests.length` unchanged across owner AI preview + apply. |

## OWN-T04 — backend refuses unauthorized callers

| Clause | Evidence |
| --- | --- |
| a. Principal with no management grant | `own.rs::a_missing_or_revoked_grant_refuses_the_operation`: Bob `GET /own/config` → 403 `missing_grant`. `own.rs::crm_presentation_…`: Bob `/own/config` → 403. **Only inspect is asserted for a never-granted principal**; plan, chat, apply, restore, status, history are not. |
| b. Public session (no / forged bearer) | `own.rs::the_public_owner_surface_rejects_missing_and_forged_credentials`: 401 on `GET /own/config` (none, forged), `POST /own/plan` (forged), `POST /own/chat` (none); `config_revisions == 0`. **NOT COVERED:** `/own/apply`, `/own/restore/plan`, `/own/restore`, `/own/status`, `/own/history` unauthenticated. |
| c. Forged tenant | `own.rs::a_foreign_tenant_role_or_approval_is_refused_without_writing`: `/own/plan` for `globex-corp` → 403 `missing_grant`. Not asserted for apply/status/history. `own/disclosure.rs::owner_disclosure_review_cannot_cross_tenants_…`: a signed approval from one tenant is refused in another where Alice does hold a grant. |
| d. Forged role | Same test: `role` field on `/own/plan` → 422. |
| e. Forged approval | Same test: `approval` field on `/own/apply` → 422, `config_revisions == 0`. `own/disclosure.rs::owner_disclosure_approval_binds_whole_policy_actor_expiry_and_one_effect`: missing, forged-signature, expired, other-actor and broadened approvals → 409 `disclosure_approval_required`, zero revisions/receipts. |
| f. Revoked grant — confirm | `own.rs::a_missing_or_revoked_grant_refuses_the_operation`: new apply after revocation → 403, revisions stay 1. Also `own/disclosure.rs::…survive_management_revocation` → 403. |
| g. Revoked grant — replay | Same test: the original request id replayed after revocation → 403 `missing_grant` (not answered from the receipt). |
| h. Revoked grant — status | **NOT COVERED.** The test's comment says the same gate covers reads, but `GET /own/status` is never called after revocation (config, history and plan are). Missing (Rust integration). |
| i. Revoked grant — restore / restore-plan / chat | NOT COVERED (Rust integration). |
| j. Read-only grant (`config.read` without `config.apply`) cannot apply | NOT COVERED — no fixture seeds a read-only owner. UNSURE whether the PRD intends this as part of "tanpa management grant"; the UI has an `owner-read-only` state. |
| k. Other owner cannot read/replay a foreign request id | `own.rs::another_principal_cannot_read_or_replay_a_foreign_request_id`: Carol's status → `unknown`; her apply with Alice's id → 409 `request_id_reused`. |
| l. Browser negatives | **NOT COVERED.** No browser check signs in as a non-owner or revoked owner at `/app/owner`. |

## OWN-T05 — blocked / gap results are explicit and never executed

| Clause | Evidence |
| --- | --- |
| a. Missing dependency → explicit block | `engine.rs::t05_a_missing_dependency_…` and `t07_disabling_a_dependency_of_an_enabled_module_is_blocked` (engine only). Host unit `catalogue_requirements_follow_the_registry` proves the real catalogue *declares* `accommodation → scheduling`, `accounting → organizations`. **NOT COVERED over HTTP:** no test asserts `blocked.reason == "missing_dependency"` on the real catalogue. `product_workspaces.rs:208-216` asserts only `blocked.is_object()` for disabling `commerce`; the reason is not checked (UNSURE which reason it is). |
| b. Incompatible | UNSURE what the host maps "incompatible" to. `engine.rs::t05_entitlement_install_and_readiness_are_distinct_refusals` covers `NotEntitled`/`NotInstalled`; `t08_a_restore_is_revalidated_against_the_current_catalogue` covers a restore blocked by a moved catalogue. Engine only; nothing over HTTP. |
| c. Module unavailable / unknown | Same engine test (`ModuleUnavailable`, `UnknownModule`). **NOT COVERED over HTTP:** `/own/plan` enabling an id that is not in the catalogue. |
| d. Unsupported workflow → explicit gap | `own.rs::the_chat_plan_equals_the_dashboard_plan_and_applies`: "paint the sky", "rename invented to Bid", "disable" → 400 `unrecognized_request`, revisions unchanged. Host unit: `parse_chat("bisnis sulap").is_err()`. The result is a refusal, not a structured gap document; UNSURE whether the PRD wants more than that. Not rendered-checked in the browser. |
| e. Blocked plan is not executed | `own.rs::a_mandatory_module_is_blocked_and_nothing_is_written`: `blocked.reason == "mandatory"`, apply → 409 `plan_blocked`, `config_revisions == 0`. Only the mandatory reason is exercised over HTTP. `engine.rs::unavailable_disclosure_blocks_the_whole_configuration_without_a_partial_enable` (engine only). |
| f. Source-policy / resource-lifecycle dependencies stay blocked | `erp_accounting/stock_sources.rs::stock_policy_alone_pins_owner_dependencies_without_a_purchase_or_invoice_policy`, `purchase_sources.rs::purchase_policy_pins_owner_dependencies_and_financial_sources_remain_permission_scoped`, `invoice_sources.rs::source_policy_versions_scope_recovery_and_owner_toggle_preserve_capture`: `/own/plan` disabling the pinned modules → `blocked.reason == "mandatory"` with the module named. Browser `INVOICEPOST-native-source-close-and-owner-dependency` (`invoice-posting.mjs:96`) asserts the same from the page. None of them submits the apply. |
| g. "Not counted PASS" | Process clause, not a test. `product.mjs` `observe()` records `gap` without failing; the four `OWNER-*` passes are hard asserts. |

## OWN-T06 — one authoritative effect under duplicates, races, stale tabs, lost responses, restart

| Clause | Evidence |
| --- | --- |
| a. Duplicate apply | `own.rs::a_duplicate_submission_replays_one_receipt_and_one_effect`: second call `replayed == true`, same revision, one row each in `config_revisions`, `apply_receipts`, `config_audit`. Engine: `t06_the_same_request_id_yields_one_effect_and_the_same_receipt`. |
| b. Racing apply | `own.rs::racing_applies_converge_to_one_revision_and_one_receipt`: two request ids, same plan, concurrent → statuses `[200, 409]`, one row in each table. The 409's `reason` is not asserted. Racing the *same* request id concurrently is NOT COVERED (UNSURE how valuable; the unique key should make it equivalent). |
| c. Two-tab stale config | API: `own.rs::a_stale_base_and_a_foreign_confirmation_refuse_without_writing`: plan on base 0 after revision 1 → 409 `stale_base`, `detail.currentRevision == 1`; a wrong hash → 409 `confirmation_mismatch`. Engine: `t06_a_stale_base_is_refused_and_demands_a_replan`. **NOT COVERED:** an apply carrying a still-valid older plan hash and stale base under a fresh id, with the reason asserted (the racing test reaches this path but checks status only). **NOT COVERED in the browser:** two tabs. |
| d. Refresh / lost response | API: `own.rs::a_committed_request_id_reads_back_its_receipt_after_a_lost_response` (status → `receipt`, verbatim receipt, still one row each) and `an_uncommitted_request_id_answers_unknown_and_writes_nothing`. Browser `OWNER-native-disable-lost-response-recovery` (`:29-36`): the `/own/apply` response is dropped after a real commit, `owner-pending-id` equals the sent `requestId`, exactly one apply was sent, one `apply_receipts` row exists, and after reload the status reads "recovered apply committed revision N" with the pending marker cleared. Slice unit tests cover marker reuse and non-adoption of a foreign marker. |
| e. Host restart | API: `own.rs::a_rebuilt_host_recovers_the_same_receipt_and_history_from_postgres`: a new `AppState`/router over the same database reads revision 1, replays the same receipt, one revision and one receipt. This is a rebuilt router in one process, not a process restart. `own/disclosure.rs::…one_effect` also replays on a rebuilt router. **NOT COVERED in the browser:** a real server restart between commit and recovery. |
| f. Recovery is authorized | `own.rs::another_principal_cannot_read_or_replay_a_foreign_request_id`. Revoked-principal status is not covered (see T04-h). |
| g. Uncommitted pending id in the browser (`unknown`) | NOT COVERED: the UI's handling of a pending id the server never committed (should keep/retry the same id, not mint a second effect). Browser gate. |

## OWN-T07 — enable/disable changes real availability; dependents protect their dependencies

| Clause | Evidence |
| --- | --- |
| a. UI availability changes | Browser `:41`: after disabling `crm-pipeline`, `/app/contact` shows `record-pipeline-disabled` and no `record-opportunity`; `:57` shows it again after restore. Only this one module. **NOT COVERED:** UI of any ERP module (navigation entry / workspace screen) when it is disabled. |
| b. API availability changes | `own.rs::disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it`: `/crm/commands` `opportunity.create` → 403 `module_disabled`, then 200 after re-enable. `product_workspaces.rs:171-207`: with `projects` disabled, `GET /erp/projects/workspace` → 403 and `POST /erp/projects/commands` → 403 `module_disabled`; workspace → 200 after re-enable (the re-enabled command write is not re-tested). Browser `:38-39`: direct CRM write → 403 (status only). **Two of the registered modules are exercised**; nothing iterates the registry, although the command routes are generated from it. |
| c. Tool availability changes | `assistant.rs::disabling_the_pipeline_module_removes_the_tool_and_zeroes_the_counts`: the assistant's pipeline tool → 404 while disabled, the overview's `pipeline` section is null, both return after re-enable. **NOT COVERED:** agent runtime (Ava) dispatch to a disabled ERP module — no test in `agent_runtime.rs` or `agent_runtime/*.rs` mentions a disabled module. |
| d. Direct API still refuses when disabled | As (b). |
| e. Disabling a dependency of an enabled module is refused | `engine.rs::t07_disabling_a_dependency_of_an_enabled_module_is_blocked` (engine only, synthetic catalogue). **NOT COVERED over HTTP with the real catalogue** (e.g. disable `scheduling` while `accommodation` is enabled → `blocked.reason` asserted, apply → `plan_blocked`, zero revisions). `product_workspaces.rs:208-216` is the nearest and asserts only that `blocked` is an object. |
| f. Mandatory module cannot be disabled | `own.rs::a_mandatory_module_is_blocked_and_nothing_is_written`; `engine.rs::t07_a_mandatory_module_cannot_be_disabled`. |
| g. Configured labels in business views (project's own `remaining`) | `own.rs::crm_presentation_uses_current_labels_without_granting_owner_access` (API) and browser `:57-62` (rendered option caption). The 2026-11-08 review's statement that the view renders canonical strings is superseded by this hard assertion — provided the gate passes; I did not run it. |

## OWN-T08 — history preserved; labels traceable; restore rewrites nothing

| Clause | Evidence |
| --- | --- |
| a. Existing business records preserved | `own.rs::restore_lands_as_a_new_revision_and_keeps_history_and_business_rows`: contact **count** is 1 after restore (count only). Browser `:43, :47, :55, :66`: per-table row count + md5 digest of 23 CRM/accounting tables is identical after disable, restore, relabel and final restore. Browser `:40`: the pre-existing opportunity is still returned while the module is disabled. |
| b. Audit preserved | Browser digests include `archava_accounting.audit` and `archava_crm.commands`. **NOT COVERED:** owner `config_audit` rows of earlier revisions unchanged after a restore (only counts in the duplicate/race tests, never after restore). |
| c. Receipts preserved | **NOT COVERED** after restore: no test re-reads an earlier operation's receipt (via `/own/status` or `apply_receipts`) after a later restore. `own.rs` restore tests assert only the new receipt (`revision 3`, `outcome "restore"`). |
| d. Permissions preserved | **NOT COVERED** for `archava_owner.owner_grants` and `archava_crm.memberships` (not in the digest list). `archava_accounting.memberships` is in the browser digest. `own.rs::crm_presentation_…` shows a relabel grants a CRM reader no owner access. |
| e. Historical label / meaning traceable | **NOT COVERED.** `/own/history` returns each revision's `config` (`crates/host/src/own.rs:1335-1395`), but tests assert only the revision numbers `[3,2,1]` and `revisions[0].actor`. Nothing asserts that revision 2's row still shows the label it had after a restore removed it. Engine `disclosure_is_an_explicit_versioned_change…` checks a historical snapshot for disclosure only. |
| f. Restore is a new revision | `own.rs::restore_lands_…` (revision 3, `outcome == "restore"`, history `[3,2,1]`), `own.rs::restore_removes_the_labels_the_target_snapshot_never_had`, `engine.rs::t08_restore_creates_a_new_revision_and_never_rewrites_history`, `a3_restore_removes_the_labels_the_target_never_had`. Browser `OWNER-native-compatible-restore` (`:45-48`) and `OWNER-native-label-restore-preserves-history` (`:65-67`): final `terminology` and `enabled` equal the baseline. |
| g. Restore does not rewrite business/financial history | Browser digests as in (a). UNSURE how much financial data exists when the owner step runs (`verify.mjs:387`); if the accounting tables are empty for `acme-retail` at that point the digest proves little. Worth checking the gate's ordering before relying on it for "financial". The Rust restore test checks one CRM count and no financial table. |

## OWN-T09 — stale identity demands replan; no invented price/readiness

| Clause | Evidence |
| --- | --- |
| a. Stale config | `own.rs::a_stale_base_and_a_foreign_confirmation_refuse_without_writing` (plan → 409 `stale_base` + `currentRevision`); engine `t06_a_stale_base_…`, `t06_the_same_request_id…` (stale apply under a new id). Apply-side reason over HTTP not asserted (see T06-c). |
| b. Stale catalogue | `own.rs::a_confirmation_from_a_previous_catalogue_or_pricing_refuses`: confirmation naming `owner-catalogue@0` → 409 `catalogue_changed` with `held`/`live`, zero revisions; the live identity applies and is recorded on the receipt. Engine: `a2_a_catalogue_change_is_a_plan_identity_change`, `a2_a_moved_catalogue_identity_refuses_the_stale_plan`. The HTTP test forges the *confirmation's* version; the catalogue itself never moves in a host test (it is a compile-time constant), so "catalogue really changed between preview and apply" is engine only. |
| c. Stale price snapshot | Same test: `included@0` → 409 `pricing_changed`. Same caveat. |
| d. Changed target resources | `own/disclosure.rs::owner_disclosure_provider_account_and_model_changes_require_new_review_but_offline_revoke_works`: provider model or account changed after preview → 409 `ai_provider_changed`; an old approval swapped onto a fresh plan is refused; restore-plan under a changed provider → 409. `…companies_are_resolved_from_current_real_company_grants`: company read grant removed after preview → 403 `company_scope_unavailable`, zero revisions. **NOT COVERED:** a module becoming pinned between plan and apply (e.g. plan `disable accounting`, then a stock/invoice policy is set, then apply) — Rust integration, in an `erp_accounting` suite. |
| e. Replan / reconfirmation is requested | The refusals carry `currentRevision` or `held`/`live`. **NOT COVERED in the browser:** the UI's reaction to a stale/changed refusal (confirm disabled, fresh preview required). |
| f. No invented price or readiness | `engine.rs::t09_a_request_cannot_invent_readiness_cost_or_authority`: a body with `readiness`/`cost`/`price`/`role` fails deserialization; effects come from the catalogue (engine only). Over HTTP only `role` is tested (422). **NOT COVERED over HTTP:** `readiness`/`cost`/`price` fields on `/own/plan`. On "model": `/own/chat` is a deterministic parser in the tests I read (`parse_chat`), so no model is involved there; UNSURE whether any model-backed path (Ava) can propose owner configuration — I found no test of one. |

## OWN-T10 — real browser journey end to end

| Step | Evidence |
| --- | --- |
| inspect | `owner-configuration.mjs:14, :22`: `/app/owner` loads, `owner-preview` enabled, `owner-module-crm-pipeline` text matches `/optional/`. |
| plan → preview | `:23`: toggle + `owner-preview` click, wait for `owner-confirm` enabled. Chat route: `:51`. The preview's rendered content (`owner-plan`, `owner-changes`, `owner-effects`) is never read. |
| confirm → applied | `:30-37` (with dropped response), `:46`, `:55`, `:65`: `owner-status` contains "committed revision N", cross-checked against `/own/config`. Also `F05-owner-confirmation` (`verify.mjs:382-386`). |
| recovery | `OWNER-native-disable-lost-response-recovery` (see T06-d). |
| history | `:45, :65`: waits for `owner-restore-<base>` to exist. History row content (actor, outcome, revision list) is not asserted. |
| restore | `OWNER-native-compatible-restore`, `OWNER-native-label-restore-preserves-history`. |
| no code / config file / terminal by the owner | Structural only: every owner action is a click or fill; `psql` and page-side `fetch` are used for setup (creating an opportunity) and verification. Nothing asserts it, and it is not really assertable. |
| Not in the journey | enabling a module through the UI (only disable + restore-to-enabled); a blocked plan rendered (`owner-blocked`); a refusal rendered (`owner-refusal`); read-only owner (`owner-read-only`); two tabs; server restart; a second business profile. |

---

## Missing tests, most valuable first

Each is one test. "Rust" = `crates/host/tests/own.rs` unless stated; "browser" = `tools/e2e/owner-configuration.mjs` unless stated.

1. **Rust — dependent-module disable is refused on the real catalogue (T07-e, T05-a, T05-e).** Disable a module another enabled module requires (per `catalogue_requirements_follow_the_registry`: `scheduling` with `accommodation` enabled, or `organizations` with `accounting`). Assert `blocked.reason`, `blocked.module`, apply → 409 `plan_blocked`, zero revisions. This is a named clause of T07 with engine-only proof today.
2. **Rust — two tenants, two configurations, one router (T01-a).** Seed a second tenant with its own owner grant and contact; apply different module sets and labels to each; assert each tenant's `/own/config`, `/crm/records` `pipelineLabels` and module gate reflect only its own config and that tenant A's revision counter is untouched by B.
3. **Rust — revoked/ungranted principal on every remaining route (T04-a, -h, -i).** After revocation: `GET /own/status` for the committed id, `POST /own/restore/plan`, `POST /own/restore`, `POST /own/chat` → 403 `missing_grant`. For never-granted Bob: plan, chat, apply, status, history → 403. Can extend `a_missing_or_revoked_grant_refuses_the_operation` or be one new test.
4. **Rust — unauthenticated confirm/status/replay (T04-b).** Missing and forged bearer on `/own/apply`, `/own/restore`, `/own/restore/plan`, `/own/status`, `/own/history` → 401, zero rows. T04 names confirm/status/replay explicitly and only inspect/plan/chat are covered.
5. **Rust — historical labels are readable from history after restore (T08-e).** Extend the scenario of `restore_removes_the_labels_the_target_snapshot_never_had`: assert `history.revisions[rev 2].config.terminology.pipelineLabels` still contains `new: "Inquiry"` and revision 1's row is byte-equal before and after the restore.
6. **Rust — restore leaves earlier receipts, audit and grants untouched (T08-b, -c, -d).** Snapshot `apply_receipts`, `config_audit`, `owner_grants`, `archava_crm.memberships` rows (text digest) before the restore; assert equal for the pre-existing rows after it, and that `/own/status` for the first request id still returns the original receipt.
7. **Rust — preview carries readiness/cost and dependency detail on the real catalogue (T03-b, -c).** `/own/plan` enabling a previously disabled module: assert `plan.effects` has the module with the `readiness`/`cost` that `/own/config` reports for it; for a blocked plan assert `blocked.detail` names the missing module.
8. **Rust — every registered module's routes refuse when disabled (T07-b, -d).** Iterate `archava_domain::modules::all()` (optional modules only): disable, then `POST /erp/<id>/commands` and `GET /erp/<id>/workspace` → 403 `module_disabled`. Probably belongs in `product_workspaces.rs`, which already seeds grants for thirteen domains. UNSURE about cost: each disable is a plan+apply, so keep it in one fixture.
9. **Browser — the rendered preview shows changes, effects and a blocked reason (T03-a/-c, T10).** After `owner-preview`: read `owner-changes` and `owner-effects` text and compare with the API plan; then request a mandatory disable (toggle or chat `disable crm-contacts`) and assert `owner-blocked` is shown and `owner-confirm` stays disabled.
10. **Browser — two-tab stale apply (T06-c, T09-e).** Preview in tab A, apply a different change from tab B (or a page-side fetch standing in for it), confirm in A: `owner-status` shows the stale refusal, revision advanced once, one receipt for B's id and none for A's.
11. **Rust — apply with a stale base and a valid old hash names its reason (T06-c, T09-a).** Plan at base 0, apply it, re-submit the same plan under a new request id: 409 with `reason == "stale_base"` and `currentRevision`. Small; closes the unasserted reason in the racing test.
12. **Rust — forged `readiness`/`cost`/`price` on `/own/plan` (T09-f).** 422 (or the same refusal as `role`), zero revisions. Small.
13. **Rust — chat/dashboard equivalence for an enable sentence and for a blocked plan (T02-d, -e).** Post the business sentence to `/own/chat`, post its returned `request` to `/own/plan`, assert plans equal; same for `disable crm-contacts` with equal `blocked`.
14. **Rust (`erp_accounting`) — resource pinned between plan and apply (T09-d).** Plan `disable accounting` before any policy, set a stock or invoice policy, apply the old plan → refused, zero revisions.
15. **Browser (`ai-disclosure.mjs`) — no provider request during owner preview/apply (T03-f).** Record `fixture.requests.length` before `owner-ai-preview` and assert it is unchanged after `apply(base+1)`.
16. **Browser — non-owner at `/app/owner` (T04-l).** Sign in as a principal without the owner grant: the screen shows the refusal and no enabled `owner-preview`/`owner-confirm`. Needs a second login in the gate; UNSURE how costly that is in the current harness.
17. **Rust (`agent_runtime.rs`) — Ava cannot dispatch to a disabled module (T07-c).** UNSURE of the expected refusal shape; I did not read the agent dispatch path, only confirmed no test mentions a disabled module.
18. **Browser — server restart between commit and recovery (T06-e).** Most expensive; the gate would need to restart the server mid-run. The Rust rebuilt-router test is the current stand-in.

## Things to verify before trusting this mapping

- Whether accounting tables hold `acme-retail` rows when `verifyOwnerConfiguration` runs (T08-g).
- Which `blocked.reason` `disable commerce` returns at `product_workspaces.rs:212` (dependency or policy pin).
- Whether any model-backed path can propose owner configuration (T09-f "model").
- The `product.mjs` `OWN-*` observations run only with `ARCHAVA_E2E_PRODUCT=1` and never fail the gate; I did not count them as coverage anywhere above.
