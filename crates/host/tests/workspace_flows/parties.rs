use super::*;
async fn setup_parties() -> (Fixture, Router, Value) {
    let (f, app, s) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(if actor=="reader"{vec!["parties.read","audit.read"]}else{vec!["parties.read","parties.write","organizations.read","organizations.write","affiliations.read","affiliations.write","audit.read"]}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,ARRAY['contacts.read','contacts.write'],$3)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
    }
    (f, app, s)
}
fn input(kind: &str, data: Value) -> Value {
    json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}})
}
async fn send(app: &Router, s: &Value, b: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/organizations/commands",
        b.clone(),
        Some(s),
    )
    .await
}
async fn contact(app: &Router, s: &Value, name: &str) -> Value {
    let(st,b)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":name,"email":"party@example.test"}}}),Some(s)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b["receipt"].clone()
}
async fn org(app: &Router, s: &Value, name: &str) -> Value {
    let (st, b) = send(app, s, &input("create_organization", json!({"name":name}))).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b["receipt"].clone()
}
fn create(source: &Value, organization: bool) -> Value {
    let mut d =
        json!({"expectedSourceRevision":source["revision"],"customer":true,"supplier":true});
    d[if organization {
        "organizationId"
    } else {
        "contactId"
    }] = source[if organization { "organizationId" } else { "id" }].clone();
    input(
        if organization {
            "party.create_from_organization"
        } else {
            "party.create_from_contact"
        },
        d,
    )
}
fn roles(p: &Value, revision: i32, customer: bool, supplier: bool) -> Value {
    input(
        "party.set_roles",
        json!({"id":p["partyId"],"expectedRevision":revision,"customer":customer,"supplier":supplier,"reason":"Reviewed business role change"}),
    )
}
fn refresh(p: &Value, revision: i32, source_revision: i32) -> Value {
    input(
        "party.refresh_source",
        json!({"id":p["partyId"],"expectedRevision":revision,"expectedSourceRevision":source_revision,"reason":"Reviewed source profile"}),
    )
}
async fn workspace(app: &Router, s: &Value) -> Value {
    let (st, b) = snapshot(app, s, "organizations").await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b
}
async fn renamed(app: &Router, s: &Value, c: &Value, revision: i32, name: &str) {
    let(st,b)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.update","id":c["id"],"expectedRevision":revision,"data":{"fullName":name}}}),Some(s)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
}

#[tokio::test]
async fn party_registry_keeps_contact_and_organization_identities_distinct_with_independent_roles()
{
    let (_f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Same name").await;
    let o = org(&app, &s, "Same name").await;
    let (a, ca) = send(&app, &s, &create(&c, false)).await;
    let (b, oa) = send(&app, &s, &create(&o, true)).await;
    assert_eq!(a, StatusCode::OK, "{ca}");
    assert_eq!(b, StatusCode::OK, "{oa}");
    assert_ne!(ca["receipt"]["partyId"], oa["receipt"]["partyId"]);
    let p = &ca["receipt"];
    assert_eq!(
        send(&app, &s, &roles(p, 1, false, true)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, &s, &roles(p, 2, false, false)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, &s, &roles(p, 3, true, true)).await.0,
        StatusCode::OK
    );
    let rows = workspace(&app, &s).await;
    assert_eq!(rows["parties"].as_array().unwrap().len(), 2);
    assert_eq!(rows["party_audit"].as_array().unwrap().len(), 5);
    assert!(rows["parties"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["name"] == "Same name" && r["customer"] == true && r["supplier"] == true));
}

#[tokio::test]
async fn party_registry_refresh_preserves_versions_and_replay_after_source_renames() {
    let (f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Old name").await;
    let original = create(&c, false);
    let (st, b) = send(&app, &s, &original).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let p = &b["receipt"];
    renamed(&app, &s, &c, 1, "Reviewed name").await;
    let update = refresh(p, 1, 2);
    assert_eq!(send(&app, &s, &update).await.0, StatusCode::OK);
    renamed(&app, &s, &c, 2, "Later name").await;
    for i in [&original, &update] {
        let (st, b) = send(&app, &s, i).await;
        assert_eq!(st, StatusCode::OK, "{b}");
        assert_eq!(b["receipt"]["partyId"], p["partyId"]);
        assert_eq!(b["receipt"]["replayed"], true);
    }
    let history:Value=sqlx::query_scalar("SELECT json_agg(snapshot->>'name' ORDER BY revision) FROM archava_organizations.party_audit WHERE tenant_id='acme'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(history, json!(["Old name", "Reviewed name"]));
    let rows = workspace(&app, &s).await;
    assert_eq!(rows["parties"][0]["revision"], 2);
    assert_eq!(rows["parties"][0]["sourceRevision"], 2);
    assert_eq!(rows["parties"][0]["name"], "Reviewed name");
}

#[tokio::test]
async fn party_registry_rejects_stale_forged_missing_and_duplicate_sources_atomically() {
    let (f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Before").await;
    renamed(&app, &s, &c, 1, "After").await;
    let old = create(&c, false);
    assert_eq!(send(&app, &s, &old).await.0, StatusCode::CONFLICT);
    let mut body = old.clone();
    body["mutation"]["data"]["expectedSourceRevision"] = json!(2);
    for key in ["name", "ownerId", "sourceKind", "permissions"] {
        let mut forged = body.clone();
        forged["mutation"]["data"][key] = json!("forged");
        assert_eq!(send(&app, &s, &forged).await.0, StatusCode::BAD_REQUEST);
    }
    let mut missing = body.clone();
    missing["mutation"]["data"]["contactId"] = json!(Uuid::new_v4());
    assert_eq!(send(&app, &s, &missing).await.0, StatusCode::NOT_FOUND);
    let (st, p) = send(&app, &s, &body).await;
    assert_eq!(st, StatusCode::OK, "{p}");
    let mut duplicate = body.clone();
    duplicate["mutation"]["requestId"] = json!(Uuid::new_v4());
    assert_eq!(send(&app, &s, &duplicate).await.0, StatusCode::CONFLICT);
    assert_eq!(
        send(&app, &s, &refresh(&p["receipt"], 1, 2)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &s, &roles(&p["receipt"], 2, false, true))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_organizations.commands")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn party_registry_grants_are_distinct_from_raw_sources_and_audit_alone() {
    let (f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Private source").await;
    let original = create(&c, false);
    let (st, b) = send(&app, &s, &original).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    sqlx::query(
        "UPDATE archava_crm.memberships SET permissions=ARRAY[]::text[] WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(send(&app, &s, &original).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        send(&app, &s, &refresh(&b["receipt"], 1, 2)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, &s, &roles(&b["receipt"], 1, false, true))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        workspace(&app, &s).await["parties"][0]["name"],
        "Private source"
    );
    let reader = login(&app, "reader").await;
    assert_eq!(
        send(&app, &reader, &original).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        workspace(&app, &reader).await["parties"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['organizations.read','audit.read'] WHERE principal_id='reader'").execute(&f.admin_pool).await.unwrap();
    let rows = workspace(&app, &reader).await;
    assert!(rows.get("parties").is_none());
    assert!(rows.get("party_audit").is_none());
    let path = format!(
        "/erp/organizations/commands/status?tenant=acme&requestId={}",
        original["mutation"]["requestId"].as_str().unwrap()
    );
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&s)).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['parties.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&s)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn party_registry_enforces_tenant_assigned_source_and_party_scope() {
    let (f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Alice source").await;
    let (st, b) = send(&app, &s, &create(&c, false)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let assigned = login(&app, "assigned").await;
    assert_eq!(
        send(&app, &assigned, &create(&c, false)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&app, &assigned, &roles(&b["receipt"], 1, false, true))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(workspace(&app, &assigned).await["parties"], json!([]));
    sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('other','alice','human',ARRAY['contacts.read'],'all')").execute(&f.admin_pool).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('other',$1,'Foreign','alice')").bind(id).execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        send(&app, &s, &create(&json!({"id":id,"revision":1}), false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let service = login(&app, "service-seat").await;
    assert_eq!(
        send(&app, &service, &create(&c, false)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn party_registry_concurrent_create_and_compare_swap_roles_have_one_winner() {
    let (_f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "Race source").await;
    let a = create(&c, false);
    let b = create(&c, false);
    let (a, b) = tokio::join!(send(&app, &s, &a), send(&app, &s, &b));
    assert!(
        a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT
            || b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT,
        "{a:?} {b:?}"
    );
    let p = if a.0 == StatusCode::OK {
        a.1["receipt"].clone()
    } else {
        b.1["receipt"].clone()
    };
    let a = roles(&p, 1, false, true);
    let b = roles(&p, 1, true, false);
    let (a, b) = tokio::join!(send(&app, &s, &a), send(&app, &s, &b));
    assert!(
        a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT
            || b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT,
        "{a:?} {b:?}"
    );
    assert_eq!(workspace(&app, &s).await["parties"][0]["revision"], 2);
}

#[tokio::test]
async fn party_registry_database_guards_require_true_sources_complete_receipts_and_history() {
    let (f, app, s) = setup_parties().await;
    let c = contact(&app, &s, "True name").await;
    let (st, b) = send(&app, &s, &create(&c, false)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let id = Uuid::parse_str(b["receipt"]["partyId"].as_str().unwrap()).unwrap();
    for query in ["UPDATE archava_organizations.parties SET name='Forged',revision=revision+1 WHERE id=$1","UPDATE archava_organizations.parties SET source_contact_id=gen_random_uuid(),revision=revision+1 WHERE id=$1","DELETE FROM archava_organizations.parties WHERE id=$1"]{assert!(sqlx::query(query).bind(id).execute(&f.admin_pool).await.is_err());}
    let mut tx = f.admin_pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE archava_organizations.parties SET customer=false,revision=revision+1 WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert!(tx.commit().await.is_err());
    assert_eq!(workspace(&app, &s).await["parties"][0]["revision"], 1);
    assert!(sqlx::query(
        "UPDATE archava_organizations.party_audit SET snapshot='{}' WHERE party_id=$1"
    )
    .bind(id)
    .execute(&f.admin_pool)
    .await
    .is_err());
}

#[tokio::test]
async fn party_registry_org_archive_preserves_master_and_original_replay_but_refuses_refresh() {
    let (_f, app, s) = setup_parties().await;
    let o = org(&app, &s, "Organization").await;
    let original = create(&o, true);
    let (st, b) = send(&app, &s, &original).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(
        send(
            &app,
            &s,
            &input(
                "archive_organization",
                json!({"id":o["organizationId"],"revision":1,"reason":"Source no longer active"})
            )
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(send(&app, &s, &original).await.0, StatusCode::OK);
    assert_eq!(
        send(&app, &s, &refresh(&b["receipt"], 1, 2)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &s, &roles(&b["receipt"], 1, false, false))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn party_registry_approved_mission_and_request_namespace_recovery_are_exact() {
    let (_f, app, s) = setup_parties().await;
    let c = contact(&app, &s, &"供".repeat(160)).await;
    let payload = create(&c, false)["mutation"]["data"].clone();
    let(st,m)=call(&app,"POST","/api/v1/cockpit/missions",json!({"tenant_id":"acme","mission_code":"party-register","title":"Reviewed party registration","description":"Stable business identity","auto_launch":true,"steps":[{"step_index":0,"step_name":"Register reviewed party","tool_name":"organizations.party.create_from_contact","payload_input":payload,"depends_on":[]}]}),Some(&s)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    let base = format!("/api/v1/cockpit/missions/{}", m["id"].as_str().unwrap());
    let path = format!("{base}/run-next?tenant=acme");
    assert_eq!(
        call(&app, "POST", &path, json!({"stepIndex":0}), Some(&s))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(workspace(&app, &s).await["parties"], json!([]));
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/steps/0/approve?tenant=acme"),
            json!({}),
            Some(&s)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (st, result) = call(&app, "POST", &path, json!({"stepIndex":0}), Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "touchdown");
    assert_eq!(
        call(&app, "POST", &path, json!({"stepIndex":0}), Some(&s))
            .await
            .1,
        result
    );
    let rows = workspace(&app, &s).await;
    assert_eq!(rows["parties"].as_array().unwrap().len(), 1);
    assert_eq!(rows["parties"][0]["name"], "供".repeat(160));
    let unknown = Uuid::new_v4();
    let (st, status) = call(
        &app,
        "GET",
        &format!("/erp/organizations/commands/status?tenant=acme&requestId={unknown}"),
        json!({}),
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{status}");
    assert_eq!(status["decision"], "not_committed");
}
