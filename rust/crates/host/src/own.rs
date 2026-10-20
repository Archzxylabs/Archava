//! C03 — the persisted owner-configuration lifecycle.
//!
//! ## What this module is
//!
//! The product surface of the engine in `archava-own`. The engine decides WHAT
//! a request means (dependencies, readiness, plan identity, blocked results);
//! this module is the only writer of those decisions and the authority around
//! them:
//!
//! - **Own ledger.** `archava_owner` is additive and owns its own grants
//!   (`config.read` / `config.apply`), because the owner surface's users and
//!   authority are separate from the CRM memberships (owner PRD §6). The
//!   runtime role can read grants and append revisions/receipts/audit — it
//!   cannot update or delete them, and RLS forces every row to the session
//!   tenant.
//! - **One transaction per apply.** Advisory tenant lock → grant check →
//!   duplicate-receipt lookup → current revision → catalogue plan →
//!   confirmation binding → revision + receipt + audit, committed together.
//!   A duplicate request id returns the SAME receipt; a different plan under
//!   it is a conflict. A lost response is therefore recoverable, and a
//!   failure is never labelled applied.
//! - **Authoritative inputs only.** The catalogue, its version and the
//!   pricing classification are server facts. A request cannot carry
//!   readiness, cost or authority (the engine refuses unknown fields), and
//!   the confirmation names the plan hash plus the base revision it was shown
//!   for. A changed base or catalogue means replan/reconfirmation.
//! - **History/restore.** Revisions are append-only with actor, request,
//!   plan hash, base revision and outcome. Restore reads a prior config,
//!   revalidates it against the CURRENT catalogue, and lands as a NEW
//!   revision; it never rewrites history or business data.
//!
//! ## What this module deliberately does NOT do
//!
//! It does not invent pricing: the only cost classes are the engine's
//! catalogue classifications (`included` for the current modules). It does not
//! install artifacts, and it does not touch CRM tables. Module behaviour is
//! gated in `crm.rs` against the same persisted config, so a disabled module
//! refuses even when the caller skips the UI.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgConnection;

use archava_adapters::rls::TenantTransaction;
use archava_core::tenant::TenantId;
use archava_own::{
    Catalogue, Confirmation, CostClass, ModuleSpec, OwnerConfig, Plan, Readiness, Request,
    Terminology,
};

use crate::router::{authenticate, AppState};

pub const NAMESPACE: &str = "archava_owner";
pub const LEDGER: &str = "Owner";
/// The tenant-scoped tables startup readiness demands forced RLS on.
pub const OWNER_TABLES: &[&str] = &[
    "owner_grants",
    "config_revisions",
    "apply_receipts",
    "config_audit",
];
pub const CATALOGUE_VERSION: &str = "owner-catalogue@1";
pub const PRICING_VERSION: &str = "included@1";

/// The installed, supported capabilities the owner may configure. Module ids
/// are stable; the UI maps them to names. `crm-contacts` is the mandatory
/// base; `crm-pipeline` is the optional capability A1-OWN-02 names.
pub fn catalogue() -> Catalogue {
    Catalogue {
        modules: vec![
            ModuleSpec {
                id: "crm-contacts".to_string(),
                requires: Vec::new(),
                entitled: true,
                installed: true,
                mandatory: true,
                readiness: Readiness::Ready,
                cost: CostClass::Included,
            },
            ModuleSpec {
                id: "crm-pipeline".to_string(),
                requires: vec!["crm-contacts".to_string()],
                entitled: true,
                installed: true,
                mandatory: false,
                readiness: Readiness::Ready,
                cost: CostClass::Included,
            },
        ],
    }
}

/// The configuration a tenant has before its first apply: every installed,
/// entitled, ready module enabled. Existing tenants therefore keep working
/// until the owner explicitly changes something.
fn default_config(catalogue: &Catalogue) -> OwnerConfig {
    OwnerConfig {
        revision: 0,
        enabled: catalogue
            .modules
            .iter()
            .filter(|module| {
                module.installed && module.entitled && module.readiness == Readiness::Ready
            })
            .map(|module| module.id.clone())
            .collect(),
        terminology: Terminology::default(),
    }
}

#[derive(Debug)]
pub struct OwnRefusal {
    status: StatusCode,
    reason: &'static str,
    message: String,
    detail: Option<Value>,
}

impl OwnRefusal {
    fn new(status: StatusCode, reason: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            reason,
            message: message.into(),
            detail: None,
        }
    }

    fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }

    fn database() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "owner_unavailable",
            "The owner configuration store is unavailable; nothing was changed.",
        )
    }

    fn response(self) -> (StatusCode, Json<Value>) {
        let mut body = json!({"reason": self.reason, "message": self.message});
        if let Some(detail) = self.detail {
            body["detail"] = detail;
        }
        (self.status, Json(body))
    }
}

impl From<sqlx::Error> for OwnRefusal {
    fn from(_: sqlx::Error) -> Self {
        Self::database()
    }
}

impl From<archava_adapters::rls::TenantContextError> for OwnRefusal {
    fn from(_: archava_adapters::rls::TenantContextError) -> Self {
        Self::database()
    }
}

impl From<archava_own::PlanError> for OwnRefusal {
    fn from(error: archava_own::PlanError) -> Self {
        match error {
            archava_own::PlanError::StaleBase { current } => Self::new(
                StatusCode::CONFLICT,
                "stale_base",
                "The configuration moved; replan against the current revision.",
            )
            .with_detail(json!({"currentRevision": current})),
        }
    }
}

fn parse_config(raw: &str) -> Result<OwnerConfig, OwnRefusal> {
    serde_json::from_str(raw).map_err(|_| {
        OwnRefusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "owner_state_corrupt",
            "The stored configuration is unreadable; refusing to guess.",
        )
    })
}

async fn begin<'a>(
    state: &'a AppState,
    tenant: &TenantId,
) -> Result<TenantTransaction<'a>, OwnRefusal> {
    TenantTransaction::begin(state.pool(), NAMESPACE, LEDGER, tenant.clone())
        .await
        .map_err(OwnRefusal::from)
}

/// Refuses unless the principal holds `permission` in this tenant. A revoked
/// or absent grant is the same answer; the caller cannot tell them apart.
async fn require_grant(
    connection: &mut PgConnection,
    tenant: &TenantId,
    principal: &str,
    permission: &str,
) -> Result<(), OwnRefusal> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT array_to_string(permissions, ',') FROM archava_owner.owner_grants \
         WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(tenant.as_str())
    .bind(principal)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(permissions) = row else {
        return Err(OwnRefusal::new(
            StatusCode::FORBIDDEN,
            "missing_grant",
            "This principal has no owner-configuration grant for the tenant.",
        ));
    };
    if !permissions.split(',').any(|held| held == permission) {
        return Err(OwnRefusal::new(
            StatusCode::FORBIDDEN,
            "missing_grant",
            format!("The {permission} grant is required for this operation."),
        ));
    }
    Ok(())
}

/// The current persisted configuration, or the catalogue defaults at the
/// synthetic revision 0 before the first apply.
async fn current_config(
    connection: &mut PgConnection,
    tenant: &TenantId,
) -> Result<OwnerConfig, OwnRefusal> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT revision, config::text FROM archava_owner.config_revisions \
         WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1",
    )
    .bind(tenant.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((revision, raw)) => {
            let config = parse_config(&raw)?;
            // The stored JSON names the revision it IS. A row whose two
            // records disagree is corrupt, not a usable base: planning from
            // it could mint a revision number the ledger already holds.
            if config.revision != revision {
                return Err(OwnRefusal::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "owner_state_corrupt",
                    "The stored configuration disagrees with its revision; refusing to guess.",
                ));
            }
            Ok(config)
        }
        None => Ok(default_config(&catalogue())),
    }
}

struct StoredReceipt {
    revision: i64,
    plan_hash: String,
    config: String,
    catalogue_version: String,
    pricing_version: String,
    outcome: String,
}

async fn stored_receipt(
    connection: &mut PgConnection,
    tenant: &TenantId,
    request_id: &str,
) -> Result<Option<StoredReceipt>, OwnRefusal> {
    let row: Option<(i64, String, String, String, String, String)> = sqlx::query_as(
        "SELECT revision, plan_hash, config::text, catalogue_version, pricing_version, outcome \
         FROM archava_owner.apply_receipts WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((revision, plan_hash, config, catalogue_version, pricing_version, outcome)) => {
            let parsed = parse_config(&config)?;
            if parsed.revision != revision {
                return Err(OwnRefusal::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "owner_state_corrupt",
                    "The stored receipt disagrees with its revision; refusing to replay it.",
                ));
            }
            Ok(Some(StoredReceipt {
                revision,
                plan_hash,
                config,
                catalogue_version,
                pricing_version,
                outcome,
            }))
        }
        None => Ok(None),
    }
}

fn receipt_json(request_id: &str, receipt: &StoredReceipt, replayed: bool) -> Value {
    json!({
        "decision": "receipt",
        "replayed": replayed,
        "receipt": {
            "requestId": request_id,
            "revision": receipt.revision,
            "planHash": receipt.plan_hash,
            "config": serde_json::from_str::<Value>(&receipt.config).unwrap_or(Value::Null),
            "catalogueVersion": receipt.catalogue_version,
            "pricingVersion": receipt.pricing_version,
            "outcome": receipt.outcome,
        }
    })
}

fn validate_request_id(request_id: &str) -> Result<(), OwnRefusal> {
    let valid = (8..=120).contains(&request_id.len())
        && request_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(OwnRefusal::new(
            StatusCode::BAD_REQUEST,
            "invalid_request_id",
            "The request id must be 8–120 characters of letters, digits, '-' or '_'.",
        ))
    }
}

/// Persists one confirmed plan: revision, receipt and audit in the caller's
/// transaction. Every check has already passed; this function only writes.
#[allow(clippy::too_many_arguments)]
async fn write_revision(
    connection: &mut PgConnection,
    tenant: &TenantId,
    principal: &str,
    request_id: &str,
    plan: &Plan,
    outcome: &str,
) -> Result<StoredReceipt, OwnRefusal> {
    let config = serde_json::to_string(&plan.config).map_err(|_| OwnRefusal::database())?;
    sqlx::query(
        "INSERT INTO archava_owner.config_revisions \
         (tenant_id, revision, config, base_revision, actor, request_id, plan_hash, \
          catalogue_version, pricing_version, outcome) \
         VALUES ($1,$2,$3::jsonb,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(tenant.as_str())
    .bind(plan.config.revision)
    .bind(&config)
    .bind(plan.base_revision)
    .bind(principal)
    .bind(request_id)
    .bind(&plan.plan_hash)
    .bind(CATALOGUE_VERSION)
    .bind(PRICING_VERSION)
    .bind(outcome)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO archava_owner.apply_receipts \
         (tenant_id, request_id, revision, plan_hash, config, catalogue_version, \
          pricing_version, outcome) VALUES ($1,$2,$3,$4,$5::jsonb,$6,$7,$8)",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(plan.config.revision)
    .bind(&plan.plan_hash)
    .bind(&config)
    .bind(CATALOGUE_VERSION)
    .bind(PRICING_VERSION)
    .bind(outcome)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO archava_owner.config_audit \
         (tenant_id, revision, actor, action, plan_hash) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(tenant.as_str())
    .bind(plan.config.revision)
    .bind(principal)
    .bind(outcome)
    .bind(&plan.plan_hash)
    .execute(&mut *connection)
    .await?;
    Ok(StoredReceipt {
        revision: plan.config.revision,
        plan_hash: plan.plan_hash.clone(),
        config,
        catalogue_version: CATALOGUE_VERSION.to_string(),
        pricing_version: PRICING_VERSION.to_string(),
        outcome: outcome.to_string(),
    })
}

fn check_confirmation(plan: &Plan, confirmation: &Confirmation) -> Result<(), OwnRefusal> {
    if confirmation.plan_hash != plan.plan_hash || confirmation.base_revision != plan.base_revision
    {
        return Err(OwnRefusal::new(
            StatusCode::CONFLICT,
            "confirmation_mismatch",
            "The confirmation names another plan or base revision; replan and confirm again.",
        ));
    }
    Ok(())
}

fn refuse_blocked(plan: &Plan) -> Result<(), OwnRefusal> {
    if let Some(blocked) = &plan.blocked {
        return Err(OwnRefusal::new(
            StatusCode::CONFLICT,
            "plan_blocked",
            "The plan is blocked; nothing was applied.",
        )
        .with_detail(serde_json::to_value(blocked).unwrap_or(Value::Null)));
    }
    Ok(())
}

/// `GET /own/config` — inspect.
pub async fn config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TenantQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    inspect(&state, &headers, query.tenant)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn inspect(
    state: &AppState,
    headers: &HeaderMap,
    tenant: TenantId,
) -> Result<Value, OwnRefusal> {
    let principal = session(state, headers, false).await?.principal_id;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    let read = require_grant(connection, &tenant, &principal, "config.read").await;
    let apply = require_grant(connection, &tenant, &principal, "config.apply").await;
    // A principal with neither grant learns nothing about the tenant.
    let (read, apply) = match (read, apply) {
        (Ok(()), Ok(())) => (true, true),
        (Ok(()), Err(_)) => (true, false),
        (Err(_), Ok(())) => (false, true),
        (Err(read_err), Err(_)) => return Err(read_err),
    };
    let config = current_config(connection, &tenant).await?;
    transaction.commit().await?;

    let catalogue = catalogue();
    let modules: Vec<Value> = catalogue
        .modules
        .iter()
        .map(|module| {
            json!({
                "id": module.id,
                "enabled": config.enabled.contains(&module.id),
                "mandatory": module.mandatory,
                "requires": module.requires,
                "entitled": module.entitled,
                "installed": module.installed,
                "readiness": module.readiness,
                "cost": module.cost,
            })
        })
        .collect();
    Ok(json!({
        "config": config,
        "catalogueVersion": CATALOGUE_VERSION,
        "pricingVersion": PRICING_VERSION,
        "grants": {"read": read, "apply": apply},
        "modules": modules,
    }))
}

/// `POST /own/plan` — the deterministic preview; no write.
pub async fn plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PlanBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    preview(&state, &headers, body.tenant, body.request)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn preview(
    state: &AppState,
    headers: &HeaderMap,
    tenant: TenantId,
    request: Request,
) -> Result<Value, OwnRefusal> {
    let principal = session(state, headers, true).await?.principal_id;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.read").await?;
    let current = current_config(connection, &tenant).await?;
    let plan = catalogue().plan(&current, &request)?;
    transaction.commit().await?;
    Ok(json!({
        "plan": plan,
        "baseConfig": current,
        "catalogueVersion": CATALOGUE_VERSION,
        "pricingVersion": PRICING_VERSION,
    }))
}

/// `POST /own/apply` — confirm and apply exactly one plan.
pub async fn apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApplyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    submit(&state, &headers, body)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn submit(
    state: &AppState,
    headers: &HeaderMap,
    body: ApplyBody,
) -> Result<Value, OwnRefusal> {
    validate_request_id(&body.request_id)?;
    let principal = session(state, headers, true).await?.principal_id;
    let tenant = body.tenant;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.apply").await?;
    // One writer per tenant at a time, including the first (no revision row).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("archava-owner:{}", tenant.as_str()))
        .execute(&mut *connection)
        .await?;
    if let Some(receipt) = stored_receipt(connection, &tenant, &body.request_id).await? {
        if receipt.plan_hash != body.confirmation.plan_hash {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "request_id_reused",
                "This request id already committed a different plan.",
            ));
        }
        transaction.commit().await?;
        return Ok(receipt_json(&body.request_id, &receipt, true));
    }
    let current = current_config(connection, &tenant).await?;
    let plan = catalogue().plan(&current, &body.request)?;
    refuse_blocked(&plan)?;
    check_confirmation(&plan, &body.confirmation)?;
    let receipt = write_revision(
        connection,
        &tenant,
        &principal,
        &body.request_id,
        &plan,
        "apply",
    )
    .await?;
    transaction.commit().await?;
    Ok(receipt_json(&body.request_id, &receipt, false))
}

/// The request a restore of `target_revision` means against the CURRENT
/// configuration: enable what the target had, disable what it did not, and
/// carry the historical terminology. Revalidated by the catalogue exactly like
/// any other request.
async fn restore_request(
    connection: &mut PgConnection,
    tenant: &TenantId,
    target_revision: i64,
) -> Result<Request, OwnRefusal> {
    let target: Option<String> = sqlx::query_scalar(
        "SELECT config::text FROM archava_owner.config_revisions \
         WHERE tenant_id=$1 AND revision=$2",
    )
    .bind(tenant.as_str())
    .bind(target_revision)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(target) = target else {
        return Err(OwnRefusal::new(
            StatusCode::NOT_FOUND,
            "unknown_revision",
            "No configuration revision with that number exists in this tenant.",
        ));
    };
    let target = parse_config(&target)?;
    let current = current_config(connection, tenant).await?;
    Ok(Request {
        base_revision: current.revision,
        enable: target
            .enabled
            .iter()
            .filter(|module| !current.enabled.contains(*module))
            .cloned()
            .collect(),
        disable: current
            .enabled
            .iter()
            .filter(|module| !target.enabled.contains(*module))
            .cloned()
            .collect(),
        terminology: Some(target.terminology),
    })
}

/// `POST /own/restore/plan` — preview the restore as a plan the client can
/// confirm by hash.
pub async fn restore_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RestorePlanBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    restore_preview(&state, &headers, body)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn restore_preview(
    state: &AppState,
    headers: &HeaderMap,
    body: RestorePlanBody,
) -> Result<Value, OwnRefusal> {
    let principal = session(state, headers, true).await?.principal_id;
    let tenant = body.tenant;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.read").await?;
    let request = restore_request(connection, &tenant, body.target_revision).await?;
    let current = current_config(connection, &tenant).await?;
    let plan = catalogue().plan(&current, &request)?;
    transaction.commit().await?;
    Ok(json!({
        "plan": plan,
        "baseConfig": current,
        "targetRevision": body.target_revision,
        "catalogueVersion": CATALOGUE_VERSION,
        "pricingVersion": PRICING_VERSION,
    }))
}

/// `POST /own/restore` — restore a revision as a NEW revision.
pub async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RestoreBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    restore_inner(&state, &headers, body)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn restore_inner(
    state: &AppState,
    headers: &HeaderMap,
    body: RestoreBody,
) -> Result<Value, OwnRefusal> {
    validate_request_id(&body.request_id)?;
    let principal = session(state, headers, true).await?.principal_id;
    let tenant = body.tenant;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.apply").await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("archava-owner:{}", tenant.as_str()))
        .execute(&mut *connection)
        .await?;
    if let Some(receipt) = stored_receipt(connection, &tenant, &body.request_id).await? {
        if receipt.plan_hash != body.confirmation.plan_hash {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "request_id_reused",
                "This request id already committed a different plan.",
            ));
        }
        transaction.commit().await?;
        return Ok(receipt_json(&body.request_id, &receipt, true));
    }
    let request = restore_request(connection, &tenant, body.target_revision).await?;
    let current = current_config(connection, &tenant).await?;
    let plan = catalogue().plan(&current, &request)?;
    refuse_blocked(&plan)?;
    check_confirmation(&plan, &body.confirmation)?;
    let receipt = write_revision(
        connection,
        &tenant,
        &principal,
        &body.request_id,
        &plan,
        "restore",
    )
    .await?;
    transaction.commit().await?;
    Ok(receipt_json(&body.request_id, &receipt, false))
}

/// revision, config, actor, request, plan hash, base revision, outcome,
/// created-at — the history column order.
type HistoryRow = (i64, String, String, String, String, i64, String, String);

/// `GET /own/history` — revisions, newest first.
pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TenantQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    history_inner(&state, &headers, query.tenant)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn history_inner(
    state: &AppState,
    headers: &HeaderMap,
    tenant: TenantId,
) -> Result<Value, OwnRefusal> {
    let principal = session(state, headers, false).await?.principal_id;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.read").await?;
    let rows: Vec<HistoryRow> = sqlx::query_as(
        "SELECT revision, config::text, actor, request_id, plan_hash, base_revision, outcome, \
         created_at::text FROM archava_owner.config_revisions \
         WHERE tenant_id=$1 ORDER BY revision DESC",
    )
    .bind(tenant.as_str())
    .fetch_all(&mut *connection)
    .await?;
    transaction.commit().await?;
    let revisions: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                revision,
                config,
                actor,
                request_id,
                plan_hash,
                base_revision,
                outcome,
                created_at,
            )| {
                json!({
                    "revision": revision,
                    "config": serde_json::from_str::<Value>(&config).unwrap_or(Value::Null),
                    "actor": actor,
                    "requestId": request_id,
                    "planHash": plan_hash,
                    "baseRevision": base_revision,
                    "outcome": outcome,
                    "createdAt": created_at,
                })
            },
        )
        .collect();
    Ok(json!({
        "revisions": revisions,
        "catalogueVersion": CATALOGUE_VERSION,
        "pricingVersion": PRICING_VERSION,
    }))
}

/// Whether a module is enabled in the tenant's CURRENT persisted
/// configuration. Used by the CRM gate; no config row means the defaults.
pub async fn module_enabled(
    connection: &mut PgConnection,
    tenant: &TenantId,
    module: &str,
) -> Result<bool, OwnRefusal> {
    Ok(current_config(connection, tenant)
        .await?
        .enabled
        .contains(module))
}

async fn session(
    state: &AppState,
    headers: &HeaderMap,
    csrf_required: bool,
) -> Result<crate::identity::AuthenticatedSession, OwnRefusal> {
    match authenticate(state, headers, csrf_required).await {
        Ok(session) => Ok(session),
        Err((status, Json(outcome))) => {
            let reason = match outcome.reason.as_deref() {
                Some("csrf_required") => "csrf_required",
                _ => "unauthenticated",
            };
            let message = outcome
                .message
                .unwrap_or_else(|| "A signed-in owner session is required.".to_string());
            Err(OwnRefusal::new(status, reason, message))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantQuery {
    pub tenant: TenantId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanBody {
    pub tenant: TenantId,
    pub request: Request,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyBody {
    pub tenant: TenantId,
    pub request_id: String,
    pub request: Request,
    pub confirmation: Confirmation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestorePlanBody {
    pub tenant: TenantId,
    pub target_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreBody {
    pub tenant: TenantId,
    pub request_id: String,
    pub target_revision: i64,
    pub confirmation: Confirmation,
}
