use super::*;
async fn seed(f: &Fixture) {
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1) ON CONFLICT DO NOTHING").bind(actor).bind(format!("{actor}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        let kind = if actor == "service-seat" {
            "service"
        } else {
            "human"
        };
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)").bind(actor).bind(kind).bind(if actor=="reader"{vec!["organizations.read","affiliations.read"]}else{vec!["organizations.read","organizations.write","affiliations.read","affiliations.write","audit.read"]}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,ARRAY['contacts.read','contacts.write'],$3)").bind(actor).bind(kind).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
    }
}
async fn setup_orgs() -> (Fixture, Router, Value) {
    let (f, app, session) = setup().await;
    seed(&f).await;
    (f, app, session)
}
async fn send(app: &Router, session: &Value, input: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/organizations/commands",
        input.clone(),
        Some(session),
    )
    .await
}
fn command(kind: &str, data: Value) -> Value {
    json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}})
}
async fn org(app: &Router, session: &Value) -> Value {
    let (status, body) = send(
        app,
        session,
        &command(
            "create_organization",
            json!({"name":"Same organization","legalName":"Synthetic business"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
async fn contact(app: &Router, session: &Value, name: &str) -> Value {
    let (status,body)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":name,"email":"relation@example.test"}}}),Some(session)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
async fn rename(app: &Router, session: &Value, source: &Value, revision: i64, name: &str) {
    let (s,b)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.update","id":source["id"],"expectedRevision":revision,"data":{"fullName":name}}}),Some(session)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}
fn reviewed(org: &Value, contact: &Value) -> Value {
    command(
        "create_reviewed_affiliation",
        json!({"organizationId":org["organizationId"],"expectedOrganizationRevision":org["revision"],"contactId":contact["id"],"expectedContactRevision":contact["revision"],"role":"Business contact","isPrimary":true}),
    )
}
async fn workspace(app: &Router, session: &Value) -> Value {
    let (status, body) = snapshot(app, session, "organizations").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn organization_identity_keeps_selected_ids_and_captured_name_after_rename_and_replay() {
    let (_f, app, s) = setup_orgs().await;
    let other = org(&app, &s).await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Original contact").await;
    let input = reviewed(&org, &source);
    let (status, answer) = send(&app, &s, &input).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["receipt"]["revision"], 2);
    rename(&app, &s, &source, 1, "Changed contact").await;
    let (status, replayed) = send(&app, &s, &input).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(
        answer["receipt"]["affiliationId"],
        replayed["receipt"]["affiliationId"]
    );
    let rows = workspace(&app, &s).await;
    let row = &rows["affiliations"][0];
    assert_eq!(row["organizationId"], org["organizationId"]);
    assert_ne!(row["organizationId"], other["organizationId"]);
    assert_eq!(row["contactId"], source["id"]);
    assert_eq!(row["contactRevision"], 1);
    assert_eq!(row["contactName"], "Original contact");
    assert_eq!(rows["affiliations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn organization_identity_refuses_stale_reviews_and_forged_optional_fields_atomically() {
    let (f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Original").await;
    let mut input = reviewed(&org, &source);
    rename(&app, &s, &source, 1, "Updated").await;
    assert_eq!(send(&app, &s, &input).await.0, StatusCode::CONFLICT);
    input["mutation"]["data"]["expectedContactRevision"] = json!(2);
    input["mutation"]["data"]["expectedOrganizationRevision"] = json!(2);
    assert_eq!(send(&app, &s, &input).await.0, StatusCode::CONFLICT);
    input["mutation"]["data"]["expectedOrganizationRevision"] = json!(1);
    for key in [
        "name",
        "ownerId",
        "contactName",
        "contactRevision",
        "permissions",
    ] {
        let mut forged = input.clone();
        forged["mutation"]["data"][key] = json!("forged");
        assert_eq!(send(&app, &s, &forged).await.0, StatusCode::BAD_REQUEST);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_organizations.commands")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(workspace(&app, &s).await["affiliations"], json!([]));
}

#[tokio::test]
async fn organization_identity_legacy_create_now_requires_a_real_visible_crm_contact() {
    let (_f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let mut input = command(
        "create_affiliation",
        json!({"organizationId":org["organizationId"],"contactId":Uuid::new_v4(),"role":"Liaison"}),
    );
    assert_eq!(send(&app, &s, &input).await.0, StatusCode::NOT_FOUND);
    let source = contact(&app, &s, "Real contact").await;
    input["mutation"]["data"]["contactId"] = source["id"].clone();
    assert_eq!(send(&app, &s, &input).await.0, StatusCode::OK);
    let row = &workspace(&app, &s).await["affiliations"][0];
    assert_eq!(row["contactName"], "Real contact");
    assert_eq!(row["contactRevision"], 1);
}

#[tokio::test]
async fn organization_identity_rechecks_crm_read_scope_and_reactivation_authority() {
    let (f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Supplier contact").await;
    let input = reviewed(&org, &source);
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (status, answer) = send(&app, &s, &input).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(send(&app, &s, &input).await.0, StatusCode::FORBIDDEN);
    let inactive = command(
        "update_affiliation",
        json!({"id":answer["receipt"]["affiliationId"],"organizationId":org["organizationId"],"status":"inactive"}),
    );
    assert_eq!(send(&app, &s, &inactive).await.0, StatusCode::OK);
    let mut activate = inactive.clone();
    activate["mutation"]["requestId"] = json!(Uuid::new_v4());
    activate["mutation"]["data"]["status"] = json!("active");
    assert_eq!(send(&app, &s, &activate).await.0, StatusCode::FORBIDDEN);
    for actor in ["reader", "service-seat"] {
        let peer = login(&app, actor).await;
        assert_eq!(send(&app, &peer, &input).await.0, StatusCode::FORBIDDEN);
    }
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read'],scope='assigned' WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let hidden = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('acme',$1,'Peer contact','assigned')").bind(hidden).execute(&f.admin_pool).await.unwrap();
    let mut hidden_input = reviewed(&org, &json!({"id":hidden,"revision":1}));
    hidden_input["mutation"]["data"]["expectedOrganizationRevision"] = json!(3);
    assert_eq!(send(&app, &s, &hidden_input).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn organization_identity_refuses_cross_tenant_sources_and_assigned_organization_writes() {
    let (f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Alice contact").await;
    sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('other','alice','human',ARRAY['contacts.read'],'all')").execute(&f.admin_pool).await.unwrap();
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('other',$1,'Foreign','alice')").bind(foreign).execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        send(
            &app,
            &s,
            &reviewed(&org, &json!({"id":foreign,"revision":1}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE archava_organizations.organizations SET owner_id='assigned' WHERE tenant_id='acme' AND id=$1").bind(Uuid::parse_str(org["organizationId"].as_str().unwrap()).unwrap()).execute(&f.admin_pool).await.unwrap();
    sqlx::query(
        "UPDATE archava_organizations.memberships SET scope='assigned' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        send(&app, &s, &reviewed(&org, &source)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn organization_identity_concurrent_reviews_and_contact_rename_preserve_coherence() {
    let (_f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Before").await;
    let first = reviewed(&org, &source);
    let second = reviewed(&org, &source);
    let (a, b) = tokio::join!(send(&app, &s, &first), send(&app, &s, &second));
    assert!(
        a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT
            || b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT,
        "{a:?} {b:?}"
    );
    let mut next = reviewed(
        &json!({"organizationId":org["organizationId"],"revision":2}),
        &source,
    );
    next["mutation"]["data"]["role"] = json!("Secondary");
    next["mutation"]["data"]["isPrimary"] = json!(false);
    let (created, ()) = tokio::join!(send(&app, &s, &next), rename(&app, &s, &source, 1, "After"));
    assert!(
        matches!(created.0, StatusCode::OK | StatusCode::CONFLICT),
        "{created:?}"
    );
    let rows = workspace(&app, &s).await;
    assert!(rows["affiliations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["contactName"] == "Before" && row["contactRevision"] == 1));
}

#[tokio::test]
async fn organization_identity_database_guards_reject_false_snapshots_and_repointing() {
    let (f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, "Source").await;
    let (_, answer) = send(&app, &s, &reviewed(&org, &source)).await;
    let id = Uuid::parse_str(answer["receipt"]["affiliationId"].as_str().unwrap()).unwrap();
    for query in ["UPDATE archava_organizations.affiliations SET contact_name='Forged' WHERE id=$1","UPDATE archava_organizations.affiliations SET contact_revision=NULL,contact_name=NULL WHERE id=$1","UPDATE archava_organizations.affiliations SET contact_id=gen_random_uuid() WHERE id=$1"] {assert!(sqlx::query(query).bind(id).execute(&f.admin_pool).await.is_err());}
    let org_id = Uuid::parse_str(org["organizationId"].as_str().unwrap()).unwrap();
    let contact_id = Uuid::parse_str(source["id"].as_str().unwrap()).unwrap();
    assert!(sqlx::query("INSERT INTO archava_organizations.affiliations(tenant_id,id,organization_id,contact_id,role,contact_revision,contact_name) VALUES('acme',$1,$2,$3,'Forged',1,'Wrong')").bind(Uuid::new_v4()).bind(org_id).bind(contact_id).execute(&f.admin_pool).await.is_err());
    assert!(sqlx::query("INSERT INTO archava_organizations.affiliations(tenant_id,id,organization_id,contact_id,role) VALUES('acme',$1,$2,$3,'Missing')").bind(Uuid::new_v4()).bind(org_id).bind(Uuid::new_v4()).execute(&f.admin_pool).await.is_err());
}

async fn mission(app: &Router, s: &Value, code: &str, tool: &str, payload: Value) {
    let (status,mission)=call(app,"POST","/api/v1/cockpit/missions",json!({"tenant_id":"acme","mission_code":code,"title":"Reviewed relationship","description":"Verify CRM and organization sources","auto_launch":true,"steps":[{"step_index":0,"step_name":"Apply reviewed relationship","tool_name":tool,"payload_input":payload,"depends_on":[]}]}),Some(s)).await;
    assert_eq!(status, StatusCode::CREATED, "{mission}");
    let base = format!(
        "/api/v1/cockpit/missions/{}",
        mission["id"].as_str().unwrap()
    );
    let path = format!("{base}/run-next?tenant=acme");
    assert_eq!(
        call(app, "POST", &path, json!({"stepIndex":0}), Some(s))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            app,
            "POST",
            &format!("{base}/steps/0/approve?tenant=acme"),
            json!({}),
            Some(s)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, result) = call(app, "POST", &path, json!({"stepIndex":0}), Some(s)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "touchdown");
    assert_eq!(result["steps"][0]["payload_output"]["verified"], true);
    assert_eq!(
        call(app, "POST", &path, json!({"stepIndex":0}), Some(s))
            .await
            .1,
        result
    );
}
#[tokio::test]
async fn organization_identity_reviewed_mission_requires_approval_and_uses_unicode_source() {
    let (_f, app, s) = setup_orgs().await;
    let org = org(&app, &s).await;
    let source = contact(&app, &s, &"供".repeat(160)).await;
    mission(
        &app,
        &s,
        "reviewed-org-contact",
        "organizations.create_reviewed_affiliation",
        reviewed(&org, &source)["mutation"]["data"].clone(),
    )
    .await;
    let rows = workspace(&app, &s).await;
    assert_eq!(rows["affiliations"].as_array().unwrap().len(), 1);
    assert_eq!(rows["affiliations"][0]["contactName"], "供".repeat(160));
}

#[tokio::test]
async fn organization_identity_real_prior_profile_preserves_legacy_and_verifies_it_explicitly() {
    let mut f = archava_pg_fixture::provision_pre_verified_affiliations_profile().await;
    seed(&f).await;
    // Opening data written to the genuine pre-002 schema, including an opaque
    // historical UUID which must survive the upgrade without becoming verified.
    let org = Uuid::new_v4();
    let source = Uuid::new_v4();
    let valid = Uuid::new_v4();
    let missing = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('acme',$1,'Legacy source','alice')").bind(source).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_organizations.organizations(tenant_id,id,name,owner_id) VALUES('acme',$1,'Legacy organization','alice')").bind(org).execute(&f.admin_pool).await.unwrap();
    for (id, contact) in [(valid, source), (missing, Uuid::new_v4())] {
        sqlx::query("INSERT INTO archava_organizations.affiliations(tenant_id,id,organization_id,contact_id,role,status) VALUES('acme',$1,$2,$3,'Legacy contact','inactive')").bind(id).bind(org).bind(contact).execute(&f.admin_pool).await.unwrap();
    }
    f.organizations_ledger =
        archava_pg_fixture::apply_pending_organization_migrations(&f.owner_pool).await;
    assert_eq!(f.organizations_ledger.len(), 4);
    assert_eq!(
        archava_pg_fixture::apply_pending_organization_migrations(&f.owner_pool).await,
        f.organizations_ledger
    );
    // The current host also verifies newer dependent purchasing tables. Upgrade
    // the whole mounted trade profile before constructing that serving host.
    archava_pg_fixture::apply_pending_party_transaction_migrations(&f.owner_pool).await;
    f.commerce_ledger = archava_pg_fixture::ledger_expectations("archava_commerce");
    f.purchasing_ledger = archava_pg_fixture::ledger_expectations("archava_purchasing");
    f.billing_ledger = archava_pg_fixture::ledger_expectations("archava_billing");
    let pending:i64=sqlx::query_scalar("SELECT count(*) FROM archava_organizations.affiliations WHERE contact_revision IS NULL AND contact_name IS NULL").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(pending, 2);
    let app = router(
        AppState::try_new_all(
            f.runtime_pool.clone(),
            Ledger {
                namespace: "archava_commerce",
                ledger: "Commerce",
                tables: &[
                    "memberships",
                    "products",
                    "stock_balances",
                    "orders",
                    "order_lines",
                    "allocations",
                    "commands",
                    "movements",
                    "returns",
                    "audit",
                    "approvals",
                ],
            },
            AllReadiness {
                commerce: &f.commerce_ledger,
                identity: &f.identity_ledger,
                crm: &f.crm_ledger,
                owner: &f.owner_ledger,
                purchasing: &f.purchasing_ledger,
                additional_erp: &f.erp_readiness(),
            },
            &[7; 32],
        )
        .await
        .unwrap(),
    );
    let s = login(&app, "alice").await;
    let activate = command(
        "update_affiliation",
        json!({"id":missing,"organizationId":org,"status":"active"}),
    );
    assert_eq!(send(&app, &s, &activate).await.0, StatusCode::NOT_FOUND);
    let verify = command(
        "verify_affiliation_contact",
        json!({"id":valid,"organizationId":org,"expectedOrganizationRevision":1,"expectedContactRevision":1,"reason":"Reviewed original source"}),
    );
    // This fixture needs a current Commerce tenant grant for the mission API.
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme','alice','human',ARRAY['catalog.read','orders.read'],'all','owner','enterprise')").execute(&f.admin_pool).await.unwrap();
    mission(
        &app,
        &s,
        "verify-old-org-contact",
        "organizations.verify_affiliation_contact",
        verify["mutation"]["data"].clone(),
    )
    .await;
    let rows = workspace(&app, &s).await;
    assert_eq!(
        rows["affiliations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["contactRevision"].is_null())
            .count(),
        1
    );
    assert_eq!(send(&app, &s, &verify).await.0, StatusCode::CONFLICT); // Different request after mission already verified it.
    let mut wrong = verify.clone();
    wrong["mutation"]["data"]["id"] = json!(missing);
    wrong["mutation"]["data"]["expectedOrganizationRevision"] = json!(2);
    assert_eq!(send(&app, &s, &wrong).await.0, StatusCode::NOT_FOUND);
    let remove = command(
        "remove_affiliation",
        json!({"id":missing,"organizationId":org,"reason":"Replace unverifiable relationship"}),
    );
    assert_eq!(send(&app, &s, &remove).await.0, StatusCode::OK);
}
