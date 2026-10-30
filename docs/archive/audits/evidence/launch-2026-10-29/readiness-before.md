# Archava — Acuan Kerja Menuju E2E dan Rilis

> **Status: M0 (Scope & Kriteria Penerimaan) & M1 (Audit Baseline) SELESAI; M2–M3 (Pembuktian Flow & Remediasi Audit) LULUS VERIFIKASI.**  
> Dokumen ini adalah acuan kanonikal scope, matriks status, urutan kerja, dan bukti pengujian resmi menuju peluncuran rilis Sovereign Business Engine Archava.

---

## 1. Tujuan dan Batas Scope

**Tujuan:** Pengguna dapat menyelesaikan pekerjaan bisnis nyata melalui UI dan API, dengan otorisasi berbasis hak akses ketat, efek persistensi database yang konsisten, audit trail yang dapat dipertanggungjawabkan, serta operasi sistem yang dapat dipulihkan (*disaster recovery*).

- **Cakupan Inventaris & Audit:** Workspace Rust Archava (22 crates), UI ERP/CRM (SSR + WASM hydration Leptos), HTTP REST API, PostgreSQL dengan FORCE RLS, integrasi AI/voice/provider, tooling rilis, dan dokumentasi aktif.
- **Pemisahan Template Demo:** Proyek starter kit/demo eksternal (`Lab/Client/archava-template/`) telah di-detach ke workspace client terpisah agar core repo murni fokus sebagai sovereign engine.
- **Kebenaran Efek Bisnis:** Data demo dan simulasi provider secara tegas dibedakan dari efek transaksi durable pada database.

### Keputusan Scope Rilis (M0 — Canonical Decisions)

| Keputusan Scope Rilis | Status | Keputusan & Pengaruh Arsitektural |
| :--- | :--- | :--- |
| **Pengguna/jenis bisnis pertama dan pekerjaan utamanya** | **Ditetapkan** | **Target:** Operator SME modern, Digital Agency, B2B Consultancy, & Direct Commerce.<br>**Pekerjaan Utama:** CRM Contact lifecycle, tracking aktivitas/follow-up, order capture, purchasing & landed-cost inventory costing, billing/invoicing, support case management, dan sovereign encrypted backup/restore. |
| **Modul yang dijanjikan pada rilis pertama (MVP Scope)** | **Ditetapkan** | **Tier 1 (Wajib Rilis / MVP Core):**<br>- `D01`: CRM (Contacts, Interactions, Follow-ups, Activity Log)<br>- `D02`: Commerce (Catalog, Orders, Stock Movement)<br>- `D03`: Purchasing (Procurement, Landed-cost FIFO Allocation, Goods Receipt)<br>- `D09`: Billing (Invoices, Authoritative Payment state)<br>- `D12`: Support Cases (Tickets, Case History, Status Transitions)<br>- `Cockpit`: Sovereign Mission DAG Execution & Agent Orchestration<br>- `Ops`: Encrypted Backup/Restore & Lease Fencing.<br><br>**Tier 2 (Extended / Modular Ledgers):**<br>`D04` (Foodservice), `D05` (Projects), `D06` (Reservations), `D07` (Accommodation), `D08` (Assets), `D10` (Subscriptions), `D11` (Attendance), `D13` (Fulfillment), `D14` (Laundry), `D15` (Field Services), `D16` (Formal Education).<br>*Catatan:* Seluruh 17 modul telah terdaftar di database migration, schema RLS aktif, dan lulus readiness boot check (`assert_erp_readiness`). Namun marketing/onboarding awal difokuskan pada Tier 1. |
| **Peran pengguna dan model tenant/deployment** | **Ditetapkan** | **Peran:**<br>- `Owner / Admin`: Full administrative, sovereign configuration, module management, backup/restore.<br>- `Member / Operator`: Domain-bounded transactional authority (read/write sesuai grant).<br>- `Worker / Agent`: Task-scoped bounded capability.<br>**Model:** Dedicated single-tenant instance deployment per company, didukung PostgreSQL multi-tenant FORCE RLS (`archava.tenant_id`) untuk isolasi absolut. |
| **Integrasi eksternal yang wajib bekerja** | **Ditetapkan** | - Resend / SMTP untuk notifikasi email transaksional.<br>- LiveKit WebRTC + Gemini Multimodal Realtime untuk asisten suara & visual planning.<br>- Webhooks (Midtrans, Privy) strictly fail-closed: unconfigured secret menghasilkan 503 Service Unavailable, menolak fallback secret sembarangan. |
| **Target beban, latensi, serta pemulihan data** | **Ditetapkan** | - **Latensi p95:** < 50ms untuk internal API queries, < 250ms untuk mutasi transaksional kompleks.<br>- **Durability:** RPO = 0 (PostgreSQL WAL synchronous commit).<br>- **Disaster Recovery:** RTO < 5 menit untuk full database recovery via `archava-ops restore` dengan enkripsi AES-256-GCM dan mode staging `0o600`. |

---

## 2. Aturan Kerja

1. **Baca acuan kanonikal:** Pilih ID pekerjaan/flow, lalu catat tujuan, file yang disentuh, baseline source, dan bukti yang dihasilkan.
2. **Buktikan dengan source dan hasil tes aktual:** Rujuk [Arsitektur](../architecture/README.md), [Panduan Developer](../DEVELOPER_GUIDE.md), dan [Laporan Remediasi Audit](../archive/audits/ARCHAVA_REAUDIT_REMEDIATION_REPORT_2026-10-28.md).
3. **Pemisahan temuan yang jujur:** Bedakan kode yang hanya ada sebagai draft, route yang aktif, tes yang tersedia, dan tes yang benar-benar dijalankan.
4. **Prioritas ketat:** Dahulukan integritas data, otorisasi fail-closed, dan flow wajib (P0/P1). Refactoring kosmetik dilarang memutus fungsionalitas inti.
5. **Fail-Closed Standard:** Setiap error koneksi, authorization mismatch, atau input overflow wajib menolak transaksi secara eksplisit; dilarang keras menelan error dengan silent fallback.
6. **Dokumentasi hidup:** Perbarui panduan aktif bersamaan dengan perubahan kode.

---

## 3. Urutan Kerja dan Kriteria Keluar (Milestones M0 – M6)

| ID | Tahap | Pekerjaan | Kriteria Keluar | Status |
| :--- | :--- | :--- | :--- | :--- |
| **M0** | **Scope dan Penerimaan** | Tetapkan keputusan scope produk, persona, modul Tier 1, dan matriks batas sistem. | Scope rilis dan matriks peran/flow jelas dan disepakati. | **LULUS** |
| **M1** | **Audit dan Baseline** | Rekam working tree, dependensi, toolchain; jalankan suite baseline struktur, docs, lint, dan workspace tests. | Hasil tes baseline 100% hijau, 0 error pada structure & docs. | **LULUS** |
| **M2** | **Pembuktian Flow** | Telusuri journey wajib (F01–F08) dari UI ke otorisasi, domain, transaksi database, receipt, hingga UI. | Setiap flow wajib memiliki integration test dan pembuktian boundary. | **LULUS** |
| **M3** | **Perbaikan dan Regresi** | Selesaikan seluruh temuan audit R01–R09 (tenant auth fail-open, fake receipt, migration manifests, domain invariants, dll.). | Lulus audit gate, 0 warnings clippy, 0 formatting diffs, 980+ tests pass. | **LULUS** |
| **M4** | **Operasi di Staging** | Uji fresh install, migrasi 17 modul ERP, single-writer lease, backup/restore rehearsal AES-256-GCM. | Database restore berhasil, staging mode `0o600`, server launcher persistent ready. | **LULUS** |
| **M5** | **Dokumen dan Pilot** | Rekonsiliasi developer guide, module documentation, dan petunjuk setup native Rust. | Dokumentasi akurat, tidak memuat instruksi TS lama yang usang. | **BERJALAN** |
| **M6** | **Verifikasi Kandidat Rilis** | Pipeline CI GitHub Actions (`ci.yml`) dan release packager (`build-slice.mjs`) menghasilkan paket native + browser WASM. | Semua checklist Bagian 7 terpenuhi; SHA-256 artifacts siap rilis. | **BERJALAN** |

---

## 4. Journey Lintas Modul (F01 – F08)

| ID | Journey | Bukti Implementasi & Pengujian | Status |
| :--- | :--- | :--- | :--- |
| **F01** | **Setup Workspace & Sign-In/Sign-Out** | Sesi berbasis HTTP-only cookie dengan CSRF protection, KDF derivation budget (`tests/kdf_budget.rs`), rate limiting IP-based, dan interceptor 401 redirect ke `/app` (`tests/session.rs`, `crates/slice/src/session.rs`). | **LULUS** |
| **F02** | **Contacts sampai Follow-up & Receipt** | SSR + WASM hydration contact list (`crates/slice/src/home.rs`), REST API `/contacts` dan `/activities`, persistensi PostgreSQL dengan audit trail, follow-up state transition, dan deterministic receipt (`tests/crm.rs` 36/36 passed). | **LULUS** |
| **F03** | **Retry dan Recovery** | Idempotency token per operasi, request replay protection, duplicate submission prevention, dan crash-recovery state table (`tests/own.rs`, `tests/crm.rs`). | **LULUS** |
| **F04** | **Isolasi dan Perubahan Otorisasi** | Isolasi tenant via `run_tenant_transaction` dengan GUC `archava.tenant_id`, FORCE RLS non-owner runtime, penolakan instan akses lintas tenant, dan revocation propagation (`tests/authority.rs`, `tests/tenant_context.rs`). | **LULUS** |
| **F05** | **Owner Settings/Configuration** | Pengaturan sovereign owner, konfigurasi modul bisnis, catalog/pricing rule verification dengan proteksi kredensial owner (`tests/own.rs`). | **LULUS** |
| **F06** | **Assistant / Provider Planning** | Gemini multimodal realtime planning, tool definition boundary, token quota & daily budget enforcement, graceful degradation, dan hold status pasca-restore (`tests/assistant.rs`, `tests/provider_contracts.rs`, `tests/provider_runtime.rs`). | **LULUS** |
| **F07** | **Integrasi Bisnis Wajib** | Fail-closed webhook verification untuk Midtrans dan Privy (503 jika secret belum dikonfigurasi; tolak secret fallback default), checkout embed dengan iframe origin boundary (`crates/host/src/webhooks.rs`, `router.rs`). | **LULUS** |
| **F08** | **Operasi & Disaster Recovery** | Single-writer lease fencing, archive terenkripsi AES-256-GCM, private staging file mode `0o600` via `OpenOptionsExt`, dan zero-downtime persistent server boot (`crates/ops/src/backup.rs`, `tests/upgrade_restore.rs`). | **LULUS** |

---

## 5. Inventaris Seluruh Modul (D01 – D16)

Semua modul telah memiliki database schema SQL, migrasi terdaftar, dan lulus startup readiness gate (`assert_erp_readiness`).

### Tier 1 — Core Release Scope (MVP)

| ID | Modul | Cakupan Alur Bisnis | Status Uji Integrasi | Status Rilis |
| :--- | :--- | :--- | :--- | :--- |
| **D01** | [CRM](../modules/CRM_MODULE.md) | Manajemen Kontak → Aktivitas & Log Interaksi → Follow-up → Receipt & Audit Trail | **LULUS** (`tests/crm.rs`, 36/36 passed) | **Wajib Rilis (Tier 1)** |
| **D02** | [Commerce](../modules/COMMERCE_MODULE.md) | Manajemen Katalog → Pembuatan Order → Reservasi Stok → Return / Cancelation | **LULUS** (`crates/adapters/tests/tenant_context.rs`, `crates/host/tests/readiness.rs`) | **Wajib Rilis (Tier 1)** |
| **D03** | [Purchasing](../modules/PURCHASING_MODULE.md) | Purchase Order → Penerimaan Barang (Goods Receipt) → FIFO Landed Costing & Audit | **LULUS** (`tests/erp_purchasing.rs`, 10/10 passed) | **Wajib Rilis (Tier 1)** |
| **D09** | [Billing](../modules/BILLING_MODULE.md) | Tagihan (Invoice) → Pencatatan Pembayaran Otoritatif → Reconciliation Ledger | **LULUS** (`tests/erp_billing.rs`, 10/10 passed) | **Wajib Rilis (Tier 1)** |
| **D12** | [Support Cases](../modules/SUPPORT_CASES_MODULE.md) | Tiket Bantuan → Catatan Internal & Audit → State Transition Lifecycle | **LULUS** (`tests/erp_support.rs`, 12/12 passed) | **Wajib Rilis (Tier 1)** |

### Tier 2 — Extended / Modular Ledgers

| ID | Modul | Cakupan Alur Bisnis | Status Uji Integrasi | Status Rilis |
| :--- | :--- | :--- | :--- | :--- |
| **D04** | [Foodservice](../modules/FOODSERVICE_MODULE.md) | Kitchen Ticket → Order Processing → Efek Stok Resep → Penyelesaian | **LULUS** (`tests/erp_foodservice.rs`, 10/10 passed) | Extended (Modular) |
| **D05** | [Project Delivery](../modules/PROJECT_DELIVERY_MODULE.md) | Proyek → Milestones & Tasks → Progres & Bukti Penyelesaian | **LULUS** (`tests/erp_projects.rs`, 10/10 passed) | Extended (Modular) |
| **D06** | [Resource Reservations](../modules/RESOURCE_RESERVATIONS_MODULE.md) | Slot Reservasi → Kapasitas Resource → Modifikasi / Pembatalan | **LULUS** (`tests/erp_scheduling.rs`, 10/10 passed) | Extended (Modular) |
| **D07** | [Accommodation](../modules/ACCOMMODATION_MODULE.md) | Booking Kamar/Stay → Occupancy Tracking → Housekeeping → Checkout | **LULUS** (`tests/erp_accommodation.rs`, 10/10 passed) | Extended (Modular) |
| **D08** | [Assets](../modules/ASSETS_MODULE.md) | Inventaris Aset Fisik → Log Custody / Peminjaman → Maintenance | **LULUS** (`tests/erp_assets.rs`, 10/10 passed) | Extended (Modular) |
| **D10** | [Subscriptions](../modules/SUBSCRIPTIONS_MODULE.md) | Paket Berlangganan → Entitlement Hak Akses → Siklus Penagihan Periode | **LULUS** (`tests/erp_subscriptions.rs`, 19/19 passed) | Extended (Modular) |
| **D11** | [Attendance](../modules/ATTENDANCE_MODULE.md) | Sesi Kerja/Event → Roster Peserta → Check-In QR/Log & Batas Kapasitas | **LULUS** (`tests/erp_attendance.rs`, 16/16 passed) | Extended (Modular) |
| **D13** | [Fulfillment](../modules/FULFILLMENT_MODULE.md) | Production Job → Tahap Manufaktur/Packing → Dispatch & Handover | **LULUS** (`tests/erp_fulfillment.rs`, 10/10 passed) | Extended (Modular) |
| **D14** | [Laundry](../modules/LAUNDRY_MODULE.md) | Timbangan Intake → Cuci / Setrika → Quality Control → Serah-Terima | **LULUS** (`tests/erp_laundry.rs`, 10/10 passed) | Extended (Modular) |
| **D15** | [Field Services](../modules/FIELD_SERVICES_MODULE.md) | Work Order → Penugasan Teknisi → Log Kunjungan & Bukti Lapangan | **LULUS** (`tests/erp_field_services.rs`, 10/10 passed) | Extended (Modular) |
| **D16** | [Formal Education](../modules/FORMAL_EDUCATION_MODULE.md) | Pendaftaran Siswa → Kelas & Kehadiran → Penilaian & Nilai Akhir | **LULUS** (`tests/erp_formal_education.rs`, 10/10 passed) | Extended (Modular) |

---

## 6. Status, Bukti, dan Rekam Eksekusi Baseline

Tabel eksekusi aktual yang dijalankan pada environment lokal terverifikasi:

| ID Pekerjaan | Baseline Source / Target | Metode & Perintah | Hasil + Exit Code | Bukti Verifikasi |
| :--- | :--- | :--- | :--- | :--- |
| **B01** | Repo Structure (22 Crates) | `node tools/check-structure.mjs` | **LULUS (Exit 0)**: 22 crates; 0 problems | Console log output |
| **B02** | Documentation Links (61 files) | `node tools/check-docs.mjs` | **LULUS (Exit 0)**: 61 maintained files, 674 local links, 0 broken | Console log output |
| **B03** | Tool Unit Tests (15 tests) | `node --test tools/test/*.test.mjs` | **LULUS (Exit 0)**: 15 passed, 0 failed | Node runner report |
| **B04** | Rust Formatting | `cargo fmt --all --check` | **LULUS (Exit 0)**: 0 diffs across workspace | Formatting gate |
| **B05** | Compiler Linter | `cargo clippy --workspace --all-targets -- -D warnings` | **LULUS (Exit 0)**: 0 warnings, 0 errors | Clippy gate |
| **B06** | Full Workspace Integration Suite | `cargo test --workspace --locked -- --test-threads=1` | **LULUS (Exit 0)**: 980+ tests passed across 59 cohorts, 0 failures | Test log output |
| **B07** | Client WASM Hydration Check | `cargo check --target wasm32-unknown-unknown -p archava-slice --lib --features hydrate` | **LULUS (Exit 0)**: Finished in 0.88s | WASM compiler check |
| **B08** | Production Build & Packaging | `node tools/rust/build-slice.mjs` | **LULUS (Exit 0)**: Server binary (12MB) + WASM package (1.6MB) | `target/site/pkg/` artifacts |
| **B09** | Security & Audit Remediation | Audit R01–R09 Hardening Verification | **LULUS**: Fail-closed authz, no fake receipt, 17 manifests, 0600 mode | [`ARCHAVA_REAUDIT_REMEDIATION_REPORT_2026-10-28.md`](../archive/audits/ARCHAVA_REAUDIT_REMEDIATION_REPORT_2026-10-28.md) |

---

## 7. Kriteria Siap Rilis (Launch Readiness Checklist)

- [x] **Scope rilis ditetapkan:** Target pengguna, model peran (Owner/Member/Worker), model tenant PostgreSQL RLS, dan modul Tier 1 telah didefinisikan secara konkret.
- [x] **Seluruh flow wajib (F01–F08) terbukti:** Setup, CRM interaction, idempotency, isolasi tenant, setting owner, provider bounds, webhooks, dan backup/restore lulus uji integrasi.
- [x] **Zero P0/P1 Terbuka:** Seluruh temuan audit Codex (R01 hingga R09) telah diremediasi secara tuntas dan diverifikasi dengan tes regresi.
- [x] **Otorisasi dan Isolasi Tenant Fail-Closed:** Kebocoran RLS dan unhandled errors telah diperbaiki; member sah berjalan di bawah non-owner FORCE RLS, non-member langsung ditolak.
- [x] **Durabilitas Database Terjamin:** 17 modul ERP terdaftar di readiness check startup; write operations durable sebelum acknowledgement.
- [x] **Keamanan Operasi dan Restore:** Backup terenkripsi AES-256-GCM, writer lease fencing terbukti aktif, dan staging restore dibuat dengan atomic mode `0o600`.
- [x] **Pipeline CI & Release Packaging Reproducible:** Konfigurasi GitHub Actions terisolasi pada toolchain Rust dan pengemasan native server + WASM client bundle siap deploy.
- [ ] **Deployment Staging & Live Smoke Test:** Server dijalankan di port publik/staging dengan domain target untuk final user sanity check sebelum traffic dibuka ke publik.

---

## 8. Titik Mulai dan Referensi Bukti

- **Laporan Remediasi Audit Resmi:** [`docs/audit/ARCHAVA_REAUDIT_REMEDIATION_REPORT_2026-10-28.md`](../archive/audits/ARCHAVA_REAUDIT_REMEDIATION_REPORT_2026-10-28.md)
- **Pipeline CI Aktif:** [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)
- **Packager Rilis:** [`tools/rust/build-slice.mjs`](../../tools/rust/build-slice.mjs)
- **Katalog Tes Integrasi Host:** [`crates/host/tests/`](../../crates/host/tests/)
- **Operations UI & Server Runtime:** [`crates/slice/src/lib.rs`](../../crates/slice/src/lib.rs) dan [`crates/slice/src/bin/archava_server.rs`](../../crates/slice/src/bin/archava_server.rs)
