# Manual bank evidence and cash matching

Accounting006–008 add manual bank registration, immutable statement imports,
reviewed links to posted cash entries and complete bank-period controls. This
bounded profile executes no external bank transfer. Supplier settlement and
complete financial/subledger close acceptance remain separate work.

## Authority and setup

A human selects the managed company, its native official book and an existing
active `asset` account classified as `cash`, reviewed by ID and revision. Payment
clearing retains its separate classification. Installing the migrations creates
no bank account, opening journal or historical import and gives existing
memberships no additional grants.

Every command requires `books.read`, `bank.read` and its action grant:

| Command | Action grant | Effect |
| --- | --- | --- |
| `bank_account.create` | `bank.configure` | Register the reviewed cash account |
| `bank_statement.import` | `bank.import` | Record complete manual statement evidence |
| `bank_allocation.record` | `bank.reconcile` | Match exact same-direction statement and posted-cash remainders |
| `bank_allocation.reverse` | `bank.reconcile` | Append one correction while retaining the original match |
| `bank_policy.set` | `bank.configure` | Set an explicit opening basis and reviewer policy |
| `bank_reconciliation.confirm` | `bank.reconcile` | Attest the complete current period evidence hash |

Session/current membership/company/book determine authority. Assigned scope,
revocation, human restrictions, FORCE RLS and writer fencing remain effective.
`books.read` alone cannot read bank rows or IDs. Status/replay requires current
grants and the original command actor.

## Statement and matching contract

The API uses canonical decimal **strings in book currency minor units**.
Incoming rows are positive; withdrawals negative. Ambiguous signs, `-0`, leading
zeros, fractions, exponents, whitespace and values outside
±9,000,000,000,000,000 are refused. No currency conversion or rounding is inferred.
Native fields accept human currency decimals and preserve exact wire strings.

A statement supplies its reference, dates, currency, opening/closing balances
and every transaction. Up to 1,000 rows are accepted; excess input is refused.
Each row has its original bank reference, real date, signed amount and optional
description. References are unique across the bank account, including separate
imports. Opening plus the complete row sum must equal closing; gross totals use
wider exact arithmetic. Windows cannot overlap, adjacent balances must agree,
and each import fits within one configured open financial period.

Importing records evidence only. Same-key retries recover the original receipt;
changed contents conflict. Late row additions and updates/deletes are refused.
Statement correction/replacement remains separate work; an overlapping import
cannot hide the original evidence.

Matching requires a posted same-book/bank cash leg, current account revision,
observed journal revision and cash-line ordinal. An allocation cannot exceed
either remainder or change direction. Book locking serializes competing matches.
Reversal appends immutable history, frees the amounts for a reviewed replacement,
and requires the statement and journal periods to be open. It adds no journal.

Native review shows account names, human currency amounts, complete import
totals/rows, and the selected posted journal. Technical references are collapsed.
Current authority and remainders are checked again on save. The first complete
native journey failed on unmatch acknowledgement; the
[native finding](../product/PRD_BANK_NATIVE_FINDING_2026-11-09.md) is preserved.
The repaired source has [bounded local native acceptance](../product/PRD_MANUAL_BANK_ACCEPTANCE_2026-11-11.md),
including receipt recovery, independent review and a 101-row save. This does not
accept supplier settlement or the full financial profile.

## Reviewed bank periods — Accounting008

`bank_policy.set` explicitly chooses the first open financial period and whether
an independent reviewer is required. Earlier imported statements cannot be
excluded. Setup can be corrected before the first confirmed reconciliation;
its opening basis becomes sealed after that review. No date is inferred from
account creation time. Reopen an affected later closed period before introducing
an earlier reconciliation basis.

The mounted `GET /erp/accounting/bank-reconciliation` read checks all statement
days, opening/closing bank versus posted GL balances, every statement remainder
and every posted cash leg. Fully posted exact same-period journal reversal pairs
can resolve unallocated ledger-only corrections; unrelated offsetting entries
cannot. Wider totals remain strings. Ordered complete evidence is SHA-256 bound.

`bank_reconciliation.confirm` requires observed policy and period revisions
**and the exact evidence hash reviewed by the human**. Importers cannot confirm
under an independent-review policy. Immutable audit/receipt claims bind that
reviewer and hash. Native controls show human currency balances, coverage and
exceptions, guard asynchronous responses against changed selection/company/
session, and offer an explicit refresh. Current native/browser evidence is scoped
in the [manual-bank acceptance](../product/PRD_MANUAL_BANK_ACCEPTANCE_2026-11-11.md).

Close requires every registered bank to have an explicit basis and each active
bank to match a current attestation. Commit-time checks prevent late changes
within the closing transaction. Backdated cash affecting a later reconciled
closed period requires reopening it first. Reopening changes the review revision;
changed evidence needs a fresh review. Replaying an old confirmation preserves
its historical receipt and never reviews the changed current control.

Three owned router/PostgreSQL cases for this increment passed, followed by all
**36 Accounting integration tests**. They cover half-month/29-day gaps, independent
review, forged database attestations, unchanged net cash with unresolved entries,
explicit reversals, stale hashes, closed backdating and reopening. This source
is separate from every earlier native acceptance.

Two independent offline consumers of the packaged Core/Domain crates then passed
**12 contract tests**, including large exact statement totals, duplicate references
and negative allocation remainders. Their current source is
`ce17f01bb23979bea2091808ea9cabd1ac355d49ae839b6a2fbeb3057740f33a`;
Domain archive SHA-256 is
`f8d95c72ad1202741443b5f1a607a415fe63afaedb38ee4ac6b88e2abe5b9eb2`.
This accepts the portable public contracts; browser, installed-role upgrade and
encrypted recovery of the seven bank tables remain pending for this source.

The repaired source
`efbedbf4324eb23795f46120b8b19ecedd1f23b1919cf89c3850d392620cfa5f`
adds the receipt's optional `originalBankAllocationId`. Current status/replay
derives a missing legacy reference from the immutable unmatch, checks any stored
reference against that evidence, and leaves the original stored receipt/audit
unchanged. Native acknowledgement binds the original allocation and new unmatch;
bank confirmation also binds its reviewed hash. Five focused receipt tests and
12 fresh packaged-consumer tests passed. The new Domain archive SHA-256 is
`1ce366c52bb0cac000f8efa03a7c8298713f4140af92b8466e8ff5088781422d`.
The complete current browser gate then passed 204 checks, 77 product probes and
30 Ava checks, followed by two independent persistent clients and 11 native reuse
checks. Restore preserves all 26 financial tables, including seven nonempty bank
tables. Actual old-binary recovery derives the missing reference with no stored
history change or duplicate unmatch. The form saves 101 rows and enforces the
1,000-item draft bound. See the dated acceptance for source, process/log evidence
limits and remaining profile obligations.

## Earlier checkpoint and remaining acceptance

At Accounting007, close only blocked unmatched imported amounts. Its fixture
could close after matching a half-month statement; that evidence proves only
the earlier row guard. Accounting008 adds the bank controls above. Complete
subledger close and fresh native acceptance remain separate requirements.

Bank collections and the new journal `cashLegs` field were not added to the
provider's closed disclosure list. Broader sharing selections do not automatically
accept these fields. Natural-language bank planning, CSV/vendor formats,
statement corrections, AP/AR settlement, terms/aging and localization remain work.

At the earlier Accounting007 checkpoint: eight domain units, five bank
router/PostgreSQL cases and all
**33 Accounting integration tests** passed. They cover exact/duplicate/overlapping
imports, replay, read permissions, immutable evidence, draft/posted distinction,
independent journal posting, partial matching, corrections and racing allocations.
That checkpoint did not accept native/browser/upgrade/restore of its five bank
tables. Accounting008 adds two more tables. Older 19-table native proofs do not
cover either bank increment.

Sources: [pure rules](../../crates/domain/src/erp/accounting/bank.rs),
[persistence](../../crates/adapters/src/erp/accounting/bank.rs),
[router/PostgreSQL tests](../../crates/host/tests/erp_accounting/bank.rs),
[native review](../../crates/slice/src/operations/accounting/bank.rs), and
[form contracts](../../config/operator-actions.v1.json).
