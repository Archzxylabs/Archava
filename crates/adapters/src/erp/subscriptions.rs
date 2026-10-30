//! The subscriptions SQL adapter — the Archxy 2 port of
//! `packages/subscriptions-pg/src/repository.ts` (`execute()` +
//! `getCommandStatus()` + the read projections), the R07 entitlements
//! cohort (PACKET-008).
//!
//! ## Statement order (the TS port)
//!
//! The `execute()` ladder with the subscriptions vocabulary: the membership
//! rows (the actor's own row, and for a SERVICE principal the human it acts
//! for), the operation's own permission (`create`/`change_status`/
//! `grant_entitlement` → `subscriptions.write`; `record_payment` →
//! `subscriptions.payment`; `consume_usage` → `subscriptions.usage`), then
//! the arm's own row lock — and the `commands` claim pairing the TS
//! defines: SELECT the command row first, refuse a fingerprint mismatch on
//! an existing row, return a stored receipt as the idempotent REPLAY
//! (`idempotencyReplay: true`), claim (`INSERT`) only when the row is
//! absent, and fall THROUGH on a receipt-less row so a partial command
//! re-executes inside the same transaction. The replay gates on the
//! FINGERPRINT CELL alone — a second actor resubmitting an identical
//! `commandId` replays, exactly as the TS does.
//!
//! ## The corrected rule (audit A05 — P1)
//!
//! The TS lets a `draft` subscription's entitlements be consumed; the port
//! refuses it (`not_paid`, the 402 class). The policy itself lives in the
//! domain crate (`consumption_refusal` / `activation_refusal`) so the rule
//! is one pure function, not a branch buried in SQL.
//!
//! ## What the port tightens (recorded in the acceptance matrix)
//!
//! - The consumption arm reads its owning subscription FOR SHARE, so a
//!   concurrent cancel cannot slip between the status read and the usage
//!   write. The TS reads the row plain; the observable outcome is the same
//!   or stricter, never looser.
//! - The usage arm's duplicate idempotency key is a `conflict`, not the
//!   baseline's database failure: the TS lets the `usage_idempotency`
//!   UNIQUE constraint raise after the entitlement row was already
//!   updated, and the whole transaction rolls back — the caller sees a
//!   500-class failure for what is a retry of a request that already
//!   happened. The port refuses with the conflict class and records
//!   nothing.
//!
//! ## The sampled clock
//!
//! One `clock_timestamp()` sample per command (the R04/R05/R07 adapters'
//! family), bound into every stamping position of the arm's statements.

use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::subscriptions::{
    activation_refusal, consumption_refusal, ActivationRefusal, ConsumptionRefusal, Entitlement,
    EntitlementStatus, Subscription, SubscriptionAccess, SubscriptionCommand,
    SubscriptionCommandStatus, SubscriptionDetail, SubscriptionMutation, SubscriptionReceipt,
    SubscriptionStatus, UsageEvent,
};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::Row;
use thiserror::Error;

use super::commerce_kernel::load_locked_membership;
use super::{purchase_database_error, PurchaseError};

/// The ledger this adapter owns.
pub const SUBSCRIPTIONS_LEDGER: &str = "subscriptions";
/// The schema the statements ride.
pub const SUBSCRIPTIONS_NAMESPACE: &str = "archava_subscriptions";

/// The subscriptions refusal vocabulary — the TS `SubscriptionError` codes,
/// carried one-for-one so the host can answer the TS's own HTTP classes
/// (`past_due` is its own 402 class in the TS api's `codeMap`).
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// `forbidden` / `not_found` / `conflict` / `invalid_request` — the
    /// shared domain classes.
    #[error("subscription domain refusal: {0:?}")]
    Domain(ErpError),
    /// `not_paid` — the corrected refusal (audit A05): the owning
    /// subscription has never been paid, so it grants no consumption
    /// right. Its own label; the same 402 class as `past_due`.
    #[error("subscription is not paid")]
    NotPaid,
    /// `past_due` — the TS's own refusal (402 class).
    #[error("subscription is past due")]
    PastDue,
    /// `exhausted` — the quota is spent (409 class).
    #[error("subscription entitlement is exhausted")]
    Exhausted,
    /// `suspended` — the subscription or the entitlement is shut off (409
    /// class).
    #[error("subscription is suspended")]
    Suspended,
    /// `already_active` — the target equals the current status (409 class).
    #[error("subscription is already active")]
    AlreadyActive,
    /// A database failure the SQL-state whitelist did not clear.
    #[error("subscription database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<SubscriptionError> for PurchaseError {
    fn from(error: SubscriptionError) -> Self {
        match error {
            SubscriptionError::Domain(domain) => PurchaseError::Domain(domain),
            SubscriptionError::NotPaid
            | SubscriptionError::PastDue
            | SubscriptionError::Exhausted
            | SubscriptionError::Suspended
            | SubscriptionError::AlreadyActive => PurchaseError::Domain(ErpError::Conflict),
            SubscriptionError::Database { ledger, source } => {
                PurchaseError::Database { ledger, source }
            }
        }
    }
}

fn forbidden() -> SubscriptionError {
    SubscriptionError::Domain(ErpError::Forbidden)
}

fn not_found() -> SubscriptionError {
    SubscriptionError::Domain(ErpError::NotFound)
}

fn conflict() -> SubscriptionError {
    SubscriptionError::Domain(ErpError::Conflict)
}

fn database(error: sqlx::Error) -> SubscriptionError {
    SubscriptionError::Database {
        ledger: SUBSCRIPTIONS_LEDGER,
        source: error,
    }
}

// --- The context ---------------------------------------------------------------

/// The member ladder — the TS `transaction()`'s membership resolution.
///
/// A service principal may never widen past the human it acts for: the
/// permissions INTERSECT and the scope narrows to `assigned` when either
/// side is `assigned`.
pub async fn resolve_subscriptions_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<SubscriptionAccess, SubscriptionError> {
    let own = load_locked_membership(
        connection,
        SUBSCRIPTIONS_NAMESPACE,
        SUBSCRIPTIONS_LEDGER,
        tenant,
        &principal.id,
        archava_domain::erp::subscriptions::SUBSCRIPTION_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => SubscriptionError::Domain(domain),
        PurchaseError::Database { ledger, source } => {
            SubscriptionError::Database { ledger, source }
        }
    })?
    .ok_or_else(forbidden)?;
    if !own.active {
        return Err(forbidden());
    }
    // The TS: `actor.kind !== principal.kind` refuses.
    let principal_kind = match principal.kind {
        PrincipalKind::Human => ErpMembershipKind::Human,
        PrincipalKind::Service => ErpMembershipKind::Service,
    };
    if own.kind != principal_kind {
        return Err(forbidden());
    }
    let mut access = SubscriptionAccess {
        permissions: own.permissions.clone(),
        owner_id: principal.id.clone(),
        scope: match own.scope {
            ErpScope::All => "all".to_string(),
            ErpScope::Assigned => "assigned".to_string(),
        },
    };
    if principal.kind == PrincipalKind::Service {
        let Some(on_behalf_of) = principal.on_behalf_of.as_deref() else {
            return Err(forbidden());
        };
        let human = load_locked_membership(
            connection,
            SUBSCRIPTIONS_NAMESPACE,
            SUBSCRIPTIONS_LEDGER,
            tenant,
            on_behalf_of,
            archava_domain::erp::subscriptions::SUBSCRIPTION_PERMISSIONS,
        )
        .await
        .map_err(|error| match error {
            PurchaseError::Domain(domain) => SubscriptionError::Domain(domain),
            PurchaseError::Database { ledger, source } => {
                SubscriptionError::Database { ledger, source }
            }
        })?
        .ok_or_else(forbidden)?;
        if !human.active || human.kind != ErpMembershipKind::Human {
            return Err(forbidden());
        }
        access.permissions = own
            .permissions
            .iter()
            .filter(|held| human.permissions.contains(held))
            .cloned()
            .collect();
        access.owner_id = principal.on_behalf_of.clone().unwrap_or_default();
        access.scope = if own.scope == ErpScope::Assigned || human.scope == ErpScope::Assigned {
            "assigned".to_string()
        } else {
            "all".to_string()
        };
    }
    Ok(access)
}

// --- The claim -----------------------------------------------------------------

/// The command claim — SELECT first, refuse a fingerprint mismatch OR a
/// foreign recorded initiator, replay a stored receipt, claim only when
/// absent, fall through on a receipt-less row (the TS partial-command case).
enum Claim {
    Fresh,
    Replay(Box<SubscriptionReceipt>),
}

/// The `commands` row read + claim.
async fn claim_command(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    fingerprint: &str,
) -> Result<Claim, SubscriptionError> {
    let row = sqlx::query(
        "SELECT receipt, fingerprint, actor_id FROM archava_subscriptions.commands \
WHERE tenant_id=$1 AND request_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    if let Some(row) = row {
        let stored_fingerprint: String = row.try_get("fingerprint").map_err(database)?;
        // The recorded initiator: the receipt belongs to the actor that
        // claimed the command, so a same-grant rival refuses exactly like
        // a fingerprint mismatch (A1 — `actor_id=$3`).
        let recorded_actor: String = row.try_get("actor_id").map_err(database)?;
        if stored_fingerprint != fingerprint || recorded_actor != actor_id {
            return Err(conflict());
        }
        let receipt: Option<Value> = row.try_get("receipt").map_err(database)?;
        if let Some(receipt) = receipt {
            let parsed = SubscriptionReceipt::parse(&receipt, true)
                .map_err(|_| SubscriptionError::Domain(ErpError::Unavailable))?;
            return Ok(Claim::Replay(Box::new(parsed)));
        }
        return Ok(Claim::Fresh);
    }
    sqlx::query(
        "INSERT INTO archava_subscriptions.commands(tenant_id, request_id, actor_id, fingerprint) \
VALUES($1, $2, $3, $4)",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(Claim::Fresh)
}

/// The receipt binding — the one UPDATE the `command_guard` trigger allows.
async fn bind_receipt(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    receipt: &SubscriptionReceipt,
) -> Result<(), SubscriptionError> {
    let document = serde_json::to_value(receipt)
        .map_err(|_| SubscriptionError::Domain(ErpError::Unavailable))?;
    sqlx::query(
        "UPDATE archava_subscriptions.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3",
    )
    .bind(document)
    .bind(tenant)
    .bind(uuid_of(command_id))
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

// --- The arms ------------------------------------------------------------------

/// The whole command ladder — claim, arm, audit, receipt binding.
///
/// The commit is the HOST's (one transaction per request); this function
/// only speaks SQL.
pub async fn record_subscriptions_command(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    fingerprint: &str,
    mutation: &SubscriptionMutation,
) -> Result<SubscriptionReceipt, SubscriptionError> {
    // The per-command grant gates the CLAIM, not just the arm: a seat that is
    // still active but has lost the permission must not recover a stored
    // receipt (A1-FU3 - the revoked-grant replay bypass).
    require(access, mutation.command.required_permission())?;
    let command_id = mutation.command.command_id();
    match claim_command(
        connection,
        tenant,
        command_id,
        &access.owner_id,
        fingerprint,
    )
    .await?
    {
        Claim::Replay(receipt) => return Ok(*receipt),
        Claim::Fresh => {}
    }

    // The sampled clock — one instant per command, bound everywhere.
    let (sampled,): (String,) = sqlx::query_as(
        "SELECT to_char(clock_timestamp() at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;

    let receipt = match &mutation.command {
        SubscriptionCommand::CreateSubscription {
            command_id,
            contact_id,
            plan_code,
            plan_name,
            billing_interval,
            unit_price,
            currency,
            quantity,
            initial_entitlements,
        } => {
            let subscription_id = uuid_v4();
            let row = sqlx::query(
                "INSERT INTO archava_subscriptions.subscriptions(\
tenant_id, id, contact_id, plan_code, plan_name, status, billing_interval, \
unit_price, currency, quantity, owner_id, created_at, updated_at) \
VALUES($1,$2,$3,$4,$5,'draft',$6,$7,$8,$9,$10,$11::timestamptz,$11::timestamptz) RETURNING to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at",
            )
            .bind(tenant)
            .bind(subscription_id)
            .bind(contact_id)
            .bind(plan_code)
            .bind(plan_name)
            .bind(billing_interval)
            .bind(unit_price)
            .bind(currency)
            .bind(quantity)
            .bind(&access.owner_id)
            .bind(&sampled)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;
            let created_at: String = row.try_get("created_at").map_err(database)?;

            for entitlement in initial_entitlements {
                sqlx::query(
                    "INSERT INTO archava_subscriptions.entitlements(\
tenant_id, id, subscription_id, contact_id, feature_code, feature_name, \
entitlement_type, allocated_quantity, consumed_quantity, status, valid_from, \
valid_until, created_at, updated_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,0,'active',$9::timestamptz,$10::timestamptz,$9::timestamptz,$9::timestamptz)",
                )
                .bind(tenant)
                .bind(uuid_v4())
                .bind(subscription_id)
                .bind(contact_id)
                .bind(&entitlement.feature_code)
                .bind(&entitlement.feature_name)
                .bind(&entitlement.entitlement_type)
                .bind(entitlement.allocated_quantity)
                .bind(&sampled)
                .bind(&entitlement.valid_until)
                .execute(&mut *connection)
                .await
                .map_err(database)?;
            }

            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "create_subscription",
                Some(subscription_id),
                1,
                &format!("Created {plan_code}"),
            )
            .await?;

            SubscriptionReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "create_subscription".to_string(),
                subscription_id: Some(Some(subscription_id.to_string())),
                entitlement_id: None,
                usage_event_id: None,
                revision: Some(1),
                status: "draft".to_string(),
                idempotency_replay: false,
                executed_at: created_at,
            }
        }
        SubscriptionCommand::RecordPayment {
            command_id,
            subscription_id,
            payment_reference,
            amount_paid,
            period_start,
            period_end,
        } => {
            // The subscription's own row lock — the TS `FOR UPDATE`.
            let row = sqlx::query(
                "SELECT revision::bigint AS revision FROM archava_subscriptions.subscriptions \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(subscription_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;
            let revision: i64 = row.try_get("revision").map_err(database)?;
            let next_revision = revision + 1;
            let updated = sqlx::query(
                "UPDATE archava_subscriptions.subscriptions SET status='active', \
payment_reference=$1, current_period_start=$2::timestamptz, current_period_end=$3::timestamptz, revision=$4, \
updated_at=clock_timestamp() WHERE tenant_id=$5 AND id=$6 RETURNING to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at",
            )
            .bind(payment_reference)
            .bind(period_start)
            .bind(period_end)
            .bind(next_revision)
            .bind(tenant)
            .bind(uuid_of(subscription_id))
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;
            let updated_at: String = updated.try_get("updated_at").map_err(database)?;
            // The suspended entitlements of this subscription come back.
            sqlx::query(
                "UPDATE archava_subscriptions.entitlements SET status='active', \
updated_at=clock_timestamp() WHERE tenant_id=$1 AND subscription_id=$2 AND status='suspended'",
            )
            .bind(tenant)
            .bind(uuid_of(subscription_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "record_payment",
                Some(uuid_of(subscription_id)),
                next_revision,
                &format!("Payment confirmed: {payment_reference} ({amount_paid})"),
            )
            .await?;
            SubscriptionReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "record_payment".to_string(),
                subscription_id: Some(Some(subscription_id.clone())),
                entitlement_id: None,
                usage_event_id: None,
                revision: Some(next_revision),
                status: "active".to_string(),
                idempotency_replay: false,
                executed_at: updated_at,
            }
        }
        SubscriptionCommand::ConsumeUsage {
            command_id,
            entitlement_id,
            units,
            idempotency_key,
            evidence_ref,
        } => {
            let row = sqlx::query(
                "SELECT subscription_id::text AS subscription_id, contact_id, feature_code, allocated_quantity::bigint AS allocated_quantity, \
consumed_quantity::bigint AS consumed_quantity, status, to_char(valid_until at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_until FROM archava_subscriptions.entitlements \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(entitlement_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;
            let entitlement_subscription: Option<String> =
                row.try_get("subscription_id").map_err(database)?;
            let contact_id: String = row.try_get("contact_id").map_err(database)?;
            let feature_code: String = row.try_get("feature_code").map_err(database)?;
            let allocated: Option<i64> = row.try_get("allocated_quantity").map_err(database)?;
            let consumed: i64 = row.try_get("consumed_quantity").map_err(database)?;
            let status: String = row.try_get("status").map_err(database)?;
            let valid_until: Option<String> = row.try_get("valid_until").map_err(database)?;

            let entitlement_status = EntitlementStatus::parse(&status).ok_or_else(forbidden)?;
            // The owning subscription's status — FOR SHARE so a concurrent
            // cancel cannot slip between this read and the usage write.
            let subscription_status = match entitlement_subscription.as_deref() {
                None => None,
                Some(id) => {
                    let row = sqlx::query(
                        "SELECT status FROM archava_subscriptions.subscriptions \
WHERE tenant_id=$1 AND id=$2 FOR SHARE",
                    )
                    .bind(tenant)
                    .bind(uuid_of(id))
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(database)?;
                    match row {
                        None => None,
                        Some(row) => {
                            let value: String = row.try_get("status").map_err(database)?;
                            Some(SubscriptionStatus::parse(&value).ok_or_else(forbidden)?)
                        }
                    }
                }
            };

            // The corrected consumption right (audit A05).
            if let Some(refusal) = consumption_refusal(entitlement_status, subscription_status) {
                return Err(match refusal {
                    ConsumptionRefusal::NotPaid => SubscriptionError::NotPaid,
                    ConsumptionRefusal::PastDue => SubscriptionError::PastDue,
                    ConsumptionRefusal::EntitlementSuspended => SubscriptionError::Suspended,
                    ConsumptionRefusal::EntitlementRevoked => {
                        SubscriptionError::Domain(ErpError::Forbidden)
                    }
                    ConsumptionRefusal::EntitlementExhausted => SubscriptionError::Exhausted,
                    ConsumptionRefusal::SubscriptionNotRunning(_) => SubscriptionError::Suspended,
                });
            }
            // An expired validity window refuses as exhausted. The TS flips
            // the row to `exhausted` and then throws — the throw rolls the
            // flip back, so the observable state is the refusal alone; the
            // port refuses without writing the doomed row.
            if let Some(valid_until) = valid_until.as_deref() {
                if valid_until < sampled.as_str() {
                    return Err(SubscriptionError::Exhausted);
                }
            }
            let next_consumed = consumed + units;
            if let Some(allocated) = allocated {
                if next_consumed > allocated {
                    return Err(SubscriptionError::Exhausted);
                }
            }
            let next_status = match allocated {
                Some(allocated) if next_consumed >= allocated => "exhausted",
                _ => "active",
            };
            sqlx::query(
                "UPDATE archava_subscriptions.entitlements SET consumed_quantity=$1, \
status=$2, updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4",
            )
            .bind(next_consumed)
            .bind(next_status)
            .bind(tenant)
            .bind(uuid_of(entitlement_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let usage_id = uuid_v4();
            let inserted = sqlx::query(
                "INSERT INTO archava_subscriptions.usage_events(\
tenant_id, id, entitlement_id, subscription_id, contact_id, units_consumed, \
idempotency_key, evidence_ref, recorded_by, recorded_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::timestamptz) RETURNING to_char(recorded_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at",
            )
            .bind(tenant)
            .bind(usage_id)
            .bind(uuid_of(entitlement_id))
            .bind(entitlement_subscription.as_deref().map(uuid_of))
            .bind(&contact_id)
            .bind(units)
            .bind(idempotency_key)
            .bind(evidence_ref)
            .bind(&access.owner_id)
            .bind(&sampled)
            .fetch_one(&mut *connection)
            .await;
            let recorded_at = match inserted {
                Ok(row) => row.try_get("recorded_at").map_err(database)?,
                Err(error) => {
                    // The `usage_idempotency` UNIQUE violation is a RETRY of
                    // a request that already happened — a conflict, not a
                    // database failure. The baseline lets it raise after the
                    // entitlement row was already updated (and the whole
                    // transaction rolls back); the port refuses with the
                    // conflict class and records nothing.
                    if is_unique_violation(&error) {
                        return Err(conflict());
                    }
                    return Err(database(error));
                }
            };
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "consume_usage",
                entitlement_subscription.as_deref().map(uuid_of),
                1,
                &format!("Consumed {units} units from {feature_code}"),
            )
            .await?;
            SubscriptionReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "consume_usage".to_string(),
                subscription_id: Some(entitlement_subscription.clone()),
                entitlement_id: Some(Some(entitlement_id.to_string())),
                usage_event_id: Some(Some(usage_id.to_string())),
                revision: None,
                status: next_status.to_string(),
                idempotency_replay: false,
                executed_at: recorded_at,
            }
        }
        SubscriptionCommand::ChangeStatus {
            command_id,
            subscription_id,
            expected_revision,
            target_status,
            reason,
        } => {
            let row = sqlx::query(
                "SELECT status, revision::bigint AS revision, payment_reference FROM \
archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(subscription_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;
            let status: String = row.try_get("status").map_err(database)?;
            let revision: i64 = row.try_get("revision").map_err(database)?;
            let payment_reference: Option<String> =
                row.try_get("payment_reference").map_err(database)?;
            if revision != *expected_revision {
                return Err(conflict());
            }
            let current = SubscriptionStatus::parse(&status).ok_or_else(forbidden)?;
            // The corrected activation policy.
            if let Some(refusal) =
                activation_refusal(current, target_status, payment_reference.is_some())
            {
                return Err(match refusal {
                    ActivationRefusal::AlreadyActive => SubscriptionError::AlreadyActive,
                    ActivationRefusal::ActivationWithoutPayment => SubscriptionError::NotPaid,
                    ActivationRefusal::UnknownTarget => {
                        SubscriptionError::Domain(ErpError::Unavailable)
                    }
                    ActivationRefusal::IllegalTransition(_, _) => conflict(),
                });
            }
            let next_revision = revision + 1;
            let updated = sqlx::query(
                "UPDATE archava_subscriptions.subscriptions SET status=$1, revision=$2, \
updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4 RETURNING to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at",
            )
            .bind(target_status)
            .bind(next_revision)
            .bind(tenant)
            .bind(uuid_of(subscription_id))
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;
            let updated_at: String = updated.try_get("updated_at").map_err(database)?;
            // The status propagates to the subscription's entitlements.
            match target_status.as_str() {
                "canceled" => {
                    sqlx::query(
                        "UPDATE archava_subscriptions.entitlements SET status='revoked', \
updated_at=clock_timestamp() WHERE tenant_id=$1 AND subscription_id=$2 AND status != 'revoked'",
                    )
                    .bind(tenant)
                    .bind(uuid_of(subscription_id))
                    .execute(&mut *connection)
                    .await
                    .map_err(database)?;
                }
                "paused" => {
                    sqlx::query(
                        "UPDATE archava_subscriptions.entitlements SET status='suspended', \
updated_at=clock_timestamp() WHERE tenant_id=$1 AND subscription_id=$2 AND status != 'revoked'",
                    )
                    .bind(tenant)
                    .bind(uuid_of(subscription_id))
                    .execute(&mut *connection)
                    .await
                    .map_err(database)?;
                }
                "active" => {
                    sqlx::query(
                        "UPDATE archava_subscriptions.entitlements SET status='active', \
updated_at=clock_timestamp() WHERE tenant_id=$1 AND subscription_id=$2 AND status='suspended'",
                    )
                    .bind(tenant)
                    .bind(uuid_of(subscription_id))
                    .execute(&mut *connection)
                    .await
                    .map_err(database)?;
                }
                _ => {}
            }
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "change_status",
                Some(uuid_of(subscription_id)),
                next_revision,
                &format!("Changed status to {target_status}: {reason}"),
            )
            .await?;
            SubscriptionReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "change_status".to_string(),
                subscription_id: Some(Some(subscription_id.clone())),
                entitlement_id: None,
                usage_event_id: None,
                revision: Some(next_revision),
                status: target_status.clone(),
                idempotency_replay: false,
                executed_at: updated_at,
            }
        }
        SubscriptionCommand::GrantEntitlement {
            command_id,
            subscription_id,
            contact_id,
            feature_code,
            feature_name,
            entitlement_type,
            allocated_quantity,
            valid_until,
        } => {
            let entitlement_id = uuid_v4();
            let row = sqlx::query(
                "INSERT INTO archava_subscriptions.entitlements(\
tenant_id, id, subscription_id, contact_id, feature_code, feature_name, \
entitlement_type, allocated_quantity, consumed_quantity, status, valid_from, \
valid_until, created_at, updated_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,0,'active',$9::timestamptz,$10::timestamptz,$9::timestamptz,$9::timestamptz) RETURNING to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at",
            )
            .bind(tenant)
            .bind(entitlement_id)
            .bind(subscription_id.as_deref().map(uuid_of))
            .bind(contact_id)
            .bind(feature_code)
            .bind(feature_name)
            .bind(entitlement_type)
            .bind(allocated_quantity)
            .bind(&sampled)
            .bind(valid_until)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;
            let created_at: String = row.try_get("created_at").map_err(database)?;
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "grant_entitlement",
                subscription_id.as_deref().map(uuid_of),
                1,
                &format!("Granted {feature_code} to contact {contact_id}"),
            )
            .await?;
            SubscriptionReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "grant_entitlement".to_string(),
                subscription_id: Some(subscription_id.clone()),
                entitlement_id: Some(Some(entitlement_id.to_string())),
                usage_event_id: None,
                revision: None,
                status: "active".to_string(),
                idempotency_replay: false,
                executed_at: created_at,
            }
        }
    };

    bind_receipt(connection, tenant, command_id, &receipt).await?;
    Ok(receipt)
}

/// The audit row — one per command, append-only by the migration's trigger.
#[allow(clippy::too_many_arguments)]
async fn audit(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    operation: &str,
    subscription_id: Option<uuid::Uuid>,
    revision: i64,
    reason: &str,
) -> Result<(), SubscriptionError> {
    sqlx::query(
        "INSERT INTO archava_subscriptions.audit(tenant_id, request_id, actor_id, operation, \
subscription_id, revision, reason, occurred_at) VALUES($1,$2,$3,$4,$5,$6,$7,clock_timestamp())",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(actor_id)
    .bind(operation)
    .bind(subscription_id)
    .bind(revision)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

/// The permission gate — the TS `grant(access, …)`.
fn require(access: &SubscriptionAccess, wanted: &str) -> Result<(), SubscriptionError> {
    if access.holds(wanted) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// The status read — the TS `getCommandStatus()`.
pub async fn subscriptions_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    command_id: &str,
) -> Result<SubscriptionCommandStatus, SubscriptionError> {
    // The read permission, then the recorded initiator (the envelope's
    // `owner_id` — the on-behalf-of human for a delegated service). The
    // actor rides the WHERE clause: a foreign key answers `unknown`.
    require(access, "subscriptions.read")?;
    let row = sqlx::query(
        "SELECT receipt FROM archava_subscriptions.commands \
         WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(access.owner_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(row) = row else {
        return Ok(SubscriptionCommandStatus::Unknown);
    };
    let receipt: Option<Value> = row.try_get("receipt").map_err(database)?;
    let Some(receipt) = receipt else {
        return Ok(SubscriptionCommandStatus::Unknown);
    };
    let parsed = SubscriptionReceipt::parse(&receipt, false)
        .map_err(|_| SubscriptionError::Domain(ErpError::Unavailable))?;
    Ok(SubscriptionCommandStatus::Recorded {
        receipt: Box::new(parsed),
    })
}

// --- The read projections ------------------------------------------------------

/// The page query — the TS `subscriptionPageQuerySchema`.
#[derive(Debug, Clone)]
pub struct SubscriptionPageQuery {
    pub limit: i64,
    pub offset: i64,
    pub status: Option<String>,
    pub contact_id: Option<String>,
}

/// The list read — the TS `listSubscriptions()`, bounded and ordered.
pub async fn list_subscriptions(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    query: &SubscriptionPageQuery,
) -> Result<Vec<Subscription>, SubscriptionError> {
    require(access, "subscriptions.read")?;
    // The bound parameters carry their own types — a JSON value would reach
    // the column as its JSON spelling, not as the text the column wants.
    enum Param {
        Text(String),
        Count(i64),
    }
    let mut conditions = vec!["tenant_id = $1".to_string()];
    let mut params: Vec<Param> = vec![Param::Text(tenant.to_string())];
    if let Some(status) = query.status.as_deref() {
        params.push(Param::Text(status.to_string()));
        conditions.push(format!("status = ${}", params.len()));
    }
    if let Some(contact_id) = query.contact_id.as_deref() {
        params.push(Param::Text(contact_id.to_string()));
        conditions.push(format!("contact_id = ${}", params.len()));
    }
    if access.scope == "assigned" {
        params.push(Param::Text(access.owner_id.clone()));
        conditions.push(format!("owner_id = ${}", params.len()));
    }
    params.push(Param::Count(query.limit));
    let limit_index = params.len();
    params.push(Param::Count(query.offset));
    let offset_index = params.len();
    let statement = format!(
        "SELECT id::text AS id, tenant_id, contact_id, plan_code, plan_name, status, billing_interval, \
unit_price::bigint AS unit_price, currency, quantity::bigint AS quantity, to_char(current_period_start at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS current_period_start, to_char(current_period_end at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS current_period_end, \
cancel_at_period_end, payment_reference, owner_id, revision::bigint AS revision, to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
FROM archava_subscriptions.subscriptions WHERE {} ORDER BY created_at DESC \
LIMIT ${} OFFSET ${}",
        conditions.join(" AND "),
        limit_index,
        offset_index
    );
    let mut builder = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()));
    for param in &params {
        builder = match param {
            Param::Text(value) => builder.bind(value),
            Param::Count(value) => builder.bind(*value),
        };
    }
    let rows = builder
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    rows.iter().map(map_subscription).collect()
}

/// The detail read — the TS `getSubscriptionDetail()`.
pub async fn read_subscription_detail(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    id: &str,
) -> Result<Option<SubscriptionDetail>, SubscriptionError> {
    require(access, "subscriptions.read")?;
    let row = sqlx::query(
        "SELECT id::text AS id, tenant_id, contact_id, plan_code, plan_name, status, billing_interval, \
unit_price::bigint AS unit_price, currency, quantity::bigint AS quantity, to_char(current_period_start at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS current_period_start, to_char(current_period_end at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS current_period_end, \
cancel_at_period_end, payment_reference, owner_id, revision::bigint AS revision, to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(uuid_of(id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let owner_id: String = row.try_get("owner_id").map_err(database)?;
    // The `assigned` scope narrows to the owner's own rows — the TS hides
    // the row entirely rather than answering a 403.
    if access.scope == "assigned" && owner_id != access.owner_id {
        return Ok(None);
    }
    let subscription = map_subscription(&row)?;
    let entitlement_rows = sqlx::query(
        "SELECT id::text AS id, tenant_id, subscription_id::text AS subscription_id, contact_id, feature_code, feature_name, \
entitlement_type, allocated_quantity::bigint AS allocated_quantity, consumed_quantity::bigint AS consumed_quantity, status, to_char(valid_from at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_from, \
to_char(valid_until at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_until, to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at FROM archava_subscriptions.entitlements \
WHERE tenant_id=$1 AND subscription_id=$2 ORDER BY created_at ASC",
    )
    .bind(tenant)
    .bind(uuid_of(id))
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;
    let entitlements = entitlement_rows
        .iter()
        .map(map_entitlement)
        .collect::<Result<Vec<Entitlement>, SubscriptionError>>()?;
    Ok(Some(SubscriptionDetail {
        subscription,
        entitlements,
    }))
}

/// The subscription entitlement read — the TS `listEntitlements()`: the
/// entitlements of ONE subscription, addressed by subscription id.
///
/// The target kind is the function, not the value. A contact reference has
/// its own read ([`read_contact_entitlements`]), so no caller has to guess a
/// kind from the shape of an id — the old `valid_uuid` inference sent every
/// UUID-shaped contact reference into this branch and answered `not_found`.
///
/// The parent probe is the gate: with the owner predicate inside it a foreign
/// subscription answers exactly like a missing one, and no child row is read
/// for either.
pub async fn read_entitlements(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    subscription_id: &str,
) -> Result<Vec<Entitlement>, SubscriptionError> {
    require(access, "subscriptions.read")?;
    // The id is parsed here, fallibly: `targetId` is an unrestricted query
    // string, so a malformed one must answer like a missing subscription
    // rather than panic inside the driver (`uuid_of` asserts its input).
    let Ok(subscription_uuid) = uuid::Uuid::parse_str(subscription_id) else {
        return Err(not_found());
    };
    let probe = if access.scope == "assigned" {
        "SELECT id FROM archava_subscriptions.subscriptions \
         WHERE tenant_id=$1 AND id=$2 AND owner_id=$3"
    } else {
        "SELECT id FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2"
    };
    let mut exists = sqlx::query(probe).bind(tenant).bind(subscription_uuid);
    if access.scope == "assigned" {
        exists = exists.bind(access.owner_id.as_str());
    }
    let exists = exists
        .fetch_optional(&mut *connection)
        .await
        .map_err(database)?;
    if exists.is_none() {
        return Err(not_found());
    }
    let rows = sqlx::query(
        "SELECT id::text AS id, tenant_id, subscription_id::text AS subscription_id, contact_id, feature_code, feature_name, \
entitlement_type, allocated_quantity::bigint AS allocated_quantity, consumed_quantity::bigint AS consumed_quantity, status, to_char(valid_from at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_from, \
to_char(valid_until at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_until, to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at FROM archava_subscriptions.entitlements \
WHERE tenant_id=$1 AND subscription_id=$2 ORDER BY created_at ASC",
    )
    .bind(tenant)
    .bind(subscription_uuid)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;
    rows.iter().map(map_entitlement).collect()
}

/// The contact entitlement read — the TS `listContactEntitlements()`: every
/// entitlement granted to one contact reference, across that contact's
/// subscriptions.
///
/// An entitlement carries no owner of its own; ownership lives on the
/// subscription it points at, so `assigned` scope adds the owner predicate as
/// an `EXISTS` on that tenant-scoped parent — never an inner join, because
/// `subscription_id` is nullable: a subscription-less entitlement stays
/// visible to `all` scope (it has no owner to inherit) and is hidden from
/// `assigned`. An unknown contact is `[]`, indistinguishable from a contact
/// whose entitlements all belong to someone else.
pub async fn read_contact_entitlements(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    contact_id: &str,
) -> Result<Vec<Entitlement>, SubscriptionError> {
    require(access, "subscriptions.read")?;
    let statement = if access.scope == "assigned" {
        "SELECT e.id::text AS id, e.tenant_id, e.subscription_id::text AS subscription_id, e.contact_id, e.feature_code, e.feature_name, \
e.entitlement_type, e.allocated_quantity::bigint AS allocated_quantity, e.consumed_quantity::bigint AS consumed_quantity, e.status, to_char(e.valid_from at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_from, \
to_char(e.valid_until at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_until, to_char(e.created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(e.updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at FROM archava_subscriptions.entitlements e \
WHERE e.tenant_id=$1 AND e.contact_id=$2 AND EXISTS (SELECT 1 FROM archava_subscriptions.subscriptions p \
WHERE p.tenant_id=e.tenant_id AND p.id=e.subscription_id AND p.owner_id=$3) ORDER BY e.created_at DESC"
    } else {
        "SELECT e.id::text AS id, e.tenant_id, e.subscription_id::text AS subscription_id, e.contact_id, e.feature_code, e.feature_name, \
e.entitlement_type, e.allocated_quantity::bigint AS allocated_quantity, e.consumed_quantity::bigint AS consumed_quantity, e.status, to_char(e.valid_from at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_from, \
to_char(e.valid_until at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS valid_until, to_char(e.created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, to_char(e.updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at FROM archava_subscriptions.entitlements e \
WHERE e.tenant_id=$1 AND e.contact_id=$2 ORDER BY e.created_at DESC"
    };
    let mut read = sqlx::query(statement).bind(tenant).bind(contact_id);
    if access.scope == "assigned" {
        read = read.bind(access.owner_id.as_str());
    }
    let rows = read.fetch_all(&mut *connection).await.map_err(database)?;
    rows.iter().map(map_entitlement).collect()
}

/// The usage read — the TS `listUsageEvents()`.
///
/// A usage event is reachable only through its entitlement, and an
/// entitlement only through its subscription, so `assigned` scope binds the
/// owner with an `EXISTS` over that whole path: a foreign entitlement answers
/// `[]` exactly like a missing one, instead of an error that would confirm the
/// entitlement exists. The join inside the `EXISTS` is the point — an
/// entitlement with no subscription inherits no owner, so `all` keeps its
/// history and `assigned` hides it.
pub async fn read_usage_events(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SubscriptionAccess,
    entitlement_id: &str,
) -> Result<Vec<UsageEvent>, SubscriptionError> {
    require(access, "subscriptions.read")?;
    let statement = if access.scope == "assigned" {
        "SELECT u.id::text AS id, u.tenant_id, u.entitlement_id::text AS entitlement_id, u.subscription_id::text AS subscription_id, u.contact_id, u.units_consumed::bigint AS units_consumed, \
u.idempotency_key, u.evidence_ref, u.recorded_by, to_char(u.recorded_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at FROM \
archava_subscriptions.usage_events u WHERE u.tenant_id=$1 AND u.entitlement_id=$2 \
AND EXISTS (SELECT 1 FROM archava_subscriptions.entitlements e \
JOIN archava_subscriptions.subscriptions p ON p.tenant_id=e.tenant_id AND p.id=e.subscription_id \
WHERE e.tenant_id=u.tenant_id AND e.id=u.entitlement_id AND p.owner_id=$3) \
ORDER BY u.recorded_at DESC"
    } else {
        "SELECT u.id::text AS id, u.tenant_id, u.entitlement_id::text AS entitlement_id, u.subscription_id::text AS subscription_id, u.contact_id, u.units_consumed::bigint AS units_consumed, \
u.idempotency_key, u.evidence_ref, u.recorded_by, to_char(u.recorded_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at FROM \
archava_subscriptions.usage_events u WHERE u.tenant_id=$1 AND u.entitlement_id=$2 \
ORDER BY u.recorded_at DESC"
    };
    let mut read = sqlx::query(statement)
        .bind(tenant)
        .bind(uuid_of(entitlement_id));
    if access.scope == "assigned" {
        read = read.bind(access.owner_id.as_str());
    }
    let rows = read.fetch_all(&mut *connection).await.map_err(database)?;
    rows.iter().map(map_usage_event).collect()
}

// --- The row mappers ------------------------------------------------------------

fn map_subscription(row: &sqlx::postgres::PgRow) -> Result<Subscription, SubscriptionError> {
    Ok(Subscription {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        contact_id: row.try_get("contact_id").map_err(database)?,
        plan_code: row.try_get("plan_code").map_err(database)?,
        plan_name: row.try_get("plan_name").map_err(database)?,
        status: row.try_get("status").map_err(database)?,
        billing_interval: row.try_get("billing_interval").map_err(database)?,
        unit_price: row.try_get("unit_price").map_err(database)?,
        currency: row.try_get("currency").map_err(database)?,
        quantity: row.try_get("quantity").map_err(database)?,
        current_period_start: row.try_get("current_period_start").map_err(database)?,
        current_period_end: row.try_get("current_period_end").map_err(database)?,
        cancel_at_period_end: row.try_get("cancel_at_period_end").map_err(database)?,
        payment_reference: row.try_get("payment_reference").map_err(database)?,
        owner_id: row.try_get("owner_id").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

fn map_entitlement(row: &sqlx::postgres::PgRow) -> Result<Entitlement, SubscriptionError> {
    Ok(Entitlement {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        subscription_id: row.try_get("subscription_id").map_err(database)?,
        contact_id: row.try_get("contact_id").map_err(database)?,
        feature_code: row.try_get("feature_code").map_err(database)?,
        feature_name: row.try_get("feature_name").map_err(database)?,
        entitlement_type: row.try_get("entitlement_type").map_err(database)?,
        allocated_quantity: row.try_get("allocated_quantity").map_err(database)?,
        consumed_quantity: row.try_get("consumed_quantity").map_err(database)?,
        status: row.try_get("status").map_err(database)?,
        valid_from: row.try_get("valid_from").map_err(database)?,
        valid_until: row.try_get("valid_until").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

fn map_usage_event(row: &sqlx::postgres::PgRow) -> Result<UsageEvent, SubscriptionError> {
    Ok(UsageEvent {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        entitlement_id: row.try_get("entitlement_id").map_err(database)?,
        subscription_id: row.try_get("subscription_id").map_err(database)?,
        contact_id: row.try_get("contact_id").map_err(database)?,
        units_consumed: row.try_get("units_consumed").map_err(database)?,
        idempotency_key: row.try_get("idempotency_key").map_err(database)?,
        evidence_ref: row.try_get("evidence_ref").map_err(database)?,
        recorded_by: row.try_get("recorded_by").map_err(database)?,
        recorded_at: row.try_get("recorded_at").map_err(database)?,
    })
}

// --- Helpers -------------------------------------------------------------------

/// A fresh identifier — the TS `randomUUID()`.
fn uuid_v4() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

/// The uuid a grammar-admitted id string carries. The columns are `uuid`
/// columns: a text bind raises `operator does not exist: uuid = text`.
fn uuid_of(value: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(value).expect("the grammar admits only uuid ids")
}

/// The unique-violation test — SQLSTATE 23505.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(inner) => inner.code().as_deref() == Some("23505"),
        _ => false,
    }
}

// --- The kernel -----------------------------------------------------------------

/// The ledger's own readiness facts — the port of the TS repository's
/// `create()` guards: a NON-OWNER runtime role, the exact migration
/// ledger, and six FORCED tenant policies.
pub async fn assert_subscriptions_kernel(
    pool: &sqlx::PgPool,
    subscriptions_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
WHERE relnamespace='archava_subscriptions'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("subscriptions runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Subscriptions requires a non-owner runtime".to_string());
    }
    let actual: Vec<(String, String)> = sqlx::query_as(
        "SELECT id,checksum FROM archava_subscriptions.schema_migrations ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("unsupported subscriptions ledger read: {error}"))?;
    if actual.len() != subscriptions_ledger.len()
        || actual.iter().zip(subscriptions_ledger).any(|(a, b)| a != b)
    {
        return Err("Unsupported subscriptions migration checksum".to_string());
    }
    let names: &[&str] = &[
        "memberships",
        "subscriptions",
        "entitlements",
        "usage_events",
        "commands",
        "audit",
    ][..];
    let statement = "SELECT count(*)::integer AS count FROM pg_class WHERE \
relnamespace='archava_subscriptions'::regnamespace AND relname=ANY($1::text[]) \
AND relrowsecurity AND relforcerowsecurity";
    let (count,): (i32,) = sqlx::query_as(statement)
        .bind(names)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("subscriptions policy check failed: {error}"))?;
    if count as usize != names.len() {
        return Err("Subscriptions tenant policies must be forced".to_string());
    }
    Ok(())
}

/// The database-error helper the shared crate exposes — re-exported so the
/// host can map a failure the same way the other ERP ledgers do.
pub fn subscriptions_database_error(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(SUBSCRIPTIONS_LEDGER, error)
}
