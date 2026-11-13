/** Synthetic customer goals, not promises of mounted agent capabilities. */
export const businessGoals = [
  ['crm', 'Susun prioritas tindak lanjut calon pelanggan minggu ini dan ajukan tindakan yang perlu persetujuan.', 'Prioritize this week’s customer follow-ups and propose the work for my approval.'],
  ['commerce', 'Periksa stok menipis lalu susun rencana replenishment minggu depan sebelum melakukan perubahan.', 'Review low stock and plan next week’s replenishment before making changes.'],
  ['purchasing', 'Bandingkan pemasok yang tersedia lalu siapkan rencana pembelian untuk saya tinjau.', 'Compare available suppliers and prepare a purchasing plan for my review.'],
  ['billing', 'Periksa invoice jatuh tempo dan buat rencana penagihan yang perlu saya setujui.', 'Review overdue invoices and propose a collection plan that I can approve.'],
  ['support', 'Prioritaskan tiket pelanggan yang terlambat dan usulkan pembagian pekerjaan tim.', 'Prioritize overdue support cases and propose how the team should handle them.'],
  ['organizations', 'Periksa organisasi pelanggan dan sarankan pembaruan hubungan bisnis yang perlu ditinjau.', 'Review customer organizations and propose changes to their business relationships.'],
  ['proposals', 'Tinjau proposal aktif dan siapkan rencana perubahan scope untuk persetujuan saya.', 'Review active proposals and prepare a scope-change plan for my approval.'],
  ['projects', 'Periksa milestone proyek yang berisiko terlambat dan susun rencana pemulihannya.', 'Review project milestones at risk and prepare a recovery plan.'],
  ['foodservice', 'Periksa pesanan restoran dan bahan yang kurang lalu usulkan pekerjaan persiapan hari ini.', 'Review restaurant orders and ingredient shortages and propose today’s preparation work.'],
  ['scheduling', 'Cari jadwal resource yang bentrok dan ajukan pilihan pemindahan reservasi.', 'Find conflicting resource bookings and propose reservation changes.'],
  ['accommodation', 'Periksa kedatangan tamu besok dan kesiapan kamar lalu buat rencana housekeeping.', 'Review tomorrow’s guest arrivals and room readiness and plan housekeeping work.'],
  ['assets', 'Periksa aset yang butuh perawatan lalu susun jadwal dan tindakan untuk persetujuan.', 'Review assets needing maintenance and prepare a proposed maintenance schedule.'],
  ['subscriptions', 'Periksa langganan bermasalah dan susun tindak lanjut dengan dampaknya pada entitlement.', 'Review problematic subscriptions and propose follow-up work and entitlement impacts.'],
  ['attendance', 'Periksa sesi dan peserta besok lalu ajukan rencana penanganan kapasitas berlebih.', 'Review tomorrow’s sessions and attendees and propose how to handle excess capacity.'],
  ['laundry', 'Periksa pesanan laundry yang belum siap lalu prioritaskan pekerjaan prosesnya.', 'Review unfinished laundry orders and prioritize their processing work.'],
  ['field', 'Periksa pekerjaan teknisi lapangan besok dan ajukan rencana dispatch yang sesuai.', 'Review tomorrow’s field-service jobs and propose a suitable dispatch plan.'],
  ['fulfillment', 'Periksa pengiriman yang tertunda dan susun rencana penyelesaian fulfillment.', 'Review delayed shipments and propose a fulfillment recovery plan.'],
  ['education', 'Periksa enrollment dan sesi belajar lalu usulkan tindak lanjut siswa yang tertinggal.', 'Review enrollments and learning sessions and propose follow-ups for students falling behind.'],
].flatMap(([domain, id, en]) => [{ id: `GOAL-${domain}-id`, domain, language: 'id', utterance: id }, { id: `GOAL-${domain}-en`, domain, language: 'en', utterance: en }])
export const crossDomainGoals = [
  ['retail-replenishment', ['commerce', 'purchasing'], 'Periksa stok dan penawaran pemasok, bandingkan biaya, lalu ajukan pembelian yang dapat saya setujui.'],
  ['sales-to-cash', ['crm', 'commerce', 'billing'], 'Review customer orders, available inventory and outstanding invoices, then propose the next sales-to-cash actions.'],
  ['hotel-readiness', ['accommodation', 'scheduling', 'laundry'], 'Periksa reservasi tamu, jadwal kamar dan laundry linen, lalu buat rencana kesiapan hotel besok.'],
  ['service-to-invoice', ['field', 'assets', 'billing'], 'Review completed service jobs and equipment maintenance, then propose eligible invoicing work.'],
  ['learning-membership', ['education', 'attendance', 'subscriptions'], 'Periksa kelas, kehadiran dan hak langganan siswa lalu ajukan tindak lanjut yang sesuai.'],
  ['milestone-billing', ['projects', 'proposals', 'billing'], 'Review delivered project milestones against approved proposal scope and propose the next invoice.'],
].map(([id, domains, utterance]) => ({ id: `CROSS-${id}`, domains, utterance }))
export const edgeGoals = [
  ['ambiguous', 'Yang kemarin tolong urusin ya.'],
  ['missing-budget', 'Belikan stok yang dibutuhkan, tapi saya belum menentukan anggarannya.'],
  ['conflicting-constraints', 'Buat semua kebutuhan pembelian tapi jangan membuat dokumen atau mengubah data apa pun.'],
  ['changed-intent', 'Batalkan rencana sebelumnya; prioritaskan pemasok berbeda dan minta saya meninjau lagi.'],
  ['remember-preference', 'Ingat preferensi usahaku: rencana pembelian perlu memperlihatkan supplier dan biaya sebelum persetujuan.'],
  ['recall-preference', 'Apa preferensi pembelian yang baru saya sampaikan?'],
  ['task-status', 'Apa saja pekerjaan yang sudah kamu selesaikan dan mana yang masih menunggu persetujuan?'],
  ['partial-failure', 'Kalau salah satu tugas gagal, jelaskan hasil yang sudah tersimpan dan langkah pemulihannya.'],
].map(([id, utterance]) => ({ id: `EDGE-${id}`, utterance }))
export const supportedCommands = [
  ['crm-overview-id', 'ringkas kondisi CRM', 'overview'],
  ['crm-overview-en', 'show crm overview', 'overview'],
  ['contacts', 'list contacts', 'contacts'],
  ['followups', 'list follow-ups', 'followups'],
  ['pipeline', 'list opportunities', 'opportunities'],
].map(([id, utterance, kind]) => ({ id: `CONTROL-${id}`, utterance, kind }))
