# Supplier payments from actual bank evidence

Accounting 009 adds an explicitly reviewed manual supplier-payment profile. It
records an actual imported bank withdrawal against a posted, current supplier
bill. It sends no money. The existing native book, purchase-source, immutable
bank-evidence and independent posting policies remain authoritative.

`payable_payment.draft` binds a captured bill, its matched document version, the
original payable account, a current cash account, an actual negative bank row
and an exact positive amount. The journal date comes from the bank row. Its two
lines debit supplier payable and credit bank cash. Partial payments reserve both
the bill and the unmatched bank amount while their draft remains live.

Posting is a separate `journal.post` review. Voiding an unposted draft releases
its reservation. A posted payment cannot be erased with `journal.reverse`.
`payable_return.draft` records a real positive bank row against the original
posted payment and its original accounts. Partial returns are bounded by that
payment and other live return drafts. Only a posted return restores bill
capacity. Posting neither creates nor reverses a bank allocation automatically.

A settlement's cash leg may match only its reviewed bank row. Other journals
cannot consume the portion held by its live reservation. Actual allocations are
deducted from the reservation so the same amount is not counted twice. Unmatching
restores the reservation without undoing the posted supplier payment. Bill void
and payment creation serialize on the book; a source correction rolls back if
there is still a live payment reservation or an effective posted payment.

The human workspace exposes paged `payables` and `payable_settlements` collections.
Current summary totals use every captured bill, independently of page size.
Operational void alone does not hide an unposted financial correction. Amounts
are decimal strings in book minor units. Absent terms and due dates remain
unknown. The summary is current and does not follow the financial report's date
filter.

New commands require `payables.read`, `payables.write`, `bank.read` and
`journals.write` as well as book/company access. Posting additionally uses the
existing `journals.post` permission and independent actor policy. A poster needs
payable and bank read access; it need not hold payable drafting authority. New
permissions are not granted to existing memberships by migration. Settlement
details require payable and bank read access; generic journal detail and audit
snapshots do not acquire the subordinate bank-row reference. Provider projection
withholds the new raw collections by default.

Implementation: [domain rules](../../crates/domain/src/erp/accounting/payables.rs),
[atomic persistence](../../crates/adapters/src/erp/accounting/payables.rs),
[SQL guards](../../migrations/accounting-pg/009_supplier_bank_settlements.sql),
[HTTP/transaction tests](../../crates/host/tests/erp_accounting/payables.rs), and
[native review](../../crates/slice/src/operations/accounting/payables.rs).

The [dated native acceptance](../product/PRD_SUPPLIER_PAYMENTS_ACCEPTANCE_2026-11-11.md)
records the source-bound local browser, installed-role, upgrade, recovery and
public-client results. It does not establish full AP/AR subledger
close, opening/import acceptance, aging, advances, supplier credits, fee/FX
settlements, statutory filing, automated bank feeds, or bank-transfer execution.
The [delivery register](../product/PRD_DELIVERY_REGISTER.v1.json) retains the wider
financial requirements as partial until their separate acceptance is complete.
