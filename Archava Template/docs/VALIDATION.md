# Validasi template — 3 Oktober 2026

Pemeriksaan source/runtime dilakukan pada template serta hasil ekstraksi arsip di direktori terpisah `/tmp`, memakai mock dan tanpa deployment Archava lama. Audit profil pribadi tambahan memakai env lokal pengguna atas permintaan eksplisit, hanya untuk import/perbandingan konfigurasi dan build; tidak membuka panggilan provider.

| Pemeriksaan | Hasil |
| --- | --- |
| Install JavaScript dari lockfile (`npm ci`) | Lolos, memakai dependency sendiri. |
| Test JavaScript, protokol JS/Python, dan profil environment | 55 test lolos, termasuk 12 pemeriksaan CLI/profile/export. |
| Test knowledge, prompt, dan page context Python | 4 test lolos tanpa SDK provider. |
| Test tool agent, metadata, startup worker, dan tuning SDK LiveKit | 7 test lolos; room/provider di-mock. |
| Build TypeScript + Vite + asset WASM Spatius | Lolos. |
| Install Python requirements ke venv baru Python 3.12 | Lolos, terpisah dari venv Archava asal. |
| Konstruksi AgentSession dengan SDK terpasang | Lolos dengan fake API key; tidak membuka koneksi provider. |
| Tampilan 1440, 768, 390, dan 320 px | Gambar dan section lengkap, tidak overflow horizontal, tidak ada error JavaScript atau UI blockchain. |
| Microphone ditolak | UI menampilkan petunjuk dan tidak meminta pembuatan sesi. |
| Backend start gagal | UI menampilkan pesan, bisa dismiss, dan kontrol pulih. |
| Preload avatar saat halaman terbuka | Lolos di browser dengan provider mock: preload berjalan sebelum klik tanpa microphone/room; tombol start memakai promise yang sama dan menunggu asset siap. |
| Supervisor API/worker | Kegagalan tiap proses mempertahankan exit status dan menghentikan sibling. |
| Shutdown ketika room masih dibuat | Pembuatan yang sedang berjalan ditunggu dan room ditutup; sesi baru ditolak selama shutdown. |
| Kegagalan cleanup satu room | Room lain tetap diproses, recovery tetap dijalankan, dan room yang gagal dicoba lagi pada sweep berikutnya. |
| Urutan startup Spatius/Tavus/voice | Entrypoint diuji langsung: connect, avatar start/join bila diperlukan, lalu voice session. Spatius mematikan audio langsung agar playback tidak ganda. |
| Worker expiry dan ack | Deadline menghapus room tanpa API Node; callback page mengakui hanya guest yang terikat metadata. |
| `docker compose config --quiet` | Lolos dengan environment contoh dari setup. |
| Export dan ekstraksi | Source/config/docs/asset lengkap; tidak ada `.env` asli, dependency, cache, build, atau state pengguna. |
| Rename brand/host pada hasil ekstraksi | Lolos, termasuk brand baru yang mengandung nama host lama. Test dan build tetap lolos setelah rename. |
| Audit petunjuk pemasangan | Link file lokal valid dan snippet Bash dapat diparse. Arsip terbaru diekstrak ke folder kosong; `npm ci`, setup/rename, config check, build, dan Compose config lolos. |
| Smoke check mengikuti panduan | API hasil ekstraksi menjawab health/config/CORS yang diharapkan dan melayani frontend build. Kredensial kosong ditolak `check:env` sesuai petunjuk. |
| Pemisahan env personal/client | Profil pribadi hanya menyimpan provider credentials/tuning, di luar source. Setup personal memakai namespace baru; client tidak membaca profil tersebut dan reset client menghapus key pribadi. |
| Kebocoran key pada handoff | Test canary acak memastikan key tidak ada di isi arsip; env contoh yang terisi ditolak dan file env nested dikecualikan. |
| Profil pribadi yang diizinkan pengguna | Import cocok dengan source env, file permission 600, setup/check/build pada hasil ekstraksi lolos, dan parser Node/Python membaca nilai yang sama. Scan build frontend dan arsip client tidak menemukan API key pribadi. Folder verifikasi berisi env sementara dihapus setelah pemeriksaan. |
| File Archava Onchain asal | Tidak berubah; setelah pemindahan template ke Archava Platform, repository Onchain kembali bersih. |

Build memiliki warning ukuran chunk dari dependency LiveKit/renderer; proses tetap sukses. Ketika konfigurasi memilih Spatius, asset dipreload saat halaman terbuka. Tuning realtime worker dibandingkan dengan source Archava dan konfigurasi model/suara/provider aktif; baseline sama. Latency panggilan live belum diukur.

Yang belum diverifikasi: panggilan dengan provider nyata, perilaku model live terhadap pertanyaan/adversarial prompts, deployment ke akun/domain baru, serta build/run image Docker. Docker daemon mesin ini tidak aktif; Compose dan script lifecycle telah diperiksa, tetapi image belum dibangun. Ketika memakai template untuk proyek baru, isi kredensial baru dan lakukan validasi live sesuai `DEPLOYMENT.md`.

## Audit terhadap sumber

Tuning model/suara, thinking budget, endpointing, interruption, versi dependency call, urutan renderer, microphone/audio controls, page acknowledgement, named agent dispatch, dan preload/cache dibandingkan dengan source Archava. Tidak ada kekurangan lain yang teridentifikasi dalam audit bagian inti itu. Total test otomatis setelah penambahan profil env: 66, ditambah build dan pemeriksaan browser/lifecycle yang dijelaskan di atas. Ini bukti validasi source dan mock, bukan jaminan seluruh deployment/provider selalu berhasil.

Perbedaan yang disengaja: akses wallet/kontrak/billing dihapus; sesi menjadi anonymous dengan batas admission yang dijelaskan di README; knowledge dan identitas menjadi konfigurasi proyek; fallback audio dipilih eksplisit lewat `AVATAR_PROVIDER=voice` sehingga kegagalan provider video tidak diam-diam mengubah jenis sesi. Docker/hosting memakai recipe dan domain proyek baru. Perbedaan ini tidak dinyatakan sebagai fitur yang identik dengan Archava Onchain.

## Audit panduan pemasangan

`INSTALLATION.md` menjelaskan prasyarat, ekstraksi, asal kredensial, pembagian environment, setup lokal, dan tanda keberhasilan. `DEPLOYMENT.md` mencakup setting service Railway/Vercel, pemisahan API/worker, Docker, contoh VPS dengan HTTPS, update/restart, CORS/proxy, diagnosis, dan uji live. `INTEGRATION.md` menambahkan contoh iframe serta kebutuhan integrasi langsung/page awareness.

Dokumentasi hosting resmi diperiksa ulang. Recipe `railway.json` lama dihapus dari template/export karena [Railway menyatakan service baru tidak dapat memakai Config as Code lama](https://docs.railway.com/config-as-code); panduan memakai setting Docker service yang eksplisit. Setting dashboard Railway, penerbitan sertifikat Caddy, image Docker, dan panggilan provider nyata belum dieksekusi oleh audit ini. Pemeriksaan panduan tidak mengubah tuning percakapan.

## Lokasi di Archava Platform

Template dipindahkan dari Archava Onchain ke `Archverse/Lab/Archava_Platform/Archava Template`. Manifest 80 file source/export cocok sebelum dan sesudah pemindahan. Panduan import env disesuaikan dengan lokasi baru; profil pribadi di direktori config pengguna tetap sama. Config check dan 12 test profile/export lolos dari lokasi baru. Structure, lint, dan pemeriksaan format file index/config Platform juga lolos; template memakai npm/check sendiri di luar workspace pnpm Platform. Arsip export diperbarui setelah petunjuk lokasi diubah.

## Verifikasi konsolidasi — 7 Oktober 2026

Install bersih `npm ci`, 55 test JavaScript, 4 test guardrail Python, dan build template lolos ketika source digabung dengan platform. CI repository sekarang menjalankan pemeriksaan tersebut sebagai job template tersendiri. Tujuh test integrasi SDK Python belum diulang pada konsolidasi ini karena venv proyek tidak tersedia; hasil historisnya tercatat di atas.

`npm audit --omit=dev` melaporkan 16 advisory pada dependency graph yang terpasang: 14 moderate dan 2 high. Dua package berlevel high adalah `source-map-js` dan `vite`; versi tetap mengikuti lockfile template yang diaudit. Upgrade dependency dan pemeriksaan ulang advisory perlu diselesaikan sebelum deployment live. Konsolidasi source dan build yang lolos belum menjadi bukti kesiapan produksi.

## Dependency cleanup — 8 Oktober 2026

Vite dinaikkan dari 5.4.21 ke 6.4.4; source-map-js diperbarui ke versi patched.
Node minimum/pin platform menjadi 22.22.3 karena dependency machina meminta
22.22+. `npm audit --omit=dev` setelah cleanup melaporkan 13 package entries
moderate, nol high dan nol critical. Sisa advisory berasal dari rantai
OpenTelemetry yang dipin SDK Spatius; versi inti 2.7.1 masih dikunci beberapa
exporter/SDK. Tidak ada downgrade Spatius atau override lintas versi internal
telemetry yang dipaksakan. Ikuti upgrade vendor yang kompatibel dan ulang
provider validation sebelum mengklaim produksi. Angka registry tidak sama
dengan jumlah exploit yang dapat dijangkau.

Guardrail/JavaScript/build tetap diuji offline. Agent/session behavior dan
protokol tidak diubah. Tidak ada live provider, deployment atau Docker test
baru; tujuh test SDK Python belum diulang karena venv tidak tersedia.
