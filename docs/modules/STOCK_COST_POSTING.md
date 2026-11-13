# Original stock cost posting

Accounting migration 005 and Commerce migration 010 connect future fulfillment
and returns to immutable acquisition costs. The bridge uses an explicit native
IDR book, its reviewed whole-Rupiah purchase origin and a separately configured
stock cost policy. Installation creates no book, policy, opening balance or
financial journal.

`stock_policy.set` binds the current book, currency catalogue and reviewed
`cogsAccount` revision. It uses `books.write` and an optimistic policy revision:
zero means the first policy. A configured policy keeps Accounting, Commerce and
Purchasing enabled. First setup, later policy changes and company trade share
the same company-before-book transaction fence.

Future acquired layers retain the original inventory account from their actual
purchase source. Migration attaches that metadata to existing captured purchase
sources across tenant scopes; it does not post or rewrite their money. Opening
stock without an approved financial origin needs a separate opening/import
workflow. Unknown cost never becomes zero.

Fulfillment captures the actual cost portions, original inventory accounts,
reviewed COGS account and UTC source date in the same operating transaction.
Restocking uses the original recognized fulfillment COGS account, even if the
current policy has changed. An exact immutable return-cost/use reference links
new returned layers. Historical return layers without that reference stay
operationally consumable, but their accounting origin is never guessed.
Discard does not replenish inventory or reverse COGS. Enabling the policy after
an old fulfillment does not invent its prior financial recognition.

`stock_source.draft` accepts only a captured source ID, journal code, all current
reviewed account references and a reason. The portable Domain engine and the
SQL guard independently derive the exact original account allocations. The
server compares them before drafting. Source amounts, effects, book and actor
cannot be supplied as caller claims. Stock costs use exact decimal strings;
conversion from operating whole Rupiah to the book's ISO minor units is checked
before capture. Sources support at most 10,000 portions and 100 effective
accounts; oversized operations refuse atomically before creating an unusable
source.

Saving creates one live ordinary journal draft per monetary source. Posting is
a separate action and follows the book's independent approver policy. Replay
and receipt recovery recheck current financial and company authority. Source
links, snapshots, receipts and audits are immutable. Reserved source references
and independent journal reversal cannot detach accounting from physical stock
corrections.

The workspace returns bounded source summaries, exact amounts and the complete
account allocation, rather than thousands of raw cost portions. Native review
shows the recognized cost change, source date, original accounts and proposed
debit/credit before the closed technical reference disclosure. Reports count
captured Billing, Purchasing and Commerce sources. A pending monetary source
prevents period close, and a source in a closed period rolls back its complete
operating transaction. This is captured-source close control; full inventory
subledger reconciliation, bank reconciliation and statutory close remain
separate requirements.

Runtime startup verifies the capture/link/immutability guards, private origin
writer privileges, exact return provenance and writer fences. Runtime users may
read original layer metadata; only the guarded source binder can create it.
The router/PostgreSQL scenarios are in
[stock_sources.rs](../../crates/host/tests/erp_accounting/stock_sources.rs), and
the portable calculation is in
[stock_sources.rs](../../crates/domain/src/erp/accounting/stock_sources.rs).
The [scoped native acceptance](../product/PRD_STOCK_POSTING_ACCEPTANCE_2026-11-08.md)
binds the current rebuilt server/WASM, browser review, migration/recovery and
one-company source-cooperation evidence. Its limits remain explicit.

Counted stock adjustments, approved financial opening/import, full as-of
inventory reconciliation and other currencies are outside this bridge's current
accepted profile. They remain delivery work; synthetic acquisition/return tests
do not certify every business model or jurisdiction.

## Inventory control comparison

`GET /erp/accounting/inventory-control?tenant=<tenant>&bookId=<id>&asOf=<date>` compares, per inventory account of a native book, what posted sources explain with what the ledger holds through the date. It requires `reports.read` and writes nothing.

| Field | Meaning |
| --- | --- |
| `controls[].expectedMinor` | posted receipts (goods plus landed fee) minus posted fulfillment layer costs plus posted restock costs, on the inventory account each source captured |
| `controls[].actualMinor` | debit minus credit of every posted journal line on that account |
| `controls[].differenceMinor` | actual minus expected |
| `postedSourceControlsAgree` | every difference is zero |
| `coverage.pendingMonetarySourceCount` | captured receipts and stock cost sources dated through the cutoff that carry money and have no posted journal |

The accounts listed are those named as `inventory` by a purchase policy, by a captured purchase order or by a stock layer origin. A posted [declared opening balance](ACCOUNTING.md#declared-opening-balances) is added to the expected balance and reported as `openingMinor`. A counted adjustment or a manual journal on an inventory account is not a captured source and appears as a difference.

This is a control-account comparison, not a stock valuation: it does not read the commerce cost layers, so stock consumed without a captured stock source (for example a fulfillment before the stock policy existed) leaves ledger and expected balance equal while the physical layers are gone. A dated valuation of layers against the ledger is not delivered. The Accounting screen shows it under Financial reports as "Inventory accounts against posted stock sources".

Covered by `inventory_control_compares_posted_stock_and_receipt_sources_with_the_ledger` in [stock_sources.rs](../../crates/host/tests/erp_accounting/stock_sources.rs): two receipts on two inventory accounts, a pending and then posted fulfillment, a posted restock, an earlier cutoff, a manual journal shown as a difference, and the refusals.
