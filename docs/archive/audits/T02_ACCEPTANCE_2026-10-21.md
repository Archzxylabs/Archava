# T02 / A10 (P2) — assigned scope on parent + child reads

Date: 2026-10-21
Scope: `packages/billing-pg`, `packages/attendance-pg`, `packages/laundry-pg`, `packages/subscriptions-pg`

## Finding

Audit A10 (P2): every read path resolved `access.scope` / `access.ownerId` from the
transaction envelope and then ignored it. Under `assigned` scope a reader could list
another owner's invoices / sessions / orders / subscriptions, open their detail, and
read every child row (lines, corrections, attempts, events, rosters, check-ins, process
events, findings, entitlements, usage events).

## Change

Pattern, applied identically in all four repositories:

1. **List** — the owner predicate is pushed into the parent `WHERE` **before** the
   `LIMIT/OFFSET` placeholders are appended, so pagination counts only visible rows.
2. **Detail** — the parent lookup carries `AND owner_id=$3`; a parent the reader does
   not own resolves to `null` / `not_found` _before_ any child query runs.
3. **Child reads** — each child method is protected independently by joining its
   tenant-scoped parent and (under `assigned`) carrying `AND p.owner_id=$3` inside that
   same join. A foreign parent and a missing parent are indistinguishable: both yield
   zero rows instead of an error that would confirm the parent exists
   (`packages/billing-pg/src/repository.ts`, `childRowsSql` doc comment).
4. `all` scope preserves the pre-change result set, ordering and page envelope: parent
   list and detail statements keep their exact previous text (no predicate, no extra
   parameter), and child reads gain only the tenant-scoped parent join the assigned path
   already needs — same rows, same `ORDER BY`, no extra parameter.

### billing-pg

- `listInvoices` — `owner_id = $n` before `LIMIT`.
- `getInvoiceDetail` — parent gated on `owner_id`; children only read afterwards.
- `listLines`, `listCorrections`, `listAttempts`, `listEvents` — join
  `archava_billing.invoices p` on `tenant_id` + `invoice_id`, `p.owner_id=$3` under
  `assigned`.

### attendance-pg

- `listSessions`, `getSessionDetail`, `listRoster`, `listCheckIns` — same shape over
  `archava_attendance.sessions`.

### laundry-pg

- `listOrders`, `getOrderDetail`, `listLines`, `listProcess`, `listFindings` — same shape
  over `archava_laundry.orders`.

### subscriptions-pg

- `listEntitlements` UUID branch: parent probe now
  `SELECT id ... WHERE tenant_id=$1 AND id=$2 AND owner_id=$3`; a foreign subscription
  raises the same `not_found` a missing one does.
- `listEntitlements` contact-reference branch: `EXISTS (SELECT 1 FROM
archava_subscriptions.subscriptions p WHERE ... AND p.owner_id=$3)`. `EXISTS`, never an
  inner join, because `entitlements.subscription_id` is nullable — an inner join would
  silently drop subscription-less entitlements under `all` scope.
- `listUsageEvents`: two-hop `EXISTS` through `entitlements` → `subscriptions`, so a
  foreign entitlement returns an empty page (not an error) and a subscription-less
  entitlement stays unreachable under `assigned`.
- `listSubscriptions` / `getSubscriptionDetail` already enforced assigned scope
  (repository.ts:663-668, 705-707) and were left untouched.

## Tests added

Each new suite is a scope proof, not a mock rehearsal: the fake tables evaluate the
predicate the emitted SQL actually carries, so deleting an owner predicate makes the
assigned reader see the other owner's rows and the suite fails.

- `packages/billing-pg/test/scope.test.ts`
- `packages/attendance-pg/test/scope.test.ts`
- `packages/laundry-pg/test/scope.test.ts`
- `packages/subscriptions-pg/test/scope.test.ts`

Coverage: two owners with populated children; assigned reader vs all-scope reader;
foreign tenant rows; foreign UUID answered exactly like a missing one; owner predicate
before pagination (page 2 content and page size); detail hidden before any child query
(asserted against the client query log); delegated service principal inheriting the
human owner and narrowed scope; read grant still enforced before the scope filter.

## Acceptance

### Gate 1 — unit/integration suites

```
$ pnpm exec vitest run packages/billing-pg/test packages/attendance-pg/test \
      packages/laundry-pg/test packages/subscriptions-pg/test
 Test Files  10 passed (10)
      Tests  98 passed (98)
GATE1_EXIT=0
```

### Gate 2 — offline library pack + verification

```
$ pnpm library:pack
Packed 62 compiled internal packages in dist/library/tarballs.
PACK_EXIT=0

$ for d in billing attendance laundry subscriptions; do
    TMPDIR=/tmp npm_config_offline=true node tools/library/verify-$d.mjs || exit 1
  done
billing: 30 PASS
attendance: 18 PASS
laundry: 30 PASS
subscriptions: 19 PASS
LOOP_EXIT=0
```

### Gate 3 — typecheck

```
$ pnpm typecheck
GATE3_EXIT=0   (0 `error TS` lines)
```

## Observation (not fixed here)

`listEntitlements` selects its branch with `isUuid(targetId)`, while
`entitlements.contact_id` is validated as a UUID by `entitlementSchema`. Any value that
could match a stored contact id therefore routes to the subscription branch, so the
contact-reference branch is unreachable through the public API today. It is included in
the ownership fix and covered by a statement-contract test, but the branch itself is a
candidate for a separate dead-code/API cleanup card.
