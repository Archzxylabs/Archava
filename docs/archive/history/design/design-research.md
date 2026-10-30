# Archava — Catatan riset desain

**Tanggal:** 4 Oktober 2026.\
**Tujuan:** menentukan art direction website Archava dan dashboard CRM/ERP, memakai proyek pengguna dan sumber primer.\
**Hasil:** [design.md](../design.md) dan [preview lokal](design-preview.html).

## Metode dan batas pengamatan

Source Archcore dan ArchavaOnchain dibaca. Build yang sudah ada disajikan dengan server statis lokal, kemudian dibuka di Chromium. Tidak menjalankan backend, wallet, sesi avatar, pembayaran, atau provider berbayar. Endpoint API pada inspeksi lokal diblokir; karena itu status demo avatar di capture bukan bukti uptime layanan aslinya.

Browser mengamati desktop 1440×1000 dan ArchavaOnchain mobile 390×844. Source adalah bukti implementasi; screenshot menjelaskan komposisi pada frame yang tertangkap. Banyaknya canvas/animasi tidak menjadi ukuran kualitas atau klaim performa.

Riset web memakai halaman produk, dokumentasi pembuat, dan galeri resmi Awwwards. Beberapa adegan WebGL tidak selesai dimuat dalam browser headless. Batas ini dicatat; motion yang belum terlihat tidak dianggap sudah diverifikasi.

## Audit proyek pengguna

### Archcore marketplace

Sumber lokal:

- [Motion audit](../../Archcore/docs/ARCHCORE_FRONTEND_MOTION_AUDIT.md).
- [Main scene controller](../../Archcore/apps/marketplace-web/src/main.ts).
- [GPU Three.js](../../Archcore/apps/marketplace-web/src/gpuScene.ts).
- [Layout](../../Archcore/apps/marketplace-web/public/index.html).
- [Styles](../../Archcore/apps/marketplace-web/public/styles.css).

Temuan: hero dengan empat babak scroll, GPU prosedural yang berputar/terurai, portal canvas, katalog horizontal, timeline, FAQ sticky, marquee, dan fallback/reduced motion. Capture desktop menampilkan GPU dan headline besar. Keduanya bertemu cukup dekat pada viewport inspeksi; desain Archava menetapkan area aman untuk teks dan objek.

Adaptasi: satu objek khas, cerita yang terhubung ke manfaat bisnis, variasi bidang terang/gelap. Panjang sticky section ditentukan ulang untuk jumlah pesan Archava; seluruh durasi scroll Archcore tidak otomatis diwarisi.

### ArchavaOnchain

Sumber lokal:

- [Hero](../../Archava_onchain/src/components/VideoHero.tsx).
- [Scroll reveal](../../Archava_onchain/src/lib/motion.ts).
- [Styles](../../Archava_onchain/src/styles.css).
- [Live call UI](../../Archava_onchain/src/components/LiveCallDock.tsx).

Temuan: portrait WebP besar, technical grid, graphite/lime, typography besar, CSS introduction, parallax, reveal, marquee, serta state pada percakapan/avatar. Hero memakai image, bukan video hero atau model wajah 3D yang dibuktikan oleh nama komponen. Capture mobile yang diinspeksi tidak mempunyai overflow horizontal halaman.

Adaptasi: identitas lime `#78FF54`, kedalaman material, hero editorial, status asisten yang punya hubungan dengan state. Wallet, token, dan fitur onchain tidak menjadi kebutuhan desain platform agency ini.

### Archava Platform / Template

Sumber: [PRD](../PRD.md), [agent rules](../agent.md), [README](../../README.md), [build status](../BUILD_STATUS.md), [template guide](../Archava%20Template/README.md).

PRD sudah memisahkan Presence, Capability, Environment, serta industri/modul. Design mengikuti pembagian itu. Integrasi CRM/ERP dan workspace baru tetap perlu implementasi; screen concept tidak mengubah status tersebut.

## Referensi web yang digunakan

| Sumber primer                                                                                     | Yang dipastikan                                                                                                                                                     | Keputusan / batas                                                                                                             |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| [Awwwards — Sites of the Year](https://www.awwwards.com/websites/sites_of_the_year/)              | Lusion v3 tercatat SOTY 2023; Igloo Inc tercatat SOTY 2024                                                                                                          | Benchmark ambition dan identitas visual; bukan jaminan Archava mendapat award                                                 |
| [Lusion](https://lusion.co/)                                                                      | Situs pembuat menjelaskan fokus pada storytelling 3D; pengamatan kedua menampilkan objek modular hitam, putih, dan biru di dalam bidang besar dengan sudut membulat | Material dan objek menjadi pusat komposisi. Ini pengamatan satu frame; seluruh choreography dan performa situs belum diuji    |
| [Igloo Inc](https://www.igloo.inc/)                                                               | Situs resmi dan entry di galeri Awwwards                                                                                                                            | Capture live kosong/belum siap; dijadikan referensi art direction untuk review berikutnya, bukan bukti choreography tertentu  |
| [Linear](https://linear.app/)                                                                     | Pengamatan kedua menampilkan headline besar dan contoh workspace dengan sidebar, activity, dan panel agent                                                          | Hierarki produk terlihat lewat interface konkret. Dasar keputusan untuk konsistensi app juga memakai artikel pembuat di bawah |
| [Linear — How we redesigned the UI](https://linear.app/now/how-we-redesigned-the-linear-ui)       | Tim menjelaskan alignment, hierarchy, theme, panel, dan pengujian berbagai view                                                                                     | Dipakai untuk konsistensi shell/table/board; bukan klaim menyalin seluruh UI Linear                                           |
| [Stripe](https://stripe.com/)                                                                     | Halaman menjelaskan solusi bisnis dan menampilkan contoh interface                                                                                                  | Dipakai untuk urutan manfaat dan contoh alur. Fitur atau angka Stripe tidak menjadi claim Archava                             |
| [Frappe CRM first steps](https://docs.frappe.io/crm/setup-guide/your-first-steps-with-frappe-crm) | Alur lead/deal, tasks, komunikasi, role, dan konfigurasi awal                                                                                                       | Fondasi nama record dan proses CRM; kebutuhan industri tetap dikonfigurasi                                                    |
| [Frappe CRM branding](https://docs.frappe.io/crm/custom-branding)                                 | Nama, logo, favicon dapat disesuaikan                                                                                                                               | Branding native tidak sama dengan pembangunan UI penuh pada preview                                                           |
| [Frappe REST API](https://docs.frappe.io/framework/user/en/api/rest)                              | API DocType, autentikasi, dan role pengguna                                                                                                                         | Adapter workspace diusulkan; belum diuji terhadap deployment CRM klien                                                        |

## Dasar motion, aksesibilitas, dan performa

- [Motion for React](https://motion.dev/docs/react): library untuk layout/state/gestures; dipertimbangkan untuk workspace React.
- [GSAP ScrollTrigger](https://gsap.com/docs/v3/Plugins/ScrollTrigger/): timeline yang mengikuti scroll; dipertimbangkan untuk satu adegan website.
- [GSAP matchMedia](<https://gsap.com/docs/v3/GSAP/gsap.matchMedia()/>): kondisi breakpoint/reduced motion dan cleanup pada konfigurasi animasi.
- [Chrome View Transition API](https://developer.chrome.com/docs/web-platform/view-transitions/): enhancement perpindahan view dengan fallback.
- [MDN prefers-reduced-motion](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion): menghormati preferensi sistem.
- [W3C animation from interactions](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html): dasar menyediakan kontrol untuk motion interaksi.
- [Google Core Web Vitals](https://web.dev/articles/vitals): LCP ≤2,5s, INP ≤200ms, CLS ≤0,1 pada percentile ke-75 sebagai target produksi. Pemeriksaan visual lokal bukan pengukuran field tersebut.

## Kesimpulan keputusan

Website menggunakan satu adegan khas untuk menceritakan percakapan yang berubah menjadi pekerjaan. Workspace mempertahankan identitas material dan warna, dengan motion singkat yang membantu membaca state. Implementasi bertahap mengawali CRM native dan integrasi AI sebelum memperluas custom workspace serta ERP.

Detail yang harus diuji saat implementasi: asset 3D final, ukuran bundle, kontras seluruh tema, performa perangkat mobile sasaran, mapping API, permissions, dan hasil end-to-end terhadap sistem yang benar-benar terhubung.

## Verifikasi preview lokal

Preview dibuka dari file lokal di Chromium melalui Playwright. Pemeriksaan berikut lolos:

- Website, CRM, dan ERP pada lebar 360, 390, 768, 1024, 1440, dan 1920px tanpa overflow horizontal halaman. Tabel ERP pada mobile bergeser di dalam area tabel; headline website tidak terpotong.
- Pergantian tab melalui click, tombol panah, Home/End; CTA website membuka contoh CRM.
- Pencarian lead, jumlah hasil per kolom, dan empty result.
- Dialog record/order, Escape, serta pengembalian focus ke pemicu.
- Buka/tutup panel AI dengan `aria-expanded` yang sesuai.
- Pembatalan proposal tidak mengubah contoh data; approval memperbarui tugas/activity satu kali dan memberi label simulasi.
- Preferensi reduced motion sistem dan pengaturan manual memengaruhi animasi yang tampil.
- Tidak ada JavaScript page error atau permintaan jaringan eksternal dari preview.

Screenshot desktop/mobile disimpan di [folder design](.). Ini verifikasi prototype offline, belum audit pembaca layar, sertifikasi aksesibilitas, benchmark produksi, atau pengujian integrasi CRM/ERP.
