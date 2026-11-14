# Manual customer receipts

A customer who pays an invoice by bank transfer or in cash has their invoice settled by
a **manual receipt**: a person's statement that the money arrived. It is not verified
by a bank or a payment provider and is never labelled as verified. It flows through the
chain a verified gateway collection already uses: Billing settlement → `collection`
source → reviewed journal (clearing against receivable) → clearing settlement against an
imported statement row → bank cash. Billing stays the only writer of `settled_minor`
and `outstanding_minor`.

Sources: [domain](../../crates/domain/src/erp/billing.rs),
[adapter](../../crates/adapters/src/erp/billing.rs),
[Billing 013](../../migrations/billing-pg/013_manual_customer_receipts.sql),
[Accounting 020](../../migrations/accounting-pg/020_manual_receipt_sources.sql),
[invoice screen](../../crates/slice/src/operations/receipts.rs),
[tests](../../crates/host/tests/erp_accounting/manual_receipts.rs). Related:
[Billing](BILLING_MODULE.md), [payments](../architecture/PAYMENTS.md),
[invoice posting](INVOICE_POSTING.md), [clearing](GATEWAY_BANK_CLEARING.md).

## Commands

Both are Billing commands (`POST /erp/billing/commands`), need `billing.settle` and a
human in a session (a service principal is refused), raise the invoice revision by one
and follow the usual receipt, replay and audit contract. Neither carries a signature;
the signature check of `record_provider_event` is unchanged.

| Command | Members |
| --- | --- |
| `record_manual_receipt` | `invoiceId`, `expectedRevision`, `amountMinor` (positive, in the invoice's unit and currency), `method` (`bank_transfer` or `cash`), `receivedOn` (`YYYY-MM-DD`), `reference` (1–100 characters: the payer's or bank's reference, or the cash receipt number), `reason` (1–500) |
| `reverse_manual_receipt` | `invoiceId`, `expectedRevision`, `receiptEventId` (the payment event of one manual receipt), `amountMinor` (positive), `reason` |

A receipt runs the settlement arithmetic of a succeeded provider event: the invoice must
be issued, a part payment leaves the rest outstanding, the invoice is settled at zero
and an amount above what is outstanding is refused. A reversal runs the refund side: it
reopens the invoice by its amount. It reverses all or part of one manual receipt of the
same invoice, the reversals of a receipt never exceed it, and a gateway event or another
reversal cannot be reversed. The invoice row is locked, so two concurrent receipts that
together exceed what is owed end with one recorded and one conflict.

## What is stored

A payment event (`archava_billing.payment_events`) with outcome `succeeded` (receipt) or
`refunded` (reversal), provider id `manual:bank_transfer` or `manual:cash`, and a
server-generated provider event id `manual:<event id>`. Billing 013 adds five columns,
null on every provider event:

| Column | Meaning |
| --- | --- |
| `manual_method` | `bank_transfer` or `cash` |
| `received_on` | The UTC calendar date the money was received (receipt only) |
| `reference` | The payer's or bank's reference, or the cash receipt number (receipt only) |
| `reason` | Why the receipt or the reversal was recorded |
| `reverses_event_id` | The manual receipt a reversal reverses |

The person is the event's `recorded_by` and the time of recording its `verified_at` (the
column keeps its name; for a manual event it is when the person recorded it). Columns on
the event were chosen over a companion table: one insert carries the event and its
facts, the existing append-only trigger makes them immutable, the ledger reads the
received-on date from the row its receipt names, and no table, policy or grant is added.

The `manual:` provider namespace is reserved. A submitted `record_provider_event` naming
it is refused at the parse (400), the provider arm refuses it for a caller that builds
the command itself, and the database refuses an event in that namespace without the
manual facts.

## Dates

`receivedOn` is a UTC calendar date ([decisions](../architecture/TAX.md#decisions-taken-after-review-2026-11-14)):
not after the day the command is recorded and not before the day the invoice was issued.
A reversal is dated the day it is recorded.

## Online payment links

A manual receipt is refused (`manual_receipt_open_payment_link`) while the invoice has a
gateway link that is not closed and has received nothing: the customer could still pay
the frozen amount online and settle the same money twice. A link closes when the gateway
reports it paid, failed or expired (check its status on the invoice). The other
direction needs no rule: a link is created for the invoice's outstanding amount, so a
manual part payment followed by a link for the remainder works, and the gateway's
allocation never exceeds what is outstanding.

## Ledger

For a company whose book has an invoice posting policy, Accounting 020 maps the two
actions like provider events: a receipt is a `collection` source of its amount, a
reversal a `refund` source of the negated amount, both without tax. **The source date of
a manual receipt is its `receivedOn`**, so a transfer received yesterday and entered
today is dated yesterday and can be settled against yesterday's bank row
(`clearing_settlement.draft` requires a row dated on or after the source date). A
reversal's source date is the day it is recorded. A receipt dated in a closed period of
the book is refused at entry with the existing reason `invoice_in_closed_period`, and the
billing command is rolled back; reopen the period or date the receipt in an open one.

Drafting (`invoice_source.draft` with the receivable and the payment clearing account),
independent posting, the clearing settlement, receivable positions, aging and the
control comparison are unchanged. The clearing source row carries `origin` (`manual` or
`gateway`) and `method`, and the clearing review words a manual source as manual.

## Cash

A cash receipt follows the same chain: it sits in the payment clearing account until a
clearing settlement moves it to a cash account against a statement row. There is no
evidence-free cash. Today a cash drawer is handled by registering it as a bank account
(`bank_account.create` on a cash account) and entering its statement rows by hand
(`bank_statement.import`). A dedicated cash book is future work.

## Reads and the invoice screen

Each invoice of the Billing workspace carries `issuedOn` and `payments`: every payment
event, oldest first, with `origin` (`manual` or `gateway`), `method`, `providerId`,
`outcome`, `amount`, `recordedAt`, `recordedOn`, `receivedOn`, `reference`, `reason`,
`recordedBy`, `recordedByName`, `reversesEventId`, `reversedAmount` and
`verifiedByProvider` (false for a manual event). The workspace also returns `today`, the
UTC date of the database clock.

The invoice detail lists the payments with their origin (payment gateway, manual · bank
transfer, manual · cash), date, reference, reason and recorder. A person with
`billing.settle` gets "Record a payment received" on an issued invoice that still owes
(amount in the invoice's currency defaulting to the outstanding amount, method,
received-on date defaulting to today UTC, reference, reason) and a reversal form per
manual receipt with something left to reverse. Without the permission no form is
rendered; the server refuses the commands either way.

## Refusals

| Code | When |
| --- | --- |
| `manual_receipt_not_issued` | The invoice is a draft or void |
| `manual_receipt_exceeds_outstanding` | The amount is more than is outstanding, or the invoice is already settled |
| `manual_receipt_future_date` | `receivedOn` is after today (UTC) |
| `manual_receipt_before_issue` | `receivedOn` is before the day the invoice was issued (UTC) |
| `manual_receipt_open_payment_link` | The invoice has an open, unpaid gateway link |
| `manual_reversal_not_manual` | The named event is a gateway event, a reversal, or belongs to another invoice |
| `manual_reversal_exceeds_receipt` | The amount is more than what is left of the receipt after its reversals |
| `invoice_in_closed_period` (existing) | `receivedOn`, or the day of a reversal, lies in a closed period of the book |
| `manual_receipt_incomplete` | Database only: a manual event without its facts, not recorded by an active person with `billing.settle`, not in the invoice's currency or not at the transaction's clock |

A zero or negative amount, an unknown method, a date that does not exist and a blank
reference or reason are refused at the parse (400). A stale `expectedRevision` and a
reversal larger than what the invoice has settled are plain conflicts; an event id that
names nothing is not found (404). A provider event that overpays keeps its plain
conflict.

## Database authority

`guard_manual_receipt()` (trigger `payment_event_manual_guard`, before insert) holds a
direct insert by the serving role to the same rules as the command, and the row check
`payment_event_manual_facts` ties the facts to the `manual:` provider id of their own
method and to the event's own id. The facts cannot be changed or removed: the serving
role holds no `UPDATE` or `DELETE` on `payment_events`, and `events_immutable` refuses
the table owner. The guard does not prove that the invoice's settled amount moved with
the event; that pairing is the command's, as it is for provider events.

Billing has no trigger readiness of its own at startup (`assert_billing_kernel` checks
the runtime role, the migration checksums and forced row security, and only the test
suite calls it). The tax guards of Billing are covered by Accounting's startup readiness
(`tax_document_guards_complete()`, Accounting 019); the two triggers above are covered
the same way by `archava_accounting.receipt_guards_complete()`, which the host checks at
startup with the other Accounting guard functions.

## Not covered

One receipt across several invoices, customer credit or unapplied cash, overpayments,
credit notes, a cash book, bank-statement auto-matching, receipts recorded by an API key
or service, and a browser journey. Not exercised in a browser.
