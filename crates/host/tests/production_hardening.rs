//! Permanent HTTP/PostgreSQL regressions for the November production audit.
mod common;
mod email {
    //! Real router + PostgreSQL + owned mock Resend HTTP provider. No live email service is contacted.

    use crate::common::{call, login, HASH};
    use archava_host::{
        mail::{EmailIntegration, EmailRegistry},
        router,
        router::AllReadiness,
        AppState, Ledger,
    };
    use archava_pg_fixture::{provision, Fixture};
    use archava_providers::resend::ResendClient;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    const KEY: &str = "re_synthetic_owned_email_key_12345";

    #[derive(Default)]
    struct Responses {
        send_count: u32,
        last_idempotency_key: String,
        last_to: Vec<String>,
        last_html: String,
    }

    struct Mock {
        origin: String,
        state: Arc<Mutex<Responses>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn mock_send(
        State(state): State<Arc<Mutex<Responses>>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let mut s = state.lock().unwrap();
        s.send_count += 1;
        if let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) {
            s.last_idempotency_key = key.to_string();
        }
        if let Some(to) = body.get("to").and_then(|v| v.as_array()) {
            s.last_to = to
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        s.last_html = body["html"].as_str().unwrap_or("").into();
        (StatusCode::OK, Json(json!({"id": "resend_msg_test_9876"})))
    }

    impl Mock {
        async fn start() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}/", listener.local_addr().unwrap());
            let state = Arc::new(Mutex::new(Responses::default()));
            let app = Router::new()
                .route("/emails", post(mock_send))
                .with_state(state.clone());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            Self {
                origin,
                state,
                task,
            }
        }

        fn registry(&self) -> EmailRegistry {
            EmailRegistry::new(vec![EmailIntegration {
                id: "resend-primary".into(),
                tenant: archava_core::tenant::TenantId::new("acme").unwrap(),
                from_address: "Acme Billing <billing@acme.com>".into(),
                client: ResendClient::for_loopback(&self.origin, KEY.into()).unwrap(),
            }])
            .unwrap()
        }
    }

    async fn setup(mock: &Mock) -> (Fixture, Router, Value, Uuid) {
        let f = provision().await;
        for user in ["alice", "bob"] {
            sqlx::query(
            "INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)",
        )
        .bind(user)
        .bind(format!("{user}@archava.test"))
        .bind(HASH)
        .execute(&f.admin_pool)
        .await
        .unwrap();

            sqlx::query(
            "INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) \
             VALUES('acme',$1,'human',$2,'all',true)",
        )
        .bind(user)
        .bind(if user == "alice" {
            vec!["billing.read", "billing.write", "billing.settle"]
        } else {
            vec!["billing.read"]
        })
        .execute(&f.admin_pool)
        .await
        .unwrap();
        }

        let additional = f.erp_readiness();
        let state = AppState::try_new_all(
            f.runtime_pool.clone(),
            Ledger {
                namespace: "archava_commerce",
                ledger: "Commerce",
                tables: &[
                    "products",
                    "stock_balances",
                    "orders",
                    "order_lines",
                    "allocations",
                    "movements",
                    "returns",
                    "commands",
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
                additional_erp: &additional,
            },
            &[7u8; 32],
        )
        .await
        .unwrap()
        .with_email_registry(mock.registry());

        let app = router(state);
        let session = login(&app, "alice").await;

        let (s, invoice) = call(
            &app,
            "POST",
            "/erp/billing/commands",
            json!({
                "tenant": "acme",
                "mutation": {
                    "type": "create_invoice",
                    "commandId": Uuid::new_v4(),
                    "invoiceCode": "INV-EMAIL",
                    "customerRef": "Customer Co",
                    "consumerRef": "test",
                    "currency": "IDR",
                    "lines": [{"description": "Service", "quantity": 1, "unitMinor": 100}]
                }
            }),
            Some(&session),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{invoice}");
        let invoice_id = Uuid::parse_str(invoice["invoiceId"].as_str().unwrap()).unwrap();

        let (s, b) = call(
            &app,
            "POST",
            "/erp/billing/commands",
            json!({
                "tenant": "acme",
                "mutation": {
                    "type": "issue_invoice",
                    "commandId": Uuid::new_v4(),
                    "invoiceId": invoice_id,
                    "expectedRevision": 1,
                    "reason": "Ready for email"
                }
            }),
            Some(&session),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{b}");

        (f, app, session, invoice_id)
    }

    #[tokio::test]
    async fn audit_revoked_email_actor_must_not_replay_receipt() {
        let mock = Mock::start().await;
        let (f, app, auth, invoice_id) = setup(&mock).await;
        let path = format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme");
        let body = json!({"requestId":Uuid::new_v4(),"recipient":"customer@example.com","subject":"Audit invoice"});
        let (status, _) = call(&app, "POST", &path, body.clone(), Some(&auth)).await;
        assert_eq!(status, StatusCode::OK);
        sqlx::query(
            "UPDATE archava_billing.memberships SET active=false WHERE principal_id='alice'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        let (list_status, _) = call(
            &app,
            "GET",
            &format!("/erp/billing/invoices/{invoice_id}/emails?tenant=acme"),
            json!({}),
            Some(&auth),
        )
        .await;
        assert_eq!(
            list_status,
            StatusCode::FORBIDDEN,
            "control: ordinary history read refuses revoked membership"
        );
        let (status, receipt) = call(&app, "POST", &path, body, Some(&auth)).await;
        println!(
            "AUDIT revoked replay: status={status}, returned_state={}, provider_calls={}",
            receipt["state"],
            mock.state.lock().unwrap().send_count
        );
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "revoked membership must refuse cached email receipt"
        );
    }
    #[tokio::test]
    async fn audit_second_email_intent_must_have_recoverable_identity() {
        let mock = Mock::start().await;
        let (_f, app, auth, invoice_id) = setup(&mock).await;
        let path = format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme");
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        for key in [first, second] {
            let (status,receipt)=call(&app,"POST",&path,json!({"requestId":key,"recipient":"customer@example.com","subject":"Same invoice"}),Some(&auth)).await;
            if key == first {
                assert_eq!(status, StatusCode::OK);
            }
            if key == second {
                assert_eq!(status, StatusCode::CONFLICT);
                assert_eq!(receipt["reason"], "email_already_recorded");
                assert_eq!(receipt["duplicateOf"], json!(first));
                let (_, history) = call(
                    &app,
                    "GET",
                    &format!(
                        "/erp/billing/invoices/{invoice_id}/emails?tenant=acme&requestId={second}"
                    ),
                    json!({}),
                    Some(&auth),
                )
                .await;
                println!("AUDIT second intent: submitted={second}, returned={}, exact_recovery={history}, provider_calls={}",receipt["requestId"],mock.state.lock().unwrap().send_count);
                assert_eq!(
                    receipt["requestId"],
                    json!(second),
                    "a duplicate refusal must acknowledge exactly the rejected key"
                );
            }
        }
    }
    #[tokio::test]
    async fn terminal_and_expired_email_intents_require_current_scope_and_actor() {
        let mock = Mock::start().await;
        let (f, app, alice, invoice) = setup(&mock).await;
        let path = format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme");
        let body = json!({"requestId":Uuid::new_v4(),"recipient":"customer@example.com","subject":"Access regression"});
        assert_eq!(
            call(&app, "POST", &path, body.clone(), Some(&alice))
                .await
                .0,
            StatusCode::OK
        );
        let bob = login(&app, "bob").await;
        sqlx::query("UPDATE archava_billing.memberships SET permissions=ARRAY['billing.read','billing.write'] WHERE principal_id='bob'").execute(&f.admin_pool).await.unwrap();
        assert_eq!(
            call(&app, "POST", &path, body.clone(), Some(&bob)).await.0,
            StatusCode::FORBIDDEN,
            "another writer cannot adopt the initiator's key"
        );
        sqlx::query(
            "UPDATE archava_billing.memberships SET scope='assigned' WHERE principal_id='bob'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        assert_eq!(
            call(&app, "POST", &path, body, Some(&bob)).await.0,
            StatusCode::NOT_FOUND,
            "assigned writer cannot read an unassigned invoice through replay"
        );
        let expired = Uuid::new_v4();
        sqlx::query("INSERT INTO archava_billing.email_outbox(tenant_id,id,invoice_id,actor_id,request_id,recipient,subject,payload_sha256,state,provider,created_at,updated_at) VALUES('acme',$1,$2,'alice',$3,'customer@example.com','Expired',repeat('0',64),'invoking','resend',clock_timestamp()-interval '25 hours',clock_timestamp()-interval '25 hours')")
            .bind(Uuid::new_v4()).bind(invoice).bind(expired).execute(&f.admin_pool).await.unwrap();
        sqlx::query(
            "UPDATE archava_billing.memberships SET active=false WHERE principal_id='alice'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        assert_eq!(
            call(
                &app,
                "POST",
                &path,
                json!({"requestId":expired,"recipient":"customer@example.com","subject":"Expired"}),
                Some(&alice)
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let stored: String = sqlx::query_scalar(
            "SELECT state FROM archava_billing.email_outbox WHERE request_id=$1",
        )
        .bind(expired)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
        assert_eq!(
            stored, "invoking",
            "a revoked caller cannot hold an expired intent"
        );
        assert_eq!(mock.state.lock().unwrap().send_count, 1);
    }
}
mod memory {
    //! Real router + non-owner PostgreSQL regression tests for durable mission commands.
    use archava_host::{router, AppState, Ledger};
    use archava_pg_fixture::{provision, Fixture};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        Router,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const KEY: &[u8] = &[7u8; 32];
    const PASSWORD: &str = "correct horse battery staple";
    const HASH: &str =
        "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";
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
    fn ledger() -> Ledger {
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: TABLES,
        }
    }
    async fn state(f: &Fixture, pool: sqlx::PgPool) -> AppState {
        AppState::try_new(pool, ledger(), &f.commerce_ledger, &f.identity_ledger, KEY)
            .await
            .unwrap()
    }
    async fn setup() -> (Fixture, AppState, Router, Value) {
        let f = provision().await;
        for (id, tenant, capability, scope) in [
            ("alice", "acme", "enterprise", "all"),
            ("viewer", "acme", "assist", "all"),
            ("assigned", "acme", "act", "assigned"),
            ("mallory", "other", "enterprise", "all"),
        ] {
            sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(id).bind(format!("{id}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
            sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human','{}',$3,true,'owner',$4)").bind(tenant).bind(id).bind(scope).bind(capability).execute(&f.admin_pool).await.unwrap();
        }
        let state = state(&f, f.runtime_pool.clone()).await;
        let app = router(state.clone());
        let session = login(&app, "alice").await;
        (f, state, app, session)
    }
    async fn call(
        app: &Router,
        method: &str,
        path: &str,
        body: Value,
        session: Option<&Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-archava-client", "cli");
        if let Some(s) = session {
            request = request
                .header(
                    "authorization",
                    format!("Bearer {}", s["token"].as_str().unwrap()),
                )
                .header("x-archava-csrf", s["csrf"].as_str().unwrap());
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn login(app: &Router, user: &str) -> Value {
        let (status, value) = call(
            app,
            "POST",
            "/login",
            json!({"username":format!("{user}@archava.test"),"password":PASSWORD}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }
    fn memory(scope: Value, min: &str) -> Value {
        json!({"tenant_id":"acme","scope":scope,"category":"notes","resource":"audit","summary_l0":"confidential audit fact","overview_l1":"confidential audit fact","detail_l2":{"secret":42},"min_authority":min})
    }
    #[tokio::test]
    async fn audit_department_claim_must_not_grant_memory_access() {
        let (f, _state, app, owner) = setup().await;
        sqlx::query(
            "UPDATE archava_commerce.memberships SET role='viewer' WHERE principal_id='viewer'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/crystallize",
            memory(json!({"department":"finance"}), "L1"),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let reader = login(&app, "viewer").await;
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/query",
            json!({"tenant_id":"acme","tier":"L2RawPayload"}),
            Some(&reader),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.as_array().unwrap().is_empty());
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/query",
            json!({"tenant_id":"acme","tier":"L2RawPayload","department":"finance"}),
            Some(&reader),
        )
        .await;
        println!("AUDIT forged department: status={status}, result={body}");
        assert!(
            body.as_array().unwrap().is_empty(),
            "a request body cannot confer finance department membership"
        );
    }
    #[tokio::test]
    async fn audit_non_owner_must_not_write_sovereign_memory() {
        let (f, _state, app, _owner) = setup().await;
        sqlx::query(
            "UPDATE archava_commerce.memberships SET role='editor' WHERE principal_id='assigned'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        let editor = login(&app, "assigned").await;
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/crystallize",
            memory(json!("sovereign"), "L5"),
            Some(&editor),
        )
        .await;
        println!("AUDIT sovereign write by assigned editor: status={status}, result={body}");
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "only an authorized owner may write sovereign executive memory"
        );
    }
    #[tokio::test]
    async fn audit_memory_must_survive_cold_host_restart() {
        let (f, _state, app, owner) = setup().await;
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/crystallize",
            memory(json!("public"), "L1"),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let query = json!({"tenant_id":"acme","tier":"L0Abstract"});
        let (_, warm) = call(
            &app,
            "POST",
            "/api/v1/orbz/query",
            query.clone(),
            Some(&owner),
        )
        .await;
        assert_eq!(warm.as_array().unwrap().len(), 1);
        let cold = router(state(&f, f.runtime_pool.clone()).await);
        let (status, body) = call(&cold, "POST", "/api/v1/orbz/query", query, Some(&owner)).await;
        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM archava_orbz.nodes WHERE tenant_id='acme'")
                .fetch_one(&f.admin_pool)
                .await
                .unwrap();
        println!(
            "AUDIT cold memory: status={status}, warm={warm}, cold={body}, durable_rows={stored}"
        );
        assert_eq!(
            body.as_array().unwrap().len(),
            1,
            "committed memory must load through tenant-scoped SQL after restart"
        );
    }
    #[tokio::test]
    async fn audit_entity_memory_scope_must_persist() {
        let (_f, _state, app, owner) = setup().await;
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/orbz/crystallize",
            memory(
                json!({"entity":{"entity_type":"customer","entity_id":"c1"}}),
                "L1",
            ),
            Some(&owner),
        )
        .await;
        println!("AUDIT entity memory: status={status}, result={body}");
        assert_eq!(
            status,
            StatusCode::CREATED,
            "valid entity scope must fit SQL scope column"
        );
    }
    async fn proxied_login(app: &Router, forwarded: &str, password: &str) -> StatusCode {
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;
        let request = Request::builder()
            .method("POST")
            .uri("/login")
            .header("content-type", "application/json")
            .header("x-archava-client", "cli")
            .header("x-forwarded-for", forwarded)
            .extension(ConnectInfo(
                "127.0.0.1:52345".parse::<SocketAddr>().unwrap(),
            ))
            .body(Body::from(
                json!({"username":"alice@archava.test","password":password}).to_string(),
            ))
            .unwrap();
        app.clone().oneshot(request).await.unwrap().status()
    }
    #[tokio::test]
    async fn audit_proxy_clients_must_not_share_account_lockout() {
        let (_f, state, _app, _owner) = setup().await;
        let app = router(
            state.with_trusted_proxies(archava_host::TrustedProxies::parse("127.0.0.1").unwrap()),
        );
        for _ in 0..5 {
            assert_eq!(
                proxied_login(&app, "203.0.113.7", "wrong password").await,
                StatusCode::UNAUTHORIZED
            );
        }
        let status = proxied_login(&app, "198.51.100.9", PASSWORD).await;
        println!("AUDIT quiet client behind same trusted proxy: status={status}");
        assert_eq!(
            status,
            StatusCode::OK,
            "a client with a different trusted forwarded address must retain its login budget"
        );
    }
    #[tokio::test]
    async fn durable_memory_queries_preserve_history_and_refuse_unavailable_sql() {
        let (f, _state, app, owner) = setup().await;
        let (status, node) = call(
            &app,
            "POST",
            "/api/v1/orbz/crystallize",
            memory(json!("public"), "L1"),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let at = node["created_at_millis"].as_i64().unwrap();
        sqlx::query("UPDATE archava_orbz.nodes SET valid_to=to_timestamp($2::double precision/1000.0) WHERE id=$1")
            .bind(node["orb_id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).bind(at+10).execute(&f.admin_pool).await.unwrap();
        let query =
            |point| json!({"tenant_id":"acme","tier":"L0Abstract","point_in_time_millis":point});
        let (status, then) =
            call(&app, "POST", "/api/v1/orbz/query", query(at), Some(&owner)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(then.as_array().unwrap().len(), 1);
        let (status, later) = call(
            &app,
            "POST",
            "/api/v1/orbz/query",
            query(at + 10),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(later.as_array().unwrap().is_empty());
        let cold = router(state(&f, f.runtime_pool.clone()).await);
        assert_eq!(
            call(&cold, "POST", "/api/v1/orbz/query", query(at), Some(&owner))
                .await
                .1,
            then,
            "restart preserves original validity timestamps"
        );
        sqlx::raw_sql("REVOKE SELECT ON archava_orbz.nodes FROM archava_runtime")
            .execute(&f.admin_pool)
            .await
            .unwrap();
        assert_eq!(
            call(&app, "POST", "/api/v1/orbz/query", query(at), Some(&owner))
                .await
                .0,
            StatusCode::SERVICE_UNAVAILABLE,
            "a warm process must not hide unavailable durable state"
        );
    }
    #[tokio::test]
    async fn memory_membership_revocation_and_claimed_episode_roles_cannot_widen_authority() {
        let (f, _state, app, owner) = setup().await;
        sqlx::query("UPDATE archava_commerce.memberships SET role='editor',scope='all' WHERE principal_id='assigned'").execute(&f.admin_pool).await.unwrap();
        let editor = login(&app, "assigned").await;
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/orbz/crystallize",
                memory(json!("public"), "L5"),
                Some(&editor)
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/orbz/crystallize",
                memory(json!({"department":"finance"}), "L1"),
                Some(&editor)
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let episode = json!({"tenant_id":"acme","session_id":"audit-session","user_id":"alice","user_role":"owner","channel":"web","user_utterance":"test","agent_response":"reported client text","tools_called":[]});
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/orbz/episodes",
                episode,
                Some(&editor)
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let identity: (String, String) = sqlx::query_as(
            "SELECT user_id,user_role FROM archava_orbz.episodes WHERE session_id='audit-session'",
        )
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
        assert_eq!(identity, ("assigned".into(), "editor".into()));
        sqlx::query(
            "UPDATE archava_commerce.memberships SET active=false WHERE principal_id='alice'",
        )
        .execute(&f.admin_pool)
        .await
        .unwrap();
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/orbz/query",
                json!({"tenant_id":"acme","tier":"L0Abstract"}),
                Some(&owner)
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/orbz/crystallize",
                memory(json!("public"), "L1"),
                Some(&owner)
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
}
