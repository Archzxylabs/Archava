# Penerimaan lokal supplier goods documents — 7 November 2026

**M03a diterima untuk dokumen operasional unposted di bawah.** Pekerjaan meneruskan company trade tanpa berhenti pada increment, atas instruksi owner. Tidak ada deploy atau keputusan sumber pembukuan resmi yang dibuat atas nama owner.

Source: `7ad7dd30159ea45cf2d4f30b4a30180dba19ee654b5f3a5046d71a9ec590d871`, 476 input. Kandidat: `dist/native-candidate-20261107-prd-supplier-documents-01`, 73 artifact native. Inventori kontrak saat ini 18 business domains, 165 operasi kanonis, 163 form native dan dua operasi billing khusus provider. Angka tersebut tidak mengukur kedalaman ERP atau kesetaraan pasar.

## Perilaku yang diterima

- Supplier goods document menunjuk satu PO company yang sudah dikonfirmasi. Operator menyuplai vendor reference/date/currency, declared total dan claimed quantity/unit price per line. Supplier, PO, owner dan captured company identity tetap historis dan immutable.
- Empat operasi Purchasing: capture, revise, exact match dan void. Selisih harga/total/accepted capacity tersimpan sebagai held dengan alasan. Barang ditolak tidak menciptakan kapasitas. Partial claims diperbolehkan; policy exact-goods tidak menebak tolerance, pajak, diskon atau freight.
- Match mengalokasikan accepted goods ke receipt request/line yang immutable. Lock PO serta guard PostgreSQL mencegah dua tagihan memakai kuantitas yang sama. Held tidak menyisakan alokasi parsial. Satu check dibatasi 500 allocation rows; kebutuhan lebih besar ditahan dengan alasan eksplisit.
- Void melepas alokasi aktif dan mempertahankan history. Koreksi matched claim memerlukan void lalu explicit revised version; vendor reference/source tetap sama. Duplikasi reference per company/supplier master ditolak juga setelah void. Receipt command lama tidak berubah saat barang datang kemudian, claim dikoreksi atau dibatalkan.
- Current documents.read/write, source reads, company visibility dan PO ownership berlaku pada mutation/replay/status. Read collection memerlukan document/source reads. Pencabutan hak menghapus detail native. Izin baru tidak ditambahkan otomatis ke membership lama. Historical company/supplier inactivity tidak menghalangi review komitmen lama yang masih dapat diakses.
- Native form menyediakan actual claim entry, held findings, correction, match, void dan recent history. Registered action search mempertahankan company, menolak respons stale dan membersihkan hasil refused. Ava memakai contracts yang sama dan tetap meminta host approval sebelum perubahan.

[Kontrak lengkap](../modules/SUPPLIER_DOCUMENTS.md) menjelaskan exact policy, batas input dan authority. Dokumen ini **UNPOSTED**: tidak membuat AP/GL, jurnal, pajak, payment, movement stok atau perubahan FIFO/acquisition cost.

## Verifikasi berlapis

| Lapisan | Bukti |
| --- | --- |
| Domain, database dan authority | Gregorian/strict claim grammar; tujuh router/PostgreSQL cases untuk exact/held/correction/rejected goods, duplicate/capacity, concurrent claims, void/replay/status, historical company, revocation/scope, raw DB forgery dan upgrade profile sebelumnya dua kali |
| Ava dan regresi penuh | Owned Gemini-shaped two-step capture → match dengan verified receipt binding dan approval replay satu efek; revoked approval403 tanpa void. Full gate 1238 Rust tests/98 suites, 0 failed/ignored; 25 Node helpers; fmt, all-target Clippy, WASM, docs dan struktur |
| Native browser dan distribusi | 132 Chromium checks (sembilan supplier-document), 75 product probes/0 gap, 30 owned agent-runtime checks, owned media; 0 JS errors; cleanup exit0; screenshot benar-benar dilihat; 73 native artifacts diverifikasi dengan SHA/provenance |

Regresi penuh di atas mengompilasi source `5da5a8becef06c23fc320940e2b644f8bfb74b5419872d71f2dd374e511cf31b` sebelum satu kondisi visibility UI dikoreksi: form koreksi tidak boleh tetap terbuka saat matched. Backend/domain/SQL/tests tidak berubah. Gate final `verify-03-ui.log` menguji 585 library tests, Clippy/WASM dan helper pada source final, kemudian native build/browser memeriksa delta UI itu. Full gate tidak diklaim mengompilasi kondisi visibility yang lebih baru.

Journey native mencatat claim total21.001, menyimpan held, mengoreksi menjadi21.000 dan match tiga accepted goods; klaim kedua ditahan karena kuantitas sudah dialokasikan. Void melepas tiga, explicit reopen/recheck mencapai version7. SQL independen membuktikan active3/released3, tujuh versions, held history version2; stock6, acquisition value36.000 serta settled sales invoice tidak berubah. Executable produksi direstart pada database/session fixture yang sama: document matched version7 dan original capture receipt version1 tetap tersedia. Owned transports berjalan pada fixture preview; konfigurasi produksi HTTPS tidak dilonggarkan.

Upgrade memakai database profile nyata sebelum Purchasing006, membuat PO/physical receipt/stok lewat adapter lama, lalu menerapkan migration dua kali. Row/receipt/stock JSON lama identik dan tidak ada supplier document yang dibuat dari tebakan. Tiga tabel baru memakai FORCE RLS, immutable versions, constrained allocation releases dan runtime grants terbatas.

## Bukti dan batas

- `target/prd-supplier-documents-20261107/verify-02.log`, `target/prd-supplier-documents-20261107/verify-03-ui.log`, `target/prd-supplier-documents-20261107/focused-03.log`, `target/prd-supplier-documents-20261107/agent-03.log`.
- `target/prd-supplier-documents-20261107/build-02.log`, `target/prd-supplier-documents-20261107/package-01.log`, `target/e2e/prd-supplier-documents-20261107-03/result.json`.
- `target/prd-supplier-documents-20261107/visual-review.json`, `target/prd-supplier-documents-20261107/product-audit.json`, `target/prd-supplier-documents-20261107/rust-evidence.json`, `target/prd-supplier-documents-20261107/final-verification.json`.
- [Router/database witnesses](../../crates/host/tests/workspace_flows/supplier_documents.rs), [Ava witnesses](../../crates/host/tests/agent_runtime.rs), [native/SQL journey](../../tools/e2e/supplier-documents.mjs).

Seluruh SQL historis, source/status working tree di luar scope, delapan penerimaan sebelumnya dan archive dipertahankan. Total migrasi61 SQL/54 mounted/7 unmounted dalam24 namespaces/30 ledger directories. Catalogue actions lama dipertahankan; hanya empat operasi serta dua explicit Purchasing grants ditambahkan.

Penerimaan ini belum menerima full AP/AR/GL/procure-to-pay, official company/book/account mapping, bank/close, legal verification, jurisdiction/currency/tax, intercompany/consolidation atau strategi accounting R0. Real-model reasoning umum, live-provider produksi, load/SLA, pilot, public production dan market parity tetap memerlukan bukti tersendiri. Website/deployment tetap mengikuti jadwal owner.
