//! The laundry SQL adapter — the Archxy 2 port of `packages/laundry-pg/src/repository.ts`:
//! the laundry ledger's command ladder (claim → authorize → arm → history → receipt)
//! and read projections (access, orders list, detail, lines, process events, findings, command status).
//!
//! ## Invariants preserved
//!
//! - RLS tenant isolation and member ladder (kind check, human vs service intersection).
//! - Machine events (`wash`, `dry`, `press`) require `laundry.process`; other mutations require `laundry.write`.
//! - Read queries require `laundry.read`; `assigned` scope restricts orders to `owner_id = access.owner_id`.
//! - Sequential processing chain: begins with `intake` and strictly advances `PROCESS_STAGE`.
//! - Quality gate: moving to `ready` requires `unreconciled_count == 0` AND an existing `quality_check` process event.
//! - Exception findings (`partial`, `rejected`) increment `unreconciled_count`; a finding with valid `reconciles_id` decrements it.
//! - Optimistic locking: `change_order_status` requires matching `expectedRevision` and advances revision by 1.
//! - Idempotency replay with fingerprint verification and actor isolation.

use archava_domain::erp::laundry::{
    Finding, FindingCategory, FindingDisposition, LaundryAccess, LaundryCommandStatus,
    LaundryMutation, LaundryOrder, LaundryReceipt, LineUnit, OrderDetail, OrderKind, OrderLine,
    OrderPageQuery, OrderStatus, ProcessEvent, ProcessRecord, LAUNDRY_PERMISSIONS,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sqlx::{PgConnection, Row};
use thiserror::Error;
use uuid::Uuid;

use super::commerce_kernel::load_locked_membership;
use super::PurchaseError;

pub const LAUNDRY_LEDGER: &str = "laundry";
pub const LAUNDRY_NAMESPACE: &str = "archava_laundry";

#[derive(Debug, Error)]
pub enum LaundryError {
    #[error("laundry domain refusal: {0:?}")]
    Domain(ErpError),
    #[error("laundry document refused by the grammar")]
    Invalid,
    #[error("laundry bad request: {0}")]
    BadRequest(&'static str),
    #[error("laundry invalid transition: {0}")]
    InvalidTransition(&'static str),
    #[error("laundry process out of order: {0}")]
    ProcessOutOfOrder(&'static str),
    #[error("laundry unreconciled exceptions: {0}")]
    UnreconciledExceptions(&'static str),
    #[error("laundry database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<LaundryError> for PurchaseError {
    fn from(error: LaundryError) -> Self {
        match error {
            LaundryError::Domain(domain) => PurchaseError::Domain(domain),
            LaundryError::Invalid => PurchaseError::Domain(ErpError::Unavailable),
            LaundryError::BadRequest(_) => PurchaseError::Domain(ErpError::Unavailable),
            LaundryError::InvalidTransition(_) => PurchaseError::Domain(ErpError::Conflict),
            LaundryError::ProcessOutOfOrder(_) => PurchaseError::Domain(ErpError::Conflict),
            LaundryError::UnreconciledExceptions(_) => PurchaseError::Domain(ErpError::Conflict),
            LaundryError::Database { ledger, source } => PurchaseError::Database { ledger, source },
        }
    }
}

fn forbidden() -> LaundryError {
    LaundryError::Domain(ErpError::Forbidden)
}

fn not_found() -> LaundryError {
    LaundryError::Domain(ErpError::NotFound)
}

fn conflict() -> LaundryError {
    LaundryError::Domain(ErpError::Conflict)
}

fn database(error: sqlx::Error) -> LaundryError {
    LaundryError::Database {
        ledger: LAUNDRY_LEDGER,
        source: error,
    }
}

fn uuid_of(value: &str) -> Result<Uuid, LaundryError> {
    Uuid::parse_str(value).map_err(|_| LaundryError::Invalid)
}

fn require_permission(access: &LaundryAccess, permission: &str) -> Result<(), LaundryError> {
    if access.permissions.iter().any(|p| p == permission) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub async fn resolve_laundry_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<LaundryAccess, LaundryError> {
    let own = load_locked_membership(
        connection,
        LAUNDRY_NAMESPACE,
        LAUNDRY_LEDGER,
        tenant,
        &principal.id,
        LAUNDRY_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => LaundryError::Domain(domain),
        PurchaseError::Database { ledger, source } => LaundryError::Database { ledger, source },
    })?
    .ok_or_else(forbidden)?;

    if !own.active {
        return Err(forbidden());
    }

    let principal_kind = match principal.kind {
        PrincipalKind::Human => ErpMembershipKind::Human,
        PrincipalKind::Service => ErpMembershipKind::Service,
    };
    if own.kind != principal_kind {
        return Err(forbidden());
    }

    let mut permissions = own.permissions.clone();
    let mut scope = own.scope;
    let owner_id = principal.id.clone();

    if principal.kind == PrincipalKind::Service {
        let human_id = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
        let human = load_locked_membership(
            connection,
            LAUNDRY_NAMESPACE,
            LAUNDRY_LEDGER,
            tenant,
            human_id,
            LAUNDRY_PERMISSIONS,
        )
        .await
        .map_err(|error| match error {
            PurchaseError::Domain(domain) => LaundryError::Domain(domain),
            PurchaseError::Database { ledger, source } => LaundryError::Database { ledger, source },
        })?
        .ok_or_else(forbidden)?;

        if !human.active || human.kind != ErpMembershipKind::Human {
            return Err(forbidden());
        }

        permissions.retain(|p| human.permissions.iter().any(|hp| hp == p));
        if human.scope == ErpScope::Assigned {
            scope = ErpScope::Assigned;
        }
    }

    Ok(LaundryAccess {
        permissions,
        owner_id,
        scope,
    })
}

enum Claim {
    Fresh,
    Replay(Box<LaundryReceipt>),
}

async fn claim_command(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    fingerprint: &str,
) -> Result<Claim, LaundryError> {
    let cmd_uuid = uuid_of(command_id)?;
    let inserted = sqlx::query(
        "INSERT INTO archava_laundry.commands(tenant_id, request_id, actor_id, fingerprint) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT DO NOTHING RETURNING request_id",
    )
    .bind(tenant)
    .bind(cmd_uuid)
    .bind(actor_id)
    .bind(fingerprint)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if inserted.is_some() {
        return Ok(Claim::Fresh);
    }

    let existing = sqlx::query(
        "SELECT actor_id, fingerprint, receipt FROM archava_laundry.commands \
         WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(cmd_uuid)
    .bind(actor_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?
    .ok_or_else(conflict)?;

    let existing_actor: String = existing.try_get("actor_id").map_err(database)?;
    let existing_fingerprint: String = existing.try_get("fingerprint").map_err(database)?;
    if existing_actor != actor_id || existing_fingerprint != fingerprint {
        return Err(conflict());
    }

    let raw_receipt: Option<Value> = existing.try_get("receipt").map_err(database)?;
    let raw_receipt = raw_receipt.ok_or_else(conflict)?;
    let mut receipt: LaundryReceipt =
        serde_json::from_value(raw_receipt).map_err(|_| LaundryError::Invalid)?;
    receipt.idempotency_replay = true;

    Ok(Claim::Replay(Box::new(receipt)))
}

#[allow(clippy::too_many_arguments)]
async fn append_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    order_id: Uuid,
    revision: i32,
    reason: &str,
) -> Result<(), LaundryError> {
    sqlx::query(
        "INSERT INTO archava_laundry.audit(tenant_id, request_id, actor_id, operation, order_id, revision, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(order_id)
    .bind(revision)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    Ok(())
}

pub async fn record_laundry_command(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    fingerprint: &str,
    mutation: &LaundryMutation,
) -> Result<LaundryReceipt, LaundryError> {
    require_permission(access, mutation.required_permission())?;

    match claim_command(
        connection,
        tenant,
        mutation.command_id(),
        &access.owner_id,
        fingerprint,
    )
    .await?
    {
        Claim::Replay(receipt) => return Ok(*receipt),
        Claim::Fresh => {}
    }

    let cmd_uuid = uuid_of(mutation.command_id())?;

    let receipt = match mutation {
        LaundryMutation::CreateOrder {
            order_code,
            kind,
            customer_ref,
            currency,
            lines,
            ..
        } => {
            let order_id = Uuid::new_v4();
            let mut total_minor: i64 = 0;
            let mut item_count: i64 = 0;
            let mut weight_grams: i64 = 0;

            struct PreparedLine {
                id: Uuid,
                item_ref: String,
                quantity: i32,
                unit: LineUnit,
                unit_price_minor: i32,
                line_total_minor: i32,
            }

            let mut prepared_lines = Vec::with_capacity(lines.len());
            for line in lines {
                let line_total = (line.quantity as i64) * (line.unit_price_minor as i64);
                total_minor += line_total;
                match line.unit {
                    LineUnit::Piece => item_count += line.quantity as i64,
                    LineUnit::Gram => weight_grams += line.quantity as i64,
                }
                if line_total > i32::MAX as i64 {
                    return Err(LaundryError::BadRequest("Line total overflow"));
                }
                prepared_lines.push(PreparedLine {
                    id: Uuid::new_v4(),
                    item_ref: line.item_ref.clone(),
                    quantity: line.quantity,
                    unit: line.unit,
                    unit_price_minor: line.unit_price_minor,
                    line_total_minor: line_total as i32,
                });
            }

            if total_minor > i32::MAX as i64
                || item_count > i32::MAX as i64
                || weight_grams > i32::MAX as i64
            {
                return Err(LaundryError::BadRequest(
                    "Order totals exceed supported integer range",
                ));
            }

            let inserted_order = sqlx::query(
                "INSERT INTO archava_laundry.orders(
                    tenant_id, id, order_code, kind, customer_ref, status, currency,
                    total_minor, item_count, weight_grams, owner_id
                 ) VALUES ($1, $2, $3, $4, $5, 'received', $6, $7, $8, $9, $10)
                 RETURNING to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(order_id)
            .bind(order_code)
            .bind(kind.as_str())
            .bind(customer_ref.as_deref())
            .bind(currency)
            .bind(total_minor as i32)
            .bind(item_count as i32)
            .bind(weight_grams as i32)
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = inserted_order.try_get("executed_at").map_err(database)?;

            for line in prepared_lines {
                sqlx::query(
                    "INSERT INTO archava_laundry.order_lines(
                        tenant_id, id, order_id, item_ref, quantity, unit,
                        unit_price_minor, line_total_minor, recorded_by
                     ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                )
                .bind(tenant)
                .bind(line.id)
                .bind(order_id)
                .bind(line.item_ref)
                .bind(line.quantity)
                .bind(line.unit.as_str())
                .bind(line.unit_price_minor)
                .bind(line.line_total_minor)
                .bind(&access.owner_id)
                .execute(&mut *connection)
                .await
                .map_err(database)?;
            }

            append_audit(
                connection,
                tenant,
                cmd_uuid,
                &access.owner_id,
                "create_order",
                order_id,
                1,
                &format!("Created {currency} order {order_code}"),
            )
            .await?;

            LaundryReceipt {
                command_id: mutation.command_id().to_string(),
                tenant_id: tenant.to_string(),
                action: "create_order".to_string(),
                order_id: Some(order_id.to_string()),
                process_id: None,
                finding_id: None,
                status: "received".to_string(),
                revision: Some(1),
                idempotency_replay: false,
                executed_at,
            }
        }
        LaundryMutation::RecordProcess {
            order_id,
            event,
            note,
            evidence_ref,
            ..
        } => {
            let o_uuid = uuid_of(order_id)?;
            let order_row = sqlx::query(
                "SELECT status, revision FROM archava_laundry.orders \
                 WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(o_uuid)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let status_str: String = order_row.try_get("status").map_err(database)?;
            let order_status = OrderStatus::parse(&status_str).ok_or_else(conflict)?;
            if !order_status.is_open() {
                return Err(LaundryError::InvalidTransition(
                    "Evidence cannot be recorded on a returned or canceled order",
                ));
            }
            let revision: i32 = order_row.try_get("revision").map_err(database)?;

            let history = sqlx::query(
                "SELECT event FROM archava_laundry.process_events \
                 WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC",
            )
            .bind(tenant)
            .bind(o_uuid)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?;

            if history.is_empty() {
                if *event != ProcessEvent::Intake {
                    return Err(LaundryError::ProcessOutOfOrder(
                        "Processing chain must begin with intake",
                    ));
                }
            } else {
                let mut max_stage = -1;
                for row in &history {
                    let ev_str: String = row.try_get("event").map_err(database)?;
                    if let Some(ev) = ProcessEvent::parse(&ev_str) {
                        let st = ev.stage();
                        if st > max_stage {
                            max_stage = st;
                        }
                    }
                }
                let cur_stage = event.stage();
                if cur_stage <= max_stage {
                    return Err(LaundryError::ProcessOutOfOrder(
                        "Process event must strictly advance sequence",
                    ));
                }
            }

            let process_id = Uuid::new_v4();
            let proc_row = sqlx::query(
                "INSERT INTO archava_laundry.process_events(
                    tenant_id, id, order_id, event, note, evidence_ref, recorded_by
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                 RETURNING to_char(recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(process_id)
            .bind(o_uuid)
            .bind(event.as_str())
            .bind(note.as_deref())
            .bind(evidence_ref.as_deref())
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = proc_row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                cmd_uuid,
                &access.owner_id,
                "record_process",
                o_uuid,
                revision,
                &format!("Recorded process step {}", event.as_str()),
            )
            .await?;

            LaundryReceipt {
                command_id: mutation.command_id().to_string(),
                tenant_id: tenant.to_string(),
                action: "record_process".to_string(),
                order_id: Some(order_id.clone()),
                process_id: Some(process_id.to_string()),
                finding_id: None,
                status: event.as_str().to_string(),
                revision: Some(revision),
                idempotency_replay: false,
                executed_at,
            }
        }
        LaundryMutation::RecordFinding {
            order_id,
            category,
            disposition,
            quantity,
            reason,
            evidence_ref,
            reconciles_id,
            ..
        } => {
            let o_uuid = uuid_of(order_id)?;
            let order_row = sqlx::query(
                "SELECT status, revision, unreconciled_count FROM archava_laundry.orders \
                 WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(o_uuid)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let status_str: String = order_row.try_get("status").map_err(database)?;
            let order_status = OrderStatus::parse(&status_str).ok_or_else(conflict)?;
            if !order_status.is_open() {
                return Err(LaundryError::InvalidTransition(
                    "Evidence cannot be recorded on a returned or canceled order",
                ));
            }

            let revision: i32 = order_row.try_get("revision").map_err(database)?;
            let mut unreconciled_count: i32 =
                order_row.try_get("unreconciled_count").map_err(database)?;

            if disposition.is_exception() {
                unreconciled_count += 1;
            }

            let rec_uuid = match reconciles_id {
                Some(r_id) => {
                    let r_uuid = uuid_of(r_id)?;
                    let _parent_finding = sqlx::query(
                        "SELECT id FROM archava_laundry.findings \
                         WHERE tenant_id=$1 AND id=$2 AND order_id=$3",
                    )
                    .bind(tenant)
                    .bind(r_uuid)
                    .bind(o_uuid)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(database)?
                    .ok_or_else(|| {
                        LaundryError::InvalidTransition(
                            "Reconciled finding not found on this order",
                        )
                    })?;

                    let already_reconciled = sqlx::query(
                        "SELECT 1 FROM archava_laundry.findings \
                         WHERE tenant_id=$1 AND reconciles_id=$2",
                    )
                    .bind(tenant)
                    .bind(r_uuid)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(database)?;

                    if already_reconciled.is_some() {
                        return Err(conflict());
                    }

                    unreconciled_count = (unreconciled_count - 1).max(0);
                    Some(r_uuid)
                }
                None => None,
            };

            sqlx::query(
                "UPDATE archava_laundry.orders SET unreconciled_count=$1, updated_at=clock_timestamp() \
                 WHERE tenant_id=$2 AND id=$3",
            )
            .bind(unreconciled_count)
            .bind(tenant)
            .bind(o_uuid)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let finding_id = Uuid::new_v4();
            let finding_row = sqlx::query(
                "INSERT INTO archava_laundry.findings(
                    tenant_id, id, order_id, category, disposition, quantity,
                    reason, evidence_ref, reconciles_id, recorded_by
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 RETURNING to_char(recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(finding_id)
            .bind(o_uuid)
            .bind(category.as_str())
            .bind(disposition.as_str())
            .bind(quantity)
            .bind(reason)
            .bind(evidence_ref.as_deref())
            .bind(rec_uuid)
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = finding_row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                cmd_uuid,
                &access.owner_id,
                "record_finding",
                o_uuid,
                revision,
                &format!(
                    "Recorded {} finding ({}): {}",
                    category.as_str(),
                    disposition.as_str(),
                    reason
                ),
            )
            .await?;

            LaundryReceipt {
                command_id: mutation.command_id().to_string(),
                tenant_id: tenant.to_string(),
                action: "record_finding".to_string(),
                order_id: Some(order_id.clone()),
                process_id: None,
                finding_id: Some(finding_id.to_string()),
                status: disposition.as_str().to_string(),
                revision: Some(revision),
                idempotency_replay: false,
                executed_at,
            }
        }
        LaundryMutation::ChangeOrderStatus {
            order_id,
            expected_revision,
            target_status,
            reason,
            ..
        } => {
            let o_uuid = uuid_of(order_id)?;
            let order_row = sqlx::query(
                "SELECT status, revision, unreconciled_count FROM archava_laundry.orders \
                 WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(o_uuid)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_rev: i32 = order_row.try_get("revision").map_err(database)?;
            if cur_rev != *expected_revision {
                return Err(conflict());
            }

            let cur_status_str: String = order_row.try_get("status").map_err(database)?;
            let cur_status = OrderStatus::parse(&cur_status_str).ok_or_else(conflict)?;

            if !cur_status.can_transition_to(*target_status) {
                return Err(LaundryError::InvalidTransition(
                    "Order cannot transition to target status",
                ));
            }

            if *target_status == OrderStatus::Ready {
                let unreconciled: i32 =
                    order_row.try_get("unreconciled_count").map_err(database)?;
                if unreconciled > 0 {
                    return Err(LaundryError::UnreconciledExceptions(
                        "Quality findings must be reconciled before order is marked ready",
                    ));
                }

                let qc = sqlx::query(
                    "SELECT 1 FROM archava_laundry.process_events \
                     WHERE tenant_id=$1 AND order_id=$2 AND event='quality_check' LIMIT 1",
                )
                .bind(tenant)
                .bind(o_uuid)
                .fetch_optional(&mut *connection)
                .await
                .map_err(database)?;

                if qc.is_none() {
                    return Err(LaundryError::InvalidTransition(
                        "A quality check must be recorded before order is marked ready",
                    ));
                }
            }

            let next_rev = cur_rev + 1;
            let updated = sqlx::query(
                "UPDATE archava_laundry.orders \
                 SET status=$1, revision=$2, updated_at=clock_timestamp() \
                 WHERE tenant_id=$3 AND id=$4 \
                 RETURNING to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(target_status.as_str())
            .bind(next_rev)
            .bind(tenant)
            .bind(o_uuid)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = updated.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                cmd_uuid,
                &access.owner_id,
                "change_order_status",
                o_uuid,
                next_rev,
                reason,
            )
            .await?;

            LaundryReceipt {
                command_id: mutation.command_id().to_string(),
                tenant_id: tenant.to_string(),
                action: "change_order_status".to_string(),
                order_id: Some(order_id.clone()),
                process_id: None,
                finding_id: None,
                status: target_status.as_str().to_string(),
                revision: Some(next_rev),
                idempotency_replay: false,
                executed_at,
            }
        }
    };

    let receipt_json = serde_json::to_value(&receipt).map_err(|_| LaundryError::Invalid)?;
    sqlx::query(
        "UPDATE archava_laundry.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3",
    )
    .bind(receipt_json)
    .bind(tenant)
    .bind(cmd_uuid)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    Ok(receipt)
}

pub async fn laundry_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command_id: &str,
) -> Result<LaundryCommandStatus, LaundryError> {
    let access = resolve_laundry_context(connection, tenant, principal).await?;
    require_permission(&access, "laundry.read")?;

    let cmd_uuid = match Uuid::parse_str(command_id) {
        Ok(u) => u,
        Err(_) => return Ok(LaundryCommandStatus::Unknown),
    };

    let row = sqlx::query(
        "SELECT actor_id, receipt FROM archava_laundry.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(cmd_uuid)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let Some(row) = row else {
        return Ok(LaundryCommandStatus::Unknown);
    };

    let actor_id: String = row.try_get("actor_id").map_err(database)?;
    if actor_id != access.owner_id {
        return Ok(LaundryCommandStatus::Unknown);
    }

    let receipt_val: Option<Value> = row.try_get("receipt").map_err(database)?;
    let Some(receipt_val) = receipt_val else {
        return Ok(LaundryCommandStatus::Unknown);
    };

    let receipt: LaundryReceipt =
        serde_json::from_value(receipt_val).map_err(|_| LaundryError::Invalid)?;
    Ok(LaundryCommandStatus::Recorded {
        receipt: Box::new(receipt),
    })
}

fn map_order(row: &sqlx::postgres::PgRow, tenant: &str) -> Result<LaundryOrder, LaundryError> {
    let id_uuid: Uuid = row.try_get("id").map_err(database)?;
    let kind_str: String = row.try_get("kind").map_err(database)?;
    let status_str: String = row.try_get("status").map_err(database)?;

    Ok(LaundryOrder {
        id: id_uuid.to_string(),
        tenant_id: tenant.to_string(),
        order_code: row.try_get("order_code").map_err(database)?,
        kind: OrderKind::parse(&kind_str).ok_or_else(|| LaundryError::Invalid)?,
        customer_ref: row.try_get("customer_ref").map_err(database)?,
        status: OrderStatus::parse(&status_str).ok_or_else(|| LaundryError::Invalid)?,
        currency: row.try_get("currency").map_err(database)?,
        total_minor: row.try_get("total_minor").map_err(database)?,
        item_count: row.try_get("item_count").map_err(database)?,
        weight_grams: row.try_get("weight_grams").map_err(database)?,
        unreconciled_count: row.try_get("unreconciled_count").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

pub async fn list_laundry_orders(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    query: &OrderPageQuery,
) -> Result<Vec<LaundryOrder>, LaundryError> {
    require_permission(access, "laundry.read")?;

    let is_assigned = access.scope == ErpScope::Assigned;
    let status_val = query.status.map(|s| s.as_str());
    let kind_val = query.kind.map(|k| k.as_str());

    let rows = sqlx::query(
        "SELECT id, order_code, kind, customer_ref, status, currency, total_minor, \
                item_count, weight_grams, unreconciled_count, revision, \
                to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_laundry.orders \
         WHERE tenant_id=$1 \
           AND ($2::text IS NULL OR status = $2) \
           AND ($3::text IS NULL OR kind = $3) \
           AND (NOT $4::boolean OR owner_id = $5) \
         ORDER BY created_at ASC, id ASC \
         LIMIT $6 OFFSET $7",
    )
    .bind(tenant)
    .bind(status_val)
    .bind(kind_val)
    .bind(is_assigned)
    .bind(&access.owner_id)
    .bind(query.limit)
    .bind(query.offset)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_order(&row, tenant)?);
    }
    Ok(out)
}

pub async fn get_laundry_order_detail(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    order_id: &str,
) -> Result<Option<OrderDetail>, LaundryError> {
    require_permission(access, "laundry.read")?;
    let o_uuid = match Uuid::parse_str(order_id) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };

    let is_assigned = access.scope == ErpScope::Assigned;
    let order_row = sqlx::query(
        "SELECT id, order_code, kind, customer_ref, status, currency, total_minor, \
                item_count, weight_grams, unreconciled_count, revision, \
                to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_laundry.orders \
         WHERE tenant_id=$1 AND id=$2 \
           AND (NOT $3::boolean OR owner_id = $4)",
    )
    .bind(tenant)
    .bind(o_uuid)
    .bind(is_assigned)
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let Some(order_row) = order_row else {
        return Ok(None);
    };

    let order = map_order(&order_row, tenant)?;
    let lines = list_laundry_lines(connection, tenant, access, order_id).await?;
    let process = list_laundry_process(connection, tenant, access, order_id).await?;
    let findings = list_laundry_findings(connection, tenant, access, order_id).await?;

    Ok(Some(OrderDetail {
        order,
        lines,
        process,
        findings,
    }))
}

pub async fn list_laundry_lines(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    order_id: &str,
) -> Result<Vec<OrderLine>, LaundryError> {
    require_permission(access, "laundry.read")?;
    let o_uuid = match Uuid::parse_str(order_id) {
        Ok(u) => u,
        Err(_) => return Ok(Vec::new()),
    };
    let is_assigned = access.scope == ErpScope::Assigned;

    let rows = sqlx::query(
        "SELECT c.id, c.order_id, c.item_ref, c.quantity, c.unit, c.unit_price_minor, \
                c.line_total_minor, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_laundry.order_lines c \
         JOIN archava_laundry.orders p ON p.tenant_id = c.tenant_id AND p.id = c.order_id \
         WHERE c.tenant_id=$1 AND c.order_id=$2 \
           AND (NOT $3::boolean OR p.owner_id = $4) \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(o_uuid)
    .bind(is_assigned)
    .bind(&access.owner_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let id_uuid: Uuid = row.try_get("id").map_err(database)?;
        let unit_str: String = row.try_get("unit").map_err(database)?;
        out.push(OrderLine {
            id: id_uuid.to_string(),
            tenant_id: tenant.to_string(),
            order_id: order_id.to_string(),
            item_ref: row.try_get("item_ref").map_err(database)?,
            quantity: row.try_get("quantity").map_err(database)?,
            unit: LineUnit::parse(&unit_str).ok_or_else(|| LaundryError::Invalid)?,
            unit_price_minor: row.try_get("unit_price_minor").map_err(database)?,
            line_total_minor: row.try_get("line_total_minor").map_err(database)?,
            recorded_by: row.try_get("recorded_by").map_err(database)?,
            recorded_at: row.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(out)
}

pub async fn list_laundry_process(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    order_id: &str,
) -> Result<Vec<ProcessRecord>, LaundryError> {
    require_permission(access, "laundry.read")?;
    let o_uuid = match Uuid::parse_str(order_id) {
        Ok(u) => u,
        Err(_) => return Ok(Vec::new()),
    };
    let is_assigned = access.scope == ErpScope::Assigned;

    let rows = sqlx::query(
        "SELECT c.id, c.order_id, c.event, c.note, c.evidence_ref, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_laundry.process_events c \
         JOIN archava_laundry.orders p ON p.tenant_id = c.tenant_id AND p.id = c.order_id \
         WHERE c.tenant_id=$1 AND c.order_id=$2 \
           AND (NOT $3::boolean OR p.owner_id = $4) \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(o_uuid)
    .bind(is_assigned)
    .bind(&access.owner_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let id_uuid: Uuid = row.try_get("id").map_err(database)?;
        let ev_str: String = row.try_get("event").map_err(database)?;
        out.push(ProcessRecord {
            id: id_uuid.to_string(),
            tenant_id: tenant.to_string(),
            order_id: order_id.to_string(),
            event: ProcessEvent::parse(&ev_str).ok_or_else(|| LaundryError::Invalid)?,
            note: row.try_get("note").map_err(database)?,
            evidence_ref: row.try_get("evidence_ref").map_err(database)?,
            recorded_by: row.try_get("recorded_by").map_err(database)?,
            recorded_at: row.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(out)
}

pub async fn list_laundry_findings(
    connection: &mut PgConnection,
    tenant: &str,
    access: &LaundryAccess,
    order_id: &str,
) -> Result<Vec<Finding>, LaundryError> {
    require_permission(access, "laundry.read")?;
    let o_uuid = match Uuid::parse_str(order_id) {
        Ok(u) => u,
        Err(_) => return Ok(Vec::new()),
    };
    let is_assigned = access.scope == ErpScope::Assigned;

    let rows = sqlx::query(
        "SELECT c.id, c.order_id, c.category, c.disposition, c.quantity, c.reason, \
                c.evidence_ref, c.reconciles_id, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_laundry.findings c \
         JOIN archava_laundry.orders p ON p.tenant_id = c.tenant_id AND p.id = c.order_id \
         WHERE c.tenant_id=$1 AND c.order_id=$2 \
           AND (NOT $3::boolean OR p.owner_id = $4) \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(o_uuid)
    .bind(is_assigned)
    .bind(&access.owner_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let id_uuid: Uuid = row.try_get("id").map_err(database)?;
        let rec_uuid: Option<Uuid> = row.try_get("reconciles_id").map_err(database)?;
        let cat_str: String = row.try_get("category").map_err(database)?;
        let disp_str: String = row.try_get("disposition").map_err(database)?;

        out.push(Finding {
            id: id_uuid.to_string(),
            tenant_id: tenant.to_string(),
            order_id: order_id.to_string(),
            category: FindingCategory::parse(&cat_str).ok_or_else(|| LaundryError::Invalid)?,
            disposition: FindingDisposition::parse(&disp_str)
                .ok_or_else(|| LaundryError::Invalid)?,
            quantity: row.try_get("quantity").map_err(database)?,
            reason: row.try_get("reason").map_err(database)?,
            evidence_ref: row.try_get("evidence_ref").map_err(database)?,
            reconciles_id: rec_uuid.map(|u| u.to_string()),
            recorded_by: row.try_get("recorded_by").map_err(database)?,
            recorded_at: row.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(out)
}
