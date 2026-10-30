-- Archava Universal Enterprise Ledgers (10 Pillars Migration)
-- Multi-Tenant PostgreSQL Schema with Mandatory Row Level Security (RLS) and SHA-256 Idempotency

-- ============================================================================
-- Pillar 1: POS & Consignment (archava_pos, archava_consignment)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_pos;

CREATE TABLE archava_pos.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '',
  permissions text[] NOT NULL,
  scope text NOT NULL DEFAULT 'all',
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_pos.shifts (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  cashier_id text NOT NULL,
  register_id text NOT NULL,
  opened_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  closed_at timestamptz,
  opening_cash_minor bigint NOT NULL,
  closing_cash_minor bigint,
  expected_cash_minor bigint,
  status text NOT NULL CHECK(status IN ('open', 'closed', 'reconciled')),
  PRIMARY KEY(tenant_id, id)
);

CREATE TABLE archava_pos.transactions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  shift_id uuid NOT NULL,
  cashier_id text NOT NULL,
  subtotal_minor bigint NOT NULL,
  tax_minor bigint NOT NULL,
  discount_minor bigint NOT NULL DEFAULT 0,
  final_total_minor bigint NOT NULL,
  offline_created_at timestamptz NOT NULL,
  idempotency_key text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  CONSTRAINT pos_tx_idempotency UNIQUE(tenant_id, idempotency_key)
);

ALTER TABLE archava_pos.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_pos.memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_pos.shifts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_pos.shifts FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_pos.transactions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_pos.transactions FORCE ROW LEVEL SECURITY;

CREATE POLICY pos_mem_tenant ON archava_pos.memberships USING (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY pos_shift_tenant ON archava_pos.shifts USING (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY pos_tx_tenant ON archava_pos.transactions USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_consignment;

CREATE TABLE archava_consignment.agreements (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  consignor_id text NOT NULL,
  commission_bps integer NOT NULL CHECK(commission_bps BETWEEN 0 AND 10000),
  settlement_period_days integer NOT NULL,
  status text NOT NULL CHECK(status IN ('draft', 'active', 'suspended', 'terminated')),
  PRIMARY KEY(tenant_id, id)
);

CREATE TABLE archava_consignment.items (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  agreement_id uuid NOT NULL,
  consignor_id text NOT NULL,
  sku text NOT NULL,
  title text NOT NULL,
  retail_price_minor bigint NOT NULL,
  status text NOT NULL CHECK(status IN ('received', 'display', 'sold_pending', 'settled', 'returned')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_consignment.agreements ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_consignment.agreements FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_consignment.items ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_consignment.items FORCE ROW LEVEL SECURITY;

CREATE POLICY cons_agree_tenant ON archava_consignment.agreements USING (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY cons_items_tenant ON archava_consignment.items USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 2: Services, Rental & Salon (archava_rental, archava_salon)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_rental;

CREATE TABLE archava_rental.contracts (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  renter_id text NOT NULL,
  equipment_id text NOT NULL,
  start_at timestamptz NOT NULL,
  agreed_end_at timestamptz NOT NULL,
  actual_returned_at timestamptz,
  daily_rate_minor bigint NOT NULL,
  security_deposit_minor bigint NOT NULL,
  state text NOT NULL CHECK(state IN ('reserved', 'dispatched', 'in_use', 'returned', 'damage_claimed', 'closed')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_rental.contracts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_rental.contracts FORCE ROW LEVEL SECURITY;
CREATE POLICY rental_tenant ON archava_rental.contracts USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_salon;

CREATE TABLE archava_salon.sessions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  customer_id text NOT NULL,
  therapist_id text NOT NULL,
  station_id text NOT NULL,
  service_name text NOT NULL,
  price_minor bigint NOT NULL,
  commission_bps integer NOT NULL CHECK(commission_bps BETWEEN 0 AND 10000),
  status text NOT NULL CHECK(status IN ('booked', 'in_session', 'completed', 'paid', 'canceled')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_salon.sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_salon.sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY salon_tenant ON archava_salon.sessions USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 3: Hospitality & Living (archava_venue)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_venue;

CREATE TABLE archava_venue.reservations (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  space_id text NOT NULL,
  organizer_id text NOT NULL,
  start_at timestamptz NOT NULL,
  end_at timestamptz NOT NULL,
  guest_count integer NOT NULL,
  layout text NOT NULL,
  total_cost_minor bigint NOT NULL,
  deposit_minor bigint NOT NULL,
  status text NOT NULL CHECK(status IN ('inquiry', 'hold', 'deposit_paid', 'settled', 'completed', 'canceled')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_venue.reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_venue.reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY venue_tenant ON archava_venue.reservations USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 4: Discrete MRP & Quality Control (archava_mrp, archava_qc)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_mrp;

CREATE TABLE archava_mrp.production_orders (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  bom_id uuid NOT NULL,
  target_sku text NOT NULL,
  quantity_planned integer NOT NULL,
  quantity_completed integer NOT NULL DEFAULT 0,
  quantity_scrapped integer NOT NULL DEFAULT 0,
  status text NOT NULL CHECK(status IN ('draft', 'released', 'in_production', 'completed', 'closed')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_mrp.production_orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_mrp.production_orders FORCE ROW LEVEL SECURITY;
CREATE POLICY mrp_tenant ON archava_mrp.production_orders USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_qc;

CREATE TABLE archava_qc.orders (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  item_sku text NOT NULL,
  lot_number text NOT NULL,
  stage text NOT NULL CHECK(stage IN ('incoming', 'in_process', 'final')),
  batch_size integer NOT NULL,
  sample_size integer NOT NULL,
  critical_defects integer NOT NULL DEFAULT 0,
  major_defects integer NOT NULL DEFAULT 0,
  minor_defects integer NOT NULL DEFAULT 0,
  verdict text NOT NULL CHECK(verdict IN ('pending', 'accepted', 'rejected', 'quarantined', 'rework')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_qc.orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_qc.orders FORCE ROW LEVEL SECURITY;
CREATE POLICY qc_tenant ON archava_qc.orders USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 5: Healthcare EMR, Queue & Pharmacy (archava_clinic, archava_pharmacy)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_clinic;

CREATE TABLE archava_clinic.encounters (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  patient_id text NOT NULL,
  practitioner_id text NOT NULL,
  chief_complaint text NOT NULL,
  icd10_primary_code text,
  status text NOT NULL CHECK(status IN ('draft', 'in_consultation', 'completed', 'referred')),
  PRIMARY KEY(tenant_id, id)
);

CREATE TABLE archava_clinic.queue_tickets (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  ticket_code text NOT NULL,
  patient_id text NOT NULL,
  poli_room_id text NOT NULL,
  triage_priority integer NOT NULL CHECK(triage_priority IN (1, 2, 3)),
  status text NOT NULL CHECK(status IN ('waiting', 'called', 'in_consultation', 'completed', 'noshow')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_clinic.encounters ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_clinic.encounters FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_clinic.queue_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_clinic.queue_tickets FORCE ROW LEVEL SECURITY;

CREATE POLICY clinic_enc_tenant ON archava_clinic.encounters USING (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY clinic_q_tenant ON archava_clinic.queue_tickets USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_pharmacy;

CREATE TABLE archava_pharmacy.prescriptions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  patient_id text NOT NULL,
  doctor_id text NOT NULL,
  pharmacist_license text,
  allergy_cleared boolean NOT NULL DEFAULT false,
  status text NOT NULL CHECK(status IN ('pending', 'approved', 'dispensed', 'refused')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_pharmacy.prescriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_pharmacy.prescriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY rx_tenant ON archava_pharmacy.prescriptions USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 6: Fintech, Payroll & Expenses (archava_payroll, archava_expenses)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_payroll;

CREATE TABLE archava_payroll.runs (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  period_label text NOT NULL,
  total_gross_minor bigint NOT NULL,
  total_tax_pph21_minor bigint NOT NULL,
  total_bpjs_tk_minor bigint NOT NULL,
  total_bpjs_kes_minor bigint NOT NULL,
  total_take_home_minor bigint NOT NULL,
  status text NOT NULL CHECK(status IN ('draft', 'calculated', 'approved', 'disbursed')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_payroll.runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_payroll.runs FORCE ROW LEVEL SECURITY;
CREATE POLICY payroll_tenant ON archava_payroll.runs USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_expenses;

CREATE TABLE archava_expenses.claims (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  employee_id text NOT NULL,
  category text NOT NULL,
  amount_minor bigint NOT NULL,
  receipt_sha256 text NOT NULL,
  manager_id text,
  status text NOT NULL CHECK(status IN ('draft', 'submitted', 'manager_approved', 'disbursed', 'rejected')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_expenses.claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_expenses.claims FORCE ROW LEVEL SECURITY;
CREATE POLICY expense_tenant ON archava_expenses.claims USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 7: Digital LMS (archava_lms)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_lms;

CREATE TABLE archava_lms.enrollments (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  course_id text NOT NULL,
  student_id text NOT NULL,
  score_percent integer CHECK(score_percent BETWEEN 0 AND 100),
  certificate_sha256 text,
  status text NOT NULL CHECK(status IN ('enrolled', 'in_progress', 'passed', 'failed', 'dropped')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_lms.enrollments ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_lms.enrollments FORCE ROW LEVEL SECURITY;
CREATE POLICY lms_tenant ON archava_lms.enrollments USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 8: Automotive Fleet & Workshop (archava_fleet, archava_workshop)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_fleet;

CREATE TABLE archava_fleet.vehicles (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  license_plate text NOT NULL,
  current_odometer_km integer NOT NULL CHECK(current_odometer_km >= 0),
  status text NOT NULL CHECK(status IN ('active', 'in_maintenance', 'decommissioned')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_fleet.vehicles ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_fleet.vehicles FORCE ROW LEVEL SECURITY;
CREATE POLICY fleet_tenant ON archava_fleet.vehicles USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_workshop;

CREATE TABLE archava_workshop.work_orders (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  spk_number text NOT NULL,
  vehicle_plate text NOT NULL,
  customer_id text NOT NULL,
  odometer_km integer NOT NULL,
  total_bill_minor bigint NOT NULL DEFAULT 0,
  status text NOT NULL CHECK(status IN ('draft', 'diagnosed', 'approved', 'in_progress', 'qc', 'test_drive', 'billed')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_workshop.work_orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_workshop.work_orders FORCE ROW LEVEL SECURITY;
CREATE POLICY workshop_tenant ON archava_workshop.work_orders USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 9: Property & Tenancy (archava_lease)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_lease;

CREATE TABLE archava_lease.contracts (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  property_unit_id text NOT NULL,
  tenant_user_id text NOT NULL,
  monthly_rent_minor bigint NOT NULL,
  monthly_service_charge_minor bigint NOT NULL,
  security_deposit_minor bigint NOT NULL,
  status text NOT NULL CHECK(status IN ('draft', 'active', 'expired', 'settled', 'terminated')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_lease.contracts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_lease.contracts FORCE ROW LEVEL SECURITY;
CREATE POLICY lease_tenant ON archava_lease.contracts USING (tenant_id = current_setting('archava.tenant_id', true));

-- ============================================================================
-- Pillar 10: Creative Studio & Marketplace (archava_canvas, archava_marketplace)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS archava_canvas;

CREATE TABLE archava_canvas.projects (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  owner_id text NOT NULL,
  title text NOT NULL,
  soul_id_anchor text,
  graph_dag_json jsonb NOT NULL,
  status text NOT NULL DEFAULT 'draft',
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_canvas.projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_canvas.projects FORCE ROW LEVEL SECURITY;
CREATE POLICY canvas_tenant ON archava_canvas.projects USING (tenant_id = current_setting('archava.tenant_id', true));

CREATE SCHEMA IF NOT EXISTS archava_marketplace;

CREATE TABLE archava_marketplace.orders (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  store_id text NOT NULL,
  customer_id text NOT NULL,
  subtotal_minor bigint NOT NULL,
  platform_commission_minor bigint NOT NULL,
  merchant_net_minor bigint NOT NULL,
  escrow_state text NOT NULL CHECK(escrow_state IN ('held', 'released', 'refunded')),
  PRIMARY KEY(tenant_id, id)
);

ALTER TABLE archava_marketplace.orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_marketplace.orders FORCE ROW LEVEL SECURITY;
CREATE POLICY market_tenant ON archava_marketplace.orders USING (tenant_id = current_setting('archava.tenant_id', true));
