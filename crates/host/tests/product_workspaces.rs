//! Real mounted collection reads, current grants, parent scope and pagination.
mod common;
use archava_domain::erp::{
    accommodation::ACCOMMODATION_PERMISSIONS, assets::ASSET_PERMISSIONS,
    foodservice::FOOD_PERMISSIONS, organizations::ORGANIZATION_PERMISSIONS,
    projects::PROJECT_PERMISSIONS, proposals::PROPOSAL_PERMISSIONS,
    scheduling::SCHEDULING_PERMISSIONS,
};
use archava_host::{AppState, Ledger};
use archava_pg_fixture::provision;
use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;
const TABLES: &[&str] = &[
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
];
/// A command body that deserializes for module `id`, so the request reaches
/// the module check inside the tenant transaction. Each one is the simplest
/// create-style command of that module's own suite; `projects` instead names a
/// project that does not exist, because this suite counts the project rows
/// afterwards and the command is sent again once the module is enabled.
fn sample_command(id: &str) -> Option<Value> {
    let key = Uuid::new_v4().to_string();
    let inner = match id {
        "organizations" => json!({"mutation":{"requestId":key,"type":"create_organization","data":{
            "name":"Must not write","legalName":"PT Must not write","taxId":"01.234.567.8-999.000",
            "website":"https://acme.test","industry":"retail","billingAddress":"Jl. Merdeka 10, Jakarta"}}}),
        "proposals" => json!({"mutation":{"type":"create_proposal","commandId":key,
            "title":"Must not write","summary":"Disabled module","organizationId":null,
            "contactId":null,"leadId":null,"currency":"IDR","scopeItems":[{
                "title":"Scope","description":"Delivery of scope","estimatedDays":5,
                "estimatedValue":1_000,"deliverableType":"service"}]}}),
        "projects" => json!({"mutation":{"requestId":key,"command":{"operation":"task.create",
            "projectId":Uuid::new_v4().to_string(),"projectRevision":1,"title":"Must not write",
            "milestoneId":null,"required":true}}}),
        "foodservice" => json!({"requestId":key,"command":{"operation":"table.create",
            "code":"T-OFF","title":"Table T-OFF","seats":4}}),
        "scheduling" => json!({"mutation":{"requestId":key,"command":{
            "operation":"resource.create","title":"Must not write","kind":"room",
            "timezone":"Asia/Jakarta"}}}),
        "accommodation" => json!({"mutation":{"requestId":key,"command":{
            "operation":"property.create","title":"Must not write","timezone":"UTC"}}}),
        "assets" => json!({"mutation":{"requestId":key,"type":"create_asset","data":{
            "code":"OFF-001","name":"Must not write","category":"Pressure washer",
            "ownership":"business_owned"}}}),
        "subscriptions" => json!({"mutation":{"type":"create_subscription","commandId":key,
            "contactId":Uuid::new_v4().to_string(),"planCode":"off","planName":"Team plan",
            "billingInterval":"monthly","unitPrice":100_000,"currency":"IDR","quantity":1,
            "initialEntitlements":[{"featureCode":"api-calls","featureName":"API calls",
                "entitlementType":"metered","allocatedQuantity":10}]}}),
        "attendance" => json!({"mutation":{"type":"create_session","commandId":key,
            "title":"Must not write","sessionCode":"OFF-1",
            "startTime":"2026-11-01T09:00:00.000Z","endTime":"2026-11-01T17:00:00.000Z"}}),
        "laundry" => json!({"mutation":{"type":"create_order","commandId":key,
            "orderCode":"LND-OFF","kind":"weight","customerRef":"cust-off","currency":"USD",
            "lines":[{"itemRef":"wash-fold-kg","quantity":3000,"unit":"gram","unitPriceMinor":15}]}}),
        "field" => json!({"command":{"action":"create_job","commandId":key,"job":{
            "jobId":format!("job-{key}"),"customerRef":"cust-off","siteRef":"site-off",
            "summary":"Must not write","priority":"urgent","appointmentRef":"appt-off"}}}),
        "fulfillment" => json!({"mutation":{"type":"create_job","commandId":key,
            "jobCode":"FUL-OFF","kind":"dispatch","title":"Must not write","ownerRef":"ORD-OFF"}}),
        "education" => json!({"command":{"action":"admit_applicant","commandId":key,"enrollment":{
            "enrollmentId":format!("enr-{key}"),"studentRef":"student-off","classRef":"class-off",
            "termRef":"term-off","guardianRef":"guardian-off"}}}),
        _ => return None,
    };
    let mut body = inner;
    body["tenant"] = json!("acme-retail");
    Some(body)
}
/// A disabled module answers no command status either. The modules name
/// the receipt key differently, so each spelling is tried: a spelling the
/// module does not read is a malformed query, and the one it reads must be
/// refused as disabled.
async fn status_refused(app: &axum::Router, session: &Value, id: &str) {
    let key = Uuid::new_v4();
    let mut refusals = 0;
    for name in ["requestId", "commandId", "request_id"] {
        let (status, answer) = common::call(
            app,
            "GET",
            &format!("/erp/{id}/commands/status?tenant=acme-retail&{name}={key}"),
            json!({}),
            Some(session),
        )
        .await;
        if status == StatusCode::BAD_REQUEST {
            continue;
        }
        assert_eq!(status, StatusCode::FORBIDDEN, "{id} status: {answer}");
        assert_eq!(answer["reason"], "module_disabled", "{id}: {answer}");
        refusals += 1;
    }
    assert!(refusals > 0, "{id}: no status query spelling was read");
}
/// Rows in a module's command ledger, read as the owner role.
async fn command_rows(pool: &sqlx::PgPool, namespace: &str) -> i64 {
    let statement = format!("SELECT count(*) FROM {namespace}.commands");
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_one(pool)
        .await
        .unwrap()
}
#[tokio::test]
async fn thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants() {
    let f = provision().await;
    for actor in ["alice", "bob"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(actor).bind(format!("{actor}@archava.test")).bind(common::HASH).execute(&f.admin_pool).await.unwrap();
        for (schema, permissions) in [
            (
                "archava_commerce",
                archava_adapters::erp::COMMERCE_PERMISSION_VOCABULARY,
            ),
            ("archava_organizations", ORGANIZATION_PERMISSIONS),
            ("archava_proposals", PROPOSAL_PERMISSIONS),
            ("archava_projects", PROJECT_PERMISSIONS),
            ("archava_foodservice", FOOD_PERMISSIONS),
            ("archava_scheduling", SCHEDULING_PERMISSIONS),
            ("archava_accommodation", ACCOMMODATION_PERMISSIONS),
            ("archava_assets", ASSET_PERMISSIONS),
            (
                "archava_subscriptions",
                archava_domain::erp::subscriptions::SUBSCRIPTION_PERMISSIONS,
            ),
            (
                "archava_attendance",
                archava_domain::erp::attendance::ATTENDANCE_PERMISSIONS,
            ),
            (
                "archava_laundry",
                archava_domain::erp::laundry::LAUNDRY_PERMISSIONS,
            ),
            (
                "archava_field",
                archava_domain::erp::field_services::FIELD_PERMISSIONS,
            ),
            (
                "archava_fulfillment",
                archava_domain::erp::fulfillment::FULFILLMENT_PERMISSIONS,
            ),
            (
                "archava_edu",
                archava_adapters::erp::formal_education::EDU_PERMISSIONS,
            ),
        ] {
            let statement=format!("INSERT INTO {schema}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail',$1,'human',$2,'all',true)");
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .bind(actor)
                .bind(permissions)
                .execute(&f.admin_pool)
                .await
                .unwrap();
        }
    }
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    for (id, owner) in [(a, "alice"), (b, "bob")] {
        sqlx::query("INSERT INTO archava_projects.projects(tenant_id,id,owner_id,title,scope_summary) VALUES('acme-retail',$1,$2,$3,'Scoped project')").bind(id).bind(owner).bind(format!("{owner} project")).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_projects.tasks(tenant_id,id,project_id,title,required) VALUES('acme-retail',$1,$2,$3,true)").bind(Uuid::new_v4()).bind(id).bind(format!("{owner} task")).execute(&f.admin_pool).await.unwrap();
    }
    let state = AppState::try_new_erp(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "commerce",
            tables: TABLES,
        },
        &f.commerce_ledger,
        &f.identity_ledger,
        &[7; 32],
        &f.purchasing_ledger,
        &f.erp_readiness(),
    )
    .await
    .unwrap();
    let app = archava_host::router(state);
    let session = common::login(&app, "alice").await;
    for domain in [
        "organizations",
        "proposals",
        "projects",
        "foodservice",
        "scheduling",
        "accommodation",
        "assets",
        "subscriptions",
        "attendance",
        "laundry",
        "field",
        "fulfillment",
        "education",
    ] {
        let path = format!("/erp/{domain}/workspace?tenant=acme-retail");
        let (status, body) = common::call(&app, "GET", &path, json!({}), Some(&session)).await;
        assert_eq!(status, StatusCode::OK, "{domain}: {body}");
        assert!(!body["collections"].as_array().unwrap().is_empty());
        let (status, _) = common::call(&app, "GET", &path, json!({}), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = common::call(
            &app,
            "GET",
            &format!("/erp/{domain}/workspace?tenant=foreign"),
            json!({}),
            Some(&session),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions,active) VALUES('acme-retail','alice',ARRAY['config.read','config.apply'],true)").execute(&f.admin_pool).await.unwrap();
    let (_, config) = common::call(
        &app,
        "GET",
        "/own/config?tenant=acme-retail",
        json!({}),
        Some(&session),
    )
    .await;
    let mut expected = archava_domain::operator::catalogue()["modules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .filter(|id| *id != "crm")
        .collect::<std::collections::BTreeSet<_>>();
    expected.extend(["crm-contacts", "crm-pipeline"]);
    let actual = config["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().expect("owner capability id"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "Owner exposes each installed native capability, including Accounting"
    );
    let (_,discovery)=common::call(&app,"POST","/own/chat",json!({"tenant":"acme-retail","message":"Bisnis saya hotel, tambahkan laundry tanpa restaurant"}),Some(&session)).await;
    assert!(discovery["request"]["enable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "accommodation"));
    assert!(discovery["request"]["enable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "laundry"));
    assert_eq!(discovery["request"]["disable"], json!(["foodservice"]));
    for (revision, enable, disable) in [
        (0, json!([]), json!(["projects"])),
        (1, json!(["projects"]), json!([])),
    ] {
        let request = json!({"baseRevision":revision,"enable":enable,"disable":disable});
        let (_, planned) = common::call(
            &app,
            "POST",
            "/own/plan",
            json!({"tenant":"acme-retail","request":request}),
            Some(&session),
        )
        .await;
        let(status,result)=common::call(&app,"POST","/own/apply",json!({"tenant":"acme-retail","requestId":format!("project-toggle-{revision}"),"request":request,"confirmation":{"planHash":planned["plan"]["planHash"],"baseRevision":revision}}),Some(&session)).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let (status, _) = common::call(
            &app,
            "GET",
            "/erp/projects/workspace?tenant=acme-retail",
            json!({}),
            Some(&session),
        )
        .await;
        assert_eq!(
            status,
            if revision == 0 {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::OK
            }
        );
        if revision == 0 {
            let(status,refused)=common::call(&app,"POST","/erp/projects/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4().to_string(),"command":{"operation":"project.create","title":"Must not write","scopeSummary":"Disabled"}}}),Some(&session)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
            assert_eq!(refused["reason"], "module_disabled");
        }
    }
    let (_, blocked) = common::call(
        &app,
        "POST",
        "/own/plan",
        json!({"tenant":"acme-retail","request":{"baseRevision":2,"disable":["commerce"]}}),
        Some(&session),
    )
    .await;
    assert!(blocked["plan"]["blocked"].is_object(), "{blocked}");
    // OWN-T07 for every workspace module: disabling it (with whatever the
    // registry says requires it) refuses its workspace read while an unrelated
    // module keeps answering, and enabling it serves it again.
    let mut revision = 2;
    for domain in archava_adapters::erp::projections::WorkspaceDomain::ALL {
        let id = domain.slug();
        let mut toggled = vec![id];
        toggled.extend(
            archava_domain::modules::all()
                .iter()
                .filter(|module| module.owner_requires.contains(&id))
                .map(|module| module.id),
        );
        let other = if id == "assets" { "projects" } else { "assets" };
        let command = sample_command(id).expect("every workspace module has a sample command");
        for enabling in [false, true] {
            let request = if enabling {
                json!({"baseRevision":revision,"enable":toggled,"disable":[]})
            } else {
                json!({"baseRevision":revision,"enable":[],"disable":toggled})
            };
            let (_, planned) = common::call(
                &app,
                "POST",
                "/own/plan",
                json!({"tenant":"acme-retail","request":request}),
                Some(&session),
            )
            .await;
            assert!(planned["plan"]["blocked"].is_null(), "{id}: {planned}");
            let (status, result) = common::call(
                &app,
                "POST",
                "/own/apply",
                json!({"tenant":"acme-retail","requestId":format!("toggle-{id}-{revision}"),"request":request,"confirmation":{"planHash":planned["plan"]["planHash"],"baseRevision":revision}}),
                Some(&session),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{id}: {result}");
            revision += 1;
            let (status, read) = common::call(
                &app,
                "GET",
                &format!("/erp/{id}/workspace?tenant=acme-retail"),
                json!({}),
                Some(&session),
            )
            .await;
            if enabling {
                assert_eq!(status, StatusCode::OK, "{id} enabled again: {read}");
                let (_, answer) = common::call(
                    &app,
                    "POST",
                    &format!("/erp/{id}/commands"),
                    command.clone(),
                    Some(&session),
                )
                .await;
                assert_ne!(
                    answer["reason"], "module_disabled",
                    "{id} enabled again accepts commands: {answer}"
                );
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{id} disabled: {read}");
                assert_eq!(read["reason"], "module_disabled", "{id}: {read}");
                let before = command_rows(&f.admin_pool, domain.namespace()).await;
                let (status, refused) = common::call(
                    &app,
                    "POST",
                    &format!("/erp/{id}/commands"),
                    command.clone(),
                    Some(&session),
                )
                .await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{id} write: {refused}");
                assert_eq!(refused["reason"], "module_disabled", "{id}: {refused}");
                assert_eq!(
                    command_rows(&f.admin_pool, domain.namespace()).await,
                    before,
                    "{id}: a refused command records nothing"
                );
                status_refused(&app, &session, id).await;
                let (status, _) = common::call(
                    &app,
                    "GET",
                    &format!("/erp/{other}/workspace?tenant=acme-retail"),
                    json!({}),
                    Some(&session),
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{other} stays served without {id}");
            }
        }
    }
    assert_eq!(
        revision, 28,
        "thirteen modules were disabled and enabled again"
    );
    // The same for the ledger modules that have no workspace projection:
    // while disabled, the command route refuses before anything else is read.
    let key = || Uuid::new_v4().to_string();
    for (id, command) in [
        (
            "commerce",
            json!({"requestId":key(),"command":{"operation":"product.create","sku":"OFF-1","title":"Must not write","unit":"unit","trackStock":false,"price":1000}}),
        ),
        (
            "purchasing",
            json!({"requestId":key(),"command":{"operation":"supplier.create","code":"OFF-SUP","name":"Must not write"}}),
        ),
        (
            "billing",
            json!({"mutation":{"type":"create_invoice","commandId":key(),"invoiceCode":"INV-OFF","customerRef":"customer-off","currency":"IDR","consumerRef":"owner-off","lines":[{"description":"Night","quantity":1,"unitMinor":1000}]}}),
        ),
        (
            "support",
            json!({"mutation":{"requestId":key(),"command":{"operation":"case.create","requesterName":"Ada Lovelace","subject":"Must not write","description":"The module is disabled for this business.","priority":"normal","dueAt":null}}}),
        ),
        (
            "accounting",
            json!({"requestId":key(),"command":{"type":"journal.post","id":key(),"expectedRevision":1,"reason":"Must not write"}}),
        ),
    ] {
        let mut toggled = vec![id];
        toggled.extend(
            archava_domain::modules::all()
                .iter()
                .filter(|module| module.owner_requires.contains(&id))
                .map(|module| module.id),
        );
        let mut body = command;
        body["tenant"] = json!("acme-retail");
        for enabling in [false, true] {
            let request = if enabling {
                json!({"baseRevision":revision,"enable":toggled,"disable":[]})
            } else {
                json!({"baseRevision":revision,"enable":[],"disable":toggled})
            };
            let (_, planned) = common::call(
                &app,
                "POST",
                "/own/plan",
                json!({"tenant":"acme-retail","request":request}),
                Some(&session),
            )
            .await;
            assert!(planned["plan"]["blocked"].is_null(), "{id}: {planned}");
            let (status, result) = common::call(
                &app,
                "POST",
                "/own/apply",
                json!({"tenant":"acme-retail","requestId":format!("toggle-{id}-{revision}"),"request":request,"confirmation":{"planHash":planned["plan"]["planHash"],"baseRevision":revision}}),
                Some(&session),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{id}: {result}");
            revision += 1;
            if !enabling {
                let (status, refused) = common::call(
                    &app,
                    "POST",
                    &format!("/erp/{id}/commands"),
                    body.clone(),
                    Some(&session),
                )
                .await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{id} write: {refused}");
                assert_eq!(refused["reason"], "module_disabled", "{id}: {refused}");
                status_refused(&app, &session, id).await;
            }
        }
    }
    let (_, all) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail",
        json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(all["projects"].as_array().unwrap().len(), 2);
    assert_eq!(all["tasks"].as_array().unwrap().len(), 2);
    let (_, first) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail&collection=projects&limit=1",
        json!({}),
        Some(&session),
    )
    .await;
    let cursor = first["collections"][0]["nextCursor"].as_str().unwrap();
    let (_,second)=common::call(&app,"GET",&format!("/erp/projects/workspace?tenant=acme-retail&collection=projects&limit=1&cursor={cursor}"),json!({}),Some(&session)).await;
    assert_ne!(first["projects"][0]["id"], second["projects"][0]["id"]);
    sqlx::query(
        "UPDATE archava_projects.memberships SET scope='assigned' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let (status, assigned) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail",
        json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assigned["projects"].as_array().unwrap().len(), 1);
    assert_eq!(assigned["projects"][0]["id"], a.to_string());
    assert_eq!(assigned["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(assigned["tasks"][0]["title"], "alice task");
    sqlx::query("UPDATE archava_projects.memberships SET permissions=ARRAY['projects.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (_, narrow) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail",
        json!({}),
        Some(&session),
    )
    .await;
    assert!(narrow.get("tasks").is_none());
    let (status, _) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail&collection=tasks",
        json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for query in [
        "limit=0",
        "limit=101",
        "collection=memberships",
        "limit=not-a-number",
    ] {
        let (status, _) = common::call(
            &app,
            "GET",
            &format!("/erp/projects/workspace?tenant=acme-retail&{query}"),
            json!({}),
            Some(&session),
        )
        .await;
        assert!(status.is_client_error());
    }
    sqlx::query("UPDATE archava_projects.memberships SET active=false WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (status, _) = common::call(
        &app,
        "GET",
        "/erp/projects/workspace?tenant=acme-retail",
        json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
