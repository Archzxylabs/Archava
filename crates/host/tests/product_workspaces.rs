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
use serde_json::json;
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
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{id} disabled: {read}");
                assert_eq!(read["reason"], "module_disabled", "{id}: {read}");
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
