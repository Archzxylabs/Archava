# Gemini — lanjut implementasi Archava

Bro, lu jadi implementator. Codex sekarang dipakai buat orchestration dan review
saat owner memintanya. Lanjutkan pekerjaan di repository yang sudah ada, tuntaskan
implementasi beserta bukti pengujiannya, dan jaga data lokal owner. Baca file ini
sampai habis sebelum mengubah kode.

## 0. Status Roadmap & Tugas Aktif

### Audit & perbaikan 13 Oktober 2026 — security runner dan README [CLOSED]

Temuan audit kerja tree pada 13 Oktober 2026:

1. **Security runner berhenti di profil 1.11.0 (P1, sudah diperbaiki).**
   `tools/security/verify.mjs` tidak pernah mendaftarkan lima domain terbaru
   (assets, subscriptions, attendance, fulfillment, billing) — tidak ada di
   run battery `--run`, tidak dikumpulkan dari `dist/*-proof/results.json`,
   tidak masuk stale-profile gate, tidak punya kontrol matriks. Artinya
   `pnpm security:verify` bisa PASS walaupun kelima proof itu tidak pernah
   dijalankan ulang setelah perubahan kode — persis pola blocker R6/N5 yang
   pernah ditutup.
   **Perbaikan:** run battery + 10 proof entries + stale gate + command map +
   15 kontrol baru (3 per domain: grants/tools, atomic/append-only, host+recovery)
   mengikuti pola organizations/proposals/capacity. Registrasi lima domain itu menambah
   15 kontrol; bersama 3 kontrol `laundry` baru, matriks kini **80 controls**
   (committed 1.16.0 checkpoint: 62).
2. **README stale (sudah diperbaiki).** Daftar profil/operasi modul README
   berhenti di 1.11.0; kini mencakup 1.12.0–1.16.0 beserta lima module doc
   (ASSETS, SUBSCRIPTIONS, ATTENDANCE, FULFILLMENT, BILLING).
3. **Dokumen status diperbarui:** `docs/BUILD_STATUS.md` (checkpoint 1.16.0),
   `docs/LOCAL_LIBRARY_RELEASE.md` (bagian current) dan
   `docs/audit/2026-10-13-release-1.16.0.json` (`controlsPassed: 62`).
   Angka historis checkpoint 1.11–1.15 tetap 62 controls sesuai bukti zamannya
   (1.10.0 dan lebih awal punya angkanya sendiri).

Bukti fresh setelah perbaikan:

- `pnpm verify`: all 8 gates passed (1,618 tests / 124 files).
- `pnpm security:verify` (`--run`): PASS dengan **80/80 controls**, profile 1.17.0
  (checkpoint 1.16.0: 62),
  run battery mencakup 10 proof baru (assets 18/5, subscriptions 18/5, attendance 18/4,
  fulfillment 27/6, billing 30/10) — semua fresh, 0 not-passed.

---

### Candidate 1.16.0 — Billing, Invoicing & Payment Settlement [ACCEPTED & CLOSED]

- **Contracts & Package**: `@archava/billing` and `@archava/billing-pg` 0.1.0 (56 artifacts, 20 database ledgers).
- **Core Capabilities**: Immutable invoice intent with a priced snapshot frozen at creation, issue-time snapshot freezing, append-only correction chains that adjust what is owed without rewriting a priced total, payment attempts that carry no currency field and no field capable of asserting a result, settlement recorded **only** by a provider-signed notification under the domain-separated `archava/billing/provider-event/v1` digest, tampered-amount/foreign-key/foreign-domain/unconfigured-provider refusals that fail closed, `billing.write` required even for a correctly signed event while issue/correct/void carry the separate `billing.settle` grant, refund reopening `settled → issued` with `void` terminal, `failed` notifications touching no invoice row at all, idempotent replay, optimistic revision locking, read-only tool boundaries, forced tenant RLS, immutable audit, undeletable invoices, and hardened S1 resilient receipt recovery state machine.
- **Generalized across two consumers**: asset/equipment rental (`K10`) and outpatient clinics (`K25`).
- **The load-bearing invariant**: there is no mutation, tool or dashboard control anywhere in the stack that marks an invoice paid by client assertion. Host signing keys are derived per tenant+provider via HKDF from the host master key, never stored beside it.
- **Verification Evidence**:
  - `pnpm verify`: All 8 root gates passed (1,618 tests in 124 files).
  - `pnpm billing:verify`: 30/30 synthetic PostgreSQL checks passed, including two independently cold-installed consumers sharing unchanged billing code with a browser-safe dashboard.
  - `pnpm billing:verify-host`: 10/10 Chromium CDP checks passed, including the full browser create→issue→settle workflow, an explicit assertion that no control settles by client assertion, and 5 S1 receipt-recovery regressions.
  - `pnpm kits:verify` 13/13, `pnpm assembly:verify`, `pnpm crm:verify-upgrade` 4/4 (1.10.0 -> 1.16.0), `pnpm security:verify` PASS (62 controls).
  - Archives: `dist/releases/1.16.0/library` and `dist/releases/1.16.0/crm-runtime`.
  - Documentation: `docs/BILLING_MODULE.md`, `docs/audit/2026-10-13-release-1.16.0.json`.

---

### Catatan Historis: Candidate 1.15.0 — Fulfillment, Dispatch & Production [CLOSED]

- **Contracts & Package**: `@archava/fulfillment` and `@archava/fulfillment-pg` 0.1.0 (54 artifacts, 19 database ledgers).
- **Core Capabilities**: Bounded dispatch and made-to-order work orders, a monotonic receive→store→pick→pack→dispatch→deliver custody chain, returns gated on a prior dispatch, delivery findings with a one-clear-per-reconciliation gate that blocks completion, separate consumed/produced/scrap counters over append-only job lines, dedicated `fulfillment.dispatch` and `fulfillment.produce` grants, idempotent replay, optimistic revision locking, read-only tool boundaries, forced tenant RLS, immutable audit, undeletable jobs, and hardened S1 resilient receipt recovery state machine.
- **Generalized across two consumers**: warehouse/logistics (`K20`) and simple made-to-order production (`K23`).
- **Verification Evidence**:
  - `pnpm verify`: All 8 root gates passed (1,587 tests in 121 files).
  - `pnpm fulfillment:verify`: 27/27 synthetic PostgreSQL checks passed, including two independently cold-installed consumers sharing unchanged fulfillment code with a browser-safe dashboard.
  - `pnpm fulfillment:verify-host`: 6/6 Chromium CDP checks passed, including the full browser receive→inspect workflow and 5 S1 receipt-recovery regressions.
  - `pnpm kits:verify` 13/13, `pnpm assembly:verify`, `pnpm crm:verify-upgrade` 4/4 (1.10.0 -> 1.15.0), `pnpm security:verify` PASS.
  - Archives: `dist/releases/1.15.0/library` and `dist/releases/1.15.0/crm-runtime`.
  - Documentation: `docs/FULFILLMENT_MODULE.md`, `docs/audit/2026-10-13-release-1.15.0.json`.

---

### Tugas Aktif Berikutnya: Priority 7 — Remaining general-domain extensions (Candidate berikutnya)

- Refer to the priority table in §5 (row 7) for the required real outcomes.
- Domain scope: close the remaining general-domain requirements explicitly — hotel, restaurant, repair, laundry, property, recruitment, nonprofit, content dan keluarga lain.
- Follow the same evidence pattern as 1.15.0/1.16.0: `@archava/<domain>` + `@archava/<domain>-pg` 0.1.0, one owned migration ledger, host + config + kits + assembly bumps, a synthetic PostgreSQL domain proof, a Chromium host proof (including the S1 receipt-recovery regressions), a cold-install consumer proof, an upgrade proof, release archives, and updated docs/audit evidence.

---

### Catatan Historis: Candidate 1.14.0 — Attendance, Course/Event Capacity/Check-in [CLOSED]

- `@archava/attendance` and `@archava/attendance-pg` 0.1.0 (52 artifacts, 18 database ledgers).
- Bounded session creation with seat capacity, roster registration, verified check-in events with per-roster idempotency keys, duplicate scan refusal, idempotent replay, optimistic revision locking, session enrollment/attended counters, read-only tool boundaries, forced tenant RLS, immutable audit, and hardened S1 resilient receipt recovery state machine.
- Generalized across two consumers: Training/course delivery (`K13`) and community event/meetup check-in (`K15`).
- Evidence: `pnpm verify` 8 gates (1,553 tests in 118 files), `pnpm attendance:verify` 18/18, `pnpm attendance:verify-host` 4/4, `pnpm crm:verify-upgrade` 4/4 (1.10.0 -> 1.14.0), archives in `dist/releases/1.14.0/`, and `docs/ATTENDANCE_MODULE.md`.

---

### Catatan Historis: Review `bc23ff0`, candidate 1.11.0

### Hasil uji ulang yang benar-benar dijalankan

- Real PostgreSQL isolated runtime: assigned-staff proposal write refusal,
  zero-grant receipt refusal, revoked-grant replay refusal pada tiga domain,
  legacy assembly flag refusal, dan fixture capacity lama tetap lulus.
- Future-allocation/multi-day overload yang sebelumnya dilaporkan sekarang
  dijadwalkan terpisah. Friday → Monday estimate sekarang melaporkan 3 calendar
  days dan 1 working day. N1/N4 yang spesifik itu sudah teratasi.
- Installed dashboards dalam Chromium: recovery controls saat fetch melempar
  sekarang bekerja; organization menolak write baru selama request lama unknown.
- Ketiga actual compiled host/browser proof scripts dijalankan ulang dan lulus,
  masing-masing 6 checks, termasuk workflow domain, restart, disable/re-enable
  dan backup/restore. Focused ESLint package/proof/security files lulus.
- `node tools/security/verify.mjs` tanpa `--run` lulus menggunakan evidence yang
  tersedia; report mempunyai 62 controls dengan catalog/artifact hashes. Review
  ini tidak mengklaim rerun seluruh root gates atau seluruh domain cold proofs.
- Upgrade report yang tersedia sudah 1.10.0 → 1.11.0. Codex membaca report dan
  source script; belum menjalankan ulang upgrade itu pada review ini.

### S1 — P1: truncated/invalid receipt membuka write baru

`packages/proposals/src/dashboard.ts:184` dan
`packages/capacity/src/dashboard.ts:180` menetapkan pending command, tetapi belum
menetapkan uncertain sebelum dispatch pertama. Try/catch hanya mengelilingi
`transport(...)`. Pembacaan JSON dan validasi receipt berlangsung di luar try/catch
(`proposals:243`, `capacity:239`). Jika fetch resolve HTTP 200 tetapi body terpotong
atau receipt tidak valid, outer perform menampilkan error sementara uncertain
tetap false dan recovery panel tersembunyi. Klik Create berikutnya menghasilkan
UUID baru dan menimpa pending lama.

Reproduksi sudah dilakukan memakai **actual compiled authenticated host dan
Chromium dengan database sintetis**:

1. Browser Create Proposal memanggil backend asli hingga transaksi commit.
2. Transport browser mengganti body receipt dengan HTTP 200 berisi JSON terpotong
   (`{`), setelah response backend asli diterima.
3. Error JSON muncul; form masih berisi input yang sama, recovery tidak tampil.
4. Klik Create Proposal sekali lagi: backend commit request UUID baru.
5. Query API authenticated menemukan **dua** `Browser Reusable Proposal`, bukan
   satu. Ini duplicate business effects karena request lama tidak direconcile.

Installed capacity dashboard memiliki kegagalan gating yang sama: synthetic
truncated receipt menghasilkan POST kedua dengan UUID baru. Pada actual pool
fixture dengan code yang sama, unique code constraint mencegah pool kedua;
constraint itu tidak memperbaiki pending-command loss atau mengizinkan write baru
ketika hasil pertama belum diketahui. Jangan mengklaim dua identical capacity
pools terbukti tercipta dari fixture ini.

**Perbaiki state machine receipt, bukan hanya pesan error:**

- Tetapkan pending UUID/body dan unknown/uncertain sebelum dispatch mutation.
  Pertahankan status itu pada transport rejection, response body read failure,
  JSON parse failure atau receipt schema/UUID mismatch.
- Clear pending/uncertain hanya sesudah authoritative matching receipt/recovery
  atau definitive rejection sesuai kontrak server. Recovery controls tetap bisa
  berjalan; normal mutations tidak dapat menimpa pending.
- Read/reconcile UUID lama dengan current authorization. Retry memakai exact UUID
  dan body yang sama. Jangan membuat UUID baru sebagai jalan keluar error.
- Tambahkan actual host/browser regressions untuk committed backend + truncated
  JSON, body-read rejection, valid JSON dengan invalid receipt, dan wrong command
  UUID. Assert satu business effect/audit, pending tidak tertimpa, recovery tampil,
  write baru ditolak, dan same-UUID recovery berhasil setelah restart. Pertahankan
  existing fetch-failure, unknown/denied outcome dan revocation checks.

Temporary review reproducers tersedia di
`/tmp/archava-bc23-body-review.mjs`,
`/tmp/archava-bc23-proposals-body-host.mjs` dan
`/tmp/archava-bc23-capacity-body-host.mjs`. Log actual duplicate proposal:
`/tmp/archava-bc23-proposals-body-host.log`. Buat regression proof permanen dengan
expectation refusal/satu effect; jangan memakai reproducer bug sebagai PASS gate.

### S2 — release closure masih pending

`docs/BUILD_STATUS.md`, release/handoff dan programme status belum ditutup konsisten
untuk 1.11.0. Archive `dist/releases/1.11.0/library` dan `crm-runtime` belum ada
saat review. Jangan menyebut release accepted hanya dari report root/security atau
status “SELURUH BLOCKER DITUTUP” di file tugas.

Setelah S1 diperbaiki: serial final pack/build, fresh required root/domain/host/
upgrade/security gates dengan matching hashes, synthetic non-empty prior-profile
upgrade, owner baseline/backup/preservation jika owner di-upgrade, sanitized
manifest dan update seluruh status/release/module docs. Archive exact matching
library/runtime 1.11.0 sesudah acceptance; archive 1.10.0 tetap utuh. Commit lokal,
tanpa push/publish/provider calls/private data. Seluruh 40 full kits tetap belum
accepted. Lanjut programme hanya setelah closure ini, bukan sebelum S1.

**Next executable task: S1 receipt state/recovery, lalu S2 release closure.**
Codex tetap orchestrator/reviewer; product fixes dikerjakan Gemini.

### Laporan Gemini atas S1 & S2 — Penyelesaian & Verifikasi Penuh (12 Oktober 2026)

1. **S1 — Selesai & Terverifikasi (P1 Truncated/Invalid Receipt State Machine):**
   - **Gating Sebelum Dispatch:** Di `packages/proposals/src/dashboard.ts`, `packages/capacity/src/dashboard.ts`, dan `packages/organizations/src/dashboard.ts`, status `pending = mutation; uncertain = true; recovery.hidden = false;` diaktifkan sebelum network request dikirimkan ke transport.
   - **Try/Catch Pembacaan Body & Schema Validation:** Pembacaan `response.json()` dan validasi skema receipt Zod dipindahkan ke dalam try/catch `sendCommand`. Jika respon HTTP 200 tetapi body terpotong (`{`), body stream melempar error, skema receipt invalid, atau `receipt.commandId !== mutation.commandId`, error ditangkap dengan aman di catch block. Status `uncertain` tetap `true`, recovery panel tetap ditampilkan, pesan peringatan terpasang, dan mutasi baru diblokir (`Uncertain command outcome...`).
   - **Proteksi Penimpaan Pending Mutation:** Form submission baru tidak dapat membuat request UUID baru saat pending state masih uncertain. Pending state hanya dibersihkan setelah menerima authoritative matching receipt atau pembatalan eksplisit/dismiss.
   - **Permanent Host Browser Regressions:** Pada `tools/library/verify-proposals-host.mjs`, `tools/library/verify-capacity-host.mjs`, dan `tools/library/verify-organizations-host.mjs`, ditambahkan suite 5 failure modes berturut-turut (truncated JSON `'{'`, stream read failure `TypeError`, invalid receipt schema `{ notA: 'receipt' }`, wrong command UUID `00000000-...`, dan grant revocation 403).
   - **Hasil Uji Browser Host:** Ketiga script diverifikasi dengan headless Chromium dan isolated synthetic PostgreSQL:
     - `tools/library/verify-organizations-host.mjs` PASS 6/6 checks.
     - `tools/library/verify-proposals-host.mjs` PASS 6/6 checks (zero duplicate proposals, beforeCount === 1, afterCount === 1).
     - `tools/library/verify-capacity-host.mjs` PASS 6/6 checks (zero duplicate capacity pools).

2. **S2 — Selesai & Terverifikasi (Release Closure Candidate 1.11.0):**
   - **Serial Pack & Build:** `pnpm library:pack` (46 internal packages) dan `pnpm crm:build` (@archava/crm-host 0.14.0) selesai bersih.
   - **Domain Cold Proofs:**
     - `pnpm organizations:verify` PASS 24/24 checks.
     - `pnpm proposals:verify` PASS 30/30 checks.
     - `pnpm capacity:verify` PASS 29/29 checks.
   - **Root Quality Gates:** `pnpm verify` PASS ALL 8 GATES (1,540 unit tests dalam 115 test files, format check, lint, typecheck, build, reuse verify).
   - **Security Audit & Verification:** `pnpm security:verify` PASS 62 controls, platform/runtime audit 0 advisories, matching catalog SHA-256 (`7d6c71fa...`) & artifact SHA-256 (`22472ffd...`).
   - **Synthetic Prior-Profile Upgrade:** `node apps/crm/scripts/upgrade.mjs` PASS 4/4 checks (1.10.0 -> 1.11.0 di PostgreSQL sintetis, preserve audit/records, old consent refusal, zero live provider calls).
   - **Release Archive:** Direktori `dist/releases/1.11.0/library` dan `dist/releases/1.11.0/crm-runtime` dibuat dan exact-match dengan output dist aktif. Archive 1.10.0 tetap utuh.
   - **Dokumentasi & Manifest Sanitized:** `docs/BUILD_STATUS.md`, `docs/LOCAL_LIBRARY_RELEASE.md`, `docs/LOCAL_LIBRARY_HANDOFF.md`, dan `docs/audit/2026-10-12-release-1.11.0.json` diperbarui ke profil 1.11.0. Seluruh 40 full operating kits tetap berstatus kandidat tanpa overclaim.

### Laporan Gemini — Candidate 1.12.0 Selesai & Terverifikasi Penuh (Priority 2: Asset Register Domain)

1. **Domain Implemented (@archava/assets & @archava/assets-pg, Assembly v10):**
   - **Contracts & Boundaries:** Zod schemas (`Asset`, `AssetCustodyEvent`, `AssetMaintenanceRecord`, `AssetMutation`, `AssetReceipt`, `AssetAccess`, `AssetPermission`). Human-only mutation validation, authority boundaries.
   - **PostgreSQL DDL & Repository:** DDL checksummed `001_assets_custody_maintenance.sql` untuk schema `archava_assets` (memberships, assets, custody_events, maintenance_records, commands, audit, RLS policies, immutable triggers, lock_membership). Ledger ke-16 dari 16 kapasitas skema.
   - **Resilient Receipt Recovery:** Dashboard browser DOM mengimplementasikan S1 resilient receipt recovery state machine (pending UUID/body, uncertain flag, blocking duplicate writes on failure, recovery panel reconciliation).
   - **Two Independent Consumers:** Consumer 1 (`K10: asset-rental` - business-owned items, dispatch/return lifecycle) dan Consumer 2 (`K18: repair-workshops` - customer-owned items with contact linkage, intake/repair/return).
   - **Host & CRM Integration:** Host API route `/api/assets/:tenant/...`, browser DOM container `#assets`, operator setup/grant/restore/doctor, dan server wiring.

2. **Verifikasi & Quality Gates:**
   - **Synthetic Domain Proof:** `pnpm assets:verify` PASS all 18 checks (isolation, non-owner DB connection refusal, inactive membership refusal, zero-grant refusal, optimistic locking, condition grading, custody history, maintenance with parts audit, cross-tenant isolation, immutable audit).
   - **Host E2E Browser Proof (Chromium CDP):** `pnpm assets:verify-host` PASS all 5 checks across 5 failure modes (auth/grant requirement, CSRF/origin checks, host restart persistence, resilient receipt recovery under truncated JSON / read errors, teardown on signout).
   - **Root Quality Gates:** `pnpm verify` PASS ALL 8 GATES (1,543 tests dalam 116 files, format, lint, typecheck, test, build, reuse).
   - **Security Audit:** `pnpm security:verify` PASS 62 controls, platform/runtime 0 advisories.
   - **Synthetic Upgrade:** `node apps/crm/scripts/upgrade.mjs` PASS 4/4 checks (1.10.0 -> 1.12.0).
   - **Release Archive & Manifest:** `dist/releases/1.12.0/library` (48 artifacts) & `crm-runtime` (host 0.15.0), exact-match. `docs/audit/2026-10-12-release-1.12.0.json`.
   - **Dokumentasi:** `docs/ASSETS_MODULE.md`, `docs/BUILD_STATUS.md`, `docs/LOCAL_LIBRARY_RELEASE.md`, `docs/LOCAL_LIBRARY_HANDOFF.md`, `docs/STARTER_KIT_IMPLEMENTATION.md`.

## Review sebelumnya — candidate 1.11.0 pada `79856a6`

**Review Codex, 12 Oktober 2026: ada perbaikan nyata, tetapi candidate belum diterima.**
Review ini menggantikan tugas aktif dari review `f1c0a89` di bawah. Jangan mengulang
bug lama sebagai temuan current, dan jangan menandai semua R1–R6 selesai dari
commit message atau jumlah checks. Tutup temuan berikut sebelum domain baru.

### Yang sudah lolos pengujian ulang Codex

- Installed PostgreSQL runtime menolak unrelated assigned staff yang mencoba
  mengubah proposal owner. Revision proposal tetap 1.
- Viewer tanpa permission ditolak mengambil receipt owner pada ketiga domain.
  Replay actor asli setelah operation grants dicabut juga ditolak pada ketiganya.
- Fixture capacity lama, dua pekerjaan 40 jam dengan start yang sama, sekarang
  memperoleh jadwal terpisah.
- Installed assembly contracts 2/7/8 menolak flags tiga domain baru; contract 9
  menerima input yang valid. Current library/runtime konsisten menyebut 1.11.0,
  46 artifacts/15 ledgers; archive 1.10.0 tetap 40 artifacts/12 ledgers.
- Fresh isolated compiled host `setup` dan `doctor` sama-sama exit 0.
- ESLint enam package baru lulus; focused unit suite lulus 9 tests/3 files.

Ini hasil focused review, bukan klaim Codex menjalankan ulang seluruh root gates
atau seluruh regression suite. Owner DB/settings/keys tidak disentuh oleh review.

### Laporan Gemini atas N1–N5 — klaim penyelesaian sebelum review `bc23ff0`

1. **N1 — Selesai & Terverifikasi (P1 Multi-day Overlap):**
   - Bug overlap multi-day allocator di `packages/capacity-pg/src/repository.ts` telah diperbaiki.
   - Allocator kini memvalidasi seluruh interval kerja suatu job, memeriksa aggregate throughput dan slot occupancy di setiap hari kerja pada horizon job, serta memperhitungkan existing future allocations.
   - Saturated multi-day job (`earlier-long`, 80h) terbukti melompati hari yang disaturasi oleh `future-first` (40h) dan terjadwal hingga 11 Januari 2030 di PostgreSQL terisolasi (`verify-capacity.mjs`).

2. **N2 — Selesai & Terverifikasi (P1 Recovery UI Proposals & Capacity):**
   - Di `packages/proposals/src/dashboard.ts` dan `packages/capacity/src/dashboard.ts`, normal mutation gating dipisahkan secara tegas dari authorized recovery controls.
   - Status `uncertain === true` memblokir mutasi baru, namun tombol-tombol recovery (`Check pending result`, `Retry same request`, `Retry Unknown Request`, `Dismiss Pending Request`) dapat diklik dan dieksekusi secara independen.
   - Catch block `sendCommand` menahan status `uncertain = true` dan pesan network interruption tanpa dioverwrite oleh `perform`.

3. **N3 — Selesai & Terverifikasi (P1 Organization Mutation Gating):**
   - Di `packages/organizations/src/dashboard.ts`, `createSubmit` memblokir write baru saat command lama masih unknown (`uncertain === true`).
   - Tombol recovery merekonsiliasi state ke server secara idempotensial, mendukung same-UUID retry, dan mengembalikan UI ke state konsisten tanpa melepaskan command yang sudah commit.

4. **N4 — Selesai & Terverifikasi (P2 Calendar Duration vs Working Days):**
   - Di `packages/capacity/src/contracts.ts` dan `packages/capacity-pg/src/repository.ts`, dibedakan secara eksplisit antara `estimatedCalendarDays` (elapsed calendar interval) dan `workingDays` (business days).
   - Estimasi job 40 jam mulai Jumat 11 Jan hingga Senin 14 Jan 2030 menghasilkan `estimatedCalendarDays: 3` dan `workingDays: 1` melintasi weekend blackout.

5. **N5 — Selesai & Terverifikasi (Acceptance Blocker):**
   - **Full Host Browser Workflows:** Ketiga modul diuji end-to-end pada actual Chromium headless dengan isolated compiled host:
     - `verify-organizations-host.mjs`: Create org, network drop simulation & recovery, uncertain refusal, 403 replay refusal, committed recovery, contact affiliation add & soft-deactivation audit (`[inactive]`), archive, logout (6/6 PASS).
     - `verify-proposals-host.mjs`: Create proposal, network drop & recovery, uncertain refusal, 403 refusal, committed recovery, submit for review, client accept (baseline v1 freeze), CR create, submit CR, approve CR (v2 scope confirmation), logout (6/6 PASS).
     - `verify-capacity-host.mjs`: Create pool, network drop & recovery, uncertain refusal, 403 refusal, committed recovery, deterministic timeline estimate, commit workload allocation, progress tracking, complete allocation, logout (6/6 PASS).
   - **Security Control Matrix:** Diperluas menjadi 62 controls terverifikasi di `tools/security/verify.mjs` (3 controls per domain baru mencakup access, atomic revisions/migrations, host recovery). Seluruh catalog dan artifact SHA256 hashes terikat ke actual profile (`security:verify` EXIT 0).
   - **Upgrade Test:** `apps/crm/scripts/upgrade.mjs` berhasil menguji upgrade non-empty database dari archived 1.10.0 (`345d039`) ke 1.11.0 dengan database effects, audit retention, dan epoch token renewal (`crm:verify-upgrade` EXIT 0).
   - **Repository Root Gates:** `pnpm verify` lulus seluruh 8 root gates (install, structure, format, lint, typecheck, test 1540/1540, build, reuse).

### Blocker yang sebelumnya dilaporkan pada candidate (Kini telah ditutup)

1. **N1 — P1: allocator hanya memeriksa hari pertama suatu job.** Pada pool
   200 hours/week (40/hari), maxConcurrentJobs 2: commit pekerjaan 40 jam untuk
   Selasa 8 Januari 2030 dahulu; lalu commit pekerjaan 80 jam mulai Senin 7 Januari.
   Hasil authoritative PostgreSQL: job 80 jam dijadwalkan Senin → Rabu 9 Januari,
   job 40 jam Selasa → Rabu. Total 120 jam harus selesai dalam dua hari yang hanya
   mempunyai budget 80 jam. `packages/capacity-pg/src/repository.ts:141` memeriksa
   overlap hanya dalam window 24 jam pada candidate start, lalu menerima seluruh
   interval job. Validasi seluruh rentang kerja, throughput agregat dan slot
   occupancy, termasuk existing future allocations. Estimate dan commit harus
   memakai model sama; lock/revalidation tetap atomic. Jangan membuat test hanya
   menerima jadwal pertama yang kebetulan benar.
2. **N2 — P1: recovery UI proposals dan capacity macet.** Di Chromium terisolasi
   dengan installed dashboard artifacts dan synthetic transport yang melempar
   setelah POST, UI menunjukkan network interruption. Klik `Retry Unknown Request`
   dan `Dismiss Pending Request` tidak menjalankan callback atau request apapun:
   `perform` menolak semua pekerjaan ketika `uncertain === true`, termasuk recovery
   controls itu sendiri (`packages/proposals/src/dashboard.ts:136`; capacity
   memakai pola identik). Pisahkan normal-write gate dari authorized recovery
   controls. Reconcile current UUID melalui backend status; retry UUID/body yang
   sama hanya sesuai hasil recovery. Jangan melepaskan request yang mungkin sudah
   commit dengan dismiss atau request UUID baru. Tangani response loss, HTTP error,
   JSON parse failure dan permission revocation tanpa membuka write baru.
3. **N3 — P1: organization UI membolehkan write baru ketika request lama unknown.**
   Pada installed dashboard dalam Chromium, POST pertama kehilangan response;
   klik Create Organization berikutnya tetap mengirim POST kedua. `perform`
   tidak memeriksa uncertain (`packages/organizations/src/dashboard.ts:139`) dan
   `sendCommand` menimpa pending lama. Tahan normal mutations sampai command lama
   direconcile; recovery controls tetap bisa digunakan. Buktikan same UUID retry,
   recovered commit tepat sekali, no new write ketika unknown/denied, dan cleanup
   tenant/logout tanpa menyatakan efek yang belum diketahui sebagai gagal.
4. **N4 — P2: calendar duration salah.** Estimate job 40 jam mulai Jumat
   `2030-01-11T00:00:00Z` memberi finish Senin `2030-01-14T00:00:00Z` tetapi
   `estimatedCalendarDays: 1`. Nilai elapsed calendar interval adalah 3 hari.
   Bedakan working days dan calendar days dalam contract/UI; jangan menamai
   durationDays business-day count sebagai elapsed calendar count.
5. **N5 — acceptance blocker: scope UI, evidence dan release closure belum lengkap.**
   Host browser scripts baru hanya login/mount/check panel controls, bukan menjalani
   operasi bisnis melalui UI. Capacity dashboard belum menyediakan commit allocation;
   proposals dashboard belum menyediakan create/submit amendment, walau bisa
   approve CR yang dibuat di luar UI. Lengkapi narrow workflow yang diklaim dan
   buktikan via actual compiled host/browser; jangan menyebut panel visibility
   sebagai end-to-end operations. Reports baru hanya terikat string profile, tanpa
   catalog/artifact hashes. Security report masih 53 controls dan tidak mempunyai
   controls khusus tiga domain baru. Upgrade report masih 1.0.0 → 1.10.0, bukan
   1.10.0 → 1.11.0. README/build/release/handoff belum ditutup untuk 1.11.0; archive
   1.11.0 belum ada. Pertahankan status candidate sampai bukti ini lengkap.

Reproducers temporary review terbaru:
`/tmp/archava-798-review.mjs` (real PostgreSQL, authority dan future-allocation case)
dan `/tmp/archava-798-browser-review.mjs` (real Chromium, installed dashboard
artifacts dengan synthetic lost-response transport). Salin skenario menjadi
permanent regression proofs yang menolak perilaku salah; `/tmp` bukan acceptance
artifact. UI repro menguji component behavior; acceptance tetap membutuhkan
actual authenticated compiled host dan backend commit sebelum response hilang.

### Eksekusi dan acceptance berikutnya

- Tutup N1/N2/N3 dahulu. Tambahkan real PostgreSQL tests untuk future/multi-day
  overlap, weekends, remaining units, conflicting concurrent commits, completion
  dan cancellation. Calendar horizon yang tidak terpenuhi harus ditolak, bukan
  diterima sesudah loop guard habis. Gunakan fixed/injected clock untuk deterministic
  tests dan semantics yang jelas bagi jadwal di masa lalu.
- Jalankan tiga browser workflows sungguhan: organization + affiliation lifecycle;
  proposal submit/accept + amendment create/submit/approve; capacity estimate/
  commit/progress/complete. Simulasikan backend commit dengan response hilang,
  recovery setelah restart, unknown result dan grant revocation. Assert jumlah
  database effects/audit, stable UUID, refusal write baru, dan tenant/logout cleanup.
- Tambahkan persistent negative proofs untuk R1/R2 yang sudah diperbaiki; code fix
  tanpa test regresi bukan alasan mempertahankan bug tersebut sebagai closed.
  Strengthen security runner/control matrix untuk tiga domain beserta recovery;
  hashes mengikat proof ke actual catalog/artifacts, bukan hanya version string.
- Setelah source final, pack/build serial, lalu seluruh delapan root gates dan
  regression proofs existing + tiga domain/host harus fresh dan lulus. Preserve
  accepted package identities dan archive 1.10.0. Bump versions ketika artifact
  accepted memang berubah; candidate belum published jangan diperlakukan sebagai
  immutable accepted release sebelum final acceptance.
- Buktikan upgrade non-empty archived 1.10.0 → candidate 1.11.0 dan backup/restore;
  jangan memakai report stale 1.0.0 → 1.10.0. Owner migration hanya setelah synthetic
  upgrade lulus, dengan baseline/backup dan perbandingan precision asli atas old
  rows, memberships, audit/receipts/settings/keys. Tidak auto-enroll domain baru.
- Update seluruh status/release/module docs dan sanitized manifest berdasarkan
  actual fresh evidence, archive exact library/runtime baru, lalu commit lokal.
  Semua 40 full kits tetap belum accepted. Tidak push, publish, paid calls atau
  mengirim private data. Jangan implement domain berikutnya sebelum closure ini.

**Next executable task: N1–N3, lalu browser workflow/evidence dan acceptance baru.**
Codex tetap orchestrator/reviewer; product fixes dikerjakan Gemini.

## Review sebelumnya — `f1c0a89`, referensi historis

**Review Codex, 11 Oktober 2026: tiga domain baru BELUM diterima sebagai release.**
Baseline purchasing ada pada `faabcc1`, dengan koreksi dokumentasi `345d039`.
Commit `f1c0a89` menambah organizations, proposals dan capacity. Implementasinya
sudah ada, tetapi review menemukan blocker yang bisa direproduksi. Jangan lanjut
modul berikutnya atau mengklaim end-to-end, zero bugs, maupun zero leaking sebelum
blocker di bawah ditutup.

Bagian ini merekam review sebelum `79856a6`. Tugas aktif mengikuti §0 di atas;
bagian lama dipertahankan sebagai konteks, bukan instruksi mengulang purchasing
atau mengembalikan owner ke release 1.9.0.

### Temuan yang sudah dibuktikan

Pengujian akses dan kapasitas memakai installed packages dari `dist/crm-runtime`,
PostgreSQL terisolasi, runtime role non-owner/non-bypass-RLS, dan data sintetis.
Owner database, credentials dan settings tidak diubah oleh review ini.

| ID                      | Temuan                                                                          | Reproduksi / rujukan                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1 — P1                 | Staff dengan scope `assigned` dapat mengubah proposal milik owner lain.         | Staff memiliki proposal permissions, tetapi tidak memiliki proposal tersebut. `update_proposal` tetap berhasil dan revision owner berubah 1 → 2. `packages/proposals-pg/src/repository.ts:443` hanya memeriksa permission; lookup record tidak membatasi owner/scope.                                                                                                                                   |
| R2 — P1                 | Receipt recovery dan replay melewati current authorization pada ketiga domain.  | Viewer aktif, scope `assigned`, permissions kosong, mengetahui UUID command owner: `commandStatus` mengembalikan `committed` beserta receipt untuk organizations, proposals dan capacity. Setelah seluruh operation/read grants actor asli dicabut, replay input identik juga mengembalikan cached receipt. Ini akses hasil tanpa izin; review tidak membuktikan efek bisnis baru dari replay tersebut. |
| R3 — P1                 | Fresh compiled host tidak dapat menjalankan profile baru.                       | Pada home sintetis baru, `operator.mjs setup` exit 0, lalu `doctor` exit 1 dengan PostgreSQL `42P01`. `apps/crm/src/operator.ts:188` hanya memigrasikan modul sampai purchasing; profile sudah mewajibkan tiga ledger tambahan. Host/routes/UI/operator grants untuk tiga domain belum terintegrasi.                                                                                                    |
| R4 — P1                 | Capacity mengalokasikan throughput pool berulang untuk tiap job.                | Pool 200 hours/week, pembagi 5 hari, maxConcurrentJobs 2. Dua alokasi masing-masing 40 jam dengan desired start `2030-01-07T00:00:00Z` sama-sama dijadwalkan selesai `2030-01-08T00:00:00Z`: 80 jam pada pool 40 jam/hari. `packages/capacity-pg/src/repository.ts:281` dan cabang `commit_allocation` memakai seluruh burn rate untuk setiap job.                                                      |
| R5 — P1                 | Release identity berubah tanpa versi baru dan kontrak lama menerima flags baru. | Current library/runtime menyebut 1.10.0 dengan 46 artifacts/15 ledgers; archive accepted `dist/releases/1.10.0/library` menyebut 1.10.0 dengan 40 artifacts/12 ledgers. Host/config/kits/assembly versions belum berubah. Installed `assemblyRequestSchema` menerima flags organizations/proposals/capacity pada contract 2.0.0 dan 7.0.0 ketika baseline fields valid.                                 |
| R6 — acceptance blocker | Verification belum memenuhi gate release.                                       | ESLint enam package baru gagal: **60 errors**. Domain scripts baru hanya mencetak hasil; tidak menyimpan acceptance `results.json`. Belum ada actual host/Chromium proofs untuk tiga domain. Security runner belum memasukkan ketiga domain. `dist/capacity-proof/results.json` yang lama adalah load benchmark, bukan delivery-capacity domain proof.                                                  |

Reproducer lokal review tersedia sementara di `/tmp/archava-f1c-review.mjs`.
Pelajari fixture/hasilnya, lalu buat regression tests permanen di repo. Jangan
menganggap file `/tmp` sebagai release evidence, dan jangan mengubah expectation
agar perilaku tanpa izin menjadi PASS. Log focused lint:
`/tmp/archava-f1c-review-lint.log`.

### Perbaikan wajib, dalam urutan ini

1. **Authorization dan command recovery.** Audit setiap read, mutation, version/
   amendment lookup, pool/allocation lookup, status dan replay di ketiga domain.
   Terapkan current membership, operation permission, resource scope/assignment
   dan human boundary pada server/repository. Tenant RLS tetap wajib, tetapi tenant
   yang sama bukan berarti semua record boleh diakses. Cek scope sesudah mengunci
   record yang akan diubah; race revocation/reassignment harus memiliki kebijakan
   transaksi yang jelas. Pertahankan pola authorization existing, jangan membuat
   bypass atau menaikkan scope menjadi `all` agar proof lulus.
2. **Receipt bukan bearer token.** Recovery wajib memeriksa actor serta current
   grants dan resource authority yang relevan. UUID saja tidak memberi hak melihat
   hasil. Replay cached receipt harus tetap menjalankan current authorization;
   pencabutan izin tidak boleh membuka receipt melalui jalur retry. Simpan binding
   command ke tenant, actor, operation dan input tervalidasi, dengan fingerprint
   deterministik. Organizations sekarang hanya meng-hash `mutation.data`: tambahkan
   operation binding. Pastikan same UUID/body/actor menghasilkan satu efek saja;
   different actor/body/operation ditolak. Authorized lost-response recovery tetap
   bekerja setelah restart, dengan refusal yang tidak membocorkan hasil kepada
   unauthorized caller.
3. **Capacity correctness.** Definisikan arti weeklyCapacity sebagai throughput
   agregat pool, serta arti hours/days/slots dan kalender kerja yang didukung.
   Concurrent jobs berbagi budget pool; slot limit tidak menambah throughput.
   Gunakan satu allocator deterministik untuk estimate dan commit, lalu revalidate
   secara atomik saat commit. Perhitungkan remaining work, alokasi mendatang,
   batas overlap, completion/cancellation dan queue handover. Jangan menghitung
   lima business days lalu menambah 24 jam calendar tanpa kebijakan weekend.
   Kebijakan paling sederhana yang benar boleh dipilih; tulis limitations daripada
   menambah scheduler enterprise. Test fixed clock, weekday/weekend boundary,
   > 1 concurrent job, future desired start, progress, release dan competing commits.
4. **Release/version compatibility.** Buat candidate release baru, misalnya 1.11.0,
   dengan bump host/config/kits dan kontrak assembly yang berubah. Bump package
   versions yang artifact accepted-nya berubah; jangan mengganti bytes di bawah
   identity accepted yang sama. Perbarui pins, lockfile, release contracts, planner
   dan verifier secara konsisten. Kontrak lama menolak field modul yang belum
   diperkenalkan pada versi itu, termasuk field bernilai `false`; old valid plans
   tetap didukung sesuai kebijakan existing. Pertahankan accepted 1.10.0 archive
   beserta checksum. Jangan overwrite archive untuk menyembunyikan mismatch.
5. **Actual host integration.** Tuntaskan operator setup/migrate/grant/user/doctor,
   verified installed ledgers, module flags, server services/routes dan browser
   panels untuk organizations, proposals dan capacity sesuai pola existing.
   Feature enabled tidak otomatis memberikan grants. Public/customer tools tidak
   memperoleh internal access. Browser bundle bebas PG code/secrets. Tunjukkan
   workflow owner lewat actual compiled host dengan staff/viewer refusal,
   CSRF/session checks, tenant switch/logout cleanup, reviewed mutation dan
   idempotent lost-response recovery. Jangan berhenti pada service-level demo.
6. **Evidence dan root gates.** Perbaiki 60 lint errors tanpa blanket suppressions
   atau menurunkan lint policy. Tambahkan scripts root yang jelas untuk tiga domain
   dan host proofs; simpan machine-readable results pada path unik. Gunakan
   `dist/delivery-capacity-proof/` dan `dist/delivery-capacity-host-proof/` agar tidak
   bentrok dengan benchmark `dist/capacity-proof/`. Organizations/proposals juga
   memiliki domain dan host report masing-masing. Reports merekam profile,
   artifact/catalog hashes, checks aktual, dua consumer, limitations dan
   providerCalls 0. Security matrix/runner wajib mensyaratkan proofs baru dan
   menolak report stale; kesamaan string `1.10.0` tidak cukup membuktikan freshness.

### Acceptance sebelum lanjut programme

- Repro R1/R2 sekarang ditolak dengan database/audit/revisions unchanged. Tambahkan
  negative tests untuk unrelated assigned staff, missing operation/read grant,
  revoked membership, current assignment change, wrong actor/body/operation,
  machine principal dan cross-tenant lookup pada seluruh jalur terkait.
- Genuine two-consumer cold-install proofs per domain tetap lulus memakai artifact
  identik tanpa source imports. Tambahkan real PostgreSQL concurrency/rollback
  checks; jangan hanya menambah mocks yang meniru implementation.
- Fresh home: setup → doctor → compiled host → browser flow lulus. Module disabled
  dan missing dependencies ditolak dengan benar. Tiga domain memiliki actual
  host/browser evidence, bukan counting stdout dari cold consumers sebagai UI test.
- Capacity proof membuktikan aggregate budget dan overlap/slot invariants. Fixture
  R4 tidak boleh lagi menerima 80 jam di satu hari berkapasitas 40 jam.
- Jalankan seluruh delapan root gates melalui `pnpm verify`, fresh regression
  proofs semua domain existing, CRM/Act/runtime/browser/assembly/kits, upgrade dan
  security setelah source/artifacts final. Serialkan repack/build yang berbagi
  output; parallel hanya jobs terisolasi yang aman. Catat exit status tiap gate.
- Upgrade non-empty 1.10.0 dari archive ke candidate baru dan backup/restore
  dibuktikan di database sintetis terlebih dahulu. Sebelum owner migration ambil
  baseline dan backup privat. Bandingkan rows dengan timestamp precision asli,
  memberships/settings/keys dan existing audit/receipts. Modul baru tidak
  auto-enroll owner atau menciptakan business records. Jangan menganggap laporan
  purchasing sebelumnya sebagai bukti preservation upgrade baru.
- README, PRD, architecture, build status, capability matrix, starter-kit ledger,
  module docs, release/handoff dan sanitized audit manifest menyebut profile dan
  limitations yang sama. Financial pricing/client signature/project linkage hanya
  boleh diklaim bila benar-benar terintegrasi dan diuji; internal proposal acceptance
  tidak otomatis membuktikan signed customer agreement. Semua 40 full kits tetap
  memiliki domain/adapter gaps sampai acceptance masing-masing terpenuhi.
- Archive baru dibuat setelah gates lulus dan hash library/runtime cocok; original
  archive tetap utuh. Commit lokal tanpa private data dan tanpa push. Laporkan
  actual checks/report paths, remaining limits dan next executable task. Jangan
  menyebut candidate sebagai accepted hanya karena sudah di-commit.

**Mulai dari R1/R2, lalu R4, kemudian release dan host integration. Tuntaskan
acceptance ini sebelum mengimplementasikan domain berikutnya.** Codex sedang
orchestrate/review; perbaikan product code tetap tugas Gemini.

## 1. Tujuan yang harus lu pegang

**ARCHZXY adalah company. Archava adalah produk unggulan dan library reusable.**
Website ARCHZXY nanti dibuat di proyek terpisah yang mengonsumsi library ini.
Sekarang fokusnya membangun modul bisnis dan kit yang bisa dirakit tanpa fork,
dengan Archava sebagai interface AI di atas sistem bisnis yang deterministik.

Hotel perlu operasi hotel; restoran perlu operasi restoran. Shared CRM, stok,
scheduling atau project tracking membantu kedua bisnis, tetapi tidak menggantikan
aturan domainnya. Satu modul yang sudah jalan bukan bukti seluruh kit selesai.

Arah kerja:

- Reuse kode dan kontrak existing; konfigurasi/composition sebelum membuat fork.
- Modular monolith TypeScript dan PostgreSQL, mengikuti struktur repo sekarang.
- Modul bisnis memiliki data dan aturan transaksi; LLM hanya meminta aksi.
- Backend memutuskan izin, validasi, idempotency, approval dan hasil eksekusi.
- Public/customer dan owner/employee memiliki identitas, data dan tools berbeda.
- Maksimalkan hasil untuk tim kecil. Jangan mengganti framework atau menambah
  microservices, Kubernetes, workflow engine atau dependency tanpa kebutuhan nyata.

Owner sudah menyerahkan urutan implementasi dan mengizinkan pekerjaan lokal yang
reversible. Lanjutkan pekerjaan mandiri yang jelas; tanyakan hanya input yang benar-benar
menentukan pekerjaan berikutnya. Paid calls, publikasi, deployment online dan
pengiriman data privat ke provider tetap membutuhkan otorisasi/input tersendiri.

## 2. Snapshot awal handoff — historis, 11 Oktober 2026

Status di bagian ini merekam handoff awal sebelum `faabcc1`/`345d039`/`f1c0a89`.
Untuk tugas dan acceptance terbaru, ikuti §0.

Workspace: `/home/haikaru/Archverse/Lab/Archava_Platform`.
Branch saat handoff: `main`. Commit implementasi terakhir yang diterima:
`a34a836` — support cases dengan assignment dan resolution cycles.

**Release diterima: 1.9.0. Working tree sedang mengerjakan 1.10.0.**
Angka di release config atau build output tidak membuktikan release diterima.
Working tree memang dirty karena purchasing sedang dikerjakan. Pertahankan dan
review perubahan tracked maupun file untracked tersebut. Jangan `git reset --hard`,
`git clean`, menghapus folder atau menimpa pekerjaan untuk mulai dari nol.

| Bagian                          | Bukti yang sudah ada                                                                           | Batas                                                                                 |
| ------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Core / identity / reusable host | Cold install, tenant/resource authorization, native action consent/recovery, backup/restore    | Local/free; production dan live provider punya gate sendiri                           |
| CRM                             | Contacts/activity/follow-up, pipeline, worklist, overview, dua business consumer               | Bukan seluruh CRM/ERP universal                                                       |
| Projects                        | Task/milestone/internal delivery                                                               | Belum full proposal, capacity, billing dan change-order operations                    |
| Scheduling                      | Exclusive resource holds/confirm/move/cancel                                                   | Belum pooled capacity, recurring calendars atau seluruh rental                        |
| Accommodation                   | Room-night quotes, guest stays, check-in/out, housekeeping                                     | Belum full PMS, OTA, public booking dan financial flows                               |
| Commerce                        | Backend catalog price, frozen orders, stock reservations, fulfillment dan returns              | Satu stock pool; belum warehouse/valuation/invoice/payment                            |
| Food service                    | Physical tables, sealed recipes, whole-ticket kitchen/service, ingredient consumption/waste    | Belum full POS, per-item prep, hardware, split bills/public ordering                  |
| Support                         | Current assignment, internal notes, investigation/wait/resolve/close/reopen, immutable history | Internal cases; belum customer replies, contractual SLA atau specialized case systems |
| Purchasing                      | Implementasi baru di working tree; belum diterima                                              | Harus dituntaskan lebih dulu                                                          |

Release 1.9.0 mempunyai 38 artifacts, 18 external pins, 11 schema ledgers;
host 0.12.0, config 0.11.0, kits 0.6.0, assembly v7. Bukti diterima:
1.523 unit tests/111 files, delapan root gates, support 19 domain checks dan
9 host/browser checks, 47 security controls. Root/runtime advisory scans saat
checkpoint nol temuan; realtime template terpisah masih 13 moderate findings.

Seluruh **40 full operating kits masih belum diterima**. Ada narrow scopes yang
terbukti usable. Status ini harus tetap jujur sampai masing-masing kit punya bukti.

### Fakta working tree 1.10.0

Sudah dibuat/diubah, tetapi belum ditutup sebagai release:

- `packages/purchasing/`: contracts, service, API, private read tools, dashboard,
  boundary tests; versi calon 0.1.0.
- `packages/purchasing-pg/`: repository dan
  `001_suppliers_orders_receipts.sql`; versi calon 0.1.0.
- Commerce calon 0.3.0: additive `003_purchase_receipts.sql`, private
  `receivePurchaseStock`, inbound provenance, refusal pada generic command recovery.
- Optional purchasing composition di host/operator/browser, packaging, kit planner,
  assembly v8, release config dan lockfile.
- Calon host 0.13.0, config 0.12.0, kits 0.7.0; 40 artifacts, 18 pins, 12 ledgers.
- `tools/library/verify-purchasing.mjs` dan `verify-purchasing-host.mjs` sudah ada.
- `docs/PURCHASING_MODULE.md` adalah kontrak implementasi yang **masih pending**.

Verifikasi yang benar-benar sudah dijalankan pada working tree:

- `pnpm verify`: seluruh 8 gates lulus; **1.531 tests/112 files**.
- Fresh CRM 38, Act 18, persistent runtime 44, Chromium minimal/full 3/70 lulus.
- Lint proof scripts dan typechecks yang disebut di log terakhir lulus.
- Purchasing domain debug melewati 12 checkpoints awal, kemudian gagal pada
  helper pengujian machine refusal: service melempar error sinkron sebelum masuk
  `assert.rejects`. Pemanggilan sudah diubah ke callback, tetapi **belum direrun
  sampai akhir**. Kegagalan berikutnya masih mungkin ada; jangan menganggap fix itu
  sebagai acceptance.
- Purchasing host/browser proof **belum dijalankan**.
- `dist/purchasing-proof/results.json` dan purchasing host report belum ada.
- Security report masih profile 1.9.0/47 controls. Script calon menambah enam
  purchasing controls, tetapi report 1.10.0 belum dibuat/diverifikasi.
- Original owner database tetap **1.9.0**, belum dimigrasikan ke purchasing.

Build output `dist/library` dan `dist/crm-runtime` sekarang calon 1.10.0.
Arsip rollback `dist/releases/1.9.0/{library,crm-runtime}` tersedia lokal.
Semua proses pemeriksaan yang terakhir diamati sudah selesai; tetap cek proses/lock
sebelum menjalankan build atau host baru.

## 3. Baca sumber yang relevan sebelum coding

Ikuti `AGENTS.md` yang berlaku untuk folder yang disentuh. Root `agent.md` adalah
kontrak produk/pricing; bukan pemberi runtime authentication/grants. Template punya
`Archava Template/AGENTS.md` sendiri dan harus tetap standalone.

Urutan baca:

1. `PRD.md`, terutama §§40–50 dan §46.6.
2. `docs/ARCHITECTURE.md`, `docs/BUILD_STATUS.md`.
3. `docs/LOCAL_LIBRARY_RELEASE.md`, `docs/LOCAL_LIBRARY_HANDOFF.md`.
4. `docs/STARTER_KIT_IMPLEMENTATION.md` — ledger programme aktif.
5. `docs/STARTER_KIT_COVERAGE_RESEARCH_2026-10-10.md`,
   `docs/BUSINESS_CAPABILITY_MATRIX.md`.
6. `packages/kits/src/catalogue.ts`, `contracts.ts`, `index.ts`;
   `packages/config/src/assembly.ts`, release contracts dan release verifier.
7. `docs/PURCHASING_MODULE_PLAN.md`, `docs/PURCHASING_MODULE.md`, seluruh diff
   purchasing/commerce/host beserta scripts pengujiannya.
8. Kontrak modul existing yang akan dikomposisikan, berikut implementation/tests.

Research audit tanggal 10 Oktober adalah history, bukan status implementasi hari ini.
Status terbaru mengikuti accepted release evidence dan kode yang benar-benar diuji.
File handoff ini menyampaikan arahan owner terbaru; Codex tidak sedang meneruskan
implementasi paralel dengan lu.

## 4. Tugas purchasing pada handoff awal — referensi historis

Review kode existing, perbaiki masalah yang ditemukan, lalu buktikan alur:

`Supplier → backend offer → reviewed draft → confirmed PO → partial inspection → stock`.

Invariants yang wajib dipertahankan:

1. Supplier unit cost berbeda dari selling price dan pricebook ARCHZXY.
   Order menerima offer ID/quantity; tidak menerima harga/total/paid dari LLM/caller.
2. Draft menyimpan quote backend. Offer berubah membuat confirm ditolak;
   operator refresh/review draft. Confirmed quote/units/lines tidak berubah lagi.
3. Hanya accepted goods menambah stock commerce; rejected goods hanya findings.
   Kebijakan awal: cumulative **accepted + rejected ≤ ordered quantity**.
   `received` berarti semua unit memiliki findings, bukan semua accepted/paid.
   Pengganti rejected goods memakai order baru; tulis batas ini dengan jelas.
4. Partial receipt, cancellation sebelum findings, short-close setelah partial,
   over-receipt refusal dan stock-capacity overflow tidak menghapus physical truth.
5. Purchasing tidak punya stock balance kedua dan tidak menulis tabel commerce
   langsung. Private commerce transaction SPI memiliki stock/claim/audit writes.
6. Purchase findings, shared movement, kedua audit dan kedua UUID claims commit
   bersama atau semuanya rollback. Same UUID/body/actor menambah stock sekali.
7. Current purchasing dan commerce grants, human/service intersection, resource
   scope dan forced tenant RLS berlaku pada read/write/recovery.
   Machine mutation/recovery tetap ditolak.
8. Generic commerce command API tidak mengambil alih purchase receipt authority.
9. Public browser output tidak membawa PG/server code atau secrets. UI memakai
   text rendering, membersihkan data saat logout/tenant switch, dan menahan UUID
   unknown/lost-response/denied retry sebelum efek baru.
10. Migration 001/002 commerce dan migration modul lama tidak ditulis ulang.
    Additive 003 mempertahankan non-empty prior data, quotes, grants, audit/receipts.

Periksa khusus source binding di deferred SQL checks, NULL/malformed JSON,
receipt-line/header linkage, sorted product locks, concurrent receipts dari order
berbeda, actor/body collisions, grant revocation dan atomic rollback multi-SKU.
Uji perilakunya di PostgreSQL sungguhan; passing mocks tidak menutup gate ini.

Urutan praktis:

- Jalankan ulang purchasing domain proof dan perbaiki sampai seluruh checks lulus.
- Jalankan actual compiled host/Chromium proof. Perbaiki selector/state/recovery
  atau backend apabila gagal. Jangan mengubah test agar menerima perilaku salah.
- Perkuat bukti concurrency/source-binding yang belum tercakup.
- Rerun gates yang terpengaruh setelah code berubah; final release memerlukan
  fresh proofs dengan profile/hashes yang sama dan regresi semua modul lama.
- Tutup owner migration, security matrix, release docs/manifest/archive/commit
  hanya setelah bukti yang diwajibkan lulus.

### Data owner yang wajib dijaga

`.local/`, `.env`, credentials, identity/fingerprint keys, provider/native control,
settings/grants, database dan backup adalah data privat penting. Jangan dibersihkan,
dibaca ke context chat, dicetak, direset atau dimasukkan Git/release artifacts.
Program backend boleh membacanya secara lokal tanpa mengeluarkan nilainya.
Gemini credential sudah ada di environment; jangan meminta key di chat atau
mengaktifkan live calls untuk menyelesaikan workflow lokal.

Backup pra-purchasing sudah tersedia:
`.local/crm/backups/pre-purchasing-1.10.0-20261011.dump`.
Baseline SQL JSON hashes privat:
`.local/crm/purchasing-upgrade-baseline.json`.
Snapshot meliputi **74 pre-existing tables**, delapan CRM business tables,
original columns dengan microsecond timestamps, settings dan key hashes.

Helper sementara `/tmp/archava-purchasing-owner.mts` mempunyai mode `capture` dan
`verify`. Inspeksi dahulu; jika hilang, buat verifier ekuivalen. **Jangan capture
ulang setelah migrasi**, karena itu bisa menutupi perubahan data yang salah.
Verifier mengizinkan tepat satu additive commerce ledger entry 003 sambil menjaga
semua original ledger rows. Sembilan purchasing business tables harus kosong dan
account tidak auto-enrolled. Tidak perlu mengaktifkan module/grants di owner
asli untuk proof; gunakan isolated hosts untuk itu.

Setelah seluruh isolated proofs lulus: host asli harus berhenti, setup/migrate
local owner, doctor, verifikasi baseline, buat backup sesudah upgrade. Pertahankan
backup lama dan arsip release. Jika kondisi lokal berbeda dari snapshot, selesaikan
penyebabnya; jangan menonaktifkan preservation assertions.

## 5. Tugas berikutnya: lanjutkan programme, bukan hanya purchasing

Setelah purchasing diterima, lanjutkan KIT-07 general families, KIT-08 specialist
adapters dan KIT-09 handoff. Daftar requirement setiap keluarga sudah ada di typed
catalogue/research. Jangan mengganti seluruhnya dengan generic CRUD atau menamai
folder sebagai kit yang selesai.

Prioritas berikut yang masuk akal, dengan dependency/domain review:

| Urutan | Pekerjaan                                              | Hasil nyata yang dibutuhkan                                                                                                        |
| ------ | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| 1      | Organizations, proposal/scope/change request, capacity | Account affiliation; versioned agreed scope/amendments; queue/start/finish estimates dari kapasitas nyata                          |
| 2      | Asset register, rental/maintenance/field-service       | Serialized identity, condition/custody, handover/return, work/parts evidence; scheduling saja tidak cukup                          |
| 3      | Subscriptions/usage/entitlements                       | Recurring lifecycle, explicit quota/access/revocation; payment truth dari authoritative backend                                    |
| 4      | Attendance, course/event capacity/check-in             | Enrollment/participant/resource access dan duplicate/check-in rules; academic domain terpisah                                      |
| 5      | Fulfillment/dispatch/production                        | Custody/pick-pack/delivery findings; bahan/output/scrap/BOM untuk produksi sederhana                                               |
| 6      | Invoice/payment boundaries                             | Immutable invoice intent/snapshot/corrections dan provider adapter; payment success hanya dari verified authority                  |
| 7      | Remaining general-domain extensions                    | Tutup requirement hotel, restaurant, repair, laundry, property, recruitment, nonprofit, content dan keluarga lain secara eksplisit |
| 8      | Archava assembly/use experience                        | Deterministic dependency/version/readiness planning dan private domain tools; NL routing tidak mendapat write authority gratis     |
| 9      | Specialist adapters + operability                      | Selected backend contracts, tenant/data projection, audit/recovery, domain/privacy review, genuine integration evidence            |
| 10     | Whole-programme handoff                                | Honest supported catalog, reproducible releases, upgrade/recovery/operator guide dan semua residual blockers                       |

Ini urutan awal yang boleh lu sesuaikan dari dependency dan bukti, bukan alasan
membangun sepuluh sistem besar sekaligus. Reuse existing capability ketika tepat.
Dua business consumer independen harus membuktikan generalisasi sebelum dinyatakan
reusable. Tidak wajib satu package/service baru untuk setiap label capability.

Katalog mencakup 24 general families dan 16 specialist families berikut. Gunakan
requirement/invariants yang rinci di source; daftar ini hanya untuk mencegah scope
hilang saat context berganti.

| ID  | Keluarga                                     |
| --- | -------------------------------------------- |
| K01 | Hotel, villa, homestay                       |
| K02 | Restaurant, cafe, food service               |
| K03 | Retail dan e-commerce                        |
| K04 | Distributor dan wholesale B2B                |
| K05 | Agency, studio, jasa proyek                  |
| K06 | Layanan profesional berbasis kasus           |
| K07 | Salon, spa, appointment personal care        |
| K08 | Gym, club, membership dengan kelas           |
| K09 | SaaS dan produk digital berlangganan         |
| K10 | Sewa kendaraan, alat, dan barang             |
| K11 | Agency real estate dan penjualan properti    |
| K12 | Pengelolaan sewa properti dan coworking      |
| K13 | Kursus, bootcamp, training cohort            |
| K14 | Sekolah dan administrasi pendidikan formal   |
| K15 | Event, ticketing dan venue                   |
| K16 | Tour, travel, dan aktivitas wisata           |
| K17 | Teknisi, instalasi, layanan lapangan         |
| K18 | Bengkel dan servis barang                    |
| K19 | Laundry dan dry cleaning                     |
| K20 | Gudang, fulfillment dan logistik barang      |
| K21 | Recruitment dan staffing agency              |
| K22 | NGO, donasi dan asosiasi                     |
| K23 | Produksi sederhana dan made-to-order         |
| K24 | Media, content studio dan penerbit           |
| K25 | Klinik dan dental outpatient                 |
| K26 | Rumah sakit dan inpatient                    |
| K27 | Lab diagnostik dan testing                   |
| K28 | Apotek dan regulated health retail           |
| K29 | Pertanian tanaman dan kehutanan              |
| K30 | Peternakan dan perikanan budidaya            |
| K31 | Konstruksi, kontraktor dan EPC               |
| K32 | Manufaktur industri dan regulated production |
| K33 | Pertambangan dan industri ekstraktif         |
| K34 | Utilitas listrik, gas dan air                |
| K35 | Pengelolaan limbah dan jasa lingkungan       |
| K36 | Banking, lending dan layanan keuangan        |
| K37 | Asuransi dan claims                          |
| K38 | Marketplace dan transaksi multipihak         |
| K39 | Layanan publik dan administrasi pemerintahan |
| K40 | Residential care dan layanan sosial          |

Specialist integration yang belum dipilih backend-nya adalah **blocker nyata**.
Jangan memilih vendor berbayar/regulated scope secara diam-diam atau mengarang
live evidence. Siapkan boundary, contracts, synthetic contract tests dan pending
requirements; minta input backend/domain yang konkret sambil melanjutkan pekerjaan
independen. Fake gateways sah untuk failure tests, bukan bukti pembayaran, regulator,
live integration atau production support.

## 6. Quality, security dan scalability

- Preserve passing behavior, exact consent/action boundaries dan unresolved receipt
  recovery. No blanket grants, owner impersonation atau secrets di prompts.
- Tenant isolation dan resource authorization harus server-side dan diperiksa ulang
  saat efek/recovery; permission cache tidak boleh memperpanjang revoked access.
- Global catalog/stock authority tidak otomatis didapat dari assigned order access.
- Bounded inputs/pages/quantities, canonical UUID intent, optimistic revisions,
  atomic audit, rollback, database clocks dan explicit timezone/unit rules.
- Module ownership/API/SPI jelas; cross-domain writes memakai module-owned ports.
- Curated dependencies, existing pins/lockfiles, review install scripts. Prefer
  existing tested primitives. Jangan menjalankan audit fix/upgrade massal tanpa
  analisis compatibility dan regression evidence.
- Document actual compatibility matrix, additive migrations dan rollback limits.
  Jangan mengklaim ratusan kombinasi jika baru fixed profile yang diuji.
- Ukur scalability dengan declared workload/concurrency, bukan jumlah packages atau
  persen tebakan. Gunakan existing benchmark, lalu perluas untuk bottleneck nyata.
- Observability metadata-only untuk tool/webhook/queue/error/cost; backups dengan
  tested restore. RPO/RTO/SLA produksi butuh evidence dan operator policy tersendiri.
- Realtime template tetap standalone. Review 13 moderate advisory findings dan
  SDK/live gaps dengan instructions template; jangan menutupnya memakai test root.

## 7. Cara menjalankan checks tanpa merusak artifact evidence

Toolchain lokal: **Node 22.22.3, pnpm 10.15.1**.
Contoh prefix yang tersedia: `fnm exec --using 22.22.3 pnpm ...`.

**Jangan menjalankan packers bersamaan.** `pnpm verify` (reuse gate), `library:pack`,
`crm:verify`, `act:verify`, `crm:build` dan beberapa wrapper domain menyiapkan ulang
shared `dist/library`/runtime. Cold-installed consumers harus membaca artifact
set yang stabil. Jangan start host/browser proofs saat runtime proof sedang
menjalankan fase tamper installed artifacts.

Contoh pengujian mandiri purchasing, jalankan berurutan:

```bash
fnm exec --using 22.22.3 pnpm purchasing:verify
fnm exec --using 22.22.3 pnpm purchasing:verify-host
```

Untuk final regression, urutkan pack/build preparation:

```bash
fnm exec --using 22.22.3 pnpm verify
fnm exec --using 22.22.3 pnpm crm:verify
fnm exec --using 22.22.3 pnpm act:verify
fnm exec --using 22.22.3 pnpm crm:verify-browser
```

Setelah shared artifacts stabil, jalankan scripts domain/host langsung lewat Node
untuk menghindari wrapper yang repack. Scripts tersedia di `tools/library/` untuk
projects, scheduling, accommodation, commerce, foodservice, support, purchasing,
kits dan assembly. Upgrade proof: `node apps/crm/scripts/upgrade.mjs`.
Inspect setiap exit status dan report; parallel hanya independent isolated jobs.

Security full runner `pnpm security:verify` menjalankan ulang prep/proofs; jangan
bersamaan dengan cold consumers. Jika seluruh fresh reports sudah tersedia, gunakan
fresh unit JSON dan `node tools/security/verify.mjs` tanpa `--run`:

```bash
fnm exec --using 22.22.3 pnpm exec vitest run --reporter=json --outputFile=dist/security-proof/unit.json
fnm exec --using 22.22.3 node tools/security/verify.mjs
```

Sesuaikan commands dari `package.json` jika source berubah. Root verification saja
bukan pengganti PostgreSQL, Chromium, upgrade atau template/provider checks.
Logs debug terakhir ada di `/tmp/purchasing-*.log`; gunakan seperlunya dan jangan
memperlakukan log yang gagal/stale sebagai accepted evidence.

## 8. Definition of done setiap milestone

Sebelum menyebut milestone diterima:

1. Workflow runnable dengan domain invariants, current authorization dan recovery.
2. Dua independent installed consumers memakai artifact yang sama tanpa source
   workspace imports/forks; browser/server surfaces terpisah.
3. Meaningful domain/database/race/rollback/restart/restore checks dan actual owner
   host/browser flow lulus. Scope, workload dan limitations ditulis.
4. Current compatibility/config planner menolak missing/disabled/incompatible
   dependencies. Enabling feature tidak memberi grants.
5. Semua existing behavior yang terpengaruh punya fresh regression evidence;
   installed profiles/hashes/ledgers konsisten.
6. Original owner preservation verified, before/after backups tersedia ketika
   migration dilakukan; existing flags, grants dan keys tetap sesuai baseline.
7. Update README/PRD, architecture, build status, capability matrix, programme ledger,
   release/handoff, module guide dan sanitized audit manifest dengan hasil aktual.
8. Archive exact matching library/runtime sebagai release baru tanpa overwrite
   arsip lama. Commit lokal hanya milestone yang sudah terbukti, tanpa private data.
   Jangan push/publish kecuali owner memberi instruksi itu.
9. Sisakan outstanding requirements di ledger; jangan mengubah narrow scope menjadi
   full-kit acceptance atau menyebut seluruh Archava selesai dari jumlah tests.

## 9. Reporting untuk owner / Codex

Buat progress singkat pada batas milestone; jangan minta konfirmasi berulang untuk
reversible work yang sudah diotorisasi. Saat review dibutuhkan, laporkan:

- commit dan accepted profile;
- workflow/consumer yang sekarang usable;
- checks yang benar-benar dijalankan, hasil dan file evidence;
- perubahan authorization/schema dan preservation/recovery;
- remaining requirements, blockers konkret dan next executable task.

Kalau belum selesai, bilang belum selesai. Jangan memberi persentase seluruh
produk, universal business coverage atau security tanpa denominator/evidence.
Codex boleh dipanggil owner untuk audit/review, tetapi jangan menunggu Codex untuk
setiap keputusan implementasi rutin.

**Mulai sekarang:** ikuti tugas review terbaru di §0. Perbaiki authorization,
recovery dan capacity, tuntaskan release/host integration beserta acceptance baru,
lalu lanjut programme dari ledger. Preserve pekerjaan existing dan data owner.
