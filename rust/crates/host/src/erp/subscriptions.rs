//! The subscriptions routes — the Archxy 2 port of the R07 entitlements
//! surface against `archava_subscriptions`, in the shape
//! [`crate::erp::billing`] pinned for this host: extraction → session →
//! ONE tenant transaction whose member ladder, authorization, idempotency
//! claim, effect and audit all run on the adapters' step functions.
//!
//! ## The answers the six routes give
//!
//! - `POST /erp/subscriptions/commands` — the TS api answers
//!   `json(await service.mutate(...))`: the BARE receipt document, or one
//!   of the repository's refusal codes folded by HTTP class exactly as the
//!   TS api's own `codeMap` does — `forbidden` 403 (`denied`), `not_found`
//!   404 (`denied`/`unknown_resource`), `conflict` 409 (`refused`),
//!   `already_active`/`exhausted`/`suspended` 409 (`refused`),
//!   `past_due` **402** (`refused` — the TS gives `past_due` its own
//!   Payment-Required class), the corrected `not_paid` refusal on the same
//!   402 class, and a malformed submission 400 (`rejected`/
//!   `invalid_submission`).
//! - `GET /erp/subscriptions/commands/status?tenant=&requestId=` — the TS
//!   commandStatus document verbatim: `recorded` with its receipt, or the
//!   bare `unknown`. The status read grants NOTHING beyond the
//!   transaction's own enrollment — the TS status read never checks a
//!   permission, and neither does this one.
//! - `GET /erp/subscriptions/subscriptions?tenant=&limit=&offset=&status=&contactId=` —
//!   the TS `listSubscriptions()`: the caller's own rows, bounded
//!   (`limit` 1..100, default 50; `offset` ≥ 0, default 0), newest first.
//! - `GET /erp/subscriptions/subscriptions/detail?tenant=&id=` — the TS
//!   `getSubscriptionDetail()`: the subscription with its entitlements; an
//!   `assigned`-scope caller sees nothing for a row that is not its own.
//! - `GET /erp/subscriptions/entitlements?tenant=&targetId=` — the TS
//!   `listEntitlements()`: a uuid target reads one subscription's
//!   entitlements (and refuses an unknown subscription); any other target
//!   reads a contact's entitlements.
//! - `GET /erp/subscriptions/usage-events?tenant=&entitlementId=` — the TS
//!   `listUsageEvents()`.
//!
//! ## The labor order (the service layer's)
//!
//! The TS `SubscriptionService.execute` runs its parse, the HUMAN-OPERATOR
//! rule (a service principal refuses every arm — this ledger has no
//! provider path) and the repository call. This gate keeps that order:
//! parse → human rule → transaction (member ladder → grant → claim → arm).
//! The repository's own SERVICE branch (the permissions INTERSECT with the
//! human it acts for; the scope narrows to `assigned` when either side is
//! `assigned`) is ported in the adapter and stays reachable for a caller
//! that carries a service principal.
//!
//! ## The corrected rule (audit A05 — P1)
//!
//! The baseline lets a `draft` subscription's entitlements be consumed. The
//! gate refuses that with the `not_paid` class (402) — the policy itself
//! lives in the domain crate (`consumption_refusal`), so the rule is one
//! pure function and not a branch buried in SQL. Activation follows the
//! same rule: `change_status` may only resume a subscription that carries
//! a payment record; `record_payment` is the activation path. A manual
//! payment reference is NOT provider-verified settlement — this ledger
//! records the payment, the R07 billing ledger owns that authority.
//!
//! ## The fingerprint
//!
//! The TS composition is `HMAC(this.key, JSON.stringify(mutation))` — the
//! MUTATION OBJECT ONLY, no tenant, no actor (this ledger's replay gates
//! on the payload, so a second actor resubmitting an identical
//! `commandId` REPLAYS here exactly as the TS does). The port carries it
//! on the labelled HMAC the other ERP ledgers use
//! (`subscriptions-command-fingerprint`) over the exact typed arm document
//! the parse produced.
//!
//! Every route is session-bound (`authenticate`), CSRF-bound on the
//! write, and the tenant comes only from the caller's own document — the
//! database, not the session, has the final word on whether the principal
//! belongs to it.

use archava_adapters::erp::subscriptions::{
    list_subscriptions, read_entitlements, read_subscription_detail, read_usage_events,
    record_subscriptions_command, resolve_subscriptions_context, subscriptions_command_status,
    SubscriptionError, SubscriptionPageQuery,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::subscriptions::{
    parse_mutation, SubscriptionCommandStatus, SubscriptionMutation, SubscriptionReceipt,
};
use archava_domain::erp::ErpError;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::erp::{begin_subscriptions, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};

// --- Response envelope ------------------------------------------------------

/// The union body this module's routes answer with — same shape as the
/// other ERP modules', module-local by the codebase's per-module rule.
pub enum ErpBody {
    Doc(Value),
    Outcome(CommandOutcome),
}

impl IntoResponse for ErpBody {
    fn into_response(self) -> Response {
        match self {
            ErpBody::Doc(value) => Json(value).into_response(),
            ErpBody::Outcome(value) => Json(value).into_response(),
        }
    }
}

/// Levels a host `Json<CommandOutcome>` answer up to this module's type.
fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, ErpBody) {
    (answer.0, ErpBody::Outcome(answer.1 .0))
}

/// 400 for a document the boundary parse refuses.
fn invalid_submission() -> (StatusCode, ErpBody) {
    (
        StatusCode::BAD_REQUEST,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("invalid_submission".to_string()),
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// 403 for a grant gate the caller can act on.
fn forbidden() -> (StatusCode, ErpBody) {
    (
        StatusCode::FORBIDDEN,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("forbidden".to_string()),
            message: Some("This session cannot perform that operation.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 404 for a record the caller cannot see or that no longer exists.
fn unknown_resource() -> (StatusCode, ErpBody) {
    (
        StatusCode::NOT_FOUND,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("unknown_resource".to_string()),
            message: Some("No resource matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 409 for the conflicts the repository rejects — a stale revision, a
/// fingerprint-mismatched replay, an illegal transition, an exhausted
/// quota, a suspended entitlement, an already-active subscription.
fn conflict() -> (StatusCode, ErpBody) {
    (
        StatusCode::CONFLICT,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("conflict".to_string()),
            message: Some(
                "A concurrent write or a state boundary refused this submission.".to_string(),
            ),
            ..CommandOutcome::base("refused")
        }),
    )
}

/// 402 — the TS api's own Payment-Required class for `past_due`, and the
/// corrected `not_paid` refusal's class (a subscription that was never
/// paid grants no consumption right).
fn payment_required(reason: &str, message: &str) -> (StatusCode, ErpBody) {
    (
        StatusCode::PAYMENT_REQUIRED,
        ErpBody::Outcome(CommandOutcome {
            reason: Some(reason.to_string()),
            message: Some(message.to_string()),
            ..CommandOutcome::base("refused")
        }),
    )
}

/// The refusal vocabulary mapped by the TS api's own `codeMap`.
fn map_subscription_result(
    error: SubscriptionError,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        SubscriptionError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        SubscriptionError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        SubscriptionError::Domain(ErpError::Conflict) => Ok(conflict()),
        SubscriptionError::Domain(ErpError::Unavailable) => Ok(invalid_submission()),
        SubscriptionError::NotPaid => Ok(payment_required(
            "not_paid",
            "This subscription has not been paid, so it grants no usage right.",
        )),
        SubscriptionError::PastDue => Ok(payment_required(
            "past_due",
            "This subscription is past due.",
        )),
        SubscriptionError::Exhausted => Ok(conflict()),
        SubscriptionError::Suspended => Ok(conflict()),
        SubscriptionError::AlreadyActive => Ok(conflict()),
        SubscriptionError::Database { ledger, source } => {
            Err(ErpGateError::Database(ledger, source))
        }
    }
}

fn gate_outcome(error: ErpGateError) -> (StatusCode, ErpBody) {
    match error {
        ErpGateError::Authority(refusal) => outcome(authority_refusal(&refusal)),
        ErpGateError::Database(ledger, source) => {
            tracing::error!(error = %source, "refusing: {ledger} database failure");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErpBody::Outcome(CommandOutcome {
                    reason: Some("database_failure".to_string()),
                    ..CommandOutcome::base("unavailable")
                }),
            )
        }
    }
}

// --- Actor model ------------------------------------------------------------

/// Who is acting — module-local, same shape as the other ERP modules'.
#[derive(Debug, Clone)]
pub enum ErpActor {
    Human(String),
}

impl ErpActor {
    fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal::human(id.clone()),
        }
    }
}

// --- Shapes -----------------------------------------------------------------

/// The mutation submission: tenant from the caller's own document and the
/// raw flat mutation document the strict parse judges.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The list query — the TS `subscriptionPageQuerySchema`'s bounds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionListQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
    pub contact_id: Option<String>,
}

/// The detail query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionDetailQuery {
    pub tenant: TenantId,
    pub id: Uuid,
}

/// The entitlement query — a uuid target or a contact id.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionEntitlementsQuery {
    pub tenant: TenantId,
    pub target_id: String,
}

/// The usage query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionUsageQuery {
    pub tenant: TenantId,
    pub entitlement_id: Uuid,
}

/// The fingerprint a submission records — the TS digest composition
/// (`HMAC(this.key, JSON.stringify(mutation))`) carried on the labelled
/// HMAC: over the typed arm document ALONE, because the TS covers the
/// mutation object and nothing else.
fn fingerprint(state: &AppState, mutation: &SubscriptionMutation) -> String {
    let document = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    state
        .identity()
        .digest("subscriptions-command-fingerprint", &document.to_string())
}

// --- Routes -----------------------------------------------------------------

/// `POST /erp/subscriptions/commands` — submit (or replay) a subscription
/// command. CSRF-bound like the other ERP write routes; the tenant comes
/// only from the body. The OK body is the BARE receipt.
pub(crate) async fn subscription_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<SubscriptionSubmission>, JsonRejection>,
) -> (StatusCode, ErpBody) {
    let submission = match submission {
        Ok(submission) => submission,
        Err(rejected) => return outcome(rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match submit_subscription_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/subscriptions/commands/status?tenant=&requestId=` — the
/// durable status of one command id.
pub(crate) async fn subscription_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubscriptionStatusQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match subscription_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/subscriptions/subscriptions?tenant=&limit=&offset=&status=&contactId=`
/// — the subscription list.
pub(crate) async fn subscription_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubscriptionListQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match subscription_list_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/subscriptions/subscriptions/detail?tenant=&id=` — one
/// subscription with its entitlements.
pub(crate) async fn subscription_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubscriptionDetailQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match subscription_detail_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/subscriptions/entitlements?tenant=&targetId=` — the
/// entitlements of one subscription or one contact.
pub(crate) async fn subscription_entitlements(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubscriptionEntitlementsQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match subscription_entitlements_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/subscriptions/usage-events?tenant=&entitlementId=` — the usage
/// history of one entitlement.
pub(crate) async fn subscription_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubscriptionUsageQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match subscription_usage_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- One transaction per request: the submit ---------------------------------

/// The submission pipeline — the port of `mutate()` driven by the host's
/// identity key, with the service layer's labor order: parse, the human
/// rule, then ONE transaction for the whole request.
pub async fn submit_subscription_command(
    state: &AppState,
    actor: ErpActor,
    submission: SubscriptionSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    // 0. The boundary parse — the strict verdict the TS service's parse
    //    raises (400 `invalid_request` in TS, the gate's shared 400
    //    carrier here).
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(invalid_submission()),
    };
    // 0b. The human-operator rule — the TS service refuses a service
    //     principal on every arm of this ledger.
    if principal.kind != archava_domain::erp::purchasing::PrincipalKind::Human {
        return Ok(forbidden());
    }
    // 0c. The replay's key — over the mutation document alone.
    let fingerprint = fingerprint(state, &mutation);

    let mut transaction = begin_subscriptions(state, submission.tenant.clone()).await?;

    // 1. The member ladder — the principal's own membership, active, with
    //    its permissions and scope.
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, submission.tenant.as_str(), &principal)
            .await
        {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };

    // 2. The claim → execute → audit → receipt pipeline.
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_subscriptions_command(
            connection,
            submission.tenant.as_str(),
            &access,
            &fingerprint,
            &mutation,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };

    transaction.commit().await.map_err(ErpGateError::from)?;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&receipt)
                .expect("a typed receipt serializes back to its own JSON"),
        ),
    ))
}

// --- One transaction per request: the reads ----------------------------------

/// The durable status of a command id — the TS read grants nothing beyond
/// enrollment.
pub async fn subscription_status_read(
    state: &AppState,
    actor: ErpActor,
    query: SubscriptionStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_subscriptions(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let status: SubscriptionCommandStatus = {
        let connection = transaction.checked_executor().await?;
        let _ = &access;
        match subscriptions_command_status(
            connection,
            query.tenant.as_str(),
            &query.request_id.to_string(),
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&status).expect("a typed status serializes back to its own JSON"),
        ),
    ))
}

/// The subscription list — bounded, ordered, scoped.
pub async fn subscription_list_read(
    state: &AppState,
    actor: ErpActor,
    query: SubscriptionListQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    // The TS query's own bounds: `limit` 1..100 (default 50), `offset` ≥ 0
    // (default 0), and the status filter from the status vocabulary.
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    if !(1..=100).contains(&limit) || offset < 0 {
        return Ok(invalid_submission());
    }
    if let Some(status) = query.status.as_deref() {
        if archava_domain::erp::subscriptions::SubscriptionStatus::parse(status).is_none() {
            return Ok(invalid_submission());
        }
    }
    let principal = actor.principal();
    let mut transaction = begin_subscriptions(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let page = SubscriptionPageQuery {
        limit,
        offset,
        status: query.status.clone(),
        contact_id: query.contact_id.clone(),
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match list_subscriptions(connection, query.tenant.as_str(), &access, &page).await {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}

/// One subscription with its entitlements.
pub async fn subscription_detail_read(
    state: &AppState,
    actor: ErpActor,
    query: SubscriptionDetailQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_subscriptions(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let detail = {
        let connection = transaction.checked_executor().await?;
        match read_subscription_detail(
            connection,
            query.tenant.as_str(),
            &access,
            &query.id.to_string(),
        )
        .await
        {
            Ok(detail) => detail,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    // The TS answers `null` for an unknown subscription; the gate answers
    // the same document (a caller may tell "no such subscription" from a
    // forbidden row by the shape, not by a fabricated empty page).
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&detail).expect("a typed detail serializes back to its own JSON"),
        ),
    ))
}

/// The entitlements of one subscription or one contact.
pub async fn subscription_entitlements_read(
    state: &AppState,
    actor: ErpActor,
    query: SubscriptionEntitlementsQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_subscriptions(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match read_entitlements(connection, query.tenant.as_str(), &access, &query.target_id).await
        {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}

/// The usage history of one entitlement.
pub async fn subscription_usage_read(
    state: &AppState,
    actor: ErpActor,
    query: SubscriptionUsageQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_subscriptions(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_subscriptions_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match read_usage_events(
            connection,
            query.tenant.as_str(),
            &access,
            &query.entitlement_id.to_string(),
        )
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_subscription_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}

/// The receipt type the module answers with — re-exported so the crate's
/// other modules can name it without importing the domain crate directly.
pub type SubscriptionReceiptDocument = SubscriptionReceipt;
