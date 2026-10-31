//! ERP cohort command, query, and workspace routes across all 17 ledgers.

use axum::routing::{get, post};
use axum::Router;

use crate::router::AppState;

/// Mounts all ERP ledger routes.
pub fn router() -> Router<AppState> {
    Router::new()
        // Billing payments & invoice email integrations
        .route(
            "/erp/billing/payments",
            get(crate::payments::list).post(crate::payments::create),
        )
        .route("/erp/billing/payments/check", post(crate::payments::check))
        .route("/erp/billing/payments/status", get(crate::payments::status))
        .route(
            "/erp/billing/invoices/{id}/send-email",
            post(crate::mail::send_invoice_email),
        )
        .route(
            "/erp/billing/invoices/{id}/emails",
            get(crate::mail::list_invoice_emails),
        )
        // Archxy 2's ERP namespace (PACKET-000/001): the two kernel cohorts
        // mount into the same session-scoped api scope, mirroring the CRM
        // gate's decision/receipt documents. CSRF-bearing mutation per gate.
        .route(
            "/erp/commerce/commands",
            post(crate::erp::commerce::command),
        )
        .route(
            "/erp/commerce/commands/status",
            get(crate::erp::commerce::status),
        )
        .route(
            "/erp/commerce/workspace",
            get(crate::erp::commerce::workspace),
        )
        .route(
            "/erp/purchasing/workspace",
            get(crate::erp::workspace::purchasing),
        )
        .route(
            "/erp/billing/workspace",
            get(crate::erp::workspace::billing),
        )
        .route(
            "/erp/purchasing/commands",
            post(crate::erp::purchasing::purchase_command),
        )
        .route(
            "/erp/purchasing/commands/status",
            get(crate::erp::purchasing::purchase_status),
        )
        .route(
            "/erp/foodservice/commands",
            post(crate::erp::foodservice::food_service_command),
        )
        .route(
            "/erp/foodservice/commands/status",
            get(crate::erp::foodservice::food_service_status),
        )
        // Archxy 2's R04 ledgers (PACKET-003): organizations, proposals and
        // projects, the same session+CSRF/tenant-path shape as the cohorts
        // above.
        .route(
            "/erp/organizations/commands",
            post(crate::erp::organizations::organization_command),
        )
        .route(
            "/erp/organizations/commands/status",
            get(crate::erp::organizations::organization_status),
        )
        .route(
            "/erp/proposals/commands",
            post(crate::erp::proposals::proposal_command),
        )
        .route(
            "/erp/proposals/commands/status",
            get(crate::erp::proposals::proposal_status),
        )
        .route(
            "/erp/projects/commands",
            post(crate::erp::projects::project_command),
        )
        .route(
            "/erp/projects/commands/status",
            get(crate::erp::projects::project_status),
        )
        // Archxy 2's R05 scheduling ledger (PACKET-004): reservations mounted
        // beside the R04 cohorts, same session+CSRF and tenant-body shape.
        .route(
            "/erp/scheduling/commands",
            post(crate::erp::scheduling::scheduling_command),
        )
        .route(
            "/erp/scheduling/commands/status",
            get(crate::erp::scheduling::scheduling_status),
        )
        // Archxy 2's R05 accommodation ledger (PACKET-005): room-night stays
        // compose the same occupancy kernel the scheduling ledger owns.
        .route(
            "/erp/accommodation/commands",
            post(crate::erp::accommodation::accommodation_command),
        )
        .route(
            "/erp/accommodation/commands/status",
            get(crate::erp::accommodation::accommodation_status),
        )
        // Archxy 2's R05 assets ledger (PACKET-006): custody and maintenance,
        // its own ledger with no shared kernel.
        .route(
            "/erp/assets/commands",
            post(crate::erp::assets::asset_command),
        )
        .route(
            "/erp/assets/commands/status",
            get(crate::erp::assets::asset_status),
        )
        // Archxy 2's R07 billing ledger (PACKET-007): the settlement
        // cohort, the R07 area's first subject — its own ledger; the
        // provider-event arm is authorised by a signature the gate
        // verifies BEFORE the transaction, so an under-signed claim
        // claims nothing.
        .route(
            "/erp/billing/commands",
            post(crate::erp::billing::billing_command),
        )
        .route(
            "/erp/billing/commands/status",
            get(crate::erp::billing::billing_status),
        )
        // Archxy 2's R07 subscriptions ledger (PACKET-008): the
        // entitlements/usage cohort — the READ surfaces come with it
        // (list, detail, entitlements, usage history), because a ledger
        // whose rows can only be written is not a usable domain.
        .route(
            "/erp/subscriptions/commands",
            post(crate::erp::subscriptions::subscription_command),
        )
        .route(
            "/erp/subscriptions/commands/status",
            get(crate::erp::subscriptions::subscription_status),
        )
        .route(
            "/erp/subscriptions/subscriptions",
            get(crate::erp::subscriptions::subscription_list),
        )
        .route(
            "/erp/subscriptions/subscriptions/detail",
            get(crate::erp::subscriptions::subscription_detail),
        )
        .route(
            "/erp/subscriptions/entitlements",
            get(crate::erp::subscriptions::subscription_entitlements),
        )
        .route(
            "/erp/subscriptions/contact-entitlements",
            get(crate::erp::subscriptions::subscription_contact_entitlements),
        )
        .route(
            "/erp/subscriptions/usage-events",
            get(crate::erp::subscriptions::subscription_usage),
        )
        // Archxy 2's R07 attendance ledger (PACKET-009): the
        // session/roster/check-in cohort — the corrected roster binding
        // (audit A04) is a row-SELECT property, and the read surfaces
        // (list, detail, roster, check-ins, access) ride the same
        // transaction opener.
        .route(
            "/erp/attendance/commands",
            post(crate::erp::attendance::attendance_command),
        )
        .route(
            "/erp/attendance/commands/status",
            get(crate::erp::attendance::attendance_status),
        )
        .route(
            "/erp/attendance/access",
            get(crate::erp::attendance::attendance_access),
        )
        .route(
            "/erp/attendance/sessions",
            get(crate::erp::attendance::attendance_list),
        )
        .route(
            "/erp/attendance/sessions/detail",
            get(crate::erp::attendance::attendance_detail),
        )
        .route(
            "/erp/attendance/roster",
            get(crate::erp::attendance::attendance_roster),
        )
        .route(
            "/erp/attendance/check-ins",
            get(crate::erp::attendance::attendance_check_ins),
        )
        // Archxy 2's R07 support ledger (PACKET-010): the case lifecycle
        // (start/wait/resume/close/cancel/resolve/reopen/assign/priority/
        // target/note) and the four read projections.
        .route(
            "/erp/support/commands",
            post(crate::erp::support::support_command),
        )
        .route(
            "/erp/support/commands/status",
            get(crate::erp::support::support_status),
        )
        .route(
            "/erp/support/access",
            get(crate::erp::support::support_access_route),
        )
        .route(
            "/erp/support/assignees",
            get(crate::erp::support::support_assignee_route),
        )
        .route(
            "/erp/support/cases",
            get(crate::erp::support::support_case_route),
        )
        .route(
            "/erp/support/cases/detail",
            get(crate::erp::support::support_detail_route),
        )
        // Archxy 2's R08 laundry ledger: the order intake & processing lifecycle
        .route(
            "/erp/laundry/commands",
            post(crate::erp::laundry::laundry_command),
        )
        .route(
            "/erp/laundry/commands/status",
            get(crate::erp::laundry::laundry_status),
        )
        .route(
            "/erp/laundry/access",
            get(crate::erp::laundry::laundry_access_route),
        )
        .route(
            "/erp/laundry/orders",
            get(crate::erp::laundry::laundry_orders_route),
        )
        .route(
            "/erp/laundry/orders/detail",
            get(crate::erp::laundry::laundry_detail_route),
        )
        .route(
            "/erp/laundry/process",
            get(crate::erp::laundry::laundry_process_route),
        )
        .route(
            "/erp/laundry/findings",
            get(crate::erp::laundry::laundry_findings_route),
        )
        // Archxy 2's R08 field services ledger: dispatch, visits, work lines, signoffs
        .route(
            "/erp/field/commands",
            post(crate::erp::field_services::field_command),
        )
        .route(
            "/erp/field/commands/status",
            get(crate::erp::field_services::field_status),
        )
        .route(
            "/erp/field/access",
            get(crate::erp::field_services::field_access_route),
        )
        .route(
            "/erp/field/jobs",
            get(crate::erp::field_services::field_jobs),
        )
        .route(
            "/erp/field/jobs/detail",
            get(crate::erp::field_services::field_job_detail),
        )
        .route(
            "/erp/field/audit",
            get(crate::erp::field_services::field_audit),
        )
        // Archxy 2's R08 fulfillment ledger: jobs, lines, findings, custody
        .route(
            "/erp/fulfillment/commands",
            post(crate::erp::fulfillment::fulfillment_command),
        )
        .route(
            "/erp/fulfillment/commands/status",
            get(crate::erp::fulfillment::fulfillment_status),
        )
        .route(
            "/erp/fulfillment/access",
            get(crate::erp::fulfillment::fulfillment_access_route),
        )
        .route(
            "/erp/fulfillment/jobs",
            get(crate::erp::fulfillment::fulfillment_jobs_route),
        )
        .route(
            "/erp/fulfillment/jobs/detail",
            get(crate::erp::fulfillment::fulfillment_detail_route),
        )
        .route(
            "/erp/fulfillment/lines",
            get(crate::erp::fulfillment::fulfillment_lines_route),
        )
        .route(
            "/erp/fulfillment/findings",
            get(crate::erp::fulfillment::fulfillment_findings_route),
        )
        .route(
            "/erp/fulfillment/custody",
            get(crate::erp::fulfillment::fulfillment_custody_route),
        )
        // Archxy 2's R08 formal education ledger: admissions, enrollments, sessions, results, reviews, audit
        .route(
            "/erp/education/commands",
            post(crate::erp::formal_education::education_command),
        )
        .route(
            "/erp/education/commands/status",
            get(crate::erp::formal_education::education_status),
        )
        .route(
            "/erp/education/access",
            get(crate::erp::formal_education::education_access_route),
        )
        .route(
            "/erp/education/enrollments",
            get(crate::erp::formal_education::education_enrollments),
        )
        .route(
            "/erp/education/enrollments/detail",
            get(crate::erp::formal_education::education_enrollment_detail),
        )
        .route(
            "/erp/education/enrollments/audit",
            get(crate::erp::formal_education::education_audit),
        )
}
