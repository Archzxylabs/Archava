//! Party commands share the Organizations HTTP and request-ID namespace.
use super::organizations::{
    invalid_submission, map_purchase_result, ErpBody, OrganizationSubmission,
};
use super::{begin_organizations, ErpGateError};
use crate::router::AppState;
use archava_adapters::erp::parties;
use archava_domain::erp::{parties::parse, purchasing::PurchasePrincipal};
use axum::http::StatusCode;
use serde_json::json;
pub(super) async fn submit(
    state: &AppState,
    principal: PurchasePrincipal,
    input: OrganizationSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mutation = match parse(&input.mutation) {
        Ok(m) => m,
        Err(_) => return Ok(invalid_submission()),
    };
    let fingerprint=state.identity().digest("erp-parties-fingerprint",&json!({"domain":"parties","tenant":input.tenant.as_str(),"principal":principal,"mutation":mutation}).to_string());
    let mut tx = begin_organizations(state, input.tenant.clone()).await?;
    let access = match parties::access(
        tx.checked_executor().await?,
        input.tenant.as_str(),
        &principal,
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            let _ = tx.rollback().await;
            return map_purchase_result(e);
        }
    };
    let contact = match parties::uses_contact(
        tx.checked_executor().await?,
        input.tenant.as_str(),
        &access,
        &mutation.command,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return map_purchase_result(e);
        }
    };
    if contact {
        crate::own::require_erp_module(&mut tx, "crm-contacts")
            .await
            .map_err(ErpGateError::Configuration)?;
    }
    let receipt = match parties::record(
        tx.checked_executor().await?,
        input.tenant.as_str(),
        &principal,
        &access,
        &fingerprint,
        &mutation,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return map_purchase_result(e);
        }
    };
    if let Err(error) = tx.commit().await {
        return map_purchase_result(archava_adapters::erp::purchase_database_error(
            "organizations",
            match error {
                archava_adapters::rls::TenantContextError::Database { source, .. } => source,
                other => sqlx::Error::Protocol(other.to_string()),
            },
        ));
    }
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            json!({"decision":if receipt.replayed{"replayed"}else{"receipt"},"tenant":input.tenant.as_str(),"requestId":receipt.request_id,"receipt":receipt}),
        ),
    ))
}
