//! Command submission, idempotent replay, status queries, and bound approvals.

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use archava_core::actions::{ActionRegistry, ConfirmationMode};
use archava_core::roles::RoleName;
use archava_core::tenant::TenantId;
use archava_domain::authority::{ActionDenial, PolicyDecision, NEVER_AUTONOMOUS_LEVEL};

use crate::authority::{self, AuthorityError, MembershipKind};
use crate::identity::AuthenticatedSession;
use crate::router::{
    authenticate, authority_refusal, conflict, forbidden, query_rejection, rejection, status_for,
    unavailable, unknown_approval, unknown_request, AppState, CommandOutcome,
};

/// The bound approval's lifetime. An approval that can wait forever is an
/// approval that authorises a command nobody expects any more — the mandate's
/// contract names expiry as part of the binding, and this is the number.
pub const APPROVAL_TTL_SECONDS: i32 = 15 * 60;

/// The largest input this host will fingerprint into a receipt.
///
/// The receipt stores the input verbatim so authorized recovery can replay
/// what actually ran; an unbounded column would be a denial-of-service shape
/// dressed as a convenience.
pub const MAX_INPUT_BYTES: usize = 16 * 1024;

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

/// Mounts the commands, status, and approvals route family.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/commands", post(command))
        .route("/commands/status", get(command_status))
        .route("/approvals", post(approval_request))
        .route("/approvals/approve", post(approval_grant))
}

/// The reviewed-command route: extraction → session → grant → gate → write.
pub async fn command(
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
pub(crate) async fn execute_command(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: CommandSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.ledger().namespace;
    let ledger = state.ledger().ledger;
    let tenant = submission.tenant.clone();
    let fingerprint = authority::input_fingerprint(
        &submission.action_id,
        submission.revision,
        submission.input.as_deref(),
    );

    let mut transaction = authority::begin(state.pool(), *state.ledger(), tenant.clone()).await?;

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
        let decision = state.authorizer().evaluate(&request);
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
    let decision = state.authorizer().evaluate(&request);
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

/// `GET /commands/status`.
pub async fn command_status(
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
pub(crate) async fn status_receipt(
    state: &AppState,
    presented: &AuthenticatedSession,
    query: StatusQuery,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.ledger().namespace;
    let ledger = state.ledger().ledger;
    let mut transaction =
        authority::begin(state.pool(), *state.ledger(), query.tenant.clone()).await?;
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
pub(crate) fn receipt_read_refusal(
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
    match state.authorizer().evaluate(&request) {
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

/// `POST /approvals`: record the *need* for a human approval, bound to this
/// command. Stamping it is a separate request by a different principal.
pub async fn approval_request(
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

pub(crate) async fn request_approval(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: ApprovalSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.ledger().namespace;
    let ledger = state.ledger().ledger;
    let tenant = submission.tenant.clone();
    let fingerprint = authority::input_fingerprint(
        &submission.action_id,
        submission.revision,
        submission.input.as_deref(),
    );

    let mut transaction = authority::begin(state.pool(), *state.ledger(), tenant.clone()).await?;
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
    let decision = state.authorizer().evaluate(&request);
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
pub async fn approval_grant(
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

pub(crate) async fn grant_approval(
    state: &AppState,
    presented: &AuthenticatedSession,
    submission: ApproveSubmission,
) -> Result<(StatusCode, Json<CommandOutcome>), AuthorityError> {
    let namespace = state.ledger().namespace;
    let ledger = state.ledger().ledger;
    let tenant = submission.tenant.clone();

    let mut transaction = authority::begin(state.pool(), *state.ledger(), tenant.clone()).await?;
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
