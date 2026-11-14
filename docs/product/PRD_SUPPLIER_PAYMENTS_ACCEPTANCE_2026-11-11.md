# Manual supplier payment acceptance — 2026-11-11

The named local profile records actual imported supplier withdrawals and returned
payments against posted original bills, with separate journal posting and bank
matching. This is bounded acceptance; it does not complete the financial PRD,
real-model Ava quality, customer acceptance or production deployment.

The accepted shipping source is
`6b67fab8be6f01b9215015d110ecbe4a2a698b965a7abb38ff9c7665dc346ab5`
(531 public inputs). The native distribution is
`dist/native-candidate-20261111-prd-supplier-payments-02`; its release-profile SHA-256
is `b350cb9f3f09c9fdcf0374f55ac7c4fc0d52306d9b60cb461f9df3bcc78f185d`.
All 85 packaged artifacts verified. Gate, build and packaging process exits were
captured as zero in `target/prd-completion-20261107/ap-release-job-02.json`.

- The full gate passed 1,387 Rust tests across 99 suites, 36 Node tests, formatting,
  all-target Clippy with warnings denied, and both WASM boundaries. No tests were
  ignored. All 42 Accounting integration tests passed, including six new payment
  tests covering reservations, concurrency, returns, current authority, SQL claim
  integrity, tenant policies and startup guard metadata.
- Focused Chromium acceptance passed. Native forms recorded a 10,000 IDR payment,
  a 2,000 IDR actual bank return, and a 6,000 IDR final payment. Independent SQL
  proved three settlement records, net paid 14,000 IDR, AP zero, cash 6,000 IDR
  after the explicit 20,000 IDR opening, and no unmatched imported amount. The
  human summary showed the same exact balances. Desktop and mobile review images
  were inspected; both journal legs and money remained readable.
- The complete browser gate passed 208 checks, all 77 observed product criteria,
  current deterministic Ava acceptance, restart, owned provider transports and
  TLS checks, with no browser exceptions and disposable-PostgreSQL cleanup zero.
- Five read repetitions passed the local latency targets: Contacts p95
  2.98–4.01 ms; Commerce p95 15.04–16.29 ms. Two hundred writes had p95 4.95 ms,
  200 matching receipts and no errors. These are local synthetic measurements,
  not production sizing or a competitor comparison.
- Encrypted recovery preserved exact row counts and digests for all 27 financial
  tables, including three nonempty supplier settlements and all seven bank
  tables. The native restored server served successfully with external effects
  held. This does not establish production RTO/RPO or offsite recovery.
- Actual prior-native upgrades applied seven and five declared migrations. They
  preserved existing rows and receipts, refused the older executable against the
  new profile, and rehearsed isolated encrypted rollback. No supplier history or
  payable authority was invented for existing members. Fresh installed owners
  created, posted, matched and replayed an actual supplier payment with the real
  non-owner runtime role and no administrator grant repair.
- An actual defective legacy bank executable upgraded through one new migration;
  old unmatch status and replay recovered the original allocation reference while
  its stored receipt/audit stayed unchanged. This is an append-only upgrade,
  rather than the earlier same-SQL-profile repair.
- Two cold public-crate consumers passed 12 pure tests. Two independently built
  persistent clients passed their configured native checks and cleaned their
  scratch databases. Core archive SHA-256 remained
  `91e849f5e33453556e1d73c1c6b18e6ad2a1b2fd6036764468e8c3236fc5769b`;
  Domain was `29bdd8e8f343d7d931b1e15f37f88ab074e33ea83e1dce4b5097bb8bf9046e50`.

Evidence is under `target/e2e/prd-payables-focused-20261111-02`,
`target/e2e/prd-payables-20261111-02`, and the `ap-*-02` results in
`target/prd-completion-20261107`. Durable validation job 03 captured focused,
broad, cold and persistent exits as zero; its separate real-Gemini run exited
one for the [CRM planner finding](PRD_CRM_LABEL_PLANNER_FINDING_2026-11-11.md).
Consequently the combined job correctly reports `completed_with_failure`.

The first focused helper failed by reading the final summary immediately after
navigation. Its transaction and SQL assertions had passed. The corrected helper
waited for the exact recognized, paid, outstanding, reserved and available values
and retained the SQL proof before checking the UI. The failed run and original
helper remain preserved; shipping code did not change to make that test pass.

Gateway clearing/fees, complete B01 cooperation, formal credit/correction
documents, terms/aging, opening/import controls and full subledger close remain
separate requirements. See [the module contract](../modules/SUPPLIER_PAYMENTS.md)
and [delivery register](PRD_DELIVERY_REGISTER.v1.json).
