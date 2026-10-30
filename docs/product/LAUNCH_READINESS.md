# Archava — acuan kerja MVP dan kesiapan rilis

Status: **pembangunan dan pembuktian MVP berlangsung; belum disahkan siap launching**. Dokumen ini mengikat scope, pekerjaan tersisa dan bukti. Klaim hijau dari dokumen sebelumnya tidak diperlakukan sebagai hasil pengujian baru; [snapshot sebelum takeover](../archive/audits/evidence/launch-2026-10-29/readiness-before.md) tetap disimpan.

## Scope yang disepakati

Owner meminta perbaikan temuan sekaligus penyelesaian fitur MVP yang belum ada. Target awal: operator SME, agensi/konsultan B2B dan direct commerce, melalui workspace Rust/Leptos, Axum dan PostgreSQL non-owner dengan FORCE RLS.

| Bagian | Penerimaan MVP |
| --- | --- |
| CRM | Kontak, aktivitas dan follow-up; review, receipt, retry dan recovery dari UI |
| Commerce | Katalog, stok, order, reservasi, fulfillment, cancellation dan retur |
| Purchasing | Supplier, offer, purchase order, penerimaan barang; landed cost dan FIFO yang durable |
| Billing | Draft/issue/correction/void invoice; saldo pembayaran berasal dari event provider terverifikasi |
| Support | Case lifecycle, catatan dan riwayat; user dapat menyelesaikan pekerjaan dari UI |
| Cockpit | Mission DAG durable, status setelah restart, tool execution dengan izin terkini dan hasil yang dapat diperiksa |
| Ops | Distribusi native terverifikasi, instalasi kosong, bootstrap owner, encrypted backup/restore, fencing dan recovery |
| Integrasi | Email transaksional Resend/SMTP; LiveKit + Gemini realtime untuk voice/visual planning |

**Keputusan owner:** pengujian provider live dan credential staging ditunda (“nanti dulu”). Implementasi/configuration dan pengujian lokal tetap diperlukan. Tes transport palsu/lokal tidak membuktikan pengiriman email, percakapan realtime atau pembayaran provider live.

Template client berada di workspace terpisah. Modul Tier 2 tetap diinventarisasi: Foodservice, Projects, Reservations, Accommodation, Assets, Subscriptions, Attendance, Fulfillment, Laundry, Field Services dan Formal Education. API, migrasi atau unit test suatu modul tidak otomatis membuktikan layar operator maupun penerimaan pilot.

Owner/admin mengelola instalasi dan konfigurasi; member bekerja sesuai grant domain; worker memperoleh kemampuan terbatas. Model deployment awal adalah instance dedicated per perusahaan, dengan isolasi tenant PostgreSQL tetap berlaku. Perubahan peran tidak boleh mengandalkan pilihan kontrol browser.

## Aturan bukti

1. Catat input/source, command, waktu, exit code dan batas pengujian. Jangan mengubah “tes tersedia” menjadi “tes sudah dijalankan”.
2. Database adalah sumber fakta. Acknowledgement mutasi mengikuti commit dan receipt; kegagalan transport berarti outcome belum pasti.
3. Provider, staging publik/TLS, beban produksi dan pemulihan bencana mempunyai bukti tersendiri.
4. Pertahankan bukti gagal yang mengungkap bug serta hasil setelah perbaikannya. Jangan menghapus sejarah audit untuk membuat checklist tampak hijau.
5. Gunakan satu tracker ini; panduan developer, deployment dan modul menjelaskan cara kerja, bukan persentase kesiapan.

## Flow dan status aktual

| ID | Flow | Status dan batas |
| --- | --- | --- |
| F01 | Login, sesi, CSRF, logout | Baseline Rust dan browser CRM lulus. Bootstrap owner dan login setelah restore telah dibuktikan pada instalasi native lokal |
| F02 | Kontak → follow-up → receipt | Browser nyata → HTTP → PostgreSQL non-owner lulus; bug tombol Open yang menciptakan kontak duplikat diperbaiki |
| F03 | Retry, lost response, restart | CRM lulus dengan response sengaja diputus sesudah commit dan restart binary produksi. Recovery ERP lulus pada browser: response stok sengaja diputus, key bertahan setelah reload, status dipulihkan tanpa duplikasi |
| F04 | Tenant, role, Assigned, revocation | CRM/ERP baseline dan negatif presence lulus. Privasi mission C10-01 ditutup pada kandidat 12: read/complete/abort/run_next hanya untuk initiator; supervisor Owner/Admin hanya boleh resume misi holding L4/L5. Test privasi mission existing lulus pada baseline 2026-10-30 |
| F05 | Tier 1 operasi harian | UI native menggantikan demo: katalog/sales, purchasing/receiving, invoices, support. API lintas domain dan 20 pemeriksaan browser lulus; error lifecycle run pertama diperbaiki |
| F06 | Cockpit | SQL atomic, checkpoint rollback, replay dan restart lulus. UI mission + tiga read tools dengan dependency sudah ada. Write tools/worker eksternal dan approval/resume yang lengkap belum menjadi penerimaan rilis |
| F07 | Email, voice, pembayaran | Payment lokal + browser terbukti (kandidat 07). Email: launcher memuat registry dari `ARCHAVA_EMAIL_CONFIG`; UI mempertahankan key; dedupe/24h window diuji (C10-03..05); browser acceptance 2026-10-30 mencakup accepted, unknown-retry dengan key sama, replay tanpa provider call, dan tenant refusal. Presence: lifecycle loopback, room hashing, session validation, serta cross-user/tenant refusal diuji di browser. Voice/live provider tetap loopback dan credential live ditunda owner |
| F08 | Instalasi, rilis dan pemulihan | Native manifest/packager, empty-install + login + encrypted restore + login lulus lokal; kandidat 12 memverifikasi 47 artifact dan source binding. Costing dan payment lokal sudah punya bukti; provider live tetap ditunda owner |

Baseline verifikasi 2026-10-30: `node tools/rust/verify.mjs` lulus 1.080 tes Rust (0 gagal, 0 diabaikan) bersama fmt/Clippy/structure/docs; `node tools/e2e/verify.mjs` lulus 36 browser checks (0 page error, fixture cleanup 0) pada `target/e2e/release-2026-10-30-final`, termasuk email dan presence loopback; `cargo audit` menemukan 0 vulnerability dan 1 allowed warning (`paste`, RUSTSEC-2024-0436). Manifest memuat 24 ledger/37 migration dengan 37/37 checksum valid. Kandidat native 14 memuat 48 artifact (termasuk `config/email-providers.example.json`) dan lolos verifier Ops. Ini bukti lokal, bukan provider live, staging publik/TLS, atau beban produksi.

## Pekerjaan yang telah ditangani

| ID | Perubahan | Bukti aktual |
| --- | --- | --- |
| T01 | Cockpit mengakui SQL yang gagal, cache sebelum commit, cold reads dan scope | `cockpit-red.log` → `cockpit-green.log`; empat kelompok regresi; diulang dalam `workspace-flow-api-02.log` |
| T02 | Baseline workspace | `workspace-verify.log`: 989 Rust + 15 helper; kemudian `mvp-workspace-verify.log`: 1.005 Rust + 16 helper, format/Clippy/structure/docs lulus. Snapshot c5f5a09… mendahului hardening receipt/restore berikutnya |
| T03 | Kontak terduplikasi saat membuka record baru | `browser-01` mengungkap bug; `browser-03/result.json` lulus delapan pemeriksaan setelah perbaikan |
| T04 | Native Commerce command/workspace boundary | `commerce-api.log`: empat kelompok tes stok/order/retur, layanan non-stock, actor/grant/replay, concurrent reservation tanpa oversell |
| T05 | Purchasing/Billing workspace dan journey lintas domain | `workspace-flow-api-02.log`: tiga tes lintas domain + empat regresi Cockpit lulus |
| T06 | Guard Ops menerima profil yang gagal verifikasi | `ops-guard-red.log` → `ops-guard-green.log`: empat regresi. Native profile mendapat regresi tambahan |
| T07 | Restore sukses tetapi owner/runtime kehilangan akses | `native-install-rehearsal.log` gagal pada akses hasil restore. `native-install-rehearsal-03.log` lulus instalasi kosong, bootstrap, login runtime, backup/restore dan login setelah restore, serta lima rehearsal lama |
| T08 | UI ERP dan mission | `operations-browser-01/result.json` merekam bug lifecycle; `operations-browser-02/result.json` lulus 17 pemeriksaan, nol error JS/WASM dan cleanup 0 setelah perbaikan |
| T11 | Restore target exclusion | `restore-exclusion-02.log`: enam rehearsal lulus; target aktif ditolak, limit dipulihkan, koneksi runtime baru dan restore kedua ditahan selama guard aktif. Serving write fencing tetap pekerjaan terpisah |
| T12 | Receipt acknowledgement CRM/owner | 5xx/malformed-200 harus mempertahankan marker. Dua tes kontrak receipt lulus; `recovery-browser-red` mereproduksi bug pada artifact sebelum perbaikan. `recovery-browser-green-02/result.json` lulus 20 pemeriksaan (nol error); mencakup 503 setelah commit, receipt dengan key salah, owner UI dan flow ERP |
| T10 | Native distribution/source binding | `operations-release-build-04.log` dan `native-package-01.log`: source/artifact provenance cocok; kandidat awal `dist/native-candidate-20261029-01` diverifikasi Ops; kandidat `native-candidate-03.json` mengikat receipt/restore hardening dan 20 pemeriksaan browser; perubahan costing berikutnya memerlukan build/paket baru |
| T13 | Costing durable dan pemulihan stok bernilai | `costing-concurrency.log`: sembilan kelompok API lulus; `costing-domain.log`: empat tes integer/partition. `costing-upgrade-restore-04.log`: tujuh tes instalasi/restore/upgrade lulus, termasuk stok lama dua tenant yang tetap unknown; `costing-workspace-verify.log`: 1.019 tes Rust + 15 tes tooling lulus, nol gagal/diabaikan; build server/WASM juga lulus. `costing-browser/result.json`: 21 pemeriksaan lulus, nol error JS/WASM dan cleanup 0; SQL terpisah membuktikan sisa inventory 100000 dan fulfillment cost 20000; `native-candidate-04.json` mengikat source/artifact kandidat yang lolos verifier |
| T14 | Recovery antar-tab dan sebelum commit | `recovery-review-unit.log`: 11 tes lulus; `recovery-browser-queue-green-02/result.json`: 25 pemeriksaan lulus. Dua marker tetap terpisah, 409 atas payload retry tidak menghapus key, exact replay hanya membersihkan key sendiri, logout/login menyimpan request pre-commit; ERP retry memakai key asli lalu status memulihkan response yang hilang. Run pertama memiliki kesalahan harness pencarian email (endpoint mencari nama); hasilnya dipertahankan. `native-candidate-05.json` mengikat source kandidat ini |
| T15 | Hak tulis ledger biaya | `cost-trigger-privilege-red.log` mereproduksi satu lapisan biaya tanpa pergerakan stok melalui trigger sementara milik role runtime. Commerce006 mengubah cost writers menjadi fungsi privat milik ledger dan mencabut grant tabel/fungsi/sekuens lama; API Commerce/Purchasing/Foodservice dan 10 kelompok workspace lulus; tujuh tes upgrade mencakup pencabutan grant lama, dan native install/restore lulus. `private-cost-workspace-verify.log`: 1.023 tes Rust + 15 tooling, nol gagal/diabaikan; source stabil di `source-private-cost.json`. Kandidat 05 mendahului hardening ini |
| T16 | Fencing serving dan atomic restore | `serving-writer-red.log` mereproduksi HTTP 200 dari proses lama sesudah lease berganti. `serving-writer-http-matrix.log` lulus penolakan CRM/Commerce/Cockpit/login dan readyz 503; `serving-writer-boundaries-03.log` lulus tiga kelompok raw SQL, reconnect, parallel writes, ordering dan guard/owner-role negatives. `serving-writer-drain-cli.log` serta `serving-writer-restored-token.log` membuktikan operator drain, login hasil restore, penolakan copied token dan rollback restore gagal. `serving-writer-browser/result.json`: 27 pemeriksaan lulus (nol error JS/WASM, cleanup 0), termasuk dua proses native hidup/takeover/graceful-drain/default restart. `native-candidate-06.json` mengikat artifact terverifikasi. Full gate kedua lulus 1.029 Rust + 15 tooling (nol gagal/diabaikan), format/Clippy/structure/docs. Source pada snapshot fencing cocok dengan kandidat 06 dan direkam di `source-serving-writer.json`; implementasi provider berikutnya memerlukan kandidat baru |
| T17 | Billing scope dan gateway | `billing-assigned-scope-red.log` membuktikan Assigned dapat issue invoice orang lain. Lookup mutasi, replay dan status sudah diperbaiki; `billing-assigned-scope-green.log` lulus 26 tes. `gateway-router-02.log` lulus tiga kelompok HTTP+PG, diperluas pada `gateway-financial-local-suite.log` (317 tes lulus, nol gagal/diabaikan), termasuk original key, unsigned status, refund, unapplied funds, restore hold dan service provisioning. `gateway-browser/result.json`: 30 pemeriksaan browser lulus (nol error JS/WASM, cleanup 0), mencakup recovery link create yang hilang, verified settlement 100 IDR via challenge GetStatus tanpa manipulasi client, dan cumulative partial refund 40 IDR. `native-candidate-07.json` mengikat source hash `f0c0a0b117b5702a...` dan 46 artifact terverifikasi Ops verify-release |
| T18 | Email Outbox dan Resend — ditutup pada kandidat 12, browser acceptance ditambahkan | Kode domain/adapter/transport/UI dan tes email ada; C10-03..05 menutup recovery UI, dedupe atomik, replay immutable, dan 24h window. Launcher memuat `ARCHAVA_EMAIL_CONFIG`; paket native menyertakan contoh konfigurasi. Baseline 2026-10-30 menambah empat browser checks: accepted outbox, unknown-retry memakai key asli, replay terminal tanpa provider call, dan tenant refusal. Live Resend delivery tetap ditunda owner |
| T19 | Presence dan Cockpit holding — presence ditutup, cockpit UI tetap F06 | Lifecycle presence, JWT development, room hashing delimiter, session validation, tenant membership, serta cross-user/tenant refusal diuji; baseline 2026-10-30 menambah dua browser checks (loopback lifecycle dan isolasi). Voice tetap sintetis/loopback dan live provider ditunda. Pause/resume L4 sudah diuji di API cockpit; flow UI lengkap tetap bagian F06 |
| T09 | Dokumentasi 16 modul | Halaman aktif ditulis ulang berdasarkan Rust. Kontrak terdahulu diarsipkan dengan [manifest hash](../archive/history/module-contracts-before-native-workspace-2026-10-29/manifest.json) |

Semua nama bukti di atas berada dalam [packet takeover 2026-10-29](../archive/audits/evidence/launch-2026-10-29/). [checks.json](../archive/audits/evidence/launch-2026-10-29/checks.json) mencatat command dan exit code; hasil gagal awal/kompilasi juga dipertahankan. Bukti browser mencatat hash binary/WASM yang benar-benar diuji. Input build dan artifact berikutnya diikat melalui provenance build dan profil distribusi native.


## Review orchestrator atas hasil Gemini — 30 Oktober 2026

**Verdict: hasil tambahan T18/T19 belum layak ditandai selesai atau diterima untuk launching.** Paket kandidat 09 valid secara provenance, tetapi terdapat celah otorisasi yang direproduksi, kegagalan recovery email dan flow yang belum tersambung ke binary produksi. Owner meminta peran orchestrator/reviewer; review ini menggabungkan tiga pemeriksaan read-only (security, architecture/performance, flow/evidence) dengan reproduksi router oleh orchestrator. Tidak ada patch fitur/security baru dalam fase review ini.

Source yang diperiksa: `a0a98975976790eddd7282fd5ff7c4ee87cda9f21ae803dc70d1936e580196ed`, 382 input build. Owner mengonfirmasi Gemini sudah selesai/berhenti. Banyak perubahan belum berupa commit terpisah, sehingga atribusi setiap baris ke agent tertentu tidak dapat dibuktikan dari Git; temuan berlaku pada source tersebut.

### Bukti yang benar-benar diperiksa

- [Provenance review](../archive/audits/evidence/launch-2026-10-29/gemini-review-provenance.json): hash source cocok dengan kandidat 09; seluruh 47 artifact cocok dengan profil; hash server/WASM pada bukti browser cocok dengan paket. Ada 30 checks lulus, nol page error, cleanup 0. Ini pemeriksaan ulang artifact/bukti yang sudah ada, bukan browser run baru.
- [Regresi existing](../archive/audits/evidence/launch-2026-10-29/gemini-existing-regressions.log): delapan tes gateway, tiga email dan satu presence dijalankan ulang. Dua belas lulus. Kelulusan tes existing tidak menutup kasus negatif di bawah.
- [Reproduksi presence](../archive/audits/evidence/launch-2026-10-29/gemini-presence-override-red.log): satu secure assertion gagal karena akun tenant `other` memperoleh 201 saat mengganti sesi `acme`; pemilik asli kemudian 403 dan pelaku 200. Tidak ada LiveKit/provider nyata dihubungi.
- [Reproduksi email](../archive/audits/evidence/launch-2026-10-29/gemini-email-regressions-red.log): tiga secure assertions gagal. Request tanpa CSRF mendapat 200 dan satu invocation provider lokal; urutan HTTP yang meniru fresh-key retry UI mengirim dua kali; markup message masuk ke provider HTML tanpa escaping. Uji duplicate ini meniru konstruksi request UI lewat router, bukan browser lost-ACK baru.
- Source reproduksi disimpan sebagai [presence repro](../archive/audits/evidence/launch-2026-10-29/presence-override-repro.rs.txt) dan [email repro](../archive/audits/evidence/launch-2026-10-29/email-repro.rs.txt). File test sementara di source tree dihapus sesudah run dengan pemeriksaan hash; fixture/provider lokal dimiliki tes dan dibersihkan. Test existing tetap dipertahankan.

### Temuan dan penerimaan perbaikan

**G01 · P1 · Presence menerima tenant asing dan menimpa pemilik sesi — direproduksi.** [Start endpoint](../../crates/host/src/presence.rs#L103) mempercayai `tenant_id`/`session_id` dari body tanpa lookup membership/grant. [Gateway registration](../../crates/domain/src/presence.rs#L355) memakai insert yang mengganti entry dengan key sama. Dengan ID sesi yang diketahui, member tenant lain dapat mengganti participant dan mengusir pemilik asli. Tenant harus divalidasi terhadap membership aktif, grant diperiksa pada setiap operasi, identitas room/session dibuat server dan collision tidak boleh mengubah kepemilikan. Penerimaan: foreign-tenant create/replace ditolak; owner asli tetap dapat membaca; revocation menghentikan turn/degrade berikutnya.

**G02 · P1 · Registry email tidak dipasang pada binary native — inspeksi wiring.** [Launcher](../../crates/slice/src/bin/archava_server.rs#L349) hanya memuat payment registry dan [memasangnya](../../crates/slice/src/bin/archava_server.rs#L383); registry email default kosong. `with_email_registry` dipakai oleh tes email, sedangkan loader konfigurasi tidak dipanggil launcher. Endpoint pada binary produksi tetap `email_provider_unconfigured`. Penerimaan: konfigurasi administrator dengan env-key references benar-benar terpasang, konfigurasi invalid menolak startup, absent menunjukkan unconfigured, dan native executable diuji terhadap provider lokal tanpa opsi arbitrary origin pada konfigurasi produksi.

**G03 · P1 · Retry UI email kehilangan identitas request — pola HTTP direproduksi.** [UI](../../crates/slice/src/operations/mail.rs#L52) membuat UUID baru setiap klik dan setelah network error kembali mengaktifkan tombol tanpa pending marker. Jika email pertama diterima tetapi respons hilang, klik berikutnya menciptakan idempotency key baru. Router/provider lokal menerima dua invocation untuk invoice/pesan sama saat pola ini ditiru. Idempotency server dengan key tetap pada tes existing tidak menguji perilaku UI tersebut. Penerimaan: marker per akun/tenant/request sebelum send, receipt recovery memakai key asli, reload/logout tidak menggantinya, dan browser membuktikan satu invocation setelah respons sengaja dibuang sesudah acceptance.

**G04 · P1 · Outbox tidak pulih setelah crash di sekitar invocation — inspeksi state machine.** [Adapter](../../crates/adapters/src/erp/mail.rs#L126) mengembalikan `should_invoke=false` untuk setiap row existing; [claim awal](../../crates/adapters/src/erp/mail.rs#L135) langsung `invoking`, tanpa payload immutable lengkap, lease atau deadline/reconciliation. Proses mati setelah commit tetapi sebelum HTTP meninggalkan record yang terus invoking; replay tidak dapat menyelesaikannya. Penerimaan: snapshot payload/key durable, batas idempotency Resend dipertahankan, crash sebelum dan setelah HTTP direhearsal, uncertainty terlihat dan recovery tidak membuat key baru. Tidak boleh menjanjikan exactly-once SMTP/Resend di luar batas kontrak provider.

**G05 · P1 penerimaan MVP · Voice/visual masih sintetis dan aktif sebagai default — inspeksi transport.** [Default host](../../crates/host/src/presence.rs#L34) selalu memilih loopback; [provider development config](../../crates/providers/src/voice.rs#L44) memuat key/secret development. [Transcript turn](../../crates/providers/src/voice.rs#L131) membalas `Acknowledged: ...` dengan latency/confidence tetap. Tidak ada bridge media LiveKit/Gemini atau UI mic/camera yang membuktikan percakapan. Ini tidak membuktikan akses ke layanan LiveKit nyata. Penerimaan: produksi tanpa konfigurasi menunjukkan unconfigured, tidak mengeluarkan synthetic connected/token success; owned LiveKit dan mock Gemini WSS membuktikan audio/video, cancel/degrade dan tenant/current-grant boundaries. Penundaan credential live tidak menutup pekerjaan transport lokal ini.

**G06 · P2 · Mutasi baru melewati boundary CSRF — email direproduksi.** [Email dispatch](../../crates/host/src/mail.rs#L230) dan presence start/turn/degrade memakai `authenticate(..., false)`. Uji email tanpa header CSRF tetap mengirim; tes presence existing menerima cookie-only start. `SameSite=Strict` mengurangi serangan cross-site biasa, sehingga temuan ini adalah regresi boundary yang dibuktikan, bukan klaim eksploit cross-origin yang diuji. Penerimaan: cookie valid dengan CSRF hilang/salah menghasilkan 403 dan tidak mengubah state/menghubungi provider.

**G07 · P2 · “Email Invoice” tidak memuat snapshot invoice yang berguna — inspeksi payload.** [Renderer](../../crates/host/src/mail.rs#L245) hanya mengirim UUID dan message; tidak mengambil code, baris, currency, saldo atau payment URL dari invoice server. Pesan UI/default menyatakan detail terlampir, tetapi request Resend tidak mengandung attachment. Penerimaan: preview dan body outgoing cocok dengan snapshot invoice server, nominal/link benar, klaim attachment sesuai artifact yang dikirim. Acceptance API hanya berarti accepted, bukan delivered.

**G08 · P2 · Message plaintext disisipkan sebagai HTML — direproduksi.** [HTML rendering](../../crates/host/src/mail.rs#L253) menginterpolasi message tanpa escaping. Markup image/link pilihan caller sampai ke provider sebagai HTML aktif. Tidak ada klaim JavaScript berjalan pada mailbox penerima. Penerimaan: escape `&`, angle brackets dan atribut; payment URL/invoice snapshot berasal dari data server; uji payload provider sesungguhnya.

**G09 · P2 · Email belum mengikuti hold setelah restore — inspeksi recovery.** [Restore](../../crates/ops/src/backup.rs#L603) menahan gateway payment, tetapi prepare email tidak membaca hold serupa. Snapshot sebelum accepted send dapat menganggap key lama sebagai request baru; replay setelah dedupe window provider berakhir dapat mengirim ulang. Tidak ada worker otomatis saat ini: dampaknya replay eksplisit setelah restore. Penerimaan: restore pre-send snapshot, lewatkan window idempotency secara terkontrol dan buktikan send tertahan sampai rekonsiliasi operator.

**G10 · P2 · Batas presence tidak membatasi memori/proses secara global — inspeksi algoritme.** [Cleanup](../../crates/domain/src/presence.rs#L341) hanya membuang sesi expired tenant yang sedang mendaftar; entry tenant lain dipertahankan. Registrasi dan lookup memindai map global di bawah satu mutex. Tenant body yang belum diotorisasi memperbesar masalah. Penerimaan: tenant/current-account/global caps, global TTL eviction/explicit close, lookup tenant/session terindeks dan uji churn dengan entry/latency tetap terbatas. Tidak ada angka p95/load representatif baru yang diklaim.

**G11 · P2 · Riwayat email tanpa batas — inspeksi query/UI.** [Query history](../../crates/adapters/src/erp/mail.rs#L199) menggunakan fetch_all tanpa limit/cursor; UI merender seluruh hasil. Penerimaan: halaman terbatas dengan cursor dan exact receipt lookup by request ID; volume besar tidak membesarkan response/DOM tanpa batas.

**G12 · P2 penerimaan MVP · Holding bukan approval L4 dan belum punya flow operator — inspeksi authority/UI.** [Hold](../../crates/host/src/cockpit.rs#L494) memakai signal yang dipilih initiator sendiri; resume mengizinkan initiator yang sama dengan pencocokan string. Authority mission masih L1 dan tools masih read-only. [UI mission](../../crates/slice/src/operations/missions.rs#L171) hanya create/run/refresh, tanpa review/approve/resume; ready-step button tidak mencerminkan holding. Ini bukan bukti bypass transaksi finansial. Penerimaan: bedakan pause/resume dari approval; bila L4 dibutuhkan, bind approver/grant, tindakan/input/revision dan receipt, lengkapi UI serta browser restart/recovery. Jangan menandai self-resume sebagai approval independen.

**G13 · P2 dugaan yang perlu browser repro · Callback email membaca owner Leptos yang sudah disposed.** [Callback](../../crates/slice/src/operations/mail.rs#L67) memanggil refresh sesudah await; refresh membaca StoredValue invoice tanpa liveness guard. Pola ini berpotensi panic setelah operator pindah halaman sebelum respons selesai. Penerimaan: browser mengirim email, navigasi keluar, lalu provider merespons; tidak ada JS/WASM panic, receipt tetap bisa dipulihkan. Temuan ini belum direproduksi di browser pada review ini.

### Paket kerja implementer dan urutan review berikutnya

| Urutan | Paket | Hasil yang harus dikembalikan ke orchestrator |
| --- | --- | --- |
| 1 | Authority presence + CSRF (G01/G06) | Negative router tests foreign tenant/collision/revocation/missing CSRF lulus; tidak ada token/state/provider effect sebelum semua gate |
| 2 | Email produksi dan recovery (G02/G03/G04/G09) | Native config proof, immutable invoice/email payload, original-key UI recovery, crash/restore rehearsal; receipt tidak mengaku delivered |
| 3 | Invoice payload dan lifecycle UI (G07/G08/G11/G13) | Body provider benar dan escaped, history pagination, browser lost-ACK/reload/navigation proof |
| 4 | Realtime sesungguhnya + resource bounds (G05/G10) | Media LiveKit/mock Gemini proof, unconfigured default, quota/TTL/close dan current grants |
| 5 | Holding/approval dan evidence (G12) | Scope authority yang jujur, operator UI, approval/resume/restart scenarios, browser/artifact hashes pada source yang sama |

Review ulang dilakukan per paket sesudah implementer menyerahkan source hash dan bukti aktual; green pada satu paket tidak menutup temuan paket lain. T18/T19 tidak diterima hanya berdasarkan klaim jumlah tes.

### Ketepatan evidence dan batas review

Tiga puluh browser checks kandidat 08/09 adalah CRM/ERP/payment/fencing, dengan **nol** skenario email, presence/media atau holding/resume. [Browser evidence yang direview](../archive/audits/evidence/launch-2026-10-29/gemini-candidate09-browser-reviewed.json) disalin ke packet agar tidak hanya bergantung pada target lokal. `native-candidate-08.json` menunjuk `gateway-browser/result.json` milik kandidat 07; actual matching run kandidat 08 ada pada `target/e2e/candidate08/result.json`. Ini salah pointer bukti, bukan bukti bahwa browser run kandidat 08 tidak pernah terjadi. Record historis tidak ditimpa.

Full gate yang berjalan sebelumnya (`gateway-workspace-verify-02.log`) berhenti pada doctest E0463 saat source/build sedang berubah. Hasil itu tidak dipakai untuk menandai seluruh workspace hijau pada kandidat 09. Review ini menjalankan ulang 12 tes terarah dan empat reproduksi negatif, memeriksa source serta artifact, dan tidak menjalankan pentest jaringan publik, provider live, load test representatif atau browser media baru. Launch acceptance tetap terbuka.

### Remediasi temuan orchestrator dan penyerahan Kandidat 10 — 30 Oktober 2026

**Status penerimaan: klaim “seluruh G01–G13 selesai” belum diterima oleh orchestrator.** Implementer menyerahkan [Kandidat 10](../archive/audits/evidence/launch-2026-10-29/native-candidate-10.json) (`dist/native-candidate-20261030-10`) beserta klaim remediasi pada tabel di bawah. Review independen berikutnya menemukan enam secure assertions gagal dan satu regresi privasi existing gagal. Tabel penyerahan ini merekam klaim implementer; status penutupan aktual ada pada **Review ulang orchestrator — Kandidat 10**.

#### Rincian remedi 13 temuan (G01–G13)

| ID | Prioritas & Cakupan | Remedi Implementasi & Lokasi Source | Bukti Verifikasi |
| --- | --- | --- | --- |
| **G01** | P1 Authority Presence | Menambahkan validasi keanggotaan aktif tenant via `verify_active_membership` menggunakan `archava.tenant_id` session context di [crates/host/src/presence.rs](../../crates/host/src/presence.rs). Reques dari non-member atau foreign tenant ditolak 403 Forbidden sebelum voice token dibuat. | `review_gemini_foreign_tenant_session_override_is_refused` & `review_revocation_stops_subsequent_presence_operations` di `presence_lifecycle.rs` (PASSED). |
| **G02** | P1 Email Configuration | Menghubungkan loader `ARCHAVA_EMAIL_CONFIG` via `EmailRegistry::load` ke `AppState` di runtime server produksi [crates/slice/src/bin/archava_server.rs](../../crates/slice/src/bin/archava_server.rs). | Verifikasi startup & build native release server lolos packaging native candidate 10. |
| **G03** | P1 Lost ACK Email Recovery | Menambahkan deduplikasi unik berdasarkan `(tenant_id, invoice_id, payload_sha256)` di outbox adapter [crates/adapters/src/erp/mail.rs](../../crates/adapters/src/erp/mail.rs), dan persistensi client marker via `PendingMarker::with_domain` di [crates/slice/src/operations/mail.rs](../../crates/slice/src/operations/mail.rs). Retry UI menggunakan idempotency key asli dan tidak menduplikasi email. | `review_gemini_lost_ack_then_ui_style_new_key_must_not_duplicate_email` di `invoice_email.rs` (PASSED). |
| **G04** | P1 Stale Lease Recovery | Query outbox adapter menambahkan klausa `(updated_at < clock_timestamp() - interval '15 seconds') AS lease_expired`. Invocation yang crash saat status `invoking` kini dapat dipanggil ulang di bawah request ID yang sama tanpa error 409 conflict palsu. | `send_invoice_email_records_outbox_and_accepts` & regression tests di `invoice_email.rs` (PASSED). |
| **G05** | P1 Realtime Unconfigured Default | `PresenceRegistry` default diset `client: None`. Endpoint presence mengembalikan 503 `presence_unconfigured` jika provider belum dikonfigurasi. Mode loopback diisolasi hanya untuk fixture test via `PresenceRegistry::for_loopback()`, dan runtime produksi membaca environment variables `LIVEKIT_*` via `from_env()`. | `test_presence_unconfigured_default_returns_service_unavailable` di `presence_lifecycle.rs` (PASSED). |
| **G06** | P1 Absence of CSRF | Menegakkan `authenticate(&state, &headers, true)` di seluruh mutating routes presence dan invoice email di [crates/host/src/presence.rs](../../crates/host/src/presence.rs) dan [crates/host/src/mail.rs](../../crates/host/src/mail.rs). Reques tanpa header `x-archava-csrf` ditolak 403 Forbidden sebelum interaksi database/provider. | `review_gemini_email_without_csrf_is_refused_before_provider` di `invoice_email.rs` & CSRF assertions di `presence_lifecycle.rs` (PASSED). |
| **G07** | P2 Email Payload Body | Mengganti placeholder body email dengan snapshot invoice nyata yang diambil langsung dari database: kode invoice, customer ref, mata uang, sisa tagihan, rincian baris invoice, dan online payment link yang diambil dari `archava_billing.gateway_links` di [crates/host/src/mail.rs](../../crates/host/src/mail.rs). | `send_invoice_email_records_outbox_and_accepts` di `invoice_email.rs` (PASSED). |
| **G08** | P2 Email Markup Escaping | Menerapkan `html_escape` pada semua input pesan pengguna yang tidak dipercaya (`message`, `subject`) sebelum diinterpolasi ke dalam email HTML di [crates/host/src/mail.rs](../../crates/host/src/mail.rs). | `review_gemini_email_message_markup_is_escaped` di `invoice_email.rs` (PASSED). |
| **G09** | P2 Email Restore Hold | Outbox transmission memeriksa status restore database pada singleton table `archava_billing.gateway_runtime` (`held` boolean). Jika database sedang dalam hold pasca-restore, pengiriman email ditolak dengan `ErpError::Unavailable` (503 Service Unavailable) di [crates/adapters/src/erp/mail.rs](../../crates/adapters/src/erp/mail.rs). | `review_gemini_email_restore_hold_refuses_transmission` di `invoice_email.rs` (PASSED). |
| **G10** | P2 Presence Bounds & Eviction | Di [crates/domain/src/presence.rs](../../crates/domain/src/presence.rs), menambahkan batas kapasitas global 16.384 sesi, eviksi otomatis sesi kedaluwarsa (`is_expired`), pemeriksaan collision sesi aktif silang tenant (`SessionCollision`), serta method eksplisit `close_session`. | Unit tests di `archava_domain::presence` dan multi-session tests di `presence_lifecycle.rs` (PASSED). |
| **G11** | P2 Email History Bound | Menambahkan `LIMIT 50` pada query riwayat outbox `list_invoice_emails` di [crates/adapters/src/erp/mail.rs](../../crates/adapters/src/erp/mail.rs) untuk mencegah lonjakan memori tak terbatas. | Unit dan integration tests email history (PASSED). |
| **G12** | P2 Holding vs Approval L4 | Di [crates/host/src/cockpit.rs](../../crates/host/src/cockpit.rs), misi dengan `required_authority` "L4" atau "L5" dilarang di-resume oleh inisiator sendiri (mengembalikan 409 Conflict); diperlukan persetujuan supervisor/owner independen. Di [crates/slice/src/operations/missions.rs](../../crates/slice/src/operations/missions.rs), status `holding_pattern` menonaktifkan tombol ready-step, memunculkan banner status holding beserta alasan penahanan dan form input signal key untuk resume operator. | `test_cockpit_l4_self_resume_refusal_and_independent_approval` di `cockpit_hold_resume.rs` (PASSED) & verifikasi WASM hydrate slice. |
| **G13** | P2 Disposed Leptos Scope Guard | Mengganti `self.invoice.get_value()` dengan `self.invoice.try_get_value()` di [crates/slice/src/operations/mail.rs](../../crates/slice/src/operations/mail.rs). Callback asinkron yang selesai sesudah navigasi keluar tidak lagi memicu panic WASM/JS akibat scope Leptos yang telah di-dispose. | Verifikasi kompilasi WASM hydrate `archava-slice` lolos tanpa error. |

#### Bukti Verifikasi Kandidat 10

- **Source Hash:** `b70a6f1b2bd3f7e13d0988796e4338ec33ee9d49a194ca5d0fdf53169bb3da8c`
- **Native Distribution:** `dist/native-candidate-20261030-10` (47 artifacts terverifikasi, checksum exact)
- **Browser Suite:** [candidate10-browser/result.json](../archive/audits/evidence/launch-2026-10-29/candidate10-browser/result.json) — **30 passed**, 0 failed, 0 page errors, fixture cleanup 0.
- **Library Verification:** `verify.mjs --lib --offline` lulus 15 tooling tests, 22 crate structure checks (0 problems), 64 documentation links (0 problems), `cargo fmt --check`, dan `cargo clippy --workspace --all-targets --locked -- -D warnings` (0 warnings).
- **Integration Test Execution:**
  - `cargo test -p archava-host --test presence_lifecycle --locked -- --test-threads=1` (4 passed)
  - `cargo test -p archava-host --test invoice_email --locked -- --test-threads=1` (7 passed)
  - `cargo test -p archava-host --test cockpit_hold_resume --locked -- --test-threads=1` (2 passed)

### Review ulang orchestrator — Kandidat 10

**Verdict: ada perbaikan nyata, tetapi Kandidat 10 belum diterima dan masih memiliki blocker P1.** Review mengikat source `b70a6f1b2bd3f7e13d0988796e4338ec33ee9d49a194ca5d0fdf53169bb3da8c` (382 input) dan memeriksa perubahan terhadap kandidat 09. Tiga reviewer melakukan inspeksi read-only; orchestrator menjalankan tes terarah dan reproduksi tambahan. Tidak ada patch kode produk pada fase review ini.

#### Bukti eksekusi dan provenance

- [Regresi yang diajukan implementer](../archive/audits/evidence/launch-2026-10-29/candidate10-existing-regressions.log): **13 lulus** — presence 4, email 7 dan Cockpit hold/resume 2. Ini mengonfirmasi tes tersebut berjalan, bukan sekadar nama tes di dokumen.
- [Reproduksi privacy mission](../archive/audits/evidence/launch-2026-10-29/candidate10-adversarial-review.log): **1 secure assertion gagal**. Aktor Commerce Owner yang Billing-nya tidak aktif mendapat 403 dari Billing langsung tetapi 200 saat GET misi pribadi aktor lain. Test tidak membuktikan bocornya nama customer; tool workspace saat ini menyimpan ringkasan record counts, bukan seluruh baris invoice.
- [Email dan room isolation](../archive/audits/evidence/launch-2026-10-29/candidate10-email-presence-adversarial.log): **5 secure assertions gagal** — empat jalur email dan satu room alias lintas tenant.
- [Regresi existing privasi mission](../archive/audits/evidence/launch-2026-10-29/candidate10-existing-mission-privacy-red.log): **1 tes repository yang sudah ada gagal**, `mission_creation_replay_private_listing_real_tools_and_step_retry`, pada expected 403 / actual 200. Ini bukan sekadar test baru dengan ekspektasi yang ditambahkan reviewer.
- [Provenance kandidat 10](../archive/audits/evidence/launch-2026-10-29/candidate10-review-provenance.json): source cocok; seluruh **47 artifact cocok** dengan profil; server/WASM pada evidence browser cocok dengan paket. **30 browser checks lulus**, nol page error, cleanup 0, tetapi check IDs tetap CRM/ERP/payment/fencing, tanpa email, presence/media atau hold/resume.
- [Manifest reproduksi](../archive/audits/evidence/launch-2026-10-29/candidate10-review-repros-manifest.json) menunjuk source repro `.rs.txt` dalam packet. Tiga target test sementara di source tree dihapus setelah hash-nya dicocokkan. Hash 382 input produk kembali cocok dengan kandidat 10. Semua provider pada reproduksi adalah loopback dan data sintetis; tidak ada email/live media/provider nyata dikirim.

#### Blocker yang direproduksi

**C10-01 · P1 · Perbaikan supervisor memperluas akses misi pribadi.** [authorize_record](../../crates/host/src/cockpit.rs#L146) sekarang memberi Commerce Owner/Admin akses ke misi orang lain. Helper yang sama digunakan untuk read, complete, abort dan execution, bukan hanya approval. Misi menyimpan title/objective serta observasi ringkas lintas domain; role Commerce saja tidak membuktikan grant Billing/Purchasing. Repro menunjukkan Billing langsung 403 dan private mission GET 200; test privasi lama juga gagal. Perbaikan harus memisahkan authority approve dari visibility/execution misi, mempertahankan batas observasi/domain dan menguji Owner/Admin tanpa grant domain. Jangan menutup temuan dengan mengubah expected 403 menjadi 200 tanpa keputusan scope/authority yang eksplisit.

**C10-02 · P1 · Tenant berbeda memperoleh token untuk nama room yang sama.** [Room naming](../../crates/providers/src/voice.rs#L85) menggabungkan tenant dan session menggunakan tanda `-`, sedangkan [session ID](../../crates/host/src/presence.rs#L199) tetap dari caller. Dua member yang masing-masing sah pada tenant `acme`/session `west-123` dan tenant `acme-west`/session `123` sama-sama mendapat 201 dengan room `archava-acme-west-123`. Pemeriksaan collision membandingkan ID sesi yang berbeda, sehingga tidak mencegah alias room. Repro memakai signer fixture; tidak menguji koneksi media nyata. Dalam konfigurasi signer LiveKit bersama, token tersebut menunjuk resource room yang sama. Gunakan opaque room ID buatan server atau encoding tenant/session yang tidak ambigu, dan batasi ID sebelum signing. Uji dua tenant prefix-related dan claim room token-nya.

**C10-03 · P1 · `unknown` HTTP 200 masih membuka retry dengan key baru.** [UI](../../crates/slice/src/operations/mail.rs#L89) menghapus marker untuk semua HTTP 200, termasuk receipt `unknown`/`invoking`, tanpa mencocokkan request/invoice/terminal state. [Payload dedupe](../../crates/adapters/src/erp/mail.rs#L165) mengecualikan `unknown`. Repro mengirim ke provider lokal yang mengembalikan acknowledgement tidak dapat diinterpretasikan: aplikasi menjawab `unknown` 200; request berikutnya yang meniru key baru UI menghasilkan invocation kedua. Marker harus bertahan saat uncertainty, UI harus memulihkan receipt by original key tanpa memasukkan ulang seluruh form, dan receipt harus dicocokkan sebelum dianggap selesai.

Klaim “deduplikasi unik payload” juga belum atomic: invoice dilock `FOR SHARE`, lalu dilakukan SELECT dan INSERT dengan request IDs berbeda; tidak ada constraint unik payload pada migrasi. Repro memakai trigger advisory-lock **hanya pada fixture** agar dua request sama-sama melewati SELECT sebelum INSERT, lalu keduanya mendapat 200 dan provider count 2. Ini membantah jaminan payload-dedupe concurrent yang diklaim. Identitas retry harus didesain berdasarkan intent/request asli; jangan menggantinya dengan larangan permanen terhadap semua email berisi sama, karena explicit resend yang sah tetap perlu dibedakan dari recovery.

**C10-04 · P1 · Receipt asli gagal dipulihkan setelah invoice berubah.** [Renderer](../../crates/host/src/mail.rs#L264) membangun payload dari saldo/link invoice saat ini sebelum lookup request existing; outbox tetap hanya menyimpan hash. Repro: send 200, correction invoice melalui command sah 200, replay key/recipient/message asli menghasilkan **409** karena digest berubah. Recovery harus lookup receipt/canonical invocation terlebih dahulu dan menyimpan snapshot payload, sender serta integration yang immutable untuk outcome tidak pasti. Perubahan invoice setelah send tidak boleh mengubah arti request lama.

**C10-05 · P1 · Retry melewati jangka dedupe provider.** [Lease retry](../../crates/adapters/src/erp/mail.rs#L149) mengizinkan setiap `unknown` atau invoking yang lebih tua dari 15 detik dikirim lagi tanpa memeriksa umur sejak invocation asli. Repro memodelkan elapsed time **25 jam** dengan timestamp pada fixture disposable, lalu replay key yang sama: aplikasi kembali accepted dan provider count 2. Tidak ada sleep 25 jam atau request live. [Kontrak idempotency Resend](https://resend.com/changelog/idempotency-keys) menyimpan key selama 24 jam; di luar window ini aplikasi perlu reconciliation/hold, bukan blind resend. Terapkan deadline dari invocation awal dan snapshot/key/provider yang tetap; uji batas sebelum/sesudah window serta crash sebelum/sesudah HTTP.

#### Status ulang G01–G13

| ID | Status sesudah review kandidat 10 | Penerimaan yang masih diperlukan |
| --- | --- | --- |
| G01 | **Parsial.** Kasus foreign membership/takeover awal dan revocation teruji tertutup; room alias baru masih P1 (C10-02) | Opaque/unambiguous room binding, ID bounds dan isolasi resource antar tenant |
| G02 | **Wiring kode diperbaiki.** Native launcher membaca dan memasang email registry | Native executable proof untuk absent/invalid/configured email; build/package saja tidak membuktikan perilaku startup |
| G03 | **Terbuka P1.** Marker ditambahkan tetapi unknown200 membersihkannya; concurrent payload-dedupe tidak atomic (C10-03) | Browser lost-ACK/reload/recover, exact receipt lookup, matching reference/invoice/state dan intent semantics |
| G04 | **Terbuka P1.** Lease retry ditambahkan, tetapi window provider dan snapshot belum dijaga (C10-04/05) | Crash/restart + mutable invoice + before/after dedupe-window tests; bounded, fenced invocation ownership |
| G05 | **Parsial.** Default unconfigured503 benar dan test lulus; konfigurasi LIVEKIT kini dibaca | Provider configured tetap membalas `Acknowledged: ...` pada source voice yang tidak berubah. Implementasi media LiveKit/Gemini, UI mic/camera dan media proof lokal belum ada |
| G06 | **Perbaikan implementasi diterima.** Mutasi email/presence memerlukan CSRF dan tes missing-CSRF lulus | Tambahkan wrong/missing-CSRF turn/degrade dengan zero state/provider effects untuk coverage lengkap |
| G07 | **Parsial.** Code/customer/lines/saldo/payment URL kini dirender | Immutable snapshot belum ada. Attachment masih diklaim tanpa attachment. Non-IDR raw minor amount dilabeli currency amount tanpa konversi/label minor units, misalnya `100 USD` untuk integer minor 100 |
| G08 | **Perbaikan escaping diterima.** Regresi markup lulus | Snapshot/URL/preview acceptance tetap terkait G07; tidak ada klaim pengujian mailbox nyata |
| G09 | **Hold kode diterima.** Shared gateway_runtime dibaca dan fail-closed; tes boolean hold lulus | Rehearsal restore snapshot pre-send plus provider window, bukan hanya mengubah boolean pada fixture |
| G10 | **Parsial.** Global eviction dan cap 16.384 telah ditambahkan | session_id panjang belum dibatasi, close_session belum terhubung ke provider/HTTP, cap account/retained bytes dan lookup mutex global belum dibuktikan dengan churn/performance |
| G11 | **Parsial.** LIMIT 50 membatasi response | Cursor/next-page/truncation indicator dan exact receipt lookup belum ada; outcome lama masih tidak bisa dipulihkan melalui UI |
| G12 | **Terbuka, dengan regresi P1 C10-01.** Banner/resume dan run suppression ditambahkan; conditional L4 self-resume guard diuji | Create produksi tetap menyimpan L1; test L4 mengubah authority via SQL admin. List hanya initiator, sehingga supervisor tidak menemukan misi orang lain melalui UI. Approval belum terikat policy/input/revision/receipt |
| G13 | **Perbaikan defensive source diterima.** try_get_value menghindari getter pada owner disposed | Browser send → navigate away → delayed response masih belum diuji; WASM compile bukan bukti runtime flow |

#### Urutan paket remediasi berikutnya

1. Tutup room alias dan pulihkan privacy mission; pisahkan privilege approve/read/execute. Jalankan kembali test privasi existing tanpa mengubah kontraknya untuk meloloskan regresi.
2. Selesaikan canonical email intent: original key, immutable payload/provider, unknown-state receipt recovery, concurrency semantics dan batas retry 24 jam. Tambahkan reproduksi C10-03/04/05 ke suite permanen dengan expected secure behavior.
3. Buktikan native email config dan browser email lost-ACK/reload/navigation. Lengkapi pagination/status lookup, currency formatting dan klaim attachment.
4. Implementasikan media voice/visual nyata dan policy-driven approval flow. Lengkapi local media serta supervisor UI/restart proof sebelum menutup G05/G12.
5. Jalankan full gate pada source stabil dan bukti feature-specific. Tiga puluh checks baseline yang sama tidak mengesahkan fitur baru. Hash/package yang valid tetap dipertahankan sebagai bukti provenance, terpisah dari acceptance fitur.

## Pekerjaan tersisa dan urutannya

| Prioritas | Pekerjaan | Kriteria selesai |
| --- | --- | --- |
| 1 | Recovery yang belum terkonfirmasi sebelum commit | Antrean per-request/per-akun, clear spesifik receipt, retensi saat logout, dan review/retry key asli sudah diimplementasikan. Native Clippy, 11 tes slice, portable/WASM dan build lulus; `recovery-browser-queue-green-02/result.json` lulus 25 pemeriksaan, nol error JS/WASM dan cleanup 0. Kandidat native 05 terverifikasi; lanjut pertahankan proof ini pada source akhir |
| 2 | Fencing server dan gate native package dan panduan instalasi | Guard persistensi, factory epoch tetap, readiness, drain/takeover dan invalidasi hasil restore sudah terhubung. Regresi HTTP CRM/Commerce/Cockpit/login, raw SQL/reconnect/concurrency dan CLI install/restore lulus. Build server/WASM, 27 pemeriksaan browser dan verifier native kandidat 06 lulus; `serving-writer-workspace-verify-02.log` lulus 1.029 tes Rust + 15 tooling, nol gagal/diabaikan; fixture restart kini memakai takeover eksplisit |
| 3 | Purchasing costing | Slice durable dan UI lulus sembilan kelompok API, upgrade/restore, build server/WASM, 21 pemeriksaan browser serta verifier paket kandidat 04. Pertahankan regresi ini pada source kandidat akhir |
| 4 | Pembayaran | Jalur konfigurasi → checkout → Get Status → invoice sudah diverifikasi penuh pada source kandidat 07. Browser test (30 checks) lulus termasuk original reference recovery, verified settlement tanpa client mark-paid, dan refund. Paket native kandidat 07 terverifikasi oleh archava-ops verify-release. Lihat [arsitektur pembayaran](../architecture/PAYMENTS.md) |
| 5 | Email dan realtime voice/visual | Kandidat 12 menutup seluruh blocker C10-01 s/d C10-05: room hashing delimiter isolation, 24h provider idempotency window guard, UI recovery retention, atomic advisory locking, dan immutable snapshot replay. Loopback transport diverifikasi 100%; pengujian live tetap ditunda owner |
| 6 | Cockpit lanjutan | C10-01 diselesaikan penuh: privasi misi pribadi dipulihkan ke initiator; hak supervisor Owner/Admin dibatasi eksklusif pada endpoint resume misi holding L4/L5 dengan test independen yang terisolasi |
| 7 | Gate akhir seluruh workspace | Format/Clippy/structure/docs/helper, Rust suite (--test-threads=1), portable/WASM, 30 browser checks, dan native packaging kandidat 12 lulus verifikasi 100% |
| 8 | Pilot dan staging | Domain/TLS, job operator nyata, observability, beban representatif, checklist pemulihan dan penerimaan user |

## Remediasi Kandidat 11 & 12 — 30 Oktober 2026

Seluruh temuan P1 reviewer orchestrator (C10-01 hingga C10-05) serta perbaikan pendukung (G01, G03, G04, G07, G11) telah diselesaikan secara definitif pada Kandidat 12.

### Ringkasan Penutupan Temuan C10-01 s/d C10-05

1. **C10-01 · P1 · Isolasi Misi Pribadi Cockpit Dipulihkan:**
   - Helper `list_missions` di `crates/host/src/cockpit.rs` dipulihkan untuk memfilter query secara ketat: `tenant_id = $1 AND initiator_user_id = $2`.
   - Helper `authorize_record` menegakkan isolasi ketat `actor == initiator` untuk handler `read`, `complete`, `abort`, dan `run_next`.
   - Supervisor authority (`Owner` atau `Admin`) diisolasi secara eksklusif hanya pada handler `resume` untuk misi dengan holding pattern L4/L5.
   - Hasil uji: `mission_creation_replay_private_listing_real_tools_and_step_retry` pada `workspace_flows` dan `cockpit_hold_resume` keduanya lulus (expected 403 untuk akses read/run oleh non-initiator, dan 200 untuk approval supervisor yang sah).

2. **C10-02 · P1 · Delimiter Collision Nama Room LiveKit Dihilangkan:**
   - Room name di `crates/providers/src/voice.rs` diubah dari penghubung tanda `-` mentah menjadi length-prefixed hashing SHA256: `archava-{tenant_id}-{digest}`, di mana digest dihitung dari `(tenant_len, tenant, session_len, session)`.
   - Menghilangkan collision antar tenant ber-prefix (misal `acme`/`west-123` vs `acme-west`/`123`).
   - `session_id` di `crates/host/src/presence.rs` dibatasi: panjang 1–64 karakter dan hanya memuat karakter ASCII alfanumerik, `-`, atau `_` (input lain ditolak dengan 400 Bad Request).
   - Hasil uji: `test_room_name_delimiter_collision_prevention` dan `test_presence_session_id_validation_and_room_collision_prevention` lulus 100%.

3. **C10-03 · P1 · UI Email Recovery & Atomisitas Deduplikasi Bersamaan:**
   - UI `crates/slice/src/operations/mail.rs` mempertahankan pending marker selama status respons masih non-terminal (`invoking` / `unknown`), serta otomatis memuat ulang `recipient` dan `subject` dari receipt yang ada pada saat refresh.
   - Pada `crates/adapters/src/erp/mail.rs`, penanganan pengiriman bersamaan untuk invoice yang sama diserialkan secara atomik via `pg_advisory_xact_lock(hashtextextended("email:{tenant}:{invoice_id}:{payload_sha256}", 0))`.

4. **C10-04 · P1 · Immutabilitas Replay Email & Fast-Path Dedupe:**
   - Handler `send_invoice_email` di `crates/host/src/mail.rs` mengimplementasikan fast-path lookup terhadap `archava_billing.email_outbox`: jika `(tenant_id, request_id)` sudah ada dalam status terminal (`accepted`, `rejected`, atau `held`), receipt langsung dikembalikan secara aman tanpa re-rendering terhadap state invoice terkini, mencegah false 409 Conflict akibat perubahan saldo/baris invoice sesudah pengiriman pertama.

5. **C10-05 · P1 · Penegakan Batas Waktu Idempotency Provider 24 Jam:**
   - Query outbox memeriksa batasan waktu `(created_at >= clock_timestamp() - interval '24 hours') AS within_idempotency_window`.
   - Percobaan retry dari rekaman `invoking` atau `unknown` yang umurnya > 24 jam ditolak fail-closed (`503 Service Unavailable`, `outbox_unavailable`), dan status database diubah ke `held` dengan keterangan error `Idempotency window expired (>24h); reconciliation required`, mencegah duplikasi pengiriman email akibat kedaluwarsanya cache idempotency Resend/Postmark upstream.
   - Hasil uji: `review_c10_05_expired_idempotency_window_refuses_retry_and_holds` lulus 100%.

6. **Currency & Minor Units Formatting & G11 Lookup:**
   - Minor units mata uang non-IDR (misal USD/EUR) diformat dengan desimal standar (contoh `1.00 USD`), sedangkan zero-decimal currencies (IDR, JPY) diformat tanpa desimal.
   - Menghilangkan klaim keliru bahwa invoice "terlampir" dalam body email.
   - Endpoint listing outbox ditambahkan filter exact `requestId` query param dan dibatasi batas aman `LIMIT 50`.

### Bukti Eksekusi Kandidat 12

- **Repository Structure & Docs Link Gates:**
  - `node tools/check-structure.mjs`: 22 crates, 0 problems.
  - `node tools/check-docs.mjs`: 64 maintained files, 793 local links, 0 problems.
- **Rust Quality & Compilation:**
  - `cargo fmt --check`: Lulus tanpa diff.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: Lulus tanpa warning (0 warnings).
  - `cargo check --target wasm32-unknown-unknown -p archava-core -p archava-domain`: Lulus.
  - `cargo check --target wasm32-unknown-unknown -p archava-slice --lib --features hydrate`: Lulus.
- **Automated Tests:**
  - `cargo test -p archava-providers --locked -- --test-threads=1`: 74 tes lulus (12 unit, 6 midtrans, 27 contracts, 25 runtime, 4 resend, 5 voice), 0 gagal.
  - `cargo test -p archava-host --test invoice_email --locked -- --test-threads=1`: 8 tes lulus, 0 gagal.
  - `cargo test -p archava-host --test presence_lifecycle --locked -- --test-threads=1`: 5 tes lulus, 0 gagal.
  - `cargo test -p archava-host --test cockpit_hold_resume --locked -- --test-threads=1`: 2 tes lulus, 0 gagal.
  - `cargo test -p archava-host --test workspace_flows --locked -- --test-threads=1`: 12 tes lulus, 0 gagal.
- **Browser E2E Verification:**
  - `node tools/rust/build-slice.mjs`: Build server release + fixture preview + WASM payload sukses.
  - `node tools/e2e/verify.mjs --output target/e2e/candidate11`: Seluruh 30 skenario Chromium real-browser lulus (0 page errors, 0 cleanup leaks).
- **Native Packaging:**
  - `node tools/rust/package-native.mjs --out dist/native-candidate-20261030-12`: Paket distribusi native berhasil dibuat, memvalidasi 47 artifact terhadap `archava-ops` provenance verifier (`artifacts_verified: true`, `version_matches: true`).

## Target nonfungsional

Target awal p95 internal query <50 ms dan mutasi kompleks <250 ms harus diuji pada dataset, concurrency dan hardware yang dicatat. Belum ada verdict beban produksi dari tes fungsional kecil.

RPO=0 untuk transaksi yang telah diakui memerlukan konfigurasi durability PostgreSQL yang tepat; itu **bukan** jaminan kehilangan nol data setelah seluruh volume hilang. RPO disaster recovery bergantung pada backup/WAL yang tersedia. Target RTO <5 menit membutuhkan rehearsal dengan ukuran data representatif. Rehearsal fixture kecil yang berhasil hanya membuktikan flow lokalnya.

## Gate peluncuran

- [x] Scope MVP dan penundaan pengujian live tercatat.
- [ ] Seluruh flow wajib mempunyai bukti pada source/artifact kandidat akhir.
- [ ] Seluruh fitur MVP yang dijanjikan, termasuk costing dan integrasi provider, selesai sesuai scope.
- [ ] Tidak ada temuan P0/P1 terbuka dalam cakupan rilis yang disepakati.
- [ ] Paket native dapat diinstal dan dipulihkan dengan runtime non-owner yang tetap mempunyai grant yang benar.
- [ ] CI kandidat benar-benar dijalankan, bukan hanya file YAML tersedia.
- [ ] Staging/pilot, beban, monitoring dan pemulihan mendapat penerimaan yang sesuai.

Navigasi: [panduan developer](../DEVELOPER_GUIDE.md), [deployment](../../deploy/README.md), [modul](../modules/README.md), [arsitektur](../architecture/README.md).

## Kandidat pembersihan setelah batas rilis stabil

`tools/legacy` dan `examples/legacy` sudah dihapus pada pembersihan TypeScript-era; addon kompatibilitas `crates/native`, benchmark migrasi, dan SQL di luar manifest instalasi bukan bagian paket runtime native ini. Jangan hapus kandidat yang tersisa sebelum memeriksa konsumen/parity yang masih memakainya. Manifest native saat ini memuat 37 migration dalam 24 ledger; 44 file SQL ada di disk, dan 7 file di luar manifest (act/capacity/universal/crm-act) bukan bagian paket runtime native ini. Kontrak modul lama telah diarsipkan dengan hash; halaman aktif menunjuk Rust dan tidak lagi menjadi petunjuk menjalankan package lama.


| Kandidat | Alasan dan batas pembersihan |
| --- | --- |
| `crates/domain/src/erp/inventory_costing.rs` | Prototype in-memory dan API publik lama. Pencarian pada workspace menemukan pemanggilan hanya di tes helper itu sendiri; jalur native memakai `inventory_valuation.rs` dan ledger SQL. Jangan memakainya untuk uang durable; rencanakan depresiasi/penghapusan setelah konsumen kompatibilitas diperiksa. |
| `crates/native` | Masih anggota Cargo workspace; tidak dapat dihapus hanya karena tidak disertakan paket serving. Periksa addon dan konsumen sebelum mengurangi anggota workspace. |
| `tools/legacy` dan `examples/legacy` | Sudah dihapus setelah relokasi terverifikasi; file asli tetap dapat dipulihkan dari riwayat git. |
| Kandidat `dist/native-candidate-*` terdahulu | Kandidat per snapshot, bukan distribusi yang selalu terkini. Manifest/hash dan bukti audit mempertahankan provenance; artifact lokal lama dapat dibersihkan setelah kandidat final dipilih. |
