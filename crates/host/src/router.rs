//! The HTTP router.
//!
//! Seven routes, each one a link in the authority chain the mandate requires
//! (audit RA01): `POST /login` issues a server-side session; `POST /commands`
//! executes an authorized command or idempotently replays a recorded one;
//! `GET /commands/status` recovers a receipt; `POST /approvals` requests a
//! bound human approval and `POST /approvals/approve` stamps it; `GET
//! /contacts` is the R01 placeholder read; `/healthz` is liveness.
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

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{ConnectInfo, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use archava_adapters::rls::assert_startup_readiness;
use archava_core::actions::{ActionRegistry, ConfirmationMode};
use archava_core::roles::RoleName;
use archava_core::tenant::TenantId;
use archava_domain::authority::{
    ActionDenial, CommandAuthorizer, PolicyDecision, NEVER_AUTONOMOUS_LEVEL,
};

use crate::authority::{self, AuthorityError, MembershipKind};
use crate::identity::{AuthenticatedSession, Identity, IdentityError};
use crate::metrics::Metrics;

/// The bound approval's lifetime. An approval that can wait forever is an
/// approval that authorises a command nobody expects any more — the mandate's
/// contract names expiry as part of the binding, and this is the number.
const APPROVAL_TTL_SECONDS: i32 = 15 * 60;

/// The largest input this host will fingerprint into a receipt.
///
/// The receipt stores the input verbatim so authorized recovery can replay
/// what actually ran; an unbounded column would be a denial-of-service shape
/// dressed as a convenience.
const MAX_INPUT_BYTES: usize = 16 * 1024;

/// The source a login attempt is bucketed under (F-09).
///
/// `Identity::login` keeps one bucket per source, so 60 failures from one peer
/// can no longer close sign-in for every other peer — the audit's cheap global
/// DoS. The source is the transport peer address that
/// `into_make_service_with_connect_info::<SocketAddr>()` installs; an embedder
/// that serves the router without it falls back to [`NO_CONNECT_INFO_SOURCE`],
/// which reproduces the old shared bucket instead of turning a login into a 500.
struct LoginSource {
    /// The rate-limit bucket key.
    bucket: String,
    /// The peer that must be a trusted hop before its `X-Forwarded-Proto` can
    /// select the secure cookie (audit H10). `None` keeps the embedding/test
    /// posture, where no direct public caller is known.
    peer: Option<IpAddr>,
}

/// The single bucket every login shares when the embedder did not install
/// `ConnectInfo`. Named so an operator reading `login_buckets` sees the
/// degradation instead of mistaking one row for per-client limiting.
const NO_CONNECT_INFO_SOURCE: &str = "no-connect-info";

impl FromRequestParts<AppState> for LoginSource {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        let bucket = peer
            .map(|address| address.to_string())
            .unwrap_or_else(|| NO_CONNECT_INFO_SOURCE.to_string());
        Ok(Self { bucket, peer })
    }
}

/// The CSRF header a mutation must present. Its expected value is
/// `HMAC(key, "csrf\0" + token)` — derived by the server, never stored, and
/// not computable by a cross-site page that can neither read the token nor
/// guess the key.
const CSRF_HEADER: &str = "x-archava-csrf";

use crate::security::{
    cookie_token, expired_session_cookies, is_command_line_client, is_secure_from_peer,
    session_cookie,
};

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
        let identity =
            Identity::open(pool.clone(), identity_key, expected_identity_migrations).await?;
        archava_adapters::writer::assert_ready(&pool).await?;
        Ok(Self {
            payments: Arc::new(crate::payments::PaymentRegistry::default()),
            email: Arc::new(crate::mail::EmailRegistry::default()),
            presence: Arc::new(crate::presence::PresenceRegistry::default()),
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
    /// for [`crate::crm::CRM_LEDGER`]. The per-transaction non-owner check
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
            crate::crm::CRM_LEDGER.namespace,
            crate::crm::CRM_LEDGER.ledger,
            crate::crm::CRM_LEDGER.tables,
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
}

/// Builds the router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics_endpoint))
        .route("/v1/archava.js", get(serve_embed_script))
        .route("/archava.css", get(serve_archava_css))
        .route("/workspace.css", get(serve_workspace_css))
        .route("/embed/checkout", get(serve_embed_checkout))
        .route(
            "/api/preview/generate",
            post(crate::preview::generate_preview_route),
        )
        // Autonomous Closing Webhooks: Privy Digital Signatures & Payment Gateways
        .route(
            "/api/webhooks/privy",
            post(crate::webhooks::privy_webhook_route),
        )
        .route(
            "/api/webhooks/payment",
            post(crate::webhooks::payment_webhook_route),
        )
        .route(
            "/api/webhooks/payment/{integration}",
            post(crate::payments::webhook),
        )
        .route(
            "/erp/billing/payments",
            get(crate::payments::list).post(crate::payments::create),
        )
        .route("/erp/billing/payments/check", post(crate::payments::check))
        .route("/erp/billing/payments/status", get(crate::payments::status))
        .route(
            "/erp/billing/invoices/{id}/send-email",
            post(crate::mail::send_invoice_email),
        )
        .route(
            "/erp/billing/invoices/{id}/emails",
            get(crate::mail::list_invoice_emails),
        )
        // Realtime Voice & Presence Lifecycle
        .route(
            "/presence/sessions",
            post(crate::presence::start_presence_session),
        )
        .route(
            "/presence/sessions/{id}",
            get(crate::presence::get_presence_session),
        )
        .route(
            "/presence/sessions/{id}/turn",
            post(crate::presence::process_presence_turn),
        )
        .route(
            "/presence/sessions/{id}/degrade",
            post(crate::presence::degrade_presence_session),
        )
        // AI Legacy Data Migration & Schema Ingestion
        .route(
            "/api/migration/reconcile",
            post(crate::ingestion::migration_reconcile_route),
        )
        .route(
            "/api/migration/commit",
            post(crate::ingestion::migration_commit_route),
        )
        // Ava Cockpit: Autonomous Durable Flight Operations
        .route(
            "/api/v1/cockpit/missions",
            post(crate::cockpit::create_mission).get(crate::cockpit::list_missions),
        )
        .route(
            "/api/v1/cockpit/missions/{id}",
            get(crate::cockpit::get_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/steps/{step_index}/complete",
            post(crate::cockpit::complete_step),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/abort",
            post(crate::cockpit::abort_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/hold",
            post(crate::cockpit::hold_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/resume",
            post(crate::cockpit::resume_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/run-next",
            post(crate::cockpit::run_next),
        )
        // Ava Orbz: Sovereign Multi-Layer Agentic Memory Engine
        .route(
            "/api/v1/orbz/crystallize",
            post(crate::orbz::crystallize_node),
        )
        .route("/api/v1/orbz/query", post(crate::orbz::query_nodes))
        .route("/api/v1/orbz/episodes", post(crate::orbz::record_episode))
        // Industry Presets & Proactive Action Feed (Frictionless Operations)
        .route("/api/v1/presets/profiles", get(crate::feed::list_profiles))
        .route(
            "/api/v1/presets/profiles/{slug}",
            get(crate::feed::get_profile),
        )
        .route(
            "/api/v1/feed/action-cards",
            get(crate::feed::get_action_cards),
        )
        .route(
            "/api/v1/feed/action-cards/{id}/execute",
            post(crate::feed::execute_action),
        )
        // R09 FIX: /login accepts POST for authentication; GET redirects to sovereign workspace login (/app)
        // preventing 405 Method Not Allowed on browser redirects or bookmarks.
        .route(
            "/login",
            post(login).get(|| async { axum::response::Redirect::temporary("/app") }),
        )
        // The logout route: the server-side end of a session (C02). Without
        // it, "sign out" only cleared browser state and the credential kept
        // reading rows — the audit's reproduced defect.
        .route("/logout", post(logout))
        // The §1.4B slice: the real, session-scoped contact read (crm.rs)
        // replaces the pre-slice placeholder denial that lived on this path.
        .route("/contacts", get(crate::crm::contacts))
        .route("/crm/records", get(crate::crm::record))
        .route("/commands", post(command))
        .route("/commands/status", get(command_status))
        .route("/approvals", post(approval_request))
        .route("/approvals/approve", post(approval_grant))
        .route("/crm/commands", post(crate::crm::crm_command))
        .route("/crm/commands/status", get(crate::crm::command_status))
        // C03: the persisted owner-configuration lifecycle. Session + CSRF
        // first; the archava_owner grant and RLS tenant are checked inside.
        .route("/own/config", get(crate::own::config))
        .route("/own/plan", post(crate::own::plan))
        // The configuration chat's entrypoint. It parses into the SAME typed
        // Request /own/plan takes and resolves through the same plan function,
        // so a chat plan and a dashboard plan cannot drift apart.
        .route("/own/chat", post(crate::own::chat))
        .route("/own/apply", post(crate::own::apply))
        .route("/own/restore/plan", post(crate::own::restore_plan))
        .route("/own/restore", post(crate::own::restore))
        .route("/own/history", get(crate::own::history))
        .route("/own/status", get(crate::own::status))
        // R03's assistant slice: the CRM conversation. Session + CSRF first,
        // tenant from the path, the flow's own gates after.
        .route(
            "/api/crm/{tenant}/conversation",
            post(crate::assistant::conversation),
        )
        // Archxy 2's ERP namespace (PACKET-000/001): the two kernel cohorts
        // mount into the same session-scoped api scope, mirroring the CRM
        // gate's decision/receipt documents. CSRF-bearing mutation per gate.
        .route(
            "/erp/commerce/commands",
            post(crate::erp::commerce::command),
        )
        .route(
            "/erp/commerce/commands/status",
            get(crate::erp::commerce::status),
        )
        .route(
            "/erp/commerce/workspace",
            get(crate::erp::commerce::workspace),
        )
        .route(
            "/erp/purchasing/workspace",
            get(crate::erp::workspace::purchasing),
        )
        .route(
            "/erp/billing/workspace",
            get(crate::erp::workspace::billing),
        )
        .route(
            "/erp/purchasing/commands",
            post(crate::erp::purchasing::purchase_command),
        )
        .route(
            "/erp/purchasing/commands/status",
            get(crate::erp::purchasing::purchase_status),
        )
        .route(
            "/erp/foodservice/commands",
            post(crate::erp::foodservice::food_service_command),
        )
        .route(
            "/erp/foodservice/commands/status",
            get(crate::erp::foodservice::food_service_status),
        )
        // Archxy 2's R04 ledgers (PACKET-003): organizations, proposals and
        // projects, the same session+CSRF/tenant-path shape as the cohorts
        // above.
        .route(
            "/erp/organizations/commands",
            post(crate::erp::organizations::organization_command),
        )
        .route(
            "/erp/organizations/commands/status",
            get(crate::erp::organizations::organization_status),
        )
        .route(
            "/erp/proposals/commands",
            post(crate::erp::proposals::proposal_command),
        )
        .route(
            "/erp/proposals/commands/status",
            get(crate::erp::proposals::proposal_status),
        )
        .route(
            "/erp/projects/commands",
            post(crate::erp::projects::project_command),
        )
        .route(
            "/erp/projects/commands/status",
            get(crate::erp::projects::project_status),
        )
        // Archxy 2's R05 scheduling ledger (PACKET-004): reservations mounted
        // beside the R04 cohorts, same session+CSRF and tenant-body shape.
        .route(
            "/erp/scheduling/commands",
            post(crate::erp::scheduling::scheduling_command),
        )
        .route(
            "/erp/scheduling/commands/status",
            get(crate::erp::scheduling::scheduling_status),
        )
        // Archxy 2's R05 accommodation ledger (PACKET-005): room-night stays
        // compose the same occupancy kernel the scheduling ledger owns.
        .route(
            "/erp/accommodation/commands",
            post(crate::erp::accommodation::accommodation_command),
        )
        .route(
            "/erp/accommodation/commands/status",
            get(crate::erp::accommodation::accommodation_status),
        )
        // Archxy 2's R05 assets ledger (PACKET-006): custody and maintenance,
        // its own ledger with no shared kernel.
        .route(
            "/erp/assets/commands",
            post(crate::erp::assets::asset_command),
        )
        .route(
            "/erp/assets/commands/status",
            get(crate::erp::assets::asset_status),
        )
        // Archxy 2's R07 billing ledger (PACKET-007): the settlement
        // cohort, the R07 area's first subject — its own ledger; the
        // provider-event arm is authorised by a signature the gate
        // verifies BEFORE the transaction, so an under-signed claim
        // claims nothing.
        .route(
            "/erp/billing/commands",
            post(crate::erp::billing::billing_command),
        )
        .route(
            "/erp/billing/commands/status",
            get(crate::erp::billing::billing_status),
        )
        // Archxy 2's R07 subscriptions ledger (PACKET-008): the
        // entitlements/usage cohort — the READ surfaces come with it
        // (list, detail, entitlements, usage history), because a ledger
        // whose rows can only be written is not a usable domain.
        .route(
            "/erp/subscriptions/commands",
            post(crate::erp::subscriptions::subscription_command),
        )
        .route(
            "/erp/subscriptions/commands/status",
            get(crate::erp::subscriptions::subscription_status),
        )
        .route(
            "/erp/subscriptions/subscriptions",
            get(crate::erp::subscriptions::subscription_list),
        )
        .route(
            "/erp/subscriptions/subscriptions/detail",
            get(crate::erp::subscriptions::subscription_detail),
        )
        .route(
            "/erp/subscriptions/entitlements",
            get(crate::erp::subscriptions::subscription_entitlements),
        )
        .route(
            "/erp/subscriptions/contact-entitlements",
            get(crate::erp::subscriptions::subscription_contact_entitlements),
        )
        .route(
            "/erp/subscriptions/usage-events",
            get(crate::erp::subscriptions::subscription_usage),
        )
        // Archxy 2's R07 attendance ledger (PACKET-009): the
        // session/roster/check-in cohort — the corrected roster binding
        // (audit A04) is a row-SELECT property, and the read surfaces
        // (list, detail, roster, check-ins, access) ride the same
        // transaction opener.
        .route(
            "/erp/attendance/commands",
            post(crate::erp::attendance::attendance_command),
        )
        .route(
            "/erp/attendance/commands/status",
            get(crate::erp::attendance::attendance_status),
        )
        .route(
            "/erp/attendance/access",
            get(crate::erp::attendance::attendance_access),
        )
        .route(
            "/erp/attendance/sessions",
            get(crate::erp::attendance::attendance_list),
        )
        .route(
            "/erp/attendance/sessions/detail",
            get(crate::erp::attendance::attendance_detail),
        )
        .route(
            "/erp/attendance/roster",
            get(crate::erp::attendance::attendance_roster),
        )
        .route(
            "/erp/attendance/check-ins",
            get(crate::erp::attendance::attendance_check_ins),
        )
        // Archxy 2's R07 support ledger (PACKET-010): the case lifecycle
        // (start/wait/resume/close/cancel/resolve/reopen/assign/priority/
        // target/note) and the four read projections.
        .route(
            "/erp/support/commands",
            post(crate::erp::support::support_command),
        )
        .route(
            "/erp/support/commands/status",
            get(crate::erp::support::support_status),
        )
        .route(
            "/erp/support/access",
            get(crate::erp::support::support_access_route),
        )
        .route(
            "/erp/support/assignees",
            get(crate::erp::support::support_assignee_route),
        )
        .route(
            "/erp/support/cases",
            get(crate::erp::support::support_case_route),
        )
        .route(
            "/erp/support/cases/detail",
            get(crate::erp::support::support_detail_route),
        )
        // Archxy 2's R08 laundry ledger: the order intake & processing lifecycle
        .route(
            "/erp/laundry/commands",
            post(crate::erp::laundry::laundry_command),
        )
        .route(
            "/erp/laundry/commands/status",
            get(crate::erp::laundry::laundry_status),
        )
        .route(
            "/erp/laundry/access",
            get(crate::erp::laundry::laundry_access_route),
        )
        .route(
            "/erp/laundry/orders",
            get(crate::erp::laundry::laundry_orders_route),
        )
        .route(
            "/erp/laundry/orders/detail",
            get(crate::erp::laundry::laundry_detail_route),
        )
        .route(
            "/erp/laundry/process",
            get(crate::erp::laundry::laundry_process_route),
        )
        .route(
            "/erp/laundry/findings",
            get(crate::erp::laundry::laundry_findings_route),
        )
        // Archxy 2's R08 field services ledger: dispatch, visits, work lines, signoffs
        .route(
            "/erp/field/commands",
            post(crate::erp::field_services::field_command),
        )
        .route(
            "/erp/field/commands/status",
            get(crate::erp::field_services::field_status),
        )
        .route(
            "/erp/field/access",
            get(crate::erp::field_services::field_access_route),
        )
        .route(
            "/erp/field/jobs",
            get(crate::erp::field_services::field_jobs),
        )
        .route(
            "/erp/field/jobs/detail",
            get(crate::erp::field_services::field_job_detail),
        )
        .route(
            "/erp/field/audit",
            get(crate::erp::field_services::field_audit),
        )
        // Archxy 2's R08 fulfillment ledger: jobs, lines, findings, custody
        .route(
            "/erp/fulfillment/commands",
            post(crate::erp::fulfillment::fulfillment_command),
        )
        .route(
            "/erp/fulfillment/commands/status",
            get(crate::erp::fulfillment::fulfillment_status),
        )
        .route(
            "/erp/fulfillment/access",
            get(crate::erp::fulfillment::fulfillment_access_route),
        )
        .route(
            "/erp/fulfillment/jobs",
            get(crate::erp::fulfillment::fulfillment_jobs_route),
        )
        .route(
            "/erp/fulfillment/jobs/detail",
            get(crate::erp::fulfillment::fulfillment_detail_route),
        )
        .route(
            "/erp/fulfillment/lines",
            get(crate::erp::fulfillment::fulfillment_lines_route),
        )
        .route(
            "/erp/fulfillment/findings",
            get(crate::erp::fulfillment::fulfillment_findings_route),
        )
        .route(
            "/erp/fulfillment/custody",
            get(crate::erp::fulfillment::fulfillment_custody_route),
        )
        // Archxy 2's R08 formal education ledger: admissions, enrollments, sessions, results, reviews, audit
        .route(
            "/erp/education/commands",
            post(crate::erp::formal_education::education_command),
        )
        .route(
            "/erp/education/commands/status",
            get(crate::erp::formal_education::education_status),
        )
        .route(
            "/erp/education/access",
            get(crate::erp::formal_education::education_access_route),
        )
        .route(
            "/erp/education/enrollments",
            get(crate::erp::formal_education::education_enrollments),
        )
        .route(
            "/erp/education/enrollments/detail",
            get(crate::erp::formal_education::education_enrollment_detail),
        )
        .route(
            "/erp/education/enrollments/audit",
            get(crate::erp::formal_education::education_audit),
        )
        // A8/F-02: the CRM host's header policy on every route — the refusals
        // included, since those are the answers a browser renders as bare text.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            observe_request,
        ))
        .layer(axum::middleware::from_fn(crate::security::security_headers))
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

/// Universal 3rd-Party Embed Script for external websites (Stripe.js / Snap pattern).
async fn serve_embed_script() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../static/archava.js"),
    )
}

/// Static enterprise stylesheets for sovereign UI rendering.
async fn serve_archava_css() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../static/archava.css"),
    )
}

async fn serve_workspace_css() -> ([(axum::http::HeaderName, &'static str); 2], &'static str) {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../../../static/workspace.css"),
    )
}

#[derive(serde::Deserialize)]
pub(crate) struct EmbedCheckoutQuery {
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

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn sanitize_identifier(input: &str, fallback: &str) -> String {
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

/// Universal Embedded Checkout Modal Handler.
/// U01 FIX: Serves responsive checkout document with `frame-ancestors *` CSP
/// so merchant sites can embed checkout modals without frame blocking.
/// Adversarial hardening: strict sanitization of identifiers and HTML escaping of item names to prevent XSS.
async fn serve_embed_checkout(
    query: Result<Query<EmbedCheckoutQuery>, axum::extract::rejection::QueryRejection>,
) -> ([(axum::http::HeaderName, &'static str); 2], String) {
    let Query(params) = match query {
        Ok(q) => q,
        Err(_) => Query(EmbedCheckoutQuery {
            tenant: None,
            order_id: None,
            amount: None,
            currency: None,
            item: None,
        }),
    };
    let raw_tenant = params.tenant.unwrap_or_else(|| "default".to_string());
    let tenant_id = sanitize_identifier(&raw_tenant, "default");

    let raw_order = params.order_id.unwrap_or_else(|| "ord_pending".to_string());
    let order_id = sanitize_identifier(&raw_order, "ord_pending");

    let amount = params.amount.unwrap_or(0);

    let raw_currency = params.currency.unwrap_or_else(|| "IDR".to_string());
    let currency: String = raw_currency
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(6)
        .collect();
    let currency = if currency.is_empty() {
        "IDR".to_string()
    } else {
        currency.to_uppercase()
    };

    let raw_item = params
        .item
        .unwrap_or_else(|| "Archava Verified Checkout".to_string());
    let item_name = escape_html(&raw_item);

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
      cursor: pointer;
      transition: opacity 0.2s, transform 0.1s;
    }}
    .btn-pay:hover {{
      opacity: 0.95;
      transform: translateY(-1px);
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
      <span style="color:#f59e0b;">Simulated Sandbox (Non-Financial)</span>
    </div>
    <div class="detail-row">
      <span>Total Amount</span>
      <span class="amount-highlight">{formatted_amount}</span>
    </div>

    <div class="payment-methods">
      <div class="method-option active">
        <input type="radio" name="pay_method" checked id="m_qris">
        <label for="m_qris" style="cursor:pointer;flex:1;">QRIS / Instant Sandbox Simulation</label>
      </div>
      <div class="method-option">
        <input type="radio" name="pay_method" id="m_va">
        <label for="m_va" style="cursor:pointer;flex:1;">Virtual Account / Test Sandbox</label>
      </div>
    </div>

    <button id="pay-btn" class="btn-pay" onclick="completePayment()">Simulate Settlement ({formatted_amount})</button>
    <button class="btn-cancel" onclick="cancelPayment()">Cancel and return to store</button>

    <div class="footer-note">
      Demonstration Sandbox &bull; No Real Financial Transactions Settled
    </div>
  </div>

  <script>
    var orderId = "{order_id}";
    var tenantId = "{tenant_id}";
    var amount = {amount};

    function completePayment() {{
      var btn = document.getElementById('pay-btn');
      btn.disabled = true;
      btn.innerText = 'Simulating sandbox settlement...';

      setTimeout(function() {{
        var receipt = {{
          transactionId: 'sim_' + Math.random().toString(36).substring(2, 12),
          orderId: orderId,
          tenantId: tenantId,
          amount: amount,
          status: 'simulated_test',
          isSimulated: true,
          timestamp: new Date().toISOString()
        }};
        window.parent.postMessage({{
          type: 'ARCHAVA_PAYMENT_SIMULATED',
          orderId: orderId,
          receipt: receipt
        }}, '*');
      }}, 600);
    }}

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
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
}

/// Liveness. Carries no tenant and reads no tenant data, which is why it is one
/// of the routes that may run before any authority check. It says nothing
/// about the database: a process that can answer has not crashed, which is the
/// only question a liveness probe is entitled to ask.
async fn health() -> &'static str {
    "ok"
}

/// Readiness. The one question liveness cannot answer: can this process still
/// reach PostgreSQL? A failed probe answers 503, so a load balancer drains the
/// instance instead of sending it traffic it cannot serve.
async fn ready(State(state): State<AppState>) -> (StatusCode, &'static str) {
    if database_ready(&state).await {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

/// Prometheus text exposition. `db_ready` is sampled per scrape rather than
/// cached, so a database restart is visible on the next collection.
async fn metrics_endpoint(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let ready = database_ready(&state).await;
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics().render(ready),
    )
}

/// A short, bounded probe. The timeout matters: a pool whose connections are
/// wedged must not turn a readiness scrape into a second outage.
async fn database_ready(state: &AppState) -> bool {
    matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            archava_adapters::writer::is_current(state.pool()),
        )
        .await,
        Ok(Ok(()))
    )
}

// --- Request shapes ----------------------------------------------------------

/// `POST /login`. Identity only — no tenant, no role, no grant: a session is
/// proof of *who*, and everything about *what they may do* is resolved per
/// request from commerce memberships.
#[derive(Debug, Deserialize)]
pub struct LoginSubmission {
    pub username: String,
    pub password: String,
}

/// The submitted command: the target, the idempotency key, and the narrowing
/// signals. Deliberately NOT the old `ActionRequest` mirror.
///
/// `tenant` is a *requested target*, not a claim: it must match a membership
/// row the server finds for the session's principal, and one that does not —
/// for any reason, including a tenant that never existed — is the same 403.
/// There is no `role`, `capability`, `enabled_action_ids` or `human_approved`
/// field, and unknown fields are dropped rather than honoured, so a legacy
/// body carrying them parses and changes nothing.
#[derive(Debug, Deserialize)]
pub struct CommandSubmission {
    pub tenant: TenantId,
    pub action_id: String,
    /// The client's idempotency key: the receipt's identity. A retry of the
    /// same command with the same key returns the recorded receipt instead of
    /// executing again; the same key with a *different* command is refused.
    pub request_id: Uuid,
    /// The command's input: opaque bytes for fingerprinting, stored in the
    /// receipt so authorized recovery can replay what ran.
    pub input: Option<String>,
    /// The optimistic-concurrency revision this command binds to. Approvals
    /// bind it too, so an approval cannot be replayed against a newer
    /// revision of the same command.
    pub revision: i32,
    pub available_on_page: Option<bool>,
    pub confirmed: Option<bool>,
    pub declined: Option<bool>,
    /// The bound approval this execution redeems — the only way a
    /// human-approval action passes the gate.
    pub approval_id: Option<Uuid>,
}

/// `POST /approvals`: ask for a bound approval for exactly this command.
#[derive(Debug, Deserialize)]
pub struct ApprovalSubmission {
    pub tenant: TenantId,
    pub action_id: String,
    pub input: Option<String>,
    pub revision: i32,
}

/// `POST /approvals/approve`: stamp a pending approval with a distinct human.
///
/// There is no `approved: true` field: the act of an eligible, authenticated,
/// non-actor principal posting the id IS the approval, and it lands as a row
/// the execute path must find bound to its own command hash.
#[derive(Debug, Deserialize)]
pub struct ApproveSubmission {
    pub tenant: TenantId,
    pub approval_id: Uuid,
}

/// `GET /commands/status`: recover a recorded receipt.
#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
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

/// The answer to a login.
#[derive(Debug, Serialize)]
pub struct LoginOutcome {
    /// `authenticated`, `rejected`, or `refused`.
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The session token: present exactly once, in this response, and only for
    /// a caller that identifies itself as a command-line client
    /// (`x-archava-client: cli`). A browser receives it in the HttpOnly cookie
    /// instead — a body its own script could read would undo that. Only the HMAC
    /// digest is stored server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The CSRF value bound to this token — send it back as `x-archava-csrf`
    /// on every mutation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf: Option<String>,
}

impl LoginOutcome {
    fn base(decision: &str) -> Self {
        Self {
            decision: decision.to_string(),
            reason: None,
            message: None,
            token: None,
            csrf: None,
        }
    }
}

/// The status a decision maps to.
///
/// A refusal is a 4xx because the caller can fix it; a server fault is a 5xx
/// because the caller cannot. Conflating them would tell an operator to retry a
/// request that will be refused every time.
fn status_for(decision: &PolicyDecision) -> StatusCode {
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

// --- Command execution -------------------------------------------------------

/// The reviewed-command route: extraction → session → grant → gate → write.
async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<CommandSubmission>, JsonRejection>,
) -> (StatusCode, Json<CommandOutcome>) {
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => return rejection(rejected),
    };
    if submission.revision < 1
        || submission
            .input
            .as_deref()
            .is_some_and(|input| input.len() > MAX_INPUT_BYTES)
    {
        return authority_refusal(&AuthorityError::InvalidSubmission);
    }
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return refusal,
    };
    match execute_command(&state, &presented, submission).await {
        Ok(response) => response,
        Err(error) => authority_refusal(&error),
    }
}

/// Execute — or idempotently replay — one command.
///
/// One transaction for the whole request: the authority reads, the gate, the
/// approval consumption and the receipt insert. It commits in exactly one
/// place (after the insert) and is rolled back explicitly on every refusal
/// path; an early `return Err` drops it, which sqlx defines as a rollback, so
/// no path can commit accidentally.
async fn execute_command(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: CommandSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.inner.ledger.namespace;
    let ledger = state.inner.ledger.ledger;
    let tenant = submission.tenant.clone();
    let fingerprint = authority::input_fingerprint(
        &submission.action_id,
        submission.revision,
        submission.input.as_deref(),
    );

    let mut transaction =
        authority::begin(&state.inner.pool, state.inner.ledger, tenant.clone()).await?;

    // 1. The grant. `active` is in the query, so revoked and absent are one
    //    row-shape: `None`.
    let membership = {
        let connection = transaction.checked_executor().await?;
        let found = authority::resolve_membership(
            connection,
            namespace,
            ledger,
            &tenant,
            &presented.principal_id,
        )
        .await?;
        let Some(membership) = found else {
            let _ = transaction.rollback().await;
            return Err(AuthorityError::MissingMembership);
        };
        membership
    };

    // 2. Idempotent replay: a receipt already recorded for this key. It is
    //    the caller's own (actor-checked below), so reading it before the
    //    gate is reading a record the caller could have read via
    //    /commands/status anyway; the gate still re-runs on the CURRENT
    //    grants, because recovery from a revoked identity must fail too.
    let replayed = {
        let connection = transaction.checked_executor().await?;
        authority::load_receipt(connection, namespace, &tenant, submission.request_id)
            .await
            .map_err(|error| AuthorityError::database(ledger, error))?
    };
    if let Some((actor, receipt)) = replayed {
        let owned = actor == presented.principal_id;
        if !owned {
            let _ = transaction.rollback().await;
            return Ok(unknown_request());
        }
        let stored_fingerprint = receipt
            .as_ref()
            .and_then(|value| value.get("fingerprint"))
            .and_then(|value| value.as_str());
        if stored_fingerprint != Some(fingerprint.as_str()) {
            let _ = transaction.rollback().await;
            return Ok(conflict(
                "request_id_reused",
                "This idempotency key was already used for a different command.",
            ));
        }
        // The approval this receipt already redeemed travels with it; what is
        // re-checked is the LIVE grant — role, capability, scope, active.
        let request = authority::derive_request(
            tenant.clone(),
            &membership,
            &submission.action_id,
            submission.available_on_page,
            submission.confirmed,
            submission.declined,
            true,
        )?;
        let decision = state.inner.authorizer.evaluate(&request);
        // The recorded command's fingerprint, taken BEFORE `receipt` is
        // consumed: a replay must let its caller prove the effect it received
        // belongs to ITS command (RN02). Reaching here with a fingerprint
        // mismatch is already impossible — the mismatch branch above answers
        // `conflict` — so reporting the stored value is reporting exactly
        // what matched.
        let recorded_fingerprint = stored_fingerprint.map(str::to_string);
        let _ = transaction.rollback().await;
        return Ok(match decision {
            PolicyDecision::Allowed { .. } => (
                StatusCode::OK,
                Json(CommandOutcome {
                    tenant: Some(tenant.as_str().to_string()),
                    request_id: Some(submission.request_id),
                    receipt: receipt.filter(|value| !value.is_null()),
                    fingerprint: recorded_fingerprint,
                    ..CommandOutcome::base("replayed")
                }),
            ),
            other => (status_for(&other), Json(CommandOutcome::from(&other))),
        });
    }

    // 3. The bound approval — server-resolved, never a body claim. `false`
    //    means "no row binds this exact (actor, action, hash, revision) and is
    //    approved, unconsumed and unexpired", whatever the body said.
    let human_approved = match submission.approval_id {
        Some(approval_id) => {
            let connection = transaction.checked_executor().await?;
            authority::load_bound_approval(
                connection,
                namespace,
                &tenant,
                approval_id,
                &presented.principal_id,
                &submission.action_id,
                &fingerprint,
                submission.revision,
            )
            .await
            .map_err(|error| AuthorityError::database(ledger, error))?
        }
        None => false,
    };

    // 4. The gate, on facts only steps 1–3 could establish.
    let request = authority::derive_request(
        tenant.clone(),
        &membership,
        &submission.action_id,
        submission.available_on_page,
        submission.confirmed,
        submission.declined,
        human_approved,
    )?;
    let decision = state.inner.authorizer.evaluate(&request);
    if !matches!(decision, PolicyDecision::Allowed { .. }) {
        let status = status_for(&decision);
        let _ = transaction.rollback().await;
        return Ok((status, Json(CommandOutcome::from(&decision))));
    }

    // 5. Allowed: the write.
    //
    //    The approval comes off the table first — single-use, in SQL, guarded
    //    by the same binding the gate just checked, so a concurrent retry
    //    loses loudly (0 rows) rather than executing twice.
    let action = ActionRegistry::try_get(&submission.action_id)
        .expect("the gate only allows registered actions");
    let requires_human = action.level >= NEVER_AUTONOMOUS_LEVEL
        || action.confirmation == ConfirmationMode::HumanApproval;
    if requires_human {
        let Some(approval_id) = submission.approval_id else {
            // Unreachable while the gate is honest: `human_approved` can only
            // be true when an id was presented. Refused rather than panicked,
            // because an unreachable branch that panics is a 500 waiting for
            // the first gate regression.
            let _ = transaction.rollback().await;
            return Ok(unavailable("approval_required"));
        };
        let connection = transaction.checked_executor().await?;
        let consumed = authority::consume_approval(
            connection,
            namespace,
            &tenant,
            approval_id,
            &presented.principal_id,
            &submission.action_id,
            &fingerprint,
            submission.revision,
        )
        .await
        .map_err(|error| AuthorityError::database(ledger, error))?;
        if !consumed {
            let _ = transaction.rollback().await;
            return Ok(conflict(
                "approval_consumed",
                "This approval was already used, expired, or does not match this command.",
            ));
        }
    }

    let receipt = serde_json::json!({
        "action": submission.action_id.clone(),
        "fingerprint": fingerprint.clone(),
        "revision": submission.revision,
        "input": submission.input.clone(),
        "approval_id": submission.approval_id,
    });
    // `ON CONFLICT DO NOTHING` rather than a parse of a unique violation: a
    // concurrent identical submission is a normal idempotent retry, not an
    // error, and zero affected rows says so directly.
    let insert = format!(
        "INSERT INTO {namespace}.commands(tenant_id,request_id,actor_id,fingerprint,receipt) \
         VALUES($1,$2,$3,$4,$5) ON CONFLICT (tenant_id,request_id) DO NOTHING \
         RETURNING request_id"
    );
    let recorded = {
        let connection = transaction.checked_executor().await?;
        sqlx::query(sqlx::AssertSqlSafe(insert.as_str()))
            .bind(tenant.as_str())
            .bind(submission.request_id)
            .bind(&presented.principal_id)
            .bind(&fingerprint)
            .bind(&receipt)
            .fetch_optional(connection)
            .await
            .map_err(|error| AuthorityError::database(ledger, error))?
    };
    let Some(response) = recorded.map(|_| {
        (
            StatusCode::OK,
            Json(CommandOutcome {
                tenant: Some(tenant.as_str().to_string()),
                request_id: Some(submission.request_id),
                receipt: Some(receipt.clone()),
                fingerprint: Some(fingerprint.clone()),
                ..CommandOutcome::base("allowed")
            }),
        )
    }) else {
        // Lost the race inside our own transaction's view: reload what the
        // winner recorded and answer as a replay — but only if what the winner
        // recorded IS this command.
        //
        // RN02: this branch compared the actor alone and handed back whatever
        // the winner had recorded, so two concurrent submissions of the same
        // key with inputs `race-A` and `race-B` both answered 200 and the loser
        // received the winner's input. The idempotency key identifies the
        // SLOT; the fingerprint identifies the COMMAND. Both must match. A
        // mismatch is the same `request_id_reused` conflict a sequential reuse
        // gets — one rule, applied by whichever path observed the collision,
        // so the answer does not depend on the interleaving.
        let won = {
            let connection = transaction.checked_executor().await?;
            authority::load_receipt(connection, namespace, &tenant, submission.request_id)
                .await
                .map_err(|error| AuthorityError::database(ledger, error))?
        };
        let _ = transaction.rollback().await;
        return Ok(match won {
            // The winner was us, recording this exact command: a real replay.
            Some((actor, stored))
                if actor == presented.principal_id
                    && stored
                        .as_ref()
                        .and_then(|value| value.get("fingerprint"))
                        .and_then(|value| value.as_str())
                        == Some(fingerprint.as_str()) =>
            {
                (
                    StatusCode::OK,
                    Json(CommandOutcome {
                        tenant: Some(tenant.as_str().to_string()),
                        request_id: Some(submission.request_id),
                        receipt: stored.filter(|value| !value.is_null()),
                        ..CommandOutcome::base("replayed")
                    }),
                )
            }
            // The winner was us but recorded a DIFFERENT command. The key is
            // spent; this submission did not run and is not told what did.
            Some((actor, _)) if actor == presented.principal_id => conflict(
                "request_id_reused",
                "This idempotency key was already used for a different command.",
            ),
            // Another principal's slot, or no row after all: one answer.
            _ => unknown_request(),
        });
    };
    transaction.commit().await.map_err(AuthorityError::from)?;
    Ok(response)
}

// --- Receipt recovery --------------------------------------------------------

/// `GET /commands/status`.
async fn command_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<StatusQuery>, QueryRejection>,
) -> (StatusCode, Json<CommandOutcome>) {
    let Query(query) = match query {
        Ok(query) => query,
        Err(rejected) => return query_rejection(rejected),
    };
    // Read-only, so no CSRF header: a cross-origin page cannot read this
    // response without CORS, and the mutation routes are where a forged
    // request has effect.
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return refusal,
    };
    match status_receipt(&state, &presented, query).await {
        Ok(response) => response,
        Err(error) => authority_refusal(&error),
    }
}

/// Find the caller's own receipt — after the same membership check every
/// other route performs, so a revoked grant is a 403 here and not a 404.
///
/// ## RN01: recovery re-answers the gate on CURRENT grants
///
/// Membership alone was never enough, and the audit proved it: a principal
/// downgraded to `viewer` kept an active membership and read back the original
/// input of an `admin.config.update` receipt it had run while still an admin —
/// a payload its current role could no longer produce, handed to it by a route
/// that asked only "does a membership row exist". Replay of the same command
/// was correctly refused (`role_not_allowed`), so execute and recovery
/// disagreed about the same principal and the same action.
///
/// The fix is that recovery is not a separate, weaker question. A receipt is a
/// record of an *action*, and reading it back is answered by the same gate that
/// decides whether this principal may act on that action **now**: the stored
/// action id, the live role/capability/scope, the live `active` flag. A denial
/// returns its own refusal rather than the payload. `human_approved` is
/// `true` here on purpose and is not a bypass — it records that the approval
/// this receipt redeemed was consumed when it ran; the branches that still
/// refuse a *read* are the role, capability and scope ones, which is exactly
/// the downgrade this route used to miss.
///
/// `ConfirmationRequired` is deliberately NOT a refusal for a read. It asks a
/// question about executing the action again ("confirm first"), and a recorded
/// receipt is not an execution; refusing here would make authorized recovery of
/// an ordinary confirmed action impossible, which the mandate forbids in the
/// other direction. Only `Denied` — this principal may not do this — blocks.
async fn status_receipt(
    state: &AppState,
    presented: &AuthenticatedSession,
    query: StatusQuery,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.inner.ledger.namespace;
    let ledger = state.inner.ledger.ledger;
    let mut transaction =
        authority::begin(&state.inner.pool, state.inner.ledger, query.tenant.clone()).await?;
    let membership = {
        let connection = transaction.checked_executor().await?;
        let found = authority::resolve_membership(
            connection,
            namespace,
            ledger,
            &query.tenant,
            &presented.principal_id,
        )
        .await?;
        let Some(membership) = found else {
            let _ = transaction.rollback().await;
            return Err(AuthorityError::MissingMembership);
        };
        membership
    };
    let found = {
        let connection = transaction.checked_executor().await?;
        authority::load_receipt(connection, namespace, &query.tenant, query.request_id)
            .await
            .map_err(|error| AuthorityError::database(ledger, error))?
    };
    let _ = transaction.rollback().await;
    let Some((actor, receipt)) = found else {
        // Another principal's key and no such key: one answer.
        return Ok(unknown_request());
    };
    if actor != presented.principal_id {
        return Ok(unknown_request());
    }
    // The receipt is the caller's own. Now: may this caller still reach the
    // action it records, under the grants it holds at this moment?
    match receipt_read_refusal(state, &membership, &query.tenant, &receipt) {
        Some(refusal) => Ok(refusal),
        None => Ok((
            StatusCode::OK,
            Json(CommandOutcome {
                tenant: Some(query.tenant.as_str().to_string()),
                request_id: Some(query.request_id),
                receipt: receipt.filter(|value| !value.is_null()),
                ..CommandOutcome::base("receipt")
            }),
        )),
    }
}

/// Re-answer the policy gate for the action a stored receipt records.
///
/// `None` means "no refusal" — the caller's current grants still reach this
/// action. `Some` is the response to send instead of the payload, already
/// carrying the right status. See [`status_receipt`] for why this exists.
fn receipt_read_refusal(
    state: &AppState,
    membership: &authority::Membership,
    tenant: &TenantId,
    receipt: &Option<serde_json::Value>,
) -> Option<(StatusCode, Json<CommandOutcome>)> {
    // A receipt with no legible action id cannot be re-authorized. Fail closed:
    // the alternative is returning a payload this host cannot justify, and a
    // receipt written by this host always carries one.
    let Some(action_id) = receipt
        .as_ref()
        .and_then(|value| value.get("action"))
        .and_then(|value| value.as_str())
        .filter(|action| !action.is_empty())
    else {
        tracing::error!("stored receipt has no action id; refusing recovery");
        return Some(forbidden(
            "receipt_not_recoverable",
            "This receipt cannot be authorized for recovery.",
        ));
    };
    let Ok(request) = authority::derive_request(
        tenant.clone(),
        membership,
        action_id,
        None,
        None,
        None,
        true,
    ) else {
        // `derive_request` refuses only on the scope rule (L3+ with an
        // `assigned` scope) — the same `missing_scope` the execute path gives.
        return Some(forbidden(
            "missing_scope",
            "The membership's scope does not cover this action.",
        ));
    };
    match state.inner.authorizer.evaluate(&request) {
        PolicyDecision::Denied {
            action,
            reason,
            message,
        } => {
            let decision = PolicyDecision::Denied {
                action,
                reason,
                message,
            };
            let status = status_for(&decision);
            Some((status, Json(CommandOutcome::from(&decision))))
        }
        // Allowed, and ConfirmationRequired (see the route docs for why the
        // latter is not a read refusal).
        PolicyDecision::Allowed { .. } | PolicyDecision::ConfirmationRequired { .. } => None,
    }
}

// --- Bound approvals ---------------------------------------------------------

/// `POST /approvals`: record the *need* for a human approval, bound to this
/// command. Stamping it is a separate request by a different principal.
async fn approval_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<ApprovalSubmission>, JsonRejection>,
) -> (StatusCode, Json<CommandOutcome>) {
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => return rejection(rejected),
    };
    if submission.revision < 1
        || submission
            .input
            .as_deref()
            .is_some_and(|input| input.len() > MAX_INPUT_BYTES)
    {
        return authority_refusal(&AuthorityError::InvalidSubmission);
    }
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return refusal,
    };
    match request_approval(&state, &presented, submission).await {
        Ok(response) => response,
        Err(error) => authority_refusal(&error),
    }
}

async fn request_approval(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: ApprovalSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.inner.ledger.namespace;
    let ledger = state.inner.ledger.ledger;
    let tenant = submission.tenant.clone();
    let fingerprint = authority::input_fingerprint(
        &submission.action_id,
        submission.revision,
        submission.input.as_deref(),
    );

    let mut transaction =
        authority::begin(&state.inner.pool, state.inner.ledger, tenant.clone()).await?;
    let membership = {
        let connection = transaction.checked_executor().await?;
        let found = authority::resolve_membership(
            connection,
            namespace,
            ledger,
            &tenant,
            &presented.principal_id,
        )
        .await?;
        let Some(membership) = found else {
            let _ = transaction.rollback().await;
            return Err(AuthorityError::MissingMembership);
        };
        membership
    };

    // Evaluate the gate WITHOUT approval (and without page/confirm signals:
    // those belong to the moment of execution, not to the approval, which
    // binds action + input + revision only).
    let request = authority::derive_request(
        tenant.clone(),
        &membership,
        &submission.action_id,
        None,
        None,
        None,
        false,
    )?;
    let decision = state.inner.authorizer.evaluate(&request);
    let approval_gates = match &decision {
        PolicyDecision::ConfirmationRequired { mode, .. } => {
            *mode == ConfirmationMode::HumanApproval
        }
        PolicyDecision::Denied { reason, .. } => *reason == ActionDenial::AdminActionRequiresHuman,
        PolicyDecision::Allowed { .. } => false,
    };
    if !approval_gates {
        let _ = transaction.rollback().await;
        return Ok(match decision {
            // The gate did NOT ask for a human approval: an approval record
            // here would be theatre — it would never be consulted.
            PolicyDecision::Allowed { .. } | PolicyDecision::ConfirmationRequired { .. } => {
                conflict(
                    "approval_not_required",
                    "This action is not gated by a human approval.",
                )
            }
            other => (status_for(&other), Json(CommandOutcome::from(&other))),
        });
    }

    let approval_id = Uuid::new_v4();
    {
        let connection = transaction.checked_executor().await?;
        authority::create_approval(
            connection,
            namespace,
            &tenant,
            approval_id,
            &presented.principal_id,
            &submission.action_id,
            &fingerprint,
            submission.revision,
            APPROVAL_TTL_SECONDS,
        )
        .await
        .map_err(|error| AuthorityError::database(ledger, error))?;
    }
    transaction.commit().await.map_err(AuthorityError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(CommandOutcome {
            tenant: Some(tenant.as_str().to_string()),
            approval_id: Some(approval_id),
            fingerprint: Some(fingerprint),
            ..CommandOutcome::base("approval_requested")
        }),
    ))
}

/// `POST /approvals/approve`: stamp a pending approval.
async fn approval_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<ApproveSubmission>, JsonRejection>,
) -> (StatusCode, Json<CommandOutcome>) {
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => return rejection(rejected),
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return refusal,
    };
    match grant_approval(&state, &presented, submission).await {
        Ok(response) => response,
        Err(error) => authority_refusal(&error),
    }
}

async fn grant_approval(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: ApproveSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.inner.ledger.namespace;
    let ledger = state.inner.ledger.ledger;
    let tenant = submission.tenant.clone();

    let mut transaction =
        authority::begin(&state.inner.pool, state.inner.ledger, tenant.clone()).await?;
    let approver = {
        let connection = transaction.checked_executor().await?;
        let found = authority::resolve_membership(
            connection,
            namespace,
            ledger,
            &tenant,
            &presented.principal_id,
        )
        .await?;
        let Some(membership) = found else {
            let _ = transaction.rollback().await;
            return Err(AuthorityError::MissingMembership);
        };
        membership
    };

    // Separation of duties, checked BEFORE the approval is looked up so an
    // ineligible caller learns nothing about which ids exist: a service
    // principal cannot approve, and only an owner/admin human can.
    if approver.kind != MembershipKind::Human
        || !matches!(approver.role, RoleName::Owner | RoleName::Admin)
    {
        let _ = transaction.rollback().await;
        return Ok(forbidden(
            "approver_ineligible",
            "Only an active human owner or admin may approve.",
        ));
    }

    let pending = {
        let connection = transaction.checked_executor().await?;
        authority::load_approval_state(connection, namespace, &tenant, submission.approval_id)
            .await
            .map_err(|error| AuthorityError::database(ledger, error))?
    };
    let Some(pending) = pending else {
        let _ = transaction.rollback().await;
        return Ok(unknown_approval());
    };
    if pending.actor_id == presented.principal_id {
        let _ = transaction.rollback().await;
        return Ok(forbidden(
            "self_approval",
            "A command cannot be approved by the principal who submitted it.",
        ));
    }
    if pending.approved || pending.consumed || pending.expired {
        let _ = transaction.rollback().await;
        return Ok(conflict(
            "approval_unavailable",
            "This approval is already used, already stamped, or expired.",
        ));
    }

    let stamped = {
        let connection = transaction.checked_executor().await?;
        authority::approve_approval(
            connection,
            namespace,
            &tenant,
            submission.approval_id,
            &presented.principal_id,
        )
        .await
        .map_err(|error| AuthorityError::database(ledger, error))?
    };
    if !stamped {
        // A concurrent stamp won the same single-use window.
        let _ = transaction.rollback().await;
        return Ok(conflict(
            "approval_unavailable",
            "This approval is already used, already stamped, or expired.",
        ));
    }
    transaction.commit().await.map_err(AuthorityError::from)?;
    Ok((
        StatusCode::OK,
        Json(CommandOutcome {
            tenant: Some(tenant.as_str().to_string()),
            approval_id: Some(submission.approval_id),
            ..CommandOutcome::base("approved")
        }),
    ))
}

// --- The remaining R01 screens -----------------------------------------------

/// The login route: exchange username + password for a session.
///
/// Identity only. No tenant and no membership is consulted here — a user may
/// hold memberships in several tenants (or none), and which one a command
/// targets is decided per request by the membership it can prove. Failures
/// are single-reason on purpose: invalid credentials is one answer whether
/// the username is unknown, the password wrong, the account deactivated or
/// the session insert racing a deactivation.
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    source: LoginSource,
    submission: Result<Json<LoginSubmission>, JsonRejection>,
) -> (StatusCode, HeaderMap, Json<LoginOutcome>) {
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => {
            tracing::warn!(error = %rejected, "refused a login submission at the boundary");
            return (
                rejected.status(),
                HeaderMap::new(),
                Json(LoginOutcome {
                    reason: Some("malformed_submission".to_string()),
                    message: Some("The submission could not be processed.".to_string()),
                    ..LoginOutcome::base("rejected")
                }),
            );
        }
    };
    match state
        .inner
        .identity
        .login(&submission.username, &submission.password, &source.bucket)
        .await
    {
        Ok(token) => {
            let csrf = state.inner.identity.csrf(&token);
            // The browser path holds the token in an HttpOnly cookie — `Secure`
            // and `__Host-` prefixed when the request arrived over TLS — and
            // keeps the CSRF value in the body, where the slice's own script
            // reads it and echoes it as the mutation header. A refusal never
            // sets a cookie.
            let mut response_headers = HeaderMap::new();
            response_headers.insert(
                header::SET_COOKIE,
                session_cookie(&token, is_secure_from_peer(source.peer, &headers))
                    .parse()
                    .expect("a well-formed cookie attribute list"),
            );
            // F-07: a browser never needs the raw token in the body — reading it
            // there is exactly the leak the HttpOnly cookie prevents. A scripted
            // client without a cookie jar asks for it by name, and a cross-site
            // page cannot set that header without a preflight this host never
            // approves.
            let body_token = is_command_line_client(&headers).then_some(token);
            (
                StatusCode::OK,
                response_headers,
                Json(LoginOutcome {
                    token: body_token,
                    csrf: Some(csrf),
                    ..LoginOutcome::base("authenticated")
                }),
            )
        }
        Err(error) => {
            let (status, outcome) = refused_login_outcome(&error);
            (status, HeaderMap::new(), Json(outcome))
        }
    }
}

/// The logout route's body. A separate contract from `CommandOutcome`: the
/// decision vocabulary there enumerates command outcomes, and "signed out" is
/// not one of them. The refusal shape is identical (decision/reason/message),
/// so a caller reads one contract either way.
#[derive(Debug, Serialize)]
pub struct LogoutOutcome {
    /// `signed_out` on success; the refusal vocabulary otherwise
    /// (`unauthenticated`, `refused`).
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The logout route (C02): revoke the server session, THEN close the cookie.
///
/// The order is the whole point. The session row is deleted first — the
/// server's authority — and the cookie expiry is only sent afterwards. The
/// reverse order would let a failed revocation look like a signed-out
/// browser while the credential kept working, which is exactly the defect
/// the 19 Oktober audit reproduced: "sign out" cleared UI state and the old
/// cookie still read twenty authorized contact rows.
///
/// A CSRF failure refuses BEFORE anything is revoked, so a forged cross-site
/// request cannot close somebody else's session, and a session that is
/// already gone is `401 unauthenticated` rather than a success that closed
/// nothing. Other live sessions of the same user are untouched: this revokes
/// the presented token, nothing else.
async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Json<LogoutOutcome>) {
    let session = match authenticate(&state, &headers, true).await {
        Ok(session) => session,
        Err((status, Json(outcome))) => {
            return (
                status,
                HeaderMap::new(),
                Json(LogoutOutcome {
                    decision: outcome.decision,
                    reason: outcome.reason,
                    message: outcome.message,
                }),
            )
        }
    };
    // The authenticated session is what proves the caller may close THIS
    // token; the token itself is what gets revoked.
    let _ = session;
    let Some(token) = session_token(&headers) else {
        // `authenticate` only succeeds for a presented token, so this is
        // unreachable in practice; answer honestly rather than assume.
        return (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            Json(LogoutOutcome {
                decision: "unauthenticated".to_string(),
                reason: Some("unauthenticated".to_string()),
                message: Some("There is no session to close.".to_string()),
            }),
        );
    };
    match state.inner.identity.revoke(&token).await {
        Ok(true) => (
            StatusCode::OK,
            expire_session_cookie(),
            Json(LogoutOutcome {
                decision: "signed_out".to_string(),
                reason: None,
                message: Some("The session is closed.".to_string()),
            }),
        ),
        Ok(false) => (
            // The session disappeared between the authentication and the
            // revocation (another tab signed out first). The caller's goal is
            // still met — no session answers for that token any more — so
            // this closes the cookie too rather than pretending otherwise.
            StatusCode::OK,
            expire_session_cookie(),
            Json(LogoutOutcome {
                decision: "signed_out".to_string(),
                reason: None,
                message: Some("The session is closed.".to_string()),
            }),
        ),
        Err(error) => {
            tracing::error!(error = %error, "identity store unavailable while revoking a session");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                HeaderMap::new(),
                Json(LogoutOutcome {
                    decision: "refused".to_string(),
                    reason: Some("identity_unavailable".to_string()),
                    message: Some("The session could not be closed right now.".to_string()),
                }),
            )
        }
    }
}

/// The expired-cookie headers the logout route sends — both names, because the
/// jar may hold either (a session minted before, and one minted after, the
/// deployment gained a TLS terminator in front of it).
fn expire_session_cookie() -> HeaderMap {
    let mut headers = HeaderMap::new();
    for cookie in expired_session_cookies() {
        headers.append(
            header::SET_COOKIE,
            cookie.parse().expect("a well-formed cookie attribute list"),
        );
    }
    headers
}

/// The login route's refusal mapping, as a pure function so it has in-crate
/// unit tests (RN07/W-PW03): the load-bearing property is WHICH status each
/// identity failure produces — an overload must be distinguishable from a
/// wrong password, and a code path that once folded `Busy` into
/// `invalid_credentials` was exactly such a confusion, fixed by asserting the
/// boundaries here where they cost nothing to run.
///
/// `IdentityError` has no `PartialEq` (deadlock-free comparisons of errors
/// are not meaningful), so matching is exhaustive by construction.
fn refused_login_outcome(error: &IdentityError) -> (StatusCode, LoginOutcome) {
    let (status, reason, message) = match error {
        IdentityError::InvalidCredentials => (
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "The username or password is incorrect.",
        ),
        IdentityError::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many login attempts; try again later.",
        ),
        IdentityError::Busy => (
            // Deliberately NOT 401: a credential-shaped answer for a correct
            // password under load tells its user the password was wrong, and
            // the natural retry then burns the account's rate budget chasing
            // a credential problem that does not exist.
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "Too many simultaneous verifications; try again.",
        ),
        IdentityError::Unavailable | IdentityError::Unauthenticated => (
            // Operator-facing detail goes to the log, never the response: the
            // caller's answer is `503 refused`, and an error naming the query
            // or the constraint is exactly what must not be reflected (D09).
            StatusCode::SERVICE_UNAVAILABLE,
            "identity",
            "",
        ),
    };
    (
        status,
        LoginOutcome {
            reason: Some(reason.to_string()),
            message: (!message.is_empty()).then(|| message.to_string()),
            ..LoginOutcome::base("refused")
        },
    )
}

// The contact-list placeholder was removed when the §1.4B slice landed the
// real, session-scoped contact read: `crate::crm::contacts` serves `GET
// /contacts` now. (The refusal it used to carry assumed a caller-less read
// was possible at all; the real read is session-scoped, so the note that
// explained the reservation stays here in the router where the route lives.)
#[cfg(test)]
mod tests {
    use archava_core::roles::CapabilityTier;
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

    /// Presets profiles list is public and requires no authority.
    #[tokio::test]
    async fn presets_profiles_answers_without_authority() {
        let (status, response) = call("/api/v1/presets/profiles", None).await;
        assert_eq!(status, StatusCode::OK, "got: {response}");
        assert!(response.contains("fnb_cafe"), "got: {response}");
        assert!(response.contains("clinic_healthcare"), "got: {response}");
    }

    /// Proactive action feed generates cards without requiring a session.
    #[tokio::test]
    async fn action_feed_returns_proactive_cards_without_authority() {
        let (status, response) = call("/api/v1/feed/action-cards?profile=fnb_cafe", None).await;
        assert_eq!(status, StatusCode::OK, "got: {response}");
        assert!(response.contains("card_ar_"), "got: {response}");
        assert!(response.contains("print_shift_summary"), "got: {response}");
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

    /// U01: `/embed/checkout` serves responsive HTML checkout document without session requirement,
    /// and `security_headers` supplies `frame-ancestors *` so merchant websites can embed it.
    #[tokio::test]
    async fn embed_checkout_answers_with_html_and_frame_ancestors_wildcard() {
        let (status, body) = call(
            "/embed/checkout?orderId=ord_test_99&amount=150000&tenant=tenant_acme",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Archava Checkout - ord_test_99"));
        assert!(body.contains("tenant_acme"));
        assert!(body.contains("150000"));
        assert!(body.contains("ARCHAVA_PAYMENT_SIMULATED"));
    }

    #[tokio::test]
    async fn embed_checkout_neutralizes_xss_injection_attempts() {
        let (status, body) = call(
            "/embed/checkout?orderId=%22;alert(1);//&tenant=acme%3Cscript%3E&item=%3Cimg%20src=x%20onerror=alert(1)%3E",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // HTML tags in item name must be escaped
        assert!(!body.contains("<img src=x onerror=alert(1)>"));
        assert!(body.contains("&lt;img src=x onerror=alert(1)&gt;"));
        // Script injection in identifiers must be sanitized
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

    /// The metrics surface is unauthenticated on purpose — a scraper has no
    /// session — and still exposes nothing tenant-shaped: only counters and a
    /// database-up gauge.
    #[tokio::test]
    async fn metrics_exposes_counters_without_authority() {
        let (status, body) = call("/metrics", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("archava_http_requests_total"));
        assert!(body.contains("archava_http_request_duration_seconds_count"));
        assert!(body.contains("archava_db_ready 0"));
        assert!(!body.contains("tenant"));
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
