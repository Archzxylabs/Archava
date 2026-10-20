# Rust migration decisions — ADR log

Per PRD §24, decision log. `proposed` until the milestone that proves it.

## ADR-R01 — Scope: backend, library, and first-party web UI

**Status:** `proposed` (owner-selected per execution task §1: "Owner sudah memilih
target sampai UI Rust").

Rust covers the reusable core/library, business modules, backend host,
first-party web UI, SDK boundaries and operational tooling. PostgreSQL stays the
system of record. The Python template backend is retained behind an explicitly
mapped compatibility facade rather than ported.

Consequence accepted: two build targets (server + `wasm32-unknown-unknown`),
WebAssembly shipping alongside HTML, and vendor SDKs that are
JavaScript-oriented being wrapped rather than reused.

## ADR-R02 — Modular monolith over microservices

**Status:** `proposed`.

Same PostgreSQL schemas and ledgers, raw SQL adapters, explicit transaction
cohorts. No new service boundaries, no new datastore. The migration changes the
implementation language of the application layer, not the data architecture.

## ADR-R03 — Leptos SSR + Axum for the web UI

**Status:** `proposed`, contingent on R01 evidence. R01 produced partial
evidence: it resolved the framework landscape but did not build a screen, so
the decision stays open. Revision history below.

Leptos renders HTML on the server and hydrates in the browser from Rust.
Dioxus remains the documented alternative and is only reconsidered with
feasibility evidence showing Leptos cannot meet an approval, recovery,
accessibility, or media contract.

Leptos server functions become public endpoints, so every endpoint still needs
authentication, authorization, validation and request protections. Rust does not
remove that obligation.

### R01 evidence — the landscape, measured with `cargo search`

The original framing of this ADR was written before the versions were measured,
and the measurement changed the shape of the problem:

| Framework  | Latest measured   | Stability         | SSR / hydration                               |
| ---------- | ----------------- | ----------------- | --------------------------------------------- |
| `leptos`   | **0.9.0-rc0**     | release candidate | yes, isomorphic                               |
| `dioxus`   | **0.8.0-alpha.2** | alpha             | yes, fullstack                                |
| `yew`      | **0.23.0**        | **stable**        | **yes — SSR since 0.20, WASI SSR since 0.22** |
| `sycamore` | 0.9.4             | stable            | hydration documented                          |
| `askama`   | 0.16.1            | stable            | server templates only, no hydration           |
| `maud`     | 0.27.0            | stable            | compile-time templates, no hydration          |

The ADR's assumption that Dioxus was the live alternative no longer matches the
registry: Dioxus measures an _alpha_, which is a weaker stability position than
Leptos's RC, not a stronger one. **Yew is the candidate that actually clears the
bar the RC was blocking on** — a stable release with SSR and hydration that
vendor docs date back several minor versions, not a new feature.

**This is documentation evidence, not a feasibility proof.** No Yew screen has
been built here, so nothing about Yew is `locally_proven`. Two things are known
and one is a real cost: Yew's documented hydration contract requires the client
Virtual DOM to match the SSR HTML _exactly_, including placeholder components for
browser-only components (`PhantomComponent`), and non-spec-compliant HTML can
silently break hydration when the browser repairs it. Against 14 browser panels
that is a maintenance hazard worth naming rather than discovering at R06.

**What the R01 gates actually showed** is that the split Yew needs is already
enforced in this workspace: `archava-domain` compiles for `wasm32-unknown-unknown`
while the host does not, so browser-renderable policy and server-side authority
are separated by the toolchain rather than by convention. That is a prerequisite
for any SSR framework and it is met.

**Decision trigger, so this does not stay `proposed` forever:** R02 builds one
real screen — the approval dialog, since it is the one with an authority gate and
a confirmation prompt, i.e. the two things that make an SSR framework earn its
complexity — under both Leptos and Yew, and records measured hydration
correctness, wasm bundle size and build time for each. The measured winner
becomes this ADR's decision. If Leptos 1.0 or a stable 0.9 ships first, the RC
objection lapses and the comparison narrows.

**What is not negotiable either way:** whichever framework is chosen, every
server function it exposes is a public endpoint requiring authentication,
authorization, validation and request protections, and the R01 `rejection()`
handler's no-reflection rule applies to every extraction path it introduces —
not just `Json`, but `Path` and `Query` too. A framework does not inherit the
host's safety properties; it has to be shown not to break them.

## ADR-R04 — Toolchain and dependency versions pinned at R01

**Status:** `proposed`, due at R01.

Toolchain: `rustc`/`cargo` 1.96.0 (measured), targets
`x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown` (both installed).
Dependency versions resolve from the live registry (the `axum` 0.8.x line,
current `sqlx`, current `leptos`) and are frozen in `Cargo.lock` at R01 rather
than carried forward from the pilot's pins. Documentation `latest` is research
material only; production pins are chosen after a compatibility check.

Advisories are recorded, not suppressed: the existing 13 moderate
OpenTelemetry-chain template findings travel into the Rust manifest verbatim if
they remain applicable.

## ADR-R05 — Cutover safety, required before any cutover

**Status:** decided and implemented at R10; proven by `P-UPGRADE-RESTORE-RUST`
(five fixture-backed rehearsal witnesses) and `P-CLI-OPERATIONS` (the
runbook's rows executed through the built binary). See the R10 record at the
end of this file for the five concrete decisions.

Writer fencing, pending-work ownership, rollback compatibility window,
supported prior profiles, and an exception inventory. Not relevant until R10,
but recorded now because R00–R09 design choices either make it possible or
foreclose it.

## ADR-R06 — The pilot stays in `Experiments/`

**Status:** `proposed`.

`~/Archverse/Lab/Experiments/rust-pilot` is a non-production experiment under
the workspace rule that experiments stay out of the product tree. The production
Rust workspace introduced at R01 lives inside this repository and is a separate
artifact. No pilot crate is promoted by being copied; each port into the product
workspace is re-verified there.

## ADR-R07 — Security boundary does not move

**Status:** decided.

Authorization is enforced by native PostgreSQL row-level security and by domain
permissions, not by the application language. A Rust port therefore inherits
authority correctness tests as _requirements_, not as solved problems. This is
why G03 is tested at real entrypoints and against a real database rather than
satisfied by Rust's memory safety.

## ADR-R08 — The cold consumers keep the repo's directory depth, and split `deps/` from `vendor/`

**Status:** decided, forced by two hard toolchain constraints rather than preference.

**Context.** G05 and TEST-06 require two independent cold consumers that build
from the shipped artifacts with no product workspace link, no absolute machine
path, and no registry access. Building them produced two constraints that no
amount of layout preference could have avoided.

**Decision 1 — the scratch tree mirrors the repository's own layout.**
`<scratch>/examples/rust-consumers/...` and `<scratch>/config/`, at the same depth
as the real repo, with the pricebook copied to the same relative position.

Both consumers locate the pricebook by climbing exactly four levels from their own
file. That is not an accident of where they were first written: `include_str!`
resolves relative to the _source file_ (D10), and Node's
`new URL(..., import.meta.url)` behaves the same way. So the depth is load-bearing
in both languages. Lay the scratch tree out shallower and consumer A no longer
compiles against the file it was written against; lay it out deeper and it silently
reads a _different_ file — which is worse, because it passes. Keeping the
committed consumer runnable against the scratch copy unmodified is the entire
claim, so the layout has to be preserved rather than flattened for tidiness.

**Decision 2 — shipped crates live in `deps/`, the third-party closure in `vendor/`.**
Two path-dependencies to unpacked `.crate` directories, plus a vendored
source-replacement for the crates.io closure.

`cargo vendor --offline` writes a `.cargo-checksum.json` beside every crate it
copies and _requires_ one for everything in the vendor directory. An unpacked
`.crate` tarball has no checksum — it is a file handed to a consumer, not a
registry entry — so the product's own crates cannot live in `vendor/` at all. And
they cannot be omitted either: the consumer needs them, offline. The only
arrangement satisfying both is to separate them, with the vendored-sources
replacement applying to the registry closure alone. 15 third-party crates vendored,
0 archava crates in `vendor/`, 2 in `deps/`.

**Consequence.** The consumer manifest is self-contained and its own lockfile
decides the serde/serde_json versions; `[workspace]` is declared empty so no
parent workspace can be discovered and pull the product's lockfile in. A consumer
that needed its third-party crates from the machine's `~/.cargo` cache would be
passing on dependencies a clean machine would not have.

**What this does not decide.** Whether the other five crates are consumable this
way. Two are packaged; seven exist. Nothing here should be read as a percentage.

**Addendum — the vendored-sources replacement is scoped to the consumer, and
nothing above authorises a repo-root `.cargo/config.toml`.** Recorded because the
mistake was made and cost a product build, so that the fix is not mistaken for the
rule.

While probing the staleness gate, a `cargo vendor` iteration was run directly
against the committed consumer instead of the scratch copy. It left 126 MB behind
inside `examples/rust-consumers/crate/` (`target/`, `vendor/` with 16 crates,
`deps/`, `Cargo.lock`, `.cargo/`) and a `.cargo/config.toml` at the **repo root**
containing the source-replacement block from decision 2. That root config made
cargo redirect crates-io for the _product_ to a `vendor/` directory that does not
exist there, and the product Rust workspace stopped resolving outright:
`cargo metadata --offline --manifest-path rust/Cargo.toml` exited **101**
(`failed to load source for dependency serde`). It was removed — backed up to
`/tmp/quarantine-cargo/config.toml`, sha256 `77e9219c…` — and the workspace
re-verified with `cargo build --offline` → 0 before anything else was trusted.

Two things follow, and only one of them is this ADR's business.

The script was not the source. `tools/library/verify-rust-consumers.mjs` uses the
repo root only for reads — package directory, addon, pricebook, evidence, the
staleness walk — and every cargo/test invocation is a scratch path. The residue
came from a manual probe of mine, not from a run of the gate. Recorded here anyway
because "the script is clean" is exactly the sentence that lets a recurrence look
harmless.

What is fixed is the staging hazard rather than the cause. `examples/rust-consumers/crate/.gitignore`
now covers the six generated paths, so a `git add examples/` cannot silently add
126 MB of build output and sixteen vendored crates to the index. That is a fence
around one specific mistake, not a guarantee it cannot recur: a future probe run
in the wrong directory will again produce files git is silent about, and the
checks a reviewer would notice are the ones mtime and `git status` still show.

## ADR-R09 — Two Rust gates, and the full one compares itself to the handoff list

**Status:** decided, and the second half of it was forced by a defect in its own
first draft.

**Context.** §1.1 item 1 requires both a fast regression gate wired into regular
repo/CI verification and a full candidate gate that includes controlled benchmarks
and the required end-to-end/upgrade/restore proofs. It also rejects the failure
mode directly: _"menambahkan scripts yang tetap hanya diingat untuk dijalankan
manual belum cukup"_. Before this decision, three Rust checks had no invocation at
all (`cargo fmt`, `cargo clippy`, `cargo test`), two proofs had one each, and
`compare.test.mjs` was written for `node --test` and then never run.

**Decision 1 — the split is `pnpm rust:verify` and `pnpm rust:verify-release`.**
The fast gate (`tools/rust/verify.mjs`) is format, clippy, `cargo test`, the
node-side gate tests, both parity harnesses and both cold consumers: about a
minute, wired into `tools/scripts/verify.mjs` and CI's verify job, so it runs on
every change. The full gate (`tools/rust/verify-release.mjs`) runs that gate and
then the platform proofs — benchmark, the 46-command security chain, browser
end-to-end, both cold-prior recovery paths, CRM benchmark — and writes
`dist/rust-release-report.json`. A gate that rebuilt an archived cold-prior commit
on every commit would spend minutes answering a question that only matters at
release time, which is why the split exists rather than convenience.

**Decision 2 — the fast gate owns artifact-build ordering.** The addon is built
first and the two parity harnesses and the cold consumers all read the same file.
The full gate invokes the fast gate as one step rather than re-implementing its
sub-steps, because duplicating that ordering is exactly what produces two
different artifact hashes for one build.

**Decision 3 — the full gate compares itself against the documented handoff list,
and never merely asserts it.** This is the part the first draft got wrong, so it
is recorded as a decision rather than quietly as a bug fix. The draft declared
`everyStepInHandoffList: true` as a literal. Nothing computed it, and it was false:
the gate ran `pnpm rust:benchmark`, which the list does not name, and omitted
`pnpm accommodation:verify --cold-prior`, which it does. A field asserting an
uncomputed fact is worse than no field, because a reader treats it as coverage.

`reconcileWithHandoffList()` now parses the list out of
`docs/LOCAL_LIBRARY_HANDOFF.md` at run time and compares it to the gate's own
steps. Every command the gate omits must appear in `DELIBERATELY_ABSENT` with a
reason; an omission without one throws, and so does a recorded reason that stopped
being true. Two of the gaps it records are removals of my own duplication, found by
measuring rather than assuming: `kits:verify` and `capacity:verify` are already
steps inside the 46-command chain (`tools/security/verify.mjs:45` and `:64`), so
running them again cost minutes and proved nothing.

**Decision 4 — the exit code follows the verdict, not the step count.** §1.1 item 1
requires the completion gate not to be green while required inventory or proof is
incomplete. `docs/rust/migration-inventory.json` carries aggregate counts and no
per-behavior item IDs, so §11 item 3's binding cannot be satisfied from the
inventory that exists. The report therefore says so — `requiredInventoryComplete:
false` with a `blockedBy` reason — and exits non-zero even with every step green.
Inventing item IDs to make the verdict reachable was rejected: an invented binding
looks like coverage, which is the failure the whole file exists to avoid.

**Decision 5 — `independent_review` is a separate field that no local run changes.**
Consequence, not preference: a gate that ran on this machine is not an independent
audit, and a report that merged the two would let a green run get quoted as an
approval. The field is `"pending"` in every branch, including the one that reaches
`candidate-verified`.

**What this does not close.** Wiring the gates makes a regression visible; it does
not make the Rust side complete. R01, R03 and R04–R11 are untouched by this
decision, and §1.1 items 2–6 (the p1 error contract, the authority boundary and its
PostgreSQL tests, the runnable vertical slice, the remaining milestones and OWN-V1)
are still open. The one thing it does close is the specific objection that a proof
existed but nobody ran it.

## ADR-R10 — The consumer closure has one pin: the committed `Cargo.lock`

**Status:** decided, after two of the obvious designs were measured and rejected.

**Context.** The evening audit of 2026-10-16 recorded the reproducibility gap
directly: _"temporary consumer registry closure still resolved with `cargo fetch`
without a lockfile; bootstrap success does not mean every dependency graph is
pinned identically."_ Bootstrap warmed the consumer closure with a bare
`cargo fetch` in a throwaway manifest — a fresh resolve of whatever the registry
index said at that moment — while the consumer proof (`pnpm rust:verify-consumers`)
separately ran `cargo generate-lockfile --offline`, resolving from whatever the
machine cache happened to contain. The two sides shared no identity. Neither was
pinned to anything.

Investigation while closing it found a second, sharper gap in the same claim. The
proof's `vendorClosure` checked that `cargo vendor` _printed_ a
`[source.vendored-sources]` configuration and its comment said "we write our own" —
but no write existed anywhere in the repo. With `vendor/` populated and no
replacement config, `cargo test --offline` builds out of the machine's registry
cache and `vendor/` sits unused. Measured: an empty `CARGO_HOME` and no config
fails with `no matching package named serde found`; the same tree with the config
written passes. Every cold-consumer result until now depended on machine warmth the
ADR-R08 claims deny.

**Decision 1 — the pin is `examples/rust-consumers/crate/Cargo.lock`, committed.**
It is the only artifact that knows which exact versions the proof will vendor, so
it is the only thing a reproducible fetch can agree with. The crate's `.gitignore`
drops `Cargo.lock` from its generated set (`deps/`, `target/`, `vendor/`,
`.cargo/`, `lib/` remain generated; the lock is source). Regeneration is
deliberate: materialize `deps/`, `cargo generate-lockfile --offline`, commit the
result — which is exactly what a manifest change should force.

**Decision 2 — bootstrap derives its fetch from that pin, and fetches `--locked`.**
`tools/rust/bootstrap.mjs` now parses the consumer manifest's registry
dependencies (the existing fail-closed parser) and derives a throwaway
manifest-plus-lock: the manifest repeats the declared dependencies verbatim —
requirements _and features_, because features are what make the resolved graph
equal the consumer's — and the lock keeps the header and every registry block
byte-for-byte (checksums included), strips the local packages, and rewrites the
root. `cargo fetch --locked` then either warms exactly the pinned closure or fails.
Three refusals guard the derivation, each naming its remediation: a declared
dependency the lock does not contain, a lock entry unreachable from the consumer's
root (stale), and a closure crate reachable only through a shipped path dependency
(a registry-only fetch manifest cannot express it — and silently fetching a subset
would starve the proof's vendor step).

The tempting shortcut — pin _every_ locked package as a direct dependency of the
throwaway — was implemented and measured wrong before this text was written: a
direct dependency takes its default features, so pinning `hashbrown` directly
pulled in `allocator-api2` from behind an optional feature the consumer never
enables, and `cargo fetch --locked` refused the rewritten lock. The declared set,
reached-through-edges design is not a preference; it is the one that resolves
identically.

**Decision 3 — the proof asserts the same pin, and proves the vendor path.**
`vendorClosure` runs `cargo generate-lockfile --offline --locked` (the committed
lock is asserted, not re-resolved — and `--locked` correctly refuses to _create_ a
missing lock, so a dropped pin fails loudly), vendors with `--offline --locked`,
and now writes the `.cargo/config.toml` its comment had always claimed existed.
Consumer A's tests run with `CARGO_HOME` pointed at an empty directory: with the
replacement in force and no cache to fall back on, every third-party crate must
come out of `vendor/`. The evidence records it as `vendoredSourceOnly: true`, a
field that can only be true because of that empty directory.

**Evidence.** `CARGO_HOME=/tmp/opencode/cold-cargo-adr10 pnpm rust:bootstrap` on an
empty cache: exit 0, 124 MB fetched, all three closures `--locked`. The derivation
has its own unit suite (`tools/rust/test/bootstrap.test.mjs`, 10 tests, in the fast
gate's `node --test` step): byte-faithful lock splitting, all three refusals, and
the real committed lock deriving into the pinned closure. `pnpm rust:verify-consumers`
exit 0 with the new assertions; consumer A 4 tests green from `vendor/` alone.

**What this does not close.** The pin covers the two crates the consumer pair
consumes; the other five shipped crates have no cold consumer and no closure to
pin. The lock's regeneration path still requires a warm machine (materializing
`deps/` and resolving offline), which is the same property the whole offline chain
has. And the addon consumer (`examples/rust-consumers/addon`) has no Rust
dependency closure at all — it consumes a copied `.node` file — so nothing here
pins its Node-side build.

## ADR-R11 — Server-side authority: identity port, bound approvals, scope rule

**Status:** decided, implemented, and proven by `P-AUTHORITY-PG`.

**Context.** Audit RA01's finding was structural: the pre-RA01 router trusted
the request body for authority. `CommandSubmission` carried `role`,
`capability`, `human_approved` and `enabled_action_ids` as deserialized fields;
the policy gate read them directly from the JSON. A caller could POST
`{"role": "owner", "capability": "enterprise", "human_approved": true}` and
pass every gate check without any database lookup. The gate was sound; its
input was not.

**Decision 1 — the request body is never authority.** `CommandSubmission` now
carries only the _target_ (tenant, action*id, request_id, revision) and the
\_narrowing signals* (available_on_page, confirmed, declined, approval_id).
Every authority fact — role, capability, scope, active, approver identity — is
resolved from a membership row the server reads under a tenant-scoped
transaction. A body carrying the legacy fields parses (they are unknown, dropped
silently) and changes nothing.

**Decision 2 — human approval is a bound, single-use database row, not a
boolean.** Migration `004_bound_approvals.sql` creates
`archava_commerce.approvals` with a foreign key to `memberships`, a CHECK
enforcing `(approved_at IS NULL) = (approver_id IS NULL)`, and a trigger that
makes the binding fields (tenant, actor, action, input_hash, revision,
expires_at) immutable once the row exists. Consumption moves NULL → value once,
and only on an approved row. The host cannot un-approve or un-consume an
approval even by bug — the trigger refuses it at the database level.

**Decision 3 — approver eligibility is structural, not role-based.** A service
principal cannot approve regardless of its role or capability. The check is
`kind = 'human'`, evaluated before the approval is looked up, so an ineligible
caller learns nothing about which approval ids exist. Self-approval is refused
by comparing `actor_id` to the approver's principal_id.

**Decision 4 — the scope rule is `SCOPE_GATE_LEVEL = L3`.** Actions at L3 and
above require `scope = 'all'`; an `assigned` scope is insufficient. This is the
same rule the TypeScript baseline enforces, ported verbatim.

**Decision 5 — identity is a port of `packages/identity-pg`, not a rewrite.**
The Rust identity module uses the same scrypt parameters (`N=32768, r=8, p=3`,
32-byte output), the same HMAC-SHA256 session digest format, the same CSRF
derivation (`HMAC(key, "csrf\0" + token)`), and the same login rate buckets
(source: 60/300s, account: 5/900s). The encoded password form
`scrypt$32768$8$3$<salt>$<key>` is byte-compatible with the baseline, so a
hash created by the TypeScript seeder verifies in Rust and vice versa.

**Decision 6 — the P-AUTHORITY-PG producer mirrors P-READINESS-PG.**
`tools/rust/pg-authority.mjs` parses the test harness output for each of the
eight required checks BY NAME, so a deleted proving test is a visible gap
rather than a green step with no producer. The witness map is frozen in
`docs/rust/required-coverage.json` (`expect.witnessTotal: 8`) and synced
against the script by `verify.test.mjs`.

**Consequences.** The router's seven routes (`/healthz`, `POST /login`,
`POST /commands`, `GET /commands/status`, `POST /approvals`,
`POST /approvals/approve`, `GET /contacts`) all resolve authority from the
database. The in-crate cold tests prove ordering (extraction before session);
the fixture-backed `tests/authority.rs` proves the authority itself — forged
bodies, revoked grants, consumed approvals, cross-tenant reads — against real
HTTP and real PostgreSQL.

## ADR-R12 — Operational cutover safety delivered at R10 (ADR-R05 closed)

**Status:** decided, implemented, and proven by `P-UPGRADE-RESTORE-RUST` +
`P-CLI-OPERATIONS` + `P-RELEASE-IDENTITY` (the named-witness runners in
`tools/rust/release-ops.mjs`, the rehearsals in `crates/ops/tests/upgrade_restore.rs`).

**Context.** ADR-R05 named five things a cutover could not do without:
writer fencing at the persistence boundary, pending-work ownership, a rollback
compatibility window, the set of supported prior profiles, and an exception
inventory. None of it existed as code at proposal time — the TS operator side
had `settings.json` workspace locks and its own `upgrade.mjs` orchestration,
and the Rust side had nothing: upgrades/provisioning were dev fixtures, backups
were a TS surface, and the "is this tree one release identity" question was
answered by Node scripts.

**Decision 1 — the fence lives in the same transaction as the write, not in a
file.** `archava_control.writer_lease` carries a monotonically advancing
`epoch`; a writer proves freshness with
`SELECT epoch … FOR UPDATE` in the transaction that does the write. A stale
writer is refused where its write lands (`LeaseError::StaleWriter`), no matter
how long ago it took the seat. The control schema is installed by OPERATOR
tooling (an explicit ownership statement), never by a product migration — the
migration set is frozen in the release identity and operational tooling must
not edit it (the same rule keeps `archava_control` out of every ledger's
`schema_migrations` history).

**Decision 2 — the prior supported profile is a REAL database, not a flag.**
`pg_fixture::provision_prior_commerce_profile` provisions every frozen ledger
with the commerce ledger applied through `003_purchase_receipts` only — the
recorded history is a strict prefix of the frozen ledger, which is exactly
what makes the subsequent full apply a LEGAL continuation (the runner's
checksummed-history assertion consumes the prefix) rather than a rewrite. The
prior-profile upgrade rehearsal seeds a real membership row, applies the
pending migrations twice (idempotence), and asserts the 004 addenda
(`role`/`capability` least-privileged defaults) are the ONLY difference on the
preserved row.

**Decision 3 — a backup is not valid until its isolated restore was proven,
and the restore refuses its own source.** `capture` publishes digests and
counts, never values (`credentialValuesPrinted: false` is a report field with
nothing a secret could flow into); the restore REFUSES the capture's own
host:port/database (`RestoreError::ActiveSource`) and an undrained writer
lease (`RestoreError::UndrainedWriter`) before any data moves, replays
`pg_dump`-marshal by `pg_restore --clean --if-exists` into a DISPOSABLE
instance, and the rehearsal asserts the captured and restored rows are
byte-identical. Ledger contents are FORCE-RLS protected, so capture/replay
run with the cluster-admin role — administration of the instance, not an
authority assertion.

**Decision 4 — the operator runbook is a REGISTRY the gate executes.** The
replacement table (`archava_ops::runbook::table()`) is machine-readable and
`release-ops.mjs` runs every row — five CLI rows through the built binary,
the rehearsal row through the cargo suite — so "operator commands replaced
with Rust equivalents" is a claim about executed rows with recorded exits,
not about a document that says so.

**Decision 5 — the release identity is verified by recompute, not by copy.**
`archava-ops verify-release` reads the CONFIG source of truth
(`config/library-release.v1.json`), recomputes every artifact digest from
`dist/library/tarballs`, and scans every `dist/releases/<version>/` slot for
identity consistency — an archive whose version does not match its directory
name, or whose slot claims the CURRENT identity, is a recorded refusal rather
than an exception. P-RELEASE-IDENTITY's evidence is the recomputed record.

**Consequences.** The five upgrades/restore/fence witnesses are fixture-backed
and run per release-gate invocation (~5s); the ops binary ships with the
workspace; the fence's refusals are typed (stale writer, drained seat,
undrained restore target, active capture source) and the rehearsals name the
class they expect. The cutover itself — flipping serving traffic from the
Node runtime to the Rust host — remains pending Archxy 2's domain lanes
(§LEGACY); what this ADR closes is the safety machinery that makes the flip
survivable, which is what R10 was scoped to deliver.


## ADR-R13 — OWN-V1: the acceptance-family interpretation (disputed), then the actual lifecycle (locally re-delivered, T01–T10)

**Status:** **REOPENED AND LOCALLY RE-DELIVERED (20 Oktober 2026).** The
19 Oktober 2026 CRM audit (`docs/ARCHXY_1_CRM_AUDIT_2026-10-19.md`, finding
C03) and the active mandate `ARCHXY_1_FULL_IMPLEMENTATION_TASK.md`
§4/A1-OWN-03 disputed the decision below; its interpretation — that the
OWN-V1 product behaviour is satisfied by an acceptance family over existing
surfaces — was NOT accepted. The audit's counterexample stands: `T01` is
derived from a policy-consumer `outcome.ok`, `T06` from writer-fence/rollback
rehearsals, `T07` from deployment-plan unit tests and `T10` from the
TypeScript CRM browser battery — none of which exercise an owner
configuration apply/restore in the product. An implementator ADR cannot lower
an owner requirement, and this one did not get to decide that. As of that
audit OWN-V1 was **not delivered**; the 20 Oktober 2026 addendum below records
the actual lifecycle and the local re-delivery (see
`docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md`, row C03). The old producer and its
ten checks remain as history under their true scope.

**Context.** `docs/OWNER_CONFIGURATION_PRD.md` §9 freezes the bounded
first-owner release: a known owner runs inspect → plan → preview → confirm →
applied → recovery/history/restore, editing only supported configuration. The
frozen manifest carries the ten proofs (T01–T10) with their required checks.
The question the lane had to answer: does OWN-V1 mean NEW product code, or the
acceptance family over what already shipped?

**Decision 1 — OWN-V1 is the acceptance family over released surfaces, not a
new product surface.** Every T0x grades a REAL run against machinery this
program already shipped and the gate already exercises: the two
business-model kit consumers from packed artifacts (T01 via the K05/K04
runner), the plan-preview contract suites (T03 via `buildDeploymentPlan`'s
named witnesses), the real-PostgreSQL authority suite (T04 via `pg-authority`),
the ops binary's executed refusals (T05), the fixture-backed rehearsal suite
(T06/T08 via `upgrade_restore.rs`), module-gating semantics (T07 via the
deployment-plan refusals plus the two independently installed ERP module
consumers), and dual-engine pricing parity (T09). The lane invents no new
readiness numbers: every grade is a boolean over a recorded, recomputable fact.

**Decision 2 — the chat/dashboard plan equivalence is an in-crate witness.**
The one genuinely NEW behavior witness is T02's
`chat_flow_and_direct_planner_resolve_the_same_plan` (in
`crates/assistant/src/conversation.rs`): a capturing planner records the
EXACT plan value the chat flow verified and executed, and for every recorded
utterance that value must equal — byte-for-byte JSON — the dashboard path's
direct `LocalCrmPlanner` answer, with the reply carrying the contract's own
planner identity. "Both paths resolve plans through the same contract" is a
consumed-plan fact, not a naming coincidence.

**Decision 3 — T10 stays a real browser journey and may not be faked.** The
PRD's own words bind the browser proof to a real driver; the gate step runs
the CRM host's browser battery (headless Chromium over CDP), whose harness
itself refuses any runtime home outside `/tmp/archava-crm-runtime-*` — the
"no owner code/file/terminal steps" check is graded from the harness's own
scope assertion plus the run's exit.

**Decision 4 — the driver guards the manifest in BOTH directions.**
`tools/own/verify.mjs` refuses to run if any P-OWN-Txx's `requiredChecks`
drift from its frozen map — a manifest edit without a producer change (or the
reverse) is an announced drift, not a silent pass. Evidence is written per
proof on EVERY run (`dist/own-evidence/`), red or green, so a failing run
replaces the previous green claim.

**Decision 5 — the kit-template lock pin is realigned, not hand-waved.** The
foundation-crate tracking commit (5bde51c) changed the shipped-manifest set
(`serde_json` entered `archava-core`'s dependency list), which left the
committed kit-template `Cargo.lock` one line behind the pin the K05/K04
runner asserts (`generate-lockfile --offline --locked`). The lock was
regenerated from the current manifest set — the exact one-line realignment —
so the pin asserts a CURRENT construction instead of failing on a stale one.

**Consequences.** The family is executable per release-gate run (suite
~10–15 min, browser ~5 min); each proof's evidence file names its facts so an
audit can recompute the claim without re-reading this document. OWN-V2's
custom-stages/typed-fields scope stays OUT (the PRD's own boundary: strict
enums do not grow custom stages without an explicit data-model extension).

**Addendum — 20 Oktober 2026: the lifecycle exists; OWN-V1 locally re-delivered.**
The audit's counterexamples are closed by product code and named witnesses:

- The engine, persistence, routes and UI are `rust/crates/own` +
  `rust/crates/host/src/own.rs` + `/app/owner`; `archava_owner` carries its own
  grants, forced RLS, revisions, apply receipts and audit.
- T02's chat is the owner-configuration chat at `POST /own/chat`, not the CRM
  assistant planner named in Decision 2. It parses the utterance into the SAME
  typed `Request` and calls the SAME `preview_at`/`catalogue().plan` path as
  `/own/plan`, deriving the base revision from the persisted config. The host
  witness `the_chat_plan_equals_the_dashboard_plan_and_applies` proves plan
  hash/baseConfig/catalogue/pricing identity, applies the chat's plan, and
  proves relabel parity.
- T10's browser journey is the owner journey in `tools/rust/browser-slice.mjs`:
  inspect → plan/preview → confirm → applied revisions 1–4 → refresh recovery
  → history → restore r1 as r3 → re-enable r4, with four
  `archava_owner.config_audit` rows by `alice` and the record pipeline hidden
  while disabled. It is NOT the TS CRM battery Decision 3 described.
- `cargo test -p archava-own` → 13/13; `cargo test -p archava-host --test own
  -- --test-threads=1` → 11/11 (the 8 earlier witnesses plus T02, public-session
  rejection and foreign tenant/role/approval refusal).
- The K05/K04 consumers now run the real CRM flow: seven source bundles, two
  isolated PostgreSQL fixtures, 6/6 witnesses each (five policy + the C04 CRM
  read/write/recovery witness), identical deps/code/migration digests.
- `tools/own/verify.mjs` is re-bound to these Rust/browser witnesses; the old
  TS/vitest grades are retired.
- Decision 5's template lock is superseded: the six-crate template cannot ship
  a usable lock before `deps/` is materialized. The producer resolves the lock
  from the bundled manifests and freezes it with `cargo vendor --locked` /
  `cargo test --locked`.

This is LOCAL verification at candidate `3754217` + the C03/C04 worktree.
Independent Codex sign-off and the full release gate at the eventual HEAD
remain pending; the honest status is `locally verified`, not
`integrated verified`.

## ADR-R14 — the frozen manifest's artifact paths resolve from the ROOT; the benchmark's binary path was authored against the wrong base

**Status:** corrected at the first full release gate after OWN-V1; proven by
the corrected manifest writing `P-ARTIFACT-PROOFS` and `P-BENCHMARK` evidence
(the next release-gate run).

**Context.** The RN01–RN08 round froze
`docs/rust/required-coverage.json`'s two benchmark-proof artifact lists with
`"rust/target/release/archava-bench"`. Every OTHER proof's artifact list uses
repo-root-relative paths (`rust/target/release/libarchava_native.node`,
`rust/target/package/…`), and `writeEvidence` resolves artifact entries with
`join(root, path)` — root-relative by construction. But the compiled consumer
lives in the benchmark lane's OWN nested workspace
(`tools/rust/benchmark/rust/`, built by that lane's `compileRust()` and
asserted alive by its vitest artifact suite at exactly that directory): no
root-workspace member declares a bin named `archava-bench`, so the frozen
path could never exist. The defect was invisible until the first full
release-gate run since the round: all 17 steps passed, but `writeStepProofs`
refused both writes ("artifact missing at write time"), the coverage
validator then counted the two proofs as missing, and 39 scope items went
"blocked" on a phantom.

**Decision — correct the two frozen strings to the files the producers
actually produce** (`tools/rust/benchmark/rust/target/release/
archava-bench`), and record the correction instead of silently rewriting:
the freeze rule guards against scope growth and quiet check-drops, not
against a declared path pointing at a file the ecosystem never writes. The
required checks, producedBy lines, proof kinds, scope bindings and digests'
source lists are untouched — this is a path realignment, the manifest's
 analogue of the kit-template lock realignment (ADR-R13 Decision 5).

**Consequence.** The validator's "artifact missing at write time" refusal is
re-honored: a future run whose nested binary vanishes STILL refuses loudly,
at the path the producer is actually responsible for.

## ADR-R15 — frozen tallies record their authoring moment; a tally must equal the producer's graded truth

**Status:** closed at the third full-gate repair pass after OWN-V1; proven by
the three producers emitting their tallies (exact deep-equal verified
standalone against the frozen expects) and the drift pins green
(`verify.test.mjs` 25/25, `verify-release.test.mjs` 24/24). One intermediate
record in this ADR — correcting the module-consumers tally `4 → 10` in
commit b141a24 — was itself refuted by the drift pin
`the P-MODULE-CONSUMERS witness map and the frozen manifest cannot drift
apart` and reverted in this commit; that catch is the pins doing exactly
what they exist for, and it is recorded here rather than erased.

**Context.** The RN01–RN08 round froze `expect` tallies —
`witnessTotal`/`namedRegressions` — into the manifest for the ERP and
module-consumer proofs, the pyramidal rule being that a proof's graded size
is pinned so a contentful weakening is a visible mismatch rather than a
silent pass. But three of the frozen expectations had never been exercised:
no full gate ran between that freeze and the first OWN-V1 gate run, and
both runs therefore surfaced the gap at once. The two ERP suite producers
emitted no `namedRegressions` at all (the authority producer, written in
the same round, DID — its RN01/RN02 aggregation is the pattern), and the
module-consumers exporter emitted no `witnessTotal`. The frozen count
(`4`) was correct all along: per module-consumers.witnesses.mjs, a witness
is a **required check × consumer stage** — two manifest checks, each
graded for both product verticals, `2 × 2 = 4` — not a count of the hashed
module packages (five including the `@archava/config` carrier and the
`-pg` ledgers, which would give 10). Counting the wrong unit is precisely how a frozen tally can seem
"never satisfiable" without being wrong.

**Decision.** (1) The two ERP producers grade their `namedRegressions` from
the checks they already run — the check names ARE the frozen strings — so a
wiring drift or a failed witness makes the record differ from the frozen
expectation loudly, the same shape as the authority producer's RN01/RN02.
(2) The module-consumers exporter counts `witnessTotal` from the sidecar
witness map it shares with the drift test
(`module-consumers.witnesses.mjs`), asserting the same check×consumer
stages the run actually performed, and the proof JSON now cites those stage
names so proof → map → manifest is walkable without another process. (3)
The frozen `4` stands unchanged; the b141a24 correction to `10` counted
module-hash entries — a unit no other artifact in the chain agrees with —
and is reverted, with this paragraph replacing the wrong record.

**Consequence.** The fourth release-gate error class (details missing frozen
tally) is closed at the producers, not the validator; the authority file's
second-writer race (the OWN suite's rewrite of the fast gate's copy) is
closed by a parameterized proof path — same suite, real PostgreSQL, own
record next to the rest of dist/own-evidence/.
