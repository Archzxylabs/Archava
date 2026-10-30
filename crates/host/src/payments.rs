//! Configured Midtrans transport → frozen Billing correlation → verified ledger.
//! No provider is contacted at startup. An absent registry disables the feature.
use crate::{
    erp::{begin_billing, ErpGateError},
    router::{authenticate, AppState},
};
use archava_adapters::erp::{billing::resolve_billing_context, payments as store, PurchaseError};
use archava_core::tenant::TenantId;
use archava_domain::erp::{
    payments::whole_idr,
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError,
};
use archava_providers::midtrans::{MidtransClient, MidtransMode};
use axum::{
    extract::{Path as RoutePath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::Path, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PaymentIntegration {
    pub id: String,
    pub tenant: TenantId,
    pub service_id: String,
    pub client: MidtransClient,
}
#[derive(Clone, Debug, Default)]
pub struct PaymentRegistry(BTreeMap<String, Arc<PaymentIntegration>>);
#[derive(Debug, thiserror::Error)]
#[error("Invalid payment provider configuration; check the file and referenced secret environment variables")]
pub struct ConfigurationError;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    schema_version: u32,
    midtrans: Vec<IntegrationConfig>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntegrationConfig {
    id: String,
    tenant: TenantId,
    service_principal: String,
    mode: String,
    server_key_env: String,
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
impl PaymentRegistry {
    pub fn new(integrations: Vec<PaymentIntegration>) -> Result<Self, ConfigurationError> {
        if integrations.len() > 100 {
            return Err(ConfigurationError);
        }
        let mut values = BTreeMap::new();
        for integration in integrations {
            if !identifier(&integration.id)
                || !identifier(&integration.service_id)
                || values
                    .insert(integration.id.clone(), Arc::new(integration))
                    .is_some()
            {
                return Err(ConfigurationError);
            }
        }
        Ok(Self(values))
    }
    /// Optional file contains environment-variable references, never key values.
    /// Deliberately no arbitrary origin/HTTP override in production configuration.
    pub fn load(path: &Path) -> Result<Self, ConfigurationError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| ConfigurationError)?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfigurationError)?;
        if bytes.len() > 65536 {
            return Err(ConfigurationError);
        }
        let config: Configuration =
            serde_json::from_slice(&bytes).map_err(|_| ConfigurationError)?;
        if config.schema_version != 1 {
            return Err(ConfigurationError);
        }
        let mut integrations = Vec::new();
        for item in config.midtrans {
            if item.server_key_env.is_empty()
                || item.server_key_env.len() > 128
                || !item
                    .server_key_env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(ConfigurationError);
            }
            let key = std::env::var(&item.server_key_env).map_err(|_| ConfigurationError)?;
            let mode = match item.mode.as_str() {
                "sandbox" => MidtransMode::Sandbox,
                "production" => MidtransMode::Production,
                _ => return Err(ConfigurationError),
            };
            integrations.push(PaymentIntegration {
                id: item.id,
                tenant: item.tenant,
                service_id: item.service_principal,
                client: MidtransClient::new(mode, key).map_err(|_| ConfigurationError)?,
            });
        }
        Self::new(integrations)
    }
    fn get(&self, id: &str) -> Result<Arc<PaymentIntegration>, Failure> {
        self.0.get(id).cloned().ok_or(Failure::Unconfigured)
    }
}

#[derive(Debug)]
enum Failure {
    Store(PurchaseError),
    Gate(ErpGateError),
    Unconfigured,
    Invalid,
    Throttle,
    Provider,
}
impl From<PurchaseError> for Failure {
    fn from(e: PurchaseError) -> Self {
        Self::Store(e)
    }
}
impl From<ErpGateError> for Failure {
    fn from(e: ErpGateError) -> Self {
        Self::Gate(e)
    }
}
impl From<archava_adapters::rls::TenantContextError> for Failure {
    fn from(e: archava_adapters::rls::TenantContextError) -> Self {
        Self::Gate(e.into())
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, reason) = match self {
            Self::Store(PurchaseError::Domain(ErpError::Forbidden)) => {
                (StatusCode::FORBIDDEN, "forbidden")
            }
            Self::Store(PurchaseError::Domain(ErpError::NotFound)) => {
                (StatusCode::NOT_FOUND, "unknown_resource")
            }
            Self::Store(PurchaseError::Domain(ErpError::Conflict)) => {
                (StatusCode::CONFLICT, "conflict")
            }
            Self::Invalid => (StatusCode::BAD_REQUEST, "invalid_submission"),
            Self::Store(PurchaseError::Domain(ErpError::Unavailable)) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "payment_creation_held_after_restore",
            ),
            Self::Throttle => (StatusCode::TOO_MANY_REQUESTS, "check_in_progress"),
            Self::Unconfigured => (StatusCode::SERVICE_UNAVAILABLE, "payment_unconfigured"),
            Self::Provider => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_response_unconfirmed",
            ),
            Self::Gate(ErpGateError::Authority(error)) => {
                return crate::router::authority_refusal(&error).into_response()
            }
            Self::Store(PurchaseError::Database { .. })
            | Self::Gate(ErpGateError::Database(..)) => {
                (StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
            }
        };
        (
            status,
            Json(json!({"status":"unavailable","reason":reason})),
        )
            .into_response()
    }
}
fn human(id: String) -> PurchasePrincipal {
    PurchasePrincipal {
        kind: PrincipalKind::Human,
        id,
        on_behalf_of: None,
    }
}
fn service(integration: &PaymentIntegration) -> PurchasePrincipal {
    PurchasePrincipal {
        kind: PrincipalKind::Service,
        id: integration.service_id.clone(),
        on_behalf_of: None,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InvoiceQuery {
    tenant: TenantId,
    invoice_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StatusQuery {
    tenant: TenantId,
    request_id: Uuid,
}
pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusQuery>,
) -> Response {
    let presented = match authenticate(&state, &headers, false).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    async {
        let mut tx = begin_billing(&state, query.tenant.clone()).await?;
        let conn = tx.checked_executor().await?;
        let access =
            resolve_billing_context(conn, query.tenant.as_str(), &human(presented.principal_id))
                .await?;
        let found =
            store::command_status(conn, query.tenant.as_str(), &access, query.request_id).await?;
        tx.rollback().await?;
        Ok::<_, Failure>(Json(match found {
            Some(link) => json!({"status":"recorded","requestId":query.request_id,"link":link}),
            None => json!({"status":"unknown","requestId":query.request_id}),
        }))
    }
    .await
    .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
}
pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<InvoiceQuery>,
) -> Response {
    let presented = match authenticate(&state, &headers, false).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    async {
        let mut tx=begin_billing(&state,query.tenant.clone()).await?;
        let conn=tx.checked_executor().await?;
        let access=resolve_billing_context(conn,query.tenant.as_str(),&human(presented.principal_id)).await?;
        let links=store::invoice_links(conn,query.tenant.as_str(),&access,query.invoice_id).await?;
        let integrations:Vec<&str>=state.payments().0.values().filter(|i|i.tenant==query.tenant).map(|i|i.id.as_str()).collect();
        tx.rollback().await?;
        Ok::<_,Failure>(Json(json!({"links":links,"integrations":integrations,"canCreate":access.holds("billing.write")})))
    }.await.map_or_else(IntoResponse::into_response,IntoResponse::into_response)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateSubmission {
    tenant: TenantId,
    invoice_id: Uuid,
    request_id: Uuid,
    integration_id: String,
}
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSubmission>,
) -> Response {
    let presented = match authenticate(&state, &headers, true).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    create_inner(&state, human(presented.principal_id), body)
        .await
        .map_or_else(IntoResponse::into_response, |v| Json(v).into_response())
}
async fn create_inner(
    state: &AppState,
    principal: PurchasePrincipal,
    body: CreateSubmission,
) -> Result<Value, Failure> {
    let integration = state.payments().get(&body.integration_id)?;
    if integration.tenant != body.tenant {
        return Err(Failure::Store(PurchaseError::Domain(ErpError::Forbidden)));
    }
    let mut tx = begin_billing(state, body.tenant.clone()).await?;
    let conn = tx.checked_executor().await?;
    let access = resolve_billing_context(conn, body.tenant.as_str(), &principal).await?;
    let (link, created) = store::request_link(
        conn,
        body.tenant.as_str(),
        &principal,
        &access,
        store::LinkRequest {
            request_id: body.request_id,
            invoice_id: body.invoice_id,
            integration_id: &integration.id,
            service_id: &integration.service_id,
        },
    )
    .await?;
    tx.commit().await?;
    // The irreversible invocation is claimed before HTTP. A replay reads the
    // original link, never creates another external charge after a timeout.
    if created {
        let response = integration
            .client
            .create_checkout(&link.order_id, link.amount_minor)
            .await;
        let provider = service(&integration);
        let mut tx = begin_billing(state, body.tenant.clone()).await?;
        let conn = tx.checked_executor().await?;
        let access = resolve_billing_context(conn, body.tenant.as_str(), &provider).await?;
        let bound = store::binding(
            conn,
            body.tenant.as_str(),
            &provider,
            &access,
            &integration.id,
            &link.order_id,
        )
        .await?;
        store::finish_creation(
            conn,
            body.tenant.as_str(),
            &provider,
            &access,
            &bound,
            response.as_ref().ok().map(|r| r.redirect_url.as_str()),
        )
        .await?;
        tx.commit().await?;
    }
    // Return metadata only after rechecking the human's current invoice scope.
    let mut tx = begin_billing(state, body.tenant.clone()).await?;
    let conn = tx.checked_executor().await?;
    let access = resolve_billing_context(conn, body.tenant.as_str(), &principal).await?;
    let visible = store::command_status(conn, body.tenant.as_str(), &access, body.request_id)
        .await?
        .ok_or(Failure::Invalid)?;
    tx.rollback().await?;
    Ok(
        json!({"status":"recorded","requestId":body.request_id,"link":visible,"idempotencyReplay":!created}),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckSubmission {
    tenant: TenantId,
    invoice_id: Uuid,
    link_id: Uuid,
}
pub(crate) async fn check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CheckSubmission>,
) -> Response {
    let presented = match authenticate(&state, &headers, true).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    async {
        let principal = human(presented.principal_id);
        let mut tx = begin_billing(&state, body.tenant.clone()).await?;
        let conn = tx.checked_executor().await?;
        let access = resolve_billing_context(conn, body.tenant.as_str(), &principal).await?;
        let links =
            store::invoice_links(conn, body.tenant.as_str(), &access, body.invoice_id).await?;
        let link = links
            .into_iter()
            .find(|l| l.id == body.link_id.to_string())
            .ok_or(Failure::Store(PurchaseError::Domain(ErpError::NotFound)))?;
        tx.rollback().await?;
        let integration = state.payments().get(&link.integration_id)?;
        if integration.tenant != body.tenant {
            return Err(Failure::Invalid);
        }
        let result = synchronize(&state, &integration, &link.order_id, None).await?;
        // The service may finish verification after a human's grants change.
        // Do not disclose the result without another current human scope check.
        let mut tx = begin_billing(&state, body.tenant.clone()).await?;
        let conn = tx.checked_executor().await?;
        let access = resolve_billing_context(conn, body.tenant.as_str(), &principal).await?;
        store::invoice_links(conn, body.tenant.as_str(), &access, body.invoice_id).await?;
        tx.rollback().await?;
        Ok::<_, Failure>(Json(json!({"status":"verified","link":result})))
    }
    .await
    .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
}

async fn synchronize(
    state: &AppState,
    integration: &PaymentIntegration,
    order: &str,
    transaction: Option<&str>,
) -> Result<archava_domain::erp::payments::PaymentLink, Failure> {
    let principal = service(integration);
    let mut tx = begin_billing(state, integration.tenant.clone()).await?;
    let conn = tx.checked_executor().await?;
    let access = resolve_billing_context(conn, integration.tenant.as_str(), &principal).await?;
    let bound = store::binding(
        conn,
        integration.tenant.as_str(),
        &principal,
        &access,
        &integration.id,
        order,
    )
    .await?;
    if bound
        .transaction_id
        .as_deref()
        .zip(transaction)
        .is_some_and(|(a, b)| a != b)
    {
        return Err(Failure::Invalid);
    }
    if !store::claim_check(conn, integration.tenant.as_str(), &bound).await? {
        return Err(Failure::Throttle);
    }
    tx.commit().await?;
    let verified = integration
        .client
        .status(
            order,
            bound.transaction_id.as_deref().or(transaction),
            bound.link.amount_minor,
        )
        .await
        .map_err(|_| Failure::Provider)?;
    let mut tx = begin_billing(state, integration.tenant.clone()).await?;
    let conn = tx.checked_executor().await?;
    let access = resolve_billing_context(conn, integration.tenant.as_str(), &principal).await?;
    let result = store::apply_observation(
        conn,
        integration.tenant.as_str(),
        &principal,
        &access,
        &bound,
        verified.observation(),
    )
    .await?;
    tx.commit().await?;
    Ok(result)
}

#[derive(Deserialize)]
pub(crate) struct Notification {
    order_id: String,
    status_code: String,
    gross_amount: String,
    signature_key: String,
    transaction_id: Option<String>,
}
/// Integration path selects server-side tenant, key and service. Unsigned fields
/// (including transaction_status) are deliberately absent from this authority.
pub(crate) async fn webhook(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<Notification>,
) -> Response {
    async {
        let integration=state.payments().get(&id)?;
        if body.order_id.len()>120||body.status_code.len()>8||body.gross_amount.len()>32||body.signature_key.len()!=128
            ||!integration.client.verifies(&body.order_id,&body.status_code,&body.gross_amount,&body.signature_key){return Err(Failure::Store(PurchaseError::Domain(ErpError::Forbidden)));}
        let gross=whole_idr(&body.gross_amount).map_err(|_|Failure::Invalid)?;
        // Check signed amount against the binding BEFORE making a network call.
        let principal=service(&integration);
        let mut tx=begin_billing(&state,integration.tenant.clone()).await?;
        let conn=tx.checked_executor().await?;
        let access=resolve_billing_context(conn,integration.tenant.as_str(),&principal).await?;
        let bound=store::binding(conn,integration.tenant.as_str(),&principal,&access,&integration.id,&body.order_id).await?;
        if bound.link.amount_minor!=gross{return Err(Failure::Invalid);}
        tx.rollback().await?;
        let link=synchronize(&state,&integration,&body.order_id,body.transaction_id.as_deref()).await?;
        Ok::<_,Failure>(Json(json!({"status":"verified","orderId":link.order_id,"paymentState":link.state,"requiresReview":link.requires_review})))
    }.await.map_or_else(IntoResponse::into_response,IntoResponse::into_response)
}
