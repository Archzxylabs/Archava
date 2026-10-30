//! The billing routes — the Archxy 2 port of the R07 settlement mutation
//! surface against `archava_billing`, in the shape [`crate::erp::assets`]
//! pinned for this host: extraction → session → ONE tenant transaction
//! whose member ladder, authorization, idempotency claim, effect and audit
//! all run on the adapters' step functions.
//!
//! ## The answers the two routes give
//!
//! - `POST /erp/billing/commands` — the TS api answers
//!   `json(await service.mutate(...))`: the BARE receipt document
//!   (`{commandId, tenantId, action, …ids/status/outstanding/revision…,
//!   idempotencyReplay, executedAt}` — the wide receipt, its optional
//!   members present exactly when the arm names them), or one of the
//!   repository's refusal codes folded by HTTP class: forbidden 403
//!   (`denied` — AND the settlement's own authority verdict, when the
//!   provider signature does not verify), missing record 404
//!   (`denied`/`unknown_resource`), conflict 409 (`refused` — stale
//!   revisions, invalid transitions, unbalanced settlements, a
//!   fingerprint-mismatched replay), malformed submission 400
//!   (`rejected`/`invalid_submission`).
//! - `GET /erp/billing/commands/status?tenant=&requestId=` — the TS
//!   commandStatus document verbatim: `recorded` with its receipt, or
//!   bare `unknown`. The read requires the current `billing.read` grant and
//!   answers only for the principal that recorded the command: a grant
//!   holder that is not the initiator gets the same `unknown` document as a
//!   missing key, with no receipt fields. A lost/corrupt receipt is a
//!   conflict, never a fabricated answer.
//!
//! ## The labor order (the service layer's)
//!
//! The TS `BillingService.mutate` runs its parse, the human-operator rule
//! (service principals refuse every arm except the provider event — the
//! one arm authorised by a signature instead of a person) and the
//! provider-SIGNATURE verification **before** the repository opens any
//! SQL. So an under-signed claim never opens a transaction, never claims
//! a command id, and audits nothing. This gate keeps that order: parse →
//! signature verify → transaction (member ladder → grant → claim → arm).
//!
//! ## The settlement's authority (the deployment's key source)
//!
//! The TS `SignatureKeySource` resolves a secret PER TENANT AND PROVIDER
//! (null = unconfigured = refused), and the digest verifies under
//! `HMAC-SHA256(key, domain ‖ framed parts)` — `signature.ts` ported in
//! the domain crate, cross-language fixtures included. The key material
//! is a deployment fact; this host derives it from the identity store's
//! own secret so that (a) no config surface has to grow in a shared file
//! for the cohort to exist, and (b) the pairing property the TS document
//! states holds — the key for `tenant T, provider P` is
//!
//! ```text
//! key = utf8(identity().digest("billing-provider-secret", T + "\0" + P))
//! ```
//!
//! — a 64-hex-character labelled digest rendered as its ASCII bytes (≥32,
//! so `SIGNATURE_KEY_MIN_BYTES` holds; the identity digest is the
//! `HMAC-SHA256(key, label + "\0" + value)` render the identity schema
//! has always produced). Two tenants settling through the same processor
//! CANNOT share a secret: the label and both ids mix into every derived
//! key. The recorded divergence rides the KNOWN_DIFFERENCES ledger: TS's
//! unconfigured-provider refusals (no secret configured →
//! `invalid_signature`) fold into the same `invalid_signature` verdict on
//! signature comparison — an event minted without the derived key still
//! refuses, the label it refuses under is lost.
//!
//! ## The fingerprint
//!
//! The TS composition is `HMAC(this.key, JSON.stringify(mutation))` — the
//! MUTATION OBJECT ONLY, no tenant, no actor (this ledger's replay gates
//! on the payload, which is why a second actor resubmitting an identical
//! `commandId` REPLAYS here exactly as the TS does). The port carries it
//! on the labelled HMAC the other ERP ledgers use
//! (`billing-command-fingerprint`) over the exact typed arm document the
//! parse produced — trims applied, optional members on their TS
//! spellings. Ledger-internal stability is exact; the same recorded
//! divergence family (a labelled digest rather than the baseline's raw
//! unlabeled HMAC) rides from the R04/R05 cohorts.
//!
//! Both routes are session-bound (`authenticate`), CSRF-bound on the
//! write, and the tenant comes only from the caller's own document — the
//! database, not the session, has the final word on whether the principal
//! belongs to it.

use archava_domain::erp::billing::{
    parse_mutation, provider_event_digest, signatures_equal, BillingCommand, BillingCommandStatus,
    BillingMutation, PaymentEventClaim, SignatureError,
};
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::ErpError;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::erp::{begin_billing, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};
use archava_adapters::erp::billing::{
    billing_command_status, record_billing_command, resolve_billing_context,
};
use archava_adapters::erp::PurchaseError;
use archava_core::tenant::TenantId;

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

/// 403 for a grant gate the caller can act on — and for the forged
/// claim's authority refusal (the TS `invalid_signature`, folded onto
/// the same HTTP class with its label lost).
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

/// 409 for the settlement conflicts the repository rejects — stale
/// revisions, `invalid_transition`, `unbalanced_settlement`, a
/// fingerprint-mismatched replay — the family the TS api's own `codeMap`
/// folds into its 409 class.
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

fn map_purchase_result(error: PurchaseError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        PurchaseError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        PurchaseError::Domain(ErpError::Conflict) => Ok(conflict()),
        // No billing arm raises the runtime 400 class (the tools boundary
        // and the parse boundary are the parse's own 400s); the arm stays
        // mapped for the shared net's completeness.
        PurchaseError::Domain(ErpError::Unavailable) => Ok(invalid_submission()),
        PurchaseError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
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

// --- Shapes ----------------------------------------------------------------

/// The mutation submission: tenant from the caller's own document (the
/// database, not the session, has the final word on whether the principal
/// belongs to it) and the raw flat mutation document the strict parse
/// judges — the TS wire's `billingMutationSchema`, its own shape (the
/// `commandId` rides INSIDE each arm; the TS route's PATH tenant rides
/// the body envelope here — the recorded divergence family every ERP
/// gate carries).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The fingerprint a submission records — the TS digest composition
/// (`HMAC(this.key, JSON.stringify(mutation))`) carried on the labelled
/// HMAC: over the typed arm document ALONE, because the TS covers the
/// mutation object and nothing else. The replay gates on the payload; a
/// second actor resubmitting the same `commandId` replays.
fn fingerprint(state: &AppState, mutation: &BillingMutation) -> String {
    let document = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    state
        .identity()
        .digest("billing-command-fingerprint", &document.to_string())
}

// --- The settlement's authority (the TS service's signature step) -------------

/// The derived provider secret: `digest("billing-provider-secret",
/// tenant ‖ "\0" ‖ provider)` rendered as its ASCII byte string. See the
/// module header for the whole recipe — the suite re-derives the same
/// bytes to mint claims.
fn provider_key_bytes(state: &AppState, tenant: &str, provider_id: &str) -> Vec<u8> {
    state
        .identity()
        .digest(
            "billing-provider-secret",
            &format!("{tenant}\0{provider_id}"),
        )
        .into_bytes()
}

/// `BillingService.verifyProviderEvent` — the signature check that runs
/// BEFORE any SQL. The claim is REBUILT from the received (already
/// parsed) fields, never from the body as a whole; a digest that does
/// not match — wrong key, tampered amount, different invoice, forged
/// outcome, or a value minted under another domain entirely — refuses
/// with ONE code, so nothing about WHY it failed reaches an
/// unauthenticated caller.
#[allow(clippy::result_large_err)]
fn verify_provider_event(
    state: &AppState,
    tenant: &str,
    mutation: &BillingMutation,
) -> Result<(), (StatusCode, ErpBody)> {
    let BillingCommand::RecordProviderEvent {
        invoice_id,
        provider_id,
        provider_event_id,
        outcome,
        amount_minor,
        currency,
        signature,
        ..
    } = &mutation.command
    else {
        // The service only calls this for the provider-event arm.
        return Ok(());
    };
    let key = provider_key_bytes(state, tenant, provider_id);
    let digest_result = provider_event_digest(
        &key,
        &PaymentEventClaim {
            tenant_id: tenant,
            invoice_id,
            provider_id,
            provider_event_id,
            outcome: outcome.as_str(),
            amount_minor: *amount_minor,
            currency,
        },
    );
    let expected = match digest_result {
        Ok(expected) => expected,
        // A short derived key cannot happen (64 ASCII bytes); the size
        // ceiling is unreachable through the grammar's own bounds. Both
        // refusal families fold closed — recorded unreachable.
        Err(SignatureError::Key) | Err(SignatureError::Size) => {
            return Err(forbidden());
        }
    };
    if !signatures_equal(&expected, signature) {
        // `Provider signature did not verify` — the TS `invalid_signature`.
        return Err(forbidden());
    }
    Ok(())
}

// --- Routes ----------------------------------------------------------------

/// `POST /erp/billing/commands` — submit (or replay) a billing command.
/// CSRF-bound like the other ERP write routes; the tenant comes only
/// from the body. The OK body is the BARE receipt — the TS api answers
/// `service.mutate` verbatim.
pub(crate) async fn billing_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<BillingSubmission>, JsonRejection>,
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
    match submit_billing_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/billing/commands/status?tenant=&requestId=` — the durable
/// status of one command id. Session-bound like the write route, it demands
/// the current `billing.read` grant and returns the receipt only to the
/// principal that recorded the command; anyone else is handed the `unknown`
/// document. The OK body is the commandStatus document verbatim.
pub(crate) async fn billing_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<BillingStatusQuery>, QueryRejection>,
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
    match billing_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- One transaction per request: the submit ---------------------------------

/// The submission pipeline — the port of `mutate()` driven by the host's
/// identity key, with the service layer's labor order: parse, the
/// authority verification BEFORE any SQL, then ONE transaction for the
/// whole request: member ladder, authorize, claim, execute, audit,
/// receipt binding. It commits in exactly one place; every refusal rolls
/// back explicitly, so no path can commit a half-applied command.
pub async fn submit_billing_command(
    state: &AppState,
    actor: ErpActor,
    submission: BillingSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    // 0. The boundary parse — the strict verdict the TS service's parse
    //    raises (400 `validation_error` in TS, the gate's shared 400
    //    carrier here).
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(invalid_submission()),
    };
    // 0b. The settlement's authority — BEFORE the transaction, always:
    //     an under-signed claim claims nothing.
    if let Err(answer) = verify_provider_event(state, submission.tenant.as_str(), &mutation) {
        return Ok(answer);
    }
    // 0c. The replay's key — over the mutation document alone.
    let fingerprint = fingerprint(state, &mutation);

    let mut transaction = begin_billing(state, submission.tenant.clone()).await?;

    // 1. The member ladder — ONE row: the principal's own membership,
    //    active, its permissions and scope, its OWN id as the owner (the
    //    TS envelope; no intersection step exists in billing).
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_billing_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };

    // 2. The claim → execute → audit → receipt pipeline. The commit at
    //    the end is the ONE commit; the idempotency reuse branch replays
    //    from the stored receipt surface through the same step function.
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_billing_command(
            connection,
            submission.tenant.as_str(),
            &principal,
            &access,
            &fingerprint,
            &mutation,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };

    transaction.commit().await.map_err(ErpGateError::from)?;
    // The TS api answers the receipt itself — no envelope, no replay
    // marker OUTSIDE the receipt (the receipt's own `idempotencyReplay`
    // member carries the replay's flag).
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&receipt)
                .expect("a typed receipt serializes back to its own JSON"),
        ),
    ))
}

// --- One transaction per request: the status read -----------------------------

/// The durable status of one command id — the read requires the current
/// `billing.read` grant and binds to the recorded initiator: the stored
/// receipt document for that principal, the bare `unknown` document (no
/// receipt fields) for every other caller, exactly as for a key that names
/// nothing. A lost/corrupt receipt is a conflict, never a fabricated
/// answer. The answer is the TS commandStatus document verbatim.
pub async fn billing_status_read(
    state: &AppState,
    actor: ErpActor,
    query: BillingStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_billing(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_billing_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };
    let status: BillingCommandStatus = {
        let connection = transaction.checked_executor().await?;
        // A receipt belongs to the actor that recorded it: the adapter
        // enforces `billing.read` and the stored `actor_id` in one statement.
        match billing_command_status(connection, query.tenant.as_str(), &access, query.request_id)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };
    // Read-only path: rolled back, never committed.
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&status).expect("a typed status serializes back to its own JSON"),
        ),
    ))
}
