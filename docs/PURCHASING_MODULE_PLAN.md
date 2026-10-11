# Purchasing implementation plan

KIT-07 continues after bounded support acceptance with the shared `purchasing`
capability. Wholesale, restaurant ingredient buying and light-production consumers
need supplier orders and physical receipts against the same commerce stock ledger.
This plan accepts no procurement, invoice, warehouse or full operating kit yet.

[ERPNext purchase-order documentation](https://docs.frappe.io/erpnext/purchase-order)
and [purchase receipts](https://docs.frappe.io/erpnext/purchase-receipt) distinguish
ordered goods, physical receipt and billed quantities. Archava's initial proposal
below is a separate bounded policy, with no copied ERP implementation or implicit
accounting/valuation acceptance.

`purchasing` should own supplier identity, versioned backend supplier offers,
purchase headers/frozen lines, partial goods-receipt evidence, memberships,
commands and immutable audit. Commerce remains the only owner of physical stock.
A controlled private transaction SPI must append inbound movements and their
receipts/audit together with the purchase receipt; no adapter writes another
module's tables directly or maintains a second stock balance.

The backend prices draft lines from configured supplier offers in immutable
product base units. Supplier unit cost is distinct from selling price and ARCHZXY's
company pricebook. LLMs never invent prices or supply totals. Confirmation freezes
the quote; later supplier-offer changes affect future orders only. Native initial
currency remains integer IDR, with explicit arithmetic and total bounds.

A confirmed order can receive partial actual quantities. Cumulative receipts may
not exceed ordered quantities. Same UUID/intent increases stock once; overlapping
receipts serialize revisions and product locks. Rejected/damaged quantities do
not enter usable stock silently. Cancellation/short closure prevents future
receipts without erasing previous physical movements. A received order is not a
paid supplier invoice, payment verification or financial posting.

Use current independent purchasing/commerce memberships and scope, human-only
writes/recovery, private read tools, strict inputs and same-transaction resource
checks. Source-bound receipts and quote snapshots must survive restart/fresh
restore. An additive commerce migration/SPI must preserve the archived non-empty
previous release and all generic sales/restaurant behavior.

Acceptance requires two independently installed business consumers, actual
PostgreSQL duplicate/partial/revision races, overflow and over-receipt refusal,
atomic cross-domain rollback, current grants, physical stock/audit integrity,
restart/restore and real compiled owner/Chromium use. Inventory valuation, taxes,
payables, bank/provider effects, multi-warehouse/lot/expiry, foreign units/currencies,
supplier communication and regulated purchasing remain additional requirements.
The complete kit programme and its external/domain-review gates stay open.
