use archava_ops::install::{bootstrap_payment_service, resume_payment_creation};
use archava_pg_fixture::provision;
#[tokio::test]
async fn payment_service_provisioning_never_overwrites_membership_and_runtime_cannot_resume() {
    let f = provision().await;
    bootstrap_payment_service(&f.admin_pool, "acme", "gateway")
        .await
        .unwrap();
    let row:(String,Vec<String>,String)=sqlx::query_as("SELECT kind,permissions,scope FROM archava_billing.memberships WHERE tenant_id='acme' AND principal_id='gateway'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        row,
        (
            "service".into(),
            vec!["billing.read".into(), "billing.write".into()],
            "all".into()
        )
    );
    assert!(bootstrap_payment_service(&f.admin_pool, "acme", "gateway")
        .await
        .is_err());
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme','human','human',ARRAY['billing.read'],'assigned',true)").execute(&f.admin_pool).await.unwrap();
    assert!(bootstrap_payment_service(&f.admin_pool, "acme", "human")
        .await
        .is_err());
    assert!(
        bootstrap_payment_service(&f.runtime_pool, "acme", "forbidden")
            .await
            .is_err()
    );
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=true")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert!(resume_payment_creation(&f.runtime_pool).await.is_err());
    let held: bool = sqlx::query_scalar("SELECT held FROM archava_billing.gateway_runtime")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert!(held);
    resume_payment_creation(&f.admin_pool).await.unwrap();
    let held: bool = sqlx::query_scalar("SELECT held FROM archava_billing.gateway_runtime")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert!(!held);
}
