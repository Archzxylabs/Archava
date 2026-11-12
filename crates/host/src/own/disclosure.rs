//! Owner disclosure approvals bind the complete policy, operation and live profile.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const APPROVAL_LIFETIME_MS: i64 = 10 * 60 * 1000;
pub(super) const PROJECTION_VERSION: &str = "archava-business-provider-context/v2";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisclosureApproval {
    pub expires_at: i64,
    pub signature: String,
}

pub(crate) fn provider_profile(state: &AppState) -> Option<Value> {
    state.agent_config().planner.as_ref().map(|p| {
        let mut profile = p.disclosure_profile();
        profile["projectionVersion"] = json!(PROJECTION_VERSION);
        profile["accountBinding"] = json!(p.disclosure_account_tag(state.identity_key()));
        profile
    })
}
pub(crate) fn provider_binding(state: &AppState) -> Option<String> {
    provider_profile(state).map(|v| format!("{:x}", Sha256::digest(v.to_string().as_bytes())))
}
/// System policy read; actual record access still uses its own authorizer.
pub(crate) async fn current_policy(
    state: &AppState,
    conn: &mut PgConnection,
    tenant: &TenantId,
) -> Result<Option<archava_own::DisclosurePolicy>, OwnRefusal> {
    let config = current_config(conn, tenant).await?;
    Ok(config.ai_disclosure.filter(|p| {
        provider_binding(state).as_ref() == Some(&p.provider_binding)
            && p.valid_for(
                &archava_domain::operator::catalogue()["modules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect(),
            )
    }))
}
pub(super) fn description(state: &AppState, current: &OwnerConfig) -> Value {
    let binding = provider_binding(state);
    json!({"provider":provider_profile(state),"binding":binding,"approvedForCurrentProvider":current.ai_disclosure.as_ref().is_some_and(|p|Some(&p.provider_binding)==binding.as_ref()),"notice":"Sharing permission selects data for the configured AI provider. It does not grant business record access. Provider retention, residency and account-specific terms are not verified by this runtime."})
}
pub(super) async fn company_review(
    conn: &mut PgConnection,
    tenant: &TenantId,
    principal: &str,
    policy: Option<&archava_own::DisclosurePolicy>,
) -> Vec<Value> {
    let mut result = Vec::new();
    for id in policy.into_iter().flat_map(|p| p.companies.iter()) {
        let Ok(key) = uuid::Uuid::parse_str(id) else {
            continue;
        };
        let row = archava_adapters::erp::company_trade::resolve(
            conn,
            tenant.as_str(),
            &archava_domain::erp::purchasing::PurchasePrincipal::human(principal),
            key,
        )
        .await;
        result.push(match row {
            Ok(c) => {
                json!({"id":id,"name":c.name,"code":c.code,"revision":c.revision,"available":true})
            }
            Err(_) => json!({"id":id,"available":false}),
        });
    }
    result
}
pub(super) async fn validate_request(
    state: &AppState,
    conn: &mut PgConnection,
    tenant: &TenantId,
    principal: &str,
    request: &Request,
) -> Result<(), OwnRefusal> {
    let Some(DisclosureChange::Replace { policy }) = &request.ai_disclosure else {
        return Ok(());
    };
    if provider_binding(state).as_ref() != Some(&policy.provider_binding) {
        return Err(OwnRefusal::new(
            StatusCode::CONFLICT,
            "ai_provider_changed",
            "Review the currently configured AI provider before allowing business-data sharing.",
        ));
    }
    let actor = archava_domain::erp::purchasing::PurchasePrincipal::human(principal);
    for company in &policy.companies {
        let id = uuid::Uuid::parse_str(company).map_err(|_| {
            OwnRefusal::new(
                StatusCode::BAD_REQUEST,
                "invalid_company_scope",
                "Select a visible company for the sharing policy.",
            )
        })?;
        archava_adapters::erp::company_trade::resolve(conn, tenant.as_str(), &actor, id)
            .await
            .map_err(|_| {
                OwnRefusal::new(
                    StatusCode::FORBIDDEN,
                    "company_scope_unavailable",
                    "A selected company is unavailable under your current company permissions.",
                )
            })?;
    }
    Ok(())
}
fn required(current: &OwnerConfig, plan: &Plan) -> bool {
    current.ai_disclosure.is_some() || plan.config.ai_disclosure.is_some()
}
async fn now(conn: &mut PgConnection) -> Result<i64, OwnRefusal> {
    Ok(
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint")
            .fetch_one(conn)
            .await?,
    )
}
fn payload(review: &Review<'_>, expires_at: i64) -> Vec<u8> {
    serde_json::to_vec(&json!({"purpose":"owner-ai-disclosure/v1","tenant":review.tenant.as_str(),"principal":review.principal,"operation":review.operation,"currentConfig":review.current,"request":review.request,"plan":review.plan,"liveProviderBinding":provider_binding(review.state),"expiresAt":expires_at})).expect("owner approval payload is serializable")
}
fn mac(state: &AppState, bytes: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(state.identity_key())
        .expect("HMAC accepts the verified identity key");
    mac.update(b"owner-ai-disclosure\0");
    mac.update(bytes);
    mac
}
pub(super) struct Review<'a> {
    pub state: &'a AppState,
    pub tenant: &'a TenantId,
    pub principal: &'a str,
    pub operation: &'a str,
    pub current: &'a OwnerConfig,
    pub request: &'a Request,
    pub plan: &'a Plan,
}
pub(super) async fn mint(
    review: Review<'_>,
    conn: &mut PgConnection,
) -> Result<Option<DisclosureApproval>, OwnRefusal> {
    if review.plan.blocked.is_some() || !required(review.current, review.plan) {
        return Ok(None);
    }
    let expires_at = now(conn)
        .await?
        .checked_add(APPROVAL_LIFETIME_MS)
        .ok_or_else(OwnRefusal::database)?;
    let bytes = payload(&review, expires_at);
    Ok(Some(DisclosureApproval {
        expires_at,
        signature: URL_SAFE_NO_PAD.encode(mac(review.state, &bytes).finalize().into_bytes()),
    }))
}
pub(super) async fn verify(
    review: Review<'_>,
    conn: &mut PgConnection,
    presented: Option<&DisclosureApproval>,
) -> Result<(), OwnRefusal> {
    if !required(review.current, review.plan) {
        return Ok(());
    }
    let invalid = || {
        OwnRefusal::new(StatusCode::CONFLICT,"disclosure_approval_required","Refresh the data-sharing review and confirm its current provider, data categories and company scope.")
    };
    let proof = presented.ok_or_else(invalid)?;
    let clock = now(conn).await?;
    if proof.expires_at < clock || proof.expires_at > clock.saturating_add(APPROVAL_LIFETIME_MS) {
        return Err(invalid());
    }
    let signature = URL_SAFE_NO_PAD
        .decode(&proof.signature)
        .map_err(|_| invalid())?;
    let bytes = payload(&review, proof.expires_at);
    mac(review.state, &bytes)
        .verify_slice(&signature)
        .map_err(|_| invalid())
}
