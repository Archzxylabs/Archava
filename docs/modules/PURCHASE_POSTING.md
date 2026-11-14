# Native purchase source posting

Implementation is under local integration validation. The earlier native package
and [Ava recheck](../product/PRD_AGENT_CONTEXT_RECHECK_2026-11-08.md) predate this
bridge and do not accept it. No actual company book or historical balance is
created by this work.

A current authorized human explicitly configures `purchase_policy.set` on a native
company book with the pinned currency catalogue and the book's
[source unit](ACCOUNTING.md#source-units). On an IDR book that is `whole_idr`: one
operating Rupiah becomes exactly 100 book minor units. On a book in any other
currency it is `minor`: unit costs, landed fees and supplier totals are already the
currency's ISO minor unit and post unchanged. A purchase order in another currency
than the book cannot be confirmed while the book has a purchase policy. Four distinct reviewed
active account mappings are required: inventory (asset), goods received but not
invoiced (liability), supplier payable (liability) and landed fee accrual
(liability); each has operating cash-flow classification. The policy has a CAS
revision and immutable audit/receipt history. Setup and subsequent updates require
current company scope and `books.write`. External declarations and guessed units
refuse. Module setup requires Purchasing, Commerce and Accounting enabled; active
capture makes those dependencies mandatory through current Owner preview/apply.

When a company PO is confirmed, an immutable origin pins that policy and revision.
A company transaction advisory fence is acquired even if its first book/policy
has not been created. Policy setup acquires the same fence before any book row
lock. Confirmation already owns its PO lock and then takes company/book locks.
Orders confirmed before setup stay outside implicit capture: receiving or matching
them later does not import historical stock or debt. A later policy update never
changes the original order's mappings, including between partial receipts.

| Actual operating effect | Captured source and reviewed journal |
| --- | --- |
| Accepted receipt | Original accepted quantities and frozen PO unit costs plus allocated landed fee are checked against original purchase inventory layers; debit inventory, credit GRNI for goods, credit separate fee accrual for the fee |
| Successful exact goods claim match | Immutable matched document version and actual accepted-receipt allocations; debit GRNI and credit supplier payable for the goods claim only |
| Void of matched claim | Release allocations and retain an equal signed AP/GRNI countereffect atomically; physical received inventory and its original cost remain |
| Held/mismatched claim, capture or revision | Operational review evidence; no invented payable effect |
| Entirely zero monetary source | Explicit no-money source and no artificial zero journal |

Accepted/rejected receipt identity, final Purchasing command receipt and its
immutable operation audit are bound to the source. Its date is the trusted audit
instant in UTC, separately from the vendor's documentary date. This is a recorded
financial-event contract; jurisdiction, cutoff, backdating and tax acceptance
remain explicit requirements. Amount components and combined acquisition amount
are bounded in exact integer book units. Overflow or a closed source period
refuses the entire operating transaction, including stock and allocations.

`purchase_source.draft` resolves a visible captured source, its original policy,
exact amount, source date and only the two or three used account roles. The human
reviews current account revisions; a separately authorized independent approver
posts when the book requires it. Source journals use the reserved
`purchasing:<source>:<code>` identity, one live journal per source and a bound
financial command/audit/receipt. A void draft may be reviewed again with a new
code; posted sources cannot be detached through an independent journal reversal.
Correct the originating supplier document. Current authority and source/book
scope apply again on replay and status. Financial data remains withheld from
provider context.

Accounting workspace exposes scoped paged purchase policies and sources, exact
string amounts, original policy revision and pending/draft/posted/no-money state.
Native forms review goods/fees and mappings before saving. Purchasing's own
`accountingStatus: not_inferred` does not disclose or guess financial state.
Reports include every captured invoice and purchase source through their end,
independently of a screen page. Closing refuses unposted nonzero sources, including
pending corrections. This is **not full AP, bank or inventory subledger close**:
GRNI, fee evidence, due terms, aging, outbound supplier settlement, COGS/returns,
opening imports and reconciliation still need their complete controls.

[Accounting migration 004](../../migrations/accounting-pg/004_purchase_source_posting.sql)
is additive; earlier SQL bytes stay unchanged. Its four additional tables use
FORCE RLS and the existing dynamic Control writer fence. Startup independently
checks exact purchase capture/immutability/review/close/reversal trigger bindings.
The manifest, runtime grants, native embeds and fixture profiles must agree.
Old profiles retain their original tables/history and require explicit upgrade;
there is no automatic setup, source backfill or financial grant.

The [HTTP/PostgreSQL integration suite](../../crates/host/tests/erp_accounting/purchase_sources.rs)
is separate from the earlier unmounted prototype. That prototype exercised
23 owned operating API/SQL scenarios, including policy ordering and first-book
races, with explicit administrative policy/draft setup. It does not substitute
for mounted command authorization, full regression, fresh native/browser/recovery
proof or real-user acceptance of this implementation.
