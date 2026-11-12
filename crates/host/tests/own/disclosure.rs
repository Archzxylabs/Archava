//! Real owner HTTP/PG approval witnesses. These do not certify outbound AI projection.
use super::*;
use serde_json::{json, Value};

fn configured(state: AppState, model: &str, key: &str) -> AppState {
    state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(
            archava_providers::agent::GeminiPlanner::new("http://127.0.0.1:1", model, key).unwrap(),
        ),
    })
}
#[tokio::test]
async fn owner_disclosure_readiness_requires_the_installed_exact_boundary_version() {
    let f = provision().await;
    state(&f).await;
    sqlx::query("DROP FUNCTION archava_owner.ai_disclosure_boundary_version()")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let result = AppState::try_new_owner(
        f.runtime_pool.clone(),
        commerce_ledger(),
        &f.commerce_ledger,
        &f.identity_ledger,
        IDENTITY_KEY,
        &f.crm_ledger,
        &f.owner_ledger,
    )
    .await;
    assert!(
        result.is_err(),
        "Matching migration rows must not hide a missing disclosure boundary"
    );
    sqlx::query("CREATE FUNCTION archava_owner.ai_disclosure_boundary_version() RETURNS text LANGUAGE sql IMMUTABLE AS $$SELECT 'unsupported-boundary'::text$$").execute(&f.admin_pool).await.unwrap();
    let result = AppState::try_new_owner(
        f.runtime_pool.clone(),
        commerce_ledger(),
        &f.commerce_ledger,
        &f.identity_ledger,
        IDENTITY_KEY,
        &f.crm_ledger,
        &f.owner_ledger,
    )
    .await;
    assert!(
        result.is_err(),
        "An incompatible disclosure protocol must not serve"
    );
}
async fn policy_request(app: &Router, session: &Session, base: i64) -> Value {
    let (st, config) = get(app, &format!("/own/config?tenant={ACME}"), session).await;
    assert_eq!(st, StatusCode::OK, "{config}");
    assert!(!config.to_string().contains("owned-ai-key"));
    json!({"baseRevision":base,"aiDisclosure":{"kind":"replace","policy":{
        "providerBinding":config["aiProvider"]["binding"],"modules":["crm"],
        "dataClasses":["record_references","business_facts","names"],
        "companies":[],"includeWorkspaceRecords":true
    }}})
}
async fn preview(app: &Router, session: &Session, request: &Value) -> Value {
    let (st, p) = post(
        app,
        "/own/plan",
        &json!({"tenant":ACME,"request":request}),
        session,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{p}");
    assert!(p["plan"]["blocked"].is_null(), "{p}");
    p
}
fn confirmed(p: &Value, id: &str) -> Value {
    json!({"tenant":ACME,"requestId":id,"request":p["request"],
        "confirmation":{"planHash":p["plan"]["planHash"],"baseRevision":p["plan"]["baseRevision"]},
        "disclosureApproval":p["disclosureApproval"]})
}
async fn approval_refused(app: &Router, session: &Session, body: &Value) {
    let (st, v) = post(app, "/own/apply", body, session).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["reason"], "disclosure_approval_required", "{v}");
}

#[tokio::test]
async fn owner_disclosure_approval_binds_whole_policy_actor_expiry_and_one_effect() {
    let f = provision().await;
    seed(&f).await;
    let state = configured(state(&f).await, "owned-model", "owned-ai-key");
    let app = archava_host::router(state.clone());
    let alice = login(&app, "alice").await;
    let carol = login(&app, "carol").await;
    let request = policy_request(&app, &alice, 0).await;
    let p = preview(&app, &alice, &request).await;
    assert!(p["disclosureApproval"]["signature"].is_string());
    let good = confirmed(&p, "disclosure-grant");
    let mut bad = good.clone();
    bad.as_object_mut().unwrap().remove("disclosureApproval");
    approval_refused(&app, &alice, &bad).await;
    bad = good.clone();
    bad["disclosureApproval"]["signature"] = json!("forged-signature");
    approval_refused(&app, &alice, &bad).await;
    for expiry in [0_i64, i64::MAX] {
        bad = good.clone();
        bad["disclosureApproval"]["expiresAt"] = json!(expiry);
        approval_refused(&app, &alice, &bad).await;
    }
    // Carol has the same real management grants and a valid deterministic plan,
    // but Alice's signed review cannot become Carol's approval.
    approval_refused(&app, &carol, &good).await;
    // A caller can obtain the OTHER valid public plan identity. That identity
    // alone must not let the caller broaden the signed category selection.
    let mut broader = request.clone();
    broader["aiDisclosure"]["policy"]["dataClasses"] =
        json!(["record_references", "business_facts", "names", "notes"]);
    let other = preview(&app, &alice, &broader).await;
    assert_ne!(p["plan"]["planHash"], other["plan"]["planHash"]);
    bad = confirmed(&other, "disclosure-broaden");
    bad["disclosureApproval"] = p["disclosureApproval"].clone();
    approval_refused(&app, &alice, &bad).await;
    assert_eq!(count(&f, "config_revisions").await, 0);
    assert_eq!(count(&f, "apply_receipts").await, 0);
    let (st, applied) = post(&app, "/own/apply", &good, &alice).await;
    assert_eq!(st, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);
    let rebuilt = archava_host::router(configured(
        super::state(&f).await,
        "owned-model",
        "owned-ai-key",
    ));
    // Receipt recovery is still possible after an expired/missing confirmation;
    // replay proves the earlier effect rather than granting a fresh permission.
    bad = good.clone();
    bad.as_object_mut().unwrap().remove("disclosureApproval");
    let (st, replay) = post(&rebuilt, "/own/apply", &bad, &alice).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(count(&f, "config_revisions").await, 1);
    assert_eq!(count(&f, "apply_receipts").await, 1);
    let (_, current) = get(&rebuilt, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(
        current["config"]["aiDisclosure"],
        p["plan"]["config"]["aiDisclosure"]
    );
    assert_eq!(current["aiProvider"]["approvedForCurrentProvider"], true);
}

#[tokio::test]
async fn owner_disclosure_provider_account_and_model_changes_require_new_review_but_offline_revoke_works(
) {
    let f = provision().await;
    seed(&f).await;
    let base = state(&f).await;
    let app = archava_host::router(configured(base.clone(), "owned-model", "owned-ai-key"));
    let alice = login(&app, "alice").await;
    let request = policy_request(&app, &alice, 0).await;
    let p = preview(&app, &alice, &request).await;
    for (model, key) in [
        ("changed-model", "owned-ai-key"),
        ("owned-model", "changed-account-key"),
    ] {
        let changed = archava_host::router(configured(base.clone(), model, key));
        let (st, refused) = post(
            &changed,
            "/own/apply",
            &confirmed(&p, "changed-provider"),
            &alice,
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "{refused}");
        assert_eq!(refused["reason"], "ai_provider_changed");
        let fresh = preview(&changed, &alice, &policy_request(&changed, &alice, 0).await).await;
        let mut swapped = confirmed(&fresh, "swapped-review");
        swapped["disclosureApproval"] = p["disclosureApproval"].clone();
        approval_refused(&changed, &alice, &swapped).await;
    }
    assert_eq!(count(&f, "config_revisions").await, 0);
    let (st, v) = post(
        &app,
        "/own/apply",
        &confirmed(&p, "grant-before-offline"),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let offline = archava_host::router(base);
    let (_, current) = get(&offline, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(current["aiProvider"]["approvedForCurrentProvider"], false);
    let revoke = preview(
        &offline,
        &alice,
        &json!({"baseRevision":1,"aiDisclosure":{"kind":"revoke"}}),
    )
    .await;
    let (st, v) = post(
        &offline,
        "/own/apply",
        &confirmed(&revoke, "offline-revoke"),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let restore_body = json!({"tenant":ACME,"targetRevision":1});
    let (st, v) = post(&offline, "/own/restore/plan", &restore_body, &alice).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["reason"], "ai_provider_changed");
    assert_eq!(count(&f, "config_revisions").await, 2);
    let (_, current) = get(&offline, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert!(current["config"]["aiDisclosure"].is_null());
    // An unchanged account may restore that historical selection, as a new
    // revision after a NEW operation-specific signed review.
    let (st, restore) = post(&app, "/own/restore/plan", &restore_body, &alice).await;
    assert_eq!(st, StatusCode::OK, "{restore}");
    let mut body = json!({"tenant":ACME,"requestId":"restore-disclosure","targetRevision":1,
        "confirmation":{"planHash":restore["plan"]["planHash"],"baseRevision":2}});
    let (st, v) = post(&app, "/own/restore", &body, &alice).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["reason"], "disclosure_approval_required");
    body["disclosureApproval"] = restore["disclosureApproval"].clone();
    let (st, v) = post(&app, "/own/restore", &body, &alice).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["receipt"]["revision"], 3);
}

#[tokio::test]
async fn owner_disclosure_review_cannot_cross_tenants_or_survive_management_revocation() {
    let f = provision().await;
    seed(&f).await;
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions,active) VALUES('other-business','alice',ARRAY['config.read','config.apply'],true)").execute(&f.admin_pool).await.unwrap();
    let app = archava_host::router(configured(state(&f).await, "owned-model", "owned-ai-key"));
    let alice = login(&app, "alice").await;
    let request = policy_request(&app, &alice, 0).await;
    let p = preview(&app, &alice, &request).await;
    let (st, other) = post(
        &app,
        "/own/plan",
        &json!({"tenant":"other-business","request":request}),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{other}");
    let mut body = confirmed(&other, "foreign-tenant-proof");
    body["tenant"] = json!("other-business");
    body["disclosureApproval"] = p["disclosureApproval"].clone();
    approval_refused(&app, &alice, &body).await;
    sqlx::query("UPDATE archava_owner.owner_grants SET active=false WHERE tenant_id=$1 AND principal_id='alice'")
        .bind(ACME).execute(&f.admin_pool).await.unwrap();
    let (st, v) = post(
        &app,
        "/own/apply",
        &confirmed(&p, "revoked-management"),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(count(&f, "config_revisions").await, 0);
    assert_eq!(count(&f, "apply_receipts").await, 0);
}

#[tokio::test]
async fn owner_disclosure_companies_are_resolved_from_current_real_company_grants() {
    let f = provision().await;
    seed(&f).await;
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write'],'all')")
        .bind(ACME).execute(&f.admin_pool).await.unwrap();
    let app = archava_host::router(configured(state(&f).await, "owned-model", "owned-ai-key"));
    let alice = login(&app, "alice").await;
    let (st, org) = post(&app, "/erp/organizations/commands", &json!({"tenant":ACME,"mutation":{"requestId":uuid::Uuid::new_v4(),"type":"create_organization","data":{"name":"Privacy test company","legalName":"Privacy test company official"}}}), &alice).await;
    assert_eq!(st, StatusCode::OK, "{org}");
    let (st, company) = post(&app, "/erp/organizations/commands", &json!({"tenant":ACME,"mutation":{"requestId":uuid::Uuid::new_v4(),"type":"company.create_from_organization","data":{"organizationId":org["receipt"]["organizationId"],"expectedSourceRevision":1,"code":"PRIVATE","reason":"Explicit synthetic company"}}}), &alice).await;
    assert_eq!(st, StatusCode::OK, "{company}");
    let mut request = policy_request(&app, &alice, 0).await;
    request["aiDisclosure"]["policy"]["modules"] = json!(["accounting"]);
    request["aiDisclosure"]["policy"]["dataClasses"] =
        json!(["record_references", "financial_amounts"]);
    request["aiDisclosure"]["policy"]["companies"] = json!([company["receipt"]["companyId"]]);
    request["aiDisclosure"]["policy"]["includeWorkspaceRecords"] = json!(false);
    let p = preview(&app, &alice, &request).await;
    // Owner permission is not a replacement for a live company read grant.
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['organizations.read'] WHERE tenant_id=$1 AND principal_id='alice'")
        .bind(ACME).execute(&f.admin_pool).await.unwrap();
    let (st, v) = post(
        &app,
        "/own/apply",
        &confirmed(&p, "company-revoked"),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["reason"], "company_scope_unavailable");
    assert_eq!(count(&f, "config_revisions").await, 0);
    // A structurally valid but unobserved company reference also refuses.
    request["aiDisclosure"]["policy"]["companies"] = json!([uuid::Uuid::new_v4()]);
    let (st, v) = post(
        &app,
        "/own/plan",
        &json!({"tenant":ACME,"request":request}),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["reason"], "company_scope_unavailable");
}
