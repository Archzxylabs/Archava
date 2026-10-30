# Reaudit Archxy 1 dan Archxy 2 — 20 Oktober 2026

**Verdict: keduanya maju, keduanya belum menyelesaikan mandat penuh.** CRM
memiliki perbaikan yang terverifikasi ulang. ERP memiliki lebih banyak implementasi
domain, tetapi belum menjadi produk ERP Rust yang lengkap dan dapat dirakit.
Tes bawaan yang lulus belum menangkap seluruh batas bisnis dan pemulihan.

Audit ini membandingkan kode dengan
[mandat Archxy 1](../ARCHXY_1_FULL_IMPLEMENTATION_TASK.md) dan
[mandat Archxy 2](../ARCHXY_2_FULL_IMPLEMENTATION_TASK.md), bukan hanya dengan
ringkasan implementator. Tidak ada perubahan kode produk, commit, push, transaksi
provider, atau penggunaan database owner oleh auditor.

## Kandidat dan metode

| Lane                 | Lokasi                         | HEAD yang diperiksa                                                            |
| -------------------- | ------------------------------ | ------------------------------------------------------------------------------ |
| Archxy 1 / integrasi | `Archava_Platform`             | `412f763e17f3a68a457fd76c7b929184c50c916a`                                     |
| Archxy 2 / ERP       | `Archava_Platform_archxy2_erp` | `21234bb012db018d7632e870575682c37aa38695` + working copy yang belum di-commit |

Working tree kedua lane berisi pekerjaan yang harus dipertahankan. HEAD copy
Archxy 2 bukan ukuran kemajuan: implementasi barunya berada di working copy dan
packet handoff. File subscriptions domain, adapter, host, dan tes pada kedua
tree identik saat audit, sehingga reproduksi pada main juga berlaku terhadap
kode subscriptions yang dikirim Archxy 2.

Auditor mengambil manifest hash sebelum pemeriksaan, membaca kontrak dan
penghubung proof, menjalankan tes release dengan PostgreSQL sintetis, membuat
probe negatif di direktori cache milik audit, serta menguji UI aktual melalui
Chromium/CDP. Harness server menyalin example server yang sekarang dan hanya
mengganti lokasi pidfile agar tidak mengambil alih server implementator.
Dependency resolution harness tersimpan dalam lockfile tersendiri; probe
mengimpor crate produk aktual. Tes bawaan memakai lockfile workspace dengan
`--locked`. Tidak ada live Gemini/email/payment call.

## Temuan yang harus ditutup

Prioritas di bawah adalah prioritas perbaikan/kelengkapan kandidat, bukan skor
CVSS. Temuan kelengkapan dibedakan dari bug yang berhasil direproduksi.

### F01 — P1 / Archxy 2: pembayaran nol mengaktifkan paket berbayar

**Bug direproduksi melalui router dan PostgreSQL nyata.** Buat subscription
dengan `unitPrice: 100000`, lalu kirim `record_payment` dengan `amountPaid: 0`.
Server mengembalikan receipt `active`; `consume_usage` berikutnya berhasil dan
merekam usage event.

[Parser](../rust/crates/domain/src/erp/subscriptions.rs:675) memakai helper uang
yang mengizinkan nol. [Adapter pembayaran](../rust/crates/adapters/src/erp/subscriptions.rs:420)
hanya membaca revision sebelum menetapkan `status='active'`: tidak memeriksa
jumlah terhadap kewajiban atau fakta settlement yang berwenang. Referensi
pembayaran buatan caller cukup untuk membuka entitlement.

Pembayaran manual tidak otomatis salah. Masalahnya adalah tidak ada policy
eksplisit yang membedakan settlement yang sah, tagihan yang belum dipenuhi, dan
arrangement gratis/non-provider yang memang diotorisasi. Mandat A2-SUBSCRIPTIONS
secara eksplisit meminta batas itu. Tambahkan policy tersebut dan tes nol,
kurang bayar, settlement asing, serta approval khusus yang didukung; jangan
menganggap string referensi sebagai bukti pembayaran.

### F02 — P1 / Archxy 2: `record_payment` melewati status terminal

**Bug direproduksi melalui router dan PostgreSQL nyata.** Urutan
`create_subscription → change_status(canceled) → record_payment` mengembalikan
subscription `active`, revision 3. Tidak perlu race condition.

[Cabang pembayaran](../rust/crates/adapters/src/erp/subscriptions.rs:430)
mengunci row tetapi hanya membaca revision. Aturan terminal milik
`activation_refusal` dipakai di `change_status`, bukan di semua jalur menuju
`active`. Tes bawaan menolak resume lewat `change_status`, sehingga jalur ini
tidak tertangkap. Terapkan satu aturan transisi untuk semua jalur dan uji
payment/cancel/expiry/replay di bawah lock yang sama. Reproduksi membuktikan
kebangkitan status subscription; tidak mengklaim entitlement revoked ikut hidup.

### F03 — P1 / Archxy 2: recovery receipt tidak memeriksa izin baca domain

**Bug direproduksi melalui router dan PostgreSQL nyata.** Actor `sxn` hanya
memiliki `audit.read`. List subscription menjawab 403, tetapi status untuk
command milik actor lain pada tenant yang sama menjawab 200 `recorded`, termasuk
ID subscription pada receipt.

[Status handler](../rust/crates/host/src/erp/subscriptions.rs:524) menyelesaikan
membership lalu membuang `access`; [pembaca receipt](../rust/crates/adapters/src/erp/subscriptions.rs:861)
hanya memfilter tenant dan request ID. Tenant RLS tetap bekerja, tetapi tidak
menggantikan granular permission dan resource scope. Ini bukan akses anonim atau
kebocoran lintas tenant; ini perbedaan otorisasi antara read normal dan recovery.

Periksa current read grant, scope, dan resource terkait sebelum mengembalikan
receipt. Jika recovery milik initiator memerlukan izin khusus, definisikan
explicit recovery permission dan bind initiator; membership saja tidak cukup.
Periksa pola sejenis pada status/replay ERP lainnya, bukan hanya subscriptions.

### F04 — P1 / Archxy 1: konfirmasi owner tidak mengikat biaya/readiness

**Bug direproduksi pada engine Rust aktual.** Plan dengan modul `Included/Ready`
dan plan dengan modul yang sama `Recurring/NeedsSetup` menghasilkan hash yang
sama, `a887488d4e449b89`. Confirmation lama tetap diterima oleh `Store::apply`
untuk plan dengan klasifikasi yang baru.

[Canonical plan identity](../rust/crates/own/src/lib.rs:488) mengikat revision,
enabled set, labels, changes, dan blocked result; tidak mengikat effects,
catalogue identity, atau pricing identity. Host menampilkan version strings,
tetapi [apply](../rust/crates/host/src/own.rs:699) tidak menerima/memvalidasi
snapshot version sebagai bagian confirmation.

Katalog host saat ini statis dan kedua modul included. Probe ini membuktikan
perilaku engine dan celah saat katalog/release berubah; bukan klaim ada tagihan
berulang yang sekarang sudah dibebankan. OWN-T09 belum terpenuhi. Bind seluruh
fakta preview yang berpengaruh beserta authoritative version/digest dan tolak
snapshot lama, termasuk setelah deploy ketika config revision belum berubah.

### F05 — P2 / Archxy 1: owner apply kehilangan identitas saat hasil tidak diketahui

**Bug direproduksi di UI Leptos dengan Chromium.** Browser mengirim apply;
server commit revision 1; probe membuang response setelah commit. UI menampilkan
`opaque value`. Request ID tidak tersimpan dalam app storage. Klik confirm lagi
menghasilkan UUID baru dan mendapat `stale_base`; reload menampilkan revision
terbaru, tanpa UI pending/recovery untuk permintaan asal.

[Confirm handler](../rust/crates/slice/src/owner.rs:188) membuat UUID setiap klik
dan hanya menyimpan hasil dalam signal. [Error handler](../rust/crates/slice/src/owner.rs:255)
tidak menggunakan pending marker yang sudah diperbaiki untuk CRM. Dalam
reproduksi ini revision guard mencegah apply kedua; masalah yang terbukti adalah
hilangnya pemulihan outcome yang sama, bukan bukti efek ganda.

Persist tenant/account-bound operation identity sebelum kirim, pertahankan saat
network/500/response malformed, blok fresh intent sampai status berwenang
menyelesaikannya, dan sediakan recovery untuk apply maupun restore.
[Proof T06](../tools/own/verify.mjs:436) sekarang menggabungkan duplicate API test
dengan reload setelah respons sukses. Gabungan itu belum menguji respons hilang
yang dilanjutkan reload dalam aplikasi yang sama.

### F06 — P2 / Archxy 1: restore konfigurasi tidak menghapus label yang lebih baru

**Bug direproduksi pada engine Rust aktual.** Revision 0 tidak memiliki label
kustom. Setelah `won → Closed deal`, restore ke revision 0 masih menghasilkan
`won → Closed deal` pada config baru.

[Planner](../rust/crates/own/src/lib.rs:448) melakukan merge label ke current
terminology. Target restore yang tidak mempunyai suatu key tidak pernah
menghapus key tersebut. [Host restore](../rust/crates/host/src/own.rs:720)
meneruskan target terminology ke planner yang sama.

Pisahkan patch terminology dari penggantian snapshot untuk restore; revalidasi
target, buat revision baru, pertahankan audit/history, dan uji penambahan serta
penghapusan label. Reproduksi target revision 0 memakai engine publik; bug merge
yang sama memengaruhi restore host ke revision tersimpan yang belum punya label.

### F07 — P1 / Archxy 2 + integrasi Archxy 1: scope ERP masih jauh dari selesai

**Gap kelengkapan yang dikonfirmasi dari source dan diakui worklist ERP.**

| Ukuran source, bukan persentase produk               | Copy Archxy 2 | Main  |
| ---------------------------------------------------- | ------------- | ----- |
| Domain dengan file Rust domain + adapter + HTTP host | 12/18         | 11/18 |
| Domain dengan panel ERP Leptos                       | 0/18          | 0/18  |

12 domain tersebut: organizations, proposals, projects, scheduling,
accommodation, assets, purchasing, foodservice, billing, subscriptions,
attendance, support. Support ada di copy/packet 010, belum diintegrasikan ke
main. Commerce mempunyai kernel parsial, bukan surface commerce lengkap.
Capacity persistence/allocations, fulfillment, laundry, field-services, dan
formal-education belum memiliki tiga lapisan implementasi tersebut.

[Worklist copy ERP](../../Archava_Platform_archxy2_erp/docs/rust/archxy2/ERP_SCOPE.md:51)
masih mencatat tool dispatch, dashboard actions, Leptos panels, CLI/worker seam,
Rust consumer proof, config/disable adapters, ERP data-ops, dan pending recovery
sebagai belum dimulai. Read projections baru ada pada tiga keluarga di copy:
subscriptions, attendance, support; tidak semua area masing-masing lengkap.
Klaim `ERP locally verified` harus menyebut bounded kernels yang diuji, bukan
mandat ERP secara keseluruhan.

Dua consumer di [producer ERP](../tools/rust/erp/module-consumers.ts:9) masih
menginstal compiled npm tarballs `@archava/*`. Itu bukti reuse TypeScript yang
berguna, tetapi belum membuktikan dua consumer Rust ERP menjalankan efek domain
persisten dari artifact dan SQL yang sama.

### F08 — P2 / Archxy 1: proof CRM dua consumer masih bergantung pada seluruh ERP

**Gap modularitas yang dikonfirmasi dari packaging dan fixture source.** Proof
CRM sudah meningkat: template kini menjalankan CRM HTTP terhadap PostgreSQL,
bukan hanya policy. Namun [artifact producer](../tools/rust/k05-k04-consumers.mjs:338)
membundel domain/adapters/host berisi seluruh ERP tanpa pemisahan fitur, dan
[daftar migrasi](../tools/rust/k05-k04-consumers.mjs:374) mewajibkan 15 direktori
SQL, termasuk purchasing, foodservice, organizations, proposals, projects,
scheduling, accommodation, assets, billing, subscriptions, attendance.

Fixture `provision()` memuat seluruh ledger tersebut untuk proof CRM. Hasil ini
membuktikan dua konsumen dari bundle besar yang identik; belum memenuhi syarat
minimal dependency closure atau membuktikan CRM bisa dirakit tanpa ERP yang
tidak dipilih. Gunakan batas modul/feature dan fixture terpilih yang nyata;
tidak perlu membuat microservice. Jalankan cold install CRM tanpa SQL/artifact
modul ERP yang tidak dibutuhkan dan uji penolakan dependency yang memang wajib.

### F09 — P1 / Archxy 1: SDK/widget masih memakai implementasi first-party TypeScript

**Gap migrasi terhadap mandat aktif.** [Producer R09](../tools/rust/sdk-widget.mjs:5)
secara eksplisit mengatakan SDK hosting dan widget shell tetap TypeScript.
[SDK public surface](../packages/sdk/src/index.ts:1) masih mengekspor host,
consent, page translation, scout, dan session TypeScript.
[Chat view](../packages/chat/src/view.ts:1) serta DOM shell juga TypeScript,
bukan hanya binding vendor atau generated facade ke Rust/WASM.

Menjaga kompatibilitas host JavaScript memang diwajibkan. Yang belum dibuktikan
adalah Rust memegang view/interaction first-party di belakang facade tipis itu,
sesuai A1-INTERFACE-03. Tes library/widget TypeScript dapat tetap menjadi
regression suite, tetapi tidak menerima migrasi Rust pada surface ini. Belum ada
dasar menyebut keseluruhan Archava all-in-Rust.

### F10 — P1 / Archxy 1: evidence kandidat belum konsisten dan belum complete

**Gap release acceptance yang direproduksi read-only.**
`node tools/rust/coverage.mjs` menghasilkan `complete: false` dengan **177
diagnostics**, atas denominator 93 required items. Angka ini adalah pesan
validator, bukan 177 bug independen. Di antaranya proof HEAD lama, source digest
berubah/tidak meliputi source manifest terbaru, dan required checks fail atau
belum dijalankan. Fast-gate proof merekam `cargo test --workspace` exit 101 dari
invocation terdahulu; audit ini tidak menyimpulkan penyebab kegagalan lama.

Mapping owner sekarang menunjuk tes Rust dan UI yang lebih relevan, tetapi
F04–F06 membuktikan nama witness/grade hijau belum cukup untuk OWN-T06/T08/T09.
Rerun producer setelah perbaikan, pada kandidat yang final dan identitas yang
konsisten; jangan memperbarui hash/HEAD evidence lama dengan edit manual.

Dokumen status juga belum sinkron: header execution state masih menyebut 53
proof dan packet sampai 007, sementara manifest memiliki 56 proof dan handoff
sudah sampai 010. ERP verdict masih mengatakan packet 004–010 belum di-main,
padahal main sudah mengintegrasikan 004–009. Gunakan disposition packet dan
candidate snapshot sebagai fakta, kemudian perbarui ringkasan dari fakta itu.

## Perbaikan dan verifikasi yang benar-benar terlihat

**120 tes bawaan lolos pada run audit ini**, tidak ada yang ignored:

| Suite                      | Lulus |
| -------------------------- | ----- |
| CRM host                   | 36    |
| Session/logout host        | 6     |
| Owner configuration host   | 11    |
| CRM assistant host         | 14    |
| Owner deterministic engine | 13    |
| Billing host               | 13    |
| Subscriptions host         | 13    |
| Attendance host            | 14    |

Chromium independen juga mengonfirmasi:

- Login dan contact read memakai host Rust aktual.
- Pagination bergerak dari 20 ke 9 baris dengan hasil terpisah.
- Search `Ada&limit=1` dibaca literal dan menghasilkan nol kontak.
- Follow-up commit satu kali saat respons dibuang; pending key disimpan aplikasi,
  bertahan setelah reload, dan recovery menampilkan receipt dengan key asal.
- Sign out melalui UI membuat authenticated contact read selanjutnya menjawab 401.

Untuk audit 19 Oktober: **C02 dan C06 dapat ditutup untuk skenario yang diuji;
C05 CRM diperbaiki untuk kehilangan respons + reload yang diuji.** C01 mengalami
kemajuan besar dengan tujuh jenis write dan UI lebih lengkap; audit ini bukan
penerimaan seluruh variasi CRM/approval/interface. C03 dan C04 belum bisa ditutup
sebagai pemenuhan mandat penuh; C07 tetap terbuka. Jangan memindahkan keberhasilan
recovery CRM menjadi klaim recovery konfigurasi owner.

Enam counterexample audit adalah hasil negatif yang berhasil direproduksi,
bukan enam acceptance test hijau tambahan. Audit tidak menjalankan ulang seluruh
release/security/benchmark/upgrade suite atau live provider; kelengkapan yang
sudah gagal tidak dinyatakan accepted. Support di working copy dibaca dan
dicocokkan packet-nya, belum diuji runtime ulang dalam audit ini.

## Pembagian pekerjaan sesudah audit

**Archxy 2:** tutup F01–F03 lebih dulu dengan policy dan negative HTTP/PG tests,
termasuk pemeriksaan status/replay sejenis di domain lain. Lanjutkan F07 memakai
vertical slice lengkap: commands + reads + UI + tools + config + recovery +
consumer Rust. Kirim packet koreksi domain dan packet 010 untuk integrasi;
pertahankan ownership shared files pada Archxy 1.

**Archxy 1:** tutup F04–F06 dan benahi proof owner agar menguji kasus negatif
tersebut. Selesaikan F08/F09, integrasikan packet ERP, lalu tutup F10 melalui
producer dan final gate pada kandidat yang sama. C03/C04 tidak boleh dinyatakan
closed hanya karena suite lokal saat ini hijau. Pekerjaan interface/ops harus
memiliki proof terkini dan terintegrasi, bukan bergantung pada label implementasi.

**Keputusan review:** lanjutkan implementasi, jangan rewrite ulang. Belum ada
acceptance final untuk kedua lane atau Full Rust. Rasio 12/18 hanya menghitung
kehadiran tiga file/lapisan per domain; tidak boleh dijadikan persentase
penyelesaian ERP, kelengkapan kit, atau progres seluruh Archava.

## Evidence audit

Ringkasan terstruktur:
[2026-10-20-archxy-both-reaudit.json](audit/2026-10-20-archxy-both-reaudit.json).
Raw artifacts, snapshot hashes, commands, test logs, browser probe, standalone
negative probes, dan lockfile harness:
`/home/haikaru/.cache/archava-codex-audits/reaudit-20261020-smdy3a0h/`.

Semua browser/server/cluster yang dibuat audit dihentikan; layanan yang sudah
ada tidak diambil alih. Tidak ada perubahan source yang sudah di-snapshot pada
kedua lane selama verifikasi. Hash drift dan cleanup juga dicatat di JSON.
