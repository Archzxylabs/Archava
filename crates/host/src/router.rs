//! The HTTP router.
//!
//! This file mounts the whole host surface — the authority chain below, the CRM
//! reads and commands, the owner-configuration lifecycle, the assistant
//! conversation, the ERP ledger cohorts, cockpit and orbz, the payment/email/
//! presence integrations, and a small set of explicitly public routes. It used
//! to describe seven routes; the surface is now grouped into route families,
//! and a family is only authorized if it actually runs the chain. Do not read
//! the chain below as something the router applies to every route for you.
//!
//! ## Route families
//!
//! | Family | Gate |
//! | --- | --- |
//! | `/login`, `/logout` | session lifecycle only |
//! | `/commands`, `/approvals`, `/crm/*`, `/own/*`, `/api/crm/*` | extraction → session → grant → gate → write |
//! | `/erp/*/commands`, `/erp/*/*` reads | session + CSRF, then the ledger's own grant and RLS |
//! | `/api/v1/cockpit/*`, `/api/v1/orbz/*` | session + CSRF; mission/memory scope from the session |
//! | `/api/v1/presets/*`, `/api/v1/feed/*` | session (mutations also CSRF); the feed is synthesized demo data |
//! | `/api/preview/generate`, `/api/migration/*` | session + CSRF; server-bound review and atomic import commit |
//! | `/api/webhooks/privy` | provider shared secret, timing-safe |
//! | `/api/webhooks/payment/*` | provider signature before any transaction |
//! | `/embed/checkout` | HMAC signature over the rendered values, minted by `POST /embed/checkout/sign` behind session + CSRF |
//! | `/healthz`, `/readyz`, `/metrics`, `/v1/archava.js`, `/*.css` | public; `/metrics` needs a loopback peer or `ARCHAVA_METRICS_TOKEN` |
//!
//! ## The order the code enforces
//!
//! 1. **Extraction.** Malformed input dies here, with a fixed refusal that
//!    never reflects the input (D09). No database, no authority.
//! 2. **Session.** [`authenticate`] resolves the token against
//!    `archava_identity.sessions` — active user, matching user version,
//!    unexpired, seen within the idle window. Mutations additionally present
//!    the CSRF header *derived from that token*.
//! 3. **Grant.** The principal's ACTIVE membership row in the *requested*
//!    tenant (role/capability/scope columns from migration 004), the bound
//!    approval for this exact command, and the caller's own receipt by its
//!    idempotency key.
//! 4. **Gate.** `CommandAuthorizer::evaluate` on an `ActionRequest` built from
//!    steps 2–3 only — never from body claims.
//! 5. **Write.** Only after `Allowed`: consume the approval (single-use),
//!    record the receipt, commit.
//!
//! The request body supplies the *target* — tenant, action, input, revision,
//! idempotency key — and the narrowing signals (`available_on_page`,
//! `confirmed`, `declined`, which can only subtract). It has no field for
//! role, capability, enabled actions or human approval, and the old
//! `human_approved: true` claim is dropped at deserialization and would not
//! satisfy the gate anyway: the gate reads the approvals table.
//!
//! ## What the negative tests cover
//!
//! G03 requires negative tenant, actor, grant, scope, revocation and unknown
//! service tests at real entrypoints. In this host:
//!
//! | Refused input | Where |
//! | --- | --- |
//! | no session / invalid / expired / deactivated | [`authenticate`] → 401 `unauthenticated` |
//! | mutation without the matching CSRF header | [`authenticate`] → 403 `csrf_required` |
//! | forged tenant in the body | [`authority::resolve_membership`] → 403 `missing_membership` — the same answer a tenant that never existed gets |
//! | `role` / `capability` / `human_approved` fields in the body | not fields of `CommandSubmission`; the grant comes from the membership row, the approval from its table |
//! | revoked grant (`active = false`) | `active` is in the membership query's WHERE — absent and revoked are one answer, on execute, status and replay alike |
//! | membership scope below the action's level | [`authority::derive_request`] → 403 `missing_scope` |
//! | approval unbound / unconsumed-but-unapproved / expired / already consumed | [`authority::load_bound_approval`] is false → the gate refuses again; consumption is single-use in SQL |
//! | unknown action | `CommandAuthorizer` → 404 `unknown_action` |
//! | declined earlier in session | `DeclinedByVisitor` (a session-level decline; **not** a grant revocation) |
//! | invalid tenant slug | `TenantId` has no `From<String>` — refused at deserialization, before any handler runs |
//!
//! The last two rows are the ones worth stating plainly: the tenant boundary
//! is a property of the type, and `declined` is the visitor's own signal —
//! revocation is resolved from the membership row or not at all.

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use archava_adapters::rls::assert_startup_readiness;
use archava_domain::authority::{ActionDenial, CommandAuthorizer, PolicyDecision};

use crate::authority::AuthorityError;
use crate::identity::{AuthenticatedSession, Identity};
use crate::metrics::Metrics;

pub use crate::routes::auth::{
    expire_session_cookie, refused_login_outcome, LoginOutcome, LoginSource, LoginSubmission,
    LogoutOutcome, NO_CONNECT_INFO_SOURCE,
};
pub use crate::routes::commands::{
    approval_grant, approval_request, command, command_status, ApprovalSubmission,
    ApproveSubmission, CommandSubmission, StatusQuery, APPROVAL_TTL_SECONDS, MAX_INPUT_BYTES,
};
pub use crate::routes::system::{
    canonical_checkout, checkout_params, checkout_refusal, database_ready, health,
    metrics_authorized, metrics_endpoint, percent_encode, ready, sanitize_identifier,
    serve_archava_css, serve_embed_checkout, serve_embed_script, serve_workspace_css,
    sign_embed_checkout, CheckoutParams, EmbedCheckoutQuery, SignCheckoutRequest,
};

/// The CSRF header a mutation must present. Its expected value is
/// `HMAC(key, "csrf\0" + token)` — derived by the server, never stored, and
/// not computable by a cross-site page that can neither read the token nor
/// guess the key.
const CSRF_HEADER: &str = "x-archava-csrf";

use crate::security::cookie_token;

/// The ledger this host serves.
///
/// The baseline hardcodes one of these per package — `'archava_commerce'`,
/// `'archava_laundry'` (`rg "regnamespace"` finds all of them) — and the
/// non-owner check reads the namespace to decide whether the connecting role
/// owns any table in it. It is a parameter here rather than a constant so one
/// host binary can serve a named ledger, and so the namespace that reaches the
/// SQL is always the one the caller declared rather than one inferred from a
/// tenant id.
#[derive(Debug, Clone, Copy)]
pub struct Ledger {
    /// The PostgreSQL schema, e.g. `archava_commerce`.
    pub namespace: &'static str,
    /// The label used in error messages, matching the baseline's per-ledger
    /// strings (`Commerce requires a non-owner runtime`).
    pub ledger: &'static str,
    /// The tenant-scoped tables whose RLS must be forced.
    ///
    /// The baseline's `assert*Kernel` checks exactly this list per package, and
    /// the count must match: a table that is missing, or enabled but not forced,
    /// does not appear in the `pg_class` result and the gate falls short.
    /// Since migration 004 that list includes `approvals`.
    pub tables: &'static [&'static str],
}

/// The checksum ledgers a complete host assembly must verify before it binds.
/// Keeping these together makes it harder for a launcher to mount a route
/// family while silently omitting that family's startup contract.
pub struct AllReadiness<'a> {
    pub commerce: &'a [(String, String)],
    pub identity: &'a [(String, String)],
    pub crm: &'a [(String, String)],
    pub owner: &'a [(String, String)],
    pub purchasing: &'a [(String, String)],
    pub additional_erp: &'a [(&'a str, &'a [(String, String)])],
}

/// Shared state for every route.
#[derive(Debug, Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
    payments: Arc<crate::payments::PaymentRegistry>,
    email: Arc<crate::mail::EmailRegistry>,
    pub presence: Arc<crate::presence::PresenceRegistry>,
    /// The merchant origins allowed to frame `/embed/checkout`. Empty means
    /// `frame-ancestors 'none'`. Configured at startup, never per request.
    embed_frame_ancestors: Arc<Vec<String>>,
    /// The credential `/metrics` requires from a non-loopback peer. `None`
    /// means only a loopback peer may scrape it.
    metrics_token: Option<Arc<str>>,
    trusted_proxies: crate::security::TrustedProxies,
    agent_config: Arc<crate::agent::AgentConfig>,
}

#[derive(Debug)]
struct AppStateInner {
    pool: PgPool,
    ledger: Ledger,
    authorizer: CommandAuthorizer,
    identity: Identity,
    metrics: Metrics,
    cockpit: archava_cockpit::CockpitEngine,
    orbz: archava_orbz::OrbzStore,
}

impl AppState {
    /// Builds serving state, but only after startup readiness passes.
    ///
    /// This is the only constructor reachable outside this crate, and that is
    /// the structural half of the required check *"host refuses to serve
    /// before readiness passes"*: a `Router` is built from an `AppState`, an
    /// `AppState` outside this crate comes only from here, and here runs two
    /// gates before any state exists —
    ///
    /// 1. [`archava_adapters::rls::assert_startup_readiness`] (the baseline's
    ///    `assert*Kernel`, in order: non-owner runtime, exact migration
    ///    checksum ledger, forced row-level security on every table the ledger
    ///    declares — `packages/commerce-pg/src/kernel.ts:42-76`), then
    /// 2. the identity gate: a non-owner runtime on `archava_identity`, its
    ///    exact `(id, sha256)` history, and an HMAC key of at least 32 bytes
    ///    (`packages/identity-pg/src/sessions.ts:25-33`).
    ///
    /// On any failure no state exists, so no router exists, so nothing binds
    /// and nothing answers — a configuration error is detected before the
    /// first request rather than as a cross-tenant leak after it.
    ///
    /// `expected_migrations` / `expected_identity_migrations` are the two
    /// `(id, sha256)` ledgers this build ships, in apply order — the same
    /// bytes their migrators recorded. The caller supplies them because the
    /// host is ledger-agnostic: which migrations "this build expects" belongs
    /// to whoever assembles the binary.
    pub async fn try_new(
        pool: PgPool,
        ledger: Ledger,
        expected_migrations: &[(String, String)],
        expected_identity_migrations: &[(String, String)],
        identity_key: &[u8],
    ) -> Result<Self, AuthorityError> {
        assert_startup_readiness(
            &pool,
            ledger.namespace,
            ledger.ledger,
            ledger.tables,
            expected_migrations,
        )
        .await?;
        if ledger.namespace == "archava_commerce"
            && expected_migrations
                .iter()
                .any(|(id, _)| id == "010_return_layer_provenance")
        {
            crate::erp::commerce::assert_return_provenance_readiness(&pool).await?;
        }
        let identity =
            Identity::open(pool.clone(), identity_key, expected_identity_migrations).await?;
        archava_adapters::writer::assert_ready(&pool).await?;
        Ok(Self {
            payments: Arc::new(crate::payments::PaymentRegistry::default()),
            email: Arc::new(crate::mail::EmailRegistry::default()),
            presence: Arc::new(crate::presence::PresenceRegistry::default()),
            embed_frame_ancestors: Arc::new(Vec::new()),
            metrics_token: None,
            trusted_proxies: crate::security::TrustedProxies::default(),
            agent_config: Arc::new(crate::agent::AgentConfig::default()),
            inner: Arc::new(AppStateInner {
                pool,
                ledger,
                authorizer: CommandAuthorizer::new(),
                identity,
                metrics: Metrics::new(),
                cockpit: archava_cockpit::CockpitEngine::new(),
                orbz: archava_orbz::OrbzStore::new(),
            }),
        })
    }

    /// The pool every ledger transaction opens on. `crm.rs` opens the CRM
    /// ledger's transactions through it; nothing else in this crate needs the
    /// field directly.
    pub(crate) fn pool(&self) -> &PgPool {
        &self.inner.pool
    }

    pub fn with_agent_config(mut self, config: crate::agent::AgentConfig) -> Self {
        self.agent_config = Arc::new(config);
        self
    }
    pub(crate) fn agent_config(&self) -> &crate::agent::AgentConfig {
        &self.agent_config
    }

    pub fn with_payment_registry(mut self, registry: crate::payments::PaymentRegistry) -> Self {
        self.payments = Arc::new(registry);
        self
    }
    pub(crate) fn payments(&self) -> &crate::payments::PaymentRegistry {
        &self.payments
    }

    pub fn with_email_registry(mut self, registry: crate::mail::EmailRegistry) -> Self {
        self.email = Arc::new(registry);
        self
    }
    pub(crate) fn email_registry(&self) -> &crate::mail::EmailRegistry {
        &self.email
    }

    pub fn with_presence_registry(mut self, registry: crate::presence::PresenceRegistry) -> Self {
        self.presence = Arc::new(registry);
        self
    }
    pub fn presence(&self) -> &crate::presence::PresenceRegistry {
        &self.presence
    }

    /// The identity store — sessions plus the server secret the CRM
    /// fingerprints are keyed by.
    pub(crate) fn identity(&self) -> &Identity {
        &self.inner.identity
    }

    /// The identity gate and the serving state, plus a readiness pass for the
    /// CRM ledger — the constructor the §1.4B slice uses.
    ///
    /// Serving CRM is a second ledger's worth of forced row-level security and
    /// checksum-exact migrations (`archava_crm`), so a host that serves the
    /// slice refuses to build any state at all unless BOTH ledgers pass: this
    /// method is [`AppState::try_new`] followed by the same startup assertion
    /// for [`crate::erp::crm::CRM_LEDGER`]. The per-transaction non-owner check
    /// (inside [`TenantTransaction::begin`]) still applies on every CRM
    /// request on top of this gate.
    pub async fn try_new_crm(
        pool: PgPool,
        ledger: Ledger,
        expected_migrations: &[(String, String)],
        expected_identity_migrations: &[(String, String)],
        identity_key: &[u8],
        expected_crm_migrations: &[(String, String)],
    ) -> Result<Self, AuthorityError> {
        let state = Self::try_new(
            pool,
            ledger,
            expected_migrations,
            expected_identity_migrations,
            identity_key,
        )
        .await?;
        assert_startup_readiness(
            state.pool(),
            crate::erp::crm::CRM_LEDGER.namespace,
            crate::erp::crm::CRM_LEDGER.ledger,
            crate::erp::crm::CRM_LEDGER.tables,
            expected_crm_migrations,
        )
        .await?;
        Ok(state)
    }

    /// Serving state for the owner-configuration surface: [`try_new_crm`]'s
    /// gates continued by the `archava_owner` ledger's own checksum-exact
    /// readiness and forced-RLS check. Only the owner surface needs it, so
    /// existing CRM/ERP callers keep their signatures.
    pub async fn try_new_owner(
        pool: PgPool,
        ledger: Ledger,
        expected_migrations: &[(String, String)],
        expected_identity_migrations: &[(String, String)],
        identity_key: &[u8],
        expected_crm_migrations: &[(String, String)],
        expected_owner_migrations: &[(String, String)],
    ) -> Result<Self, AuthorityError> {
        let state = Self::try_new_crm(
            pool,
            ledger,
            expected_migrations,
            expected_identity_migrations,
            identity_key,
            expected_crm_migrations,
        )
        .await?;
        assert_startup_readiness(
            state.pool(),
            crate::own::NAMESPACE,
            crate::own::LEDGER,
            crate::own::OWNER_TABLES,
            expected_owner_migrations,
        )
        .await?;
        crate::own::assert_disclosure_readiness(state.pool()).await?;
        Ok(state)
    }

    /// Serving state for the complete mounted product surface: commerce, CRM,
    /// owner configuration, purchasing, and every ERP ledger mounted by this
    /// host. The persistent launcher uses this constructor so a process cannot
    /// bind only after checking a partial subset of the routes it serves.
    pub async fn try_new_all(
        pool: PgPool,
        ledger: Ledger,
        readiness: AllReadiness<'_>,
        identity_key: &[u8],
    ) -> Result<Self, AuthorityError> {
        let state = Self::try_new_owner(
            pool,
            ledger,
            readiness.commerce,
            readiness.identity,
            identity_key,
            readiness.crm,
            readiness.owner,
        )
        .await?;
        state
            .assert_erp_readiness(readiness.purchasing, readiness.additional_erp)
            .await?;
        Ok(state)
    }

    async fn assert_erp_readiness(
        &self,
        expected_purchasing_migrations: &[(String, String)],
        additional_erp_readiness: &[(&str, &[(String, String)])],
    ) -> Result<(), AuthorityError> {
        assert_startup_readiness(
            self.pool(),
            crate::erp::PURCHASING_LEDGER.namespace,
            crate::erp::PURCHASING_LEDGER.ledger,
            crate::erp::PURCHASING_LEDGER.tables,
            expected_purchasing_migrations,
        )
        .await?;
        for (namespace, expected_migrations) in additional_erp_readiness {
            let ledger = match *namespace {
                "archava_foodservice" => crate::erp::FOODSERVICE_LEDGER,
                "archava_organizations" => crate::erp::ORGANIZATIONS_LEDGER,
                "archava_accounting" => crate::erp::ACCOUNTING_LEDGER,
                "archava_proposals" => crate::erp::PROPOSALS_LEDGER,
                "archava_projects" => crate::erp::PROJECTS_LEDGER,
                "archava_scheduling" => crate::erp::SCHEDULING_LEDGER,
                "archava_accommodation" => crate::erp::ACCOMMODATION_LEDGER,
                "archava_assets" => crate::erp::ASSETS_LEDGER,
                "archava_billing" => crate::erp::BILLING_LEDGER,
                "archava_subscriptions" => crate::erp::SUBSCRIPTIONS_LEDGER,
                "archava_attendance" => crate::erp::ATTENDANCE_LEDGER,
                "archava_support" => crate::erp::SUPPORT_LEDGER,
                "archava_laundry" => crate::erp::LAUNDRY_LEDGER,
                "archava_field" => crate::erp::FIELD_LEDGER,
                "archava_fulfillment" => crate::erp::FULFILLMENT_LEDGER,
                "archava_edu" => crate::erp::EDU_LEDGER,
                "archava_cockpit" => crate::cockpit::COCKPIT_LEDGER,
                "archava_orbz" => crate::orbz::ORBZ_LEDGER,
                _ => return Err(AuthorityError::InvalidErpLedger),
            };
            assert_startup_readiness(
                self.pool(),
                ledger.namespace,
                ledger.ledger,
                ledger.tables,
                expected_migrations,
            )
            .await?;
            if *namespace == "archava_accounting" {
                crate::erp::accounting::assert_control_readiness(self.pool()).await?;
            }
        }
        Ok(())
    }

    /// Serving state for the ERP cohorts — [`AppState::try_new_crm`]'s shape
    /// continued by readiness checks for every mounted ERP ledger. The kernel
    /// receipts write cross-ledger into `archava_commerce`, so the app still
    /// builds on the commerce ledger as the primary; the ERP descriptors are
    /// supplied by the assembler because each expected migration checksum is
    /// part of the release identity.
    pub async fn try_new_erp(
        pool: PgPool,
        ledger: Ledger,
        expected_migrations: &[(String, String)],
        expected_identity_migrations: &[(String, String)],
        identity_key: &[u8],
        expected_purchasing_migrations: &[(String, String)],
        additional_erp_readiness: &[(&str, &[(String, String)])],
    ) -> Result<Self, AuthorityError> {
        let state = Self::try_new(
            pool,
            ledger,
            expected_migrations,
            expected_identity_migrations,
            identity_key,
        )
        .await?;
        state
            .assert_erp_readiness(expected_purchasing_migrations, additional_erp_readiness)
            .await?;
        Ok(state)
    }

    /// Builds state without checking readiness — test-only, hence
    /// `#[cfg(test)]`: the in-crate tests drive refusals that fire before any
    /// database access (extraction, then the session lookup), so they run on
    /// a pool that never connects and must not pay a readiness round-trip
    /// they cannot satisfy. Nothing outside this crate can reach this even in
    /// a test build — it is `pub(crate)` — and external callers get
    /// [`AppState::try_new`], which refuses.
    #[cfg(test)]
    pub(crate) fn new(pool: PgPool, ledger: Ledger, identity: Identity) -> Self {
        Self {
            payments: Arc::new(crate::payments::PaymentRegistry::default()),
            email: Arc::new(crate::mail::EmailRegistry::default()),
            presence: Arc::new(crate::presence::PresenceRegistry::default()),
            embed_frame_ancestors: Arc::new(Vec::new()),
            metrics_token: None,
            trusted_proxies: crate::security::TrustedProxies::default(),
            agent_config: Arc::new(crate::agent::AgentConfig::default()),
            inner: Arc::new(AppStateInner {
                pool,
                ledger,
                authorizer: CommandAuthorizer::new(),
                identity,
                metrics: Metrics::new(),
                cockpit: archava_cockpit::CockpitEngine::new(),
                orbz: archava_orbz::OrbzStore::new(),
            }),
        }
    }

    pub fn ledger(&self) -> &Ledger {
        &self.inner.ledger
    }

    pub(crate) fn authorizer(&self) -> &CommandAuthorizer {
        &self.inner.authorizer
    }

    pub(crate) fn identity_key(&self) -> &[u8] {
        self.inner.identity.key()
    }

    pub fn cockpit(&self) -> &archava_cockpit::CockpitEngine {
        &self.inner.cockpit
    }

    pub fn orbz(&self) -> &archava_orbz::OrbzStore {
        &self.inner.orbz
    }

    /// Process-local HTTP counters. `router` observes every response through
    /// them; `/metrics` is the only reader.
    pub(crate) fn metrics(&self) -> &Metrics {
        &self.inner.metrics
    }

    /// The merchant origins permitted to frame `/embed/checkout`.
    ///
    /// Startup configuration, the same way the payment and email registries
    /// are: an origin cannot be widened by a request, and an empty list denies
    /// framing rather than allowing it.
    pub fn with_embed_frame_ancestors(mut self, origins: Vec<String>) -> Self {
        self.embed_frame_ancestors = Arc::new(origins);
        self
    }

    pub(crate) fn embed_frame_ancestors(&self) -> &[String] {
        &self.embed_frame_ancestors
    }

    /// The scrape credential `/metrics` requires from a non-loopback peer.
    pub fn with_metrics_token(mut self, token: impl Into<Arc<str>>) -> Self {
        self.metrics_token = Some(token.into());
        self
    }

    pub(crate) fn metrics_token(&self) -> Option<&str> {
        self.metrics_token.as_deref()
    }

    pub fn with_trusted_proxies(mut self, proxies: crate::security::TrustedProxies) -> Self {
        self.trusted_proxies = proxies;
        self
    }

    pub(crate) fn trusted_proxies(&self) -> &crate::security::TrustedProxies {
        &self.trusted_proxies
    }
}

/// A ledger descriptor for in-crate tests in *other* modules: the preview case
/// lives outside this file and cannot reach `mod tests::ledger`.
#[cfg(test)]
pub(crate) fn test_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: &[
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
        ],
    }
}

/// Builds the router.
pub fn router(state: AppState) -> Router {
    crate::routes::mount_routes()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            observe_request,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::security::security_headers,
        ))
        .with_state(state)
}

/// Records every completed response in the process counters. The middleware
/// is inside the security-header layer, so a metrics scrape carries the same
/// `nosniff`/`no-store` policy as every other answer.
async fn observe_request(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    state
        .metrics()
        .observe(response.status(), started.elapsed());
    response
}

// --- Response shapes ---------------------------------------------------------

/// The answer to a command, approval or status request.
#[derive(Debug, Serialize)]
pub struct CommandOutcome {
    /// One of: `allowed`, `replayed`, `receipt`, `confirmation_required`,
    /// `denied`, `rejected`, `refused`, `unauthenticated`,
    /// `approval_requested`, `approved`.
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Set only when the session's principal holds a membership in the tenant
    /// being reported. Deliberately absent on every refusal: a response that
    /// named a tenant to a caller with no grant there would confirm the
    /// tenant's existence (PRD §15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl CommandOutcome {
    /// An outcome with every optional field empty — responses start here and
    /// add only what they mean to return.
    pub(crate) fn base(decision: &str) -> Self {
        Self {
            decision: decision.to_string(),
            reason: None,
            message: None,
            prompt: None,
            tenant: None,
            request_id: None,
            approval_id: None,
            receipt: None,
            fingerprint: None,
        }
    }
}

impl From<&PolicyDecision> for CommandOutcome {
    fn from(decision: &PolicyDecision) -> Self {
        match decision {
            PolicyDecision::Allowed { .. } => Self::base("allowed"),
            PolicyDecision::ConfirmationRequired { prompt, mode, .. } => Self {
                // The mode travels with the prompt so the client knows which
                // kind of gate it is being asked to satisfy — a "confirm"
                // button and a "request human approval" action are not the
                // same UI, and the baseline's prompts are written per mode
                // for that reason.
                prompt: Some(format!("{}: {prompt}", mode.as_str())),
                ..Self::base("confirmation_required")
            },
            PolicyDecision::Denied {
                reason, message, ..
            } => Self {
                reason: Some(reason.as_str().to_string()),
                message: Some(message.clone()),
                ..Self::base("denied")
            },
        }
    }
}

/// The status a decision maps to.
///
/// A refusal is a 4xx because the caller can fix it; a server fault is a 5xx
/// because the caller cannot. Conflating them would tell an operator to retry a
/// request that will be refused every time.
pub(crate) fn status_for(decision: &PolicyDecision) -> StatusCode {
    match decision {
        PolicyDecision::Allowed { .. } => StatusCode::OK,
        PolicyDecision::ConfirmationRequired { .. } => StatusCode::ACCEPTED,
        PolicyDecision::Denied { reason, .. } => match reason {
            ActionDenial::UnknownAction => StatusCode::NOT_FOUND,
            ActionDenial::ActionDisabledForClient
            | ActionDenial::RoleNotAllowed
            | ActionDenial::CapabilityInsufficient
            | ActionDenial::ActionUnavailableOnPage
            | ActionDenial::AdminActionRequiresHuman
            | ActionDenial::DeclinedByVisitor => StatusCode::FORBIDDEN,
        },
    }
}

// --- Fixed refusals ----------------------------------------------------------
//
// Every one of these returns text it wrote itself. Nothing here formats a
// caller-supplied string, a database error or a field name into the response:
// the input travels in the log, not back to the client (D09).

/// The refusal for a caller with no usable session.
pub(crate) fn unauthenticated() -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(CommandOutcome {
            reason: Some("no_valid_session".to_string()),
            message: Some("A server-issued session is required.".to_string()),
            ..CommandOutcome::base("unauthenticated")
        }),
    )
}

/// The status for a request body the router could not turn into a command.
///
/// This is a security fix, not a cosmetic one. Axum's default `Json` rejection
/// body echoes the rejected input verbatim — a submission carrying
/// `"tenant": "../etc/passwd"` came back as
/// `tenant: tenantId must be a lowercase kebab-case slug, got "../etc/passwd"`
/// in the first version of this route: a reflection primitive on an endpoint
/// that has no business echoing anything. Logged, not returned. `%rejection`
/// carries the field name and the offending value, which is exactly what must
/// not reach the client.
pub(crate) fn rejection(rejection: JsonRejection) -> (StatusCode, Json<CommandOutcome>) {
    tracing::warn!(error = %rejection, "refused a submission at the boundary");

    (
        rejection.status(),
        Json(CommandOutcome {
            reason: Some("malformed_submission".to_string()),
            // Fixed text. Deliberately identical for a bad tenant, a bad
            // revision and a bad payload: telling the caller which field
            // failed tells it which vocabulary to brute-force next.
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// The same refusal for a rejected `GET` query string — `Query`'s rejection
/// carries the offending parameter, so it gets the identical treatment.
pub(crate) fn query_rejection(rejection: QueryRejection) -> (StatusCode, Json<CommandOutcome>) {
    tracing::warn!(error = %rejection, "refused a status query at the boundary");

    (
        rejection.status(),
        Json(CommandOutcome {
            reason: Some("malformed_submission".to_string()),
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// Map an authority-resolution failure to its response.
pub(crate) fn authority_refusal(error: &AuthorityError) -> (StatusCode, Json<CommandOutcome>) {
    match error {
        AuthorityError::MissingMembership => (
            StatusCode::FORBIDDEN,
            Json(CommandOutcome {
                reason: Some("missing_membership".to_string()),
                // Fixed text on purpose: "no such tenant", "no such
                // principal", "membership deleted" and "membership revoked"
                // must be ONE answer, or the refusal is an existence oracle.
                message: Some("This session cannot perform that operation.".to_string()),
                ..CommandOutcome::base("denied")
            }),
        ),
        AuthorityError::MissingScope => (
            StatusCode::FORBIDDEN,
            Json(CommandOutcome {
                reason: Some("missing_scope".to_string()),
                message: Some("The membership's scope does not cover this action.".to_string()),
                ..CommandOutcome::base("denied")
            }),
        ),
        AuthorityError::UnusableGrant => {
            // Operator-facing data problem: the log names the value, the
            // caller gets an unavailability.
            tracing::error!(error = %error, "refusing: stored grant outside its vocabulary");
            unavailable("unusable_grant")
        }
        AuthorityError::InvalidSubmission => (
            StatusCode::BAD_REQUEST,
            Json(CommandOutcome {
                reason: Some("invalid_submission".to_string()),
                message: Some("The submission could not be processed.".to_string()),
                ..CommandOutcome::base("rejected")
            }),
        ),
        AuthorityError::Context(inner) => {
            tracing::error!(error = %inner, "refusing: authority resolution failed");
            unavailable("tenant_context")
        }
        AuthorityError::IdentitySecretTooShort => {
            // Construction-time only; present so the enum is exhaustive here.
            tracing::error!("refusing: identity key is shorter than 32 bytes");
            unavailable("identity_configuration")
        }
        AuthorityError::InvalidErpLedger => {
            tracing::error!("refusing: assembler supplied an unknown ERP readiness ledger");
            unavailable("erp_configuration")
        }
    }
}

/// A server-side failure the caller cannot fix: logged detail, fixed response.
pub(crate) fn unavailable(reason: &str) -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(CommandOutcome {
            reason: Some(reason.to_string()),
            // Logged, not returned: a database or identity error naming the
            // namespace, role or query is operator information, not caller
            // information.
            message: None,
            ..CommandOutcome::base("refused")
        }),
    )
}

/// 403 with a reason the caller can act on (csrf, eligibility, key reuse).
pub(crate) fn forbidden(reason: &str, message: &str) -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::FORBIDDEN,
        Json(CommandOutcome {
            reason: Some(reason.to_string()),
            message: Some(message.to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 409: the record exists but will not do what the caller asked — consumed,
/// expired, already approved, or an idempotency key reused for another
/// command.
pub(crate) fn conflict(reason: &str, message: &str) -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::CONFLICT,
        Json(CommandOutcome {
            reason: Some(reason.to_string()),
            message: Some(message.to_string()),
            ..CommandOutcome::base("refused")
        }),
    )
}

/// 404 for a receipt that does not resolve to the caller — missing, or
/// belonging to another principal. One answer: existence is not disclosed.
pub(crate) fn unknown_request() -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::NOT_FOUND,
        Json(CommandOutcome {
            reason: Some("unknown_request".to_string()),
            message: Some("No receipt matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 404 for an approval id that resolves to nothing — distinguished from the
/// 409 of an approval that exists but is spent, because the caller here is an
/// already-eligible, already-authenticated approver.
pub(crate) fn unknown_approval() -> (StatusCode, Json<CommandOutcome>) {
    (
        StatusCode::NOT_FOUND,
        Json(CommandOutcome {
            reason: Some("approval_unavailable".to_string()),
            message: Some("No pending approval matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

// --- Session resolution ------------------------------------------------------

/// Extract the session token: `Authorization: Bearer` first, then the
/// session cookie — `__Host-archava_session` when the request arrived over TLS,
/// `archava_session` when it did not (see `crate::security`). Both carry the
/// same server-issued secret; the cookie form exists for the SSR slice's
/// browser flows.
pub(crate) fn session_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(cookie_token)
}

/// Resolve the session, then — for mutations — bind it to the CSRF header.
///
/// Order: token presence → identity row → CSRF. The identity lookup is a
/// single `UPDATE ... RETURNING` that re-checks the user is active, at the
/// version the session was minted under, unexpired and seen within the idle
/// window — so a password change (which bumps `version`) and a deactivation
/// both invalidate live sessions immediately. A token the shape check rejects
/// never reaches the database at all.
pub(crate) async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
    csrf_required: bool,
) -> Result<AuthenticatedSession, (StatusCode, Json<CommandOutcome>)> {
    if let Some(result) = crate::agent::delegation::authenticate(state).await {
        return result.map_err(|_| unauthenticated());
    }
    let Some(token) = session_token(headers) else {
        return Err(unauthenticated());
    };
    match state.inner.identity.authenticate(&token).await {
        Ok(Some(session)) => {
            if csrf_required {
                let presented = headers
                    .get(CSRF_HEADER)
                    .and_then(|value| value.to_str().ok());
                if !state.inner.identity.valid_csrf(&token, presented) {
                    return Err((
                        StatusCode::FORBIDDEN,
                        Json(CommandOutcome {
                            reason: Some("csrf_required".to_string()),
                            message: Some(
                                "The CSRF header must match this session's token.".to_string(),
                            ),
                            ..CommandOutcome::base("refused")
                        }),
                    ));
                }
            }
            Ok(session)
        }
        Ok(None) => Err(unauthenticated()),
        Err(error) => {
            tracing::error!(error = %error, "identity store unavailable while authenticating");
            Err(unavailable("identity"))
        }
    }
}

/// The front gate for the route families that live in their own modules.
///
/// [`authenticate`] speaks the command surface's `CommandOutcome`; the feed,
/// preview and ingestion routes answer their own JSON shapes. This wrapper keeps
/// the same decision — and the same two stable reasons — without forcing those
/// modules to name a type they do not own. `csrf_required` is reported apart
/// from `unauthenticated` because a caller that holds a valid session but lost
/// its CSRF header needs a different fix than one that never signed in.
pub(crate) async fn require_session(
    state: &AppState,
    headers: &HeaderMap,
    csrf_required: bool,
) -> Result<(), (StatusCode, &'static str)> {
    match authenticate(state, headers, csrf_required).await {
        Ok(_) => Ok(()),
        Err((status, _)) if status == StatusCode::FORBIDDEN => Err((status, "csrf_required")),
        Err((status, _)) => Err((status, "unauthenticated")),
    }
}

// The contact-list placeholder was removed when the §1.4B slice landed the
// real, session-scoped contact read: `crate::erp::crm::contacts` serves `GET
// /contacts` now. (The refusal it used to carry assumed a caller-less read
// was possible at all; the real read is session-scoped, so the note that
// explained the reservation stays here in the router where the route lives.)
#[cfg(test)]
mod tests {
    use crate::identity::IdentityError;
    use archava_core::roles::{CapabilityTier, RoleName};
    use archava_core::tenant::TenantId;
    use archava_domain::authority::ActionRequest;

    use super::*;

    /// The commerce ledger — every tenant-scoped table the readiness gate
    /// forces, `approvals` (migration 004) included.
    fn ledger() -> Ledger {
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: &[
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
            ],
        }
    }

    /// A fixed identity key for the cold tests — 32 bytes, like production.
    fn test_identity(pool: PgPool) -> Identity {
        Identity::unchecked(pool, &[7u8; 32])
    }

    /// Drives a request through the real router.
    ///
    /// This is the difference between a policy test and a G03 test. G03 requires
    /// negative cases at real entrypoints; calling `CommandAuthorizer::evaluate`
    /// directly proves the gate works but proves nothing about the boundary a
    /// caller actually reaches. These tests send bytes over a `Request` and read
    /// the `Response` the router produces.
    ///
    /// The pool is `connect_lazy`, so no connection is ever attempted — and
    /// none is needed for most cases below, because they are refused at
    /// extraction or at token-shape level. The one case that *does* reach for
    /// the database (a well-formed token nobody can verify) asserts the
    /// refusal is an unavailability, never an `allowed`.
    async fn call(uri: &str, body: Option<&str>) -> (StatusCode, String) {
        call_with(uri, body, &[]).await
    }

    async fn call_with(
        uri: &str,
        body: Option<&str>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, String) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://archava:archava@127.0.0.1:1/archava_test")
            .expect("a lazy pool must construct without connecting");
        let state = AppState::new(pool.clone(), ledger(), test_identity(pool));
        let router = router(state);

        let mut builder = axum::http::Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(body.unwrap_or_default().to_string()).unwrap();

        use tower::ServiceExt;
        let response = router.oneshot(request).await.expect("router must answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body must be readable");
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    /// A plausible request id for bodies that never reach a database.
    const REQUEST_ID: &str = "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f";

    /// A well-formed command body for the given tenant and action. The
    /// legacy authority fields ride along as UNKNOWN fields: they must be
    /// dropped at deserialization, not honoured, and certainly not able to
    /// make the body unparseable.
    fn submission(tenant: &str, action_id: &str) -> String {
        serde_json::json!({
            "tenant": tenant,
            "action_id": action_id,
            "request_id": REQUEST_ID,
            "revision": 1,
            "role": "superadmin",
            "capability": "godmode",
            "human_approved": true,
            "enabled_action_ids": ["everything.*"],
        })
        .to_string()
    }

    // --- Extraction -----------------------------------------------------------

    /// A cross-tenant tenant id is refused during extraction, before any handler
    /// code runs. The refusal is axum's own rejection of the deserialization,
    /// mapped through [`rejection`] so nothing reflects back.
    #[tokio::test]
    async fn an_invalid_tenant_is_refused_at_the_entrypoint() {
        let (status, _) = call(
            "/commands",
            Some(&submission("../etc/passwd", "product.read")),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "a tenant that cannot be represented must be refused at the boundary"
        );
    }

    /// The refusal must not reflect the caller's input back to it.
    ///
    /// This test exists because the first version of it passed for the wrong
    /// reason. Axum's default `Json` rejection echoes the rejected value
    /// verbatim, so a hostile `tenant` came straight back in the response body —
    /// a reflection primitive on an entrypoint. The assertion is deliberately
    /// broad: the whole body, not just the tenant field, because any part of
    /// the submission appearing in a refusal is the same bug.
    #[tokio::test]
    async fn a_boundary_refusal_does_not_reflect_the_submission() {
        let (_, body) = call(
            "/commands",
            Some(&submission("../etc/passwd", "product.read")),
        )
        .await;
        for reflected in [
            "../etc/passwd",
            "superadmin",
            "godmode",
            "human_approved",
            "everything.*",
        ] {
            assert!(
                !body.contains(reflected),
                "the refusal reflected {reflected:?} back to the caller: {body}"
            );
        }
        assert!(
            !body.contains("tenant"),
            "a refusal must not name the field that was rejected: {body}"
        );
        assert!(
            body.contains("malformed_submission"),
            "the caller still needs a stable reason, got: {body}"
        );
    }

    // --- The session boundary (all cold-safe) --------------------------------

    /// No token, no command: the request never reaches the gate, the body's
    /// claims are never read, and the refusal says exactly one thing.
    #[tokio::test]
    async fn an_unauthenticated_command_is_refused() {
        let (status, body) = call(
            "/commands",
            Some(&submission("acme-retail", "product.read")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {body}");
        assert!(body.contains("unauthenticated"), "got: {body}");
        assert!(
            !body.contains("acme-retail"),
            "a refusal must not echo the requested tenant: {body}"
        );
    }

    /// A token that cannot even have been issued by this server (wrong shape)
    /// is refused before any database work — the shape check is the first
    /// thing `authenticate` does after presence.
    #[tokio::test]
    async fn a_malformed_session_token_is_refused_without_touching_the_pool() {
        let (status, body) = call_with(
            "/commands",
            Some(&submission("acme-retail", "product.read")),
            &[("authorization", "Bearer definitely-not-a-token")],
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {body}");
        assert!(body.contains("unauthenticated"), "got: {body}");
    }

    /// A well-formed token on a database nobody can reach is a refusal, never
    /// an authorization: the only outcomes are 401 (no row) and 503 (store
    /// unreachable). `allowed` is structurally out of reach without a session
    /// row that exists.
    #[tokio::test]
    async fn a_well_formed_session_that_cannot_be_verified_is_a_refusal() {
        // 43 base64url characters: passes the shape gate, reaches the pool.
        let token = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let (status, body) = call_with(
            "/commands",
            Some(&submission("acme-retail", "product.read")),
            &[("authorization", &format!("Bearer {token}"))],
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "got: {body}");
        assert!(
            !body.contains("allowed"),
            "unverifiable ≠ authorized: {body}"
        );
        assert!(
            !body.contains("acme-retail"),
            "must not echo the tenant: {body}"
        );
    }

    /// The legacy authority fields are not part of the submission: a body
    /// carrying an INVALID role and tier parses (they are dropped, not
    /// deserialized into `RoleName`/`CapabilityTier`, which would 422) and is
    /// then refused for having no session. The full forged-authority proof —
    /// those fields alongside a *real* session changing nothing — lives in
    /// `tests/authority.rs` against PostgreSQL.
    #[tokio::test]
    async fn body_fields_that_used_to_be_authority_are_dropped_not_honoured() {
        let (status, body) = call(
            "/commands",
            Some(&submission("acme-retail", "product.read")),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "unknown fields must be dropped (not 422), then the caller refused: {body}"
        );
    }

    // --- Status and approvals boundaries -------------------------------------

    /// A malformed status query is refused at the boundary without echoing
    /// the hostile parameter back.
    #[tokio::test]
    async fn a_malformed_status_query_is_refused_without_reflection() {
        let (status, body) = call(
            "/commands/status?tenant=../etc/passwd&request_id=not-a-uuid",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "got: {body}");
        assert!(!body.contains("../etc/passwd"), "reflected: {body}");
        assert!(body.contains("malformed_submission"), "got: {body}");
    }

    /// A well-formed status read without a session is 401 — recovery is
    /// authorized work, not a public index.
    #[tokio::test]
    async fn a_status_read_is_refused_without_a_session() {
        let (status, body) = call(
            "/commands/status?tenant=acme-retail&request_id=7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {body}");
        assert!(body.contains("unauthenticated"), "got: {body}");
    }

    /// Approval requests are authenticated work.
    #[tokio::test]
    async fn an_approval_request_is_refused_without_a_session() {
        let body = serde_json::json!({
            "tenant": "acme-retail",
            "action_id": "admin.config.update",
            "revision": 1,
        })
        .to_string();
        let (status, response) = call("/approvals", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
    }

    /// Ava Cockpit mission flight requests require authentication.
    #[tokio::test]
    async fn cockpit_mission_creation_is_refused_without_a_session() {
        let body = serde_json::json!({
            "tenant_id": "acme-retail",
            "mission_code": "INV-001",
            "title": "Autonomous Reconciliation",
            "description": "Invoice matching",
            "steps": [],
        })
        .to_string();
        let (status, response) = call("/api/v1/cockpit/missions", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
    }

    /// Ava Orbz crystallization requires authentication.
    #[tokio::test]
    async fn orbz_crystallize_is_refused_without_a_session() {
        let body = serde_json::json!({
            "tenant_id": "acme-retail",
            "scope": "public",
            "category": "sop",
            "resource": "refund.md",
            "summary_l0": "Refund rule",
            "overview_l1": "Refund policy details",
            "min_authority": "L1",
        })
        .to_string();
        let (status, response) = call("/api/v1/orbz/crystallize", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
    }

    /// The preset catalogue is session-scoped. It was mounted anonymously,
    /// which made a packaging catalogue readable without authority; the feed
    /// below is the same story with fabricated tenant-shaped data on top.
    #[tokio::test]
    async fn the_preset_catalogue_refuses_without_a_session() {
        let (status, response) = call("/api/v1/presets/profiles", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
        assert!(
            !response.contains("fnb_cafe"),
            "a refusal must not hand back the catalogue: {response}"
        );
    }

    /// The action feed refuses before it synthesizes anything, so an anonymous
    /// caller cannot read cards it might mistake for a tenant overview.
    #[tokio::test]
    async fn the_action_feed_refuses_without_a_session() {
        let (status, response) = call("/api/v1/feed/action-cards?profile=fnb_cafe", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
        assert!(
            !response.contains("card_ar_"),
            "a refusal must not include feed cards: {response}"
        );
    }

    /// The commit route never reports a commit. It used to answer
    /// `committed_atomic` from the caller's own preview; the only truthful
    /// answers are a refusal or "not implemented".
    #[tokio::test]
    async fn the_migration_commit_refuses_before_it_could_claim_a_commit() {
        let body = serde_json::json!({
            "batch_id": "00000000-0000-4000-8000-000000000001",
            "preview": {
                "entity_kind": "catalog_products",
                "total_rows": 1,
                "valid_rows": 1,
                "invalid_rows": 0,
                "resolved_mappings": {},
                "unmapped_columns": [],
                "sample_mapped_records": [],
                "warnings": [],
                "batch_fingerprint": "synthetic"
            }
        })
        .to_string();
        let (status, response) = call("/api/migration/commit", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(
            !response.contains("committed_atomic"),
            "an unauthorized commit must not report a commit: {response}"
        );
    }

    /// And so is stamping one — an unauthenticated caller cannot approve what
    /// it cannot even name.
    #[tokio::test]
    async fn an_approve_stamp_is_refused_without_a_session() {
        let body = serde_json::json!({
            "tenant": "acme-retail",
            "approval_id": "0193b1f0-0000-4000-8000-000000000001",
        })
        .to_string();
        let (status, response) = call("/approvals/approve", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "got: {response}");
        assert!(response.contains("unauthenticated"), "got: {response}");
    }

    /// Liveness carries no tenant and reads no tenant data. The body is
    /// `text/plain` because the handler returns a `&str` rather than JSON — the
    /// first version of this test asserted a quoted JSON string and was wrong.
    #[tokio::test]
    async fn health_answers_without_any_authority_context() {
        let (status, body) = call("/healthz", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn embed_script_answers_with_javascript_without_authority() {
        let (status, body) = call("/v1/archava.js", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Archava Universal Client Embed Script"));
        assert!(body.contains("openCheckout"));
    }

    /// Signs a checkout URL exactly the way `POST /embed/checkout/sign` does,
    /// with the same identity key the test router is built from.
    fn signed_checkout_uri(
        raw_tenant: &str,
        raw_order: &str,
        amount: u64,
        currency: &str,
        item: &str,
    ) -> String {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://archava:archava@127.0.0.1:1/archava_test")
            .expect("a lazy pool must construct without connecting");
        let identity = test_identity(pool);
        let tenant = sanitize_identifier(raw_tenant, "default");
        let order = sanitize_identifier(raw_order, "ord_pending");
        let signature =
            identity.embed_signature(&canonical_checkout(&tenant, &order, amount, currency, item));
        format!(
            "/embed/checkout?tenant={tenant}&orderId={order}&amount={amount}&currency={currency}&item={}&sig={signature}",
            percent_encode(item)
        )
    }

    /// The checkout document renders only for a signed URL. An unsigned request
    /// is the caller naming its own tenant, order and amount, which is exactly
    /// the page the audit found framed on merchant sites.
    #[tokio::test]
    async fn embed_checkout_requires_a_signature_before_it_renders() {
        let (status, body) = call(
            "/embed/checkout?orderId=ord_test_99&amount=150000&tenant=tenant_acme",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, r#"{"error":"checkout_signature_required"}"#);

        let signed = signed_checkout_uri(
            "tenant_acme",
            "ord_test_99",
            150_000,
            "IDR",
            "Archava Verified Checkout",
        );
        let (status, body) = call(&signed, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("Archava Checkout - ord_test_99"));
        assert!(body.contains("tenant_acme"));
        assert!(body.contains("150000"));
        assert!(body.contains("Payment provider unavailable"));
        assert!(!body.contains("ARCHAVA_PAYMENT_SIMULATED"));
    }

    /// The signature is minted only for a session holding the CSRF header.
    #[tokio::test]
    async fn the_embed_signature_mint_requires_a_session() {
        let (status, body) = call("/embed/checkout/sign", Some("{}")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("unauthenticated"));
    }

    /// Even a signed document cannot smuggle markup: identifiers are still
    /// sanitized and the item name is still HTML-escaped, because the signature
    /// binds the sanitised values rather than the raw query.
    #[tokio::test]
    async fn a_signed_checkout_still_neutralizes_xss_injection_attempts() {
        let uri = signed_checkout_uri(
            "acme<script>",
            "\";alert(1);//",
            0,
            "IDR",
            "<img src=x onerror=alert(1)>",
        );
        let (status, body) = call(&uri, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!body.contains("<img src=x onerror=alert(1)>"));
        assert!(body.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(!body.contains("\";alert(1);//"));
        assert!(!body.contains("acme<script>"));
        assert!(body.contains("Tenant: acmescript"));
    }

    /// Readiness does not lie when the pool is unreachable: the lazy test pool
    /// points at a closed port, so the probe must answer 503 rather than
    /// inheriting liveness's unconditional `ok`.
    #[tokio::test]
    async fn readiness_reports_an_unreachable_database() {
        let (status, body) = call("/readyz", None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, "not ready");
    }

    /// Metrics is no longer an anonymous database probe: a scrape with no
    /// credential is refused before `database_ready` runs.
    #[tokio::test]
    async fn metrics_refuses_a_scrape_without_a_credential() {
        let (status, body) = call("/metrics", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, "metrics_forbidden");
    }

    /// The gate itself. Loopback is trusted without a token; every other peer
    /// needs one; and an absent peer (an embedder without `ConnectInfo`) is NOT
    /// trusted — the fallback here would be an open door, not a coarse bucket.
    #[test]
    fn metrics_authorization_prefers_loopback_and_otherwise_requires_the_token() {
        let loopback = LoginSource {
            bucket: "peer:127.0.0.1".to_string(),
            peer: Some("127.0.0.1".parse().expect("an address")),
        };
        let public = LoginSource {
            bucket: "peer:203.0.113.9".to_string(),
            peer: Some("203.0.113.9".parse().expect("an address")),
        };
        let no_peer = LoginSource {
            bucket: "no-connect-info".to_string(),
            peer: None,
        };
        let mut headers = HeaderMap::new();

        assert!(metrics_authorized(None, &loopback, &headers));
        assert!(!metrics_authorized(None, &public, &headers));
        assert!(!metrics_authorized(None, &no_peer, &headers));

        headers.insert(
            "x-archava-metrics-token",
            "scrape-secret".parse().expect("a header value"),
        );
        assert!(metrics_authorized(Some("scrape-secret"), &public, &headers));
        assert!(!metrics_authorized(Some("other-secret"), &public, &headers));
        assert!(!metrics_authorized(None, &public, &headers));
    }

    /// The contact list refuses rather than returning an empty list, because an
    /// empty list would read as "this tenant has no contacts" when the truth is
    /// "this caller has no tenant".
    ///
    /// The refusal is the identity-first one every other tenant route answers
    /// to a caller with no session — 401 `unauthenticated`, the same shape the
    /// status/approval tests above pin. The membership refusals (403
    /// `role_not_allowed` / `missing_membership`) need a real session to be
    /// meaningful, and they are pinned where one exists:
    /// `tests/crm.rs::a_principal_without_a_crm_membership_is_refused` holds a
    /// session and is refused on list and command alike.
    #[tokio::test]
    async fn the_contact_list_refuses_rather_than_faking_an_empty_result() {
        // The tenant parameter is part of the route's contract (the same shape
        // `tests/crm.rs::an_unauthenticated_contact_read_is_refused` sends), so
        // what this call exercises is the refusal after a well-formed request,
        // not the 400 a missing tenant earns.
        let (status, body) = call("/contacts?tenant=acme-retail", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            body.contains("unauthenticated"),
            "expected an explicit refusal, got: {body}"
        );
    }

    // --- Decision mapping (pure) ---------------------------------------------

    fn decision(action_id: &str, role: RoleName, capability: CapabilityTier) -> PolicyDecision {
        let request = ActionRequest {
            tenant: TenantId::new("acme-retail").expect("fixture tenant"),
            role,
            capability,
            action_id: action_id.to_string(),
            available_on_page: None,
            confirmed: None,
            declined: None,
            human_approved: None,
            enabled_action_ids: None,
        };
        CommandAuthorizer::new().evaluate(&request)
    }

    /// An unknown service is refused with a 404, not a 500: the request was
    /// understood, and what it asked for does not exist.
    #[test]
    fn an_unknown_action_maps_to_not_found() {
        let outcome = decision(
            "billing.void_everything",
            RoleName::Owner,
            CapabilityTier::Enterprise,
        );
        assert_eq!(outcome.denial_reason(), Some(ActionDenial::UnknownAction));
        assert_eq!(status_for(&outcome), StatusCode::NOT_FOUND);
    }

    /// A missing actor is a refusal, not an anonymous success.
    #[test]
    fn a_wrong_role_maps_to_forbidden() {
        let outcome = decision("product.read", RoleName::Viewer, CapabilityTier::Assist);
        assert_eq!(outcome.denial_reason(), Some(ActionDenial::RoleNotAllowed));
        assert_eq!(status_for(&outcome), StatusCode::FORBIDDEN);
    }

    /// An under-capability grant is refused.
    #[test]
    fn an_under_capability_grant_maps_to_forbidden() {
        let outcome = decision(
            "payment.initiate",
            RoleName::ArchavaAssistant,
            CapabilityTier::Act,
        );
        assert_eq!(
            outcome.denial_reason(),
            Some(ActionDenial::CapabilityInsufficient)
        );
        assert_eq!(status_for(&outcome), StatusCode::FORBIDDEN);
    }

    /// A confirmation the visitor declined earlier in the session is refused.
    /// `declined` is a session-level signal, deliberately not a grant
    /// revocation: revocation is resolved from current server-side state.
    #[test]
    fn a_declined_confirmation_maps_to_forbidden() {
        let request = ActionRequest {
            tenant: TenantId::new("acme-retail").expect("fixture tenant"),
            role: RoleName::ArchavaAssistant,
            capability: CapabilityTier::Act,
            action_id: "lead.capture".to_string(),
            available_on_page: None,
            confirmed: None,
            declined: Some(true),
            human_approved: None,
            enabled_action_ids: None,
        };
        let outcome = CommandAuthorizer::new().evaluate(&request);
        assert_eq!(
            outcome.denial_reason(),
            Some(ActionDenial::DeclinedByVisitor)
        );
        assert_eq!(status_for(&outcome), StatusCode::FORBIDDEN);
    }

    /// An L5 action without human approval is refused even for an owner.
    #[test]
    fn an_l5_action_without_human_approval_maps_to_forbidden() {
        let outcome = decision(
            "admin.config.update",
            RoleName::Owner,
            CapabilityTier::Enterprise,
        );
        assert_eq!(
            outcome.denial_reason(),
            Some(ActionDenial::AdminActionRequiresHuman)
        );
        assert_eq!(status_for(&outcome), StatusCode::FORBIDDEN);
    }

    /// A confirmation question is not a refusal, and it must not be reported as
    /// one: the caller is expected to ask again, not to treat it as an error.
    ///
    /// `form.submit` is the baseline's first `user_confirm` action
    /// (`packages/acl/src/actions.ts:246-258`). Using an ungated action here would
    /// have produced a `200` and the test would have passed while asserting the
    /// wrong thing — which is exactly how it first came out.
    #[test]
    fn a_confirmation_question_is_not_a_refusal() {
        let outcome = decision(
            "form.submit",
            RoleName::ArchavaAssistant,
            CapabilityTier::Act,
        );
        assert_eq!(outcome.denial_reason(), None);
        assert_eq!(status_for(&outcome), StatusCode::ACCEPTED);
        assert!(outcome.message().is_none());
        // The prompt must survive the mapping to an outcome: an accepted
        // response that carried no question would leave the caller with
        // nothing to answer.
        assert!(CommandOutcome::from(&outcome).prompt.is_some());
    }

    /// An allowed command is a 200.
    #[test]
    fn an_allowed_command_maps_to_ok() {
        let outcome = decision(
            "product.read",
            RoleName::ArchavaAssistant,
            CapabilityTier::Assist,
        );
        assert_eq!(outcome.denial_reason(), None);
        assert_eq!(status_for(&outcome), StatusCode::OK);
    }

    // --- The tenant boundary --------------------------------------------------

    /// The tenant boundary is enforced by the type, not by a handler check: an
    /// invalid tenant cannot be deserialized into a `TenantId` at all. This is
    /// the G03 tenant-scope negative test, and it passes before any route code
    /// runs.
    #[test]
    fn an_invalid_tenant_cannot_be_represented() {
        let parsed: Result<TenantId, _> =
            serde_json::from_value(serde_json::json!("../etc/passwd"));
        assert!(
            parsed.is_err(),
            "an unvalidated tenant string must not deserialize into a TenantId"
        );
    }

    /// A refusal must not echo the tenant back: confirming a tenant's existence
    /// to a caller who was not entitled to it is itself a disclosure.
    #[test]
    fn a_refusal_never_echoes_the_tenant() {
        let outcome = CommandOutcome::from(&decision(
            "billing.void_everything",
            RoleName::Owner,
            CapabilityTier::Enterprise,
        ));
        assert!(outcome.tenant.is_none());
    }

    // --- Ledger declaration ---------------------------------------------------

    /// The ledger's table list is what the forced-policy gate compares against.
    /// A ledger declared with no tables would make that gate vacuous — it would
    /// compare 0 against 0 and pass. Migration 004 added `approvals` to the
    /// tenant-scoped set, so it must appear here too or its forced policy goes
    /// unchecked.
    #[test]
    fn a_ledger_must_declare_its_tenant_scoped_tables() {
        let ledger = ledger();
        assert_eq!(
            ledger.tables.len(),
            11,
            "the commerce ledger's forced-policy gate expects 11 tables \
             (the 10 of migration 001 plus approvals)"
        );
        assert!(
            ledger.tables.contains(&"approvals"),
            "the bound-approval table must be under the forced-policy gate"
        );
    }

    // --- The login refusal mapping (RN07/W-PW06) ------------------------------

    /// The boundary between "the password was wrong" and "the server is
    /// overloaded" is load-bearing: a caller that receives 401 for a CORRECT
    /// password retries as if credentials were wrong, burning the account's
    /// rate budget, and cannot tell its monitoring "attack" from
    /// "capacity". Each identity failure must produce its OWN status.
    #[test]
    fn each_identity_failure_produces_its_own_login_status() {
        let (status, _) = refused_login_outcome(&IdentityError::InvalidCredentials);
        assert_eq!(status, StatusCode::UNAUTHORIZED, "bad credentials -> 401");
        let (status, _) = refused_login_outcome(&IdentityError::RateLimited);
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "over-bucket -> 429");
    }

    /// The heart of RN07: overload is NEVER a credential answer. A correct
    /// password that arrived while the derivation budget was full must be
    /// answerable as retryable load — the 503-with-`busy` shape — not as
    /// "invalid credentials" (the leak `verify_password`'s old signature
    /// produced, folding `Err(Busy)` into a boolean).
    #[test]
    fn an_overloaded_login_is_busy_never_invalid_credentials() {
        let (status, outcome) = refused_login_outcome(&IdentityError::Busy);
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "the overload answer must be retryable, not credential-shaped"
        );
        assert_eq!(
            outcome.reason.as_deref(),
            Some("busy"),
            "the client distinguishes busy from every other refusal by this reason"
        );
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "a busy server must not claim the password was wrong"
        );
    }

    /// A store failure keeps hiding its mechanism: the response says
    /// "unavailable" with no query, constraint or driver text (D09), while
    /// the operator gets the detail in the log (the `From<sqlx::Error>`
    /// boundary logs it).
    #[test]
    fn a_store_failure_names_no_mechanism() {
        let (status, outcome) = refused_login_outcome(&IdentityError::Unavailable);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(outcome.reason.as_deref(), Some("identity"));
        assert!(
            outcome.message.is_none(),
            "the response carries no diagnosis text: {outcome:?}"
        );
    }
}
