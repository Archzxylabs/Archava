# Audit seluruh project — sebelum milestone reusable

**Tanggal:** 7 Oktober 2026. **Baseline kode:** `main`, `63b221b`.
**Scope:** repo Archava Platform beserta `Archava Template`, konfigurasi, CI, dokumentasi dan task history. PRD v2/riset masih berupa perubahan dokumentasi lokal dalam working tree.

## Keputusan

**Bisa lanjut dari fondasi existing, dengan cleanup kecil sebagai pekerjaan pertama milestone reuse.** Tidak ada alasan hasil audit untuk menghapus platform, membongkar semua package atau memindahkan company website ke sini.

Sebelum mengklaim Archava bisa dipasang independen, bereskan browser/package boundary dan makna konfigurasi. Triase dependency tooling sebelum menambah banyak pekerjaan di atasnya. Sebelum real booking/email/CRM write, selesaikan trusted identity, durable host attempt dan recovery. CRM/ERP backend adalah pekerjaan baru yang belum ada, bukan folder lama yang perlu dibersihkan.

Audit ini menghasilkan daftar tindakan; tidak menghapus file, mengubah dependency, menjalankan `audit fix`, mengaktifkan provider atau mengubah runtime. Server/browser dan database disposable yang dibuat untuk audit sudah dihentikan.

## Cakupan dan keadaan workspace

- 441 tracked files, sekitar 5,22 MB isi file; 19 package platform, satu app dan dua workspace tools. Struktur memeriksa 22 project di luar root; pnpm mengenali 23 termasuk root.
- `Archava Template` memiliki 78 tracked files dan tetap standalone di luar workspace pnpm. Ada AGENTS khusus yang melarang import parent repo dan mengharuskan preservasi protokol sesi/page serta source export tanpa secrets.
- Hanya satu Git worktree aktif: folder utama pada `main`. Worktree Act lama sudah tidak terdaftar. Perubahan lokal sebelum audit adalah dokumen PRD v2, riset dan proposal; tidak ada perubahan kode runtime.
- Ukuran folder lokal sekitar 418 MB terutama dependency: root `node_modules` 115 MB dan template 279 MB. `.git` sekitar 4,6 MB; output build/cache/export di-ignore. Menghapus cache tidak memperbaiki kontrak reuse.
- Diperiksa struktur dan manifest seluruh package, entry points/import graph, boundary core/policy/SDK, configuration/pricing, Act/SQL, web demo, template/API/worker/export, CI serta dokumen aktif/history. Pemeriksaan berfokus pada kontrak dan jalur penting, bukan klaim pembacaan manual setiap baris test atau sertifikasi keamanan.

## Verifikasi yang dijalankan ulang

| Pemeriksaan                    | Hasil                                                             | Arti bukti                                                                                      |
| ------------------------------ | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `pnpm verify`                  | Enam gate lolos; 1.295 test di 80 file                            | Install frozen lockfile, structure, lint, typecheck, test dan build platform.                   |
| `pnpm format:check`            | Lolos                                                             | Root/workspace docs-source yang masuk formatter; template memang dikecualikan oleh konfigurasi. |
| Reference browser              | 31/31 lolos                                                       | UI/context/knowledge/permission reference berjalan offline.                                     |
| Frontend browser               | 16/16 lolos                                                       | Form, escaping, demo edits/approval dan mobile behavior.                                        |
| Marketing viewport             | 360, 768, 1440 px: tidak overflow/error/resource gagal            | Pemeriksaan layout/runtime DOM, bukan benchmark produksi.                                       |
| Template `npm test`            | 55/55 lolos                                                       | JavaScript/session/page/profile/export checks.                                                  |
| Template guardrails            | 4/4 lolos pada Python default dan 3.12.13                         | Knowledge/page protocol offline. Tidak memakai SDK/provider.                                    |
| Template build                 | Lolos                                                             | Ada warning chunk besar; belum pengukuran latency sesi live.                                    |
| Act SQL verifier               | Migration, 14 invariant, claim/settlement race antarproses lolos  | Database disposable dengan SQL nyata, bukan driver/deployment produksi.                         |
| Confirmation SQL verifier      | 27/27 lolos, termasuk race dan restart                            | Store/migration lokal; bukan authenticated host live.                                           |
| Browser bundle tanpa alias app | SDK dan assistant lolos; Chat gagal pada `node:fs`/`node:path`    | Membuktikan satu hambatan reuse; belum packed-artifact cold install.                            |
| Local Markdown links           | Tidak ditemukan link rusak setelah URL decoding                   | Dokumen aktif; arsip historis tidak diperlakukan sebagai instruksi aktif.                       |
| Secret-pattern scan terbatas   | 426 tracked text files; tidak ada match empat pola yang diperiksa | Nama env tracked hanya examples. Bukan secret scan lengkap atas Git history.                    |
| Template export archive        | 78 file; tidak ada nama path/env/generated/symlink terlarang      | `docs/VALIDATION.md` dalam archive tertinggal dari source.                                      |

Pemeriksaan utama memakai **Node 26.10.0**, sedangkan CI dan `.tool-versions` mempin **22.18.0**. Ini bukan pengulangan lokal pada runtime CI. Python default mesin 3.14.7; guardrail diulang pada 3.12.13 yang sesuai template. Venv dan SDK Python LiveKit/Gemini/dotenv tidak tersedia, sehingga tujuh integration test SDK tidak dijalankan. Docker daemon tidak aktif; image/container tidak dibangun atau dijalankan. Tidak ada live LLM, avatar, email, booking, CRM atau payment call, load test, restore production, SAST/DAST lengkap atau audit kepatuhan.

## Temuan yang perlu dibereskan

Prioritas berikut menyatakan kapan pekerjaan harus selesai. P0 sebelum live bukan pernyataan bahwa reference offline sedang melakukan transaksi berbahaya.

### F01 — P1, sebelum proof consumer: Chat bergantung pada alias demo

**Bukti:** [money.ts](../packages/chat/src/money.ts#L1) mengimpor currency dari root `@archava/config`. Root barrel mengekspor filesystem loaders. [server.ts](../apps/web/src/server/server.ts#L99) mengganti root import itu dengan `pure.ts` melalui plugin esbuild khusus.

Build browser langsung dari `packages/chat/src/index.ts` tanpa plugin tersebut gagal karena `node:fs` dan `node:path` dari pricing/template loaders. SDK dan assistant berhasil pada build yang sama, jadi temuan ini spesifik; bukan klaim seluruh package tidak portable.

**Tindakan:** pindahkan browser imports ke public pure/browser contract atau gunakan export yang memisahkan lingkungan secara eksplisit. Buktikan packed consumer tanpa alias app. Pertahankan aturan scale IDR/USD dan existing money tests. Jangan menjadikan konfigurasi bundler khusus app sebagai syarat tersembunyi setiap klien.

### F02 — P1, sebelum resolved deployment: enabled tidak berarti connected

**Bukti:** [resolver.ts](../packages/config/src/client/resolver.ts#L61) memasukkan `common_integrations` sebagai Standard dan `enabled: true`. [emitter](../tools/configurator/src/client-config.ts#L24) hanya mengeluarkan integrasi yang diminta intake dan bahkan mendokumentasikan perbedaan scope itu.

Reproduksi dari fixture clinic: Capability **Assist**, quoted integrations kosong, emitted integrations kosong; setelah resolve ada tiga enabled integrations: `Cal.com-or-clinic-scheduler`, `Postmark`, `CRM`. Belum ada koneksi atau eksekusi provider yang terjadi dari hasil ini. Risikonya muncul saat hasil planning dipakai untuk provisioning, readiness atau janji scope yang nyata.

Nama seperti `not-a-real-module` juga tetap lolos merge sebagai enabled karena [module schema](../packages/config/src/client/schema.ts#L44) hanya nama dan boolean. Tidak ada dependency/version/readiness resolver.

**Tindakan:** tambahkan katalog/metadata tipis dan pisahkan requested/supported/configured/connected/healthy. Template memberi suggestion. Quote dan plan deployment harus menunjuk scope yang sama. Buat migrasi eksplisit agar reference/legacy behavior tidak berubah diam-diam; jangan merombak angka pricebook.

### F03 — P1, tooling sebelum perluasan; template sebelum deploy: dependency advisories

Snapshot registry disimpan pada [dependency evidence](audit/2026-10-07-dependencies.json).

| Scope                       | Registry result                                    | Jalur yang ditemukan                                                                                 |
| --------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Platform semua dependencies | 2 critical, 1 high, 2 moderate                     | `tinypool@1.1.1` melalui Vitest; `source-map-js@1.2.1`; Vitest/mocker.                               |
| Platform `--prod`           | 0 advisory                                         | Production graph platform yang dipilih audit. Tidak mencakup template terpisah.                      |
| Template `--omit=dev`       | 16 vulnerable package entries: 2 high, 14 moderate | Vite/source-map-js, esbuild dan rantai OpenTelemetry/Spatius. Angka ini bukan 16 exploit independen. |

Registry menandai dua advisory tinypool sebagai critical; halaman maintainer memberi label high. Keduanya memerlukan kondisi prototype pollution tertentu, dan dependency di repo ini berada pada tooling test/build. Ini membutuhkan perbaikan supply chain, tetapi tidak membuktikan RCE lewat public Archava API. [Tinypool worker advisory](https://github.com/tinylibs/tinypool/security/advisories/GHSA-5gmw-xhrv-c9v3), [Tinypool run advisory](https://github.com/tinylibs/tinypool/security/advisories/GHSA-85c8-ppgw-ccpr).

Vitest/mocker advisory menyangkut redirect mocks pada dev-server paths tertentu. Ordinary unit suite memakai environment Node dan tidak membuka jalur server yang dibahas advisory. [Vitest advisory](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9).

Template Vite `5.4.21` adalah direct dev dependency, sekaligus optional peer dependency dari production `@spatius/avatarkit`. Karena itu ia muncul pada omit-dev graph. Salah satu high Vite advisory membutuhkan exposed dev server serta Windows/NTFS conditions; bukan bukti bahwa built Node API di mesin Linux ini memiliki exploit yang sama. Template `dev` memang memakai `--host 0.0.0.0`. [Vite maintainer advisory](https://github.com/vitejs/vite/security/advisories/GHSA-fx2h-pf6j-xcff). Source-map-js high berasal dari source-map parsing DoS. [Source-map-js advisory](https://github.com/advisories/GHSA-68fv-2mgg-jv7q).

**Tindakan:** upgrade/triase root test tooling secara terkoordinasi, lalu ulang gate yang relevan. Untuk template, pilih perbaikan yang mempertahankan avatar/worker compatibility dan periksa artifact/runtime yang benar-benar dideploy. Jangan jalankan `npm audit fix --force`: saran registry mencakup major upgrade Vite dan bahkan pilihan Spatius versi 1.0.0 yang bukan upgrade sederhana dari baseline sekarang. Tidak ada dependency diubah oleh audit ini.

### F04 — P2, sebelum menawarkan education: katalog dan intake berbeda

**Bukti:** [template catalog](../config/templates.v1.json) memiliki delapan template termasuk `education`; [INDUSTRIES](../tools/configurator/src/intake.ts#L52) hanya tujuh dan tidak mencakup education. Valid fixture dengan industry education ditolak parser.

**Tindakan:** satukan vocabulary yang didukung atau tandai education unsupported secara eksplisit sampai end-to-end path-nya tersedia. Tambahkan regression lintas katalog/intake; jangan otomatis menjadikan seluruh kit education production-ready hanya karena enum bertambah.

### F05 — P0 sebelum live action: host attempt belum atomic/durable

**Bukti:** [host](../packages/act-host/src/host.ts#L416) memakai `find` lalu `save` untuk canonical `occurredAt`; [ledger](../packages/act-host/src/attempts.ts#L60) hanya menyediakan kontrak read/write dan in-memory implementation. Dua concurrent review bisa membuat identitas waktu berbeda, dan state hilang setelah restart. Dua permintaan sengaja baru dengan payload sama juga belum memiliki lifecycle intent yang eksplisit.

SQL replay/outbox dan confirmation store yang sudah lolos bukan pengganti host ledger ini: mereka memecahkan boundary berbeda.

**Tindakan:** atomic durable creation/lookup intent/attempt, new-intent-versus-retry semantics dan race/restart tests sebelum real side effect. Pertahankan store/executor/confirmation yang sudah ada, bukan menggantinya dengan raw LLM tool calls.

### F06 — P0 sebelum API bisnis live: trusted identity belum tersambung host

**Bukti:** [turn request](../packages/assistant/src/turn.ts#L117) menerima role dan confirmation flags dari trusted caller. [ActHost](../packages/act-host/src/host.ts) sudah menolak browser approval flags dan memakai server-verified receipt, tetapi [web server](../apps/web/src/server/server.ts#L202) baru menyediakan stateless quote API dan halaman demo. Belum ada authenticated tenant/member/resource host route atau native CRM API.

**Tindakan:** bind public customer, employee/owner dan service actor secara eksplisit. Business API mengotorisasi resource untuk dashboard, Archava dan worker. Jangan menambahkan route yang meneruskan role/confirmation dari body langsung ke `runTurn`. Backend tenancy/RLS dan secrets tetap pekerjaan sebelum live, bukan cleanup CSS.

### F07 — P1 sebelum live email: status Postmark selalu unknown

**Bukti:** [status.ts](../packages/act-postmark/src/status.ts#L68) selalu mengembalikan unknown. Ini merupakan refusal/recovery boundary yang disengaja dan aman; interrupted send tidak boleh diasumsikan gagal lalu dikirim ulang.

**Tindakan:** tenant-bound positive authoritative reconciliation atau operator recovery yang eksplisit, transport/credentials nyata serta controlled verification. Jangan mengganti unknown menjadi success untuk membuat demo terlihat selesai.

### F08 — P1 saat milestone shared presence: realtime template punya core terpisah

**Bukti:** [worker](<../Archava Template/backend/agent.py>) memakai instruction/knowledge/page tools sendiri. Platform punya [assistant](../packages/assistant/src/turn.ts), [SDK context](../packages/sdk/src/session.ts) dan typed action boundary. Template page protocol hanya four named sections/revision, berbeda dari platform Context Graph.

**Tindakan:** rancang typed server/protocol bridge menuju shared core. Pertahankan worker Python, guest binding, expiry, dispatch dan acknowledgement. Jangan sekadar copy TypeScript parent ke template atau menggabungkan dua protokol menjadi satu tanpa compatibility tests. Standalone packaging tetap merupakan nilai yang perlu dijaga.

### F09 — P1 sebelum milestone CRM: layar demo bukan module backend

**Bukti:** [workspace data](../apps/web/src/frontend/data.ts) dan [browser storage](../apps/web/src/frontend/ui.ts#L68) memakai fictional local records. Test browser sudah membuktikan form/escaping dan local approval, bukan auth, durable audit, tenant database, CRM API atau module migrations.

**Tindakan:** pertahankan UI sebagai reference; ekstrak komponen ketika service/schema kontrak pertama jelas. Mulai contacts + activities/follow-up lintas dua konteks. Jangan menjadikan localStorage source of truth bisnis atau membangun ERP universal sekaligus.

## Cleanup ringan dan pekerjaan pendamping

| ID  | Temuan                                                                                                                                                            | Tindakan yang masuk akal                                                                                                                                                                 |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C01 | [nx.json (archived placeholder)](history/nx.placeholder.json) mereferensikan Nx schemas/preset tetapi Nx tidak terpasang/dijalankan; runtime memakai pnpm scripts | Hapus placeholder atau label legacy. Tidak perlu install Nx hanya untuk membenarkan file lama.                                                                                           |
| C02 | 20 referensi worktree lama pada 16 file task history                                                                                                              | Buat index/status historical; task aktif menunjuk primary workspace. Pertahankan execution paths/commit evidence historis, jangan memalsukan riwayat dengan rewrite semua path.          |
| C03 | Tiga Result operational-readiness masih Pending                                                                                                                   | Tetap backlog yang belum dikerjakan. Jangan mengartikan source merge atau banyak tests sebagai tugas selesai.                                                                            |
| C04 | PRD v2, proposal, riset dan archive masih local modifications/untracked                                                                                           | Review/checkpoint sebagai perubahan dokumentasi yang terpisah dari runtime cleanup. Arsip v1 tetap evidence, bukan PRD aktif kedua.                                                      |
| C05 | Root engine `>=20.11.0`, tetapi browser harness memakai global WebSocket Node 22+; audit lokal Node 26 versus pinned CI 22.18                                     | Selaraskan developer/runtime/check prerequisites; verifikasi pada versi yang dipin sebelum release. Jangan menyimpulkan hasil Node 26 sama dengan Node 22.                               |
| C06 | Template command memakai `python3`; mesin default 3.14, dokumentasi mendukung 3.12/3.13; SDK venv tidak tersedia                                                  | Jalankan worker/tests dengan venv/version yang eksplisit. Tambah SDK integration validation ketika environment-nya disiapkan; jangan pakai guardrail success sebagai bukti provider SDK. |
| C07 | CI tidak menjalankan format, browser E2E, SQL verifiers atau dependency audit; template format sengaja di luar root                                               | Tambah gates relevan bertahap, browser/test DB setup yang repeatable dan advisory triage. Hindari pipeline/security stack besar untuk sekadar checklist.                                 |
| C08 | Export archive berbeda hanya pada `docs/VALIDATION.md`                                                                                                            | Regenerate/check archive sebelum handoff agar caveat dependency terbaru ikut terbawa. Archive di-ignore dan bukan deploy package yang telah dikirim.                                     |
| C09 | Template chunks 827 KB–2,65 MB plus avatar/WASM worker assets                                                                                                     | Ukur load/preload dan realtime latency sebelum optimasi. Lazy loading sudah ada; warning build bukan alasan rewrite.                                                                     |
| C10 | Hero asset identik pada app demo dan template                                                                                                                     | Biarkan sementara: template harus standalone. Shared asset source/build-copy policy dapat ditambahkan nanti; penghematan satu file sekitar 110 KB tidak mendesak.                        |

## Yang dipertahankan

- `packages/core`, `assistant`, `acl`, `knowledge`, `decision`, SDK dan currency/quote rules: kontrak dan regressions-nya bernilai.
- `act-*`: confirmation, gateway, replay/outbox dan SQL adapters memisahkan permission dari execution/outcome. Lengkapi host/provider seams yang belum ada.
- `packages/reference` dan offline browser slice: hospitality fixtures memang berada pada package reference; pencarian legacy markers tidak menunjukkan wallet/onchain/White-Rock code pada core source yang diperiksa. Reference adalah test/demo consumer yang berguna.
- `apps/web` marketing/workspace dan screenshots/design: reference produk yang terlabel demo, bukan website company yang perlu dipindahkan otomatis.
- `Archava Template`: reusable presence/session behavior. Keberadaan foldernya bukan tanda branch belum merge.
- Pricebook, signed-scope authority, PRD v1 archive dan historical audit evidence: jangan dihapus demi folder terlihat bersih.
- Generated dependencies/builds di-ignore; tidak ada tracked `node_modules`, `.venv`, compiled cache, real `.env`, `.pem` atau `.key` yang ditemukan lewat pemeriksaan nama file ini.

## Urutan pekerjaan yang direkomendasikan

1. Checkpoint dokumentasi/temuan dan tandai history versus backlog aktif; bereskan placeholder kecil serta runtime prerequisites.
2. Triase/upgrade tooling dependencies dengan regression; template advisory ditangani sebelum deployment-nya.
3. Perbaiki public browser/server exports, kemudian packaging dan independent-consumer checks. Ini bagian pertama M1, bukan proyek rewrite terpisah.
4. Tambahkan katalog/readiness/config contract tipis; perbaiki quoted-versus-resolved scope dan vocabulary education. Preserve legacy config dengan migrasi yang eksplisit.
5. Buktikan core yang sama di dua reference consumers. Setelah itu bangun satu business module kecil dengan persistence, auth, permissions, migrations dan UI/API/tools bersama.
6. Lengkapi durable host/new-intent/recovery serta provider gates sebelum real writes. Hubungkan presence/shared core bertahap; ukur biaya/latency pada controlled live validation yang terpisah.

Tidak semua temuan harus selesai sebelum menulis kontrak/packing prototype. Auth/recovery/payment/provider gates harus selesai sebelum tindakan bisnis live yang bergantung pada boundary tersebut. Hasil ini mendukung PRD v2 milestone order dan tidak menaikkan historical 30% commercial score.

## Artefak dan reproduksi

Dependency snapshot ada di `docs/audit/2026-10-07-dependencies.json`. Log/screenshot audit lokal berada di `/tmp/archava-audit-*`; hasil gate di atas merupakan pemeriksaan ulang dalam turn ini. Tidak ada PR/commit/deploy baru dibuat oleh audit. Source perubahan tetap dokumentasi.

Browser tests memakai server localhost port 4873, debug browser 9472/9473 dan scratch profiles tersendiri. Direktori screenshot dibuat sebelum reference test; kegagalan setup path awal diperbaiki dan run lengkap 31/31 lolos. Database verifier membuat/menghapus cluster disposable sendiri. Semua proses layanan/browser audit telah ditutup.
