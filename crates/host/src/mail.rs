//! Configured transactional email transport (Resend) → durable Billing outbox correlation.
//! No provider is contacted at startup. An absent registry keeps the feature unconfigured.

use crate::{
    erp::{begin_billing, ErpGateError},
    router::{authenticate, AppState},
};
use archava_adapters::erp::{billing::resolve_billing_context, mail as store, PurchaseError};
use archava_core::tenant::TenantId;
use archava_domain::erp::{
    mail::{EmailPayload, EmailRecipient, EmailState, EmailSubject, EmailValidationError},
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError,
};
use archava_providers::{
    http::TransportError,
    resend::{ResendClient, ResendSuccess},
};
use axum::{
    extract::{Path as RoutePath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use std::{collections::BTreeMap, io::Read, path::Path, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct EmailIntegration {
    pub id: String,
    pub tenant: TenantId,
    pub from_address: String,
    pub client: ResendClient,
}

#[derive(Clone, Debug, Default)]
pub struct EmailRegistry(pub(crate) BTreeMap<String, Arc<EmailIntegration>>);

#[derive(Debug, thiserror::Error)]
#[error("Invalid email provider configuration; check the file and referenced secret environment variables")]
pub struct ConfigurationError;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    schema_version: u32,
    resend: Vec<ResendConfig>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResendConfig {
    id: String,
    tenant: TenantId,
    from_address: String,
    api_key_env: String,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn human(id: String) -> PurchasePrincipal {
    PurchasePrincipal {
        kind: PrincipalKind::Human,
        id,
        on_behalf_of: None,
    }
}

impl EmailRegistry {
    pub fn new(integrations: Vec<EmailIntegration>) -> Result<Self, ConfigurationError> {
        if integrations.len() > 100 {
            return Err(ConfigurationError);
        }
        let mut values = BTreeMap::new();
        for integration in integrations {
            if !identifier(&integration.id)
                || values
                    .insert(integration.id.clone(), Arc::new(integration))
                    .is_some()
            {
                return Err(ConfigurationError);
            }
        }
        Ok(Self(values))
    }

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
        for item in config.resend {
            if item.api_key_env.is_empty()
                || item.api_key_env.len() > 128
                || !item
                    .api_key_env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(ConfigurationError);
            }
            let key = std::env::var(&item.api_key_env).map_err(|_| ConfigurationError)?;
            integrations.push(EmailIntegration {
                id: item.id,
                tenant: item.tenant,
                from_address: item.from_address,
                client: ResendClient::new(key).map_err(|_| ConfigurationError)?,
            });
        }
        Self::new(integrations)
    }

    pub fn for_tenant(&self, tenant: &TenantId) -> Option<Arc<EmailIntegration>> {
        self.0
            .values()
            .find(|integration| &integration.tenant == tenant)
            .cloned()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EmailQuery {
    tenant: TenantId,
    #[serde(default)]
    request_id: Option<Uuid>,
}

fn format_money(amount_minor: i64, currency: &str) -> String {
    match currency.to_uppercase().as_str() {
        "IDR" | "JPY" | "KRW" => format!("{amount_minor} {currency}"),
        _ => {
            let major = amount_minor / 100;
            let minor = (amount_minor % 100).abs();
            format!("{major}.{minor:02} {currency}")
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SendEmailRequest {
    pub request_id: Uuid,
    pub recipient: String,
    pub subject: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug)]
enum Failure {
    Store(PurchaseError),
    Gate(ErpGateError),
    Validation,
    Unconfigured,
    #[allow(dead_code)]
    Transport(TransportError),
}

impl From<PurchaseError> for Failure {
    fn from(value: PurchaseError) -> Self {
        Self::Store(value)
    }
}
impl From<ErpGateError> for Failure {
    fn from(value: ErpGateError) -> Self {
        Self::Gate(value)
    }
}
impl From<archava_adapters::rls::TenantContextError> for Failure {
    fn from(e: archava_adapters::rls::TenantContextError) -> Self {
        Self::Gate(e.into())
    }
}
impl From<EmailValidationError> for Failure {
    fn from(_: EmailValidationError) -> Self {
        Self::Validation
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, reason) = match self {
            Self::Gate(ErpGateError::Authority(error)) => {
                return crate::router::authority_refusal(&error).into_response();
            }
            Self::Gate(ErpGateError::Database(..))
            | Self::Store(PurchaseError::Database { .. }) => {
                (StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
            }
            Self::Store(PurchaseError::Domain(ErpError::Forbidden)) => {
                (StatusCode::FORBIDDEN, "forbidden")
            }
            Self::Store(PurchaseError::Domain(ErpError::NotFound)) => {
                (StatusCode::NOT_FOUND, "not_found")
            }
            Self::Store(PurchaseError::Domain(ErpError::Conflict)) => {
                (StatusCode::CONFLICT, "conflict")
            }
            Self::Store(PurchaseError::Domain(ErpError::Unavailable)) => {
                (StatusCode::SERVICE_UNAVAILABLE, "outbox_unavailable")
            }
            Self::Validation => (StatusCode::BAD_REQUEST, "invalid_email_request"),
            Self::Unconfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "email_provider_unconfigured",
            ),
            Self::Transport(_) => (StatusCode::BAD_GATEWAY, "email_provider_failed"),
        };
        (
            status,
            Json(json!({"status":"unavailable","reason":reason})),
        )
            .into_response()
    }
}

fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

pub(crate) async fn send_invoice_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(invoice_id): RoutePath<Uuid>,
    Query(query): Query<EmailQuery>,
    Json(payload): Json<SendEmailRequest>,
) -> Response {
    let presented = match authenticate(&state, &headers, true).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    async {
        let integration = state
            .email_registry()
            .for_tenant(&query.tenant)
            .ok_or(Failure::Unconfigured)?;

        let recipient = EmailRecipient::new(&payload.recipient)?;
        let default_sub = format!("Invoice from {}", query.tenant.as_str());
        let subject = EmailSubject::new(payload.subject.as_deref().unwrap_or(&default_sub))?;

        let mut tx = begin_billing(&state, query.tenant.clone()).await?;
        let conn = tx.checked_executor().await?;

        // Idempotency fast path: if this request was already recorded, inspect its state
        let existing_outbox = sqlx::query(
            "SELECT id, request_id, invoice_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason, \
             (created_at >= clock_timestamp() - interval '24 hours') AS within_idempotency_window \
             FROM archava_billing.email_outbox \
             WHERE tenant_id = $1 AND request_id = $2",
        )
        .bind(query.tenant.as_str())
        .bind(payload.request_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ErpGateError::Database("billing", e))?;

        if let Some(row) = existing_outbox {
            let existing_recipient: String = row.try_get("recipient").unwrap_or_default();
            let existing_subject: String = row.try_get("subject").unwrap_or_default();
            if existing_recipient != recipient.as_str() || existing_subject != subject.as_str() {
                return Err(Failure::Store(PurchaseError::Domain(ErpError::Conflict)));
            }
            let state_str: String = row.try_get("state").unwrap_or_default();
            let state = EmailState::parse(&state_str);
            if matches!(
                state,
                Some(EmailState::Accepted | EmailState::Rejected | EmailState::Held)
            ) {
                let receipt = store::receipt_from_row(&row)
                    .map_err(|e| ErpGateError::Database("billing", sqlx::Error::Decode(Box::new(e))))?;
                tx.commit().await?;
                return Ok::<_, Failure>(Json(receipt));
            }

            let within_window: bool = row.try_get("within_idempotency_window").unwrap_or(true);
            if !within_window {
                // C10-05: Upstream idempotency window expired (>24h). Blind retry risks duplicate sends.
                sqlx::query(
                    "UPDATE archava_billing.email_outbox \
                     SET state = 'held', error_reason = 'Idempotency window expired (>24h); reconciliation required' \
                     WHERE tenant_id = $1 AND request_id = $2",
                )
                .bind(query.tenant.as_str())
                .bind(payload.request_id)
                .execute(&mut *conn)
                .await
                .map_err(|e| ErpGateError::Database("billing", e))?;
                tx.commit().await?;
                return Err(Failure::Store(PurchaseError::Domain(ErpError::Unavailable)));
            }
        }

        let invoice_row = sqlx::query(
            "SELECT invoice_code, customer_ref, currency, status, gross_minor, outstanding_minor \
             FROM archava_billing.invoices WHERE tenant_id = $1 AND id = $2",
        )
        .bind(query.tenant.as_str())
        .bind(invoice_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ErpGateError::Database("billing", e))?;

        let invoice_lines = sqlx::query(
            "SELECT description, quantity, unit_minor, line_total_minor \
             FROM archava_billing.invoice_lines WHERE tenant_id = $1 AND invoice_id = $2 ORDER BY id",
        )
        .bind(query.tenant.as_str())
        .bind(invoice_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| ErpGateError::Database("billing", e))?;

        let payment_link: Option<String> = sqlx::query_scalar::<_, String>(
            "SELECT redirect_url FROM archava_billing.gateway_links \
             WHERE tenant_id = $1 AND invoice_id = $2 AND closed_at IS NULL AND redirect_url IS NOT NULL \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(query.tenant.as_str())
        .bind(invoice_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ErpGateError::Database("billing", e))?;

        let (invoice_code, customer_ref, currency, outstanding_minor): (
            String,
            String,
            String,
            i64,
        ) = match invoice_row {
            Some(row) => (
                row.try_get::<String, _>("invoice_code")
                    .unwrap_or_else(|_| invoice_id.to_string()),
                row.try_get::<String, _>("customer_ref").unwrap_or_default(),
                row.try_get::<String, _>("currency")
                    .unwrap_or_else(|_| "IDR".to_string()),
                row.try_get::<i64, _>("outstanding_minor").unwrap_or(0),
            ),
            None => (invoice_id.to_string(), String::new(), "IDR".to_string(), 0),
        };

        let raw_message = payload
            .message
            .as_deref()
            .unwrap_or("Please review your invoice details below.");
        let escaped_message = html_escape(raw_message);
        let formatted_due = format_money(outstanding_minor, &currency);

        let mut text_content = format!(
            "Invoice: {}\nCustomer: {}\nTotal Due: {}\n\n{}\n",
            invoice_code, customer_ref, formatted_due, raw_message
        );
        if !invoice_lines.is_empty() {
            text_content.push_str("\nLine items:\n");
            for line in &invoice_lines {
                let desc: String = line.try_get("description").unwrap_or_default();
                let qty: i32 = line.try_get("quantity").unwrap_or(1);
                let total: i64 = line.try_get("line_total_minor").unwrap_or(0);
                let formatted_line = format_money(total, &currency);
                text_content.push_str(&format!("- {desc} (x{qty}): {formatted_line}\n"));
            }
        }
        if let Some(ref url) = payment_link {
            text_content.push_str(&format!("\nPay online: {url}\n"));
        }

        let mut html_content = format!(
            "<h2>Invoice {}</h2><p>Customer: <strong>{}</strong></p><p>Total Due: <strong>{}</strong></p><p>{}</p>",
            html_escape(&invoice_code),
            html_escape(&customer_ref),
            html_escape(&formatted_due),
            escaped_message
        );
        if !invoice_lines.is_empty() {
            html_content.push_str("<h3>Line items</h3><ul>");
            for line in &invoice_lines {
                let desc: String = line.try_get("description").unwrap_or_default();
                let qty: i32 = line.try_get("quantity").unwrap_or(1);
                let total: i64 = line.try_get("line_total_minor").unwrap_or(0);
                let formatted_line = format_money(total, &currency);
                html_content.push_str(&format!(
                    "<li>{} (x{}) &mdash; {}</li>",
                    html_escape(&desc),
                    qty,
                    html_escape(&formatted_line)
                ));
            }
            html_content.push_str("</ul>");
        }
        if let Some(ref url) = payment_link {
            html_content.push_str(&format!(
                "<p><a href=\"{}\">Pay online</a></p>",
                html_escape(url)
            ));
        }

        let email_payload = EmailPayload::new(
            recipient.clone(),
            subject.clone(),
            text_content.clone(),
            Some(html_content.clone()),
        )?;
        let digest = email_payload.digest();

        let (receipt, should_invoke) = store::prepare_email_transmission(
            conn,
            query.tenant.as_str(),
            &human(presented.principal_id.clone()),
            store::EmailTransmissionRequest {
                request_id: payload.request_id,
                invoice_id,
                recipient: &recipient,
                subject: &subject,
                payload_sha256: &digest,
                provider: "resend",
            },
        )
        .await?;

        tx.commit().await?;

        if !should_invoke {
            return Ok::<_, Failure>(Json(receipt));
        }

        let send_result = integration
            .client
            .send_email(
                &payload.request_id.to_string(),
                &integration.from_address,
                recipient.as_str(),
                subject.as_str(),
                &text_content,
                Some(&html_content),
            )
            .await;

        let mut update_tx = begin_billing(&state, query.tenant.clone()).await?;
        let update_conn = update_tx.checked_executor().await?;

        let final_receipt = match send_result {
            Ok(ResendSuccess { id }) => {
                store::record_email_outcome(
                    update_conn,
                    query.tenant.as_str(),
                    payload.request_id,
                    EmailState::Accepted,
                    Some(&id),
                    None,
                )
                .await?
            }
            Err(TransportError::Status(code)) if code == 409 || code == 422 => {
                store::record_email_outcome(
                    update_conn,
                    query.tenant.as_str(),
                    payload.request_id,
                    EmailState::Rejected,
                    None,
                    Some(&format!("Provider refused HTTP {code}")),
                )
                .await?
            }
            Err(e) => {
                store::record_email_outcome(
                    update_conn,
                    query.tenant.as_str(),
                    payload.request_id,
                    EmailState::Unknown,
                    None,
                    Some(&format!("Transport failure: {e:?}")),
                )
                .await?
            }
        };

        update_tx.commit().await?;

        Ok::<_, Failure>(Json(final_receipt))
    }
    .await
    .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
}

pub(crate) async fn list_invoice_emails(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(invoice_id): RoutePath<Uuid>,
    Query(query): Query<EmailQuery>,
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

        let list = store::list_invoice_emails(
            conn,
            query.tenant.as_str(),
            &access,
            invoice_id,
            query.request_id,
        )
        .await?;

        tx.rollback().await?;

        Ok::<_, Failure>(Json(list))
    }
    .await
    .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
}
