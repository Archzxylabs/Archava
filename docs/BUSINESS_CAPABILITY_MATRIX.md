# Peta pola bisnis dan kapabilitas ARCHZXY / Archava

**Versi perencanaan:** 1, 8 Oktober 2026. **Baseline kode:** `1fad26a`.
**Dasar:** [riset dan keputusan](BUSINESS_MODEL_RESEARCH_2026-10-08.md),
[sumber primer](audit/2026-10-08-business-model-sources.json), PRD §§41–49.

Peta ini mencakup 13 pola operasi, 20 contoh konteks industri, dan 25 kapabilitas
bisnis. Ini katalog discovery yang dapat berkembang. Baris kandidat belum menjadi
implementasi, harga, runtime module id, dependency graph, atau janji delivery.

[Riset starter kit 10 Oktober](STARTER_KIT_COVERAGE_RESEARCH_2026-10-10.md) menambahkan
katalog kandidat 40 keluarga kit (24 bisnis umum + 16 sektor khusus), komposisi,
status dan celah terhadap 22 kategori KBLI 2025. Peta pola di bawah tetap menjadi
dasar discovery; jumlah pola, kapabilitas, kit dan template mempunyai denominator
berbeda. Katalog baru tidak mengaktifkan runtime atau menyatakan kit sudah selesai.

## Empat sumbu discovery

| Sumbu             | Pertanyaan                                                 | Contoh                                                                               |
| ----------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Operasi           | Apa yang dikerjakan bisnis untuk melayani pelanggan?       | Menjual barang, mengerjakan proyek, menyewakan aset, menyediakan appointment.        |
| Pendapatan        | Bagaimana bisnis dibayar?                                  | Per transaksi, fixed project, time/material, recurring, usage, commission, donation. |
| Channel dan aktor | Di mana transaksi terjadi dan siapa yang mengakses sistem? | B2B/B2C, toko/POS, web, mobile, marketplace, staff/owner/customer.                   |
| Domain industri   | Record/aturan khusus apa yang dibutuhkan?                  | Room-night hotel, program sekolah, patient record, BOM manufaktur.                   |

Contoh: gym menggabungkan **B08 subscription + B03 appointment + B01 retail**.
Hotel dapat memakai **B07 accommodation + B10 food service**. Subscription box
dapat memakai **B08 subscription + B01 retail + procurement/inventory**. Gabungan
tersebut adalah contoh desain; validasi kebutuhan menentukan capability aktual.

## 13 pola operasi

Semua profil membutuhkan kontrol tenant, identitas yang sesuai, akses resource,
validasi, audit/recovery, dan health yang sebanding dengan risiko. CRM dan Archava
ditambahkan jika workflow membutuhkannya; tidak wajib menjadi dependency semua
modul bisnis. Capability yang disebut di bawah adalah target yang perlu dipilih.

| ID  | Pola dan contoh                     | Alur inti                                                  | Kandidat reusable                                            | Batas domain / bukti referensi                                                                                                                                                                                                              |
| --- | ----------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B01 | Penjualan barang; retail/e-commerce | Produk → cart/order → alokasi → fulfillment → return       | Catalog, orders, inventory, invoicing, payment               | Item/variant, per lokasi, partial fulfillment; cart/checkout adalah kebutuhan channel. [Medusa](https://docs.medusajs.com/learn/fundamentals/modules/commerce-modules).                                                                     |
| B02 | Distribusi/wholesale B2B            | Quote/order → purchase/stock → delivery → invoice          | Accounts, pipeline, catalog, orders, procurement, inventory  | Price list, units, credit terms dan partial delivery harus eksplisit. [ERPNext Sales Order](https://docs.frappe.io/erpnext/sales-order).                                                                                                    |
| B03 | Jasa appointment; salon/konsultasi  | Pilih layanan → slot/resource → hold/confirm → completion  | Scheduling, service catalog, payment refs                    | Kapasitas staff/resource, timezone, buffer, cancellation. [Odoo Appointments](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/productivity/appointments.rst).                                                |
| B04 | Jasa proyek; studio/agency          | Discovery → scope → proposal → project → review/change     | Pipeline, scope/proposals, project delivery, capacity        | Fixed scope dan time/material punya aturan billing berbeda. [Odoo Project](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/services/project.rst), [ERPNext Project](https://docs.frappe.io/erpnext/project). |
| B05 | Field service; teknisi/instalasi    | Request → assignment → kunjungan → work record             | Service tasks, scheduling, asset refs, materials             | Site/work evidence, perjalanan dan parts usage sebagai extension. [Odoo tasks](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/services/field_service/creating_tasks.rst).                                   |
| B06 | Sewa aset; kendaraan/peralatan      | Availability → reservation → handover → return             | Asset register, rental, scheduling, payment refs             | Kondisi aset, return overdue, deposit/damage dan buffer. [Odoo Rental](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/sales/rental.rst).                                                                    |
| B07 | Akomodasi; hotel/villa              | Room-night availability → reservation → stay → checkout    | Scheduling primitives, accommodation extension, payment refs | Room type/unit assignment, housekeeping, room status. [Oracle room management](https://docs.oracle.com/en/industries/hospitality/opera-cloud/25.3/ocsuh/c_housekeeping_room_management.htm).                                                |
| B08 | Subscription/membership; SaaS/gym   | Signup → recurring lifecycle → access/usage → renewal      | Subscription/usage, entitlement, invoicing/payment adapters  | Status pembayaran, cancellation, usage dan access tidak boleh ditebak. [Stripe subscriptions](https://docs.stripe.com/billing/subscriptions/overview), [usage](https://docs.stripe.com/billing/subscriptions/usage-based).                  |
| B09 | Program/cohort pendidikan; kursus   | Admission → enrollment → schedule → attendance/results     | Education extension, scheduling, fee/payment refs            | Academic term, enrollment dan guardian/student access. [Frappe enrollment](https://docs.frappe.io/education/program-enrollment).                                                                                                            |
| B10 | Food service; restoran/cafe         | Menu/order → meja/kitchen → serve → settle                 | Catalog, orders/POS, inventory, foodservice extension        | Table/service/kitchen lifecycle; resep/consumption perlu aturan tambahan. [Odoo restaurant](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/sales/point_of_sale/restaurant.rst).                             |
| B11 | Produksi; manufaktur                | Demand → BOM/planning → work order → consume/produce       | Catalog, purchasing, inventory, manufacturing extension      | Materials, operations, quantity/yield dan costing berbeda dari task biasa. [ERPNext BOM](https://docs.frappe.io/erpnext/bill-of-materials).                                                                                                 |
| B12 | Marketplace/broker platform         | Seller onboarding → transaction → fee/settlement → dispute | Catalog/service refs, orders, marketplace adapter            | Beberapa pemilik dana/data; settlement, refund, dan reconciliation. [Stripe Connect](https://docs.stripe.com/connect).                                                                                                                      |
| B13 | Fundraising/donation; nonprofit     | Donor → pledge/donation → verified receipt → allocation    | Donor/fundraising extension, payment refs, reporting         | Pledge berbeda dari dana diterima; campaign/restriction sesuai kontrak. [ERPNext Donation](https://docs.frappe.io/erpnext/donation).                                                                                                        |

Pola operasi ini adalah sintesis ARCHZXY, bukan taxonomy resmi yang diwajibkan
vendor. Kombinasi dapat berubah berdasarkan discovery dan sumber data klien.

## 25 kapabilitas bisnis dan batas kepemilikan

`crm_contacts` dan `crm_sales_pipeline` memiliki implementasi lokal. Pipeline
ditambahkan pada 9 Oktober sebagai ekstensi domain CRM; lihat
[scope dan migrasi](CRM_PIPELINE.md). Project tracking ditambahkan pada 11 Oktober; lihat [scope dan integrasi](PROJECT_DELIVERY_MODULE.md). ID lain di tabel adalah label kandidat
dokumen. Tabel ini tidak mengaktifkan module dalam config, registry atau pricing
catalog. Dependensi kandidat tetap konseptual sampai slice disepakati dan dibangun.

| Label                  | Record / aturan yang dimiliki                                     | Reuse atau hubungan                                                   | Status / arah                                                                     |
| ---------------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| crm_contacts           | Contact, immutable activity, follow-up; scope/revision/audit      | Hubungan bisnis berizin; dua konteks sudah diuji                      | **Terbukti lokal**, scope terbatas; bukan CRM lengkap.                            |
| crm_sales_pipeline     | Opportunity, stage, inherited owner, immutable transition history | Referensi contact; label config mempertahankan transition guard       | **Terbukti lokal** di dua consumer inquiry; bukan booking/project/payment.        |
| organization_accounts  | Organization, contact affiliation                                 | B2B/account ownership; organization text saat ini belum model account | Kondisional pada pipeline B2B.                                                    |
| project_delivery       | Project, task, milestone, revision dan audit                      | Scope internal dan ownership sendiri; tidak tergantung CRM/LLM        | **Terbukti lokal** untuk tracking; dependency graph, proposal dan capacity belum. |
| delivery_capacity      | Capacity/reservation, delivery queue, estimate evidence           | Project delivery; estimasi tidak menjadi janji otomatis               | Kandidat bersamaan jika queue benar-benar dibutuhkan.                             |
| proposals_scope        | Scope version, proposal acceptance, amendment reference           | Memakai quote engine saat sesuai; signature port terpisah             | Planned; bekukan isi/versi yang disetujui.                                        |
| invoicing              | Invoice snapshot, numbering, line/currency, correction refs       | Order/project/subscription mengirim billing intent                    | Planned; accounting/tax lokal dievaluasi lewat backend/adapter khusus.            |
| payment_processing     | Payment attempt/event, verification, reconciliation, refund ref   | Provider authoritative; bind tenant/order/amount/currency             | Planned; payment success/receipt belum terbukti.                                  |
| catalog_products       | Product/service identity, attributes, variant refs                | Channel/pattern memakai public service yang sama                      | Planned; physical stock bukan milik catalog.                                      |
| commerce_orders        | Order lines, totals snapshot, fulfillment/return lifecycle        | Catalog, inventory/payment lewat aplikasi orkestrasi                  | Planned; payment processing tetap terpisah.                                       |
| inventory_quantities   | Stock movements/reservation per lokasi/item                       | Orders/purchasing/manufacturing; units/lots sesuai kebutuhan          | Later; finance/valuation bukan boolean stock.                                     |
| purchasing             | Supplier, request/order, goods receipt refs                       | Inventory dan billing via service yang dimiliki masing-masing         | Later; approval limits/receipt mismatch eksplisit.                                |
| appointment_scheduling | Resource/time, holds, confirmation, reschedule                    | Users/resource directories; catalog optional                          | **Terbukti lokal** untuk resource eksklusif; recurring/buffer/pooling belum.      |
| asset_register         | Asset identity, condition, maintenance/availability refs          | Rental atau field-service; SKU tidak menggantikan serialized asset    | Later; kebutuhan berbeda dari fixed-assets accounting.                            |
| asset_rental           | Rental terms, handover, return, overdue/damage refs               | Asset register dan scheduling; payment provider optional              | Later; kontrak pengembalian milik rental.                                         |
| accommodation_ops      | Stay/room assignment, housekeeping, room status                   | Shared scheduling/payment refs; domain hotel sendiri                  | Later; PMS adapter layak dievaluasi sebelum native suite.                         |
| subscriptions_usage    | Subscription lifecycle, usage evidence/adjustments                | Payment/invoicing source refs; provider meter jika dipilih            | Later; fixed recurring dan metered berbeda aturan.                                |
| entitlements           | Feature/resource rights, quota, lifecycle policy                  | Subscription/customer references; authorization tetap server-side     | Later; pembayaran atau paid tier tidak memberi admin role.                        |
| support_cases          | Ticket, status/priority, routing, SLA applicability               | Contact/knowledge/integration refs                                    | Later atau adapter; follow-up task bukan ticketing penuh.                         |
| education_programs     | Student/program/course, term/enrollment/access                    | Scheduling dan fees; pisahkan guardian/student dari sales contacts    | Extension khusus; mulai dari kursus jika demand ada.                              |
| foodservice_ops        | Table/service/kitchen lifecycle, menu availability                | Catalog/order/POS; recipe consumption jika diperlukan                 | Extension khusus; assess POS existing.                                            |
| manufacturing_ops      | BOM version, production/work order, yield/material refs           | Inventory/procurement; specialized quality/costing                    | **DEFER**, evaluate mature ERP first.                                             |
| marketplace_settlement | Seller scope, fee contract, transfer/settlement evidence          | Multi-party provider adapter, orders/service refs                     | **DEFER**, bukan checkout satu merchant.                                          |
| fundraising            | Donor relation, pledge/donation, campaign allocation              | Payment refs; recurring optional                                      | Later, profil nonprofit tervalidasi.                                              |
| clinical_admin         | Patient-facing scheduling/admin refs dan akses khusus             | Link ke patient system via authorized service; scope terseleksi       | **DEFER**, clinical record/EHR bukan generic CRM.                                 |

Pembagian ini merupakan rancangan ownership kita. Fakta vendor mendukung adanya
workflow terkait, bukan keharusan bahwa 25 label tersebut harus menjadi 25 package.

Platform controls, Archava core, SDK, knowledge, identity adapters, notifications,
and observability merupakan lapisan lintas kapabilitas; tidak dihitung sebagai
25 modul bisnis. Status platform tetap di [BUILD_STATUS](BUILD_STATUS.md).

## 20 contoh konteks industri

Tabel ini membantu discovery, bukan template terpasang. Semua capability selain
slice CRM yang sudah diverifikasi tetap target. Aturan organisasi/customer portal
dan industri harus diuji sebelum menyatakan suatu kit didukung.

| Konteks                        | Pola operasi              | Kapabilitas awal yang layak dievaluasi                  | Extension / batas                                                             |
| ------------------------------ | ------------------------- | ------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Studio/creative agency         | B04                       | CRM, pipeline, scope, project/capacity                  | Approval deliverables dan change request.                                     |
| Konsultan/professional service | B03 + B04                 | CRM, pipeline, appointments/projects                    | Time/material atau fixed engagement.                                          |
| Kontraktor/renovasi            | B04 + B05                 | Pipeline, project/site tasks, procurement refs          | Milestone acceptance, materials/site access.                                  |
| Salon/barbershop/spa           | B03                       | Service catalog, appointment, customer history          | Skill/resource duration, no-show policy.                                      |
| Gym/fitness                    | B08 + B03 + B01           | Membership/access, classes/PT, optional product sales   | Class capacity dan usage; gabungan beberapa pola.                             |
| Klinik front-office            | B03                       | Approved scheduling/admin adapter                       | Patient data/clinical access terpisah; jangan memakai public CRM visibility.  |
| Rumah sakit                    | B03 + clinical extensions | Integrasi sistem rumah sakit yang sudah ada             | Clinical, pharmacy, billing domain perlu audit khusus; deferred native kit.   |
| Hotel/villa                    | B07 + B10 opsional        | Inquiry CRM, PMS/booking adapter                        | Stay, room assignment, housekeeping; room status bukan stok barang.           |
| Rental kendaraan/peralatan     | B06 + B05 opsional        | Assets, rental/scheduling, payment refs                 | Handover/return, condition, deposit settlement.                               |
| Retail fashion/merchandise     | B01                       | Catalog, orders, fulfillment, inventory/POS adapter     | Variants, return/exchange, multi-channel.                                     |
| Grocery/perishable retail      | B01 + B02                 | Inventory/commerce provider, procurement                | Units, expiry/batch, waste rules jika dibutuhkan.                             |
| Distributor/wholesale          | B02                       | B2B accounts, orders, purchase/delivery refs            | Credit terms, price list, warehouse ownership.                                |
| Restoran/cafe                  | B10                       | POS/order integration, menu, service/kitchen            | Tables, recipe consumption dan settlement.                                    |
| Kursus/cohort training         | B09 + B08 opsional        | Program/enrollment, scheduling, fee/payment refs        | Attendance/results/access policy.                                             |
| Sekolah/universitas            | B09                       | Integrasi education/SIS/LMS terpilih                    | Academic term, guardian/student access, admissions.                           |
| SaaS                           | B08 + B12 opsional        | Subscription, usage, entitlement, support               | Customer organization, tenant provisioning; platform-as-marketplace optional. |
| Creator/content membership     | B08 + B01 digital         | Catalog/access, subscription, payment                   | Content rights/license/revocation; physical stock optional.                   |
| Marketplace jasa/barang        | B12 + B01/B03             | Seller/provider integration dan scoped transaction refs | Multi-party settlement/disputes; deferred native build.                       |
| Nonprofit/community            | B13 + B08 opsional        | Donor/member, campaign, verified payment refs           | Pledge/allocation/audit; kebutuhan bukan profit sales funnel yang identik.    |
| Manufaktur                     | B11 + B02                 | Mature ERP integration untuk inventory/production       | BOM/work orders/materials/costing; deferred native build.                     |

Profil clinical tetap memerlukan audit khusus untuk resource dan aksesnya.
Inventory farmasi memerlukan discovery atas procurement/stock, batch/expiry,
dan kewenangan terkait; daftar umum ini belum menetapkan aturan klinis.
[FHIR Patient](https://hl7.org/fhir/R5/patient.html).

## Konfigurasi, extension, atau integrasi?

| Perubahan yang diminta                                                 | Tempat yang sesuai                                                                   |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Brand, locale, nama tampilan “Guest” / “Client”                        | Config yang didukung; resource/permissions tidak berubah.                            |
| Stage sales atau jenis layanan                                         | Config setelah transition/capacity rules disetujui; bukan arbitrary workflow engine. |
| Patient record, room-night policy, academic enrollment, production BOM | Domain extension dengan resource dan permissions sendiri.                            |
| Klien sudah punya inventory/payment/PMS/ERP                            | Scoped adapter ke authoritative source yang disepakati.                              |
| Sesuatu hanya dibutuhkan satu klien                                    | Extension terisolasi; generalisasi setelah skenario kedua menguatkan kebutuhan.      |

Frappe menunjukkan hubungan enrollment/fees; HL7 menunjukkan resource Patient.
Itu mendukung batas domain, bukan klaim bahwa mengganti label contact cukup.
[Education fees](https://docs.frappe.io/education/fees),
[FHIR R5 Patient](https://hl7.org/fhir/R5/patient.html).

## Cara memakai katalog untuk proyek berikutnya

1. Catat pola, pendapatan, channel/aktor, industri, serta authoritative system.
2. Pilih capability minimum; tandai existing-local, candidate, adapter, atau bespoke.
3. Tulis record owner, state transitions, aturan yang tidak boleh diubah, permission
   matrix, operasi Archava read/prepare/execute, dan failure/recovery cases.
4. Bedakan dependencies wajib, optional integration, dan saran package komersial.
5. Validasi capability pada dua skenario; buat exact tested release profile.
6. Harga memakai sumber komersial yang berwenang. Native scope yang belum dihargai
   masuk manual review; katalog ini tidak memberikan harga gratis/otomatis.
7. Baru setelah gate terpenuhi, tandai supported kit dan update bukti status.

**Gate generalisasi:** dua skenario dengan aturan domain yang dapat dinyatakan,
service/API tanpa LLM, consumer builds tanpa alias/fork, authorization negatif,
migration/recovery untuk data persisten, serta pemilik support dan biaya operasi.
Custom field atau label yang berbeda tidak cukup untuk membuktikan seluruh bisnis
sudah didukung. Tidak ada target 80–90% reuse yang diklaim sudah terukur.

## Prioritas untuk tim kecil

Selesaikan gate operasional pada slice yang ada. **crm_sales_pipeline** sudah
memiliki bukti lokal pada dua consumer inquiry. **project_delivery** kini juga
memiliki bukti untuk tracking internal di dua consumer. Slice berikutnya adalah
scheduling/resource reservations; operasi hotel tetap memerlukan domain tersendiri.
Scheduling, invoicing/payment, atau commerce dipilih mengikuti pelanggan pertama
yang kebutuhannya berulang. Clinical EHR, universal accounting/payroll, MRP penuh,
dan settlement marketplace native tetap ditunda. Website ARCHZXY berada di proyek
terpisah dan kelak mengonsumsi capability ini lewat API/SDK.

Checkpoint 11 October profile 1.5.0: [resource reservations](RESOURCE_RESERVATIONS_MODULE.md)
membuktikan overlap/expiry/revision/restore dan reuse appointment/rental tanpa fork.
Kapasitas satu dan interval eksplisit bukan penerimaan penuh calendar/rental/hotel kit.
