# Archava — ERP/CRM mengikuti bisnis, dirakit sendiri oleh owner

<!-- documentation-status -->
> **Status: requirements/proposal reference; implementation acceptance is separate.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

**Version:** 1.0.0  
**Date:** 15 Oktober 2026  
**Status:** Product requirement yang diperjelas owner; belum implemented/accepted.  
**Product authority:** [PRD produk aktif](PRD.md), dengan detail self-service pada
dokumen ini. Asal requirement tetap dapat ditelusuri ke
[Master PRD historis §51](../archive/history/PRD.md#51-business-shaped-crmerp-and-owner-self-service).
Pricebook existing tetap berlaku. Ini tidak mengubah harga, release acceptance
atau existing business invariants; konteks requirement bertanggal di bawah dipertahankan.

## 1. Thesis produk

**Owner menjelaskan cara bisnisnya bekerja; Archava membantu menyusun sistem dari
capabilities yang tersedia dan kompatibel.** Owner dapat mengatur supported
modules/options melalui percakapan atau dashboard tanpa harus menulis kode,
mengubah file konfigurasi, atau meminta proyek custom untuk setiap penyesuaian.

CRM/ERP mengikuti kebutuhan bisnis melalui shared primitives, tested modules,
configuration, vertical extensions dan existing-system adapters. Kit merupakan
titik awal, bukan kotak permanen yang membatasi bisnis. Hotel dapat menambahkan
laundry/restaurant/events ketika supported; tidak harus mengganti seluruh sistem
atau menyalin core menjadi fork hotel lain.

Ada dua manfaat reuse yang berbeda:

1. **Untuk ARCHZXY/developer:** engine dan UI modules yang sama dapat dipakai
   ulang antar consumer tanpa copying/forking.
2. **Untuk owner bisnis:** capabilities/options yang didukung dapat dirakit dan
   diubah sendiri dengan bantuan Archava, tanpa pengetahuan pemrograman.

Existing module library memenuhi arah manfaat pertama secara bertahap. Ia belum
otomatis membuktikan manfaat kedua sampai actual owner journey diterapkan dan diuji.

## 2. Posisi dan batas janji

Customization bukan hal yang sama sekali baru pada ERP: ERPNext, misalnya,
mendokumentasikan customization fields, reports, print formats dan workflows.
[ERPNext customization](https://docs.frappe.io/erpnext/customize-erpnext).
Jadi diferensiasi yang harus kita buktikan adalah kemudahan owner menjelaskan
kebutuhan dalam bahasa bisnis, mendapatkan komposisi yang valid, memahami dampak,
dan menerapkannya dengan recovery serta audit yang benar.

“Seperti puzzle” berarti kepingan yang mempunyai kontrak dan kombinasi yang diuji.
Itu bukan janji semua kebutuhan, semua industri, semua perubahan gratis, atau
setiap kalimat owner bisa menjadi software baru secara otomatis.

| Kategori permintaan                               | Perilaku produk                                                                                             |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Supported configuration                           | Owner dapat preview/confirm/apply sendiri dengan current authority                                          |
| Installed module yang tersedia sesuai entitlement | Owner dapat mengaktifkan lewat validated configuration; tidak perlu proyek code fork                        |
| Known module yang memerlukan installation/upgrade | Tampilkan readiness, biaya/requirements dan proses instalasi yang didukung; jangan berpura-pura sudah aktif |
| Domain extension yang belum tersedia              | Nyatakan gap dan tawarkan bounded extension scope, estimate/review atau existing-system integration         |
| Perubahan yang merusak data/invariants/authority  | Tolak atau arahkan ke explicit supported adjustment/migration process                                       |

Perubahan konfigurasi yang didukung tidak boleh otomatis diklasifikasikan sebagai
proyek development custom. Biaya module/tier, third-party usage, support atau
extension baru tetap mengikuti entitlement, agreement dan deterministic pricing
yang berlaku. Tidak ada perubahan pricebook atau janji unlimited free support
dari requirement ini. Owner melihat biaya/ketidakpastian sebelum persetujuan;
LLM tidak menentukan harga sendiri.

## 3. Contoh pengalaman owner

Contoh ini target UX, bukan klaim seluruh domain sudah ready.

**Agency:** “Pipeline gua inquiry, survey, penawaran, deal. Pakai istilah client,
bukan customer.” Archava memetakan ke supported stage labels/outcomes dan contact
terminology, menunjukkan preview serta aturan yang tetap berlaku. Jika request
mengubah makna lifecycle, sistem menawarkan supported workflow mapping atau gap,
bukan sekadar mengganti label untuk menyamarkan mismatch.

**Hotel:** “Tambahin laundry, gua belum perlu restaurant.” Archava menunjukkan
module availability, dependencies, data/UI/actions yang berubah dan cost impact.
Jika restaurant sudah mempunyai active orders, disable tidak boleh membuang order
atau memutus dependency diam-diam. Historical access/retention tetap jelas.

**Distributor:** “Pesanan pembelian di atas batas tertentu perlu approval manager.”
Jika domain menyediakan bounded approval policy itu, Archava menyiapkan rule dan
menunjukkan approver/amount behavior. Jika belum supported, permintaan menjadi
declared capability gap; model tidak membuat executable workflow sendiri.

## 4. Alur yang wajib tersedia

**OWN-01 — Inspect.** Owner dapat melihat current business profile, installed/enabled
capabilities, configurable options, permissions yang ia miliki, dan kebutuhan yang
belum tersedia. Menampilkan katalog tidak memberikan entitlement atau grants.

**OWN-02 — Discover.** Archava menanyakan hanya business details yang belum jelas,
kemudian menghasilkan structured requirements. Jangan memaksa memilih satu label
industri untuk bisnis hybrid atau meminta owner memahami crate/schema/env vars.

**OWN-03 — Plan.** Backend menyusun immutable proposed configuration dari trusted
catalogue dan current state. Dependency resolution, compatibility, permissions,
data lifecycle, readiness dan pricing dievaluasi deterministik. LLM hanya
mengusulkan structured input; tidak menjalankan schema edits atau arbitrary code.

**OWN-04 — Preview.** Tampilkan dalam bahasa bisnis: apa yang ditambah/diubah,
fitur/UI/workflows yang terdampak, dependencies, active records/jobs yang perlu
diselesaikan, biaya implementasi/recurring/usage bila berlaku, perkiraan gangguan,
dan recovery path. Missing price/readiness menjadi explicit unknown/review state.

**OWN-05 — Confirm.** Persetujuan terikat tenant/principal, operation, plan/input
hash, base config revision, catalogue/release compatibility, pricing snapshot bila
relevan, affected resources dan expiry. Tidak ada `approved: true` dari model/body
yang dapat menggantikan server-verifiable approval.

**OWN-06 — Apply.** Backend mengecek ulang current authority dan preconditions,
mengklaim request idempotency, lalu menerapkan melalui supported transaction/job.
Tampilkan progress/outcome dari authoritative backend. “Applied” hanya setelah
actual runtime configuration/activation yang relevan terkonfirmasi.

**OWN-07 — Recover.** Setelah refresh, connection loss atau worker restart, owner
dapat mengecek request yang sama tanpa double activation/charge. Unknown outcome
direkonsiliasi sebelum retry. Current authority tetap diperiksa untuk status/replay.

**OWN-08 — History/restore.** Tampilkan versi konfigurasi dan audit perubahan.
Restore supported prior configuration membuat versi baru setelah compatibility
check; tidak berarti mengembalikan seluruh database ke masa lalu atau membatalkan
payment, stock, invoice, agreement dan pekerjaan bisnis yang sudah terjadi.

Dashboard/manual configuration harus memakai service/policy yang sama dan tetap
berfungsi bila LLM tidak tersedia. Percakapan adalah interface tambahan yang
mempermudah owner, bukan satu-satunya cara mengoperasikan bisnis.

## 5. Apa yang dapat dikonfigurasi

| Capability                                      | Batas yang harus dijaga                                                                             |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Terminology, labels dan navigation              | IDs/semantics tetap stabil; perubahan nama tidak mengubah permissions/data history                  |
| Views, visible fields dan dashboard composition | Server-side field visibility tetap berlaku pada API/export/SSR; hidden UI bukan security control    |
| Module enable/disable dan supported options     | Installed code, dependencies, entitlement dan resource lifecycle diperiksa                          |
| Pipeline/workflow options yang domain dukung    | Preserve canonical outcomes/invariants dan versioned historical meaning                             |
| Supported roles/approval policies               | Specific management authority; platform-required approvals tidak dapat diturunkan                   |
| Typed custom fields/integrations                | Hanya setelah manifest/schema/privacy/migration contracts tersedia; bukan arbitrary tenant code/SQL |

Owner dapat mengatur bisnisnya dalam supported configuration surface. Owner tidak
dapat mengubah authoritative payment truth, tenant isolation, audit history,
required permission checks, posted records atau security minimums lewat percakapan.

Configurable workflows menggunakan bounded domain contracts. Jangan membangun
universal workflow/EAV/plugin engine untuk menyatakan semua industri tercakup.
Kebutuhan yang belum didukung harus terlihat dan dapat direncanakan sebagai
extension; jangan dipaksa masuk generic contact/order/table yang salah makna.

## 6. Owner surface dan platform/internal Studio

Internal ARCHZXY Studio dan tenant-owner configuration memiliki pengguna serta
otoritas berbeda, meskipun memakai shared catalogue/resolver/pricing primitives.

- Internal Studio dapat menyiapkan platform/client delivery sesuai operator grants.
- Owner surface hanya menampilkan tindakan/config/data yang permitted bagi tenant
  dan principal tersebut. Tidak membuka platform internals, tenant lain, secrets,
  package installation bebas, atau management privileges otomatis.
- Public/customer Archava tidak memperoleh owner configuration tools. Role, tenant,
  grants dan approval berasal dari verified server context, bukan pesan/body.

Public service contracts mencakup inspect, plan, preview/validate, apply, status
dan compatible restore. Stable IDs/permissions/schemas dipilih dan diregistrasi
saat implementasi; nama konseptual di dokumen ini bukan existing callable tools.

## 7. Runtime configuration dan installation

Supported settings/modul yang sudah terpasang dapat diaktifkan lewat runtime
configuration jika lifecycle/compatibility mengizinkan. Module code yang belum
ada pada host memerlukan controlled installation/upgrade/build/restart sesuai
deployment model. Owner melihat readiness/progress; tidak harus menjalankan Cargo,
SQL, terminal atau mengedit source.

Control plane tidak men-download/evaluate package berdasarkan input bebas browser
atau LLM. Hanya trusted, versioned, compatible artifacts. Saat modul dilepas,
dependencies dan active records/jobs diperiksa; disable bukan destructive uninstall.

Configuration concurrency memakai expected revision dan explicit writer ownership.
Apply request kedua terhadap stale base ditolak untuk replan; tidak silently merge
perubahan yang mengubah data/authority/cost. Jobs menyimpan receipt/status dan
failure/recovery reason. Modul/policy yang disabled tidak dapat dipanggil lewat
direct API meskipun UI atau tool cache lama masih terbuka.

## 8. Delivery scope dan hubungannya dengan migrasi Rust

Ini product requirement yang diperjelas saat migrasi berjalan. Catat sebagai
explicit scope delta; jangan diam-diam menambah semua future features ke frozen
migration denominator atau mengklaim telah diterima oleh existing tests.

**Mandat kelanjutan terbaru:** owner meminta seluruh pekerjaan dituntaskan setelah
laporan R02. OWN-V1 sekarang merupakan explicit deliverable pada
[execution task §1.1](../archive/history/tasks/ARCHXY_FULL_RUST_EXECUTION_TASK.md#11-instruksi-lanjutan-setelah-laporan-r02-benchmark),
dengan OWN-T01–OWN-T10 sebagai proof matrix terpisah. Selesaikan dalam rangkaian
lokal R00–R11 setelah authority/CRM foundation tersedia; OWN-V2/later tidak otomatis
masuk scope. Ini penugasan implementasi, bukan klaim fiturnya sudah selesai.

**OWN-V1 — Bounded first owner release**, setelah authenticated core/CRM dan
module configuration boundary tersedia:

1. Owner inspect current setup dan supported options.
2. Percakapan dan dashboard menghasilkan plan/preview melalui kontrak yang sama.
3. Satu reusable CRM implementation dipakai dua model bisnis tanpa fork; supported
   terminology/pipeline label configuration berbeda, canonical IDs/outcomes terjaga.
4. Enable/disable satu optional installed module pada synthetic consumer profile;
   dependencies, entitlement, authority, cost classification dan data retention benar.
5. Human-confirmed/idempotent apply, status recovery, history dan safe compatible
   config restore. Tidak ada owner source edits atau terminal steps.

**OWN-V2 — Next:** domain-supported custom pipeline stages/rules, typed custom
fields, configured approval policy dan curated integration setup dengan versioned
contracts serta actual customer scenarios. Existing strict enum tidak otomatis
mendukung custom stages; implementation memerlukan explicit compatible data model
dan migration/history mapping bila diperluas.

**OWN-LATER:** controlled installation self-service untuk module belum terpasang,
broader vertical configuration dan more automated delivery. Catalogue coverage
bertumbuh dari accepted modules/actual needs. Jangan membuat 40 complete kits atau
fully autonomous arbitrary app generator sebagai prerequisite OWN-V1.

Untuk Archxy: selesaikan authority/integrity baseline dan current bounded Rust slice
yang sedang aktif. Tambahkan requirement ini ke decisions/inventory/change log
dengan status/placement eksplisit. Map overlapping inspect/config/plan/approval/
history primitives ke Rust roadmap; nyatakan owner-facing flows yang merupakan
new feature scope. Tidak perlu membuang implementasi atau mengganti arsitektur.
Product feature baru yang belum dibangun dilaporkan terpisah dari migration parity.

## 9. Acceptance proof OWN-V1

| ID      | Bukti wajib                                                                                                                                                   |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OWN-T01 | Dua business-model consumers memakai identical released CRM/core tanpa source fork; owner hanya mengubah supported config melalui UI/Archava                  |
| OWN-T02 | Request yang sama lewat chat dan dashboard menghasilkan equivalent validated plan dengan authoritative base/config/catalogue/pricing identity                 |
| OWN-T03 | Preview menampilkan actual changes/dependencies/readiness/cost classification; tidak ada hidden activation/payment/provider side effect                       |
| OWN-T04 | Owner tanpa management grant, public session, forged valid tenant/role/approval dan revoked-grant confirm/status/replay ditolak oleh backend                  |
| OWN-T05 | Incompatible/missing dependency, module unavailable atau unsupported workflow menghasilkan explicit blocked/gap result; tidak dijalankan atau dihitung PASS   |
| OWN-T06 | Duplicate/racing apply, two-tab stale config, refresh/lost response dan host restart menghasilkan satu authoritative config effect dan authorized recovery    |
| OWN-T07 | Optional module enable/disable mengubah actual UI/API/tool availability; direct API tetap menolak ketika disabled; dependent module disable ditolak           |
| OWN-T08 | Existing records/audit/receipts/permissions terjaga; historical label/meaning dapat ditelusuri; config restore tidak menulis ulang business/financial history |
| OWN-T09 | Stale catalogue/config/price snapshot atau changed target resources meminta replan/reconfirmation; model tidak mengarang price atau readiness                 |
| OWN-T10 | Real browser membuktikan inspect → plan → preview → confirm → applied → recovery/history/restore; tanpa editing code/config file atau terminal oleh owner     |

Required module/two-consumer/API/browser/PostgreSQL proofs mengikat artifact dan
configuration identity yang sama. Mock/planning output tidak membuktikan actual
runtime apply. No new readiness percentage, production SLA atau custom-fee amount
ditetapkan oleh dokumen ini.
