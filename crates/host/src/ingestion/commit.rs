//! Atomic import effects reuse the real domain writers inside the plan transaction.
use super::*;
use archava_adapters::erp::{commerce, commerce_kernel::resolve_commerce_context};
use archava_domain::erp::{
    commerce::CommerceCommand,
    purchasing::{PrincipalKind, PurchasePrincipal},
};
pub(super) async fn persist(
    state: &AppState,
    headers: &HeaderMap,
    input: CommitRequest,
) -> Result<Value, Response> {
    if !input.confirmed {
        return Err(refused(StatusCode::BAD_REQUEST, "confirmation_required"));
    }
    let session = authenticate(state, headers, true)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let mut tx = workflows::begin(state, &input.tenant).await?;
    let held = workflows::load(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "import",
        input.batch_id,
        Some(&input.plan_hash),
    )
    .await?;
    let module = held.plan["module"]
        .as_str()
        .ok_or_else(|| workflows::unavailable("invalid import module"))?;
    let permission = held.plan["permission"]
        .as_str()
        .ok_or_else(|| workflows::unavailable("invalid import permission"))?;
    workflows::authorize(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        module,
        permission,
    )
    .await?;
    if module != "crm" {
        crate::own::require_erp_module(&mut tx, "archava_commerce")
            .await
            .map_err(crate::own::OwnRefusal::response)?;
    }
    if let Some(receipt) = held.receipt {
        tx.commit().await.map_err(workflows::unavailable)?;
        return Ok(receipt);
    }
    let rows = held.plan["commands"]
        .as_array()
        .ok_or_else(|| workflows::unavailable("invalid stored commands"))?;
    let actor = crate::erp::crm::CrmActor::Human(session.principal_id.clone());
    let principal = PurchasePrincipal {
        kind: PrincipalKind::Human,
        id: session.principal_id,
        on_behalf_of: None,
    };
    let mut receipts = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let key = crate::business::stable_key("import", &format!("{}:{index}", input.batch_id));
        if module == "crm" {
            let command: crate::erp::crm::CrmCommand =
                serde_json::from_value(row["command"].clone()).map_err(workflows::unavailable)?;
            if !command.valid() {
                return Err(refused(StatusCode::BAD_REQUEST, "invalid_import_record"));
            }
            let access = crate::erp::crm::authorize(
                tx.checked_executor()
                    .await
                    .map_err(workflows::unavailable)?,
                &input.tenant,
                &actor,
            )
            .await
            .map_err(|e| {
                let (s, b) = crate::router::authority_refusal(&e);
                (s, Json(json!(b.0)))
            })?;
            let fingerprint = crate::erp::crm::fingerprint(state, &input.tenant, &actor, &command);
            let execution = archava_adapters::erp::crm::execute_command(
                &mut tx,
                &input.tenant,
                &actor,
                &access,
                key,
                &command,
                &fingerprint,
            )
            .await
            .map_err(|e| {
                tracing::warn!(error=?e,"import CRM effect refused");
                refused(StatusCode::CONFLICT, "import_record_refused")
            })?;
            match execution {
                archava_adapters::erp::crm::CommandExecution::Committed { receipt, .. } => {
                    receipts.push(receipt)
                }
                _ => return Err(refused(StatusCode::CONFLICT, "unconfirmed_import_record")),
            }
        } else {
            let command: CommerceCommand =
                serde_json::from_value(row["command"].clone()).map_err(workflows::unavailable)?;
            let conn = tx
                .checked_executor()
                .await
                .map_err(workflows::unavailable)?;
            let access = resolve_commerce_context(&mut *conn, input.tenant.as_str(), &principal)
                .await
                .map_err(workflows::domain_error)?;
            if let Some(revision) = row["stockRevision"].as_i64() {
                if !access
                    .access
                    .holds(&["catalog.read", "stock.read", "stock.write"])
                    || access.access.scope != archava_domain::erp::ErpScope::All
                {
                    return Err(refused(StatusCode::FORBIDDEN, "missing_permission"));
                }
                let product = row["command"]["productId"]
                    .as_str()
                    .ok_or_else(|| workflows::unavailable("missing stock product"))?;
                let balance:Option<(i32,i32)>=sqlx::query_as("SELECT quantity,revision FROM archava_commerce.stock_balances WHERE tenant_id=$1 AND product_id=$2::uuid FOR UPDATE").bind(input.tenant.as_str()).bind(product).fetch_optional(&mut *conn).await.map_err(workflows::unavailable)?;
                if balance != Some((0, revision as i32)) {
                    return Err(refused(StatusCode::CONFLICT, "opening_stock_changed"));
                }
            }
            let fingerprint = state.identity().digest(
                "import-commerce-v1",
                &json!({"tenant":input.tenant.as_str(),"principal":principal,"command":command})
                    .to_string(),
            );
            receipts.push(json!(commerce::record(
                conn,
                input.tenant.as_str(),
                &principal,
                &access,
                key,
                &fingerprint,
                &command
            )
            .await
            .map_err(workflows::domain_error)?));
        }
    }
    let receipt = json!({"batch_id":input.batch_id,"plan_hash":held.hash,"status":"committed_atomic","entity_kind":held.plan["entity_kind"],"committed_records_count":rows.len(),"receipts":receipts});
    workflows::record(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        input.batch_id,
        &receipt,
    )
    .await?;
    tx.commit().await.map_err(workflows::unavailable)?;
    Ok(receipt)
}
