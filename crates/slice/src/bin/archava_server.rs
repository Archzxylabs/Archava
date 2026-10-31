//! Persistent Rust product server.
//!
//! Unlike the fixture-backed `slice_server` example, this binary never creates
//! or owns PostgreSQL. It accepts an operator-provided runtime database URL,
//! derives the exact migration identity embedded in this build, verifies every
//! mounted ledger before accepting application traffic. It reserves the socket
//! before acquiring its writer epoch, then serves the shared SSR/WASM surface.

use std::env;
use std::error::Error;
use std::net::SocketAddr;

use archava_adapters::rls::migration_checksum;
use archava_host::{AllReadiness, AppState, Ledger};
use axum::Router;
use sqlx::postgres::PgConnectOptions;
use tracing_subscriber::EnvFilter;

use archava_slice::server::{build_app, shutdown_signal, wasm_package_dir};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8347";
const MIN_IDENTITY_KEY_BYTES: usize = 32;

const COMMERCE_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_catalog_orders_stock",
        include_bytes!("../../../../migrations/commerce-pg/001_catalog_orders_stock.sql"),
    ),
    (
        "002_managed_menu_products",
        include_bytes!("../../../../migrations/commerce-pg/002_managed_menu_products.sql"),
    ),
    (
        "003_purchase_receipts",
        include_bytes!("../../../../migrations/commerce-pg/003_purchase_receipts.sql"),
    ),
    (
        "004_bound_approvals",
        include_bytes!("../../../../migrations/commerce-pg/004_bound_approvals.sql"),
    ),
    (
        "005_inventory_cost_layers",
        include_bytes!("../../../../migrations/commerce-pg/005_inventory_cost_layers.sql"),
    ),
    (
        "006_private_cost_writers",
        include_bytes!("../../../../migrations/commerce-pg/006_private_cost_writers.sql"),
    ),
];

const IDENTITY_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_identity",
    include_bytes!("../../../../migrations/identity-pg/001_identity.sql"),
)];

const CRM_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_crm_contacts",
        include_bytes!("../../../../migrations/crm-pg/001_crm_contacts.sql"),
    ),
    (
        "002_sales_pipeline",
        include_bytes!("../../../../migrations/crm-pg/002_sales_pipeline.sql"),
    ),
    (
        "003_native_approval",
        include_bytes!("../../../../migrations/crm-pg/003_native_approval.sql"),
    ),
];

const OWNER_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_owner_config",
    include_bytes!("../../../../migrations/owner-pg/001_owner_config.sql"),
)];

const PURCHASING_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_suppliers_orders_receipts",
        include_bytes!("../../../../migrations/purchasing-pg/001_suppliers_orders_receipts.sql"),
    ),
    (
        "002_landed_cost_receipts",
        include_bytes!("../../../../migrations/purchasing-pg/002_landed_cost_receipts.sql"),
    ),
];

const FOODSERVICE_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_tables_recipes_tickets",
    include_bytes!("../../../../migrations/foodservice-pg/001_tables_recipes_tickets.sql"),
)];

const ORGANIZATIONS_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_organizations_affiliations",
    include_bytes!("../../../../migrations/organizations-pg/001_organizations_affiliations.sql"),
)];

const PROPOSALS_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_proposals_scope_amendments",
    include_bytes!("../../../../migrations/proposals-pg/001_proposals_scope_amendments.sql"),
)];

const PROJECTS_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_project_delivery",
    include_bytes!("../../../../migrations/projects-pg/001_project_delivery.sql"),
)];

const SCHEDULING_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_resource_reservations",
        include_bytes!("../../../../migrations/scheduling-pg/001_resource_reservations.sql"),
    ),
    (
        "002_shared_occupancy",
        include_bytes!("../../../../migrations/scheduling-pg/002_shared_occupancy.sql"),
    ),
];

const ACCOMMODATION_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_room_night_stays",
    include_bytes!("../../../../migrations/accommodation-pg/001_room_night_stays.sql"),
)];

const ASSETS_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_assets_custody_maintenance",
    include_bytes!("../../../../migrations/assets-pg/001_assets_custody_maintenance.sql"),
)];

const BILLING_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_billing_invoices_settlement",
        include_bytes!("../../../../migrations/billing-pg/001_billing_invoices_settlement.sql"),
    ),
    (
        "002_gateway_payments",
        include_bytes!("../../../../migrations/billing-pg/002_gateway_payments.sql"),
    ),
    (
        "003_invoice_email_outbox",
        include_bytes!("../../../../migrations/billing-pg/003_invoice_email_outbox.sql"),
    ),
];

const SUBSCRIPTIONS_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_subscriptions_entitlements_usage",
    include_bytes!(
        "../../../../migrations/subscriptions-pg/001_subscriptions_entitlements_usage.sql"
    ),
)];

const ATTENDANCE_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_attendance_sessions_rosters",
    include_bytes!("../../../../migrations/attendance-pg/001_attendance_sessions_rosters.sql"),
)];

const SUPPORT_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_cases_notes_resolution",
    include_bytes!("../../../../migrations/support-pg/001_cases_notes_resolution.sql"),
)];

const LAUNDRY_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_laundry_orders_processing",
    include_bytes!("../../../../migrations/laundry-pg/001_laundry_orders_processing.sql"),
)];

const FIELD_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_field_services_dispatch_visits",
        include_bytes!(
            "../../../../migrations/field-services-pg/001_field_services_dispatch_visits.sql"
        ),
    ),
    (
        "002_field_services_tenant_isolation",
        include_bytes!(
            "../../../../migrations/field-services-pg/002_field_services_tenant_isolation.sql"
        ),
    ),
];

const FULFILLMENT_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_fulfillment_jobs_evidence",
    include_bytes!("../../../../migrations/fulfillment-pg/001_fulfillment_jobs_evidence.sql"),
)];

const EDU_MIGRATIONS: &[(&str, &[u8])] = &[
    (
        "001_formal_education_enrollment",
        include_bytes!(
            "../../../../migrations/formal-education-pg/001_formal_education_enrollment.sql"
        ),
    ),
    (
        "002_formal_education_tenant_keys",
        include_bytes!(
            "../../../../migrations/formal-education-pg/002_formal_education_tenant_keys.sql"
        ),
    ),
];

const COCKPIT_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_cockpit_durable_missions",
    include_bytes!("../../../../migrations/cockpit-pg/001_cockpit_durable_missions.sql"),
)];

const ORBZ_MIGRATIONS: &[(&str, &[u8])] = &[(
    "001_agentic_memory_orbz",
    include_bytes!("../../../../migrations/orbz-pg/001_agentic_memory_orbz.sql"),
)];

const COMMERCE_TABLES: &[&str] = &[
    "memberships",
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "commands",
    "movements",
    "returns",
    "audit",
    "approvals",
    "inventory_layers",
    "inventory_uses",
    "inventory_return_costs",
];

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: COMMERCE_TABLES,
    }
}

fn expected(entries: &[(&str, &[u8])]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(id, bytes)| ((*id).to_string(), migration_checksum(bytes)))
        .collect()
}

fn identity_key() -> Result<Vec<u8>, String> {
    let encoded = env::var("ARCHAVA_IDENTITY_KEY_HEX")
        .map_err(|_| "ARCHAVA_IDENTITY_KEY_HEX is required".to_string())?;
    if encoded.len() % 2 != 0 {
        return Err("ARCHAVA_IDENTITY_KEY_HEX must contain complete hex bytes".to_string());
    }
    let mut key = Vec::with_capacity(encoded.len() / 2);
    for offset in (0..encoded.len()).step_by(2) {
        let byte = u8::from_str_radix(&encoded[offset..offset + 2], 16)
            .map_err(|_| "ARCHAVA_IDENTITY_KEY_HEX contains invalid hex".to_string())?;
        key.push(byte);
    }
    if key.len() < MIN_IDENTITY_KEY_BYTES {
        return Err(format!(
            "ARCHAVA_IDENTITY_KEY_HEX must contain at least {MIN_IDENTITY_KEY_BYTES} bytes"
        ));
    }
    Ok(key)
}

fn bind_addr() -> Result<SocketAddr, String> {
    env::var("ARCHAVA_BIND_ADDR")
        .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string())
        .parse()
        .map_err(|error| format!("ARCHAVA_BIND_ADDR is invalid: {error}"))
}

#[tokio::main]
async fn main() {
    // JSON by default: the runtime's stdout is a log stream, not a console.
    // `RUST_LOG` narrows or widens it without a rebuild; an invalid filter
    // falls back to `info` rather than refusing to boot, because losing logs
    // is bad and losing the service is worse.
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    if let Err(error) = serve().await {
        // The startup refusals are the operator's only clue when readiness
        // fails: emit them through the same JSON stream, not Rust's Debug
        // formatter on `main`'s Result.
        tracing::error!(error = %error, "persistent runtime refused to start");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), Box<dyn Error>> {
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is required for the persistent Rust runtime")?;
    let identity_key =
        identity_key().map_err(|error| format!("identity configuration: {error}"))?;
    let site_addr = bind_addr().map_err(|error| format!("bind configuration: {error}"))?;
    let pkg_dir = wasm_package_dir().map_err(|error| format!("browser assets: {error}"))?;

    let commerce = expected(COMMERCE_MIGRATIONS);
    let identity = expected(IDENTITY_MIGRATIONS);
    let crm = expected(CRM_MIGRATIONS);
    let owner = expected(OWNER_MIGRATIONS);
    let purchasing = expected(PURCHASING_MIGRATIONS);
    let foodservice = expected(FOODSERVICE_MIGRATIONS);
    let organizations = expected(ORGANIZATIONS_MIGRATIONS);
    let proposals = expected(PROPOSALS_MIGRATIONS);
    let projects = expected(PROJECTS_MIGRATIONS);
    let scheduling = expected(SCHEDULING_MIGRATIONS);
    let accommodation = expected(ACCOMMODATION_MIGRATIONS);
    let assets = expected(ASSETS_MIGRATIONS);
    let billing = expected(BILLING_MIGRATIONS);
    let subscriptions = expected(SUBSCRIPTIONS_MIGRATIONS);
    let attendance = expected(ATTENDANCE_MIGRATIONS);
    let support = expected(SUPPORT_MIGRATIONS);
    let laundry = expected(LAUNDRY_MIGRATIONS);
    let field = expected(FIELD_MIGRATIONS);
    let fulfillment = expected(FULFILLMENT_MIGRATIONS);
    let edu = expected(EDU_MIGRATIONS);
    let cockpit = expected(COCKPIT_MIGRATIONS);
    let orbz = expected(ORBZ_MIGRATIONS);
    let additional_erp = [
        ("archava_foodservice", foodservice.as_slice()),
        ("archava_organizations", organizations.as_slice()),
        ("archava_proposals", proposals.as_slice()),
        ("archava_projects", projects.as_slice()),
        ("archava_scheduling", scheduling.as_slice()),
        ("archava_accommodation", accommodation.as_slice()),
        ("archava_assets", assets.as_slice()),
        ("archava_billing", billing.as_slice()),
        ("archava_subscriptions", subscriptions.as_slice()),
        ("archava_attendance", attendance.as_slice()),
        ("archava_support", support.as_slice()),
        ("archava_laundry", laundry.as_slice()),
        ("archava_field", field.as_slice()),
        ("archava_fulfillment", fulfillment.as_slice()),
        ("archava_edu", edu.as_slice()),
        ("archava_cockpit", cockpit.as_slice()),
        ("archava_orbz", orbz.as_slice()),
    ];
    let readiness = AllReadiness {
        commerce: &commerce,
        identity: &identity,
        crm: &crm,
        owner: &owner,
        purchasing: &purchasing,
        additional_erp: &additional_erp,
    };

    let payment_registry = match std::env::var_os("ARCHAVA_PAYMENT_CONFIG") {
        None => archava_host::payments::PaymentRegistry::default(),
        Some(path) => archava_host::payments::PaymentRegistry::load(std::path::Path::new(&path))?,
    };
    let email_registry = match std::env::var_os("ARCHAVA_EMAIL_CONFIG") {
        None => archava_host::mail::EmailRegistry::default(),
        Some(path) => archava_host::mail::EmailRegistry::load(std::path::Path::new(&path))
            .map_err(|_| "invalid email configuration; check file and environment variables")?,
    };
    // Reserve the socket before acquiring a writer epoch: a duplicate-port
    // startup must not fence the healthy process that already owns this port.
    let listener = tokio::net::TcpListener::bind(site_addr)
        .await
        .map_err(|error| format!("bind {site_addr} failed: {error}"))?;
    let takeover = match env::var("ARCHAVA_WRITER_TAKEOVER").as_deref() {
        Ok("true") => true,
        Ok("false") | Err(env::VarError::NotPresent) => false,
        _ => return Err("ARCHAVA_WRITER_TAKEOVER must be true or false".into()),
    };
    let options: PgConnectOptions = database_url
        .parse()
        .map_err(|_| "invalid runtime database connection configuration")?;
    let (pool, writer) = archava_adapters::writer::connect(options, 16, takeover).await?;
    let state = match AppState::try_new_all(
        pool.clone(),
        commerce_ledger(),
        readiness,
        &identity_key,
    )
    .await
    {
        Ok(state) => state,
        Err(error) => {
            let _ = writer.drain(&pool).await;
            return Err(
                format!("startup readiness refused the persistent runtime: {error}").into(),
            );
        }
    };
    let presence_registry = archava_host::presence::PresenceRegistry::from_env();
    let state = state
        .with_payment_registry(payment_registry)
        .with_email_registry(email_registry)
        .with_presence_registry(presence_registry);
    // Both are startup configuration, like the registries above: a scrape
    // credential and a frame-ancestor allowlist cannot be widened by a request,
    // and a malformed list is a startup failure rather than a silent default.
    let state = match env::var("ARCHAVA_METRICS_TOKEN") {
        Ok(token) if !token.trim().is_empty() => state.with_metrics_token(token),
        Ok(_) => return Err("ARCHAVA_METRICS_TOKEN must not be empty".into()),
        Err(env::VarError::NotPresent) => state,
        Err(_) => return Err("ARCHAVA_METRICS_TOKEN is not valid UTF-8".into()),
    };
    let state = match env::var("ARCHAVA_EMBED_FRAME_ANCESTORS") {
        Ok(raw) if raw.trim().is_empty() => state,
        Ok(raw) => state.with_embed_frame_ancestors(archava_host::parse_frame_ancestors(&raw)?),
        Err(env::VarError::NotPresent) => state,
        Err(_) => return Err("ARCHAVA_EMBED_FRAME_ANCESTORS is not valid UTF-8".into()),
    };
    let app: Router = build_app(state, site_addr, &pkg_dir);
    tracing::info!(
        bind = %site_addr,
        site_root = %pkg_dir.display(),
        "persistent Rust runtime ready; all mounted ledgers passed readiness"
    );
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await;
    if writer.drain(&pool).await.is_err() {
        tracing::warn!("writer drain failed; inspect lease before restarting")
    }
    pool.close().await;
    served.map_err(|error| format!("persistent Rust runtime stopped with error: {error}"))?;
    Ok(())
}
