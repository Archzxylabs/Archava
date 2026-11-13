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

pub(crate) mod disclosure;
pub use disclosure::DisclosureApproval;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgConnection;

use archava_adapters::rls::TenantTransaction;
use archava_core::tenant::TenantId;
use archava_own::{
    Catalogue, Confirmation, CostClass, DisclosureChange, ModuleSpec, OwnerConfig, Plan, Readiness,
    Request, Terminology,
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
    "reviewed_workflows",
    "agent_threads",
    "agent_turns",
    "agent_goals",
    "agent_preferences",
    "agent_work",
    "agent_model_attempts",
];
pub const CATALOGUE_VERSION: &str = "owner-catalogue@4";
pub const PRICING_VERSION: &str = "included@1";
/// The base capability every tenant keeps; a module that declares no
/// requirement of its own requires this one.
const MANDATORY_MODULE: &str = "crm-contacts";

pub(crate) async fn assert_disclosure_readiness(
    pool: &sqlx::PgPool,
) -> Result<(), archava_adapters::rls::TenantContextError> {
    use archava_adapters::rls::TenantContextError;
    let db = |source| TenantContextError::Database {
        ledger: "Owner AI disclosure boundary",
        source,
    };
    let present: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('archava_owner.ai_disclosure_boundary_version()') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(db)?;
    if !present {
        return Err(db(sqlx::Error::Protocol(
            "Owner disclosure boundary is not installed".into(),
        )));
    }
    let version: String =
        sqlx::query_scalar("SELECT archava_owner.ai_disclosure_boundary_version()")
            .fetch_one(pool)
            .await
            .map_err(db)?;
    if version != disclosure::PROJECTION_VERSION {
        return Err(db(sqlx::Error::Protocol(
            "Owner disclosure boundary version differs from the serving binary".into(),
        )));
    }
    Ok(())
}

/// The installed, supported capabilities the owner may configure. Module ids
/// are stable; the UI maps them to names. `crm-contacts` is the mandatory
/// base; `crm-pipeline` is the optional capability A1-OWN-02 names.
pub fn catalogue() -> Catalogue {
    let mut modules = vec![
        ModuleSpec {
            id: "crm-contacts".into(),
            requires: vec![],
            entitled: true,
            installed: true,
            mandatory: true,
            readiness: Readiness::Ready,
            cost: CostClass::Included,
        },
        ModuleSpec {
            id: "crm-pipeline".into(),
            requires: vec!["crm-contacts".into()],
            entitled: true,
            installed: true,
            mandatory: false,
            readiness: Readiness::Ready,
            cost: CostClass::Included,
        },
    ];
    for module in archava_domain::operator::catalogue()["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["id"] != "crm")
    {
        let id = module["id"].as_str().expect("embedded module id");
        // The registry names what a module needs; one that names nothing
        // needs only the mandatory base.
        let declared = archava_domain::modules::get(id)
            .map(|module| module.owner_requires)
            .unwrap_or_default();
        let requires = if declared.is_empty() {
            vec![MANDATORY_MODULE.into()]
        } else {
            declared.iter().map(|id| (*id).into()).collect()
        };
        modules.push(ModuleSpec {
            id: id.into(),
            requires,
            entitled: true,
            installed: true,
            mandatory: false,
            readiness: Readiness::Ready,
            cost: CostClass::Included,
        });
    }
    Catalogue {
        disclosure_modules: Some(
            archava_domain::operator::catalogue()["modules"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect(),
        ),
        catalogue_version: CATALOGUE_VERSION.into(),
        pricing_version: PRICING_VERSION.into(),
        modules,
    }
}

/// The configuration a tenant has before its first apply: every installed,
/// entitled, ready module enabled. Existing tenants therefore keep working
/// until the owner explicitly changes something.
fn default_config(catalogue: &Catalogue) -> OwnerConfig {
    OwnerConfig {
        revision: 0,
        ai_disclosure: None,
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

// A configured capture contract must not disappear behind a module toggle.
// The business shared owner fence and apply's exclusive fence serialize setup.
async fn current_catalogue(
    connection: &mut PgConnection,
    tenant: &TenantId,
) -> Result<Catalogue, OwnRefusal> {
    let mut result = catalogue();
    let installed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='archava_accounting' AND c.relname='invoice_policies')")
        .fetch_one(&mut *connection).await?;
    let bound = if installed {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM archava_accounting.invoice_policies WHERE tenant_id=$1)",
        )
        .bind(tenant.as_str())
        .fetch_one(&mut *connection)
        .await?
    } else {
        false
    };
    if bound {
        for module in &mut result.modules {
            if matches!(module.id.as_str(), "accounting" | "billing") {
                module.mandatory = true;
            }
        }
    }
    let purchase_installed: bool = sqlx::query_scalar(
        "SELECT to_regclass('archava_accounting.purchase_policies') IS NOT NULL",
    )
    .fetch_one(&mut *connection)
    .await?;
    let purchase_bound = if purchase_installed {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM archava_accounting.purchase_policies WHERE tenant_id=$1)",
        )
        .bind(tenant.as_str())
        .fetch_one(&mut *connection)
        .await?
    } else {
        false
    };
    if purchase_bound {
        for module in &mut result.modules {
            if matches!(module.id.as_str(), "accounting" | "purchasing" | "commerce") {
                module.mandatory = true;
            }
        }
    }
    let stock_installed: bool =
        sqlx::query_scalar("SELECT to_regclass('archava_accounting.stock_policies') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await?;
    let stock_bound = if stock_installed {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM archava_accounting.stock_policies WHERE tenant_id=$1)",
        )
        .bind(tenant.as_str())
        .fetch_one(&mut *connection)
        .await?
    } else {
        false
    };
    if stock_bound {
        for module in &mut result.modules {
            if matches!(module.id.as_str(), "accounting" | "commerce" | "purchasing") {
                module.mandatory = true;
            }
        }
    }
    Ok(result)
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

    pub(crate) fn response(self) -> (StatusCode, Json<Value>) {
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
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT revision, config::text, catalogue_version FROM archava_owner.config_revisions \
         WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1",
    )
    .bind(tenant.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((revision, raw, version)) => {
            let mut config = parse_config(&raw)?;
            if version == "owner-catalogue@1" {
                for module in catalogue().modules {
                    if module.id != "crm-contacts"
                        && module.id != "crm-pipeline"
                        && module.id != "accounting"
                    {
                        config.enabled.insert(module.id);
                    }
                }
            }
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
    /// Who committed it. `apply_receipts` is keyed by `(tenant, request_id)`,
    /// so the id alone is not a capability — every read and replay is bound
    /// to this column.
    actor: String,
}

/// revision, plan hash, config, catalogue version, pricing version,
/// outcome, and the actor the revision records (LEFT JOINed, so `None` means
/// the ledger lost the revision a receipt points at).
type StoredReceiptRow = (i64, String, String, String, String, String, Option<String>);

async fn stored_receipt(
    connection: &mut PgConnection,
    tenant: &TenantId,
    request_id: &str,
) -> Result<Option<StoredReceipt>, OwnRefusal> {
    let row: Option<StoredReceiptRow> = sqlx::query_as(
        "SELECT r.revision, r.plan_hash, r.config::text, r.catalogue_version, r.pricing_version, \
         r.outcome, v.actor FROM archava_owner.apply_receipts r \
         LEFT JOIN archava_owner.config_revisions v \
           ON v.tenant_id = r.tenant_id AND v.revision = r.revision \
         WHERE r.tenant_id=$1 AND r.request_id=$2",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((revision, plan_hash, config, catalogue_version, pricing_version, outcome, actor)) => {
            // A receipt whose revision row is gone cannot say who committed
            // it, so it must not be replayed or read as anyone's.
            let Some(actor) = actor else {
                return Err(OwnRefusal::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "owner_state_corrupt",
                    "The stored receipt has no revision to bind it to an owner.",
                ));
            };
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
                actor,
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
            "actor": receipt.actor,
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
        actor: principal.to_string(),
    })
}

fn check_confirmation(plan: &Plan, confirmation: &Confirmation) -> Result<(), OwnRefusal> {
    // The client echoes the catalogue/pricing identity it was SHOWN. A stale
    // screen is an explicit conflict with its own reason, never a silent apply:
    // the owner approved an answer that is no longer the current one, and the
    // plan hash alone cannot say which identity moved.
    if let Some(held) = &confirmation.catalogue_version {
        if held != &plan.catalogue_version {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "catalogue_changed",
                "The confirmation was shown a different catalogue; replan and confirm again.",
            )
            .with_detail(serde_json::json!({
                "held": held,
                "live": &plan.catalogue_version,
            })));
        }
    }
    if let Some(held) = &confirmation.pricing_version {
        if held != &plan.pricing_version {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "pricing_changed",
                "The confirmation was shown a different pricing sheet; replan and confirm again.",
            )
            .with_detail(serde_json::json!({
                "held": held,
                "live": &plan.pricing_version,
            })));
        }
    }
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

/// A fixed refusal for a body or query that never reached the authority checks.
///
/// The extractor rejections are handled here rather than by axum's default,
/// whose message can name the caller's own field or value; the detail is not
/// repeated anywhere in the response.
fn malformed(status: StatusCode) -> (StatusCode, Json<Value>) {
    OwnRefusal::new(
        status,
        "malformed_submission",
        "The request could not be processed.",
    )
    .response()
}

/// A query extractor whose rejection this module handles.
type GuardedQuery<T> = std::result::Result<Query<T>, axum::extract::rejection::QueryRejection>;
/// A JSON body extractor under the same rule.
type GuardedJson<T> = std::result::Result<Json<T>, axum::extract::rejection::JsonRejection>;

/// `GET /own/config` — inspect.
pub async fn config(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: GuardedQuery<TenantQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Query(query) = query.map_err(|rejection| malformed(rejection.status()))?;
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
    let catalogue = current_catalogue(connection, &tenant).await?;
    let ai_provider = disclosure::description(state, &config);
    transaction.commit().await?;
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
        "aiProvider": ai_provider,
        "disclosureAreas": archava_domain::operator::catalogue()["modules"].as_array().into_iter().flatten().map(|m|json!({"id":m["id"],"label":m["label"]})).collect::<Vec<_>>(),
        "disclosureCategories": archava_own::DataClass::ALL.iter().map(|c|json!({"id":c.as_str(),"label":c.label(),"description":c.description()})).collect::<Vec<_>>(),
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
    body: GuardedJson<PlanBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|rejection| malformed(rejection.status()))?;
    preview(&state, &headers, body.tenant, body.request)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

/// `POST /own/chat` — the configuration chat's planning entrypoint.
///
/// The message is parsed into the SAME typed `Request` the dashboard posts to
/// `/own/plan`, and the base revision is read from the persisted configuration
/// rather than taken from the caller. Both entrypoints then resolve through
/// [`preview_at`], so the plan identity, catalogue and pricing identity cannot
/// drift between a chat instruction and the dashboard's typed request.
pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: GuardedJson<ChatBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|rejection| malformed(rejection.status()))?;
    let request = parse_chat(&body.message).map_err(OwnRefusal::response)?;
    preview_at(&state, &headers, body.tenant, request, true)
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
    preview_at(state, headers, tenant, request, false).await
}

async fn preview_at(
    state: &AppState,
    headers: &HeaderMap,
    tenant: TenantId,
    mut request: Request,
    derive_base: bool,
) -> Result<Value, OwnRefusal> {
    let principal = session(state, headers, true).await?.principal_id;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.read").await?;
    let current = current_config(connection, &tenant).await?;
    if derive_base {
        request.base_revision = current.revision;
    }
    let plan = current_catalogue(connection, &tenant)
        .await?
        .plan(&current, &request)?;
    if plan.blocked.is_none() {
        disclosure::validate_request(state, connection, &tenant, &principal, &request).await?;
    }
    let disclosure_approval = disclosure::mint(
        disclosure::Review {
            state,
            tenant: &tenant,
            principal: &principal,
            operation: "apply",
            current: &current,
            request: &request,
            plan: &plan,
        },
        connection,
    )
    .await?;
    let ai_provider = disclosure::description(state, &current);
    let disclosure_companies = disclosure::company_review(
        connection,
        &tenant,
        &principal,
        plan.config.ai_disclosure.as_ref(),
    )
    .await;
    transaction.commit().await?;
    Ok(json!({
        "plan": plan,
        "disclosureCompanies":disclosure_companies,
        "disclosureApproval": disclosure_approval,
        "aiProvider": ai_provider,
        "baseConfig": current,
        "request": request,
        "catalogueVersion": plan.catalogue_version,
        "pricingVersion": plan.pricing_version,
    }))
}

/// The canonical pipeline stage ids the chat may relabel. Only the LABEL is
/// configurable; the stage vocabulary is the product's, so a chat instruction
/// cannot invent a stage the CRM does not store.
const PIPELINE_STAGES: &[&str] = &["new", "qualified", "proposal", "won", "lost"];

/// Parse one owner-configuration chat utterance into the dashboard's typed
/// request. The grammar is deliberately small and deterministic:
///
/// - `enable <module>[, <module>…]`
/// - `disable <module>[, <module>…]`
/// - `rename <stage> to <label>`
///
/// The module ids are not validated here; the shared catalogue plan answers an
/// unknown module with its `unknown_module` blocked result, which is the same
/// answer the dashboard gets. A stage outside [`PIPELINE_STAGES`] is refused at
/// the parser because it is not a request the product stores.
fn parse_chat(message: &str) -> Result<Request, OwnRefusal> {
    let trimmed = message.trim();
    if trimmed.is_empty() || trimmed.len() > 512 {
        return Err(OwnRefusal::new(
            StatusCode::BAD_REQUEST,
            "unrecognized_request",
            "The message must be between 1 and 512 characters.",
        ));
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    let mut request = Request::default();
    if matches!(
        verb.as_str(),
        "bisnis" | "usaha" | "business" | "i" | "saya"
    ) {
        let lower = trimmed.to_lowercase().replace([',', ';', '.'], " ");
        let words = lower.split_whitespace().collect::<Vec<_>>();
        let mut disabling = false;
        for word in words {
            match word {
                "tanpa" | "without" | "jangan" => {
                    disabling = true;
                    continue;
                }
                "tambahkan" | "tambah" | "dengan" | "with" | "add" | "dan" | "and" => {
                    disabling = false;
                    continue;
                }
                _ => {}
            }
            let module = module_alias(word);
            if let Some(module) = module {
                let target = if disabling {
                    &mut request.disable
                } else {
                    &mut request.enable
                };
                if !target.iter().any(|id| id == module) {
                    target.push(module.into());
                }
                // Describing a business enables what its modules need, so
                // "hotel" alone yields a plan that can be applied.
                if !disabling {
                    for required in archava_domain::modules::get(module)
                        .map(|module| module.owner_requires)
                        .unwrap_or_default()
                    {
                        if !request.enable.iter().any(|id| id == required) {
                            request.enable.push((*required).into());
                        }
                    }
                }
            }
        }
        if request.enable.is_empty() && request.disable.is_empty() {
            return Err(unrecognized("Name a supported business module, for example hotel, laundry, restaurant, retail, school or field service."));
        }
        return Ok(request);
    }
    match verb.as_str() {
        "enable" | "disable" | "aktifkan" | "nonaktifkan" | "tambahkan" => {
            let modules = chat_modules(rest);
            if modules.is_empty() {
                return Err(unrecognized(
                    "Name at least one module to enable or disable.",
                ));
            }
            if matches!(verb.as_str(), "enable" | "aktifkan" | "tambahkan") {
                request.enable = modules;
            } else {
                request.disable = modules;
            }
        }
        "rename" | "label" => {
            let (stage, label) = split_relabel(rest).ok_or_else(|| {
                unrecognized("Say `rename <stage> to <label>` with a quoted or plain label.")
            })?;
            let stage = stage.trim().to_ascii_lowercase();
            if !PIPELINE_STAGES.contains(&stage.as_str()) {
                return Err(unrecognized(
                    "The stage must be new, qualified, proposal, won or lost.",
                ));
            }
            let label = label.trim().trim_matches('"').trim();
            if label.is_empty() || label.len() > 80 {
                return Err(unrecognized(
                    "The label must be between 1 and 80 characters.",
                ));
            }
            let mut terminology = Terminology::default();
            terminology.pipeline_labels.insert(stage, label.to_string());
            request.terminology = Some(terminology);
        }
        _ => {
            return Err(unrecognized(
                "I can enable or disable a module, or rename a pipeline stage.",
            ))
        }
    }
    Ok(request)
}

fn unrecognized(message: &str) -> OwnRefusal {
    OwnRefusal::new(StatusCode::BAD_REQUEST, "unrecognized_request", message)
}

/// Split a module list on commas/semicolons or the word `and`, then strip the
/// optional article/suffix the chat grammar allows (`the crm-pipeline module`).
fn chat_modules(rest: &str) -> Vec<String> {
    rest.split([',', ';'])
        .flat_map(|part| part.split(" and "))
        .map(|token| token.trim().trim_end_matches('.').trim())
        .map(|token| token.strip_prefix("the ").unwrap_or(token))
        .map(|token| token.strip_suffix(" module").unwrap_or(token))
        .map(|token| token.trim().to_ascii_lowercase())
        .map(|token| module_alias(&token).unwrap_or(&token).to_string())
        .filter(|token| !token.is_empty())
        .filter(|token| !token.contains(char::is_whitespace))
        .collect()
}

/// The owner-configurable module a chat word names. The two CRM capabilities
/// are owner vocabulary of their own; every other word comes from the module
/// registry (`owner.aliases`, and each module's id).
fn module_alias(word: &str) -> Option<&'static str> {
    match word.trim_matches(['?', '!', ':']) {
        "crm-contacts" | "crm" => Some(MANDATORY_MODULE),
        "crm-pipeline" | "pipeline" => Some("crm-pipeline"),
        word => archava_domain::modules::owner_named(word).map(|module| module.id),
    }
}

/// `stage to label` / `stage as label`, with the label kept in its original
/// case. The delimiter is matched case-insensitively without lowercasing the
/// label itself.
fn split_relabel(rest: &str) -> Option<(&str, &str)> {
    let lower = rest.to_ascii_lowercase();
    for delimiter in [" to ", " as "] {
        if let Some(index) = lower.find(delimiter) {
            let stage = &rest[..index];
            let label = &rest[index + delimiter.len()..];
            if !stage.trim().is_empty() && !label.trim().is_empty() {
                return Some((stage, label));
            }
        }
    }
    None
}

/// `POST /own/apply` — confirm and apply exactly one plan.
pub async fn apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: GuardedJson<ApplyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|rejection| malformed(rejection.status()))?;
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
        // The ledger's key is `(tenant, request id)` — tenant-wide. Two owners
        // can name the same id, so only the one who committed it may read or
        // replay it. Without this binding the id alone would be a capability:
        // a second owner could adopt (or probe) the first one's operation.
        if receipt.actor != principal {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "request_id_reused",
                "This request id belongs to another owner's operation.",
            ));
        }
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
    let plan = current_catalogue(connection, &tenant)
        .await?
        .plan(&current, &body.request)?;
    refuse_blocked(&plan)?;
    check_confirmation(&plan, &body.confirmation)?;
    disclosure::validate_request(state, connection, &tenant, &principal, &body.request).await?;
    disclosure::verify(
        disclosure::Review {
            state,
            tenant: &tenant,
            principal: &principal,
            operation: "apply",
            current: &current,
            request: &body.request,
            plan: &plan,
        },
        connection,
        body.disclosure_approval.as_ref(),
    )
    .await?;

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
/// REPLACE the label set with the target's — a stage the target never had is
/// deleted, not merged forward. Revalidated by the catalogue exactly like any
/// other request.
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
        terminology: None,
        replace_terminology: Some(target.terminology),
        ai_disclosure: Some(match target.ai_disclosure {
            Some(policy) => DisclosureChange::Replace { policy },
            None => DisclosureChange::Revoke {},
        }),
    })
}

/// `POST /own/restore/plan` — preview the restore as a plan the client can
/// confirm by hash.
pub async fn restore_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: GuardedJson<RestorePlanBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|rejection| malformed(rejection.status()))?;
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
    let plan = current_catalogue(connection, &tenant)
        .await?
        .plan(&current, &request)?;
    if plan.blocked.is_none() {
        disclosure::validate_request(state, connection, &tenant, &principal, &request).await?;
    }
    let disclosure_approval = disclosure::mint(
        disclosure::Review {
            state,
            tenant: &tenant,
            principal: &principal,
            operation: &format!("restore:{}", body.target_revision),
            current: &current,
            request: &request,
            plan: &plan,
        },
        connection,
    )
    .await?;
    let ai_provider = disclosure::description(state, &current);
    let disclosure_companies = disclosure::company_review(
        connection,
        &tenant,
        &principal,
        plan.config.ai_disclosure.as_ref(),
    )
    .await;
    transaction.commit().await?;
    Ok(json!({
        "plan": plan,
        "disclosureCompanies":disclosure_companies,
        "disclosureApproval": disclosure_approval,
        "aiProvider": ai_provider,
        "baseConfig": current,
        "targetRevision": body.target_revision,
        "catalogueVersion": plan.catalogue_version,
        "pricingVersion": plan.pricing_version,
    }))
}

/// `POST /own/restore` — restore a revision as a NEW revision.
pub async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: GuardedJson<RestoreBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|rejection| malformed(rejection.status()))?;
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
        // The ledger's key is `(tenant, request id)` — tenant-wide. Two owners
        // can name the same id, so only the one who committed it may read or
        // replay it. Without this binding the id alone would be a capability:
        // a second owner could adopt (or probe) the first one's operation.
        if receipt.actor != principal {
            return Err(OwnRefusal::new(
                StatusCode::CONFLICT,
                "request_id_reused",
                "This request id belongs to another owner's operation.",
            ));
        }
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
    let plan = current_catalogue(connection, &tenant)
        .await?
        .plan(&current, &request)?;
    refuse_blocked(&plan)?;
    check_confirmation(&plan, &body.confirmation)?;
    disclosure::validate_request(state, connection, &tenant, &principal, &request).await?;
    disclosure::verify(
        disclosure::Review {
            state,
            tenant: &tenant,
            principal: &principal,
            operation: &format!("restore:{}", body.target_revision),
            current: &current,
            request: &request,
            plan: &plan,
        },
        connection,
        body.disclosure_approval.as_ref(),
    )
    .await?;

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

/// `GET /own/status` — "what did MY operation with this id do?".
///
/// The recovery read for a client whose answer died on the wire after the
/// transaction committed. It is a read: it never plans, never writes, and
/// never replays an execution. A request id with no receipt answers
/// `unknown` — absence is not proof of failure, so the caller keeps its key
/// instead of minting a second operation. A receipt belonging to another
/// principal answers `unknown` too: the ledger's key is tenant-wide, so the
/// actor binding (`receipt.actor == principal`) is what keeps two owners'
/// operations apart.
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: GuardedQuery<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Query(query) = query.map_err(|rejection| malformed(rejection.status()))?;
    status_inner(&state, &headers, query)
        .await
        .map(Json)
        .map_err(OwnRefusal::response)
}

async fn status_inner(
    state: &AppState,
    headers: &HeaderMap,
    query: StatusQuery,
) -> Result<Value, OwnRefusal> {
    validate_request_id(&query.request_id)?;
    let principal = session(state, headers, false).await?.principal_id;
    let tenant = query.tenant;
    let mut transaction = begin(state, &tenant).await?;
    let connection = transaction.checked_executor().await?;
    require_grant(connection, &tenant, &principal, "config.read").await?;
    let found = stored_receipt(connection, &tenant, &query.request_id).await?;
    transaction.commit().await?;
    match found {
        Some(receipt) if receipt.actor == principal => {
            Ok(receipt_json(&query.request_id, &receipt, true))
        }
        _ => Ok(json!({
            "decision": "unknown",
            "replayed": false,
            "requestId": query.request_id,
        })),
    }
}

/// revision, config, actor, request, plan hash, base revision, outcome,
/// created-at — the history column order.
type HistoryRow = (i64, String, String, String, String, i64, String, String);

/// `GET /own/history` — revisions, newest first.
pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: GuardedQuery<TenantQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Query(query) = query.map_err(|rejection| malformed(rejection.status()))?;
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

/// Public CRM labels are presentation data for authorized CRM readers, not an
/// Owner management surface. Call only inside the verified tenant transaction.
pub(crate) async fn crm_presentation(
    connection: &mut PgConnection,
    tenant: &TenantId,
) -> Result<(bool, i64, std::collections::BTreeMap<String, String>), OwnRefusal> {
    let config = current_config(connection, tenant).await?;
    Ok((
        config.enabled.contains("crm-pipeline"),
        config.revision,
        config.terminology.pipeline_labels,
    ))
}

/// The same tenant fence used by owner apply/restore is held through the
/// business commit. A disabled module cannot race a concurrent mutation.
pub(crate) async fn require_erp_module(
    transaction: &mut TenantTransaction<'_>,
    namespace: &str,
) -> Result<(), OwnRefusal> {
    let module = match namespace {
        "archava_field" => "field",
        "archava_edu" => "education",
        name => name.strip_prefix("archava_").unwrap_or(name),
    };
    let tenant = transaction.tenant().clone();
    let conn = transaction.checked_executor().await?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtext($1))")
        .bind(format!("archava-owner:{}", tenant.as_str()))
        .execute(&mut *conn)
        .await?;
    if !module_enabled(conn, &tenant, module).await? {
        return Err(OwnRefusal::new(
            StatusCode::FORBIDDEN,
            "module_disabled",
            "This module is disabled in the workspace configuration.",
        ));
    }
    Ok(())
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

/// `GET /own/status?tenant=…&requestId=…`
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusQuery {
    pub tenant: TenantId,
    pub request_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanBody {
    pub tenant: TenantId,
    pub request: Request,
}

/// The owner-configuration chat's request. Deliberately carries no
/// `baseRevision`, enable/disable list or terminology: the server parses the
/// message and reads the current revision, so the caller cannot smuggle a
/// second, divergent plan into the chat path.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatBody {
    pub tenant: TenantId,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyBody {
    pub tenant: TenantId,
    pub request_id: String,
    pub request: Request,
    pub confirmation: Confirmation,
    #[serde(default)]
    pub disclosure_approval: Option<DisclosureApproval>,
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
    #[serde(default)]
    pub disclosure_approval: Option<DisclosureApproval>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(message: &str) -> Request {
        parse_chat(message).unwrap_or_else(|_| panic!("{message:?} must parse"))
    }

    #[test]
    fn a_business_sentence_enables_the_named_modules_and_what_they_need() {
        // Aliases and requirements come from the module registry.
        let request = chat("bisnis hotel dan laundry tanpa restoran");
        assert_eq!(request.enable, ["accommodation", "scheduling", "laundry"]);
        assert_eq!(request.disable, ["foodservice"]);
        let request = chat("usaha pembelian");
        assert_eq!(request.enable, ["purchasing", "commerce"]);
        assert!(parse_chat("bisnis sulap").is_err());
    }

    #[test]
    fn every_registered_module_is_named_by_its_id_and_aliases() {
        for module in archava_domain::modules::all() {
            let expected = if module.id == "crm" {
                MANDATORY_MODULE
            } else {
                module.id
            };
            assert_eq!(module_alias(module.id), Some(expected), "{}", module.id);
            for alias in module.owner_aliases {
                assert_eq!(module_alias(alias), Some(module.id), "{alias}");
            }
        }
        assert_eq!(module_alias("pipeline"), Some("crm-pipeline"));
        assert_eq!(module_alias("unknown"), None);
        assert_eq!(chat("enable the hotel module").enable, ["accommodation"]);
    }

    #[test]
    fn catalogue_requirements_follow_the_registry() {
        let catalogue = catalogue();
        let requires = |id: &str| {
            catalogue
                .modules
                .iter()
                .find(|module| module.id == id)
                .map(|module| module.requires.clone())
                .unwrap_or_else(|| panic!("{id} is in the catalogue"))
        };
        assert_eq!(requires("accommodation"), ["scheduling"]);
        assert_eq!(requires("accounting"), ["organizations"]);
        assert_eq!(requires("laundry"), [MANDATORY_MODULE]);
        assert!(requires(MANDATORY_MODULE).is_empty());
    }
}
