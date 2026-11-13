//! Tenant-scoped agent history and immutable reviewed goals. Authority stays in the host/domain.
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub async fn thread(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query("INSERT INTO archava_owner.agent_threads(tenant_id,id,principal_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(tenant).bind(id).bind(actor).execute(&mut *conn).await?;
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_owner.agent_threads WHERE tenant_id=$1 AND id=$2 AND principal_id=$3)").bind(tenant).bind(id).bind(actor).fetch_one(conn).await
}
pub async fn threads(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT to_jsonb(t) FROM archava_owner.agent_threads t WHERE tenant_id=$1 AND principal_id=$2 ORDER BY created_at DESC,id DESC LIMIT 30").bind(tenant).bind(actor).fetch_all(conn).await
}
pub async fn turns(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    thread: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT jsonb_build_object('id',id,'utterance',utterance,'response',response,'createdAt',created_at) FROM (SELECT * FROM archava_owner.agent_turns WHERE tenant_id=$1 AND principal_id=$2 AND thread_id=$3 ORDER BY created_at DESC,id DESC LIMIT 20) t ORDER BY created_at,id").bind(tenant).bind(actor).bind(thread).fetch_all(conn).await
}
pub async fn turn_by_request(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    request: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT jsonb_build_object('utterance',utterance,'response',response,'threadId',thread_id) FROM archava_owner.agent_turns WHERE tenant_id=$1 AND principal_id=$2 AND request_id=$3").bind(tenant).bind(actor).bind(request).fetch_optional(conn).await
}
pub async fn record_turn(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    thread: Uuid,
    request: Uuid,
    utterance: &str,
    response: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO archava_owner.agent_turns(tenant_id,id,thread_id,principal_id,request_id,utterance,response) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(Uuid::new_v4()).bind(thread).bind(actor).bind(request).bind(utterance).bind(response).execute(conn).await?;
    Ok(())
}
pub async fn preferences(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT jsonb_build_object('id',id,'content',content,'sourceRequestId',source_request_id,'createdAt',created_at) FROM archava_owner.agent_preferences WHERE tenant_id=$1 AND principal_id=$2 ORDER BY created_at DESC,id DESC LIMIT 20").bind(tenant).bind(actor).fetch_all(conn).await
}
pub async fn remember(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    thread: Uuid,
    request: Uuid,
    text: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO archava_owner.agent_preferences(tenant_id,id,principal_id,thread_id,source_request_id,content) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(tenant_id,principal_id,source_request_id) DO NOTHING").bind(tenant).bind(Uuid::new_v4()).bind(actor).bind(thread).bind(request).bind(text).execute(conn).await?;
    Ok(())
}
pub async fn goal(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT to_jsonb(g) FROM archava_owner.agent_goals g WHERE tenant_id=$1 AND id=$2 AND principal_id=$3").bind(tenant).bind(id).bind(actor).fetch_optional(conn).await
}
pub async fn goals(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    thread: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT to_jsonb(g)-'session_hash' FROM archava_owner.agent_goals g WHERE tenant_id=$1 AND principal_id=$2 AND thread_id=$3 ORDER BY created_at DESC,id DESC LIMIT 20").bind(tenant).bind(actor).bind(thread).fetch_all(conn).await
}
pub struct NewGoal<'a> {
    pub tenant: &'a str,
    pub actor: &'a str,
    pub id: Uuid,
    pub thread: Uuid,
    pub request: Uuid,
    pub parent: Option<Uuid>,
    pub objective: &'a str,
    pub state: &'a str,
    pub modules: &'a Value,
    pub context: &'a Value,
    pub plan: &'a Value,
    pub hash: &'a str,
}
pub async fn insert_goal(conn: &mut PgConnection, g: NewGoal<'_>) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO archava_owner.agent_goals(tenant_id,id,thread_id,principal_id,request_id,parent_id,objective,state,modules,context,plan,plan_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)").bind(g.tenant).bind(g.id).bind(g.thread).bind(g.actor).bind(g.request).bind(g.parent).bind(g.objective).bind(g.state).bind(g.modules).bind(g.context).bind(g.plan).bind(g.hash).execute(conn).await?;
    Ok(())
}
pub async fn approve(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    id: Uuid,
    hash: &str,
    session_hash: &str,
) -> Result<bool, sqlx::Error> {
    let n=sqlx::query("UPDATE archava_owner.agent_goals SET state='approved',approved_hash=plan_hash,session_hash=$5,updated_at=clock_timestamp(),failure=NULL WHERE tenant_id=$1 AND principal_id=$2 AND id=$3 AND plan_hash=$4 AND state IN ('planned','approved','awaiting_session','holding')").bind(tenant).bind(actor).bind(id).bind(hash).bind(session_hash).execute(&mut *conn).await?.rows_affected();
    if n > 0 {
        sqlx::query("INSERT INTO archava_owner.agent_work(tenant_id,goal_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(tenant).bind(id).execute(conn).await?;
    }
    Ok(n > 0)
}
#[allow(clippy::too_many_arguments)]
pub async fn state(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    id: Uuid,
    state: &str,
    mission: Option<Uuid>,
    outcome: Option<&Value>,
    failure: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE archava_owner.agent_goals SET state=$4,mission_id=COALESCE(mission_id,$5),outcome=COALESCE($6,outcome),failure=$7,updated_at=clock_timestamp() WHERE tenant_id=$1 AND principal_id=$2 AND id=$3").bind(tenant).bind(actor).bind(id).bind(state).bind(mission).bind(outcome).bind(failure).execute(&mut *conn).await?;
    if !matches!(state, "approved" | "running") {
        sqlx::query("DELETE FROM archava_owner.agent_work WHERE tenant_id=$1 AND goal_id=$2")
            .bind(tenant)
            .bind(id)
            .execute(conn)
            .await?;
    }
    Ok(())
}
/// Current membership snapshots, not caller role claims. Context readers still authorize actual reads.
pub async fn access_fingerprint(
    conn: &mut PgConnection,
    tenant: &str,
    actor: &str,
    modules: &[String],
) -> Result<String, sqlx::Error> {
    use sha2::{Digest, Sha256};
    let mut rows = Vec::new();
    let mut modules = modules.to_vec();
    // Trade reads also depend on current company-read authority.
    if modules.iter().any(|m| {
        matches!(
            m.as_str(),
            "commerce" | "purchasing" | "billing" | "accounting"
        )
    }) {
        modules.push("organizations".into());
    }
    modules.sort();
    modules.dedup();
    for id in modules {
        let Some(module) = archava_domain::operator::module(&id) else {
            continue;
        };
        let namespace = module["namespace"].as_str().unwrap_or_default();
        // Identifiers are solely the immutable, embedded module catalogue.
        let query=format!("SELECT to_jsonb(m) FROM {namespace}.memberships m WHERE tenant_id=$1 AND principal_id=$2");
        let value: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(query.as_str()))
            .bind(tenant)
            .bind(actor)
            .fetch_optional(&mut *conn)
            .await?;
        rows.push(json!({"module":id,"grant":value}));
    }
    let owner: Option<i64> = sqlx::query_scalar(
        "SELECT max(revision)::bigint FROM archava_owner.config_revisions WHERE tenant_id=$1",
    )
    .bind(tenant)
    .fetch_one(conn)
    .await?;
    Ok(format!(
        "{:x}",
        Sha256::digest(
            json!({"memberships":rows,"ownerRevision":owner})
                .to_string()
                .as_bytes()
        )
    ))
}
/// Only routing keys are globally discoverable by the trusted worker. Business rows remain tenant scoped.
pub async fn pending(pool: &sqlx::PgPool) -> Result<Vec<(String, Uuid)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('archava.agent_worker','on',true)")
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query(
        "SELECT tenant_id,goal_id FROM archava_owner.agent_work ORDER BY created_at LIMIT 16",
    )
    .fetch_all(&mut *tx)
    .await?;
    let values = rows
        .into_iter()
        .map(|r| (r.get("tenant_id"), r.get("goal_id")))
        .collect();
    tx.commit().await?;
    Ok(values)
}

pub async fn worker_goal(
    conn: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(g) FROM archava_owner.agent_goals g WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(conn)
    .await
}
