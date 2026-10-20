//! The R07 billing cohort's host witnesses — driven through the real router
//! (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL whose
//! `archava_billing` schema is the real migration ledger
//! (`001_billing_invoices_settlement`). Every assertion is a fact about what
//! the SERVER did:
//!
//! - the authorized settlement life (create → issue → payment attempt →
//!   SIGNED full succeeded event) ends with exactly the rows the
//!   repository's SQL spec freezes — the wide receipt document's cells
//!   (`commandId/action/ids/status/outstandingMinor/revision/
//!   idempotencyReplay/executedAt`, optional ids present only where the
//!   arm names them), the invoice's status/revision chain, the priced
//!   line sheet, the attempt row, the settlement event row, and the audit
//!   naming every operation once; the durable status read answers the TS
//!   commandStatus document verbatim (`recorded` with the receipt, or the
//!   bare `unknown`);
//! - a FORGED claim settles nothing: the signature check runs before any
//!   SQL (the TS service layer's labor order), so a tampered amount, a
//!   claim minted under another provider's derived key, and a digest
//!   minted under another domain all refuse the 403 carrier with no
//!   event row, no extra command row and no audit row;
//! - the correction chain is append-only and respects its OWN invoice: a
//!   zero correction refuses at the parse, a missing reversal target is
//!   404, a cross-invoice target is 409;
//! - voiding holds on recorded settlement, a settled invoice reopens only
//!   through a refund, void is terminal;
//! - settlement beyond the balance refuses and holds nothing;
//! - a FAILED outcome is history — no money, no revision, but its own
//!   event row and `failed` receipt;
//! - the attempt arm asserts no result — its receipt stays `attempted`,
//!   its revision the invoice's own — and a voided invoice takes none;
//! - a revoked member's submissions refuse even the byte-identical
//!   replay, while the restored grant replays the stored receipt and
//!   carries new work;
//! - a cross-tenant caller is refused before any statement runs and still
//!   acts freely in its own ledger;
//! - the same `commandId` with a DIFFERENT payload conflicts without
//!   recording; the same commandId + bytes replays with
//!   `idempotencyReplay: true` and still one row everywhere;
//! - narrow grants refuse exactly the arms whose gate they lack while
//!   their status reads walk (the status read grants nothing beyond
//!   enrollment); and
//! - the transition gates (issue-on-issued, correct-a-draft, settle-a-
//!   draft) and the wire boundary (unknown member, lowercase currency,
//!   uppercase signature) refuse at the right layer.
//!
//! Zone-free by construction: the billing grammar has no calendar, so the
//! suite's only run-relative facts are the SQL-stamped instants the clock
//! binds — read back as bytes and counted, not predicted.
//!
//! ## The derived provider secret (the gate's recipe, mirrored)
//!
//! The gate derives `key = utf8(identity().digest("billing-provider-secret",
//! tenant ‖ "\0" ‖ provider))` where `digest` is `hex(HMAC-SHA256(identity
//! key, label ‖ "\0" ‖ value))` — so the suite's minting helper replicates
//! exactly that, using `hmac`+`sha2` on the SAME fixed identity key
//! `AppState` was built with, and frames claims exactly as
//! `packages/billing/src/signature.ts` does: the domain string, a
//! big-endian u32 length header AFTER it, then each of the seven parts
//! with its own header after it.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table
/// list every ERP suite pins; the billing surface RIDES the same serving
/// state without touching commerce rows.
const TABLES: &[&str] = &[
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "movements",
    "returns",
    "commands",
    "audit",
    "approvals",
];

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts.
/// THE SAME SLICE `AppState` is built with, so the suite's derived
/// provider keys match the gate's derivation byte for byte.
const IDENTITY_KEY: &[u8] = &[23u8; 32];

/// The test password. The password gate enforces `length >= 15`.
const PASSWORD: &str = "correct horse battery staple";

/// A precomputed scrypt hash of [`PASSWORD`] — one hash serves every user.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Serving state that has passed BOTH readiness gates: commerce (`try_new`)
/// and the purchasing ledger (`try_new_erp`'s second pass) — the billing
/// surface rides the same serving state; its own ledger rows exist in the
/// fixture regardless, and the suite's FIRST step checks the billing
/// ledger's own provision facts (`assert_billing_kernel`) before any
/// route is mounted.
///
/// (The billing ledger's own readiness is the SUITE's first witness below —
/// the kernel checks ride the adapter's `assert_billing_kernel`, called on
/// the fixture's pools before any route answers.)
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_erp(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.purchasing_ledger,
    )
    .await
    .expect("a ready fixture must construct serving state")
}

// --- The cast -----------------------------------------------------------------

/// The tenants this suite uses.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

/// The billing permission vocabulary (migration 001's CHECK).
const BILLING_ALL: &[&str] = &[
    "billing.read",
    "billing.write",
    "billing.settle",
    "audit.read",
];

/// `bx` is the full-grant manager with scope all — the positive-path actor.
/// `bxg` holds read + audit only: the six write arms refuse, while its
/// status reads walk (the status read grants nothing beyond enrollment).
/// `bxr` is `bx` with the membership seeded ACTIVE and revoked by the test
/// itself — the recovery must not outlive the grant.
/// `bxn` holds only `audit.read` — every grant gate refuses.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal
/// that still acts freely in its own ledger.
async fn seed(fixture: &Fixture) {
    for principal in ["bx", "bxg", "bxr", "bxn", "other"] {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(principal)
        .bind(format!("{}@archava.test", principal))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the identity user");
    }

    // The billing ledger's rows: the one-difference-per-actor pattern.
    for (principal, permissions) in [
        ("bx", BILLING_ALL),
        ("bxg", &["billing.read", "audit.read"][..]),
        ("bxr", BILLING_ALL),
        ("bxn", &["audit.read"][..]),
    ] {
        sqlx::query(
            "INSERT INTO archava_billing.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME billing membership");
    }
    sqlx::query(
        "INSERT INTO archava_billing.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(BILLING_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX billing membership");
}

// --- The merchant ids (witness-local command keys; hex only) -------------------

/// W1 — the authorized life.
const KEY_CREATE: &str = "6b240100-9999-4999-9999-0000000000a1";
const KEY_ISSUE: &str = "6b240100-9999-4999-9999-0000000000a2";
const KEY_ATTEMPT: &str = "6b240100-9999-4999-9999-0000000000a3";
const KEY_SETTLE: &str = "6b240100-9999-4999-9999-0000000000a4";

/// W2 — the forged family (needs its own issue key).
const KEY_FORGED_CREATE: &str = "6b240100-9999-4999-9999-0000000000b1";
const KEY_FORGED_ISSUE: &str = "6b240100-9999-4999-9999-0000000000b2";
const KEY_TAMPERED_EVENT: &str = "6b240100-9999-4999-9999-0000000000b3";
const KEY_SWAP_EVENT: &str = "6b240100-9999-4999-9999-0000000000b4";
const KEY_DOMAIN_EVENT: &str = "6b240100-9999-4999-9999-0000000000b5";

/// W3 — the correction chain.
const KEY_CORR_CREATE: &str = "6b240100-9999-4999-9999-0000000000c1";
const KEY_CORR_ISSUE: &str = "6b240100-9999-4999-9999-0000000000c2";
const KEY_ZERO_CORRECTION: &str = "6b240100-9999-4999-9999-0000000000c3";
const KEY_CORRECTION: &str = "6b240100-9999-4999-9999-0000000000c4";
const KEY_MISSING_REVERSAL: &str = "6b240100-9999-4999-9999-0000000000c5";
const KEY_CROSS_CREATE: &str = "6b240100-9999-4999-9999-0000000000c6";
const KEY_CROSS_ISSUE: &str = "6b240100-9999-4999-9999-0000000000c7";
const KEY_CROSS_REVERSAL: &str = "6b240100-9999-4999-9999-0000000000c8";

/// W4 — voiding holds on recorded settlement.
const KEY_VOID_CREATE: &str = "6b240100-9999-4999-9999-0000000000d1";
const KEY_VOID_ISSUE: &str = "6b240100-9999-4999-9999-0000000000d2";
const KEY_VOID_PARTIAL: &str = "6b240100-9999-4999-9999-0000000000d3";
const KEY_VOID_TRY: &str = "6b240100-9999-4999-9999-0000000000d4";
const KEY_VOID_SETTLE: &str = "6b240100-9999-4999-9999-0000000000d5";
const KEY_VOID_SETTLED: &str = "6b240100-9999-4999-9999-0000000000d6";
const KEY_REFUND: &str = "6b240100-9999-4999-9999-0000000000d7";
const KEY_VOID_OK: &str = "6b240100-9999-4999-9999-0000000000d8";
const KEY_VOID_TWICE: &str = "6b240100-9999-4999-9999-0000000000d9";

/// W5 — over-settlement holds nothing.
const KEY_OVER_CREATE: &str = "6b240100-9999-4999-9999-0000000000e1";
const KEY_OVER_ISSUE: &str = "6b240100-9999-4999-9999-0000000000e2";
const KEY_OVER_EVENT: &str = "6b240100-9999-4999-9999-0000000000e3";

/// W6 — the failed claim.
const KEY_FAIL_CREATE: &str = "6b240100-9999-4999-9999-0000000000f1";
const KEY_FAIL_ISSUE: &str = "6b240100-9999-4999-9999-0000000000f2";
const KEY_FAIL_EVENT: &str = "6b240100-9999-4999-9999-0000000000f3";

/// W7 — the attempts.
const KEY_ATTEMPT_CREATE: &str = "6b240100-9999-4999-9999-000000000271";
const KEY_ATTEMPT_ISSUE: &str = "6b240100-9999-4999-9999-000000000272";
const KEY_ATTEMPT_ONE: &str = "6b240100-9999-4999-9999-000000000273";
const KEY_ATTEMPT_TWO: &str = "6b240100-9999-4999-9999-000000000274";
const KEY_ATTEMPT_VOID: &str = "6b240100-9999-4999-9999-000000000275";
const KEY_ATTEMPT_THIRD: &str = "6b240100-9999-4999-9999-000000000276";

/// W8 — the revoked grant.
const KEY_REVOKED: &str = "6b240100-9999-4999-9999-0000000000cd";
const KEY_REVOKED_RETRY: &str = "6b240100-9999-4999-9999-0000000000ce";
const KEY_REVOKED_CREATE: &str = "6b240100-9999-4999-9999-0000000000cf";

/// W9 — the cross-tenant refusal.
const KEY_CROSS_READ: &str = "6b240100-9999-4999-9999-0000000002a1";
const KEY_GLOBEX_CREATE: &str = "6b240100-9999-4999-9999-0000000002a2";

/// W10 — the same key with a different payload.
const KEY_REUSE_CREATE: &str = "6b240100-9999-4999-9999-0000000002b1";

/// W11 — the replay (two keys: the survivor names its own refusal).
const KEY_REUSE_HOT: &str = "6b240100-9999-4999-9999-0000000002c1";

/// W12 — the narrow grants.
const KEY_GATE_CREATE: &str = "6b240100-9999-4999-9999-0000000002d1";
const KEY_GATE_ISSUE: &str = "6b240100-9999-4999-9999-0000000002d2";
const KEY_GATE_GHOST: &str = "6b240100-9999-4999-9999-0000000002d3";
const KEY_GATE_CORRECTION: &str = "6b240100-9999-4999-9999-0000000002d4";
const KEY_GATE_VOID: &str = "6b240100-9999-4999-9999-0000000002d5";
const KEY_GATE_ATTEMPT: &str = "6b240100-9999-4999-9999-0000000002d6";
const KEY_GATE_EVENT: &str = "6b240100-9999-4999-9999-0000000002d7";

/// W13 — the transition gates + the wire boundary.
const KEY_VOCAB_CREATE: &str = "6b240100-9999-4999-9999-0000000002e1";
const KEY_VOCAB_ISSUE: &str = "6b240100-9999-4999-9999-0000000002e2";
const KEY_VOCAB_ISSUE2: &str = "6b240100-9999-4999-9999-0000000002e3";
const KEY_VOCAB_CREATE2: &str = "6b240100-9999-4999-9999-0000000002e4";
const KEY_VOCAB_CORRECT: &str = "6b240100-9999-4999-9999-0000000002e5";
const KEY_VOCAB_EVENT: &str = "6b240100-9999-4999-9999-0000000002e6";
const KEY_VOCAB_BADPARSE: &str = "6b240100-9999-4999-9999-0000000002e7";

/// An unrecorded command id for the `unknown` status read.
const KEY_UNKNOWN: &str = "6b240100-9999-4999-9999-000000000000";

/// A VALID uuid that no correction ever wears — the missing reversal
/// target.
const DEAD_CORRECTION: &str = "6b240100-9999-4999-9999-00000000dead";

/// The provider identities this suite settles through — one per invoice
/// family so forged-under-wrong-key is a REAL wrong key, not a wording.
const STRIPE: &str = "stripe-test";
const ADYEN: &str = "adyen-test";

// --- The signature mint (signature.ts, mirrored) --------------------------------

/// HMAC-SHA256 → lowercase hex, the identity-store digest's render.
fn digest_hex(key: &[u8], message: &[u8]) -> String {
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    let bytes = mac.finalize().into_bytes();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes.iter() {
        out.push(char::from_digit((*byte >> 4) as u32, 16).expect("a nibble"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("a nibble"));
    }
    out
}

/// `Identity::digest(label, value)` — `HMAC-SHA256(key, label ‖ "\0" ‖
/// value)`, hex. The gate derives provider keys through exactly this, so
/// the suite's derivation mirrors it byte for byte.
fn identity_digest(label: &str, value: &str) -> String {
    let mut message = Vec::with_capacity(label.len() + 1 + value.len());
    message.extend_from_slice(label.as_bytes());
    message.push(0);
    message.extend_from_slice(value.as_bytes());
    digest_hex(IDENTITY_KEY, &message)
}

/// The derived provider secret — the gate's recipe: the labelled digest
/// rendered as its ASCII bytes (64 characters, so ≥32 always holds).
fn provider_key(tenant: &str, provider_id: &str) -> Vec<u8> {
    identity_digest(
        "billing-provider-secret",
        &format!("{tenant}\0{provider_id}"),
    )
    .into_bytes()
}

/// The framed provider-event digest, `signature.ts` ported: the domain
/// string then a big-endian u32 length header AFTER it, then each of the
/// seven parts with its own header after it (part-then-length, so a byte
/// cannot move between fields without invalidating a claim).
fn framed_digest(key: &[u8], domain: &str, parts: [&str; 7]) -> String {
    let mut framed: Vec<u8> = Vec::new();
    framed.extend_from_slice(domain.as_bytes());
    framed.extend_from_slice(&(domain.len() as u32).to_be_bytes());
    for part in parts {
        framed.extend_from_slice(part.as_bytes());
        framed.extend_from_slice(&(part.len() as u32).to_be_bytes());
    }
    digest_hex(key, &framed)
}

/// A VALID claim — the seven parts in the frozen order, signed under the
/// tenant+provider pair's derived key.
fn signed_claim(
    tenant: &str,
    invoice_id: &str,
    provider_id: &str,
    provider_event_id: &str,
    outcome: &str,
    amount_minor: i64,
    currency: &str,
) -> String {
    framed_digest(
        &provider_key(tenant, provider_id),
        "archava/billing/provider-event/v1",
        [
            tenant,
            invoice_id,
            provider_id,
            provider_event_id,
            outcome,
            &amount_minor.to_string(),
            currency,
        ],
    )
}

/// A claim minted under ANOTHER DOMAIN — same key, same framing, a
/// different label: exactly the "meaningful elsewhere" value no ledger
/// accepts for settlement here.
fn claim_under_other_domain(
    tenant: &str,
    invoice_id: &str,
    provider_id: &str,
    provider_event_id: &str,
    outcome: &str,
    amount_minor: i64,
    currency: &str,
) -> String {
    framed_digest(
        &provider_key(tenant, provider_id),
        "archava/billing/act-confirmation/v1",
        [
            tenant,
            invoice_id,
            provider_id,
            provider_event_id,
            outcome,
            &amount_minor.to_string(),
            currency,
        ],
    )
}

/// A claim minted under ANOTHER PROVIDER's derived key — the pairing
/// property: a tenant+provider mix cannot settle under its sibling's
/// secret.
fn signed_claim_under_sibling(
    tenant: &str,
    invoice_id: &str,
    provider_event_id: &str,
    outcome: &str,
    amount_minor: i64,
    currency: &str,
) -> String {
    framed_digest(
        &provider_key(tenant, ADYEN),
        "archava/billing/provider-event/v1",
        [
            tenant,
            invoice_id,
            STRIPE,
            provider_event_id,
            outcome,
            &amount_minor.to_string(),
            currency,
        ],
    )
}

// --- HTTP helpers -----------------------------------------------------------

/// A session's credentials.
struct Session {
    token: String,
    csrf: String,
}

/// How many logins this suite may have in flight at once — the production
/// budget (never raised from inside the suite).
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

/// Log in and return the session's bearer token and CSRF value.
async fn login(app: &axum::Router, principal_id: &str) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = json!({
        "username": format!("{}@archava.test", principal_id),
        "password": PASSWORD,
    })
    .to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "login as {principal_id} must succeed"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome: Value = serde_json::from_slice(&bytes).expect("the login response is JSON");

    Session {
        token: outcome["token"]
            .as_str()
            .expect("the login response carries a token")
            .to_string(),
        csrf: outcome["csrf"]
            .as_str()
            .expect("the login response carries a CSRF value")
            .to_string(),
    }
}

/// Send a POST with a session's credentials, always carrying the CSRF
/// header — every mutation writer in this suite is authenticated and
/// CSRF-bound, like the production router.
async fn post(app: &axum::Router, uri: &str, body: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", session.token))
                .header("x-archava-csrf", &session.csrf)
                .body(Body::from(body.to_string()))
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "the response is JSON: {error}; status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, outcome)
}

/// Send a GET with a session's bearer token (no CSRF — a read).
async fn get(app: &axum::Router, uri: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

// --- The priced sheet every invoice family wears --------------------------------

const IDR: &str = "IDR";
const NIGHT: &str = "Deluxe night";
const NIGHT_UNIT: i64 = 750_000;
const NIGHT_QUANTITY: i64 = 2;
const NIGHT_GROSS: i64 = NIGHT_QUANTITY * NIGHT_UNIT; // 1_500_000

// --- Submission bodies ---------------------------------------------------------

/// The mutation envelope: tenant from the caller's own document, the flat
/// billing arm verbatim (the TS wire's shape; the tenant rides the body
/// envelope instead of the path — the carriers' recorded divergence).
fn envelope(tenant: &str, mutation: Value) -> String {
    json!({"tenant": tenant, "mutation": mutation}).to_string()
}

fn create_body(tenant: &str, command_id: &str, invoice_code: &str, customer_ref: &str) -> String {
    envelope(
        tenant,
        json!({
            "type": "create_invoice",
            "commandId": command_id,
            "invoiceCode": invoice_code,
            "customerRef": customer_ref,
            "currency": IDR,
            "consumerRef": "hotel-owner-1",
            "lines": [
                {"description": NIGHT, "quantity": NIGHT_QUANTITY, "unitMinor": NIGHT_UNIT}
            ],
        }),
    )
}

fn issue_body(tenant: &str, command_id: &str, invoice_id: &str, expected_revision: i64) -> String {
    envelope(
        tenant,
        json!({
            "type": "issue_invoice",
            "commandId": command_id,
            "invoiceId": invoice_id,
            "expectedRevision": expected_revision,
            "reason": "Approved for settlement",
        }),
    )
}

fn correction_body(
    tenant: &str,
    command_id: &str,
    invoice_id: &str,
    amount_minor: i64,
    reverses_id: Option<&str>,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "append_correction",
            "commandId": command_id,
            "invoiceId": invoice_id,
            "amountMinor": amount_minor,
            "reason": "Night slip after the price freeze",
            "reversesId": reverses_id,
        }),
    )
}

fn void_body(tenant: &str, command_id: &str, invoice_id: &str, expected_revision: i64) -> String {
    envelope(
        tenant,
        json!({
            "type": "void_invoice",
            "commandId": command_id,
            "invoiceId": invoice_id,
            "expectedRevision": expected_revision,
            "reason": "Concession closed the folio",
        }),
    )
}

fn attempt_body(
    tenant: &str,
    command_id: &str,
    invoice_id: &str,
    amount_minor: i64,
    provider_ref: Option<&str>,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "record_payment_attempt",
            "commandId": command_id,
            "invoiceId": invoice_id,
            "amountMinor": amount_minor,
            "providerRef": provider_ref,
        }),
    )
}

#[allow(clippy::too_many_arguments)] // the arm's own members, in the wire's order
fn event_body(
    tenant: &str,
    command_id: &str,
    invoice_id: &str,
    provider_id: &str,
    provider_event_id: &str,
    outcome: &str,
    amount_minor: i64,
    signature: String,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "record_provider_event",
            "commandId": command_id,
            "invoiceId": invoice_id,
            "providerId": provider_id,
            "providerEventId": provider_event_id,
            "outcome": outcome,
            "amountMinor": amount_minor,
            "currency": IDR,
            "signature": signature,
        }),
    )
}

// --- Row readers ---------------------------------------------------------------

/// The invoice row's cells the witnesses check — status, the four money
/// cells, the revision and the issued stamp.
async fn invoice_row(
    fixture: &Fixture,
    tenant: &str,
    id: &str,
) -> (String, i64, i64, i64, i64, i64, Option<String>) {
    let row: (String, i64, i64, i64, i64, i32, Option<String>) = sqlx::query_as(
        "SELECT status,gross_minor,correction_minor,settled_minor,outstanding_minor,revision,\
         to_char(issued_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
         FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the invoice row answers");
    (row.0, row.1, row.2, row.3, row.4, i64::from(row.5), row.6)
}

/// How many rows a tenant's billing table holds.
async fn count_of(fixture: &Fixture, table: &str, tenant: &str) -> i64 {
    let statement =
        format!("SELECT count(*)::bigint FROM archava_billing.{table} WHERE tenant_id=$1");
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("the count answers");
    count
}

/// How many command rows a tenant has left for an exact id.
async fn command_count(fixture: &Fixture, tenant: &str, command_id: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM archava_billing.commands \
         WHERE tenant_id=$1 AND request_id=$2::uuid",
    )
    .bind(tenant)
    .bind(command_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command count answers");
    count
}

/// The audit's operation set, in occurred order.
async fn audit_operations(fixture: &Fixture, tenant: &str) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT operation FROM archava_billing.audit WHERE tenant_id=$1 \
         ORDER BY occurred_at,request_id",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the audit rows answer")
    .into_iter()
    .map(|(operation,)| operation)
    .collect()
}

/// The provider event rows a tenant's ledger names —
/// `(provider_event_id, outcome, amount_minor)`.
async fn event_rows(fixture: &Fixture, tenant: &str) -> Vec<(String, String, i64)> {
    sqlx::query_as(
        "SELECT provider_event_id,outcome,amount_minor FROM archava_billing.payment_events \
         WHERE tenant_id=$1",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the event rows answer")
}

/// The priced line rows a tenant's ledger names — `(description, quantity,
/// unit_minor, line_total_minor)`.
async fn line_rows(fixture: &Fixture, tenant: &str) -> Vec<(String, i32, i64, i64)> {
    sqlx::query_as(
        "SELECT description,quantity,unit_minor,line_total_minor FROM archava_billing.invoice_lines \
         WHERE tenant_id=$1",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the line rows answer")
}

/// The attempts a tenant's ledger names — `(amount_minor, currency,
/// provider_ref)`.
async fn attempt_rows(fixture: &Fixture, tenant: &str) -> Vec<(i64, String, Option<String>)> {
    sqlx::query_as(
        "SELECT amount_minor,currency,provider_ref FROM archava_billing.payment_attempts \
         WHERE tenant_id=$1",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the attempt rows answer")
}

/// The durable status read — the suite's channel to the commandStatus
/// document.
async fn status_of(app: &axum::Router, session: &Session, tenant: &str, command_id: &str) -> Value {
    let (_, doc) = get(
        app,
        &format!("/erp/billing/commands/status?tenant={tenant}&requestId={command_id}"),
        session,
    )
    .await;
    doc
}

/// Provision + seed + serve (one fixture per test; the billing ledger is
/// the suite's own database, the kernel assert rides the running fixture
/// before any route is mounted, and every actor logs in once).
async fn served() -> (
    Fixture,
    axum::Router,
    Session,
    Session,
    Session,
    Session,
    Session,
) {
    let fixture = provision().await;
    seed(&fixture).await;
    // The provision checks (role, the ledger, the forced-RLS width) pass
    // on the fixture's runtime role — the serving state's support check,
    // asserted here because the suite holds the receipt of it.
    archava_adapters::erp::billing::assert_billing_kernel(
        &fixture.runtime_pool,
        &fixture.billing_ledger,
    )
    .await
    .expect("the billing kernel asserts on the fixture");

    let app = archava_host::router(state(&fixture).await);
    let bx = login(&app, "bx").await;
    let bxg = login(&app, "bxg").await;
    let bxn = login(&app, "bxn").await;
    let bxr = login(&app, "bxr").await;
    let other = login(&app, "other").await;
    (fixture, app, bx, bxg, bxn, bxr, other)
}

/// Submit an arm and assert it committed — the OK receipt's channel.
async fn receipt_of(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, receipt) = post(app, "/erp/billing/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the arm must commit: {receipt} — body {body}"
    );
    receipt
}

/// Submit an arm and assert the FORBIDDEN carrier — the forged family's
/// channel (the TS `forbidden`/`invalid_signature` fold).
async fn refused_forbidden(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, answer) = post(app, "/erp/billing/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the arm must refuse: {answer}"
    );
    assert_eq!(answer["decision"].as_str(), Some("denied"), "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"), "{answer}");
    answer
}

/// Submit an arm and assert the CONFLICT carrier — the settlement
/// conflicts' channel (stale revision, `invalid_transition`,
/// `unbalanced_settlement`, fingerprint-mismatched replay).
async fn refused_conflict(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, answer) = post(app, "/erp/billing/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the arm must conflict: {answer}"
    );
    assert_eq!(answer["decision"].as_str(), Some("refused"), "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("conflict"), "{answer}");
    answer
}

/// Submit an arm and assert the NOT-FOUND carrier — the missing record.
async fn refused_unknown(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, answer) = post(app, "/erp/billing/commands", body, session).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the arm must 404: {answer}");
    assert_eq!(answer["decision"].as_str(), Some("denied"), "{answer}");
    assert_eq!(
        answer["reason"].as_str(),
        Some("unknown_resource"),
        "{answer}"
    );
    answer
}

/// Submit an arm and assert the boundary parse's 400 — the malformed
/// document's channel.
async fn refused_parse(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, answer) = post(app, "/erp/billing/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the parse must 400: {answer}"
    );
    assert_eq!(answer["decision"].as_str(), Some("rejected"), "{answer}");
    assert_eq!(
        answer["reason"].as_str(),
        Some("invalid_submission"),
        "{answer}"
    );
    answer
}

// --- W1: the authorized settlement life -------------------------------------------

/// The full settlement life walks the ledger: the priced invoice starts
/// draft, issues, records an attempt (a receipt with no outcome), and
/// settles ONLY through a claim the derived key signed — the payment
/// event row rides its own idempotent identity, the balance arithmetic
/// rides the frozen fact, and the audit names every operation once; the
/// status read answers the recorded receipt and the bare unknown.
#[tokio::test]
async fn authorized_billing_life_documents_its_settlement() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    // 1. create_invoice — the wide receipt with ONLY its arm's ids.
    let baseline = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_CREATE, "INV-001", "customer-1"),
    )
    .await;
    assert_eq!(baseline["commandId"].as_str(), Some(KEY_CREATE));
    assert_eq!(baseline["tenantId"].as_str(), Some(ACME));
    assert_eq!(baseline["action"].as_str(), Some("create_invoice"));
    assert_eq!(baseline["status"].as_str(), Some("draft"));
    assert_eq!(baseline["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    assert_eq!(baseline["revision"].as_i64(), Some(1));
    assert_eq!(baseline["idempotencyReplay"].as_bool(), Some(false));
    assert!(baseline["executedAt"].as_str().is_some());
    assert!(baseline["invoiceId"].as_str().is_some());
    assert!(
        baseline["lineId"].as_str().is_some(),
        "the create names TWO rows"
    );
    assert!(
        baseline["attemptId"].is_null(),
        "the create arm names no attempt"
    );
    assert!(
        baseline["eventId"].is_null(),
        "the create arm names no event"
    );
    let invoice_id = baseline["invoiceId"]
        .as_str()
        .expect("a fresh invoice uuid")
        .to_string();
    assert_eq!(
        line_rows(&fixture, ACME).await,
        vec![(
            NIGHT.to_string(),
            NIGHT_QUANTITY as i32,
            NIGHT_UNIT,
            NIGHT_GROSS
        )],
        "the priced line sheet is one arithmetically closed row"
    );

    // 2. issue_invoice — the optimistic revision gate rides the settle
    //    grant.
    let issued = receipt_of(&app, &bx, &issue_body(ACME, KEY_ISSUE, &invoice_id, 1)).await;
    assert_eq!(issued["status"].as_str(), Some("issued"));
    assert_eq!(issued["revision"].as_i64(), Some(2));
    assert_eq!(issued["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    assert!(issued["lineId"].is_null() && issued["eventId"].is_null());

    // 3. record_payment_attempt — the intent arm: no result, no revision.
    let attempted = receipt_of(
        &app,
        &bx,
        &attempt_body(
            ACME,
            KEY_ATTEMPT,
            &invoice_id,
            500_000,
            Some("attempt-batch-9"),
        ),
    )
    .await;
    assert_eq!(attempted["status"].as_str(), Some("attempted"));
    assert_eq!(
        attempted["revision"].as_i64(),
        Some(2),
        "an attempt moves no revision"
    );
    assert_eq!(attempted["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    assert!(attempted["attemptId"].as_str().is_some());
    assert!(attempted["eventId"].is_null());
    assert_eq!(
        attempt_rows(&fixture, ACME).await,
        vec![(
            500_000,
            IDR.to_string(),
            Some("attempt-batch-9".to_string())
        )],
        "the attempt row rides the invoice's own currency; the ref present"
    );

    // 4. record_provider_event — THE only arm moving money, signed under
    //    the derived key. Full settlement: outstanding hits zero.
    let signature = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_settle_full",
        "succeeded",
        NIGHT_GROSS,
        IDR,
    );
    let settled = receipt_of(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_SETTLE,
            &invoice_id,
            STRIPE,
            "evt_settle_full",
            "succeeded",
            NIGHT_GROSS,
            signature,
        ),
    )
    .await;
    assert_eq!(settled["status"].as_str(), Some("settled"));
    assert_eq!(settled["outstandingMinor"].as_i64(), Some(0));
    assert_eq!(settled["revision"].as_i64(), Some(3));
    assert!(settled["eventId"].as_str().is_some());
    assert!(settled["lineId"].is_null() && settled["attemptId"].is_null());
    assert_eq!(
        event_rows(&fixture, ACME).await,
        vec![(
            "evt_settle_full".to_string(),
            "succeeded".to_string(),
            NIGHT_GROSS
        )],
        "the settlement event row names its provider identity and outcome"
    );
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.0, "settled");
    assert_eq!(
        invoice.3, NIGHT_GROSS,
        "the settled amount equals the claim"
    );
    assert_eq!(invoice.4, 0);
    assert_eq!(invoice.2, 0, "no correction touched this folio");
    assert!(
        invoice.6.is_some(),
        "an issued invoice binds its issue stamp"
    );

    // 5. The audit chain names every operation once.
    assert_eq!(
        audit_operations(&fixture, ACME).await,
        vec![
            "create_invoice".to_string(),
            "issue_invoice".to_string(),
            "record_payment_attempt".to_string(),
            "record_provider_event".to_string(),
        ],
        "one audit row per arm, in the life's order"
    );

    // 6. The durable status read — the recorded receipt, then the bare
    //    unknown for a command id nobody committed.
    let recorded = status_of(&app, &bx, ACME, KEY_SETTLE).await;
    assert_eq!(recorded["status"].as_str(), Some("recorded"));
    assert_eq!(
        recorded["receipt"]["action"].as_str(),
        Some("record_provider_event")
    );
    assert_eq!(recorded["receipt"]["status"].as_str(), Some("settled"));
    let unknown = status_of(&app, &bx, ACME, KEY_UNKNOWN).await;
    assert_eq!(
        unknown,
        json!({"status": "unknown"}),
        "an unrecorded command id answers the bare unknown document"
    );

    // 7. The count integrity: one row family, four command rows — the
    //    status read records none.
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);
    assert_eq!(count_of(&fixture, "invoice_lines", ACME).await, 1);
    assert_eq!(count_of(&fixture, "payment_attempts", ACME).await, 1);
    assert_eq!(count_of(&fixture, "payment_events", ACME).await, 1);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 4);
}

// --- W2: the forged claim settles nothing ------------------------------------------

/// The signature check runs BEFORE any SQL: a tampered amount, a claim
/// signed under another provider's derived key and a digest minted under
/// another domain all refuse the 403 carrier, and NOTHING is left — no
/// event row, no extra command row, no balance movement, no audit row.
#[tokio::test]
async fn a_forged_claim_settles_nothing() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    // The ledger's precondition: one issued invoice (create + issue = two
    // command rows, two audit rows). Everything after rides those facts.
    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_FORGED_CREATE, "INV-020", "customer-2"),
    )
    .await;
    let invoice_id = created["invoiceId"]
        .as_str()
        .expect("invoice uuid")
        .to_string();
    receipt_of(
        &app,
        &bx,
        &issue_body(ACME, KEY_FORGED_ISSUE, &invoice_id, 1),
    )
    .await;

    // 1. A TAMPERED amount — a validly shaped claim whose signature
    //    covered a DIFFERENT number than the arm carries.
    let honest = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_tamper",
        "succeeded",
        1_000_000,
        IDR,
    );
    refused_forbidden(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_TAMPERED_EVENT,
            &invoice_id,
            STRIPE,
            "evt_tamper",
            "succeeded",
            1_400_000,
            honest,
        ),
    )
    .await;

    // 2. Another provider's key — the pairing property holds: a claim
    //    minted under Adyen's secret cannot settle the Stripe folio.
    let sibling = signed_claim_under_sibling(
        ACME,
        &invoice_id,
        "evt_sibling",
        "succeeded",
        1_000_000,
        IDR,
    );
    refused_forbidden(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_SWAP_EVENT,
            &invoice_id,
            STRIPE,
            "evt_sibling",
            "succeeded",
            1_000_000,
            sibling,
        ),
    )
    .await;

    // 3. Another domain's digest — a value meaningful elsewhere settles
    //    nothing here.
    let foreign = claim_under_other_domain(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_foreigner",
        "succeeded",
        1_000_000,
        IDR,
    );
    refused_forbidden(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_DOMAIN_EVENT,
            &invoice_id,
            STRIPE,
            "evt_foreigner",
            "succeeded",
            1_000_000,
            foreign,
        ),
    )
    .await;

    // 4. The ledger holds NOTHING from the forged family: no event row,
    //    no extra command row, no audit; the balance and revision ride
    //    the issue's own facts.
    assert_eq!(
        event_rows(&fixture, ACME).await,
        Vec::new(),
        "no forged claim landed"
    );
    assert_eq!(count_of(&fixture, "commands", ACME).await, 2);
    assert_eq!(
        audit_operations(&fixture, ACME).await.len(),
        2,
        "the forged family audited NOTHING"
    );
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.0, "issued");
    assert_eq!(invoice.3, 0);
    assert_eq!(invoice.5, 2);
}

// --- W3: the correction chain -------------------------------------------------------

/// The correction chain is append-only and respects its OWN invoice: a
/// zero correction refuses at the parse boundary, a real correction
/// rebalances through the frozen arithmetic, a missing reversal target is
/// 404, a cross-invoice reversal target is 409.
#[tokio::test]
async fn correction_reversal_respects_its_invoice() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_CORR_CREATE, "INV-030", "customer-3"),
    )
    .await;
    let invoice_id = created["invoiceId"]
        .as_str()
        .expect("invoice uuid")
        .to_string();
    receipt_of(&app, &bx, &issue_body(ACME, KEY_CORR_ISSUE, &invoice_id, 1)).await;

    // 1. The zero correction refuses at the parse: 400 before anything.
    refused_parse(
        &app,
        &bx,
        &correction_body(ACME, KEY_ZERO_CORRECTION, &invoice_id, 0, None),
    )
    .await;
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 0);

    // 2. A real correction −50_000: outstanding follows the arithmetic and
    //    the revision moves one step.
    let corrected = receipt_of(
        &app,
        &bx,
        &correction_body(ACME, KEY_CORRECTION, &invoice_id, -50_000, None),
    )
    .await;
    assert_eq!(corrected["status"].as_str(), Some("issued"));
    assert_eq!(
        corrected["outstandingMinor"].as_i64(),
        Some(NIGHT_GROSS - 50_000)
    );
    assert_eq!(
        corrected["revision"].as_i64(),
        Some(3),
        "issue→2, the correction→3"
    );
    assert!(corrected["correctionId"].as_str().is_some());

    // 3. A reversal that names a MISSING correction refuses 404 with
    //    nothing recorded.
    refused_unknown(
        &app,
        &bx,
        &correction_body(
            ACME,
            KEY_MISSING_REVERSAL,
            &invoice_id,
            50_000,
            Some(DEAD_CORRECTION),
        ),
    )
    .await;
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 1);

    // 4. A reversal that names a correction on ANOTHER invoice refuses
    //    409 — the chain is per-invoice.
    let other_created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_CROSS_CREATE, "INV-031", "customer-3b"),
    )
    .await;
    let other_invoice = other_created["invoiceId"]
        .as_str()
        .expect("uuid")
        .to_string();
    receipt_of(
        &app,
        &bx,
        &issue_body(ACME, KEY_CROSS_ISSUE, &other_invoice, 1),
    )
    .await;
    refused_conflict(
        &app,
        &bx,
        &correction_body(
            ACME,
            KEY_CROSS_REVERSAL,
            &other_invoice,
            50_000,
            Some(corrected["correctionId"].as_str().expect("correction uuid")),
        ),
    )
    .await;
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 1);

    // 5. The held facts: the correction cell rides the sum, the balance
    //    rides the arithmetic, the audit names ONE correction.
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.2, -50_000, "the correction cell rides the sum");
    assert_eq!(invoice.4, NIGHT_GROSS - 50_000);
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 1);
}

// --- W4: voiding holds on recorded settlement -------------------------------------------

/// Voiding holds on recorded settlement: a partially settled invoice
/// refuses (the `unbalanced_settlement` verdict), a fully settled one
/// refuses until a refund empties it — the refund REOPENS `issued` — then
/// the void walks, and void is terminal.
#[tokio::test]
async fn voiding_holds_on_recorded_settlement() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_VOID_CREATE, "INV-040", "customer-4"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(&app, &bx, &issue_body(ACME, KEY_VOID_ISSUE, &invoice_id, 1)).await;

    // 1. A PARTIAL settle (700_000 of 1_500_000): the invoice stays
    //    issued — and the void now refuses (recorded settlement).
    let partial = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_void_partial",
        "succeeded",
        700_000,
        IDR,
    );
    let receipt = receipt_of(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_VOID_PARTIAL,
            &invoice_id,
            STRIPE,
            "evt_void_partial",
            "succeeded",
            700_000,
            partial,
        ),
    )
    .await;
    assert_eq!(
        receipt["status"].as_str(),
        Some("issued"),
        "a partial settlement leaves issued, not settled"
    );
    assert_eq!(
        receipt["outstandingMinor"].as_i64(),
        Some(NIGHT_GROSS - 700_000)
    );
    refused_conflict(&app, &bx, &void_body(ACME, KEY_VOID_TRY, &invoice_id, 3)).await;
    assert_eq!(
        count_of(&fixture, "commands", ACME).await,
        3,
        "the refused void recorded nothing"
    );

    // 2. A FULL settle puts the invoice in `settled` — the void still
    //    refuses (settled must refund first, not void).
    let full = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_void_full",
        "succeeded",
        800_000,
        IDR,
    );
    let receipt = receipt_of(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_VOID_SETTLE,
            &invoice_id,
            STRIPE,
            "evt_void_full",
            "succeeded",
            800_000,
            full,
        ),
    )
    .await;
    assert_eq!(receipt["status"].as_str(), Some("settled"));
    assert_eq!(receipt["outstandingMinor"].as_i64(), Some(0));
    refused_conflict(
        &app,
        &bx,
        &void_body(ACME, KEY_VOID_SETTLED, &invoice_id, 4),
    )
    .await;

    // 3. The FULL refund empties the balance and REOPENS `issued` — now
    //    the void walks.
    let refund = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_void_refund",
        "refunded",
        NIGHT_GROSS,
        IDR,
    );
    let receipt = receipt_of(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_REFUND,
            &invoice_id,
            STRIPE,
            "evt_void_refund",
            "refunded",
            NIGHT_GROSS,
            refund,
        ),
    )
    .await;
    assert_eq!(
        receipt["status"].as_str(),
        Some("issued"),
        "the refund reopens the settled invoice"
    );
    assert_eq!(receipt["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    let receipt = receipt_of(&app, &bx, &void_body(ACME, KEY_VOID_OK, &invoice_id, 5)).await;
    assert_eq!(receipt["status"].as_str(), Some("void"));

    // 4. Void is terminal: a second void refuses.
    refused_conflict(&app, &bx, &void_body(ACME, KEY_VOID_TWICE, &invoice_id, 6)).await;

    // 5. The final row: voided, the settled counter emptied, the audit
    //    naming six commands' worth of operations (create, issue, settle,
    //    settle, refund, void — the refusals recorded none).
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.0, "void");
    assert_eq!(invoice.3, 0);
    // The void touches the STATUS — the outstanding cell keeps the
    // ledger invariant's truth (`gross − settled`), which is why the
    // priced-invoice immutability holds even here.
    assert_eq!(invoice.4, NIGHT_GROSS);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 6);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 6);
}

// --- W5: settlement beyond the balance holds nothing --------------------------------------

/// Settlement beyond the balance refuses and holds nothing: a succeeded
/// claim over the outstanding balance answers the conflict carrier with
/// NO event row and NO command row (the claim's own insertion rides the
/// same rollback), nor any audit row.
#[tokio::test]
async fn settlement_beyond_the_balance_refuses_and_holds_nothing() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_OVER_CREATE, "INV-050", "customer-5"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(&app, &bx, &issue_body(ACME, KEY_OVER_ISSUE, &invoice_id, 1)).await;

    let over = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_overpay",
        "succeeded",
        NIGHT_GROSS + 1,
        IDR,
    );
    refused_conflict(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_OVER_EVENT,
            &invoice_id,
            STRIPE,
            "evt_overpay",
            "succeeded",
            NIGHT_GROSS + 1,
            over,
        ),
    )
    .await;

    // The held-back facts: no event row, no command row, no audit row,
    // the settled counter still zero, the revision still the issue's.
    assert_eq!(event_rows(&fixture, ACME).await, Vec::new());
    assert_eq!(
        count_of(&fixture, "commands", ACME).await,
        2,
        "create + issue only"
    );
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 2);
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.3, 0);
    assert_eq!(invoice.5, 2);
}

// --- W6: a failed outcome moves no money and no revision ------------------------------------

/// A FAILED outcome is history: the signed claim lands, its row is
/// recorded, its receipt is the `failed` label — and the invoice's
/// revision, settled amount and status move not one increment.
#[tokio::test]
async fn a_failed_outcome_moves_no_money_and_no_revision() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_FAIL_CREATE, "INV-060", "customer-6"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(&app, &bx, &issue_body(ACME, KEY_FAIL_ISSUE, &invoice_id, 1)).await;

    let failed = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_failed",
        "failed",
        400_000,
        IDR,
    );
    let receipt = receipt_of(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_FAIL_EVENT,
            &invoice_id,
            STRIPE,
            "evt_failed",
            "failed",
            400_000,
            failed,
        ),
    )
    .await;
    assert_eq!(
        receipt["status"].as_str(),
        Some("failed"),
        "the receipt's own label, NOT an invoice status"
    );
    assert_eq!(
        receipt["revision"].as_i64(),
        Some(2),
        "a failed claim moves no revision"
    );
    assert_eq!(receipt["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    assert!(receipt["eventId"].as_str().is_some());
    assert_eq!(
        event_rows(&fixture, ACME).await,
        vec![("evt_failed".to_string(), "failed".to_string(), 400_000)],
        "the failed claim is STILL a recorded event row"
    );
    let invoice = invoice_row(&fixture, ACME, &invoice_id).await;
    assert_eq!(invoice.0, "issued");
    assert_eq!(invoice.3, 0, "no money moved");
    assert_eq!(invoice.5, 2, "no revision bump");
    assert_eq!(count_of(&fixture, "commands", ACME).await, 3);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 3);
}

// --- W7: the attempt arm asserts no result -----------------------------------------------------

/// The attempt arm asserts no result: a providerRef rides the intent
/// (present or absent), the receipt stays `attempted` with the invoice's
/// own revision, the balance never moves — and a VOIDED invoice takes no
/// attempt at all (the refusal recorded none).
#[tokio::test]
async fn the_attempt_arm_asserts_no_result() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_ATTEMPT_CREATE, "INV-070", "customer-7"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(
        &app,
        &bx,
        &issue_body(ACME, KEY_ATTEMPT_ISSUE, &invoice_id, 1),
    )
    .await;

    // 1. The bare attempt: no providerRef (absent = explicit null).
    let bare = receipt_of(
        &app,
        &bx,
        &attempt_body(ACME, KEY_ATTEMPT_ONE, &invoice_id, 300_000, None),
    )
    .await;
    assert_eq!(bare["status"].as_str(), Some("attempted"));
    assert_eq!(bare["revision"].as_i64(), Some(2));
    assert_eq!(bare["outstandingMinor"].as_i64(), Some(NIGHT_GROSS));
    assert!(bare["attemptId"].as_str().is_some());

    // 2. A second attempt WITH the ref: both land, still no revision.
    receipt_of(
        &app,
        &bx,
        &attempt_body(ACME, KEY_ATTEMPT_TWO, &invoice_id, 100_000, Some("ref-2")),
    )
    .await;
    let mut attempts = attempt_rows(&fixture, ACME).await;
    attempts.sort_by_key(|row| row.0);
    assert_eq!(
        attempts,
        vec![
            (100_000, IDR.to_string(), Some("ref-2".to_string())),
            (300_000, IDR.to_string(), None),
        ],
        "two attempts; the ref rides only where the arm carried one"
    );

    // 3. Void the invoice — the attempt refuses now.
    receipt_of(
        &app,
        &bx,
        &void_body(ACME, KEY_ATTEMPT_VOID, &invoice_id, 2),
    )
    .await;
    refused_conflict(
        &app,
        &bx,
        &attempt_body(ACME, KEY_ATTEMPT_THIRD, &invoice_id, 100_000, None),
    )
    .await;
    assert_eq!(count_of(&fixture, "payment_attempts", ACME).await, 2);
    // create + issue + attempt + attempt + void — the refused third
    // attempt recorded none.
    assert_eq!(count_of(&fixture, "commands", ACME).await, 5);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 5);
}

// --- W8: the revoked grant refuses the recovery -------------------------------------------

/// A revoked grant refuses the recovery: even a FRESH submission from a
/// dead membership refuses at the ladder — and once restored, the
/// byte-identical replay answers the stored receipt (`idempotencyReplay:
/// true`) while a fresh command walks again.
#[tokio::test]
async fn revoked_billing_grant_refused_on_recovery() {
    let (fixture, app, _bx, _bxg, _bxn, bxr, _other) = served().await;

    // 1. bxr commits once — the receipt facts are named.
    receipt_of(
        &app,
        &bxr,
        &create_body(ACME, KEY_REVOKED, "INV-080", "customer-8"),
    )
    .await;
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);

    // 2. Revoke: even the SAME bytes refuse at the ladder FIRST (the
    //    recovery gate is the membership, not the replay pair) — and no
    //    second invoice or command row is left.
    sqlx::query(
        "UPDATE archava_billing.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='bxr'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("deactivate bxr");
    refused_forbidden(
        &app,
        &bxr,
        &create_body(ACME, KEY_REVOKED_RETRY, "INV-081", "customer-8b"),
    )
    .await;
    assert_eq!(
        command_count(&fixture, ACME, KEY_REVOKED_RETRY).await,
        0,
        "the revoked submission claimed nothing"
    );
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 1);

    // 3. Restore: the REPLAY answers the stored receipt (fingerprint
    //    equality, no second row), then a fresh command walks again.
    sqlx::query(
        "UPDATE archava_billing.memberships SET active=true \
         WHERE tenant_id=$1 AND principal_id='bxr'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("reactivate bxr");
    let replay = receipt_of(
        &app,
        &bxr,
        &create_body(ACME, KEY_REVOKED, "INV-080", "customer-8"),
    )
    .await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(
        count_of(&fixture, "invoices", ACME).await,
        1,
        "the replay made nothing new"
    );
    let fresh = receipt_of(
        &app,
        &bxr,
        &create_body(ACME, KEY_REVOKED_CREATE, "INV-082", "customer-8c"),
    )
    .await;
    assert_eq!(fresh["status"].as_str(), Some("draft"));
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 2);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 2);
}

// --- W9: the cross-tenant refusal -----------------------------------------------------------

/// A cross-tenant caller is refused before any statement runs: the
/// GLOBEX member submitting ACME-tenant commands gets the ladder's 403
/// (both write and status read) — and acts freely in its own ledger.
#[tokio::test]
async fn cross_tenant_caller_refused_before_any_write() {
    let (fixture, app, _bx, _bxg, _bxn, _bxr, other) = served().await;

    // 1. The cross-tenant create refuses; nothing is named in ACME.
    refused_forbidden(
        &app,
        &other,
        &create_body(ACME, KEY_CROSS_READ, "INV-090", "customer-9"),
    )
    .await;
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 0);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 0);

    // 2. The cross-tenant STATUS read refuses the same way.
    let (status, _) = get(
        &app,
        &format!("/erp/billing/commands/status?tenant={ACME}&requestId={KEY_UNKNOWN}"),
        &other,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the cross-tenant read refuses"
    );

    // 3. The same member acts freely in GLOBEX — its own ledger, its own
    //    rows, ACME still untouched.
    let created = receipt_of(
        &app,
        &other,
        &create_body(GLOBEX, KEY_GLOBEX_CREATE, "INV-091", "customer-9"),
    )
    .await;
    assert_eq!(created["tenantId"].as_str(), Some(GLOBEX));
    assert_eq!(count_of(&fixture, "invoices", GLOBEX).await, 1);
    assert_eq!(count_of(&fixture, "commands", GLOBEX).await, 1);
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 0);
}

// --- W10: the same key with a different payload conflicts -------------------------------------

/// The same `commandId` with a DIFFERENT payload conflicts without
/// recording — the fingerprint cell is this ledger's idempotency gate,
/// and a mismatched fingerprint is the replay branch's own conflict,
/// whatever the arm behind it is.
#[tokio::test]
async fn same_key_different_command_conflicts() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    // 1. The honest create.
    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_REUSE_CREATE, "INV-100", "customer-10"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();

    // 2. The same key, a different customerRef — the fingerprint covers
    //    the payload, so this is the replay-branch conflict.
    refused_conflict(
        &app,
        &bx,
        &create_body(ACME, KEY_REUSE_CREATE, "INV-100", "customer-10-different"),
    )
    .await;

    // 3. The same key on a DIFFERENT arm type too — the mismatch is the
    //    fingerprint's, not the arm's.
    refused_conflict(
        &app,
        &bx,
        &issue_body(ACME, KEY_REUSE_CREATE, &invoice_id, 1),
    )
    .await;

    // 4. The ledger still carries exactly the one invoice, one command,
    //    one audit row — the refusals recorded none.
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 1);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 1);
    // And the surviving record is the honest one.
    assert_eq!(command_count(&fixture, ACME, KEY_REUSE_CREATE).await, 1);
}

// --- W11: the lost response replays the recorded receipt --------------------------------------

/// A lost response replays: the same `commandId` + bytes answer the
/// stored receipt with `idempotencyReplay: true` — and STILL one invoice,
/// one line row, one command row, one audit row. Nothing commits twice.
#[tokio::test]
async fn lost_response_replays_the_recorded_receipt() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    let body = create_body(ACME, KEY_REUSE_HOT, "INV-110", "customer-11");
    let original = receipt_of(&app, &bx, &body).await;
    assert_eq!(original["idempotencyReplay"].as_bool(), Some(false));

    let replayed = receipt_of(&app, &bx, &body).await;
    assert_eq!(replayed["commandId"].as_str(), Some(KEY_REUSE_HOT));
    assert_eq!(
        replayed["invoiceId"].as_str(),
        original["invoiceId"].as_str(),
        "the replay names the recorded invoice"
    );
    assert_eq!(replayed["idempotencyReplay"].as_bool(), Some(true));
    // Everything else is byte-for-byte the stored document.
    let mut stored = original.clone();
    stored["idempotencyReplay"] = json!(true);
    assert_eq!(replayed, stored);

    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);
    assert_eq!(count_of(&fixture, "invoice_lines", ACME).await, 1);
    assert_eq!(count_of(&fixture, "commands", ACME).await, 1);
    assert_eq!(command_count(&fixture, ACME, KEY_REUSE_HOT).await, 1);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 1);
}

// --- W12: the narrow grants refuse exactly their own gates --------------------------------------

/// Narrow grants refuse exactly the arms whose gate they lack: the
/// read+audit member refuses every write arm while its OWN status reads
/// walk (the status read grants NOTHING beyond enrollment — the TS status
/// read never checks a permission; this gate keeps exactly that).
#[tokio::test]
async fn insufficient_grants_refuse_before_any_statement() {
    let (fixture, app, bx, bxg, _bxn, _bxr, _other) = served().await;

    // The positive actor commits the setup: an ISSUED invoice (so every
    // refused arm below is a REAL grant refusal, not a state refusal).
    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_GATE_CREATE, "INV-120", "customer-12"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(&app, &bx, &issue_body(ACME, KEY_GATE_ISSUE, &invoice_id, 1)).await;
    let signature = signed_claim(
        ACME,
        &invoice_id,
        STRIPE,
        "evt_narrow",
        "succeeded",
        100,
        IDR,
    );

    // The read+audit member attemps every write arm's shape.
    for body in [
        create_body(ACME, KEY_GATE_GHOST, "INV-121", "customer-12b"),
        issue_body(ACME, KEY_GATE_ISSUE, &invoice_id, 2).to_string(),
        correction_body(ACME, KEY_GATE_CORRECTION, &invoice_id, 100, None),
        void_body(ACME, KEY_GATE_VOID, &invoice_id, 2),
        attempt_body(ACME, KEY_GATE_ATTEMPT, &invoice_id, 100, Some("ref-n")),
        event_body(
            ACME,
            KEY_GATE_EVENT,
            &invoice_id,
            STRIPE,
            "evt_narrow",
            "succeeded",
            100,
            signature.clone(),
        ),
    ] {
        let (status, answer) = post(&app, "/erp/billing/commands", &body, &bxg).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "the narrowed member refuses: {answer} for {body}"
        );
    }

    // The refusals recorded NOTHING: two commands (create + issue from
    // the setup), two audits, no events, no attempts, no corrections.
    assert_eq!(count_of(&fixture, "commands", ACME).await, 2);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 2);
    assert_eq!(count_of(&fixture, "payment_events", ACME).await, 0);
    assert_eq!(count_of(&fixture, "payment_attempts", ACME).await, 0);
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 0);
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 1);

    // The status read WALKS for the narrowed member — the TS status read
    // grants nothing beyond enrollment. Recorded receipt, then unknown.
    let recorded = status_of(&app, &bxg, ACME, KEY_GATE_CREATE).await;
    assert_eq!(recorded["status"].as_str(), Some("recorded"), "{recorded}");
    assert_eq!(
        recorded["receipt"]["action"].as_str(),
        Some("create_invoice")
    );
    let unknown = status_of(&app, &bxg, ACME, KEY_UNKNOWN).await;
    assert_eq!(unknown, json!({"status": "unknown"}));
}

// --- W13: the transition gates + the wire boundary ------------------------------------------------

/// The transition/state gates and the boundary refuse at the right layer:
/// issuing an issued invoice, correcting a draft and settling a draft are
/// the 409 class; a malformed document (unknown member, lowercase
/// currency, uppercase signature) is the parse's 400; and no refusal
/// records ANYTHING.
#[tokio::test]
async fn the_transition_gates_and_the_wire_boundary_refuse_at_the_right_layer() {
    let (fixture, app, bx, _bxg, _bxn, _bxr, _other) = served().await;

    // 1. Issue an ISSUED invoice — the second issue conflicts.
    let created = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_VOCAB_CREATE, "INV-140", "customer-14"),
    )
    .await;
    let invoice_id = created["invoiceId"].as_str().expect("uuid").to_string();
    receipt_of(
        &app,
        &bx,
        &issue_body(ACME, KEY_VOCAB_ISSUE, &invoice_id, 1),
    )
    .await;
    refused_conflict(
        &app,
        &bx,
        &issue_body(ACME, KEY_VOCAB_ISSUE2, &invoice_id, 2),
    )
    .await;

    // 2. CORRECT a DRAFT — corrections apply to an issued invoice.
    let created2 = receipt_of(
        &app,
        &bx,
        &create_body(ACME, KEY_VOCAB_CREATE2, "INV-141", "customer-14b"),
    )
    .await;
    let draft_invoice = created2["invoiceId"].as_str().expect("uuid").to_string();
    refused_conflict(
        &app,
        &bx,
        &correction_body(ACME, KEY_VOCAB_CORRECT, &draft_invoice, -100, None),
    )
    .await;

    // 3. SETTLE a DRAFT — a perfectly signed claim on a draft invoice
    //    refuses, and even the claim's event row never lands.
    let signature = signed_claim(
        ACME,
        &draft_invoice,
        STRIPE,
        "evt_draft_settle",
        "succeeded",
        100,
        IDR,
    );
    refused_conflict(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_VOCAB_EVENT,
            &draft_invoice,
            STRIPE,
            "evt_draft_settle",
            "succeeded",
            100,
            signature,
        ),
    )
    .await;
    assert_eq!(event_rows(&fixture, ACME).await, Vec::new());

    // 4. The parse boundary: an unknown member, a lowercase currency and
    //    an uppercase signature are 400s — before any statement runs.
    refused_parse(
        &app,
        &bx,
        &create_body(ACME, KEY_VOCAB_BADPARSE, "INV-142", "customer-14c")
            .replace("\"currency\":\"IDR\"", "\"currency\":\"idr\""),
    )
    .await;
    refused_parse(
        &app,
        &bx,
        &create_body(ACME, KEY_VOCAB_BADPARSE, "INV-142", "customer-14c")
            .replace("\"customerRef\"", "\"customerRefX\""),
    )
    .await;
    refused_parse(
        &app,
        &bx,
        &event_body(
            ACME,
            KEY_VOCAB_BADPARSE,
            DEAD_CORRECTION,
            STRIPE,
            "evt_upper",
            "succeeded",
            100,
            "BE969B85AC484BFC834B7E8313E1F716965D371801ADB05B21F2A1A7C52E03CA".to_string(),
        ),
    )
    .await;

    // 5. The held facts: three commands (the two creates and the issue),
    //    nothing from the refusals, and the corrections' chain untouched.
    assert_eq!(count_of(&fixture, "commands", ACME).await, 3);
    assert_eq!(count_of(&fixture, "invoice_corrections", ACME).await, 0);
    assert_eq!(count_of(&fixture, "invoices", ACME).await, 2);
}
