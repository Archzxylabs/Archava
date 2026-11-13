//! Finite business-tool dispatch. All calls use the existing authenticated
//! HTTP handlers, so automation cannot bypass grants, approvals or domain rules.
use crate::router::AppState;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
pub(crate) type Response = (StatusCode, Json<Value>);
pub(crate) fn tool(target: &str) -> Option<(&str, &str)> {
    let (module, action) = target.split_once('.')?;
    archava_domain::operator::module(module)?["actions"]
        .as_array()?
        .iter()
        .find(|a| a["id"] == action && a["providerOnly"] != true)?;
    Some((module, action))
}
pub(crate) fn stable_key(namespace: &str, identity: &str) -> Uuid {
    let digest = Sha256::digest(format!("archava-action/v1:{namespace}:{identity}").as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 15) | 128;
    bytes[8] = (bytes[8] & 63) | 128;
    Uuid::from_bytes(bytes)
}
pub(crate) fn refused(status: StatusCode, reason: &str) -> Response {
    (
        status,
        Json(
            json!({"reason":reason,"message":"The business action could not be completed. Review its inputs, current permissions and status."}),
        ),
    )
}
pub(crate) async fn execute(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &str,
    key: Uuid,
    target: &str,
    values: &Value,
) -> Response {
    let Some((module, action)) = tool(target) else {
        return refused(StatusCode::BAD_REQUEST, "unsupported_business_tool");
    };
    let body = match archava_domain::operator::envelope(
        module,
        tenant,
        &key.to_string(),
        action,
        values,
    ) {
        Ok(v) => v,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"reason":"invalid_business_input","message":message})),
            )
        }
    };
    let bytes = match serde_json::to_vec(&body) {
        Ok(v) => v,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_business_input"),
    };
    macro_rules! call {
        ($handler:path) => {
            $handler(
                State(state.clone()),
                headers.clone(),
                Json::from_bytes(&bytes),
            )
            .await
            .into_response()
        };
    }
    let response = match module {
        "crm" => call!(crate::erp::crm::crm_command),
        "commerce" => call!(crate::erp::commerce::command),
        "purchasing" => call!(crate::erp::purchasing::purchase_command),
        "billing" => call!(crate::erp::billing::billing_command),
        "support" => call!(crate::erp::support::support_command),
        "organizations" => call!(crate::erp::organizations::organization_command),
        "proposals" => call!(crate::erp::proposals::proposal_command),
        "projects" => call!(crate::erp::projects::project_command),
        "foodservice" => call!(crate::erp::foodservice::food_service_command),
        "scheduling" => call!(crate::erp::scheduling::scheduling_command),
        "accommodation" => call!(crate::erp::accommodation::accommodation_command),
        "assets" => call!(crate::erp::assets::asset_command),
        "subscriptions" => call!(crate::erp::subscriptions::subscription_command),
        "attendance" => call!(crate::erp::attendance::attendance_command),
        "laundry" => call!(crate::erp::laundry::laundry_command),
        "field" => call!(crate::erp::field_services::field_command),
        "fulfillment" => call!(crate::erp::fulfillment::fulfillment_command),
        "education" => call!(crate::erp::formal_education::education_command),
        "accounting" => call!(crate::erp::accounting::command),
        _ => return refused(StatusCode::BAD_REQUEST, "unsupported_business_tool"),
    };
    let status = response.status();
    let mut body = match axum::body::to_bytes(response.into_body(), 1_048_576).await {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    if status.is_success() && module == "accommodation" {
        let Ok(tenant) = archava_core::tenant::TenantId::new(tenant) else {
            return refused(StatusCode::BAD_REQUEST, "invalid_business_tenant");
        };
        let (status, verified) = crate::erp::accommodation::accommodation_status(
            State(state.clone()),
            headers.clone(),
            Ok(Query(crate::erp::accommodation::AccommodationStatusQuery {
                tenant,
                request_id: key,
            })),
        )
        .await;
        let crate::erp::accommodation::ErpBody::Doc(verified) = verified else {
            return refused(StatusCode::BAD_GATEWAY, "unconfirmed_business_receipt");
        };
        if status != StatusCode::OK
            || verified["requestId"] != key.to_string()
            || verified["receipt"] != body
        {
            return refused(StatusCode::BAD_GATEWAY, "unconfirmed_business_receipt");
        }
        body = verified;
    }
    if status.is_success() {
        let receipt = body.get("receipt").unwrap_or(&body);
        let reference = receipt["requestId"]
            .as_str()
            .or_else(|| receipt["commandId"].as_str())
            .or_else(|| body["requestId"].as_str())
            // CRM's established public envelope uses snake_case for this key.
            .or_else(|| body["request_id"].as_str());
        if reference != Some(key.to_string().as_str()) {
            return refused(StatusCode::BAD_GATEWAY, "unconfirmed_business_receipt");
        }
    }
    (status, Json(body))
}
