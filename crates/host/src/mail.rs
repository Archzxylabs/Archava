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

/// An invoice amount for mail, `<number> <CODE>`. Billing amounts follow the
/// trade convention: whole Rupiah for IDR (printed as before, `150000 IDR`),
/// the ISO minor unit for any other currency, shown with that currency's
/// decimals (`19.99 USD`, `1500 JPY`, `1.250 KWD`). An uncatalogued code keeps
/// the stored integer and says what it is.
fn format_money(amount_minor: i64, currency: &str) -> String {
    match archava_domain::erp::trade_money::trade_decimal(currency, amount_minor) {
        Some(number) => format!("{number} {currency}"),
        None => format!("{amount_minor} {currency} (minor units)"),
    }
}

/// The tax of a taxed invoice as its mail states it: the corrected document
/// total (gross plus corrections) split into net and tax, and the tax code the
/// invoice stored (TAX.md rule 6).
#[derive(Debug, Clone, PartialEq, Eq)]
struct MailTax {
    /// `VAT 11%`, or `VAT 12% of 11/12 of the price`.
    label: String,
    price_includes_tax: bool,
    net_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    /// The tax ids the invoice stored at creation and the tax invoice
    /// reference it carries now, in the order they are printed. Empty for an
    /// invoice without any, whose mail is then what it always was.
    identifiers: Vec<(&'static str, String)>,
}

/// Said under tax ids and a tax invoice reference: text the company entered.
const IDENTIFIER_NOTICE: &str =
    "Tax ids and the tax invoice reference are shown as the company entered them; Archava does not verify them.";

impl MailTax {
    /// Reads the stored snapshot of an invoice row (`to_jsonb`), or `None` for
    /// an untaxed invoice and for an install without the tax columns.
    fn of(stored: &serde_json::Value) -> Option<Self> {
        use archava_domain::erp::tax::{describe, Fraction};
        if stored["tax_code_id"].is_null() {
            return None;
        }
        let integer = |name: &str| stored[name].as_i64();
        let part = |name: &str| integer(name).and_then(|n| i32::try_from(n).ok());
        let total_minor = integer("gross_minor")?.checked_add(integer("correction_minor")?)?;
        let tax_minor = integer("tax_minor")?.checked_add(integer("correction_tax_minor")?)?;
        let mut identifiers = Vec::new();
        for (label, key) in [
            ("Seller tax id", "seller_tax_id"),
            ("Buyer tax id", "buyer_tax_id"),
        ] {
            if let Some(value) = stored[key].as_str() {
                identifiers.push((label, value.to_string()));
            }
        }
        if let Some(reference) = stored["tax_reference"].as_str() {
            identifiers.push((
                "Tax invoice reference",
                match stored["tax_transaction_code"].as_str() {
                    Some(code) => format!("{reference} (transaction code {code})"),
                    None => reference.to_string(),
                },
            ));
        }
        Some(Self {
            label: describe(
                stored["tax_code"].as_str()?,
                Fraction {
                    numerator: part("tax_rate_numerator")?,
                    denominator: part("tax_rate_denominator")?,
                },
                Fraction {
                    numerator: part("tax_base_numerator")?,
                    denominator: part("tax_base_denominator")?,
                },
            ),
            price_includes_tax: stored["price_includes_tax"].as_bool()?,
            net_minor: total_minor.checked_sub(tax_minor)?,
            tax_minor,
            total_minor,
            identifiers,
        })
    }
    fn basis(&self) -> &'static str {
        if self.price_includes_tax {
            "included in the prices"
        } else {
            "added to the prices"
        }
    }
    /// The plain-text block: net, tax with its code and rate, total, and the
    /// notice that the company entered the code (rule 1).
    fn text(&self, currency: &str) -> String {
        let mut text = format!(
            "\nNet: {}\nTax ({}, {}): {}\nTotal: {}\n{}\n",
            format_money(self.net_minor, currency),
            self.label,
            self.basis(),
            format_money(self.tax_minor, currency),
            format_money(self.total_minor, currency),
            archava_domain::erp::tax::DOCUMENT_NOTICE
        );
        for (label, value) in &self.identifiers {
            text.push_str(&format!("{label}: {value}\n"));
        }
        if !self.identifiers.is_empty() {
            text.push_str(IDENTIFIER_NOTICE);
            text.push('\n');
        }
        text
    }
    fn html(&self, currency: &str) -> String {
        let mut html = format!(
            "<h3>Tax</h3><ul><li>Net: {}</li><li>Tax ({}, {}): {}</li><li>Total: <strong>{}</strong></li></ul><p>{}</p>",
            html_escape(&format_money(self.net_minor, currency)),
            html_escape(&self.label),
            self.basis(),
            html_escape(&format_money(self.tax_minor, currency)),
            html_escape(&format_money(self.total_minor, currency)),
            html_escape(archava_domain::erp::tax::DOCUMENT_NOTICE)
        );
        if !self.identifiers.is_empty() {
            html.push_str("<ul>");
            for (label, value) in &self.identifiers {
                html.push_str(&format!("<li>{label}: {}</li>", html_escape(value)));
            }
            html.push_str(&format!("</ul><p>{}</p>", html_escape(IDENTIFIER_NOTICE)));
        }
        html
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
    Duplicate {
        request_id: Uuid,
        invoice_id: Uuid,
        original_request_id: String,
    },
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
            Self::Gate(ErpGateError::Configuration(error)) => {
                return error.response().into_response()
            }
            Self::Duplicate {
                request_id,
                invoice_id,
                original_request_id,
            } => {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "status":"refused", "reason":"email_already_recorded",
                        "requestId":request_id, "invoiceId":invoice_id,
                        "duplicateOf":original_request_id,
                        "message":"These invoice details already have an email transmission. Review its history before sending another message."
                    })),
                ).into_response();
            }
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
            Self::Store(PurchaseError::Domain(ErpError::Conflict) | PurchaseError::Refused(_)) => {
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
    query: std::result::Result<Query<EmailQuery>, axum::extract::rejection::QueryRejection>,
    payload: std::result::Result<Json<SendEmailRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(serde_json::json!({"error":"malformed_query"})),
            )
                .into_response()
        }
    };
    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(serde_json::json!({"error":"malformed_submission"})),
            )
                .into_response()
        }
    };
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

        store::authorize_transmission(
            conn,
            query.tenant.as_str(),
            &human(presented.principal_id.clone()),
            invoice_id,
        ).await?;

        // Idempotency fast path: if this request was already recorded, inspect its state
        let existing_outbox = sqlx::query(
            "SELECT id, request_id, invoice_id, actor_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason, \
             (created_at >= clock_timestamp() - interval '24 hours') AS within_idempotency_window \
             FROM archava_billing.email_outbox \
             WHERE tenant_id = $1 AND request_id = $2",
        )
        .bind(query.tenant.as_str())
        .bind(payload.request_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| ErpGateError::Database("billing", e))?;

        let existing_uncertain = existing_outbox.as_ref().and_then(|row| {
            row.try_get::<String, _>("state")
                .ok()
                .and_then(|state| EmailState::parse(&state))
        }).is_some_and(|state| matches!(state, EmailState::Invoking | EmailState::Unknown));
        let existing_payload_sha256 = existing_outbox
            .as_ref()
            .and_then(|row| row.try_get::<String, _>("payload_sha256").ok());

        if let Some(row) = existing_outbox {
            let recorded_actor: String = row.try_get("actor_id")
                .map_err(|e| ErpGateError::Database("billing", e))?;
            if recorded_actor != presented.principal_id {
                return Err(Failure::Store(PurchaseError::Domain(ErpError::Forbidden)));
            }
            let existing_invoice_id: Uuid = row
                .try_get("invoice_id")
                .map_err(|e| ErpGateError::Database("billing", e))?;
            if existing_invoice_id != invoice_id {
                return Err(Failure::Store(PurchaseError::Domain(ErpError::Conflict)));
            }
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
            "SELECT invoice_code, customer_ref, currency, status, gross_minor, outstanding_minor, (to_jsonb(i)->>'company_name') AS company_name, to_jsonb(i) AS stored \
             FROM archava_billing.invoices i WHERE tenant_id = $1 AND id = $2",
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

        let company_issuer=invoice_row.as_ref().and_then(|row|row.try_get::<Option<String>,_>("company_name").ok().flatten());
        // An untaxed invoice has no tax block and its mail is unchanged.
        let invoice_tax = invoice_row
            .as_ref()
            .and_then(|row| row.try_get::<serde_json::Value, _>("stored").ok())
            .and_then(|stored| MailTax::of(&stored));
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

        let snapshot = if existing_uncertain {
            Some(store::load_email_invocation(&mut *conn, query.tenant.as_str(), payload.request_id).await?)
        } else {
            None
        };

        let (text_content, html_content, digest, from_address) = if let Some(snapshot) = snapshot {
            (
                snapshot.payload_text,
                snapshot.payload_html,
                existing_payload_sha256.ok_or(Failure::Validation)?,
                snapshot.from_address,
            )
        } else {
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
            if let Some(issuer)=&company_issuer{text_content=format!("Company: {issuer}\n{text_content}");}
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
            if let Some(tax) = &invoice_tax {
                text_content.push_str(&tax.text(&currency));
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
            if let Some(issuer)=&company_issuer{html_content=format!("<p>Company: <strong>{}</strong></p>{html_content}",html_escape(issuer));}
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
            if let Some(tax) = &invoice_tax {
                html_content.push_str(&tax.html(&currency));
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
            (
                text_content,
                Some(html_content),
                email_payload.digest(),
                integration.from_address.clone(),
            )
        };

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
                payload_text: &text_content,
                payload_html: html_content.as_deref(),
                from_address: &from_address,
                provider: "resend",
            },
        )
        .await?;

        if receipt.request_id != payload.request_id.to_string() {
            // A new key with identical content was deduplicated to an older
            // intent. Refuse this new intent explicitly; it has no durable
            // receipt of its own and must not leave an unresolvable UI marker.
            return Err(Failure::Duplicate {
                request_id: payload.request_id,
                invoice_id,
                original_request_id: receipt.request_id,
            });
        }
        tx.commit().await?;

        if !should_invoke {
            return Ok::<_, Failure>(Json(receipt));
        }

        let send_result = integration
            .client
            .send_email(
                &payload.request_id.to_string(),
                &from_address,
                recipient.as_str(),
                subject.as_str(),
                &text_content,
                html_content.as_deref(),
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

        // Persist the trusted provider result even if grants changed while
        // HTTP was in flight, then recheck access before disclosing it.
        let mut read_tx = begin_billing(&state, query.tenant.clone()).await?;
        store::authorize_transmission(
            read_tx.checked_executor().await?,
            query.tenant.as_str(),
            &human(presented.principal_id),
            invoice_id,
        ).await?;
        read_tx.rollback().await?;

        Ok::<_, Failure>(Json(final_receipt))
    }
    .await
    .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
}

pub(crate) async fn list_invoice_emails(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(invoice_id): RoutePath<Uuid>,
    query: std::result::Result<Query<EmailQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(serde_json::json!({"error":"malformed_query"})),
            )
                .into_response()
        }
    };
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

#[cfg(test)]
mod tests {
    use super::{format_money, MailTax};
    use serde_json::json;

    fn stored(
        inclusive: bool,
        gross: i64,
        tax: i64,
        correction: i64,
        correction_tax: i64,
    ) -> serde_json::Value {
        json!({"tax_code_id":"01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d","tax_code":"VAT","tax_code_version":1,"tax_kind":"output",
            "tax_rate_numerator":11,"tax_rate_denominator":100,"tax_base_numerator":1,"tax_base_denominator":1,
            "price_includes_tax":inclusive,"gross_minor":gross,"tax_minor":tax,"correction_minor":correction,"correction_tax_minor":correction_tax})
    }

    #[test]
    fn a_taxed_invoice_mail_states_net_tax_total_the_code_and_the_company_notice() {
        let tax = MailTax::of(&stored(false, 33_300, 3_300, 0, 0)).unwrap();
        assert_eq!(
            tax.text("IDR"),
            "\nNet: 30000 IDR\nTax (VAT 11%, added to the prices): 3300 IDR\nTotal: 33300 IDR\nTax codes are entered by the company; Archava does not verify tax rates or rules.\n"
        );
        let html = tax.html("IDR");
        assert!(html.contains("<li>Net: 30000 IDR</li>"), "{html}");
        assert!(
            html.contains("<li>Tax (VAT 11%, added to the prices): 3300 IDR</li>"),
            "{html}"
        );
        assert!(
            html.contains("<li>Total: <strong>33300 IDR</strong></li>"),
            "{html}"
        );
        assert!(
            html.contains("Archava does not verify tax rates or rules."),
            "{html}"
        );

        // Minor units, an inclusive basis and a correction: the corrected
        // total is split, and the parts still add up.
        let tax = MailTax::of(&stored(true, 1_109, 110, -109, -11)).unwrap();
        assert_eq!(
            (tax.net_minor, tax.tax_minor, tax.total_minor),
            (901, 99, 1_000)
        );
        let text = tax.text("USD");
        assert!(text.contains("Net: 9.01 USD"), "{text}");
        assert!(
            text.contains("Tax (VAT 11%, included in the prices): 0.99 USD"),
            "{text}"
        );
        assert!(text.contains("Total: 10.00 USD"), "{text}");

        // A base fraction is named, and a code cannot inject markup.
        let mut scaled = stored(false, 33_300, 3_300, 0, 0);
        scaled["tax_rate_numerator"] = json!(12);
        scaled["tax_base_numerator"] = json!(11);
        scaled["tax_base_denominator"] = json!(12);
        scaled["tax_code"] = json!("<b>");
        let tax = MailTax::of(&scaled).unwrap();
        assert!(tax
            .text("IDR")
            .contains("Tax (<b> 12% of 11/12 of the price, added to the prices): 3300 IDR"));
        assert!(tax
            .html("IDR")
            .contains("Tax (&lt;b&gt; 12% of 11/12 of the price, added to the prices)"));
    }

    #[test]
    fn a_taxed_invoice_mail_states_the_stored_tax_ids_and_reference_when_present() {
        let mut row = stored(false, 33_300, 3_300, 0, 0);
        row["seller_tax_id"] = json!("01.234.567.8-999.000");
        row["buyer_tax_id"] = json!("<9>");
        let tax = MailTax::of(&row).unwrap();
        assert_eq!(
            tax.text("IDR"),
            "\nNet: 30000 IDR\nTax (VAT 11%, added to the prices): 3300 IDR\nTotal: 33300 IDR\nTax codes are entered by the company; Archava does not verify tax rates or rules.\nSeller tax id: 01.234.567.8-999.000\nBuyer tax id: <9>\nTax ids and the tax invoice reference are shown as the company entered them; Archava does not verify them.\n"
        );
        let html = tax.html("IDR");
        assert!(
            html.contains("<li>Seller tax id: 01.234.567.8-999.000</li>"),
            "{html}"
        );
        assert!(html.contains("<li>Buyer tax id: &lt;9&gt;</li>"), "{html}");
        assert!(!html.contains("Tax invoice reference"), "{html}");
        // A recorded reference is printed with its transaction code, if any.
        row["tax_reference"] = json!("010.000-26.00000001");
        assert!(MailTax::of(&row)
            .unwrap()
            .text("IDR")
            .contains("\nTax invoice reference: 010.000-26.00000001\n"));
        row["tax_transaction_code"] = json!("01");
        let tax = MailTax::of(&row).unwrap();
        assert!(tax
            .text("IDR")
            .contains("\nTax invoice reference: 010.000-26.00000001 (transaction code 01)\n"));
        assert!(tax
            .html("IDR")
            .contains("<li>Tax invoice reference: 010.000-26.00000001 (transaction code 01)</li>"));
        // Only the buyer has none: the seller's line stands alone.
        row["buyer_tax_id"] = json!(null);
        let text = MailTax::of(&row).unwrap().text("IDR");
        assert!(text.contains("Seller tax id:") && !text.contains("Buyer tax id:"));
    }

    #[test]
    fn an_untaxed_invoice_has_no_tax_block() {
        assert_eq!(
            MailTax::of(
                &json!({"gross_minor":30_000,"correction_minor":0,"tax_minor":0,"correction_tax_minor":0,"tax_code_id":null})
            ),
            None
        );
        // An install whose Billing ledger predates the tax columns.
        assert_eq!(
            MailTax::of(&json!({"gross_minor":30_000,"correction_minor":0})),
            None
        );
    }

    #[test]
    fn invoice_mail_amounts_follow_the_invoice_currency() {
        // IDR is whole Rupiah and prints exactly as it did before.
        assert_eq!(format_money(150_000, "IDR"), "150000 IDR");
        assert_eq!(format_money(0, "IDR"), "0 IDR");
        assert_eq!(format_money(-15_000, "IDR"), "-15000 IDR");
        // Any other currency is its ISO minor unit.
        assert_eq!(format_money(1_999, "USD"), "19.99 USD");
        assert_eq!(format_money(5, "USD"), "0.05 USD");
        assert_eq!(format_money(-50, "USD"), "-0.50 USD");
        assert_eq!(format_money(1_500, "JPY"), "1500 JPY");
        assert_eq!(format_money(1_500, "KRW"), "1500 KRW");
        assert_eq!(format_money(1_500, "VND"), "1500 VND");
        assert_eq!(format_money(1_250, "KWD"), "1.250 KWD");
        assert_eq!(format_money(1_250, "BHD"), "1.250 BHD");
        // An uncatalogued code never has its digits moved.
        assert_eq!(format_money(1_999, "ZZZ"), "1999 ZZZ (minor units)");
    }
}
