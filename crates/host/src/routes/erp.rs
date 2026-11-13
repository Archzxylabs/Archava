//! ERP command, query and workspace routes. The command and command-status
//! routes come from the module registry; the read routes below are per module.

use axum::routing::{get, post};
use axum::Router;

use crate::router::AppState;

/// `POST /erp/<id>/commands` and `GET /erp/<id>/commands/status` for every
/// module in the registry. Each module exposes the two handlers under the same
/// names, so a new module is mounted here without an entry of its own.
fn command_routes() -> Router<AppState> {
    macro_rules! mount {
        ($(($id:literal, $source:ident)),* $(,)?) => {
            Router::new()$(
                .route(
                    concat!("/erp/", $id, "/commands"),
                    post(crate::erp::$source::command),
                )
                .route(
                    concat!("/erp/", $id, "/commands/status"),
                    get(crate::erp::$source::status),
                )
            )*
        };
    }
    for_each_erp_module!(mount)
}

/// Mounts all ERP ledger routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(command_routes())
        .merge(crate::erp::projections::router())
        .route(
            "/erp/accounting/workspace",
            get(crate::erp::accounting::workspace),
        )
        .route(
            "/erp/accounting/reports",
            get(crate::erp::accounting::reports),
        )
        .route(
            "/erp/accounting/subledger-report",
            get(crate::erp::accounting::subledger_report),
        )
        .route(
            "/erp/accounting/bank-reconciliation",
            get(crate::erp::accounting::bank_reconciliation),
        )
        .route(
            "/erp/accounting/close-readiness",
            get(crate::erp::accounting::close_readiness),
        )
        // CRM customer relationship management cohort (ADR-018: normalized enterprise routes)
        .route("/erp/crm/records", get(crate::erp::crm::record))
        .route("/erp/crm/contacts", get(crate::erp::crm::contacts))
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
        // Archxy 2's R04 ledgers (PACKET-003): organizations, proposals and
        // projects, the same session+CSRF/tenant-path shape as the cohorts
        // above.
        // Archxy 2's R05 scheduling ledger (PACKET-004): reservations mounted
        // beside the R04 cohorts, same session+CSRF and tenant-body shape.
        // Archxy 2's R05 accommodation ledger (PACKET-005): room-night stays
        // compose the same occupancy kernel the scheduling ledger owns.
        // Archxy 2's R05 assets ledger (PACKET-006): custody and maintenance,
        // its own ledger with no shared kernel.
        // Archxy 2's R07 billing ledger (PACKET-007): the settlement
        // cohort, the R07 area's first subject — its own ledger; the
        // provider-event arm is authorised by a signature the gate
        // verifies BEFORE the transaction, so an under-signed claim
        // claims nothing.
        // Archxy 2's R07 subscriptions ledger (PACKET-008): the
        // entitlements/usage cohort — the READ surfaces come with it
        // (list, detail, entitlements, usage history), because a ledger
        // whose rows can only be written is not a usable domain.
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
