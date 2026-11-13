//! System routes: health, readiness probes, Prometheus metrics, static assets, and embed checkout.

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::Row;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::router::{authenticate, AppState};
use crate::routes::auth::LoginSource;
use archava_adapters::erp::{
    commerce_kernel::resolve_commerce_context, PurchaseError, COMMERCE_LEDGER, COMMERCE_NAMESPACE,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::ErpError;

/// Mounts the public, system, static assets, and embed checkout routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics_endpoint))
        .route("/v1/archava.js", get(serve_embed_script))
        .route(
            "/v1/voice-worklet.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("../../../../static/voice-worklet.js"),
                )
            }),
        )
        .route(
            "/v1/voice.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("../../../../static/voice.js"),
                )
            }),
        )
        .route(
            "/v1/livekit-client.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("../../../../static/vendor/livekit/livekit-client.umd.js"),
                )
            }),
        )
        .route("/archava.css", get(serve_archava_css))
        .route("/workspace.css", get(serve_workspace_css))
        .route("/embed/checkout", get(serve_embed_checkout))
        // The mint for that signature: session + CSRF first, then the host's
        // own identity key signs exactly the values the document renders.
        .route("/embed/checkout/sign", post(sign_embed_checkout))
}

/// Universal 3rd-Party Embed Script for external websites (Stripe.js / Snap pattern).
pub async fn serve_embed_script() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../../static/archava.js"),
    )
}

/// Static enterprise stylesheets for sovereign UI rendering.
pub async fn serve_archava_css() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../../static/archava.css"),
    )
}

pub async fn serve_workspace_css() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../../static/workspace.css"),
    )
}

#[derive(Deserialize)]
pub struct EmbedCheckoutQuery {
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(rename = "orderId", default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub amount: Option<u64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub item: Option<String>,
    /// The HMAC over the rendered values, minted by `/embed/checkout/sign`.
    #[serde(default)]
    pub sig: Option<String>,
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub fn sanitize_identifier(input: &str, fallback: &str) -> String {
    let sanitized: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

/// The canonical string an embedded-checkout signature binds.
///
/// It is built from the *sanitised* values, because those are what the document
/// renders: signing the raw query would let a caller change a character that
/// sanitisation drops and still present the same signature.
pub fn canonical_checkout(
    tenant: &str,
    order: &str,
    amount: u64,
    currency: &str,
    item: &str,
) -> String {
    format!("{tenant}|{order}|{amount}|{currency}|{item}")
}

/// Percent-encodes a value for the signed checkout URL, so a display name with
/// spaces or punctuation survives the round trip without changing meaning.
pub fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The fields a checkout document renders, after sanitisation.
pub struct CheckoutParams {
    pub tenant: String,
    pub order: String,
    pub amount: u64,
    pub currency: String,
    pub item: String,
    pub sig: Option<String>,
}

pub fn checkout_params(raw: EmbedCheckoutQuery) -> CheckoutParams {
    let tenant = sanitize_identifier(raw.tenant.as_deref().unwrap_or("default"), "default");
    let order = sanitize_identifier(
        raw.order_id.as_deref().unwrap_or("ord_pending"),
        "ord_pending",
    );
    let amount = raw.amount.unwrap_or(0);
    let currency: String = raw
        .currency
        .unwrap_or_else(|| "IDR".to_string())
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(6)
        .collect();
    let currency = if currency.is_empty() {
        "IDR".to_string()
    } else {
        currency.to_uppercase()
    };
    let item = raw
        .item
        .unwrap_or_else(|| "Archava Verified Checkout".to_string());
    CheckoutParams {
        tenant,
        order,
        amount,
        currency,
        item,
        sig: raw.sig,
    }
}

/// A fixed refusal for the checkout document: no part of the caller's input is
/// repeated, matching the refusal shape the rest of the host uses.
pub fn checkout_refusal(status: StatusCode, reason: &'static str) -> axum::response::Response {
    (
        status,
        [
            (axum::http::header::CONTENT_TYPE, "application/json"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        serde_json::json!({ "error": reason }).to_string(),
    )
        .into_response()
}

/// The embedded checkout document.
///
/// This was an anonymous page that trusted its query string, so any caller
/// could frame a checkout naming its own tenant, order and amount on this
/// origin. It now renders only for a document whose `sig` matches the HMAC over
/// exactly the sanitised values below; the mint is `POST /embed/checkout/sign`,
/// which needs a session and the CSRF header. Identifiers are still sanitised
/// and the item name is still HTML-escaped, so a *signed* document cannot smuggle
/// markup either.
pub async fn serve_embed_checkout(
    State(state): State<AppState>,
    query: Result<Query<EmbedCheckoutQuery>, QueryRejection>,
) -> axum::response::Response {
    let raw = match query {
        Ok(Query(raw)) => raw,
        Err(rejection) => {
            return checkout_refusal(rejection.status(), "malformed_checkout");
        }
    };
    let params = checkout_params(raw);
    let canonical = canonical_checkout(
        &params.tenant,
        &params.order,
        params.amount,
        &params.currency,
        &params.item,
    );
    if !state
        .identity()
        .valid_embed_signature(&canonical, params.sig.as_deref())
    {
        return checkout_refusal(StatusCode::FORBIDDEN, "checkout_signature_required");
    }

    let tenant_id = params.tenant;
    let order_id = params.order;
    let amount = params.amount;
    let currency = params.currency;
    let item_name = escape_html(&params.item);

    let formatted_amount = if currency == "IDR" {
        format!("Rp {}", amount)
    } else {
        format!("{} {}", currency, amount)
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Archava Checkout - {order_id}</title>
  <style>
    :root {{
      --bg: #090d16;
      --card-bg: rgba(15, 23, 42, 0.75);
      --border: rgba(255, 255, 255, 0.12);
      --text: #f8fafc;
      --text-muted: #94a3b8;
      --primary: #06b6d4;
      --primary-hover: #0891b2;
      --accent: #10b981;
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
    body {{
      background: var(--bg);
      color: var(--text);
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      padding: 24px;
      -webkit-font-smoothing: antialiased;
    }}
    .checkout-card {{
      background: var(--card-bg);
      border: 1px solid var(--border);
      border-radius: 20px;
      padding: 32px;
      width: 100%;
      max-width: 480px;
      box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.7);
      backdrop-filter: blur(16px);
    }}
    .badge {{
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      border-radius: 9999px;
      background: rgba(6, 182, 212, 0.15);
      color: var(--primary);
      font-size: 12px;
      font-weight: 600;
      margin-bottom: 20px;
    }}
    .order-title {{
      font-size: 20px;
      font-weight: 700;
      margin-bottom: 8px;
      text-wrap: balance;
    }}
    .order-id {{
      color: var(--text-muted);
      font-size: 13px;
      font-family: monospace;
      margin-bottom: 24px;
    }}
    .detail-row {{
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 12px 0;
      border-bottom: 1px solid rgba(255, 255, 255, 0.08);
      font-size: 14px;
      font-variant-numeric: tabular-nums;
    }}
    .detail-row:last-child {{
      border-bottom: none;
      padding-top: 16px;
      font-size: 18px;
      font-weight: 700;
    }}
    .amount-highlight {{
      color: var(--accent);
      font-weight: 700;
      font-variant-numeric: tabular-nums;
    }}
    .payment-methods {{
      margin: 24px 0;
      display: flex;
      flex-direction: column;
      gap: 10px;
    }}
    .method-option {{
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 12px 16px;
      border: 1px solid var(--border);
      border-radius: 12px;
      cursor: pointer;
      background: rgba(255, 255, 255, 0.03);
      transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
    }}
    .method-option:hover, .method-option.active {{
      border-color: var(--primary);
      background: rgba(6, 182, 212, 0.08);
    }}
    .btn-pay {{
      width: 100%;
      padding: 14px;
      border: none;
      border-radius: 12px;
      background: linear-gradient(135deg, #06b6d4, #0284c7);
      color: white;
      font-size: 16px;
      font-weight: 600;
      font-variant-numeric: tabular-nums;
      cursor: pointer;
      transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
    }}
    .btn-pay:hover {{
      opacity: 0.95;
      transform: translateY(-1px);
      box-shadow: 0 8px 20px rgba(6, 182, 212, 0.35);
    }}
    .btn-pay:active {{
      transform: translateY(1px);
    }}
    .btn-cancel {{
      width: 100%;
      margin-top: 10px;
      padding: 10px;
      border: none;
      background: transparent;
      color: var(--text-muted);
      font-size: 13px;
      cursor: pointer;
    }}
    .btn-cancel:hover {{
      color: var(--text);
    }}
    .footer-note {{
      margin-top: 20px;
      text-align: center;
      font-size: 11px;
      color: var(--text-muted);
    }}
  </style>
</head>
<body>
  <div class="checkout-card">
    <div class="badge" style="background: rgba(245, 158, 11, 0.15); color: #f59e0b; border: 1px solid rgba(245, 158, 11, 0.3);">
      <span style="font-size:14px;">&#x26A0;</span> Interactive Checkout Sandbox (Simulation Mode)
    </div>
    <h1 class="order-title">{item_name}</h1>
    <div class="order-id">Ref: {order_id} &bull; Tenant: {tenant_id}</div>

    <div class="detail-row">
      <span style="color:var(--text-muted);">Line Items</span>
      <span>1 item</span>
    </div>
    <div class="detail-row">
      <span style="color:var(--text-muted);">Settlement Model</span>
       <span style="color:#f59e0b;">Provider settlement required</span>
    </div>
    <div class="detail-row">
      <span>Total Amount</span>
      <span class="amount-highlight">{formatted_amount}</span>
    </div>

    <div class="payment-methods">
      <div class="method-option active">
         <input type="radio" name="pay_method" checked id="m_qris" disabled>
         <label for="m_qris" style="flex:1;">QRIS / configured provider</label>
      </div>
      <div class="method-option">
         <input type="radio" name="pay_method" id="m_va" disabled>
         <label for="m_va" style="flex:1;">Virtual Account / configured provider</label>
      </div>
    </div>

     <button id="pay-btn" class="btn-pay" disabled>Payment provider unavailable</button>
     <button class="btn-cancel" onclick="cancelPayment()">Close checkout</button>

    <div class="footer-note">
       Display only &bull; no financial transaction was initiated
    </div>
  </div>

  <script>
    var orderId = "{order_id}";
    var tenantId = "{tenant_id}";
     function cancelPayment() {{
       window.parent.postMessage({{
        type: 'ARCHAVA_PAYMENT_CANCEL',
        orderId: orderId
      }}, '*');
    }}
  </script>
</body>
</html>"#
    );

    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

/// The signed-checkout mint request.
#[derive(Deserialize)]
pub struct SignCheckoutRequest {
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(rename = "orderId", default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub amount: Option<u64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub item: Option<String>,
}

/// Reads the authoritative commerce quote inside the tenant-scoped RLS
/// transaction. The embed request may display a caller-supplied label, but it
/// may not invent an order, amount, or currency.
async fn checkout_order_quote(
    state: &AppState,
    tenant: &TenantId,
    order: &str,
    principal_id: &str,
) -> Result<Option<(u64, String)>, StatusCode> {
    if state.ledger().namespace != COMMERCE_NAMESPACE {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let order_id = Uuid::parse_str(order).map_err(|_| StatusCode::NOT_FOUND)?;
    let mut transaction = archava_adapters::rls::TenantTransaction::begin(
        state.pool(),
        COMMERCE_NAMESPACE,
        COMMERCE_LEDGER,
        tenant.clone(),
    )
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let connection = transaction
        .checked_executor()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let principal = PurchasePrincipal::human(principal_id.to_owned());
    let context =
        match resolve_commerce_context(&mut *connection, tenant.as_str(), &principal).await {
            Ok(context) => context,
            Err(PurchaseError::Domain(ErpError::Forbidden)) => return Err(StatusCode::FORBIDDEN),
            Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
        };
    if !context.access.holds(&["orders.read"]) {
        return Err(StatusCode::FORBIDDEN);
    }
    match archava_adapters::erp::company_trade::authorize(
        &mut *connection,
        tenant.as_str(),
        &principal,
        archava_adapters::erp::company_trade::Record::Sale,
        order_id,
    )
    .await
    {
        Ok(_) => {}
        Err(PurchaseError::Domain(ErpError::Forbidden)) => return Err(StatusCode::FORBIDDEN),
        Err(PurchaseError::Domain(ErpError::NotFound)) => return Ok(None),
        Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
    }
    let row = sqlx::query(
        "SELECT total_estimate, currency
         FROM archava_commerce.orders
         WHERE tenant_id = $1 AND id = $2 AND state IN ('draft', 'reserved')
           AND ($3 OR owner_id = $4)",
    )
    .bind(tenant.as_str())
    .bind(order_id)
    .bind(context.access.scope.covers_all())
    .bind(&context.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    row.map(|row| {
        let amount = row
            .try_get::<i64, _>("total_estimate")
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let amount = u64::try_from(amount).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let currency = row
            .try_get::<String, _>("currency")
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        Ok((amount, currency))
    })
    .transpose()
}

/// `POST /embed/checkout/sign` — exchanges a session for one signed checkout
/// URL.
///
/// This is the only way to obtain a `sig`, which is what makes the document
/// unforgeable by a caller that can reach the page directly. It signs exactly
/// what the page will render, sanitisation included, so the two sides cannot
/// disagree about what was approved.
pub async fn sign_embed_checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<SignCheckoutRequest>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let principal = match authenticate(&state, &headers, true).await {
        Ok(session) => session.principal_id,
        Err((status, _)) => {
            let reason = if status == StatusCode::FORBIDDEN {
                "csrf_required"
            } else {
                "unauthenticated"
            };
            return (status, Json(serde_json::json!({ "error": reason })));
        }
    };
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => {
            tracing::warn!(error = %rejected, "refused an embed signature request at the boundary");
            return (
                rejected.status(),
                Json(serde_json::json!({ "error": "malformed_submission" })),
            );
        }
    };

    // A signature is an authority-bearing checkout document. Do not let any
    // authenticated user mint a signed document for an arbitrary tenant or a
    // made-up order. The browser may display caller-supplied values, but the
    // signer must first prove active membership in the requested tenant.
    let (Some(raw_tenant), Some(raw_order)) =
        (submission.tenant.as_deref(), submission.order_id.as_deref())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "tenant_and_order_required" })),
        );
    };
    let requested_amount = submission.amount;
    let requested_currency = submission.currency.clone();
    let params = checkout_params(EmbedCheckoutQuery {
        tenant: Some(raw_tenant.to_string()),
        order_id: Some(raw_order.to_string()),
        amount: requested_amount,
        currency: requested_currency.clone(),
        item: submission.item,
        sig: None,
    });
    let tenant = match TenantId::new(&params.tenant) {
        Ok(tenant) => tenant,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_tenant" })),
            )
        }
    };
    let quote = match checkout_order_quote(&state, &tenant, &params.order, &principal).await {
        Ok(Some(quote)) => quote,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "checkout_order_not_found" })),
            )
        }
        Err(status) => {
            let error = if status == StatusCode::FORBIDDEN {
                "forbidden_checkout_access"
            } else {
                "checkout_storage_unavailable"
            };
            return (status, Json(serde_json::json!({ "error": error })));
        }
    };
    if requested_amount.is_some_and(|amount| amount != quote.0)
        || requested_currency.is_some_and(|currency| currency.to_ascii_uppercase() != quote.1)
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "checkout_quote_mismatch" })),
        );
    }
    let params = CheckoutParams {
        tenant: params.tenant,
        order: params.order,
        amount: quote.0,
        currency: quote.1,
        item: params.item,
        sig: None,
    };

    let signature = state.identity().embed_signature(&canonical_checkout(
        &params.tenant,
        &params.order,
        params.amount,
        &params.currency,
        &params.item,
    ));
    let url = format!(
        "/embed/checkout?tenant={}&orderId={}&amount={}&currency={}&item={}&sig={signature}",
        percent_encode(&params.tenant),
        percent_encode(&params.order),
        params.amount,
        percent_encode(&params.currency),
        percent_encode(&params.item),
    );
    (StatusCode::OK, Json(serde_json::json!({ "url": url })))
}

/// Liveness. Carries no tenant and reads no tenant data, which is why it is one
/// of the routes that may run before any authority check. It says nothing
/// about the database: a process that can answer has not crashed, which is the
/// only question a liveness probe is entitled to ask.
pub async fn health() -> &'static str {
    "ok"
}

/// Readiness. The one question liveness cannot answer: can this process still
/// reach PostgreSQL? A failed probe answers 503, so a load balancer drains the
/// instance instead of sending it traffic it cannot serve.
pub async fn ready(State(state): State<AppState>) -> (StatusCode, &'static str) {
    if database_ready(&state).await {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

/// Whether this scrape may read the counters.
///
/// A loopback peer is trusted without a credential (the local scrape agent),
/// and every other peer must present the configured token. An embedder that
/// serves without `ConnectInfo` has no peer at all, which is `None` and is
/// therefore *not* trusted — the opposite of the login-bucket fallback, because
/// here the fallback would be an open door rather than a coarse bucket.
pub fn metrics_authorized(token: Option<&str>, source: &LoginSource, headers: &HeaderMap) -> bool {
    if source.peer.is_some_and(|peer| peer.is_loopback()) {
        return true;
    }
    let (Some(expected), Some(presented)) = (
        token,
        headers
            .get("x-archava-metrics-token")
            .and_then(|value| value.to_str().ok()),
    ) else {
        return false;
    };
    expected.as_bytes().ct_eq(presented.as_bytes()).into()
}

/// Prometheus text exposition. `db_ready` is sampled per scrape rather than
/// cached, so a database restart is visible on the next collection.
///
/// The gate is in front of the sample: an unauthenticated scrape must not be
/// able to turn a public route into a database probe.
pub async fn metrics_endpoint(
    State(state): State<AppState>,
    source: LoginSource,
    headers: HeaderMap,
) -> axum::response::Response {
    if !metrics_authorized(state.metrics_token(), &source, &headers) {
        return (StatusCode::FORBIDDEN, "metrics_forbidden").into_response();
    }
    let ready = database_ready(&state).await;
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics().render(ready),
    )
        .into_response()
}

/// A short, bounded probe. The timeout matters: a pool whose connections are
/// wedged must not turn a readiness scrape into a second outage.
pub async fn database_ready(state: &AppState) -> bool {
    matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            archava_adapters::writer::is_current(state.pool()),
        )
        .await,
        Ok(Ok(()))
    )
}
