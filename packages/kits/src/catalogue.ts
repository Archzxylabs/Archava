/** Trusted first-party planning inventory. Candidate manifests are not implementations. */
export const KIT_CATALOGUE_VERSION = '1.0.0' as const
export const KIT_CATALOGUE_DATA = [
  {
    id: 'K01',
    slug: 'accommodation',
    label: 'Hotel, villa, homestay',
    group: 'general_business',
    sectorCodes: ['I'],
    capabilities: ['crm_contacts', 'appointment_scheduling', 'invoicing', 'payment_processing'],
    domainRequirement: 'domain.accommodation',
    workflow: 'Inquiry → reservation → stay → checkout',
    invariants: [
      'Night-based availability dan room assignment harus konsisten.',
      'Housekeeping/maintenance state mempengaruhi availability; OTA mempunyai authoritative mapping.',
    ],
    extensionContracts: ['room-night', 'stay/check-in-out', 'housekeeping'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S11', 'S10'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['hospitality'],
    status: 'candidate',
    specification: {
      recordOwner: 'accommodation',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'accommodation.api.v1',
      eventContract: 'accommodation.events.v1',
      uiContract: 'accommodation.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K02',
    slug: 'foodservice',
    label: 'Restaurant, cafe, food service',
    group: 'general_business',
    sectorCodes: ['I'],
    capabilities: [
      'catalog_products',
      'commerce_orders',
      'inventory_quantities',
      'invoicing',
      'payment_processing',
    ],
    domainRequirement: 'domain.foodservice',
    workflow: 'Order → prepare → serve/pickup → settle',
    invariants: [
      'Kitchen ticket/status berbeda dari status pembayaran.',
      'Split bill, cancelled item dan bahan terpakai memerlukan adjustment jelas.',
    ],
    extensionContracts: ['table/service', 'kitchen routing', 'recipe/modifiers'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S12', 'S13', 'S32'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['restaurant'],
    status: 'candidate',
    specification: {
      recordOwner: 'foodservice',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'foodservice.api.v1',
      eventContract: 'foodservice.events.v1',
      uiContract: 'foodservice.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K03',
    slug: 'retail-commerce',
    label: 'Retail dan e-commerce',
    group: 'general_business',
    sectorCodes: ['G'],
    capabilities: [
      'catalog_products',
      'commerce_orders',
      'inventory_quantities',
      'payment_processing',
      'fulfillment_dispatch',
    ],
    domainRequirement: 'domain.retail-commerce',
    workflow: 'Browse → order → allocate → fulfill → return',
    invariants: [
      'Stock reserve dan refund tidak boleh diduplikasi pada retry.',
      'Online/POS merupakan channel; offline writes memerlukan conflict policy tersendiri.',
    ],
    extensionContracts: ['variant/checkout', 'return/exchange'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S05', 'S32'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['ecommerce'],
    status: 'candidate',
    specification: {
      recordOwner: 'retail-commerce',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'retail-commerce.api.v1',
      eventContract: 'retail-commerce.events.v1',
      uiContract: 'retail-commerce.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K04',
    slug: 'wholesale-distribution',
    label: 'Distributor dan wholesale B2B',
    group: 'general_business',
    sectorCodes: ['G'],
    capabilities: [
      'organization_accounts',
      'catalog_products',
      'commerce_orders',
      'purchasing',
      'inventory_quantities',
      'invoicing',
      'fulfillment_dispatch',
    ],
    domainRequirement: 'domain.wholesale-distribution',
    workflow: 'Quote → confirmed order → procure/allocate → partial delivery → invoice',
    invariants: [
      'Ordered/delivered/invoiced quantity dan unit conversion harus dapat direkonsiliasi.',
      'Customer-specific terms tidak memberi izin otomatis mengubah ledger.',
    ],
    extensionContracts: ['price list/UOM', 'partial supply/credit terms'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S06', 'S19'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'wholesale-distribution',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'wholesale-distribution.api.v1',
      eventContract: 'wholesale-distribution.events.v1',
      uiContract: 'wholesale-distribution.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K05',
    slug: 'project-services',
    label: 'Agency, studio, jasa proyek',
    group: 'general_business',
    sectorCodes: ['N', 'K'],
    capabilities: [
      'crm_contacts',
      'crm_sales_pipeline',
      'proposals_scope',
      'project_delivery',
      'delivery_capacity',
      'invoicing',
    ],
    domainRequirement: 'domain.project-services',
    workflow: 'Lead → accepted scope → project → review → completion',
    invariants: [
      'Perubahan scope/timeline mempunyai persetujuan dan version reference.',
      'Task/follow-up CRM tidak menggantikan project acceptance.',
    ],
    extensionContracts: ['deliverable acceptance', 'change request'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S08', 'S35'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['professional_services'],
    status: 'candidate',
    specification: {
      recordOwner: 'project-services',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'project-services.api.v1',
      eventContract: 'project-services.events.v1',
      uiContract: 'project-services.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K06',
    slug: 'professional-cases',
    label: 'Layanan profesional berbasis kasus',
    group: 'general_business',
    sectorCodes: ['N'],
    capabilities: [
      'crm_contacts',
      'support_cases',
      'project_delivery',
      'proposals_scope',
      'invoicing',
    ],
    domainRequirement: 'domain.professional-cases',
    workflow: 'Intake → engagement → restricted case work → review → close',
    invariants: [
      'Akses per matter/client harus eksplisit.',
      'Alur operasional tidak memberi otoritas legal/audit/accounting advice.',
    ],
    extensionContracts: ['matter/engagement', 'document confidentiality/deadline'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S35', 'S36'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['professional_services'],
    status: 'candidate',
    specification: {
      recordOwner: 'professional-cases',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'professional-cases.api.v1',
      eventContract: 'professional-cases.events.v1',
      uiContract: 'professional-cases.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K07',
    slug: 'personal-appointments',
    label: 'Salon, spa, appointment personal care',
    group: 'general_business',
    sectorCodes: ['T'],
    capabilities: [
      'catalog_products',
      'appointment_scheduling',
      'crm_contacts',
      'payment_processing',
    ],
    domainRequirement: 'domain.personal-appointments',
    workflow: 'Choose service → reserve staff/time → visit → completion',
    invariants: [
      'Overlap, capacity, timezone dan buffer divalidasi pada commit.',
      'No-show/reschedule berbeda dari payment refund.',
    ],
    extensionContracts: ['practitioner/service duration', 'buffer/no-show'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S07'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'personal-appointments',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'personal-appointments.api.v1',
      eventContract: 'personal-appointments.events.v1',
      uiContract: 'personal-appointments.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K08',
    slug: 'fitness-membership',
    label: 'Gym, club, membership dengan kelas',
    group: 'general_business',
    sectorCodes: ['S', 'T'],
    capabilities: [
      'subscriptions_usage',
      'entitlements',
      'appointment_scheduling',
      'attendance',
      'payment_processing',
    ],
    domainRequirement: 'domain.fitness-membership',
    workflow: 'Enroll → membership → book class → check-in → renew',
    invariants: [
      'Class capacity dan membership access merupakan aturan berbeda.',
      'Past-due dan pass consumption mengikuti approved policy.',
    ],
    extensionContracts: ['class passes', 'member check-in'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S34', 'S14'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'fitness-membership',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'fitness-membership.api.v1',
      eventContract: 'fitness-membership.events.v1',
      uiContract: 'fitness-membership.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K09',
    slug: 'digital-subscription',
    label: 'SaaS dan produk digital berlangganan',
    group: 'general_business',
    sectorCodes: ['J', 'K'],
    capabilities: ['subscriptions_usage', 'entitlements', 'payment_processing', 'support_cases'],
    domainRequirement: 'domain.digital-subscription',
    workflow: 'Signup → provision → usage → renew/change/cancel',
    invariants: [
      'Payment, subscription dan access bukan satu status.',
      'Provisioning produk harus lewat adapter; software produk itu sendiri tidak otomatis dibuat.',
    ],
    extensionContracts: ['seat/metering', 'digital provisioning'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S14'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'digital-subscription',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'digital-subscription.api.v1',
      eventContract: 'digital-subscription.events.v1',
      uiContract: 'digital-subscription.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K10',
    slug: 'asset-rental',
    label: 'Sewa kendaraan, alat, dan barang',
    group: 'general_business',
    sectorCodes: ['O'],
    capabilities: [
      'asset_register',
      'appointment_scheduling',
      'inventory_quantities',
      'invoicing',
      'payment_processing',
    ],
    domainRequirement: 'domain.asset-rental',
    workflow: 'Reserve asset → handover → use → inspect return → settle',
    invariants: [
      'Serial asset dan jumlah SKU tidak dapat dipertukarkan.',
      'Late return, damage dan deposit mempunyai evidence/approval.',
    ],
    extensionContracts: ['handover/return', 'condition/deposit'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S10'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'asset-rental',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'asset-rental.api.v1',
      eventContract: 'asset-rental.events.v1',
      uiContract: 'asset-rental.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K11',
    slug: 'property-sales',
    label: 'Agency real estate dan penjualan properti',
    group: 'general_business',
    sectorCodes: ['M'],
    capabilities: [
      'crm_contacts',
      'crm_sales_pipeline',
      'catalog_products',
      'appointment_scheduling',
      'proposals_scope',
    ],
    domainRequirement: 'domain.property-sales',
    workflow: 'Inquiry → viewing → offer → agreement reference',
    invariants: [
      'Listing availability dan transaksi legal memakai authoritative source.',
      'Mortgage/conveyancing tidak dianggap fitur CRM yang tersedia.',
    ],
    extensionContracts: ['listing/viewing', 'offer/transaction reference'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S18', 'S04'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['real_estate'],
    status: 'candidate',
    specification: {
      recordOwner: 'property-sales',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'property-sales.api.v1',
      eventContract: 'property-sales.events.v1',
      uiContract: 'property-sales.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K12',
    slug: 'property-operations',
    label: 'Pengelolaan sewa properti dan coworking',
    group: 'general_business',
    sectorCodes: ['M', 'O'],
    capabilities: [
      'asset_register',
      'subscriptions_usage',
      'appointment_scheduling',
      'invoicing',
      'support_cases',
    ],
    domainRequirement: 'domain.property-operations',
    workflow: 'Lease/reserve → occupancy → billing → maintenance → exit',
    invariants: [
      'Long-term lease berbeda dari hotel room-night.',
      'Arrears, deposit dan common costs memerlukan approved terms.',
    ],
    extensionContracts: ['lease/occupancy', 'tenant/service charge'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S18', 'S10'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['real_estate'],
    status: 'candidate',
    specification: {
      recordOwner: 'property-operations',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'property-operations.api.v1',
      eventContract: 'property-operations.events.v1',
      uiContract: 'property-operations.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K13',
    slug: 'courses-training',
    label: 'Kursus, bootcamp, training cohort',
    group: 'general_business',
    sectorCodes: ['Q'],
    capabilities: ['crm_contacts', 'appointment_scheduling', 'attendance', 'payment_processing'],
    domainRequirement: 'domain.courses-training',
    workflow: 'Apply → enroll → schedule → attend/assess → complete',
    invariants: [
      'Enrollment/learner identity tidak sekadar sales contact label.',
      'Konten/courseware dan sertifikasi mempunyai provider/batas sendiri.',
    ],
    extensionContracts: ['enrollment/cohort', 'learning integration'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S15'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['education'],
    status: 'candidate',
    specification: {
      recordOwner: 'courses-training',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'courses-training.api.v1',
      eventContract: 'courses-training.events.v1',
      uiContract: 'courses-training.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K14',
    slug: 'formal-education',
    label: 'Sekolah dan administrasi pendidikan formal',
    group: 'general_business',
    sectorCodes: ['Q'],
    capabilities: ['appointment_scheduling', 'attendance', 'invoicing', 'payment_processing'],
    domainRequirement: 'domain.formal-education',
    workflow: 'Admission → term enrollment → attendance/results → fees',
    invariants: [
      'Guardian, student dan teacher scopes berbeda.',
      'Grade/academic history serta retention membutuhkan domain review.',
    ],
    extensionContracts: ['academic year/term', 'guardian/student access', 'assessment'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S15'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: ['education'],
    status: 'candidate',
    specification: {
      recordOwner: 'formal-education',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'formal-education.api.v1',
      eventContract: 'formal-education.events.v1',
      uiContract: 'formal-education.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K15',
    slug: 'events-venues',
    label: 'Event, ticketing dan venue',
    group: 'general_business',
    sectorCodes: ['S', 'O'],
    capabilities: [
      'appointment_scheduling',
      'catalog_products',
      'commerce_orders',
      'payment_processing',
      'project_delivery',
    ],
    domainRequirement: 'domain.events-venues',
    workflow: 'Plan → register/sell ticket → check-in → event → reconcile',
    invariants: [
      'Seat/capacity hold dan duplicate scan harus ditangani.',
      'Cancelled event/refund mengikuti binding policy.',
    ],
    extensionContracts: ['event capacity/ticket', 'registration/check-in'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S16'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'events-venues',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'events-venues.api.v1',
      eventContract: 'events-venues.events.v1',
      uiContract: 'events-venues.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K16',
    slug: 'tours-activities',
    label: 'Tour, travel, dan aktivitas wisata',
    group: 'general_business',
    sectorCodes: ['O', 'S'],
    capabilities: [
      'appointment_scheduling',
      'catalog_products',
      'commerce_orders',
      'payment_processing',
      'fulfillment_dispatch',
    ],
    domainRequirement: 'domain.tours-activities',
    workflow: 'Choose departure → reserve → confirm suppliers → operate → close',
    invariants: [
      'Per-departure kapasitas dan supplier confirmation harus jelas.',
      'Airline/GDS/visa bukan native capability yang diasumsikan.',
    ],
    extensionContracts: ['departure/passenger', 'supplier allotment/itinerary'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S04', 'S07'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'tours-activities',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'tours-activities.api.v1',
      eventContract: 'tours-activities.events.v1',
      uiContract: 'tours-activities.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K17',
    slug: 'field-services',
    label: 'Teknisi, instalasi, layanan lapangan',
    group: 'general_business',
    sectorCodes: ['F', 'N', 'O'],
    capabilities: [
      'project_delivery',
      'appointment_scheduling',
      'asset_register',
      'inventory_quantities',
      'invoicing',
    ],
    domainRequirement: 'domain.field-services',
    workflow: 'Request → assign → visit → work evidence → approval → invoice',
    invariants: [
      'Appointment resource dan address/site tidak hilang pada dispatch.',
      'Parts usage dan customer signoff mempunyai records sendiri.',
    ],
    extensionContracts: ['site visit/dispatch', 'work evidence/materials'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S09'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'field-services',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'field-services.api.v1',
      eventContract: 'field-services.events.v1',
      uiContract: 'field-services.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K18',
    slug: 'repair-workshops',
    label: 'Bengkel dan servis barang',
    group: 'general_business',
    sectorCodes: ['T'],
    capabilities: [
      'asset_register',
      'inventory_quantities',
      'purchasing',
      'support_cases',
      'invoicing',
    ],
    domainRequirement: 'domain.repair-workshops',
    workflow: 'Receive asset → inspect/quote → repair → test → return',
    invariants: [
      'Customer-owned serial asset harus dapat ditelusuri.',
      'Warranty coverage tidak disimpulkan dari status paid atau model output.',
    ],
    extensionContracts: ['repair job/diagnosis', 'warranty/parts custody'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S37'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'repair-workshops',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'repair-workshops.api.v1',
      eventContract: 'repair-workshops.events.v1',
      uiContract: 'repair-workshops.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K19',
    slug: 'laundry-processing',
    label: 'Laundry dan dry cleaning',
    group: 'general_business',
    sectorCodes: ['T'],
    capabilities: ['commerce_orders', 'fulfillment_dispatch', 'invoicing', 'payment_processing'],
    domainRequirement: 'domain.laundry-processing',
    workflow: 'Receive/count → tag → process → quality check → return',
    invariants: [
      'Garment count, barcode dan custody harus konsisten.',
      'Kilo-based dan item-based pricing memerlukan config tervalidasi.',
    ],
    extensionContracts: ['garment/item custody', 'processing batch/status'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S30'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'laundry-processing',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'laundry-processing.api.v1',
      eventContract: 'laundry-processing.events.v1',
      uiContract: 'laundry-processing.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K20',
    slug: 'logistics-fulfillment',
    label: 'Gudang, fulfillment dan logistik barang',
    group: 'general_business',
    sectorCodes: ['H'],
    capabilities: [
      'inventory_quantities',
      'commerce_orders',
      'fulfillment_dispatch',
      'asset_register',
      'invoicing',
    ],
    domainRequirement: 'domain.logistics-fulfillment',
    workflow: 'Receive → store → pick/pack → dispatch → proof of delivery',
    invariants: [
      'Barang milik client berbeda tidak boleh tercampur ownership/access.',
      'Carrier updates dan partial/lost delivery memerlukan reconciliation.',
    ],
    extensionContracts: ['shipment/trip', 'stock owner/custody'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S19', 'S05'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'logistics-fulfillment',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'logistics-fulfillment.api.v1',
      eventContract: 'logistics-fulfillment.events.v1',
      uiContract: 'logistics-fulfillment.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K21',
    slug: 'recruitment-staffing',
    label: 'Recruitment dan staffing agency',
    group: 'general_business',
    sectorCodes: ['O'],
    capabilities: [
      'organization_accounts',
      'crm_contacts',
      'appointment_scheduling',
      'support_cases',
      'invoicing',
    ],
    domainRequirement: 'domain.recruitment-staffing',
    workflow: 'Employer request → applicant → interview → placement → follow-up',
    invariants: [
      'Candidate data tidak terbuka untuk semua employer.',
      'Temporary staffing/payroll membutuhkan contract dan adapter tambahan.',
    ],
    extensionContracts: ['job/requisition/applicant', 'placement/assignment'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S17', 'S33'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'recruitment-staffing',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'recruitment-staffing.api.v1',
      eventContract: 'recruitment-staffing.events.v1',
      uiContract: 'recruitment-staffing.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K22',
    slug: 'nonprofit-associations',
    label: 'NGO, donasi dan asosiasi',
    group: 'general_business',
    sectorCodes: ['T', 'O'],
    capabilities: ['crm_contacts', 'payment_processing', 'project_delivery', 'subscriptions_usage'],
    domainRequirement: 'domain.nonprofit-associations',
    workflow: 'Engage → pledge → verified donation → allocate/report',
    invariants: [
      'Pledge tidak berarti dana telah diterima.',
      'Restricted funds, grant conditions dan member roles perlu policy.',
    ],
    extensionContracts: ['donor/campaign/fund allocation', 'member/grant'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S21'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'nonprofit-associations',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'nonprofit-associations.api.v1',
      eventContract: 'nonprofit-associations.events.v1',
      uiContract: 'nonprofit-associations.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K23',
    slug: 'light-production',
    label: 'Produksi sederhana dan made-to-order',
    group: 'general_business',
    sectorCodes: ['C'],
    capabilities: [
      'catalog_products',
      'commerce_orders',
      'inventory_quantities',
      'purchasing',
      'project_delivery',
    ],
    domainRequirement: 'domain.light-production',
    workflow: 'Order/demand → plan materials → produce → inspect → fulfill',
    invariants: [
      'Input/output quantity dan scrap harus traceable.',
      'Advanced MRP/process control diarahkan ke K32, bukan diasumsikan selesai.',
    ],
    extensionContracts: ['simple BOM/work order', 'materials/output'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S20'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'light-production',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'light-production.api.v1',
      eventContract: 'light-production.events.v1',
      uiContract: 'light-production.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K24',
    slug: 'media-publishing',
    label: 'Media, content studio dan penerbit',
    group: 'general_business',
    sectorCodes: ['J'],
    capabilities: ['project_delivery', 'catalog_products', 'subscriptions_usage', 'entitlements'],
    domainRequirement: 'domain.media-publishing',
    workflow: 'Commission → produce → review → publish/distribute → track rights',
    invariants: [
      'Publishing approval dan content ownership harus jelas.',
      'Broadcasting infrastructure, licensing/royalty engines dan game product memerlukan scope khusus.',
    ],
    extensionContracts: ['editorial approval', 'rights/content distribution'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S02', 'S08'],
    implementationRoute: 'evaluate_native_composition_or_existing_system',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'media-publishing',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'media-publishing.api.v1',
      eventContract: 'media-publishing.events.v1',
      uiContract: 'media-publishing.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K25',
    slug: 'outpatient-clinics',
    label: 'Klinik dan dental outpatient',
    group: 'specialist',
    sectorCodes: ['R'],
    capabilities: ['appointment_scheduling', 'invoicing', 'payment_processing'],
    domainRequirement: 'domain.outpatient-clinics',
    workflow: 'Register → appointment → encounter source → bill → follow-up',
    invariants: [
      'Clinical truth tetap di approved patient system.',
      'Patient data/private tools membutuhkan domain-specific authorization.',
    ],
    extensionContracts: ['patient/encounter integration', 'clinical consent/scope'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S22'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: ['clinic'],
    status: 'candidate',
    specification: {
      recordOwner: 'outpatient-clinics',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'outpatient-clinics.api.v1',
      eventContract: 'outpatient-clinics.events.v1',
      uiContract: 'outpatient-clinics.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K26',
    slug: 'inpatient-hospitals',
    label: 'Rumah sakit dan inpatient',
    group: 'specialist',
    sectorCodes: ['R'],
    capabilities: ['asset_register', 'appointment_scheduling', 'invoicing'],
    domainRequirement: 'domain.inpatient-hospitals',
    workflow: 'Admit → allocate bed/service → care source → discharge → reconcile',
    invariants: [
      'Bed assignment, patient movement dan orders mempunyai clinical authority.',
      'Tidak menjanjikan EHR, medical decision atau hospital suite tanpa integration acceptance.',
    ],
    extensionContracts: ['admission/bed transfer', 'discharge/care integration'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S22'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'inpatient-hospitals',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'inpatient-hospitals.api.v1',
      eventContract: 'inpatient-hospitals.events.v1',
      uiContract: 'inpatient-hospitals.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K27',
    slug: 'diagnostics-laboratories',
    label: 'Lab diagnostik dan testing',
    group: 'specialist',
    sectorCodes: ['R', 'N'],
    capabilities: ['commerce_orders', 'invoicing', 'support_cases'],
    domainRequirement: 'domain.diagnostics-laboratories',
    workflow: 'Order → specimen → analyse source → validate → release report',
    invariants: [
      'Specimen identity dan result version tidak dapat ditebak.',
      'Hanya qualified workflow dapat merilis/menyetujui hasil.',
    ],
    extensionContracts: ['specimen/order chain', 'validated result release'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S23', 'S22'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'diagnostics-laboratories',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'diagnostics-laboratories.api.v1',
      eventContract: 'diagnostics-laboratories.events.v1',
      uiContract: 'diagnostics-laboratories.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K28',
    slug: 'pharmacies',
    label: 'Apotek dan regulated health retail',
    group: 'specialist',
    sectorCodes: ['G', 'R'],
    capabilities: [
      'catalog_products',
      'inventory_quantities',
      'commerce_orders',
      'payment_processing',
    ],
    domainRequirement: 'domain.pharmacies',
    workflow: 'Receive lot → validate dispense source → supply → trace/recall',
    invariants: [
      'Expired/recall lots tidak dianggap SKU biasa yang dapat dijual.',
      'Prescription/dispensing policy memerlukan qualified system dan review.',
    ],
    extensionContracts: ['batch/expiry/recall', 'dispensing authorization'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S24', 'S32'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'pharmacies',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'pharmacies.api.v1',
      eventContract: 'pharmacies.events.v1',
      uiContract: 'pharmacies.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K29',
    slug: 'crops-forestry',
    label: 'Pertanian tanaman dan kehutanan',
    group: 'specialist',
    sectorCodes: ['A'],
    capabilities: ['asset_register', 'project_delivery', 'inventory_quantities', 'purchasing'],
    domainRequirement: 'domain.crops-forestry',
    workflow: 'Define productive unit → plan cycle → activities → harvest → trace',
    invariants: [
      'Cycle, lot dan yield mempunyai unit/ownership eksplisit.',
      'Forestry variants belum mempunyai detail workflow tervalidasi.',
    ],
    extensionContracts: ['land/crop/harvest cycle', 'yield/traceability'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S25', 'S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'crops-forestry',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'crops-forestry.api.v1',
      eventContract: 'crops-forestry.events.v1',
      uiContract: 'crops-forestry.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K30',
    slug: 'animal-aquaculture',
    label: 'Peternakan dan perikanan budidaya',
    group: 'specialist',
    sectorCodes: ['A'],
    capabilities: ['asset_register', 'inventory_quantities', 'project_delivery', 'purchasing'],
    domainRequirement: 'domain.animal-aquaculture',
    workflow: 'Define animal/cohort unit → care/feed → movement → harvest',
    invariants: [
      'Living population dan mortality bukan stock adjustment generik.',
      'Domain health, wild fishing dan aquaculture variants membutuhkan expert discovery.',
    ],
    extensionContracts: ['herd/pond/cohort', 'feed/health/harvest'],
    evidenceDepth: 'taxonomy_only',
    sourceIds: ['S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'animal-aquaculture',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'animal-aquaculture.api.v1',
      eventContract: 'animal-aquaculture.events.v1',
      uiContract: 'animal-aquaculture.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K31',
    slug: 'construction-epc',
    label: 'Konstruksi, kontraktor dan EPC',
    group: 'specialist',
    sectorCodes: ['F'],
    capabilities: [
      'proposals_scope',
      'project_delivery',
      'purchasing',
      'inventory_quantities',
      'invoicing',
    ],
    domainRequirement: 'domain.construction-epc',
    workflow: 'Contract → site work → change order → progress acceptance → close',
    invariants: [
      'Approved variations terpisah dari original commitment.',
      'Retention, subcontractor payments dan safety systems mempunyai owner.',
    ],
    extensionContracts: ['site/commitments', 'variation/progress claims'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S31'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'construction-epc',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'construction-epc.api.v1',
      eventContract: 'construction-epc.events.v1',
      uiContract: 'construction-epc.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K32',
    slug: 'industrial-manufacturing',
    label: 'Manufaktur industri dan regulated production',
    group: 'specialist',
    sectorCodes: ['C'],
    capabilities: ['catalog_products', 'inventory_quantities', 'purchasing', 'asset_maintenance'],
    domainRequirement: 'domain.industrial-manufacturing',
    workflow: 'Demand → plan/routing → execute source → quality → release → trace',
    invariants: [
      'Quality release, batch genealogy dan cost records tidak digeneralisasi menjadi CRUD.',
      'MES/process control dan statutory traceability lewat qualified authoritative integrations.',
    ],
    extensionContracts: ['MRP/routing/quality', 'lot genealogy/costing'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S20', 'S32'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'industrial-manufacturing',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'industrial-manufacturing.api.v1',
      eventContract: 'industrial-manufacturing.events.v1',
      uiContract: 'industrial-manufacturing.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K33',
    slug: 'extractive-industries',
    label: 'Pertambangan dan industri ekstraktif',
    group: 'specialist',
    sectorCodes: ['B'],
    capabilities: ['asset_register', 'asset_maintenance', 'purchasing', 'inventory_quantities'],
    domainRequirement: 'domain.extractive-industries',
    workflow: 'Plan site → operational source → production/quality → shipment',
    invariants: [
      'Operational/safety control tetap milik authoritative system.',
      'Mining-specific scheduling/assay/reserve rules belum tervalidasi.',
    ],
    extensionContracts: ['site/production dispatch', 'extraction/quality/safety integration'],
    evidenceDepth: 'taxonomy_only',
    sourceIds: ['S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'extractive-industries',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'extractive-industries.api.v1',
      eventContract: 'extractive-industries.events.v1',
      uiContract: 'extractive-industries.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K34',
    slug: 'utilities',
    label: 'Utilitas listrik, gas dan air',
    group: 'specialist',
    sectorCodes: ['D', 'E'],
    capabilities: [
      'asset_register',
      'asset_maintenance',
      'support_cases',
      'invoicing',
      'subscriptions_usage',
    ],
    domainRequirement: 'domain.utilities',
    workflow: 'Connect service → measure source → validate usage → bill → service case',
    invariants: [
      'Meter correction, estimated usage dan tariff version punya evidence.',
      'Grid/plant control serta critical operations tidak menjadi authority LLM.',
    ],
    extensionContracts: ['meter/service point', 'usage validation/tariff'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S28'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'utilities',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'utilities.api.v1',
      eventContract: 'utilities.events.v1',
      uiContract: 'utilities.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K35',
    slug: 'waste-environment',
    label: 'Pengelolaan limbah dan jasa lingkungan',
    group: 'specialist',
    sectorCodes: ['E', 'N'],
    capabilities: ['fulfillment_dispatch', 'asset_register', 'support_cases', 'invoicing'],
    domainRequirement: 'domain.waste-environment',
    workflow: 'Service request → collect → weigh/classify → treatment source → evidence',
    invariants: [
      'Material classification dan custody membutuhkan verified records.',
      'Hazardous-waste, emissions dan environmental obligations belum tervalidasi.',
    ],
    extensionContracts: ['collection/weight/material classification', 'manifest/treatment chain'],
    evidenceDepth: 'taxonomy_only',
    sourceIds: ['S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'waste-environment',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'waste-environment.api.v1',
      eventContract: 'waste-environment.events.v1',
      uiContract: 'waste-environment.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K36',
    slug: 'finance-lending',
    label: 'Banking, lending dan layanan keuangan',
    group: 'specialist',
    sectorCodes: ['L'],
    capabilities: ['organization_accounts', 'support_cases'],
    domainRequirement: 'domain.finance-lending',
    workflow: 'Verified intake → authoritative financial workflow → servicing/reconcile',
    invariants: [
      'Financial ledger/allocation bukan invoice biasa.',
      'KYC, credit decision dan money movement membutuhkan institution-approved systems.',
    ],
    extensionContracts: ['account/loan integration', 'repayment allocation/ledger'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S26'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'finance-lending',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'finance-lending.api.v1',
      eventContract: 'finance-lending.events.v1',
      uiContract: 'finance-lending.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K37',
    slug: 'insurance',
    label: 'Asuransi dan claims',
    group: 'specialist',
    sectorCodes: ['L'],
    capabilities: ['crm_contacts', 'support_cases'],
    domainRequirement: 'domain.insurance',
    workflow: 'Policy reference → loss intake → assess source → approved settlement',
    invariants: [
      'Coverage bergantung policy/version dan loss event.',
      'Claim submission tidak sama dengan approved settlement.',
    ],
    extensionContracts: ['policy coverage', 'loss/claim/settlement integration'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S27'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'insurance',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'insurance.api.v1',
      eventContract: 'insurance.events.v1',
      uiContract: 'insurance.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K38',
    slug: 'marketplaces',
    label: 'Marketplace dan transaksi multipihak',
    group: 'specialist',
    sectorCodes: ['G', 'O', 'K'],
    capabilities: ['catalog_products', 'commerce_orders', 'payment_processing', 'support_cases'],
    domainRequirement: 'domain.marketplaces',
    workflow: 'Onboard seller → order → split/settle source → payout/dispute',
    invariants: [
      'Platform, seller dan buyer ownership berbeda.',
      'Settlement, refund dan payout selalu provider-authoritative.',
    ],
    extensionContracts: ['seller/account scopes', 'commission/payout/dispute'],
    evidenceDepth: 'workflow_evidence',
    sourceIds: ['S38'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'marketplaces',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'marketplaces.api.v1',
      eventContract: 'marketplaces.events.v1',
      uiContract: 'marketplaces.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K39',
    slug: 'public-services',
    label: 'Layanan publik dan administrasi pemerintahan',
    group: 'specialist',
    sectorCodes: ['P'],
    capabilities: ['support_cases', 'proposals_scope', 'project_delivery'],
    domainRequirement: 'domain.public-services',
    workflow: 'Application → verification → review/decision source → service/appeal',
    invariants: [
      'Statutory decision dan eligibility tidak diputuskan model.',
      'Defence, compulsory social insurance dan sovereign record systems perlu dedicated scope.',
    ],
    extensionContracts: ['constituent/permit/grant', 'records/decision workflow'],
    evidenceDepth: 'partly_documented',
    sourceIds: ['S29', 'S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'public-services',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'public-services.api.v1',
      eventContract: 'public-services.events.v1',
      uiContract: 'public-services.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
  {
    id: 'K40',
    slug: 'social-care',
    label: 'Residential care dan layanan sosial',
    group: 'specialist',
    sectorCodes: ['R'],
    capabilities: ['support_cases', 'appointment_scheduling', 'attendance'],
    domainRequirement: 'domain.social-care',
    workflow: 'Referral → assessed intake source → care placement → review',
    invariants: [
      'Beneficiary/guardian records mempunyai restricted scope.',
      'Care decision, child protection dan resident operations memerlukan domain validation.',
    ],
    extensionContracts: ['beneficiary/care-plan source', 'guardian/referral/placement'],
    evidenceDepth: 'taxonomy_only',
    sourceIds: ['S02'],
    implementationRoute: 'evaluate_authoritative_adapter_first',
    legacyTemplateMatches: [],
    status: 'candidate',
    specification: {
      recordOwner: 'social-care',
      configurationPolicy:
        'closed validated options; executable scripts and caller-defined tools prohibited',
      apiContract: 'social-care.api.v1',
      eventContract: 'social-care.events.v1',
      uiContract: 'social-care.workspace.v1',
      permissionPolicy: 'server-verified principal/resource scope; metadata grants no access',
      actionPolicy:
        'read/prepare through registered tools; effects require registered executor and applicable exact approval',
      migrationPolicy:
        'domain-owned versioned migrations and tested recovery required before persistent support',
      healthPolicy:
        'each required native service or authoritative adapter must have current configuration/readiness evidence',
      failurePolicy:
        'retain canonical intent/audit; reconcile unknown effects; do not infer success',
    },
  },
] as const

export const CAPABILITY_IDS = [
  'crm_contacts',
  'crm_sales_pipeline',
  'organization_accounts',
  'catalog_products',
  'commerce_orders',
  'inventory_quantities',
  'purchasing',
  'invoicing',
  'payment_processing',
  'appointment_scheduling',
  'project_delivery',
  'proposals_scope',
  'delivery_capacity',
  'asset_register',
  'asset_maintenance',
  'subscriptions_usage',
  'entitlements',
  'support_cases',
  'fulfillment_dispatch',
  'attendance',
] as const
