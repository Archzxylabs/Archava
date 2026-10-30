# Archava Platform — AuthN/AuthZ + Multi-Tenant Isolation Audit

- **Audit ID:** archlab-2026-10-21-authz
- **Date:** 2026-10-21 (WIB)
- **Scope:** `apps/crm/src/server.ts`, `apps/crm/src/host.ts`, every `packages/*/src/api.ts` that mounts an HTTP surface, and `rust/crates/host/src/**` (router + ERP + own + identity).
- **Mode:** READ-ONLY source audit. No product file modified. No live database, cluster, or HTTP server was started; every verdict below comes from code paths, not runtime reproduction.
- **Method:** Perseus Phase-2 negative-analysis loop — Source → Sanitizers → Sink → Verdict — with `grep`/static reading to prove presence or absence of each guard. Confidence per Perseus scale (High = direct code path with the defense visibly missing; Medium = path present but one runtime/config assumption; Low = pattern match only).
- **Relation to prior audit:** re-verifies **F03** from `docs/ARCHXY_BOTH_REAUDIT_2026-10-20.md` (recovery receipt status drops the `access` result of the membership check) and searches for the same shape in every other domain, TS and Rust.

---

## 1. Scope numbers (routes examined vs guarded)

Counts are **static route entries** (distinct method + path pattern), enumerated from the code, not from a running server.

| Item                                                                                             | Count                                                                                            |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| Route entries examined                                                                           | **250** — 200 TypeScript + 50 Rust                                                               |
| TS: `apps/crm/src/host.ts` host-level entries (incl. 19 domain mounts, 6 native-CRM, 11 CRM API) | 44                                                                                               |
| TS: domain API endpoint entries (19 `create*Api` modules)                                        | 156                                                                                              |
| Rust: `router()` entries (27 GET / 23 POST)                                                      | 50                                                                                               |
| Require an authenticated session                                                                 | **245 / 250**                                                                                    |
| Unauthenticated by design                                                                        | 4 (`GET /healthz` ×2, `POST /login` ×2)                                                          |
| Guest-capable                                                                                    | 1 (`POST /api/presence/session`)                                                                 |
| Tenant-scoped (tenant taken from path/query/body, then proven by a membership lookup)            | **239**                                                                                          |
| Domain command-status read endpoints                                                             | **30** (19 TS + 11 Rust)                                                                         |
| Status reads with an explicit read-grant and/or initiator binding                                | **TS 12/19; Rust 2/11** (2 more re-authorize the recorded operation but are not initiator-bound) |

Every TS domain mount validates the tenant against `settings.tenants` and calls `service.access(...)` before delegating (`apps/crm/src/host.ts:186-440`); every domain repository opens one transaction, issues `set_config('archava.tenant_id',$1,true)` and re-reads the membership row (`packages/subscriptions-pg/src/repository.ts:140-202`, identical shape in the other `*-pg` packages). The failures below are **granular permission / resource-scope** failures, not missing tenant checks.

---

## 2. Findings

### F-01 — P1 — Rust subscription status still discards the membership `access` (F03 remains open)

- **File:line:** `rust/crates/host/src/erp/subscriptions.rs:524-549`, decisive line **`:543`** (`let _ = &access;`); sink `rust/crates/adapters/src/erp/subscriptions.rs:861-866`.
- **Route:** `GET /erp/subscriptions/commands/status` — `rust/crates/host/src/router.rs:481`.
- **Source → sink:** authenticated session (`ErpActor`) → `resolve_subscriptions_context` loads membership and returns permissions/scope → handler **discards the value** → `subscriptions_command_status(connection, tenant, request_id)` runs `SELECT receipt FROM archava_subscriptions.commands WHERE tenant_id=$1 AND request_id=$2`.
- **Missing defense:** no `subscriptions.read` grant check, no actor/initiator binding, no resource-scope check. `access` is computed (cost paid) and then thrown away.
- **Preconditions:** principal holds an active row in `archava_subscriptions.memberships` for the tenant with **any** permission set (an empty or `audit.read`-only array satisfies the schema at `packages/subscriptions-pg/migrations/001_subscriptions_entitlements_usage.sql:6`), and knows a `request_id` UUID of another actor's command in that tenant. Tenant RLS still holds, so this is not a cross-tenant leak.
- **Impact:** same-tenant receipt disclosure (subscription id, action, revision, status, executedAt) to a member who is denied the equivalent list/detail read. This is exactly the 2026-10-20 F03 repro, still present.
- **Confidence:** **High** (code path is explicit; the TS twin below is the same shape).
- **Fix:** call the domain read gate immediately after resolving context, e.g. `if !access.holds(&["subscriptions.read"]) { rollback; return Err(forbidden()) }`, and bind the receipt to its initiator (`row.actor_id == principal.id`) as `accommodation_command_status` already does (`rust/crates/adapters/src/erp/accommodation.rs:1492-1519`). Add a negative HTTP test: member with `{audit.read}` → status of another actor's request → 403.

### F-02 — P1 — Rust attendance status has the identical discarded-access shape

- **File:line:** `rust/crates/host/src/erp/attendance.rs:513-546`, decisive line **`:532`**; sink `rust/crates/adapters/src/erp/attendance.rs:813-834`.
- **Route:** `GET /erp/attendance/commands/status` — `router.rs:510`.
- **Source → sink:** session → `resolve_attendance_context` → `let _ = &access;` → `attendance_command_status(connection, tenant, request_id.to_string())` → `SELECT receipt FROM archava_attendance.commands WHERE tenant_id=$1 AND request_id=$2`.
- **Missing defense:** no `attendance.read`, no actor binding, no scope filter.
- **Preconditions:** same-tenant active membership with any permission + known command UUID.
- **Confidence:** **High.**
- **Fix:** same as F-01; reuse the `attendance.read` grant already used by `list_sessions`/`get_session_detail`.

### F-03 — P1 — Rust billing status admits in a comment that it grants nothing beyond enrollment

- **File:line:** `rust/crates/host/src/erp/billing.rs:477-510`, decisive lines **`:495-500`** (comment “no permission gate exists on this surface” + `let _ = &access;`); sink `rust/crates/adapters/src/erp/billing.rs:986-996`.
- **Route:** `GET /erp/billing/commands/status` — `router.rs:469`.
- **Source → sink:** session → `resolve_billing_context` → discard → `billing_command_status(connection, tenant, request_id)` → `SELECT receipt FROM archava_billing.commands WHERE tenant_id=$1 AND request_id=$2`.
- **Missing defense:** no `billing.read`, no initiator binding. Billing receipts carry invoice ids, amounts and status transitions.
- **Preconditions:** same-tenant membership with any permission + known command UUID.
- **Confidence:** **High.**
- **Fix:** add the grant + initiator binding; update the comment, which currently documents the vulnerability as intended behaviour.

### F-04 — P2 — Same F03 shape in seven TypeScript domains (membership-only receipt reads)

- **Files:line:**
  - `packages/subscriptions-pg/src/repository.ts:615-633`
  - `packages/attendance-pg/src/repository.ts:531-549`
  - `packages/billing-pg/src/repository.ts:700-718`
  - `packages/laundry-pg/src/repository.ts:587-605`
  - `packages/field-services-pg/src/repository.ts:527-559`
  - `packages/formal-education-pg/src/repository.ts:597-631`
  - `packages/fulfillment-pg/src/repository.ts:612-630`
- **Routes (example):** `GET /api/subscriptions/{tenant}/commands/{id}` — `packages/subscriptions/src/api.ts:90`; peers at `attendance/src/api.ts:105`, `billing/src/api.ts:112`, `laundry/src/api.ts:106`, `field-services/src/api.ts:97`, `formal-education/src/api.ts:97`, `fulfillment/src/api.ts:109`.
- **Source → sink:** cookie session → host `service.access` + tenant allow-list (`apps/crm/src/host.ts:242-255`) → `create*Api` identity callback → `Service.getCommandStatus` → repository `transaction(tenant, principal, …)` which proves membership and sets the RLS GUC (`subscriptions-pg/src/repository.ts:140-202`) → `SELECT receipt … WHERE tenant_id=$1 AND request_id=$2`. The `access` object is available in the transaction callback but never consulted; these repositories’ `transaction()` has **no permission parameter** at all, unlike `accommodation-pg`/`commerce-pg`, which take `permission: X | null`.
- **Missing defense:** domain read grant and initiator/resource binding. For reference, the corrected pattern exists in the same codebase: `packages/crm-pg/src/repository.ts:247-283` (actor + `initiator_id`, per-receipt read permission, `receiptResource`) and `packages/support-pg/src/repository.ts:339-375` (`cases.read` + `actor_id` + `ownedCase`).
- **Preconditions:** principal is in the tenant’s `archava_<domain>.memberships` with any permission array (schema allows `{}`), and knows the target `request_id` UUID. Receipts are keyed by a client-chosen UUID, so guessing is not trivial; no enumeration endpoint exists.
- **Confidence:** **High** for the missing guard; **Medium** for practical exploitability (requires the victim’s request id).
- **Fix:** pass the domain read permission into `transaction(...)` (or call `grant(access, '<domain>.read')` first) and require `row.actor_id === access.ownerId` unless the caller holds a tenant-wide read scope, then resolve the receipt’s resource and re-check it. Mirror the CRM receipt code rather than inventing a new shape.

### F-05 — P2 — Rust organizations/projects/proposals/scheduling status reads skip the read grant

- **Files:line:** `rust/crates/adapters/src/erp/organizations.rs:739-760`, `projects.rs:388-410`, `proposals.rs:522-548`, `scheduling.rs:840-866`; handlers `host/src/erp/organizations.rs:356-409`, `projects.rs:376-429`, `proposals.rs:373-425`, `scheduling.rs:353-405`.
- **Source → sink:** session → `resolve_*_context` → status adapter. `projects`/`scheduling` check `projects.read` / `resources.read` on every _other_ surface (`adapters/src/erp/projects.rs:320-327`, `scheduling.rs:500-510`) but not here; organizations/proposals only apply `access.sees(owner)` and never check a read permission.
- **Missing defense:** read permission (organizations/proposals also lack an initiator check; scope is the only gate).
- **Preconditions:** active membership; for scope `all`, read of any receipt in the tenant whose resource is visible.
- **Confidence:** **Medium** (verdict depends on whether the product treats “scope” as sufficient on status; the same code checks a read grant on the equivalent list/detail surfaces, so the inconsistency is factual).
- **Fix:** add the same `holds([...])` gate the list/detail path uses, and bind the receipt actor.

### F-06 — P2 — Declared `assigned` scope is not enforced on billing / attendance / laundry reads

- **Files:line:**
  - Billing: `packages/billing-pg/src/repository.ts:726-760` (`listInvoices`), `:762-800` (`getInvoiceDetail`), `:849-864` (`listAttempts`) — `grant` yes, owner filter no, although invoices carry `owner_id` (`migrations/001_billing_invoices_settlement.sql`, insert at `repository.ts:291-318`).
  - Attendance: `packages/attendance-pg/src/repository.ts:555-601`, `:604-688`, `:690-721`, `:723-766` — `sessions.owner_id` exists (`migrations/001_attendance_sessions_rosters.sql:24`), never filtered.
  - Laundry: `packages/laundry-pg/src/repository.ts:613-647`, `:649-700` — `orders.owner_id` exists (`migrations/…:24`), never filtered.
- **Source → sink:** membership row `scope='assigned'` (`packages/*/src/contracts.ts` `scope: z.enum(['all','assigned'])`) → `access.scope` is parsed into the context (`billing-pg:210`, `attendance-pg:110`, `laundry-pg:189`) → list/detail SQL only filters `tenant_id`, never `owner_id`.
- **Missing defense:** row-level scope predicate. Contrast with `subscriptions-pg:657-660` / `:696-698` and `field-services-pg:582`, which do apply it.
- **Preconditions:** an operator deliberately provisioned with `scope='assigned'`; they can list and open every other operator’s invoices/sessions/orders in the same tenant.
- **Confidence:** **Medium** (semantics of `assigned` inferred from the contracts and from domains that do enforce it; no product doc read that redefines it).
- **Fix:** add `AND ($n = 'all' OR owner_id = $m)` to every read in these three repositories (or a `visible_*` SQL predicate, the pattern formal-education already uses: `formal-education-pg/src/repository.ts:78-80`).

### F-07 — P2 — Subscriptions entitlement/usage reads bypass the assigned-scope check that detail enforces

- **File:line:** `packages/subscriptions-pg/src/repository.ts:714-748` (`listEntitlements`), `:751-766` (`listUsageEvents`); contrast `:696-698` in `getSubscriptionDetail`.
- **Route:** `GET /api/subscriptions/{tenant}/records/{id}/entitlements` and `…/usage` — `packages/subscriptions/src/api.ts:82-87`.
- **Source → sink:** `grant(access, 'subscriptions.read')` then `SELECT * FROM …entitlements WHERE tenant_id=$1 AND subscription_id=$2` / `WHERE tenant_id=$1 AND entitlement_id=$2`, with no ownership join back to `subscriptions.owner_id`.
- **Missing defense:** assigned-scope check. Detail hides another owner’s subscription (returns `null`), but a caller who knows the id can still read its entitlements and usage history through the two sub-resources.
- **Preconditions:** `scope='assigned'` membership + known subscription/entitlement UUID.
- **Confidence:** **High** (guard visibly missing relative to the sibling method).
- **Fix:** join `archava_subscriptions.subscriptions` and apply the same `access.scope === 'assigned' && owner_id !== access.ownerId` predicate before returning; for usage, join through `entitlements → subscriptions`.

### F-08 — P3 — Rust session cookie has no `Secure`, and the token is also returned in the JSON body

- **File:line:** `rust/crates/host/src/router.rs:1751` (`"{SESSION_COOKIE}={token}; HttpOnly; Path=/; SameSite=Strict"`), token in body at `:1765-1770`; cookie name constant `:111`; expiry cookie `:1879`.
- **Missing defense:** `Secure` (unconditional absence, even if the deployment terminates TLS in front of it) and `__Host-` prefix; returning the raw token in the response body makes it readable by any script on the page, so `HttpOnly` does not bound XSS credential theft on the browser path.
- **Preconditions:** XSS or a network downgrade on a TLS deployment; the current fixture is plain HTTP on loopback, which is why the comment excuses it.
- **Confidence:** **Medium** (deployment mode not verified).
- **Fix:** set `Secure` when the listener/public origin is HTTPS (mirror `apps/crm/src/host.ts:54-56`, which already does this and uses `__Host-` when secure), and do not put the token in the body for the cookie flow — return only the CSRF value.

### F-09 — P3 — Rust login throttle is a single global source bucket

- **File:line:** `rust/crates/host/src/router.rs:96` (`const LOGIN_SOURCE: &str = "host-router"`), used at `:1740`; bucket limits in `rust/crates/host/src/identity.rs:425-445` (`source` 60/300 s, `account` 5/900 s).
- **Missing defense:** per-client source identity. Any unauthenticated attacker can burn the single global 60-attempt budget and deny sign-in to every user for 5 minutes; the per-account 5-attempt bucket additionally permits a 15-minute lockout of any known address. The TS host passes `incoming.socket.remoteAddress` (`apps/crm/src/host.ts:718`), so the two hosts diverge.
- **Preconditions:** unauthenticated network access to the Rust host.
- **Confidence:** **High** for the code shape; impact is availability, not confidentiality.
- **Fix:** thread the peer address (or a trusted `X-Forwarded-For` with an explicit proxy allow-list) into `Identity::login`, and keep the global bucket only as a second-tier ceiling.

### F-10 — P3 — Authenticated tenant-existence oracle on domain mounts

- **File:line:** `apps/crm/src/host.ts:186-194` (and the 18 identical blocks through `:440`) return `404 not_found` when the tenant is not in `settings.tenants`, while a configured-but-not-member tenant reaches `service.access` and returns `403`.
- **Missing defense:** uniform refusal for “unknown” vs “known but forbidden”. The SDK documents tenant ids as non-secret (`packages/sdk/src/host.ts:36-42`), so this is informational only.
- **Confidence:** **High**; severity low by design.
- **Fix:** if tenant ids must not be enumerable, collapse both to 404 (the `/api/crm/{tenant}` path already effectively does, since it delegates to membership).

---

## 3. Verified secure (with evidence)

- **Password storage — TS.** scrypt with fixed OWASP profile `N=32768, r=8, p=3`, 48 MiB `maxmem`, 16-byte random salt, 32-byte key, `timingSafeEqual`, fixed-parameter parsing so a hostile stored record cannot request costlier work (`packages/identity-pg/src/password.ts:16-43`). Minimum length 15 (`:8`), enforced again at hash time.
- **Password storage — Rust.** Line-for-line port, same parameters, dummy-salt derivation for missing records, `Busy` distinguished from wrong password (`rust/crates/host/src/identity.rs:484-533`, `:553-588`).
- **Session tokens.** 32 CSPRNG bytes → 43-char base64url; only `HMAC-SHA256(key, "session\0"+token)` is stored; 8 h absolute expiry + 30 min idle window in the same `UPDATE … RETURNING` that validates user-active and user-version (`packages/identity-pg/src/sessions.ts:104-132`; `rust/crates/host/src/identity.rs:317-375`; schema `packages/identity-pg/migrations/001_identity.sql:10-16`).
- **Logout / password change invalidation.** Logout deletes the presented session row (`sessions.ts:137-151`; `identity.rs:394-418`); password change bumps `version` **and** deletes all sessions in the same transaction (`sessions.ts:152-178`). The Rust logout route authenticates with CSRF first, then revokes, then expires the cookie (`router.rs:1805-1850`).
- **Cookie flags — TS.** `HttpOnly; SameSite=Strict; Path=/`, `Secure` only on HTTPS origin, `__Host-` prefix when secure, duplicate cookie names rejected (`apps/crm/src/host.ts:45-56`, `:92`).
- **CSRF.** Every TS POST must carry the configured `Origin` and an HMAC-derived `x-archava-csrf` bound to the session token (`apps/crm/src/host.ts:100-101`, `:165-169`; `identity-pg/src/sessions.ts:133-136`). All 23 Rust POST routes call `authenticate(..., csrf_required=true)`, including `/login` and `/logout` (verified per-handler; e.g. `router.rs:1006`, `:1484`, `:1601`, `:1805`). Only GET/POST are routed; all other methods fall through to 405/404, so there is no unguarded state-changing verb.
- **Tenant scoping / RLS.** Tenant is client-selected from the path or query but is never trusted alone: every repository opens a transaction, sets `archava.tenant_id` transaction-locally, and re-reads `lock_membership(tenant, principal)` which itself asserts `wanted_tenant = current_setting('archava.tenant_id')` (`packages/subscriptions-pg/src/repository.ts:140-202`; function `migrations/001_subscriptions_entitlements_usage.sql:134-139`). All domain tables are `ENABLE`+`FORCE ROW LEVEL SECURITY` with `USING(tenant_id=current_setting('archava.tenant_id',true))` and matching `WITH CHECK` (e.g. `accommodation-pg/migrations/001_room_night_stays.sql:153-176`); an unset GUC yields NULL and therefore zero rows (fail-closed). Rust wraps this structurally: `TenantTransaction::begin` refuses a non-owner/BYPASSRLS role, sets the GUC, reads it back and compares it on every `checked_executor()` (`rust/crates/adapters/src/rls.rs:443-470`).
- **No RLS bypass in the serving path.** Startup in every `*-pg` repository asserts `NOT (rolsuper OR rolbypassrls OR schema owner)` and counts forced policies before serving (`subscriptions-pg:103-135` and peers). The runtime role is `archava_crm_app` with `-c row_security=on` (`apps/crm/src/settings.ts:79-92`); `archava_operator` is used only for migrations, backup capture and module-lifecycle checks (`apps/crm/src/operator.ts:228-260`, `backups.ts:244`, `server.ts:122`), never to answer a request.
- **Native CRM actions.** Every review/present/status/cancel path resolves the acting human through `service.access` with a service principal on-behalf-of and requires `contacts.read` + `followups.read` (+`followups.write` for writes) before doing anything (`packages/crm-act/src/native-host.ts:60-71`, `:295-309`). Attempt records are session-scoped by RLS (`archava.act_session` set server-side in `packages/crm-act/src/postgres.ts:78-90`), so `/actions/followup/{id}/cancel` cannot reach another operator’s intent.
- **CRM receipts (the model implementation).** Actor + initiating human bound, per-receipt kind→permission mapping, and a resource re-check (`packages/crm-pg/src/repository.ts:247-283`). Support does the same (`support-pg:339-375`) — which is precisely why the seven domains in F-04 stand out.
- **Error hygiene / transport.** Uniform refusal bodies, no SQL or principal data in errors (`apps/crm/src/host.ts:561-611`), 16 KiB body cap (`:696-708`), no CORS headers, CSP/HSTS/nosniff/referrer-policy (`:687-695`).

---

## 4. Limits of this audit

- **No runtime proof.** Nothing was executed against PostgreSQL or a live router; F-01…F-07 are proven by code reading, not by a reproduced 200/403 pair. F-01–F-03 match a defect that _was_ reproduced on 2026-10-20, so the runtime behaviour is well supported; the TS variants (F-04) have **not** been exercised end-to-end.
- **Route counts are static.** “250 route entries” counts distinct method+pattern handlers in source; it does not include per-area branches that share one regex, nor `apps/web` (whose only endpoint, `POST /api/quote`, is an intentionally public pricing calculator with no tenant data). Counting method: 44 host entries + 156 domain entries + 50 Rust `router()` entries.
- **Scope enforcement semantics.** F-06/F-07 assume `scope='assigned'` means “only rows owned by the principal”. That is how subscriptions, assets, capacity, organizations, proposals and field-services implement it, but no product spec was read that forbids a broader interpretation for billing/attendance/laundry.
- **Permission vocabularies differ per domain.** A domain with no `*.read` permission cannot be “missing” one; F-05 flags only domains whose list/detail surface already checks a read grant while status does not.
- **Not audited:** interactive XSS/CSRF beyond origin+token checks, the Rust CRM kernel (`/commands`, `/approvals`) beyond its session/CSRF wrapper, supply-chain and build reproducibility, and the `Archava Template/` tree (vendored, out of scope).
- **Working tree state.** The repo has uncommitted changes on branch `feat/laundry-1.17.0`; line numbers refer to the working tree as read on 2026-10-21 and may drift after the next commit.

---

## 5. Recommended order

1. **F-01/F-02/F-03** — add the read grant + initiator binding to the three Rust status handlers; add negative HTTP tests (member with `{audit.read}` must get 403, not `recorded`). These are the literal F03 reopen.
2. **F-04** — same fix across the seven TS repositories; reuse `crm-pg`’s receipt code as the template. Cover with one negative test per domain in the existing domain suites.
3. **F-05** — align Rust organizations/projects/proposals/scheduling status with their own list/detail gates.
4. **F-06/F-07** — enforce `assigned` scope in billing/attendance/laundry reads and in subscriptions’ entitlement/usage sub-resources.
5. **F-08/F-09** — Rust cookie `Secure`/body-token and per-peer login bucketing.
