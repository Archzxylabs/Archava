# Audit menyeluruh Archava — 28 Oktober 2026

**Status: audit saja; seluruh temuan di bawah masih OPEN.** Laporan ini menjadi satu deliverable untuk keamanan, pengujian lokal, correctness, performa, arsitektur, alur pengguna, dokumentasi, dan kandidat pembersihan. Tidak ada perbaikan kode, migrasi, penghapusan file produk, deployment, atau panggilan provider berbayar yang dilakukan.

## 1. Keputusan utama

**Working tree ini belum layak dinyatakan siap produksi multi-tenant.** Prioritas pertama adalah menyamakan otorisasi route Orbz/Cockpit dengan batas keamanan CRM yang sudah ada, menghilangkan klaim sukses tanpa efek bisnis, dan memulihkan jalur build/deployment setelah perpindahan struktur repository.

Fondasi CRM memiliki kontrol yang berguna dan hasil pengujian yang baik: 490 unit test workspace dan 71 tes integrasi terpilih lulus, termasuk penolakan lintas tenant, pencabutan grant, CSRF, approval, replay, session logout, serta startup readiness. Namun, kontrol tersebut belum diterapkan secara konsisten pada fitur agentic baru. Jumlah test yang lulus tidak menutup temuan pada jalur yang belum diuji.

Audit menemukan **24 kelompok temuan** di register berikut, ditambah empat observasi bottleneck. Tidak ditemukan bukti kompromi produksi; lingkungan produksi tidak diperiksa. Tidak ada jaminan bahwa seluruh kerentanan atau bottleneck sudah ditemukan.

### Prioritas yang disarankan

1. **P0 — sebelum membuka akses produksi:** S01–S03, C01–C02, D01. Batasi fitur yang belum memiliki otorisasi/efek nyata dan pulihkan verifikasi release.
2. **P1 — sebelum menggunakan data bisnis nyata:** S04–S08, C03–C07, U01–U02. Selesaikan persistence, validasi domain, keamanan integrasi, backup, dan kontrak hasil operasi.
3. **P2 — sebelum perluasan pengguna:** D02–D04, U03–U05, serta optimasi berdasarkan pengukuran di bagian 6.

P0/P1/P2 menunjukkan urutan pengerjaan. Severity menyatakan dampak; temuan library yang belum terhubung ke host diberi batas penerapan secara eksplisit.

## 2. Dasar audit dan cakupan

| Item | Dasar yang dipakai |
| --- | --- |
| Repository | `/home/haikaru/Archverse/Lab/Archava` |
| Tanggal | 2026-10-28, Asia/Jakarta |
| HEAD | `412f763e17f3a68a457fd76c7b929184c50c916a` |
| Kondisi | Working tree sangat berubah: 1.090 entri status Git; banyak penghapusan TS lama dan penambahan Rust belum di-commit |
| Toolchain aktual | Rust/Cargo 1.96.0 |
| Inventaris awal | 22 crate, 1.114 file di luar cache/build; 556 file sumber/test/script/SQL, sekitar 222.646 baris |
| Snapshot sumber/config | 567 file; SHA-256 agregat `b37f71cddb5f4d312f0f4633c3ccc81eac25c2f578a29797c70b76d56b597f42` |
| Dasar temuan | File yang benar-benar ada pada working tree, bukan isi HEAD atau klaim audit historis |

Digest agregat dihitung dari path relatif terurut, byte NUL, dan SHA-256 isi setiap file. Set mencakup sumber/config berekstensi `.rs`, `.sql`, `.js`, `.mjs`, `.ts`, `.tsx`, `.py`, `.json`, `.toml`, `.yaml`, `.yml`, `.sh` di `crates`, `migrations`, `static`, `tools`, `deploy`, `.github`, `Archava Template`, `config`, ditambah manifest/lock/toolchain/config Cargo root. Cache dan build output dikecualikan. Ini identitas sumber terpilih, bukan hash seluruh repository.

| Area | Pemeriksaan yang dilakukan | Batas pemeriksaan |
| --- | --- | --- |
| Rust host, identity, CRM, authority | Pembacaan boundary, route registration, tes unit dan 4 suite integrasi dengan PostgreSQL sementara | Tidak seluruh endpoint ERP diuji ulang secara dinamis |
| Orbz, Cockpit, MCP, evals, campaigns, RPA | Penelusuran caller, model otorisasi, persistence, state machine, representasi hasil | Cross-tenant HTTP dengan dua sesi nyata untuk route baru belum direproduksi; S01/S02 berdasarkan alur kode |
| Domain, adapters, SQL | Sampling invariants, parameter binding, RLS, aritmetika, reproduksi lokal inventory/GL/telematics | Bukan pembuktian formal seluruh domain atau validasi regulasi pajak/akuntansi |
| UI Rust, SDK embed, template React/Node/Python | Penelusuran alur, consumer event, security policy, tes offline dan simulasi DOM | Tidak ada rendered-browser, screen reader, mobile-device, atau live voice/provider test |
| Delivery dan maintenance | CI, Docker, scripts, docs, dependency scan, inventaris | Tidak menjalankan build Docker, deployment, atau GitHub Actions jarak jauh |
| Contoh consumer dan tools historis | Inventaris dan sampling referensi path/manifest | Seluruh consumer/parity/benchmark legacy belum dijalankan |

Audit menggunakan pekerjaan repository biasa; tidak ada skill yang diaktifkan. Koneksi eksternal hanya untuk metadata advisory dependency dan dokumentasi resmi. Tidak ada probing terhadap server publik, gateway pembayaran, bank, portal pemerintah, database pengguna, atau layanan produksi.

**Label bukti:** `R` = direproduksi lokal pada kode aktual; `T` = hasil tool/test aktual; `S` = dibuktikan melalui penelusuran statis; `H` = hipotesis yang masih memerlukan validasi runtime. Bukti statis tidak disebut sebagai eksploit HTTP yang telah berhasil.

## 3. Register temuan

| ID | Severity | Prioritas | Temuan | Bukti |
| --- | --- | --- | --- | --- |
| S01 | High | P0 | Orbz mempercayai tenant dan authority dari body | S |
| S02 | High | P0 | Cockpit tidak membatasi akses mission menurut tenant/worker | S |
| S03 | High, dampak integrator bersyarat | P0 | Callback checkout menerima pesan dari origin/window lain | R |
| S04 | Medium saat ini; High bila diberi efek nyata | P1 | Verifikasi webhook menerima konfigurasi/token yang tidak valid | S |
| S05 | High bila menjadi boundary browser | P1 | Parser allowlist RPA keliru mengenali host | R/S |
| S06 | High bila dipakai untuk data nyata | P1 | Backup Rust tidak dienkripsi sesuai janji dokumentasi | S |
| S07 | High, risiko operasional | P1 | Restore destruktif tidak menjalankan pemeriksaan writer lease | S |
| S08 | Medium, integrasi persistence | P1 | Migrasi baru berbeda dari kontrak RLS/readiness utama | S |
| C01 | High, integritas produk | P0 | UI/API/MCP mengaku selesai tanpa operasi bisnis | R/S |
| C02 | High, integritas assurance | P0 | Eval selalu lulus dan menghasilkan angka keamanan konstan | S |
| C03 | High, durability | P1 | Mission dan memory baru hanya tersimpan di RAM | S |
| C04 | Medium | P1 | Pencatatan hasil step melewati prerequisite/state machine | S |
| C05 | Medium | P1 | Landed cost negatif dan COGS negatif diterima | R |
| C06 | Medium; High jika dipakai untuk laporan resmi | P1 | Akumulasi ledger overflow tetapi tetap dianggap balanced | R |
| C07 | Medium | P1 | Decoder telematics menerima frame tidak lengkap | R |
| D01 | High, release blocker | P0 | CI/build/deploy masih mengacu struktur yang sudah dihapus | T/S |
| D02 | Medium | P2 | Dokumen aktif memberi instruksi dan jaminan yang tidak sesuai | S |
| D03 | Medium, advisory | P2 | Dependency template membawa advisory OpenTelemetry | T |
| D04 | Medium, verification gap | P2 | Lifecycle SDK Python tidak masuk gate template | T/S |
| U01 | High, alur integrasi terblokir | P1 | Route/CSP/host checkout tidak membentuk alur yang bekerja | R/S |
| U02 | High, kepercayaan pengguna | P1 | Tool agent mengklaim UI muncul tanpa consumer frontend | R/S |
| U03 | Medium | P2 | Onboarding produksi masih berupa fixture dan CSS belum tersambung | S |
| U04 | Medium | P2 | Recovery marker satu slot lintas domain/tab | S |
| U05 | Medium | P2 | Sesi kedaluwarsa, request menggantung, dan klik berulang sulit dipulihkan | S |

### S01 — Orbz mempercayai otorisasi dari permintaan

**Lokasi:** `crates/host/src/orbz.rs:33`, `:96`, `:145`, `:155`, `:158`, `:204`; `crates/host/src/router.rs:1368`; `crates/orbz/src/lib.rs:62`.

`authenticate()` memeriksa session dan, untuk mutation, CSRF. Fungsi tersebut tidak membuktikan membership tenant. `query_nodes` lalu membangun `OrbzActor` dari `req.tenant_id`, `req.actor_authority`, `req.department`, dan `req.is_owner`. Hanya `user_id` berasal dari sesi. Filter store bekerja terhadap actor yang sudah dipalsukan oleh input tersebut. Crystallize/episode juga tidak mengecek membership dan menerima tenant maupun identitas episode dari body.

**Dampak:** pengguna dengan sesi valid berpotensi membaca sovereign memory tenant lain, menulis memory pada tenant yang bukan miliknya, dan memalsukan atribusi episode. Dibutuhkan data yang sudah berada di store dan pengetahuan tenant tujuan. Ini bukan akses anonim. Penelusuran route ke store mengonfirmasi boundary yang hilang; reproduksi HTTP lintas sesi belum dilakukan.

**Remediasi:** buat actor terverifikasi dari membership server; tenant yang diminta harus dibuktikan aksesnya sebelum diteruskan. Hapus field authority/owner dari DTO publik. Terapkan izin baca/tulis, entity scope, dan atribusi actor server. Hindari menyebut filter RAM sebagai PostgreSQL RLS.

**Kriteria tutup:** dua pengguna/dua tenant; forged authority, owner, department, tenant, dan user ID ditolak; grant yang dicabut segera tidak dapat membaca/menulis; seluruh pengujian melalui router aktual.

### S02 — Cockpit hanya memeriksa keberadaan sesi

**Lokasi:** `crates/host/src/cockpit.rs:85`, `:106`, `:134`, `:141`, `:159`, `:178`, `:202`, `:209`; `crates/cockpit/src/lib.rs:136`.

Create menerima tenant body tanpa membership. Get/complete/abort hanya memanggil authentication lalu mengakses map global berdasarkan UUID. Hasil authentication tidak dipakai untuk memeriksa kepemilikan mission. Completion menerima `agent_role`, success, output, dan token cost dari client biasa tanpa worker identity/assignment.

**Dampak:** sesi valid yang mengetahui UUID mission dapat berpotensi membaca, membatalkan, atau memalsukan hasil pekerjaan tenant lain. UUID bukan kontrol otorisasi; penemuan UUID secara massal tidak dibuktikan. Bahkan dalam tenant yang benar, tidak ada pemisahan memadai antara pengguna dan worker pelapor hasil.

**Remediasi:** akses repository membutuhkan actor + tenant + mission; otorisasi ulang pada setiap read/mutation. Worker harus memiliki identitas dan assignment terbatas, receipt tervalidasi, serta key idempotency. Pisahkan izin cancel operator dari izin menyelesaikan step.

**Kriteria tutup:** tenant A tidak dapat get/abort/complete mission B; member read-only tidak dapat mutation; laporan worker salah assignment/replay ditolak.

### S03 — Keberhasilan checkout dapat dipicu oleh pesan tidak tepercaya

**Lokasi:** script aktif disajikan oleh `crates/host/src/router.rs:909`; handler berada di `static/archava.js:147`.

Handler hanya memeriksa `event.data.type === 'ARCHAVA_PAYMENT_SUCCESS'`. Tidak ada pemeriksaan origin, source iframe, order, nonce, atau receipt server. Reproduksi Node VM dengan DOM minimal mengirim pesan dari origin berbeda dan source `{}`: `onComplete` dipanggil dan overlay terhapus.

**Dampak yang terbukti:** status sukses pada integrator dapat dipalsukan. Ini tidak membuktikan perubahan ledger atau perpindahan dana. Dampak menjadi tinggi bila integrator melakukan fulfillment hanya dari callback tersebut.

**Remediasi:** cocokkan origin yang dinormalisasi dan `event.source === iframe.contentWindow`; ikat nonce/order/session; backend merchant harus mengonfirmasi pembayaran melalui API terautentikasi sebelum fulfillment.

**Kriteria tutup:** origin/source/order/nonce salah, receipt malformed, dan replay tidak memanggil completion; receipt pembayaran yang valid tetap dapat dipulihkan setelah reload.

### S04 — Webhook memiliki mode penerimaan yang gagal menolak

**Lokasi:** `crates/host/src/webhooks.rs:70`, `:75`, `:119`, `:138`; route publik `crates/host/src/router.rs:491`.

Privy menerima header yang tidak ada dan menerima token nonkosong apa pun; tidak ada pencocokan terhadap secret yang dikonfigurasi. Payment memakai server key contoh bila environment hilang. Payload payment juga tidak dicocokkan dengan expected order/amount pada storage. Saat ini handler hanya mengembalikan status seperti `provisioning_initiated`; tidak ada provisioning nyata yang terlihat di handler.

**Dampak:** penerimaan event palsu dan acknowledgment yang menyesatkan. Pembayaran/provisioning palsu end-to-end belum dibuktikan. Menambahkan side effect ke handler ini tanpa memperbaiki verifikasi akan memperbesar risikonya.

**Remediasi:** missing configuration harus menolak aktivasi route; lakukan verifikasi sesuai kontrak provider, order/amount/currency binding, deduplikasi event, aturan transisi, audit, dan rekonsiliasi server. Response harus menyatakan efek yang benar-benar terjadi.

**Kriteria tutup:** missing/wrong token/key, amount/order mismatch, replay, dan status out-of-order ditolak atau diproses secara idempotent; sandbox-provider test terpisah dari tes offline.

### S05 — Allowlist RPA memakai pemisahan string sebagai parser URL

**Lokasi:** `crates/rpa/src/security.rs:18`, `:28`; `crates/rpa/src/executor.rs:15`.

`is_domain_allowed` mengambil bagian sebelum `/`, lalu sebelum `:`. URL `https://pajak.go.id:443@127.0.0.1/private` lolos filter karena bagian yang dibandingkan adalah `pajak.go.id`, padahal host URL adalah loopback. Reproduksi fungsi dari sumber aktual menghasilkan `true`.

**Batas dampak:** crate yang ditemukan membangun recipe; tidak ditemukan executor browser/network produksi yang memakai input tersebut. Ini bypass validator lokal dan risiko integrasi laten, bukan SSRF jaringan yang telah dibuktikan.

**Remediasi:** parser URL standar, HTTPS wajib, userinfo ditolak, host/port dibandingkan eksplisit; validasi redirect dan tujuan koneksi saat executor benar-benar dibuat. Jangan menganggap fungsi masking credential sebagai enkripsi.

**Kriteria tutup:** userinfo, port, mixed case, encoded host, redirect keluar domain, IP lokal, dan skema non-HTTPS diuji terhadap parser yang sama dengan transport.

### S06 — Backup native belum memenuhi kontrak enkripsi

**Lokasi:** `crates/ops/src/backup.rs:180`, `:198`, `:222`; `crates/ops/src/cli.rs:294`; klaim `SECURITY.md` pada defensive baseline dan `deploy/README.md:138`.

Capture menulis `pg_dump --format custom` langsung ke `archava-backup.dump`, lalu menghitung SHA-256. Tidak ada pengambilan key atau tahap enkripsi di fungsi tersebut. SHA-256 adalah pemeriksaan integritas, bukan kerahasiaan. Format custom PostgreSQL merupakan archive yang dapat dikompresi, bukan jaminan enkripsi. [Dokumentasi pg_dump](https://www.postgresql.org/docs/current/app-pgdump.html).

**Dampak:** jika jalur backup native dipakai untuk database nyata, siapa pun yang memperoleh archive dapat membaca data tanpa backup key. Pengujian pengambilan backup data nyata tidak dilakukan; gap dibuktikan dari implementasi. Jalur CLI juga terdampak D01, sehingga kesiapan operasionalnya belum terbukti.

**Remediasi:** tetapkan satu format backup operasional dengan authenticated encryption, key reference/version, private permissions, penulisan atomik, dan restore rehearsal. Jika capture hanya untuk rehearsal, beri batas tegas dan jangan dokumentasikan sebagai backup produksi terenkripsi.

**Kriteria tutup:** key hilang/salah menolak; archive tidak dapat dibaca `pg_restore` sebelum dekripsi; tampering ditolak; restore data sintetis membuktikan schema, data, grant, dan tenant isolation.

### S07 — Restore dapat menimpa target aktif tanpa lease check

**Lokasi:** `crates/ops/src/cli.rs:343`; `crates/ops/src/backup.rs:291`, `:296`, `:314`, `:340`.

CLI memanggil `restore()` secara langsung. Helper `refuse_undrained_target()` tersedia tetapi tidak dipanggil di jalur CLI itu. Restore menjalankan `pg_restore --clean --if-exists`; penolakan source hanya membandingkan string host/port/database, sehingga bukan identitas server yang kuat. Tidak ada pemaksaan target rehearsal yang terisolasi di dalam fungsi restore.

**Dampak:** operator yang memilih target keliru dapat menghapus/mengganti objek database yang masih dipakai writer. Ini risiko operasi berprivilege, bukan eskalasi remote. Tidak dilakukan percobaan restore destruktif pada database mana pun.

**Remediasi:** pemeriksaan target/fencing wajib berada di jalur eksekusi yang tidak dapat dilewati; identifikasi database secara otoritatif, verifikasi lease/drain, target allowlist, dan lakukan restore ke target baru sebelum cutover. Hindari mencampur rehearsal dan pemulihan produksi dalam satu kontrak ambigu.

**Kriteria tutup:** target source lewat alias host, target writer aktif, identity mismatch, archive rusak, dan target yang tidak diizinkan ditolak sebelum `pg_restore` dipanggil.

### S08 — Migrasi baru tidak mengikuti kontrak RLS utama

**Lokasi:** `migrations/orbz-pg/001_agentic_memory_orbz.sql:85`; `migrations/cockpit-pg/001_cockpit_durable_missions.sql:99`; `migrations/universal-pg/001_universal_enterprise_ledgers.sql:319`; `crates/adapters/src/rls.rs:94`; `crates/slice/src/bin/archava_server.rs:232`.

Orbz/Cockpit mengaktifkan RLS tanpa `FORCE ROW LEVEL SECURITY` dan menggunakan `archava.current_tenant`. TenantTransaction utama mengatur `archava.tenant_id`. Universal ledgers juga memiliki tabel dengan ENABLE tanpa FORCE. Launcher persistent tidak mencantumkan Orbz/Cockpit/universal dalam daftar readiness yang ditelusuri.

**Dampak:** saat persistence baru dihubungkan ke transaksi utama, policy dapat menyaring semua data karena setting berbeda. Role pemilik tabel dapat melewati RLS tanpa FORCE. Runtime non-owner yang sudah benar tidak otomatis memperoleh bypass; saat ini RAM path C03 bahkan tidak menggunakan policy SQL tersebut. Temuan ini tidak sama dengan eksploit cross-tenant database yang sudah dibuktikan.

**Remediasi:** satu kontrak tenant setting, forced RLS untuk seluruh tabel tenant, role separation, grants terukur, checksum, dan katalog readiness/migrasi yang mencakup semua fitur aktif.

**Kriteria tutup:** cold bootstrap runtime non-owner; setting tenant salah/hilang menolak; source dan target tenant tak dapat saling membaca/menulis; kesiapan boot menolak policy/checksum/grant drift pada modul baru.

### C01 — Banyak permukaan produk mengembalikan sukses tanpa efek bisnis

**Lokasi:** `static/archava.js:321`, `:368`, `:458`; `crates/host/src/feed.rs:74`, `:100`; `crates/domain/src/ingestion.rs:306`; `crates/host/src/ingestion.rs:51`; `crates/mcp/src/server.rs:113`, `:124`.

Widget memakai kartu hardcoded dan timer untuk menampilkan “Berhasil!”; chat berupa percabangan keyword dan voice-only tidak membuka transport mikrofon. Feed execute mengembalikan `executed` beserta kalimat WA/PO/printer/invoice selesai tanpa dispatch. Ingestion commit hanya memeriksa UUID dan jumlah valid, kemudian mengembalikan `committed_atomic` tanpa menyimpan record. MCP menyatakan stok 142 unit dan jurnal telah diposting tanpa repository/domain execution.

**Bukti:** widget menghasilkan sukses dalam VM yang tidak menyediakan fetch/WebSocket/jaringan sama sekali. Alur API/MCP/commit diperiksa statis. Status API yang dapat dipanggil tanpa auth tidak membuktikan penulisan ledger tanpa auth karena side effect memang belum ada.

**Dampak:** pengguna dapat mengira pasien telah dihubungi, PO/invoice sudah dibuat, atau data sudah tersimpan. Ini mengganggu operasi dan kepercayaan meskipun tidak memindahkan data/uang pada backend.

**Remediasi:** beri label demo pada simulasi dan keluarkan dari jalur operasional; capability belum tersedia harus menolak secara jelas. Operasi nyata harus melalui application service yang mengembalikan receipt setelah commit atau status accepted/unknown yang dapat direkonsiliasi.

**Kriteria tutup:** tidak ada status sukses tanpa effect/receipt yang dapat dibaca ulang; network/provider gagal tidak boleh sukses; input tak valid dan operasi belum tersedia ditolak; read model menampilkan data tenant aktual.

### C02 — Eval menghasilkan bukti keamanan yang tidak diukur

**Lokasi:** `crates/evals/src/evaluator.rs:40`, `:47`, `:98`.

Setiap cabang `ExpectedBehavior` mengembalikan `true`. Tidak ada target yang dipanggil untuk menghasilkan observed behavior. Financial adherence selalu 100%, leakage selalu 0%, dan suite kosong tetap mendapat compliance 100%.

**Dampak:** laporan dapat terlihat membuktikan penolakan, invariants, dan kebocoran nol meskipun sistem diuji tidak pernah dijalankan. Hasil ini tidak dapat dipakai untuk menyatakan compliance atau menutup temuan security. Ini tidak membatalkan tes integrasi nyata yang terpisah.

**Remediasi:** runner menerima target/fixture, merekam observed output/effect, membandingkan dengan oracle, dan melaporkan untested/error secara terpisah. Klaim persentase hanya berasal dari denominator pengujian aktual; suite kosong tidak boleh lulus sebagai bukti.

**Kriteria tutup:** target sengaja rusak membuat eval gagal; perubahan seed/case terlihat; artifact mencatat source identity, input, hasil, dan alasan; hasil simulasi diberi label eksplisit.

### C03 — Durable execution dan memory masih berupa state proses

**Lokasi:** `crates/cockpit/src/lib.rs:23`; `crates/orbz/src/lib.rs:31`; inisialisasi/akses engine `crates/host/src/router.rs` pada AppState.

Mission, checkpoint, event, memory, dan episode disimpan dalam `Arc<RwLock<HashMap/Vec>>`. Adanya file migrasi tidak membuat engine tersebut persisten. Tidak ditemukan load/replay repository pada serving path engine ini.

**Dampak:** restart kehilangan state baru; beberapa instance memiliki state berbeda; checkpoint tidak memberi pemulihan crash yang dijanjikan. Data ledger CRM yang benar-benar disimpan di PostgreSQL berbeda dan tidak terkena kesimpulan ini.

**Remediasi:** definisikan port repository mission/memory, PostgreSQL sebagai authority, update step + event + checkpoint atomik, outbox untuk side effect, worker claim/lease, idempotency dan restart reconciliation. RAM hanya cache yang dapat dibangun ulang.

**Kriteria tutup:** tulis lewat HTTP, hentikan proses, mulai ulang, lalu buktikan state/receipt dapat dipulihkan; dua instance sepakat dan tidak menggandakan side effect.

### C04 — Prerequisite step tidak dipaksa pada completion

**Lokasi:** `crates/cockpit/src/lib.rs:57`; `crates/cockpit/src/mission.rs:95`; `crates/host/src/cockpit.rs:116`.

`next_executable_steps` menghitung prerequisite, tetapi `record_step_result` langsung mencari step lalu mengubah status. Tidak diperiksa bahwa mission InFlight, step Running/assigned, dependency sudah Passed, atau report belum pernah dipakai. Create juga mengabaikan error auto-launch. DAG menerima struktur tanpa validasi cycle/duplicate index yang terlihat pada konstruktor.

**Dampak:** hasil step dapat direkam di luar urutan atau setelah status yang seharusnya menolak, dan state dapat berbeda dari aturan scheduler. Ini masalah tambahan setelah otorisasi S02 diperbaiki.

**Remediasi:** satu fungsi transisi memaksa state/prerequisite/assignment/version; validasi DAG sebelum register; jangan abaikan kegagalan launch; report replay harus deterministik.

**Kriteria tutup:** complete sebelum launch, prerequisite gagal, duplicate/cyclic index, completion ganda, dan complete setelah abort ditolak; jalur sah tetap menghasilkan satu checkpoint/event konsisten.

### C05 — Inventory dapat menghasilkan biaya negatif

**Lokasi:** `crates/domain/src/erp/inventory_costing.rs:75`, `:125`, `:215`, `:224`.

Empat item identik dengan quantity 1 dan fee total 2 menghasilkan alokasi `[1, 1, 1, -1]`: pembulatan setiap item awal menghabiskan lebih dari fee total, kemudian item terakhir menjadi negatif. `receive_goods` juga menerima unit cost -50; depletion quantity 1 menghasilkan COGS -50. Input ini direproduksi pada modul sumber aktual.

**Batas dampak:** fungsi library; caller HTTP produksi untuk fungsi-fungsi ini tidak dibuktikan. Selain nilai negatif, pembagian fee per-unit membuang remainder sehingga nilai unit dapat tidak merepresentasikan nilai total inventory.

**Remediasi:** validated quantity/cost/weight, aritmetika integer terperiksa, alokasi largest-remainder yang nonnegatif dan konservatif, serta aturan penyimpanan remainder/total cost. Gagal validasi harus mengembalikan error tanpa perubahan state.

**Kriteria tutup:** property test total fee terjaga, tiap alokasi nonnegatif, urutan deterministik, remainder tak hilang; negative/zero/extreme input ditolak sesuai aturan domain.

### C06 — Trial balance dapat overflow sambil tetap balanced

**Lokasi:** `crates/domain/src/erp/general_ledger.rs:266`, `:281`, `:293`, `:315`, `:342`; operasi tak terperiksa juga ada di inventory costing `:169` dan `:174`.

Validasi tiap jurnal memakai checked addition, tetapi akumulasi trial balance memakai `i64 +=`. Dua jurnal yang masing-masing valid dengan nilai `i64::MAX` menghasilkan debit -2, kredit -2, dan `is_balanced=true` ketika memakai semantik overflow release. Reproduksi memakai sumber aktual dengan `rustc -C overflow-checks=no`, sesuai default profil release yang tidak mengaktifkan overflow checks. Pada build debug operasi dapat panic.

**Dampak:** silent corruption pada nilai ekstrem atau panic di konfigurasi lain. Nilai contoh merupakan boundary ekstrem; tidak dibuktikan bahwa API publik saat ini dapat memasukkan nilai tersebut ke laporan.

**Remediasi:** tipe Money/Quantity dengan batas domain, checked arithmetic atau accumulator lebih lebar yang akhirnya dikonversi secara terperiksa; fungsi laporan mengembalikan error, bukan balance palsu. Verifikasi akun/currency/tenant pada application boundary.

**Kriteria tutup:** batas maksimum, banyak jurnal, penjumlahan/pengurangan/perkalian overflow diuji pada debug dan release; hasil invalid tidak boleh berstatus balanced.

### C07 — Decoder menerima frame telematics tidak lengkap

**Lokasi:** `crates/domain/src/telematics/mod.rs:62`, `:155`, `:163`, `:219`.

Frame 34 byte dengan preamble nol, data length 26, Codec 8, satu record, dan sisa nol diterima sebagai satu record walaupun bagian IO/trailer/CRC tidak lengkap. Parser menjadikan sejumlah bagian IO opsional dan melakukan skip 8-byte IO tanpa pengecekan akhir yang memadai.

**Dampak:** bila dihubungkan ke ingress kendaraan, telemetry rusak dapat dianggap valid. Tidak ada pengujian terhadap tracker nyata atau bukti host ingress ini aktif.

**Remediasi:** cursor reader dengan bounds pada setiap field; panjang, record count kedua, IO count, trailer/CRC dan trailing bytes diverifikasi sesuai framing transport. Jangan menerima record parsial sebagai default value.

**Kriteria tutup:** truncation pada setiap offset, count mismatch, CRC salah, koordinat invalid, extra data, dan multi-record diuji; fuzz parser menolak malformed input tanpa panic.

### D01 — Jalur verifikasi dan deployment rusak setelah restrukturisasi

**Lokasi:** `.github/workflows/ci.yml:72`; `.github/workflows/release-identity.yml:46`; `.github/workflows/security-audit.yml` pada job pnpm; `.github/dependabot.yml:33`; `tools/rust/build-slice.mjs:15`; `tools/rust/run-persistent.mjs:14`; `deploy/Dockerfile:24`; `.dockerignore`.

Root `package.json`, `pnpm-lock.yaml`, workspace pnpm, `rust/`, `apps/`, dan `packages/` sudah tidak ada. CI/release masih memanggil pnpm root; beberapa tools masih memakai `root/rust` atau source TS yang dihapus. Dependabot Cargo diarahkan ke `/rust`. Docker COPY dan allowlist context memakai `rust/target/...`, sedangkan workspace/artifact saat ini berada di root `target/...`.

**Bukti aktual:** `pnpm run rust:verify` gagal `ERR_PNPM_NO_PKG_MANIFEST`; kedua sumber COPY Docker terdeteksi tidak ada. Rust direct tests/clippy lulus secara lokal, tetapi itu tidak memperbaiki wrapper/gate lama. Status GitHub Actions remote tidak diperiksa; klaim ini berlaku bila working tree tersebut diterbitkan dengan konfigurasi sekarang.

**Remediasi:** satu entrypoint build/check Rust yang konsisten, bootstrap compiler/linker eksplisit di CI, build native + WASM yang reproducible, workflow test/security/release/SBOM yang benar-benar menarget source sekarang; update Docker/context dan dependency automation. Pertahankan template standalone dengan job sendiri.

**Kriteria tutup:** fresh checkout tanpa cache dapat build/test/check native dan browser, membuat artifact/image dari path yang benar, menjalankan disposable DB proofs, dan menghasilkan release evidence yang cocok dengan sumber; tak ada ketergantungan ke file terhapus.

### D02 — Dokumen aktif dan source comments memberi kontrak yang keliru

**Lokasi:** `README.md` bagian overview/setup/enterprise guarantees; `docs/DEVELOPER_GUIDE.md:12`, `:60`; `deploy/README.md:13`, `:38`, `:93`, `:138`; `docs/rust/PERSISTENT_RUNTIME.md:3`, `:21`; `SECURITY.md` bagian scope/baseline.

Quickstart menyebut Rust 1.88+, sementara manifest/toolchain mengharuskan 1.96. Developer guide memakai port 3000 dan contoh role `postgres`, sedangkan launcher default 8347 dan menolak superuser/owner. Runbook memakai path/command TS yang sudah hilang. Klaim “zero-mock”, “all modules real”, durable memory, sandbox RPA, enkripsi backup, dan eval kebocoran nol belum didukung implementation evidence yang dijelaskan di laporan ini.

**Dampak:** onboarding macet, operator memilih konfigurasi yang ditolak, pengguna/reviewer menilai kemampuan dan jaminan secara keliru. Ini bukan audit kepatuhan IFRS/PSAK/HIPAA/pajak; klaim tersebut memerlukan validasi domain terpisah.

**Remediasi:** matriks kemampuan `demo / implemented / locally verified / integration verified / production validated`; quickstart yang benar-benar dijalankan dari clean state; pemilik dan tanggal verifikasi tiap runbook. Historical audit tetap disimpan sebagai bukti historis, tidak dianggap bukti current release.

**Kriteria tutup:** seluruh command/path aktif dapat dijalankan pada checkout baru; claims menunjuk tes/artifact aktual; docs versi/port/role dan status integrasi konsisten.

### D03 — Advisory dependency pada template

**Lokasi:** `Archava Template/package-lock.json:625`; dependency produksi transitive dari paket Spatius.

`npm audit --json` melaporkan 13 entri paket severity moderate, nol high/critical. Ini penyebaran satu root advisory `@opentelemetry/core <2.8.0`, sementara lock memuat 2.7.1. Advisory menyangkut alokasi memory ketika mengekstrak W3C Baggage; versi patch adalah 2.8.0. Batas header HTTP default Node mengurangi paparan transport HTTP biasa. [Advisory resmi proyek](https://github.com/advisories/GHSA-8988-4f7v-96qf).

**Batas dampak:** 13 entri bukan 13 eksploit berbeda. Reachability extractor dari input aplikasi belum dibuktikan. Scan Rust menemukan nol vulnerability advisory dan satu warning unmaintained `paste 1.0.15`; warning tersebut bukan bukti exploit. [RustSec RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html).

**Remediasi:** upgrade graph yang kompatibel atau override setelah compatibility test; jangan memakai `audit fix --force` tanpa review karena saran resolver mencakup perubahan versi Spatius. Pantau warning transitive saat upstream dapat menggantinya.

**Kriteria tutup:** dependency lock konsisten, advisory terkait hilang atau mitigation terukur dicatat, avatar renderer dan lifecycle session tetap bekerja.

### D04 — Gate template tidak menjalankan integration SDK Python

**Lokasi:** `Archava Template/package.json` script `test:agent`; `backend/integration_tools.py`, `backend/integration_worker.py`; `.github/workflows/ci.yml:203`.

`test:agent` hanya memilih `test_*.py`; dua modul lifecycle SDK memakai nama `integration_*.py`. Job template tidak memasang environment Python/requirements dan tidak menjalankan pola integration. Empat guardrail test yang lulus tidak membuktikan lifecycle SDK.

**Bukti lokal:** pola integration gagal import `livekit` karena dependency/venv tidak tersedia; ini kegagalan environment, bukan assertion regression. Build template juga terblokir `tsc` belum terpasang, dan satu entry npm test gagal import `livekit-server-sdk`.

**Remediasi:** job Python dengan requirements terpin dan integration mock tanpa provider live; pisahkan unit/protocol/SDK/sandbox-provider checks. Jalankan clean install/build template dalam gate yang benar.

**Kriteria tutup:** perubahan dispatch metadata, guest binding, expiry, provider tuning, dan cleanup dapat membuat CI merah ketika kontrak dirusak; missing deps tidak dianggap lulus.

### U01 — Checkout/embed belum membentuk alur pengguna yang dapat selesai

**Lokasi:** `static/archava.js:82`, `:115`, `:188`; `crates/host/src/security.rs:61`; `crates/slice/src/server.rs:57`; `crates/slice/src/erp.rs:102`, `:139`; `Archava Template/server/app.mjs:49`.

Checkout membuka `/app/erp`; halaman tersebut berisi sample metrics dan simulasi signal lokal, bukan checkout yang membaca order server. Policy dokumen `frame-ancestors 'none'` melarang iframe. Tanpa `options.host`, SDK mengarah ke origin merchant, bukan origin script. Reproduksi menunjukkan URL default `https://merchant.test/app/erp?...`. Instruksi iframe template juga bertentangan dengan policy server Node-nya.

**Dampak:** pemasang script dapat memperoleh route merchant yang salah atau frame terblokir; pengguna tidak dapat menyelesaikan transaksi. Browser rendering tidak dijalankan, tetapi konflik route/policy berasal dari source yang sama dengan serving path.

**Remediasi:** route customer/embed khusus dengan allowlist ancestor per tenant, order session dari server, host eksplisit/diturunkan dari script URL tervalidasi. Admin ERP tetap memiliki pembatasan framing sendiri. Tampilkan status belum tersedia sampai checkout nyata siap.

**Kriteria tutup:** dua origin integrator, allowed/disallowed tenant, mobile, cold load, payment pending/failure/success, reload dan server receipt verification berjalan end-to-end.

### U02 — Tool percakapan mengklaim efek UI yang tidak memiliki consumer

**Lokasi:** `Archava Template/backend/agent.py:99`, `:121`, `:147`; `Archava Template/backend/site_context.py:12`; `Archava Template/src/components/SiteAwareness.tsx:89`.

Tool preview, SOW Privy, dan migrasi hanya melakukan publish data pada `archava.preview`, `archava.privy`, dan `archava.migration`, lalu menyatakan tampilan telah aktif. Listener frontend hanya menangani ACK halaman/navigation; pencarian sumber frontend tidak menemukan consumer ketiga topic tersebut. SOW tidak dibuat dan modal tanda tangan tidak dipanggil oleh fungsi tool. Reproduksi fungsi aktual yang diekstrak melalui AST dengan mock publish menunjukkan klaim sukses tetap dihasilkan setelah satu publish.

**Dampak:** visitor diberi janji bahwa preview/kontrak/migrasi tersedia padahal layar tidak berubah; agent dapat meneruskan percakapan seolah langkah legal/komersial sudah terjadi.

**Remediasi:** tool yang belum tersedia dinonaktifkan atau mengembalikan status unavailable; request UI memakai ACK/result bertipe dan timeout. Knowledge/prompts/status produk diperbarui bersama kontrak UI yang nyata.

**Kriteria tutup:** consumer absen, publish gagal, ACK terlambat/ditolak, provider unavailable, dan consent belum ada tidak menghasilkan klaim selesai.

### U03 — Onboarding dan tampilan masih memakai permukaan fixture

**Lokasi:** `crates/slice/src/server.rs:53`, `:99`; `crates/slice/src/login.rs:87`, `:109`, `:149`; `crates/slice/src/session.rs:218`.

Shell tidak memuat stylesheet; serving setup hanya menyambungkan `/pkg` dan route Leptos, tanpa hubungan yang ditemukan ke CSS di `static`. Login menampilkan akun/password fixture dan copy pengujian internal, meminta tenant diketik manual, dan default `acme-retail`. Ini menambah beban pengguna baru yang belum mengenal tenant ID.

**Batas dampak:** keberadaan teks password fixture tidak membuktikan akun tersebut aktif di produksi. Tampilan belum dinilai melalui screenshot/browser.

**Remediasi:** asset pipeline eksplisit, fixture hint hanya di mode preview, sign-in lalu pemilihan workspace berdasarkan membership aktual, empty state dengan satu langkah berikutnya, istilah bisnis yang konsisten. Hilangkan jargon compiler/ledger/receipt dari aksi pengguna umum kecuali membantu keputusan mereka.

**Kriteria tutup:** pengguna nonteknis dapat sign-in, memilih bisnis, membuat record pertama, dan menemukan hasilnya tanpa tenant ID manual; viewport 320/390/768/1440px, keyboard dan status/error dapat digunakan.

### U04 — Identitas recovery mudah tertimpa lintas tab dan domain

**Lokasi:** `crates/slice/src/pending.rs:68`, `:141`, `:165`, `:191`; `crates/slice/src/owner.rs:155`.

Storage key hanya memakai tenant, sementara beberapa flow CRM dan Owner memakai marker yang sama. Write selalu menimpa; clear tidak membandingkan request ID. Owner memeriksa marker melalui `/own/status` tanpa namespace domain. Dua tab/operasi dapat saling mengganti atau menghapus marker. `clear_all()` juga menghapus key sambil iterasi index maju, sehingga key yang bergeser dapat terlewat.

**Dampak:** hilangnya identitas recovery atau lookup ke domain salah; pengguna mungkin membuat intent baru saat hasil lama belum diketahui. Ini bukan bukti bypass idempotency backend. Race dua tab belum direproduksi di browser.

**Remediasi:** key principal + tenant + domain + request; metadata operation type; compare-and-remove berdasarkan request ID; koordinasi tab; ambil snapshot key sebelum penghapusan. Simpan outcome unknown sampai dapat direkonsiliasi.

**Kriteria tutup:** dua tab, dua intent, CRM lalu Owner, response terlambat, account switch, dan logout dengan beberapa marker tidak kehilangan/mengadopsi identitas operasi yang salah.

### U05 — Kedaluwarsa sesi dan request lambat tidak memiliki pemulihan terpusat

**Lokasi:** `crates/slice/src/session.rs:181`, `:298`; `crates/slice/src/login.rs:38`, `:119`; `crates/slice/src/contacts.rs:144`, `:350`, `:421`.

Restore menerima keberadaan localStorage sebagai state UI; login mengalihkan held session langsung ke contacts. HTTP 401 ditampilkan sebagai refusal tanpa reset/re-auth terpusat. Fetch tidak memiliki AbortSignal/deadline aplikasi. Tombol submit tertentu tetap aktif saat Busy.

**Dampak:** cookie expired dapat membuat pengguna berputar ke layar refusal, koneksi yang tidak selesai mempertahankan loading, dan klik berulang memicu permintaan tambahan. Perlindungan server tetap dapat menolak akses, sehingga ini bukan session-auth bypass.

**Remediasi:** bootstrap sesi dari server, handler 401 terpusat, re-auth yang menjaga identitas operasi unknown, deadline dan retry sesuai idempotency, Busy guard serta tombol pemulihan yang jelas.

**Kriteria tutup:** expiry saat reload/write, koneksi menggantung, server unavailable, klik ganda, dan login ulang tidak membuat duplikasi operasi atau menghilangkan recovery.

## 4. Hasil pengujian yang benar-benar dijalankan

| Pemeriksaan | Hasil aktual | Makna dan batas |
| --- | --- | --- |
| `cargo fmt --all --check` | Lulus | Pemeriksaan format, bukan keamanan |
| `RUSTC_WRAPPER= cargo test --workspace --lib --locked --offline` | 490 lulus, 0 gagal; 22 cohort crate | Unit library saja; bukan seluruh integration/doctest/browser suite |
| `RUSTC_WRAPPER= cargo test --locked --offline -p archava-host --test authority --test session --test readiness --test crm -- --test-threads=1` | 71 lulus: authority 18, CRM 36, readiness 5, session 12 | PostgreSQL sementara; tidak semua domain ERP atau route baru |
| `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Lulus, exit 0 | Static compiler/lint check |
| `cargo audit --json` | 0 vulnerability advisory; 1 warning unmaintained `paste` | Advisory DB 1.380 entri, commit `99670cfc0bc574433034b75876a4b791ccaa2efb`, updated 2026-10-27 |
| Template `npm audit --json` | 13 moderate package entries; 0 high/critical | Satu root advisory OTel, reachability belum dibuktikan |
| Template `npm test` | 52 pass, 1 fail dari 53 entries | `ERR_MODULE_NOT_FOUND livekit-server-sdk`; dependency lokal tidak terpasang |
| Template `npm run test:agent` | 4/4 lulus | Offline guardrails saja |
| Template `npm run build` | Config check lulus; berhenti `tsc: command not found` | Build produksi belum tervalidasi |
| Python discover `integration_*.py` | Dua modul gagal import `livekit` | Environment SDK belum tersedia; bukan hasil assertion test |
| Node VM checkout event | Callback dari origin/source tidak tepercaya diterima | Tanpa network, data sintetis |
| Node VM widget action | Berhasil ditampilkan tanpa transport jaringan | Membuktikan simulasi sukses pada widget |
| Fungsi agent aktual + AST/AsyncMock | SOW mengklaim modal aktif setelah publish saja | Tanpa import provider/SDK atau dokumen nyata |
| Fresh-source Rust inventory/GL/telematics probe | Mereproduksi C05–C07 | Local function boundary; tidak membuktikan ingress remote |
| RPA validator probe | Host confusion diterima | Tidak menavigasikan browser atau mengirim request |
| `pnpm run rust:verify` | Gagal: root manifest tidak ada | Jalur wrapper/gate legacy rusak |
| Validasi path COPY Docker | Dua input `rust/target/...` tidak ada | Image belum dibangun |
| Narrow secret-signature scan | Tidak ada match pada signature private-key/AWS/GitHub/live-key terpilih | Bukan entropy scan, git-history audit, atau pemeriksaan seluruh ignored secret |

Tidak dilakukan install dependency baru, load test produksi, fuzzing menyeluruh, DAST seluruh route, browser QA, test provider nyata, review seluruh versi regulasi pajak, atau pengujian mobile hardware. Beberapa area memerlukan sesi pengujian lanjutan setelah environment dan kontraknya siap; tidak diberi status lulus melalui asumsi.

### Output reproduksi domain

Probe mengimpor langsung file modul sumber saat ini, link ke dependency `serde` yang sudah tersedia, memakai folder sementara, dan tidak mengubah modul:

```text
landed_cost_fees=[1, 1, 1, -1]
negative_unit_cost_cogs=-50
truncated_telematics_records=Ok(1)
individual_journals_valid=true
release_arithmetic_trial_debits=-2 credits=-2 balanced=true
```

Input minimalnya: empat `LandedCostLineInput` quantity=1/base cost=10/weight=1, total fee=2, ByQuantity; FIFO receive quantity=1/cost=-50 lalu deplete 1; packet telematics 34 byte nol dengan byte 7=26, byte 8=8, byte 9=1; dua jurnal posted dengan debit akun 10100 dan kredit akun 40100 masing-masing `i64::MAX`.

### Reproduksi callback embed tanpa network

Jalankan dari root repository. Skrip hanya mengevaluasi SDK dalam DOM tiruan dan tidak melakukan pembayaran/request:

```bash
node --input-type=module <<'JS'
import fs from 'node:fs';
import vm from 'node:vm';
const listeners = new Map();
function el() {
  return {
    children: [], style: {}, setAttribute() {},
    appendChild(c) { c.parentNode = this; this.children.push(c); },
    removeChild(c) { this.children = this.children.filter(x => x !== c); }
  };
}
const document = {
  createElement: el, body: el(), head: el(),
  currentScript: null, querySelector() { return null; }
};
const window = {
  location: { origin: 'https://merchant.test' },
  addEventListener(k, v) { listeners.set(k, v); },
  removeEventListener(k) { listeners.delete(k); }
};
vm.runInNewContext(fs.readFileSync('static/archava.js', 'utf8'),
  { window, document, URLSearchParams });
let receipt;
window.Archava('synthetic', { host: 'https://archava.test' })
  .openCheckout({ orderId: 'order-1', onComplete: x => receipt = x });
listeners.get('message')({
  origin: 'https://unrelated.test', source: {},
  data: { type: 'ARCHAVA_PAYMENT_SUCCESS', receipt: { synthetic: true } }
});
console.log({ callbackAccepted: receipt?.synthetic === true });
JS
```

Hasil audit: `{ callbackAccepted: true }`. Expected sesudah perbaikan: pesan tersebut diabaikan.

## 5. Kontrol yang perlu dipertahankan

- CRM/command gate memisahkan authentication, membership, capability, scope, approval, dan transaction boundary. Tes nyata membuktikan sejumlah penolakan dan replay/recovery yang benar.
- Password scrypt dipindahkan ke blocking worker dengan batas concurrency; jangan menggantinya dengan hashing sinkron pada async request worker.
- Session menggunakan cookie HttpOnly, kebijakan Secure pada trusted TLS hop, CSRF per sesi, dan logout yang mencabut session server.
- Banyak jalur SQL menggunakan bind parameter dan transaksi tenant yang memeriksa konteks sebelum query. Startup menolak role/policy/checksum yang salah pada ledger yang terdaftar.
- Template memiliki body limit, origin allowlist, admission control, opaque close ticket, error provider generik, expiry/cleanup, exact guest binding, dan revision protocol. Pertahankan kontrak tersebut saat menghubungkan fitur baru.
- Deployment reference memiliki non-root user, filesystem read-only, capability drop, pembatasan systemd dan blok `/metrics` pada Caddy. Pertahankan setelah path/artifact diperbaiki.

## 6. Bottleneck dan rencana pengukurannya

Ini observasi kompleksitas/struktur, bukan klaim throughput atau p95 yang sudah diukur. Benchmark historis tidak dipakai sebagai ukuran working tree sekarang.

| ID | Lokasi dan pola | Konsekuensi | Perbaikan dan bukti yang diperlukan |
| --- | --- | --- | --- |
| P01 | `crates/orbz/src/lib.rs:62`: scan seluruh orb semua tenant, clone seluruh node sebelum render; `crates/orbz/src/lib.rs:34` dan `crates/cockpit/src/lib.rs:26`: global `std::sync::RwLock`, Vec/Map tanpa quota/retention | Query bertambah sebanding total memory; L0 tetap dapat clone detail L2; write contention dapat memblokir async worker; RAM terus tumbuh | Repository tenant-indexed, projection sesuai tier, pagination/token budget, quota/retention. Ukur 1k/10k/100k memory, payload besar, banyak tenant dan concurrent read/write; ukur allocation/lock wait/RSS/p95 |
| P02 | `crates/host/src/crm.rs:1374`–`:1437`: activities/followups/opportunities/history memakai fetch_all tanpa LIMIT | Contact lama menghasilkan response, query, serialization dan render yang terus membesar; beberapa query serial menahan pool lebih lama | Pisahkan summary dari riwayat; keyset pagination pada setiap koleksi; index sesuai tenant/contact/order. EXPLAIN ANALYZE pada fixture representatif serta ukur response bytes, DB duration, pool wait |
| P03 | `crates/domain/src/erp/general_ledger.rs:271`: setiap akun menelusuri semua entry/line; `crates/cockpit/src/mission.rs:100`: prerequisite mencari step berulang | Trial balance O(accounts × lines); resolusi DAG dapat mendekati kuadratik/kubik pada struktur padat | Aggregate sekali dengan map/account ID dan checked totals; index step/dependency dan validasi DAG sekali. Benchmark output ekuivalen pada ukuran bertingkat sebelum memilih optimasi |
| P04 | `crates/ops/src/backup.rs:222`, `:262`: std::fs::read seluruh archive untuk hash | Peak RAM naik sebanding ukuran database dump; restore verify membaca archive besar kembali | Hash/enkripsi streaming dengan buffer terbatas; ukur RSS pada archive 100MB/1GB/sesuai skala pengguna, disk throughput, dan waktu restore |

Pool runtime ditetapkan maksimum 16 koneksi di `crates/slice/src/bin/archava_server.rs:252`. Angka itu sendiri bukan bug; jangan menaikkan tanpa mengukur kapasitas PostgreSQL, transaction duration dan queue wait. Metrics saat ini memberi cumulative latency/count, belum distribusi p95/p99; tambahkan histogram dengan label route terkontrol dan metrik pool/outbox/worker sebelum menetapkan target performa.

Target awal untuk pengukuran harus disepakati menurut kebutuhan pengguna dan environment. Kandidat dataset: tenant kecil/menengah/besar, contact dengan riwayat panjang, memory detail besar, dan campuran read/write. Hindari menetapkan angka kapasitas produksi dari tes unit subdetik.

## 7. Arsitektur yang disarankan untuk tahap perbaikan

Pertahankan **modular monolith Rust + PostgreSQL**. Batas module diperkuat sebelum memecah service. Template voice tetap standalone sesuai `Archava Template/AGENTS.md`; jangan membuat import ke parent repository untuk menyelesaikan integrasi.

```mermaid
flowchart TD
    UI[Owner UI / Customer Embed / Voice Template] --> HTTP[HTTP boundary dan typed DTO]
    HTTP --> Actor[Session dan membership menjadi VerifiedActor]
    Actor --> App[Application services per domain]
    App --> Domain[Pure domain rules dan checked money]
    App --> Repo[Repository ports]
    Repo --> PG[PostgreSQL TenantTransaction dan forced RLS]
    App --> Outbox[Transactional outbox dan operation receipt]
    Outbox --> Worker[Worker identity, assignment, lease dan retry]
    Worker --> Provider[Provider adapters]
    Worker --> App
    PG --> Read[Read models dan status operation]
    Read --> UI
```

| Batas | Tanggung jawab | Aturan implementasi |
| --- | --- | --- |
| HTTP boundary | Parse/size/schema validation, request identity, response contract | Tidak membuat actor dari authority body; semua route ditandai public/read/write/webhook/worker |
| VerifiedActor | Principal, tenant membership, role/capability/scope/department terverifikasi | Terbentuk di server; deny default; revocation diperiksa pada operasi dan recovery |
| Application service | Urutan authorize → validate → transaction → effect/receipt | Entry yang sama dipakai UI, assistant, MCP dan campaign; tidak ada jalur bypass untuk AI |
| Domain | Invariants, amount/date/state rules, deterministic decisions | Pure functions, typed IDs/money, checked arithmetic; error bukan panic/wrap |
| Adapters | SQL, RLS, persistence, provider transport | Query menerima konteks tenant terverifikasi; DTO eksternal tidak langsung menjadi SQL/entity |
| Orchestration | Mission state, checkpoint, retry, outbox | Durable transaction, worker assignment, fencing, idempotency, rekonsiliasi unknown outcome |
| Presentation | Intent, confirmation, status, receipt/recovery | Demo terlabel, capability unavailable jelas; “selesai” hanya dari hasil otoritatif |
| Release/ops | Manifest migrasi, artifact identity, backup, restore | Satu katalog sumber kebenaran; tool ops tidak membocorkan dependency ke domain/serving path |

`router.rs` (~3.014 baris) dan `crm.rs` (~2.612 baris) adalah kandidat pemisahan menurut area route/application service; ukuran file bukan alasan rewrite sendiri. Ekstrak boundary bersama yang menghilangkan duplikasi otorisasi dan menjaga satu kontrak status/receipt. Hindari framework internal besar yang justru menyembunyikan alur izin.

MCP perlu membawa `VerifiedActor` lengkap, bukan hanya tier. Tenant dan clock tidak berasal dari arguments/angka konstan. Provider traits yang memang hanya seam tetap berguna, tetapi status “tersambung provider” membutuhkan transport dan bukti integration yang nyata.

## 8. Alur pengguna yang ditargetkan

| Persona/alur | Kondisi saat audit | Alur yang disarankan | Acceptance utama |
| --- | --- | --- | --- |
| Owner pertama masuk | Tenant harus diketahui, fixture copy, beberapa fitur simulasi | Sign-in → pilih bisnis dari membership → pilih kebutuhan → satu tugas pertama → hasil dapat dibuka ulang | Pengguna baru tidak perlu nama schema/tenant ID; tidak ada demo data yang disangka bisnisnya |
| Operasional harian | Feed/widget mengaku aksi selesai | Dashboard ringkas → review target/biaya → confirm jika perlu → status operasi → hasil/error dengan langkah lanjut | WA/PO/invoice/print sukses hanya setelah receipt/effect; unknown punya tombol cek status |
| Visitor voice | Percakapan tersedia, tool tertentu mengklaim modal tak ada | Izin mikrofon → status koneksi → percakapan → permintaan aksi dengan consent → UI ACK/result | Agent tahu apakah tampilan benar-benar muncul; bisa melanjutkan tanpa mikrofon/provider |
| Customer checkout | Iframe salah tujuan atau diblokir CSP | Order session server → ringkasan jumlah → metode bayar → pending → verified paid → receipt | Reload/retry tidak menggandakan order; callback tidak menjadi satu-satunya bukti pembayaran |
| Response terputus | Marker satu slot dan error teknis | Tampilkan “hasil belum diketahui” → pulihkan dengan ID awal → hasil final atau retry aman | Dua tab/domain tidak saling menghapus marker; tidak membuat intent baru otomatis |
| Sesi expired | Local state mengarah kembali ke refusal | Re-auth jelas → kembali ke tugas → cek outcome pending sebelum mengulang | Akses tetap ditolak saat expired, pengguna tidak terjebak atau kehilangan recovery |

Rendered review berikutnya harus mencakup keyboard/focus, dialog semantics, Escape/close, mobile viewport, reduced motion, contrast, status live region, microphone denied, server offline, expired session, dan empty state. SDK saat ini memiliki panel 420px dan modal tanpa focus management yang perlu diverifikasi pada browser; ini checklist tindak lanjut, bukan hasil aksesibilitas yang telah lulus.

**Hipotesis H01:** CSP template `script-src 'self'` di `Archava Template/server/app.mjs:41` dapat memblokir WASM renderer Spatius (`src/lib/spatius.ts`). Verifikasi build yang dilayani Node dengan console CSP; Vite dev saja tidak cukup. Jika terbukti, berikan izin WASM sempit yang dibutuhkan tanpa membuka generic unsafe-eval.

## 9. Kandidat pembersihan — hanya ditandai

Tidak ada file berikut yang dihapus. Perubahan lokal yang sudah ada sebelum audit dipertahankan.

| Kandidat | Tindakan yang disarankan | Syarat sebelum menghapus/memindah |
| --- | --- | --- |
| `static/v1/archava.js` vs `static/archava.js` | Tentukan satu sumber SDK/versioning; keduanya berbeda, 135 vs 546 baris, event completion berbeda | Telusuri consumer luar repo/release; route aktif sekarang memakai `static/archava.js`; sediakan kontrak migrasi |
| Tools yang merujuk `root/rust`, `apps/*`, `packages/*`, pnpm root | Perbaiki tool yang masih menjadi gate; archive yang sepenuhnya historis | Petakan caller CI/docs/consumer dan jalankan pengganti sebelum membuang proof tooling |
| Hardcoded widget/ERP sample metrics dan fake success | Pindahkan ke demo/example yang jelas dan terpisah dari operasional | Consumer produksi memakai real read model; demo tidak disajikan sebagai data tenant |
| Tool agent preview/Privy/migration belum tersambung | Tandai unavailable/nonaktif sampai consumer dan backend nyata tersedia | Prompt, verified knowledge, DTO, UI dan test diperbarui bersama |
| Static CSS/assets yang belum dirujuk | Sambungkan yang diperlukan, lalu buang asset yang terbukti tidak dipakai | Render review dan pencarian runtime/build/consumer; jangan menghapus font-license files |
| Dokumen audit/history/delegation bertumpuk | Tetapkan dokumen aktif; archive/index historis dengan tanggal dan scope | Pertahankan evidence dan keputusan migrasi; jangan menghapus hanya karena audit baru tersedia |
| Komentar/source docs yang masih menyebut path/versi/behavior lama | Perbarui bersama refactor terkait | Cocokkan dengan source dan tes saat ini, bukan search-replace buta |
| `target/`, `tsconfig.tsbuildinfo`, cache bytecode, artifact lokal | Tetap ignored; kandidat pembersihan workspace saat dibutuhkan | Pastikan bukan source/release evidence yang harus disimpan; jangan menjalankan clean massal pada working tree pengguna |
| Examples konsumen TS/Rust dan native bindings | Klasifikasikan supported/compatibility/historical | Buktikan pemakai API dan strategi distribusi terlebih dahulu; ketiadaan root pnpm bukan bukti seluruh contoh tak berguna |

Pertahankan `Cargo.lock`, migrasi historis, test fixture, license, `.env.example` tanpa secret nyata, dan bukti audit. Instalasi alat atau keberadaan skill tidak menjadi alasan menghapus/menyalakan layanan.

## 10. Rencana remediasi yang dapat dijalankan kemudian

| Tahap | Isi | Bukti selesai |
| --- | --- | --- |
| A — Boundary dan kejujuran hasil | S01–S04, C01–C02; route classification; demo/capability flags | Negative route tests; false success hilang; eval gagal terhadap target yang sengaja rusak |
| B — Release yang reproducible | D01, D02, D04; root paths; native/WASM; template env | Clean checkout CI; artifact/image identity; docs quickstart dijalankan tanpa improvisasi |
| C — State dan data safety | S06–S08, C03–C04, U04 | Restart/multi-instance proof; lease/idempotency; encrypted backup dan isolated restore rehearsal |
| D — Domain correctness | C05–C07 serta review domain serupa | Property/boundary tests; release-mode arithmetic checks; parser fuzzing dengan corpus |
| E — End-to-end UX | U01–U03, U05; SDK/voice integration dan H01 | Browser dari origin integrator, mobile/keyboard/error/recovery paths, provider sandbox yang dipilih |
| F — Performa dan cleanup | P01–P04; D03; tabel kandidat cleanup | Dataset representatif, p95/p99/RSS/DB evidence, consumer checks, perubahan kecil yang dapat direview |

Rencana ini tidak mengotorisasi implementasi dalam tahap audit ini. Temuan hanya ditutup setelah perubahan dan regression criteria terkait terbukti pada sumber baru; status “OPEN” tidak diubah berdasarkan rencana saja.

## 11. Bukti tambahan yang masih dibutuhkan

- Pengujian HTTP dua tenant/dua sesi untuk route Orbz/Cockpit baru, worker identity, membership revocation dan entity scope.
- Suite ERP lengkap, cold independent consumer, WASM production build/hydration, release identity/SBOM dan container startup setelah path dipulihkan.
- Browser QA alur onboarding, SDK cross-origin, expired session, dua tab pending, styling, aksesibilitas dan CSP renderer.
- Provider sandbox yang dipilih untuk payment/Privy/voice, termasuk signature/replay/timeout/unknown outcome; tidak mengandalkan provider success yang disimulasikan.
- Review lebih dalam tenancy, retention dan field-level data minimization untuk medical/financial memory; validasi compliance/tax oleh spesialis domain bila klaim tersebut akan dipublikasikan.
- Load benchmark dan profiling pada data serta deployment representatif; dependency audit Python/entire consumer graph dan pemindaian secret/history yang lebih lengkap.

Tidak ada server aplikasi, browser, container, proxy, ADB, atau provider session yang dimulai oleh audit. PostgreSQL sementara yang dipakai tes ditutup oleh fixture; proses sccache yang sudah ada sebelum audit dipertahankan. Satu file laporan ini merupakan perubahan repository yang dimaksudkan dari pekerjaan audit.
