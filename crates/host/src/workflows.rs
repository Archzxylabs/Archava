//! Immutable reviewed input. Tenant, actor and receipt binding are server facts.
use crate::{
    business::{refused, Response},
    router::AppState,
};
use archava_adapters::{
    erp::{
        commerce_kernel::resolve_commerce_context, purchasing::resolve_purchasing_context,
        PurchaseError,
    },
    rls::TenantTransaction,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::{
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError,
};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;
pub(crate) fn unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(error=%error,"reviewed workflow storage failed");
    refused(StatusCode::SERVICE_UNAVAILABLE, "workflow_unavailable")
}
pub(crate) fn domain_error(error: PurchaseError) -> Response {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => refused(StatusCode::FORBIDDEN, "forbidden"),
        PurchaseError::Domain(ErpError::NotFound) => {
            refused(StatusCode::NOT_FOUND, "unknown_resource")
        }
        PurchaseError::Domain(ErpError::Conflict) => {
            refused(StatusCode::CONFLICT, "business_conflict")
        }
        error => unavailable(format!("{error:?}")),
    }
}
pub(crate) async fn begin<'a>(
    state: &'a AppState,
    tenant: &TenantId,
) -> Result<TenantTransaction<'a>, Response> {
    TenantTransaction::begin(
        state.pool(),
        crate::own::NAMESPACE,
        crate::own::LEDGER,
        tenant.clone(),
    )
    .await
    .map_err(unavailable)
}
pub(crate) async fn authorize(
    conn: &mut PgConnection,
    tenant: &TenantId,
    actor: &str,
    module: &str,
    permission: &str,
) -> Result<(), Response> {
    let principal = PurchasePrincipal {
        kind: PrincipalKind::Human,
        id: actor.into(),
        on_behalf_of: None,
    };
    let permissions = match module {
        "crm" => {
            crate::erp::crm::authorize(
                conn,
                tenant,
                &crate::erp::crm::CrmActor::Human(actor.into()),
            )
            .await
            .map_err(|error| {
                let (s, b) = crate::router::authority_refusal(&error);
                (s, Json(json!(b.0)))
            })?
            .permissions
        }
        "commerce" => {
            resolve_commerce_context(conn, tenant.as_str(), &principal)
                .await
                .map_err(domain_error)?
                .access
                .permissions
        }
        "purchasing" => {
            resolve_purchasing_context(conn, tenant.as_str(), &principal)
                .await
                .map_err(domain_error)?
                .purchasing
                .permissions
        }
        _ => {
            return Err(refused(
                StatusCode::BAD_REQUEST,
                "unsupported_workflow_module",
            ))
        }
    };
    if !permissions.iter().any(|p| p == permission) {
        return Err(refused(StatusCode::FORBIDDEN, "missing_permission"));
    }
    Ok(())
}
pub(crate) async fn issue(
    conn: &mut PgConnection,
    tenant: &TenantId,
    actor: &str,
    kind: &str,
    plan: &Value,
) -> Result<(Uuid, String), Response> {
    if kind == "feed" {
        let lock = format!("archava-feed-review:{}:{actor}:{}", tenant.as_str(), plan);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(lock)
            .execute(&mut *conn)
            .await
            .map_err(unavailable)?;
        let existing:Option<(Uuid,String)>=sqlx::query_as("SELECT id,plan_hash FROM archava_owner.reviewed_workflows WHERE tenant_id=$1 AND principal_id=$2 AND kind='feed' AND plan=$3 AND expires_at>clock_timestamp() AND receipt IS NULL ORDER BY created_at DESC LIMIT 1").bind(tenant.as_str()).bind(actor).bind(plan).fetch_optional(&mut *conn).await.map_err(unavailable)?;
        if let Some(held) = existing {
            return Ok(held);
        }
    }
    let id = Uuid::new_v4();
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!({"tenant":tenant.as_str(),"actor":actor,"kind":kind,"id":id,"plan":plan})
                .to_string()
                .as_bytes()
        )
    );
    sqlx::query("INSERT INTO archava_owner.reviewed_workflows(tenant_id,id,principal_id,kind,plan_hash,plan) VALUES($1,$2,$3,$4,$5,$6)").bind(tenant.as_str()).bind(id).bind(actor).bind(kind).bind(&hash).bind(plan).execute(conn).await.map_err(unavailable)?;
    Ok((id, hash))
}
pub(crate) struct Reviewed {
    pub plan: Value,
    pub hash: String,
    pub receipt: Option<Value>,
}
pub(crate) async fn load(
    conn: &mut PgConnection,
    tenant: &TenantId,
    actor: &str,
    kind: &str,
    id: Uuid,
    hash: Option<&str>,
) -> Result<Reviewed, Response> {
    let row:Option<(Value,String,Option<Value>,bool)>=sqlx::query_as("SELECT plan,plan_hash,receipt,expires_at>clock_timestamp() FROM archava_owner.reviewed_workflows WHERE tenant_id=$1 AND id=$2 AND principal_id=$3 AND kind=$4 FOR UPDATE").bind(tenant.as_str()).bind(id).bind(actor).bind(kind).fetch_optional(&mut *conn).await.map_err(unavailable)?;
    let Some((plan, held, receipt, valid)) = row else {
        return Err(refused(StatusCode::NOT_FOUND, "unknown_workflow"));
    };
    if hash.is_some_and(|h| h != held) {
        return Err(refused(StatusCode::CONFLICT, "review_mismatch"));
    }
    if !valid && receipt.is_none() {
        // A feed effect may have committed before a process died while
        // recording this receipt. Expiration must not make it unrecoverable.
        let committed = if kind == "feed" {
            sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM archava_purchasing.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3 AND receipt IS NOT NULL)").bind(tenant.as_str()).bind(id).bind(actor).fetch_one(&mut *conn).await.map_err(unavailable)?
        } else {
            false
        };
        if !committed {
            return Err(refused(StatusCode::CONFLICT, "review_expired"));
        }
    }
    Ok(Reviewed {
        plan,
        hash: held,
        receipt,
    })
}
pub(crate) async fn record(
    conn: &mut PgConnection,
    tenant: &TenantId,
    id: Uuid,
    receipt: &Value,
) -> Result<(), Response> {
    let result=sqlx::query("UPDATE archava_owner.reviewed_workflows SET receipt=$3 WHERE tenant_id=$1 AND id=$2 AND receipt IS NULL").bind(tenant.as_str()).bind(id).bind(receipt).execute(conn).await.map_err(unavailable)?;
    if result.rows_affected() != 1 {
        return Err(refused(StatusCode::CONFLICT, "receipt_already_recorded"));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusQuery {
    pub tenant: TenantId,
    pub id: Uuid,
    pub kind: String,
}
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<StatusQuery>, QueryRejection>,
) -> Response {
    async fn read(
        state: &AppState,
        headers: &HeaderMap,
        input: StatusQuery,
    ) -> Result<Value, Response> {
        let session = crate::router::authenticate(state, headers, false)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?;
        let mut tx = begin(state, &input.tenant).await?;
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let held = load(
            conn,
            &input.tenant,
            &session.principal_id,
            &input.kind,
            input.id,
            None,
        )
        .await?;
        let module = held.plan["module"]
            .as_str()
            .ok_or_else(|| unavailable("invalid stored module"))?;
        let read = match module {
            "crm" => "contacts.read",
            "commerce" => "catalog.read",
            _ => "orders.read",
        };
        authorize(conn, &input.tenant, &session.principal_id, module, read).await?;
        if module != "crm" {
            crate::own::require_erp_module(
                &mut tx,
                if module == "commerce" {
                    "archava_commerce"
                } else {
                    "archava_purchasing"
                },
            )
            .await
            .map_err(crate::own::OwnRefusal::response)?;
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(
            json!({"id":input.id,"plan_hash":held.hash,"status":if held.receipt.as_ref().is_some_and(|r|r["status"]=="cancelled"){"cancelled"}else if held.receipt.is_some(){"committed"}else{"pending"},"plan":held.plan,"receipt":held.receipt}),
        )
    }
    let input = match query {
        Ok(Query(v)) => v,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_workflow_query"),
    };
    match read(&state, &headers, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub tenant: TenantId,
    pub id: Uuid,
    pub kind: String,
    pub plan_hash: String,
}
pub async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CancelRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        input: CancelRequest,
    ) -> Result<Value, Response> {
        let actor = crate::router::authenticate(state, headers, true)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?
            .principal_id;
        if !matches!(input.kind.as_str(), "import" | "feed") {
            return Err(refused(StatusCode::BAD_REQUEST, "invalid_workflow_kind"));
        }
        let mut tx = begin(state, &input.tenant).await?;
        let plan:Option<Value>=sqlx::query_scalar("SELECT plan FROM archava_owner.reviewed_workflows WHERE tenant_id=$1 AND id=$2 AND principal_id=$3 AND kind=$4 AND plan_hash=$5").bind(input.tenant.as_str()).bind(input.id).bind(&actor).bind(&input.kind).bind(&input.plan_hash).fetch_optional(tx.checked_executor().await.map_err(unavailable)?).await.map_err(unavailable)?;
        let plan = plan.ok_or_else(|| refused(StatusCode::NOT_FOUND, "unknown_workflow"))?;
        tx.commit().await.map_err(unavailable)?;
        let mut serial = state.pool().begin().await.map_err(unavailable)?;
        if input.kind == "feed" {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
                .bind(format!(
                    "archava-feed:{}:{}",
                    input.tenant.as_str(),
                    plan["productId"]
                        .as_str()
                        .ok_or_else(|| unavailable("invalid feed product"))?
                ))
                .execute(&mut *serial)
                .await
                .map_err(unavailable)?;
        }
        let mut tx = begin(state, &input.tenant).await?;
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let held:Option<Option<Value>>=sqlx::query_scalar("SELECT receipt FROM archava_owner.reviewed_workflows WHERE tenant_id=$1 AND id=$2 AND principal_id=$3 AND kind=$4 AND plan_hash=$5 FOR UPDATE").bind(input.tenant.as_str()).bind(input.id).bind(&actor).bind(&input.kind).bind(&input.plan_hash).fetch_optional(&mut *conn).await.map_err(unavailable)?;
        let held = held.ok_or_else(|| refused(StatusCode::NOT_FOUND, "unknown_workflow"))?;
        authorize(
            conn,
            &input.tenant,
            &actor,
            plan["module"].as_str().unwrap_or_default(),
            plan["permission"].as_str().unwrap_or_default(),
        )
        .await?;
        let receipt = if let Some(receipt) = held {
            receipt
        } else {
            let committed = if input.kind == "feed" {
                sqlx::query_scalar::<_,Option<Value>>("SELECT receipt FROM archava_purchasing.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3").bind(input.tenant.as_str()).bind(input.id).bind(&actor).fetch_optional(&mut *conn).await.map_err(unavailable)?.flatten()
            } else {
                None
            };
            let receipt = if let Some(effect) = committed {
                json!({"card_id":input.id,"plan_hash":input.plan_hash,"status":"committed","action_type":"generate_reorder_po","receipt":effect})
            } else {
                json!({"id":input.id,"plan_hash":input.plan_hash,"status":"cancelled","business_effects":0})
            };
            record(conn, &input.tenant, input.id, &receipt).await?;
            receipt
        };
        tx.commit().await.map_err(unavailable)?;
        serial.commit().await.map_err(unavailable)?;
        Ok(
            json!({"id":input.id,"plan_hash":input.plan_hash,"status":if receipt["status"]=="cancelled"{"cancelled"}else{"committed"},"receipt":receipt}),
        )
    }
    if let Err((s, b)) = crate::router::authenticate(&state, &headers, true).await {
        return (s, Json(json!(b.0)));
    }
    let input = match body {
        Ok(Json(v)) => v,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_workflow_cancellation"),
    };
    match run(&state, &headers, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}
