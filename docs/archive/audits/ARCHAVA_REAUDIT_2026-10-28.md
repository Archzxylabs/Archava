# Archava — Reaudit setelah remediation

**Tanggal:** 28 Oktober 2026, WIB. **Mode:** audit saja. **Verdict:** **NOT READY untuk peluncuran publik dengan data/transaksi nyata.**

Ada perbaikan nyata sejak [audit sebelumnya](ARCHAVA_FULL_AUDIT_2026-10-28.md): pemeriksaan pesan checkout, enkripsi archive backup, penolakan URL RPA lama, pemeriksaan prerequisite mission, deteksi overflow trial balance, dependency template, serta beberapa jalur build. Namun, retest menemukan kegagalan integrasi tenant/RLS, checkout yang membuat status pembayaran sendiri, persistence yang tetap boleh gagal diam-diam, dan fallback mock di worker produksi.

**Status 24 kelompok temuan lama: 6 FIXED pada masalah asal, 17 PARTIAL, 1 OPEN.** Angka ini bukan persentase kesiapan produk. Penutupan masalah asal tidak berarti seluruh komponen sudah siap produksi; residual dan regresi dijelaskan terpisah di bawah. Empat kandidat bottleneck lama tetap memerlukan pekerjaan/pengukuran.

Audit ini menghasilkan satu file baru ini. Kode aplikasi, migrasi, dokumen produk lain, dan laporan lama tidak diubah. Tidak ada deployment, pembayaran, pengiriman pesan, maupun panggilan provider AI/voice/avatar. Browser dan PostgreSQL yang dimulai untuk probe lokal telah ditutup; proses milik pengguna/agent lain dipertahankan.

## 1. Identitas snapshot dan cakupan

| Item | Bukti |
| --- | --- |
| Repository | `/home/haikaru/Archverse/Lab/Archava` |
| HEAD | `412f763e17f3a68a457fd76c7b929184c50c916a` |
| Working tree awal | 1.093 entri status Git: 138 modified, 869 deleted, 86 untracked; perubahan agent lain belum menjadi commit |
| Manifest audit | 567 file source/config; SHA-256 agregat `f283c96693bfe2ab595b147d07d774b5d7337b20b82db2fa74fb7020f05f044f` |
| Snapshot audit lama | Agregat `b37f71cddb5f4d312f0f4633c3ccc81eac25c2f578a29797c70b76d56b597f42` |
| Konsistensi saat pengujian | Seluruh 567 hash tetap sama pada pemeriksaan akhir sebelum penulisan laporan; status Git juga sama |

Manifest mencakup source/config dalam `crates`, `migrations`, `static`, `tools`, `deploy`, `.github`, `Archava Template`, dan `config`, ditambah manifest/lock/toolchain Rust utama. Ekstensi: Rust, SQL, JS/TS, Python, JSON, TOML, YAML, dan shell. Direktori build, dependency terpasang, venv, serta cache dikecualikan. Digest dihitung dari urutan path relatif yang diurutkan, NUL, lalu digest SHA-256 mentah masing-masing file.

Pendekatan: bandingkan seluruh temuan lama dengan implementasi sekarang; tinjau perubahan dan jalur pemanggil penting; jalankan unit/integration Rust, checks template, dependency audit, probe domain, HTTP dengan database disposable, dan browser checkout dari SDK aktual. Inventaris 567 file tidak berarti setiap baris memperoleh review manual baru. Reaudit ini lebih luas pada pengujian integration daripada audit awal yang menjalankan Rust library tests saja.

**Batas bukti:** tidak menguji deployment publik, provider sandbox/live, pembayaran sungguhan, pengiriman pesan, semua kombinasi role/provider/browser, recovery backup produksi, load produksi, atau setiap kemungkinan celah. Hasil dependency audit berlaku pada lockfile dan database advisory yang diakses saat audit. Tidak ada jaminan bebas kerentanan.

## 2. Status seluruh temuan lama

**FIXED:** reproduksi/penyebab masalah asal tertangani dalam snapshot ini. **PARTIAL:** ada perubahan relevan, tetapi kontrak keseluruhan belum terpenuhi. **OPEN:** masalah asal tetap ada. Rxx menunjuk temuan terkini pada bagian 3.

| ID lama | Status | Hasil retest dan sisa pekerjaan |
| --- | --- | --- |
| S01 — Orbz body authority/tenant | PARTIAL | Authority/owner tidak lagi diambil langsung dari body; identitas episode memakai principal sesi. Lookup membership tidak mengatur konteks tenant dan tetap meloloskan error. Department masih dari request; capability Enterprise juga dipetakan menjadi owner. Lihat R01. |
| S02 — Cockpit authz | PARTIAL | Ada lookup membership, tetapi member sah ditolak pada runtime RLS normal; query error meloloskan akses tenant lain. Complete/abort juga belum memeriksa capability/assignment worker. R01. |
| S03 — pesan checkout dari origin/window lain | **FIXED, masalah asal** | Browser membuktikan pesan dari parent window tidak memicu callback. SDK memeriksa origin dan iframe source. Order ID masih boleh absen; checkout baru sendiri membuat receipt tanpa payment. R02. |
| S04 — webhook menerima konfigurasi/token tidak valid | PARTIAL | Midtrans menolak server key kosong/missing; Privy menolak header hilang. Privy masih memakai secret fallback yang diketahui dari source. R06. |
| S05 — bypass parser URL RPA | **FIXED, vector lama** | URL userinfo/host confusion lama sekarang menghasilkan `false`; HTTPS, `@`, backslash, dan host diperketat. Executor browser, redirect, serta DNS policy belum diuji end-to-end. |
| S06 — archive backup native plaintext | **FIXED, masalah asal** | Capture sekarang wajib memiliki key dan menulis ciphertext AES-256-GCM; tests crypto dan integration backup/restore lulus. Plaintext staging restore menimbulkan residual baru R07. |
| S07 — restore melewati writer lease | PARTIAL | CLI memanggil lease probe, tetapi kegagalan koneksi/runtime melewati probe dan meneruskan restore. Public restore function juga tidak memaksanya. R06. |
| S08 — RLS/readiness migrasi baru | PARTIAL | FORCE RLS dan dukungan GUC utama ditambahkan. Caller tidak memasang konteks tenant; readiness masih menerima schema Cockpit yang tidak ada. R01/R03. |
| C01 — sukses tanpa efek bisnis | PARTIAL | Feed/MCP lebih jelas melabeli simulasi/pending. Migration commit, widget, dan checkout tetap menampilkan hasil selesai tanpa efek bisnis terkait. R02/R10. |
| C02 — eval selalu lulus | PARTIAL | Ada target/observed behavior, empty suite tidak 100%, dan defective target bisa gagal. Target default masih oracle keyword lokal, bukan jalur agent/HTTP sebenarnya; label execution dan leakage tidak memadai untuk assurance produksi. R10. |
| C03 — memory/mission hanya RAM | PARTIAL | Fungsi SQL/hydration ditambahkan. Error tulis dibuang, konteks RLS tidak dipasang, schema tidak diwajibkan saat startup, cache dan restart belum konsisten. R03. |
| C04 — completion melewati dependency/state | **FIXED, masalah asal** | Engine menolak completion bila mission bukan InFlight atau dependency belum Passed; unit tests lulus. Duplicate completion/rewrite hasil tetap residual R08. |
| C05 — landed cost/COGS negatif | PARTIAL | Kasus pembagian lama menjadi `[1,1,0,0]`; receipt dengan cost negatif tidak membuat layer. Quantity negatif pada landed allocation masih menghasilkan `[-10,20]`. R08. |
| C06 — overflow trial balance tetap balanced | **FIXED, masalah asal** | Reproduksi lama menghasilkan `is_balanced=false`, total sentinel `-1`. P&L/balance sheet masih menggunakan aritmetika unchecked dan menerima trial balance gagal. R08. |
| C07 — frame telematics terpotong diterima | PARTIAL | Frame lama ditolak karena header IO hilang. Frame 40 byte tanpa trailer count/CRC masih menghasilkan satu record. R08. |
| D01 — CI/build/deploy struktur lama | PARTIAL | Path Docker, workflow root Cargo, Dependabot, dan sebagian script diperbaiki. Runner prerequisites, integration gate, release WASM/site bundle, dan script legacy masih belum tuntas. R05. |
| D02 — dokumentasi tidak sesuai | **OPEN** | README/guide/security docs masih memuat jaminan berlebihan atau path/kontrak lama. Daftar perubahan dokumen pada bagian 6. |
| D03 — advisory OpenTelemetry template | **FIXED, lockfile saat audit** | `npm audit`: 0 vulnerability. Rust audit juga 0 vulnerability; ada warning unmaintained `paste`, bukan vulnerability yang terbukti exploitable. |
| D04 — integration SDK Python tidak masuk gate | PARTIAL + regresi | Discovery kini menjalankan 11 test, tetapi dapat memakai SDK palsu dari production fallback. CI belum memasang dependencies SDK/venv untuk membuktikan kontrak sebenarnya. R04. |
| U01 — checkout/embed tidak bekerja | PARTIAL + regresi | Route checkout dan CSP embed ditambahkan; SDK dapat membuka modal. Modal membuat status settled sendiri. Default host tetap origin merchant dan mount ERP masih terikat kebijakan framing/admin. R02. |
| U02 — tool topic tanpa UI consumer | PARTIAL | Ada penerima topic LiveKit yang dispatch CustomEvent. Tidak ditemukan consumer untuk `archava:preview`, `archava:privy`, `archava:migration`; UI hasil belum terbentuk. R09. |
| U03 — fixture onboarding/CSS | PARTIAL | Wiring CSS/asset diperbaiki. Form tetap menampilkan petunjuk fixture, tenant manual/default, serta menambah `@archava.test` pada username tanpa `@`. Onboarding produksi belum terbukti. R09. |
| U04 — recovery marker bertabrakan | PARTIAL | Key sekarang tenant+domain, caller utama memakai domain, dan principal diverifikasi. Dua operasi/tab pada tenant+domain sama tetap satu slot; belum ada koordinasi tab/per-operation journal. R09. |
| U05 — expiry/timeout/klik ganda | PARTIAL + regresi | Ada interceptor 401, tetapi redirect menuju `GET /login` yang menghasilkan 405; halaman login sebenarnya `/app`. Timeout fetch dan guard submit ganda belum lengkap. R09. |

## 3. Temuan terkini yang perlu ditangani

Prioritas P0 harus selesai sebelum akses publik. P1 harus selesai sebelum alur terkait menangani data/transaksi nyata. Severity menilai dampak pada fitur terkait; audit ini tidak memberi skor CVSS numerik tanpa model deployment lengkap.

### R01 — High / P0 — Otorisasi tenant gagal pada dua arah

**Lokasi:** `crates/host/src/orbz.rs:212`, `:297`, `:334`; `crates/host/src/cockpit.rs:351`, `:512`, `:599`; `crates/host/src/authority.rs:148`; `migrations/commerce-pg/001_catalog_orders_stock.sql:187`.

Handler mengambil koneksi pool langsung untuk `resolve_membership`, tanpa transaction tenant yang memasang `archava.tenant_id`. Tabel membership memakai FORCE RLS. Akibatnya, membership valid tidak terlihat. Pada sisi lain, `Err(_) => {}` dan `if let Ok(conn)` membolehkan eksekusi berlanjut jika query/acquire gagal. Query Orbz menggunakan fallback authority L1 pada error, padahal keberadaan membership pun belum diketahui.

**Bukti HTTP aktual**, menggunakan router produk, login/session+CSRF nyata, satu principal sintetis yang hanya memiliki membership `probe-own`, dan PostgreSQL disposable:

```text
valid_member_create_without_tenant_transaction status=403
body={"error":{"message":"Tenant membership required."}}

# Fault injection: REVOKE SELECT pada tabel membership di fixture milik audit saja.
membership_lookup_error_cross_tenant_create status=201
membership_lookup_error_cross_tenant_get status=200
membership_lookup_error_orbz_write status=201
```

Cross-tenant mission dibuat/dibaca melalui RAM setelah query gagal. Tidak ada bukti anonymous bypass, pembacaan memory Orbz, atau penulisan lintas tenant ke ledger SQL dari probe ini. Revokasi privilege dilakukan harness untuk mensimulasikan kegagalan dependency; belum dibuktikan attacker dapat memicu revokasi sendiri. Masalah utamanya adalah boundary tidak fail-closed saat dependency bermasalah.

Complete/abort hanya menguji adanya membership, tanpa capability/assignment. Orbz masih mempercayai `department` dari body dan menganggap capability Enterprise sebagai owner terlepas dari role; kontrak sovereignty perlu ditentukan dan diuji.

**Arah perbaikan:** gunakan satu jalur tenant transaction yang sama dengan ledger matang; resolve membership dan otorisasi operasi di server; setiap error menolak operasi. Hindari fallback khusus unit test pada runtime produksi.

**Bukti penutupan:** matrix member/nonmember/read-only/worker/owner lintas dua tenant; error query/acquire selalu menolak; member valid bisa bekerja dengan runtime non-owner FORCE RLS; request body tidak menaikkan scope.

### R02 — High / P0 — Checkout membuat receipt settled tanpa pembayaran

**Lokasi:** `crates/host/src/router.rs:977`, `:1178`, `:1207`, `:1220`; `static/archava.js:88`, `:160`.

Route publik `/embed/checkout` mengambil tenant, order, dan amount dari query. Tombol Pay menjalankan timer 600 ms, membuat transaction ID memakai `Math.random()`, mengisi `status: 'settled'`, lalu mengirim `ARCHAVA_PAYMENT_SUCCESS`. Tidak ada lookup order, provider call, verified webhook lookup, atau durable receipt. UI menampilkan “Archava Verified Vault”, “Instant Sovereign Ledger”, dan klaim authenticated/AES record tanpa implementasi yang mendukung klaim tersebut pada alur ini.

**Bukti Chromium**, SDK `static/archava.js` aktual, host lokal aktual, order/amount sintetis:

```text
untrusted_window_callback_count=0
checkout_callback={amount:12345,orderId:"audit-order",status:"settled",tenantId:"probe-own",...}
payment_time_network_requests=[]
checkout_overlay_removed=True
missing_order_callback_accepted=True
total_external_requests=0
```

Origin/source check lama memang berhasil. Namun receipt baru berasal dari iframe origin yang benar, sehingga diterima SDK walaupun pembayaran tidak pernah dilakukan. Missing-order probe dikirim dari iframe yang sama; ini membuktikan korelasi order tidak wajib, bukan bypass origin oleh pihak luar.

**Dampak:** integrator yang mempercayai callback dapat menganggap order lunas/memberikan layanan tanpa pembayaran. Probe tidak membuktikan ledger server menandai order lunas atau uang berpindah.

**Arah perbaikan:** bila belum terintegrasi, tampilkan mode demo yang eksplisit dan jangan kirim event sukses pembayaran. Alur produksi perlu checkout session server-side yang terikat tenant/order/amount/currency, receipt durable dari provider terverifikasi, idempotency, serta correlation wajib. Default host SDK dan merchant origin policy perlu kontrak konfigurasi yang jelas.

**Bukti penutupan:** klik tanpa pembayaran tetap pending; amount/query tidak menentukan nilai tagihan server; webhook palsu/replay ditolak; callback hanya setelah status server terverifikasi; merchant origin nyata dapat menyelesaikan alur sandbox yang dipilih.

### R03 — High / P1 — SQL tambahan belum memberikan durability

**Lokasi:** `crates/host/src/orbz.rs:79`, `:148`, `:257`, `:338`; `crates/host/src/cockpit.rs:71`, `:310`, `:399`, `:547`; readiness di `crates/host` dan migration registry `crates/pg-fixture/src/lib.rs`.

Write memory/mission boleh berhasil di RAM meski SQL gagal: hasil persist dibuang dengan `let _`. Koneksi tidak memasang tenant GUC yang diwajibkan policy baru. Runtime startup masih dapat diterima ketika `archava_cockpit.missions` tidak ada; probe mencatat `startup_accepted_missing_cockpit_schema=true`, lalu create mission tetap 201 saat membership query mengalami error.

Hydration juga belum merupakan recovery lengkap. Complete/abort mencari mission dari RAM saja; load terjadi pada GET. Cache yang berisi data tidak mendapat refresh antar instance. Orbz hanya memuat DB saat hasil cache kosong, hanya mengambil `valid_to IS NULL`, lalu membuat koordinat temporal baru; historical point-in-time tidak dipulihkan sebagaimana tersimpan. Checkpoint/event recovery belum terbukti.

Ada ketidaksamaan vocabulary: persist mission memakai `Debug.to_lowercase()` (`inflight`, `holdingpattern`), loader mengenali `in_flight`, `holding_pattern`. Helper update status memakai snake case sehingga jalur tertentu benar, tetapi representasi status tidak konsisten untuk seluruh persist/reload.

**Arah perbaikan:** repository tenant-aware dengan kegagalan eksplisit; commit durable sebelum success; migrasi/grant/readiness seluruh mounted feature; codec status tunggal; cache bukan sumber kebenaran; simpan/pulihkan timestamp, checkpoint, dan event yang diperlukan.

**Bukti penutupan:** write → hentikan proses → baca/complete tanpa GET pemanasan; dua instance melihat state sama; DB/schema/RLS failure tidak menghasilkan success; historical query tetap sama setelah restart.

### R04 — High / P1 — Production worker mengganti SDK yang hilang dengan mock

**Lokasi:** `Archava Template/backend/agent.py:13`, `:19`; `Archava Template/package.json`; `.github/workflows/ci.yml:63`.

`except ImportError` pada module produksi mendefinisikan pengganti Agent/AgentSession/RealtimeModel, menjadikan `agents`, Google, Spatius, dan Tavus sebagai MagicMock. Import SDK dan semua plugin berada dalam satu blok: satu plugin hilang dapat menjatuhkan keseluruhan worker ke mode palsu. Pemanggilan `agents.cli.run_app(...)` kemudian dapat menjadi no-op.

**Bukti lokal:** LiveKit tidak terpasang di interpreter yang dipakai gate, tetapi 11 Python tests lulus. Pemeriksaan terbatas terhadap fallback module menghasilkan `agents` bertipe MagicMock dan `run_app` mengembalikan MagicMock. Tidak ada worker/provider nyata dimulai. Kelulusan ini memvalidasi perilaku pengganti lokal, belum kompatibilitas SDK sebenarnya.

**Arah perbaikan:** mocks berada di test fixtures; dependency wajib yang hilang membuat startup gagal dengan pesan jelas. Import plugin opsional hanya pada provider yang dipilih. CI memasang requirements/venv yang dipin dan menjalankan kontrak SDK asli secara offline.

**Bukti penutupan:** required SDK hilang → exit nonzero; voice-only tetap bisa berjalan tanpa plugin avatar opsional; integration lifecycle memakai class SDK asli dan tidak membutuhkan provider live.

### R05 — High / P0 — Gate release belum membuktikan artifact yang dapat dijalankan

**Lokasi:** `.cargo/config.toml:1`; `.github/workflows/ci.yml:30`; `.github/workflows/release-identity.yml:40`; `crates/slice/src/server.rs:73`; `deploy/Dockerfile`.

Workflow sudah berpindah ke Cargo root. Namun konfigurasi global mensyaratkan `sccache`, `clang`, dan `mold`; workflow tidak menyiapkan semua prasyarat tersebut. Ini risiko clean runner yang dibuktikan dari konfigurasi, bukan hasil eksekusi GitHub Actions remote. Check lokal memakai `RUSTC_WRAPPER=` dan toolchain yang sudah tersedia.

CI/release menamai gate sebagai workspace/full suite tetapi hanya menjalankan `cargo test --workspace --lib`. Sebanyak 980 tests dari unit+integration lulus pada reaudit lokal; gate saat ini tidak menjalankan seluruh cakupan tersebut. CI portable WASM hanya core/domain, bukan hydration slice. Python gate memiliki masalah R04.

Release membangun/mengunggah native server dan SHA-256 saja. Server memerlukan `archava-slice.js` dan `archava-slice_bg.wasm` dalam site bundle; workflow tidak membuat atau mengemasnya. Perbaikan path COPY Docker belum menghasilkan paket/image lengkap dari clean checkout. Tidak ada bukti deployment artifact ini berhasil start tanpa aset lokal sisa build.

**Arah perbaikan:** definisikan prasyarat runner, jalankan integration gates yang relevan, buat native+WASM/site artifact dengan identitas sama, dan jalankan startup smoke dari artifact dalam lingkungan bersih. Selaraskan release manifest/verification dan dokumentasinya; jangan mengandalkan cache workstation.

**Bukti penutupan:** clean checkout → CI → artifact/image → start runtime non-owner+migrasi → SSR/hydration/login/read/write berhasil, dengan file build yang benar-benar berasal dari pipeline tersebut.

### R06 — Medium/High bersyarat / P1 — Secret webhook dan lease restore masih memiliki fallback berbahaya

**Lokasi:** `crates/host/src/webhooks.rs:70`; `crates/ops/src/cli.rs:351`; `crates/ops/src/backup.rs:487`, `:573`.

Privy tanpa konfigurasi masih memakai token tetap `archava-privy-webhook-token`. Missing header kini ditolak, tetapi pihak yang mengetahui source dapat mengirim token fallback bila operator lupa mengonfigurasi secret. Handler saat ini terutama mengembalikan acknowledgment; dampak provisioning/ledger aktual belum dibuktikan dan akan meningkat bila acknowledgment diberi efek bisnis.

Lease restore dipanggil hanya jika runtime dibuat dan koneksi SQLx berhasil. Kegagalan salah satunya diteruskan sebagai sukses probe/skip, lalu `restore_with_key` tetap dipanggil. Connection string probe juga dibangun terpisah dari koneksi libpq restore. Public restore function sendiri tidak menjalankan lease check. Check-once juga perlu koordinasi agar writer tidak masuk setelah probe.

**Arah perbaikan:** secret hilang → feature unconfigured, tanpa nilai default. Restore harus menolak kegagalan probe, menggunakan target/credential yang sama, dan mengikat drain/lease secara aman selama restore. Batasi primitive yang dapat dipakai tanpa pemeriksaan tersebut.

**Bukti penutupan:** env secret kosong ditolak; known fallback ditolak; target tidak dapat diperiksa ditolak; undrained writer ditolak; tidak ada concurrent writer yang masuk selama restore rehearsal disposable.

### R07 — Medium bersyarat / P1 — Restore mendekripsi backup ke file dengan permission default

**Lokasi:** `crates/ops/src/backup.rs:510`.

Archive baru memang terenkripsi. Saat restore, hasil dekripsi ditulis melalui `std::fs::write` ke `.staging-pgrestore-<uuid>.tmp` di direktori manifest tanpa explicit mode 0600/private directory. Pada Unix dengan umask 022, file baru dapat menjadi 0644; keterbacaan oleh user lain bergantung pada permission direktori dan host. TempFileGuard menghapus file pada exit normal/unwind, tetapi tidak menjamin cleanup setelah SIGKILL/crash. Guard restore baru dibuat setelah write berhasil.

Ini temuan statis pada lifecycle plaintext, bukan kegagalan algoritma AES-GCM atau bukti data nyata terbaca. Deployment dengan private directory dan umask 0077 mengurangi exposure, tetapi library belum mewajibkannya.

**Arah perbaikan:** staging private dengan create-new/mode 0600 dan guard sejak pembuatan; pastikan cleanup pada error write; pertimbangkan streaming ke restore bila sesuai. Uji permission menggunakan umask yang permisif.

### R08 — Medium / P1 — Validasi domain masih berhenti pada reproduksi lama

| Komponen/lokasi | Bukti sekarang | Kontrak yang masih diperlukan |
| --- | --- | --- |
| Inventory, `crates/domain/src/erp/inventory_costing.rs:185` | Probe ByQuantity dengan quantity `[-1,2]`, fee `10` menghasilkan alokasi `[-10,20]`. Receive dengan cost negatif kini tidak membuat layer. | Tolak quantity/weight/cost invalid secara eksplisit; checked arithmetic dan konservasi nilai sampai per-unit/remaining cents. Silent no-op bukan error kontraktual yang baik untuk caller. |
| GL, `crates/domain/src/erp/general_ledger.rs:343`, `:370` | Trial balance overflow lama sekarang false; P&L/balance sheet tetap `+=`/pengurangan unchecked dan tidak menolak `tb.is_balanced=false`. | Propagasi error/sentinel yang aman; seluruh laporan menolak input invalid/overflow, termasuk batas release build. |
| Telematics, `crates/domain/src/telematics/mod.rs:60` | Frame lama ditolak. Buffer 40 byte nol dengan byte `[7]=32`, `[8]=8`, `[9]=1` diterima sebagai satu record walaupun tanpa repeated record count dan CRC. | Parse dalam declared payload boundary, validasi count/trailer/checksum dan length yang tepat; corpus malformed/fuzz lokal. Ingress perangkat produksi belum diuji. |
| Cockpit, `crates/cockpit/src/lib.rs:58` | State/prerequisite diperiksa; status step sebelum assignment tidak diperiksa, sehingga completion dapat direkam lagi dan hasil Passed ditimpa Failed selama mission tetap InFlight. | Idempotency/result identity, terminal-step policy, validasi DAG unique index/dependency/cycle, dan batas input. |

Reproduksi inventory/GL/telematics/RPA memakai source saat audit. Kegagalan fungsi domain tidak otomatis berarti ada ingress remote yang mengekspos seluruh input tersebut.

### R09 — Medium / P1–P2 — Perbaikan UX belum menyambung sampai hasil dan recovery

**Lokasi:** `crates/slice/src/session.rs:348`; `crates/slice/src/lib.rs:110`; `crates/slice/src/login.rs:55`; `crates/slice/src/pending.rs:83`; `Archava Template/src/components/SiteAwareness.tsx:99`.

Interceptor 401 membersihkan sesi lalu mengarahkan browser ke `/login`. Router UI mendefinisikan login di `/app`; backend `/login` menerima POST. Probe HTTP aktual membuktikan `GET /login` = **405**. Jadi expiry dapat memindahkan user dari halaman kerja ke error. Fetch tidak memiliki timeout/AbortSignal; login belum memiliki guard submit ganda yang lengkap.

Bridge topic sekarang menerima preview/Privy/migration lalu dispatch browser event. Penelusuran frontend tidak menemukan listener consumer tiga event tersebut. User belum memperoleh canvas/dokumen/diff sesuai tujuan tool. Marker recovery sudah dipisah domain, tetapi operasi bersamaan pada domain/tenant sama masih dapat menimpa slot yang sama.

Form login juga menambahkan domain fixture pada username tanpa `@`, menampilkan petunjuk fixture, dan meminta tenant manual. Wiring CSS yang membaik tidak menyelesaikan onboarding produksi.

**Arah perbaikan:** rute expiry menuju login yang benar dengan jalur kembali aman; feedback timeout dan retry yang mempertahankan identitas operasi; UX consumer + ack nyata; recovery per-operation dan koordinasi tab; onboarding berdasarkan akun/tenant sebenarnya.

**Bukti penutupan:** browser expired-session kembali login tanpa 405 dan melanjutkan pekerjaan aman; dua tab/operasi tidak kehilangan marker; setiap tool punya loading/success/error/cancel berbasis hasil nyata; user baru dapat masuk tanpa instruksi fixture.

### R10 — Medium/High sesuai pemakaian / P1 — Kontrak simulasi dan eval masih bisa disalahartikan

**Lokasi:** `crates/evals/src/evaluator.rs:63`, `:223`; `crates/host/src/ingestion.rs:51`; `crates/domain/src/ingestion.rs:322`; `crates/host/src/feed.rs:124`; `crates/mcp/src/server.rs:120`, `:145`; `static/archava.js:400`.

Eval kini bisa mengamati kegagalan target buatan dan memanggil validator jurnal. Tetapi target default memutuskan prompt injection, privilege, dan temporal status lewat keyword di evaluator sendiri, bukan menjalankan boundary produk yang sedang diaudit. Field `data_leakage_rate_pct` bukan pemeriksaan kebocoran data aktual, dan `verified_kernel_oracle` tidak membuktikan semua custom target/agent runtime sudah diuji.

Migration commit masih membentuk `committed_atomic` tanpa commit DB. Widget masih memakai timer untuk menampilkan aksi “berhasil dieksekusi via Ava Cockpit”. MCP menambahkan label SIMULATED tetapi teks journal masih mengatakan posted successfully; angka stock tetap fixture. Feed lebih jujur dengan `is_simulated=true`/pending, tetapi belum membuktikan worker dispatch/durable queue.

**Arah perbaikan:** capability status tunggal untuk simulated/unavailable/pending/confirmed; receipt hanya dari efek durable; evaluator menamai target dan measurement secara tepat lalu menguji handler/worker sebenarnya pada boundary offline atau sandbox yang dipilih. Oracle domain tetap berguna jika ruang lingkupnya eksplisit.

**Bukti penutupan:** tidak ada success palsu; setiap pending punya job/status yang dapat dilacak; target produk yang sengaja dirusak menyebabkan gate terkait gagal; skor leakage/keamanan memiliki observasi yang sesuai namanya.

## 4. Pemeriksaan yang benar-benar dijalankan

| Check | Hasil aktual | Batas interpretasi |
| --- | --- | --- |
| `RUSTC_WRAPPER= cargo fmt --all --check` | Lulus | Formatting saja. |
| `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Lulus | Termasuk compile target test; bukan bukti workflow clean runner. |
| `RUSTC_WRAPPER= cargo test --workspace --locked --offline -- --test-threads=1` | **980 passed, 0 failed, 0 ignored** pada 59 test binaries/cohorts; command awal exit 1 saat tahap rustdoc E0463 | Unit dan integration, termasuk PostgreSQL serta persistent launcher. Jangan menyebut command awal sepenuhnya hijau. |
| `RUSTC_WRAPPER= cargo test --workspace --locked --offline --doc` | Rerun terpisah lulus, 21 crate doc-check cohorts; 0 runnable doctests | E0463 tidak terulang setelah rebuild tanpa perubahan source. Ada beberapa build audit yang menggunakan shared target sebelumnya; penyebab pasti kegagalan artifact awal belum diisolasi. |
| `cargo check --locked --offline --target wasm32-unknown-unknown -p archava-slice --lib --features hydrate` dengan wrapper dinonaktifkan | Lulus | Compile hydration, bukan runtime browser seluruh slice atau paket release lengkap. |
| Template `npm test` | **57 passed, 0 failed** | Kontrak offline Node. |
| Template `npm run test:agent` | **11 passed** | Kualitas bukti SDK dibatasi production mock fallback R04. Tidak dihitung sebagai provider E2E. |
| Template `npm run build` | Lulus TypeScript + Vite; warning chunk besar | Main JS 849,89 kB, Spatius chunk 2.383,81 kB, WASM wrapper 1.349,77 kB sebelum gzip. Sebagian lazy-loaded; bukan semuanya initial download. |
| Cargo dependency audit | **0 vulnerability**, 1 unmaintained warning `paste 1.0.15` / RUSTSEC-2024-0436 | Database RustSec commit `40f5be6fb12ed93fc591bf4ab23b7d793b3f2239`, 1.381 advisories. Bukan bukti application logic aman. |
| Template npm dependency audit | **0 vulnerability**, seluruh severity 0 | Lockfile saat audit; 154 dependencies menurut audit metadata. |
| Domain fresh-source probes | Old landed/GL/telematics/RPA vectors tertangani; negative quantity dan missing telemetry trailer masih diterima | Local function boundary, bukan semua route ingress. |
| Router + PG disposable | Valid member 403; lookup failure → cross-tenant mission 201/get 200 dan Orbz write 201; readiness menerima missing Cockpit schema | Fault injection hanya fixture audit. Login/session/CSRF tetap diperlukan. |
| Chromium + SDK aktual | Parent-window spoof ditolak; checkout mengirim settled tanpa network; missing order diterima; GET login 405 | Origin loopback, bukan deployment/provider/payment live. Semua request eksternal diblokir; browser ditutup. |

Tidak ada source test baru ditambahkan ke produk. Harness/probe sementara berada di luar repository; hasil pentingnya disalin ke laporan ini. Tidak ada angka throughput/p95/p99 produksi yang diklaim.

## 5. Bottleneck dan arah arsitektur maintainable

| ID | Status dan bukti saat ini | Langkah yang disarankan dan bukti selesai |
| --- | --- | --- |
| P01 | **OPEN.** `crates/orbz/src/lib.rs:34`, `:62`: global RwLock, scan semua orb lintas tenant sebelum filter, clone node lengkap. Cockpit masih map/lock RAM global. Penambahan DB/cache belum memberi quota/retention atau concurrency strategy. | Repository tenant-indexed, projection sesuai tier, quota/retention, pagination. Benchmark 1k/10k/100k node dan concurrency representatif: RSS, lock wait, p95/p99. |
| P02 | **OPEN.** `crates/host/src/crm.rs:1382`, `:1396`, `:1412`, `:1437`: child history collections memakai fetch_all tanpa LIMIT. | Keyset pagination tiap koleksi, query/index sesuai pola akses. Ukur rows/bytes/query duration/pool wait dengan contact berumur panjang. |
| P03 | **OPEN.** `crates/domain/src/erp/general_ledger.rs:267`: scan entry/line untuk setiap account. Cockpit mencari dependency berulang dengan linear scan. | Aggregate GL sekali dengan checked totals; index step dan validasi DAG sekali. Buktikan output ekuivalen dan scaling pada dataset bertingkat. |
| P04 | **OPEN, overhead bertambah.** `crates/ops/src/backup.rs:404`, `:464`, `:505`: archive penuh dimuat ke RAM; encrypt/decrypt memerlukan buffer tambahan. | Hash dan crypto dengan memori dibatasi memakai format/primitive yang sesuai; jangan menciptakan skema crypto ad hoc. Ukur peak RSS dan waktu capture/restore pada ukuran database target. |
| Tambahan | Template build masih memiliki chunk besar meski avatar dipisah. Ukuran bundle sendiri belum membuktikan UX lambat pada perangkat target. | Ukur first load/join pada jaringan dan perangkat sasaran, cache/compression, long task dan memory. Optimasi berdasarkan hasil, bukan warning bundler saja. |

Susunan layer yang disarankan tetap mengikuti stack proyek: handler tipis → tenant/authz transaction service → domain invariant → repository/ledger adapter. Provider execution sebaiknya melalui durable job/outbox dengan status dan receipt yang eksplisit. Cache hanya turunan state durable. UI/SDK membaca kontrak capability/result yang sama dengan backend; simulated/pending/confirmed tidak ditentukan masing-masing komponen.

Prioritas refactor adalah mengurangi duplikasi jalur authz/persistence dan menyatukan vocabulary status, bukan menambah framework atau memecah crate tanpa kebutuhan. Gunakan codec/enum eksplisit untuk status SQL; satukan migration/readiness registry dan konfigurasi release native+browser. Ukur bottleneck dahulu sebelum mengubah struktur performa besar.

## 6. Dokumen dan kandidat cleanup

Dokumen yang perlu diperbarui setelah behavior terkait benar:

1. README, developer guide, SECURITY, dan quickstart: path root Cargo, toolchain/runner prerequisites, port/role runtime, alur build native+WASM, dan status fitur yang benar-benar tersedia. Hapus jaminan zero-mock/production-ready yang belum didukung bukti.
2. Capability/integration matrix: backend ledger yang nyata vs fixture, worker yang tersedia, mode demo checkout, provider yang sudah diuji, konfigurasi host/embed, dan arti receipt/status.
3. Backup/restore runbook: key lifecycle, private staging, target identity, lease/drain yang wajib, rehearsal, serta batas recovery. Jangan menyebut archive terenkripsi sebagai bukti seluruh lifecycle plaintext aman.
4. Eval/test guide: bedakan oracle domain, mock unit test, SDK integration offline, browser flow, dan provider E2E. Nama gate harus sesuai test yang dijalankan.
5. User flow: tenant discovery/onboarding, expired session, request timeout, pending-job recovery, multi-tab, dan tool frontend acknowledgments.

| Kandidat ditandai untuk cleanup | Tindakan sebelum menghapus/memindahkan |
| --- | --- |
| Production MagicMock/fake SDK dalam `Archava Template/backend/agent.py` | Pindah ke test-local fixture; verifikasi dependency error dan provider optional terlebih dahulu. |
| Checkout timer/receipt lokal, widget fake success, dan demo business effects | Label demo secara eksplisit atau ganti dengan alur backend nyata; jangan menghapus UI yang masih dipakai tanpa pengganti kontrak. |
| Script/tool dan comments yang masih mengacu `apps/`, `packages/`, atau `rust/` lama | Petakan caller CI/docs/export. Pertahankan hanya entrypoint aktif; update registry dan docs bersama. Snapshot migrasi memiliki banyak deletion milik agent lain: audit ini tidak menyatakan semua file deleted aman dibuang. |
| Duplikasi membership/error fallback dan best-effort persistence | Konsolidasikan setelah tenant transaction service memiliki integration tests. |
| Dead CustomEvent bridge, placeholder API, sample dashboard metrics, copy assurance berlebihan | Tentukan consumer/capability; implementasikan atau tandai unavailable, lalu hapus hanya yang tidak punya caller. |
| Generated assets, cache, local env, dump/staging, dan log | Verifikasi ignore/export allowlist dan lifecycle. Jangan hapus credential, personal profile, atau proses user yang bukan milik audit. |

Tidak ada item tabel ini yang dihapus dalam reaudit.

## 7. Urutan remediation dan syarat reaudit berikutnya

1. **Boundary dan pembayaran:** R01/R02. Jalur tenant error selalu menolak; member sah tetap bisa bekerja; tidak ada settled tanpa pembayaran server terverifikasi.
2. **Durability dan runtime asli:** R03/R04/R06/R07. Commit sebelum success; startup/readiness lengkap; worker fail-fast; restore aman pada error; staging private.
3. **Release yang bisa direproduksi:** R05. Clean pipeline menghasilkan paket native+browser yang benar-benar dapat dijalankan, dengan integration gates nyata.
4. **Correctness dan UX:** R08/R09/R10. Tutup input batas, duplicate result, expiry, tool consumer, dan kontrak simulated/pending/confirmed.
5. **Dokumentasi, cleanup, dan performa:** lakukan sesudah behavior final; gunakan pengukuran P01–P04 dan acceptance flow pengguna yang dipilih.

Untuk menilai siap launch, sepakati satu alur pelanggan prioritas dan buktikan dari onboarding sampai efek bisnis, receipt, restart/recovery, dan penanganan error melalui artifact release. Tes yang banyak dan dependency audit bersih merupakan bukti yang berguna, tetapi pada snapshot ini belum mengatasi blocker yang direproduksi di atas.
