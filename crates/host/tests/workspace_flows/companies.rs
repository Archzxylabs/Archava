use super::*;
async fn fixture() -> (Fixture, Router, Value) {
    let (f, app, s) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(if actor=="reader"{vec!["companies.read","audit.read"]}else{vec!["companies.read","companies.write","organizations.read","organizations.write","parties.read","parties.write","audit.read"]}).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
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
async fn submit_org(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
    let (st, b) = send(app, s, &input(kind, data)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b["receipt"].clone()
}
async fn org(app: &Router, s: &Value, name: &str) -> Value {
    submit_org(
        app,
        s,
        "create_organization",
        json!({"name":name,"legalName":format!("{name} Official"),"taxId":"UNVERIFIED-REF"}),
    )
    .await
}
fn create(o: &Value, code: &str) -> Value {
    input(
        "company.create_from_organization",
        json!({"code":code,"organizationId":o["organizationId"],"expectedSourceRevision":o["revision"],"reason":"Declare our managed company"}),
    )
}
fn activity(c: &Value, revision: i32, active: bool) -> Value {
    input(
        "company.set_active",
        json!({"id":c["companyId"],"expectedRevision":revision,"active":active,"reason":"Reviewed activity change"}),
    )
}
fn refresh(c: &Value, revision: i32, source: i32) -> Value {
    input(
        "company.refresh_source",
        json!({"id":c["companyId"],"expectedRevision":revision,"expectedSourceRevision":source,"reason":"Reviewed official source profile"}),
    )
}
async fn work(app: &Router, s: &Value) -> Value {
    let (st, b) = snapshot(app, s, "organizations").await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b
}
async fn rename(app: &Router, s: &Value, o: &Value, revision: i32, name: &str) {
    submit_org(app,s,"update_organization",json!({"id":o["organizationId"],"revision":revision,"name":name,"legalName":format!("{name} Official"),"taxId":format!("REF-{revision}")})).await;
}
async fn status(app: &Router, s: &Value, b: &Value) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/organizations/commands/status?tenant=acme&requestId={}",
            b["mutation"]["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(s),
    )
    .await
}
fn company(w: &Value, c: &Value) -> Value {
    w["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == c["companyId"])
        .unwrap()
        .clone()
}
#[tokio::test]
async fn managed_company_identity_is_explicit_distinct_and_keeps_reviewed_version_history() {
    let (_f, app, s) = fixture().await;
    let a = org(&app, &s, "Shared name").await;
    let b = org(&app, &s, "Shared name").await;
    let party=submit_org(&app,&s,"party.create_from_organization",json!({"organizationId":a["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":true})).await;
    assert_eq!(work(&app, &s).await["companies"], json!([]));
    let ca = create(&a, "A");
    let (st, r) = send(&app, &s, &ca).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let c = &r["receipt"];
    let (st, other) = send(&app, &s, &create(&b, "B")).await;
    assert_eq!(st, StatusCode::OK, "{other}");
    assert_ne!(c["companyId"], other["receipt"]["companyId"]);
    assert_ne!(c["companyId"], party["partyId"]);
    assert_eq!(
        send(&app, &s, &create(&a, "A-AGAIN")).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &s, &create(&b, "A")).await.0,
        StatusCode::CONFLICT
    );
    rename(&app, &s, &a, 1, "Changed company").await;
    assert_eq!(
        company(&work(&app, &s).await, c)["legalName"],
        "Shared name Official"
    );
    assert_eq!(
        send(&app, &s, &activity(c, 1, false)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, &s, &activity(c, 2, true)).await.0,
        StatusCode::CONFLICT
    );
    let update = refresh(c, 2, 2);
    assert_eq!(send(&app, &s, &update).await.0, StatusCode::OK);
    assert_eq!(
        send(&app, &s, &activity(c, 3, true)).await.0,
        StatusCode::OK
    );
    for command in [&ca, &update] {
        let (st, b) = send(&app, &s, command).await;
        assert_eq!(st, StatusCode::OK, "{b}");
        assert_eq!(b["receipt"]["companyId"], c["companyId"]);
        assert_eq!(b["receipt"]["replayed"], true);
    }
    let w = work(&app, &s).await;
    let row = company(&w, c);
    assert_eq!(row["code"], "A");
    assert_eq!(row["sourceOrganizationId"], a["organizationId"]);
    assert_eq!(row["sourceRevision"], 2);
    assert_eq!(row["revision"], 4);
    assert_eq!(row["active"], true);
    assert_eq!(row["legalName"], "Changed company Official");
    assert_eq!(row["taxId"], "REF-1");
    assert!(row.get("accountingMode").is_none());
    assert!(row.get("currency").is_none());
    assert!(row.get("verified").is_none());
    let mut history = w["company_audit"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["companyId"] == c["companyId"])
        .collect::<Vec<_>>();
    history.sort_by_key(|r| r["revision"].as_i64());
    assert_eq!(history.len(), 4);
    assert_eq!(history[0]["snapshot"]["legalName"], "Shared name Official");
    assert_eq!(
        history[3]["snapshot"]["legalName"],
        "Changed company Official"
    );
}
#[tokio::test]
async fn managed_company_replay_and_status_recheck_current_authority_without_rewriting_old_profiles(
) {
    let (f, app, s) = fixture().await;
    let o = org(&app, &s, "Authority").await;
    let request = create(&o, "AUTH");
    let (st, b) = send(&app, &s, &request).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let c = &b["receipt"];
    submit_org(
        &app,
        &s,
        "archive_organization",
        json!({"id":o["organizationId"],"revision":1,"reason":"Archive source"}),
    )
    .await;
    assert_eq!(send(&app, &s, &request).await.0, StatusCode::OK);
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['companies.read','companies.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(send(&app, &s, &request).await.0, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, &s, &request).await.0, StatusCode::OK);
    assert_eq!(
        send(&app, &s, &activity(c, 1, false)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, &s, &activity(c, 2, true)).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['companies.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(status(&app, &s, &request).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        send(&app, &s, &activity(c, 2, true)).await.0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn managed_company_refuses_missing_official_name_archived_stale_foreign_and_forged_sources() {
    let (f, app, s) = fixture().await;
    let no_name = submit_org(
        &app,
        &s,
        "create_organization",
        json!({"name":"No official name"}),
    )
    .await;
    assert_eq!(
        send(&app, &s, &create(&no_name, "NO-NAME")).await.0,
        StatusCode::CONFLICT
    );
    let o = org(&app, &s, "Stale source").await;
    rename(&app, &s, &o, 1, "New source").await;
    assert_eq!(
        send(&app, &s, &create(&o, "STALE")).await.0,
        StatusCode::CONFLICT
    );
    let mut missing = create(&o, "MISSING");
    missing["mutation"]["data"]["organizationId"] = json!(Uuid::new_v4());
    assert_eq!(send(&app, &s, &missing).await.0, StatusCode::NOT_FOUND);
    let mut foreign = create(&o, "FOREIGN");
    foreign["tenant"] = json!("foreign");
    assert_eq!(send(&app, &s, &foreign).await.0, StatusCode::FORBIDDEN);
    for key in [
        "legalName",
        "verified",
        "currency",
        "ownerId",
        "accountingMode",
    ] {
        let mut forged = create(&o, "FORGED");
        forged["mutation"]["data"][key] = json!("forged");
        assert_eq!(send(&app, &s, &forged).await.0, StatusCode::BAD_REQUEST);
    }
    submit_org(
        &app,
        &s,
        "archive_organization",
        json!({"id":o["organizationId"],"revision":2,"reason":"Archive source"}),
    )
    .await;
    let mut archived = create(&o, "ARCHIVED");
    archived["mutation"]["data"]["expectedSourceRevision"] = json!(3);
    assert_eq!(send(&app, &s, &archived).await.0, StatusCode::CONFLICT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_organizations.companies")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
#[tokio::test]
async fn managed_company_scope_grants_service_kind_and_private_history_are_independent() {
    let (f, app, s) = fixture().await;
    let o = org(&app, &s, "Private company").await;
    let request = create(&o, "PRIVATE");
    assert_eq!(send(&app, &s, &request).await.0, StatusCode::OK);
    for actor in ["reader", "service-seat", "assigned"] {
        let peer = login(&app, actor).await;
        assert!(matches!(
            send(&app, &peer, &create(&o, &actor.to_uppercase()))
                .await
                .0,
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ));
        assert_eq!(status(&app, &peer, &request).await.0, StatusCode::FORBIDDEN);
    }
    let reader = login(&app, "reader").await;
    let w = work(&app, &reader).await;
    assert_eq!(w["companies"].as_array().unwrap().len(), 1);
    assert_eq!(w["company_audit"].as_array().unwrap().len(), 1);
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['audit.read','organizations.read'] WHERE principal_id='reader'").execute(&f.admin_pool).await.unwrap();
    let w = work(&app, &reader).await;
    assert!(w.get("companies").is_none());
    assert!(w.get("company_audit").is_none());
    let path = "/erp/organizations/workspace?tenant=acme&collection=company_audit";
    assert_eq!(
        call(&app, "GET", path, Value::Null, Some(&reader)).await.0,
        StatusCode::FORBIDDEN
    );
    let assigned = login(&app, "assigned").await;
    let own = org(&app, &assigned, "Assigned company").await;
    let (st, c) = send(&app, &assigned, &create(&own, "ASSIGNED")).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    let w = work(&app, &assigned).await;
    assert_eq!(w["companies"].as_array().unwrap().len(), 1);
    assert_eq!(w["company_audit"].as_array().unwrap().len(), 1);
    // Forced tenant RLS remains effective without a caller-supplied tenant predicate.
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL archava.tenant_id='foreign'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_organizations.companies")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 0);
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn managed_company_source_uniqueness_and_concurrent_changes_are_atomic() {
    let (_f, app, s) = fixture().await;
    let o = org(&app, &s, "Concurrent company").await;
    let a = create(&o, "A");
    let b = create(&o, "B");
    let (a, b) = tokio::join!(send(&app, &s, &a), send(&app, &s, &b));
    assert_eq!(
        [a.0, b.0].iter().filter(|s| **s == StatusCode::OK).count(),
        1
    );
    assert_eq!(
        [a.0, b.0]
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let c = if a.0 == StatusCode::OK {
        a.1["receipt"].clone()
    } else {
        b.1["receipt"].clone()
    };
    rename(&app, &s, &o, 1, "Concurrent refresh").await;
    let a = refresh(&c, 1, 2);
    let b = activity(&c, 1, false);
    let (a, b) = tokio::join!(send(&app, &s, &a), send(&app, &s, &b));
    assert_eq!(
        [a.0, b.0].iter().filter(|s| **s == StatusCode::OK).count(),
        1
    );
    assert_eq!(
        [a.0, b.0]
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let w = work(&app, &s).await;
    assert_eq!(company(&w, &c)["revision"], 2);
    assert_eq!(w["company_audit"].as_array().unwrap().len(), 2);
}
#[tokio::test]
async fn managed_company_database_rejects_forgery_repointing_missing_audit_and_history_mutation() {
    let (f, app, s) = fixture().await;
    let o = org(&app, &s, "Guarded company").await;
    let (st, b) = send(&app, &s, &create(&o, "GUARD")).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let id = Uuid::parse_str(b["receipt"]["companyId"].as_str().unwrap()).unwrap();
    for change in [
        "code='FORGED'",
        "source_organization_id='00000000-0000-4000-8000-000000000001'",
        "legal_name='FORGED',source_revision=2",
        "active=false",
    ] {
        let query = format!(
            "UPDATE archava_organizations.companies SET {change},revision=revision+1 WHERE id=$1"
        );
        let e = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .bind(id)
            .execute(&f.admin_pool)
            .await
            .unwrap_err();
        assert_eq!(
            e.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514"),
            "{e}"
        );
    }
    assert!(
        sqlx::query("DELETE FROM archava_organizations.companies WHERE id=$1")
            .bind(id)
            .execute(&f.admin_pool)
            .await
            .is_err()
    );
    assert!(sqlx::query(
        "UPDATE archava_organizations.company_audit SET snapshot='{}' WHERE company_id=$1"
    )
    .bind(id)
    .execute(&f.admin_pool)
    .await
    .is_err());
    let allowed:bool=sqlx::query_scalar("SELECT has_table_privilege(current_user,'archava_organizations.companies','DELETE') OR has_table_privilege(current_user,'archava_organizations.company_audit','UPDATE') OR has_table_privilege(current_user,'archava_organizations.company_audit','DELETE')").fetch_one(&f.runtime_pool).await.unwrap();
    assert!(!allowed);
}
#[tokio::test]
async fn managed_company_source_changes_are_serialized_with_registration_or_refused() {
    let (_f, app, s) = fixture().await;
    let o = org(&app, &s, "Reviewed source").await;
    let create = create(&o, "RACE");
    let update = input(
        "update_organization",
        json!({"id":o["organizationId"],"revision":1,"name":"Later source","legalName":"Later source Official"}),
    );
    let (a, b) = tokio::join!(send(&app, &s, &create), send(&app, &s, &update));
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert!(
        matches!(a.0, StatusCode::OK | StatusCode::CONFLICT),
        "{}",
        a.1
    );
    for c in work(&app, &s).await["companies"].as_array().unwrap() {
        assert_eq!(c["sourceRevision"], 1);
        assert_eq!(c["legalName"], "Reviewed source Official");
    }
}
#[tokio::test]
async fn managed_company_approved_mission_and_shared_command_namespace_preserve_exact_receipts() {
    let (_f, app, s) = fixture().await;
    let o = org(&app, &s, "Approved company").await;
    let body = json!({"tenant_id":"acme","mission_code":"managed-company","title":"Register our reviewed company","description":"Synthetic declared company profile","auto_launch":true,"steps":[{"step_index":0,"step_name":"Register company","tool_name":"organizations.company.create_from_organization","payload_input":{"code":"MISSION","organizationId":o["organizationId"],"expectedSourceRevision":1,"reason":"Declare our company"},"depends_on":[]}]});
    let (st, m) = call(&app, "POST", "/api/v1/cockpit/missions", body, Some(&s)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    let base = format!("/api/v1/cockpit/missions/{}", m["id"].as_str().unwrap());
    let run = format!("{base}/run-next?tenant=acme");
    assert_eq!(
        call(&app, "POST", &run, json!({"stepIndex":0}), Some(&s))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(work(&app, &s).await["companies"], json!([]));
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
    let (st, b) = call(&app, "POST", &run, json!({"stepIndex":0}), Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "touchdown");
    assert_eq!(b["steps"][0]["payload_output"]["verified"], true);
    let request = activity(
        &json!({"companyId":work(&app,&s).await["companies"][0]["id"]}),
        1,
        false,
    );
    assert_eq!(send(&app, &s, &request).await.0, StatusCode::OK);
    let mut reused = input("create_organization", json!({"name":"Must conflict"}));
    reused["mutation"]["requestId"] = request["mutation"]["requestId"].clone();
    assert_eq!(send(&app, &s, &reused).await.0, StatusCode::CONFLICT);
    assert_eq!(
        status(&app, &s, &request).await.1["receipt"]["type"],
        "company.set_active"
    );
    let missing = input("company.set_active", json!({}));
    assert_eq!(
        status(&app, &s, &missing).await.1["decision"],
        "not_committed"
    );
}
#[tokio::test]
async fn managed_company_upgrade_preserves_existing_organizations_parties_and_membership_grants() {
    let f = archava_pg_fixture::provision_pre_company_profile().await;
    assert_eq!(f.organizations_ledger.len(), 3);
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',ARRAY['organizations.read','organizations.write','parties.read','parties.write'],'all')").execute(&f.admin_pool).await.unwrap();
    let source = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_organizations.organizations(tenant_id,id,name,legal_name,owner_id) VALUES('acme',$1,'Existing organization','Existing Official','alice')").bind(source).execute(&f.admin_pool).await.unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(o) FROM archava_organizations.organizations o WHERE id=$1",
    )
    .bind(source)
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_organizations",
        "organizations",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let principal = archava_domain::erp::purchasing::PurchasePrincipal::human("alice");
    let access = archava_adapters::erp::parties::access(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
    )
    .await
    .unwrap();
    let mutation=archava_domain::erp::parties::parse(&json!({"requestId":Uuid::new_v4(),"type":"party.create_from_organization","data":{"organizationId":source,"expectedSourceRevision":1,"customer":true,"supplier":true}})).unwrap();
    archava_adapters::erp::parties::record(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
        &access,
        &"a".repeat(64),
        &mutation,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // Organizations 005 adds tax_id to parties. The comparison leaves it out
    // so a prior party compares equal across the upgrade; it must arrive empty
    // (asserted below).
    let snapshot="SELECT json_build_object('parties',(SELECT json_agg(to_jsonb(p)-'tax_id' ORDER BY id) FROM archava_organizations.parties p),'audit',(SELECT json_agg(to_jsonb(a) ORDER BY request_id) FROM archava_organizations.party_audit a),'commands',(SELECT json_agg(to_jsonb(c) ORDER BY request_id) FROM archava_organizations.commands c))";
    let prior: Value = sqlx::query_scalar(snapshot)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    let now = archava_pg_fixture::apply_pending_organization_migrations(&f.owner_pool).await;
    assert_eq!(now.len(), 5);
    assert_eq!(
        archava_pg_fixture::apply_pending_organization_migrations(&f.owner_pool).await,
        now
    );
    let after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(o) FROM archava_organizations.organizations o WHERE id=$1",
    )
    .bind(source)
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    let preserved: Value = sqlx::query_scalar(snapshot)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(prior, preserved);
    let filled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_organizations.parties WHERE tax_id IS NOT NULL",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        filled, 0,
        "an upgrade copies no tax id onto an existing party"
    );
    let(n,permissions):(i64,Vec<String>)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_organizations.companies),permissions FROM archava_organizations.memberships WHERE tenant_id='acme' AND principal_id='alice'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(n, 0);
    assert!(!permissions.iter().any(|p| p.starts_with("companies.")));
}
