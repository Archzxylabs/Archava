# PRD M02a — penerimaan lokal customer identity, 6 November 2026

**Lulus untuk scope M02a:** pemilihan CRM contact secara eksplisit → sales order
→ fulfilled order → invoice, dengan UUID/revisi yang stabil dan nama historis.
Ini bagian DAT-01/DAT-02/FIN-03 dari [PRD aktif](PRD.md), bukan penerimaan seluruh
master data, accounting atau global ERP parity. [M01](PRD_M01_ACCEPTANCE_2026-11-06.md)
tetap catatan kandidat sebelumnya dan tidak ditulis ulang.

## Perilaku yang diterima

Sales orders menawarkan **Use an existing CRM customer**. Operator mencari nama,
membuka halaman berikutnya dan membedakan kontak memakai email/organisasi.
Pemilihan mengirim `contactId`, `expectedContactRevision` serta line sheet, tanpa
nama/owner/role hasil klaim browser. Server mengambil snapshot nama dari CRM
yang benar-benar berizin pada revisi yang direview.

Nama yang berubah setelah pemilihan membuat submit baru ditolak tanpa order.
**Refresh selected customer** mengambil record berizin terbaru agar operator
dapat mereview dan mencoba lagi. Enter pada kolom pencarian menjalankan pencarian;
order yang sedang disiapkan tidak ikut tersubmit. Pilihan tetap konsisten saat
hasil pencarian atau halaman berubah.

Order dan invoice menyimpan `customerContactId` serta `customerContactRevision`.
Perubahan nama CRM berikutnya tidak mengubah nama pada dokumen lama. Detail
order/invoice menyediakan link ke profil CRM, yang tetap memerlukan izin baca
profil saat dibuka. Invoice conversion menggunakan histori order yang berizin,
tanpa mensyaratkan akses baru terhadap profil CRM saat ini.

Original order replay mempertahankan hasil setelah rename dan memeriksa ulang
CRM read/scope. Pembacaan receipt order historis tetap mengikuti izin order.
Native command juga terdaftar sebagai `commerce.order.create_from_contact` dan
berhasil dieksekusi melalui mission setelah approval; run sebelum approval
ditolak dan pengulangan tidak menambah order.

## Bukti yang dijalankan

| Pemeriksaan | Hasil aktual |
| --- | --- |
| Full repository gate final | **1.164 tes Rust**, 98 suites, 0 gagal/diabaikan; structure, docs, format, Clippy dan WASM boundary lulus |
| Node helpers | 25 gate tests dan 3 product-audit helper tests lulus |
| Workflow gabungan | 28 tests lulus, termasuk 8 customer-identity integration cases baru |
| Upgrade/restore | 8 tests lulus, termasuk prior profile, existing stock, rollback dan writer fencing |
| Parser case baru | Linked command menolak customer/owner/role tambahan, revision tidak valid dan line sheet kosong |
| Chromium/native final | **81 pemeriksaan lulus**, termasuk 6 customer-identity checks serta 30 agent-runtime checks |
| Product journey probes | **75 terpenuhi, 0 gap** pada suite operasional; bukan penerimaan seluruh requirement PRD baru |
| Media | Owned LiveKit/Gemini-compatible transports lulus; provider produksi tidak diterima oleh pengujian ini |
| Browser dan cleanup | 0 JavaScript/WASM errors; fixture cleanup exit 0 |
| Serving build | Production server, disposable preview dan SSR/WASM dibangun ulang pada source yang sama |
| Native package | **62 artifact** dihitung ulang dan diverifikasi Ops; tidak dideploy |

Integration cases mencakup dua kontak bernama sama, rename/replay, stale/missing/
forged inputs, read-only CRM grant, assigned scope, revoke, cross-tenant refusal,
database repoint/false snapshot guards, 160 karakter Unicode, unlinked manual
orders, concurrent rename dan approved mission. Full gate menguji kembali
workflow M01 dan semua suite lama.

### Simulasi browser dan oracle SQL

1. Fixture mengisi **120 synthetic CRM contacts bernama sama** sebagai opening
   data. UI memakai search/cursor, memilih record di halaman kedua, dan menampilkan
   email serta organisasi milik UUID yang dipilih.
2. Dengan cart yang sudah terisi, Enter menjalankan pencarian, mempertahankan
   pilihan lintas halaman dan menghasilkan **nol order**.
3. Profil berubah dari revision 1 menjadi 2 saat form terbuka. Submit pertama
   menjawab **409** tanpa order; explicit refresh memperbarui review.
4. Order berhasil pada contact revision 2, untuk dua unit @11.000 IDR. Profil
   kemudian berubah lagi ke revision 3; original request replay memberi order
   yang sama dan tidak mengubah snapshot revision 2.
5. Fulfillment dan invoice dibuat melalui native UI. Independent SQL membuktikan
   satu order dan satu invoice dengan contact UUID yang sama, gross 22.000 IDR,
   historical names yang sama serta contact revision 2, sementara CRM current
   name sudah berbeda pada revision 3.
6. Reload mempertahankan invoice dan link ke CRM UUID yang benar.

Opening data SQL hanya mengenai database milik gate. Pemilihan, order, refresh,
fulfillment dan invoice memakai UI/HTTP nyata; tidak ada penggantian response
bisnis dengan hasil sintetis untuk meluluskan skenario baru.

## Migration dan kandidat

[Commerce 007](../../migrations/commerce-pg/007_customer_contact_identity.sql) dan
[Billing 006](../../migrations/billing-pg/006_customer_contact_identity.sql)
menambah nullable references, composite tenant FK, index, snapshot checks dan
immutable linkage. Data lama tetap null tanpa pencocokan nama atau merge.

Manifest native dan fixture kini menerapkan **Identity → CRM → Commerce →
dependent ledgers**, karena Commerce references memerlukan schema CRM terlebih
dahulu. Historical migration IDs, SQL bytes dan checksums tidak berubah. Rehearsal
upgrade membuktikan kompatibilitas prefix profile; serving startup tetap menolak
profile yang belum sesuai dengan embedded migrations.

Source SHA-256:
`0d4f6609d02dcde7bafea2276d3306ad66bd1d5a1a1cd13846f605a78daffe8d`.

Candidate: `dist/native-candidate-20261106-prd-m02a-01`.
Inventory: **18 domains, 143 canonical operations, 141 native forms, 2 provider-only
operations**; 50 SQL files, 43 mounted migrations dan 7 unmounted reserved specs.
Tidak ada modul bisnis baru, perubahan harga atau product version bump.

Evidence:

- `target/prd-m02-20261106/gate-full-03.log`
- `target/prd-m02-20261106/workspace-tests-02.log`
- `target/prd-m02-20261106/upgrade-tests-01.log`
- `target/prd-m02-20261106/product-helper-tests.log`
- `target/prd-m02-20261106/build-03.log`
- `target/prd-m02-20261106/browser-01.log`
- `target/prd-m02-20261106/package-01.log`
- `target/prd-m02-20261106/final-verification.json`
- `target/e2e/prd-m02a-20261106-01/result.json`
- `target/e2e/prd-m02a-20261106-01/operations-customer-identity.png`

Full gate memakai `--offline --wasm` tanpa `--lib`. Browser memakai installed
Chromium, explicit no-sandbox fallback, owned media fixture, product probes dan
agent-runtime probes. Rincian perintah ada pada [developer guide](../DEVELOPER_GUIDE.md).

Builds 01/02 dihentikan untuk menyelesaikan pemulihan stale selection dan perilaku
keyboard sebelum penerimaan. Gate 03 dan build 03 adalah bukti source final;
log intermediate dipertahankan dan tidak dipakai sebagai penerimaan kandidat final.

## Batas penerimaan

- Ini referensi **CRM contact** pada flow opt-in, bukan universal business-party,
  legal entity, customer-credit account, vendor roles atau automatic deduplication.
- Legacy free-text orders/invoices tetap tersedia dan unlinked; nama yang sama
  tidak otomatis diberi identitas dari CRM. Kebijakan wajib link belum ditambahkan.
- Picker CRM mendukung cursor pages; picker source invoice tetap mengikuti bounded
  Commerce read seperti pada M01. Tidak ada klaim seluruh histori muncul sekaligus.
- Rekonsiliasi merge/contact replacement, supplier identity lintas domain, related
  customer transaction timeline dan migrasi association data lama belum termasuk.
- GL/AP/AR penuh, bank reconciliation, close, localization dan B01–B07 menyeluruh
  belum diterima. Strategi ledger native/adapter tetap keputusan R0.
- Agent-runtime probes menggunakan owned planning fixtures. Tidak ada penerimaan
  baru atas real Google planning untuk command customer-linked baru atau provider
  media/payment produksi.
- Tidak ada competitive task benchmark, pengukuran penghematan 30%, production
  load/SLA atau deployment. Target Commerce 50 ms yang masih terbuka tidak ditutup
  oleh hasil fungsional ini.

Scope berikutnya tercatat pada [tracker eksekusi](PRD_EXECUTION_WORK.md).
