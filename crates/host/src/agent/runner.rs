use super::*;
use axum::http::{HeaderMap, Method};
use std::time::Duration;
/// Task-owned worker. Dropping it signals shutdown; explicit shutdown waits for cleanup.
pub struct AgentWorker {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl AgentWorker {
    pub fn start(state: AppState) -> Self {
        let (send, mut stop) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {_=&mut stop=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{}}
                match store::pending(state.pool()).await {
                    Ok(rows) => {
                        for (tenant, id) in rows {
                            if let Ok(tenant) = TenantId::new(tenant) {
                                let _ = drive(&state, &tenant, id).await;
                            }
                        }
                    }
                    Err(error) => tracing::warn!(error=%error,"agent work scan unavailable"),
                }
            }
        });
        Self {
            stop: Some(send),
            task: Some(task),
        }
    }
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for AgentWorker {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
async fn serial(
    state: &AppState,
    goal: Uuid,
    wait: bool,
) -> Result<Option<sqlx::Transaction<'static, sqlx::Postgres>>, Response> {
    let mut tx = state.pool().begin().await.map_err(unavailable)?;
    if wait {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("agent-goal:{goal}"))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
    } else {
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!("agent-goal:{goal}"))
                .fetch_one(&mut *tx)
                .await
                .map_err(unavailable)?;
        if !locked {
            return Ok(None);
        }
    }
    Ok(Some(tx))
}
async fn raw(state: &AppState, tenant: &TenantId, id: Uuid) -> Result<Value, Response> {
    let mut tx = workflows::begin(state, tenant).await?;
    let goal = store::worker_goal(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        id,
    )
    .await
    .map_err(unavailable)?
    .ok_or_else(|| refused(StatusCode::NOT_FOUND, "unknown_agent_goal"))?;
    tx.commit().await.map_err(unavailable)?;
    Ok(goal)
}
async fn update(
    state: &AppState,
    tenant: &TenantId,
    goal: &Value,
    status: &str,
    mission: Option<&Value>,
    failure: Option<&str>,
) -> Result<(), Response> {
    let actor = goal["principal_id"]
        .as_str()
        .ok_or_else(|| unavailable("missing goal actor"))?;
    let id = id(goal, "id")?;
    let mission_id = mission.map(|m| super::id(m, "id")).transpose()?;
    let mut tx = begin(state, tenant, actor).await?;
    store::state(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        id,
        status,
        mission_id,
        mission,
        failure,
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(())
}
fn status_path(tenant: &TenantId, mission: Uuid, step: &Value) -> Result<String, Response> {
    let target = step["tool_target"]
        .as_str()
        .ok_or_else(|| unavailable("missing mission tool"))?;
    let (module, _) = crate::business::tool(target)
        .ok_or_else(|| refused(StatusCode::BAD_REQUEST, "unknown_agent_tool"))?;
    let index = step["step_index"]
        .as_u64()
        .ok_or_else(|| unavailable("missing step index"))?;
    let key = crate::business::stable_key("cockpit", &format!("{mission}:{index}"));
    let key_name = archava_domain::operator::module(module)
        .and_then(|m| m["statusKey"].as_str())
        .unwrap_or("requestId");
    Ok(format!(
        "/{}/commands/status?tenant={tenant}&{key_name}={key}",
        if module == "crm" {
            "crm".to_string()
        } else {
            format!("erp/{module}")
        }
    ))
}
fn has_receipt(body: &Value) -> bool {
    body["receipt"].is_object()
        || ((body["requestId"].is_string() || body["commandId"].is_string())
            && body["revision"].as_i64().is_some())
}
async fn guards_match(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    goal: &Value,
    step: &Value,
) -> Result<bool, Response> {
    let mut context = read::context(
        state,
        &HeaderMap::new(),
        tenant,
        actor,
        &modules(&goal["modules"]),
        goal["context"]["companyId"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok()),
    )
    .await?;
    super::lookups::refresh_guards(state, tenant, &mut context, goal, step).await?;
    for guard in step["guards"].as_array().into_iter().flatten() {
        let row = read::rows(
            &context,
            guard["module"].as_str().unwrap_or_default(),
            guard["collection"].as_str().unwrap_or_default(),
        )
        .into_iter()
        .find(|r| r["id"] == guard["id"]);
        let Some(row) = row else { return Ok(false) };
        if guard["fields"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(k, v)| row[k] != *v)
        {
            return Ok(false);
        }
    }
    if step["observation"].is_object() {
        let observation = &step["observation"];
        let row = read::rows(&context, "commerce", "products")
            .into_iter()
            .find(|r| r["id"] == observation["productId"]);
        if row.is_none_or(|r| {
            r["stockRevision"] != observation["stockRevision"]
                || r["quantity"] != observation["quantity"]
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) async fn drive(
    state: &AppState,
    tenant: &TenantId,
    goal_id: Uuid,
) -> Result<(), Response> {
    let Some(serial) = serial(state, goal_id, false).await? else {
        return Ok(());
    };
    let goal = raw(state, tenant, goal_id).await?;
    if !matches!(goal["state"].as_str(), Some("approved" | "running")) {
        return Ok(());
    }
    let actor = goal["principal_id"]
        .as_str()
        .ok_or_else(|| unavailable("invalid agent actor"))?
        .to_string();
    let session_hash = goal["session_hash"]
        .as_str()
        .ok_or_else(|| unavailable("missing approved session"))?
        .to_string();
    let live = state
        .identity()
        .active_session_hash(&session_hash)
        .await
        .map_err(unavailable)?;
    if live.is_none_or(|s| s.principal_id != actor) {
        update(state,tenant,&goal,"awaiting_session",None,Some("The authorizing session is no longer active. Sign in and resume the reviewed work.")).await?;
        return Ok(());
    }
    let mut tx = begin(state, tenant, &actor).await?;
    let fp = store::access_fingerprint(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        &actor,
        &modules(&goal["modules"]),
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    if goal["context"]["accessHash"] != fp {
        update(
            state,
            tenant,
            &goal,
            "holding",
            None,
            Some("Access or module configuration changed. Review current access before resuming."),
        )
        .await?;
        return Ok(());
    }
    let execution = delegation::Execution {
        tenant: tenant.clone(),
        goal: goal_id,
        actor: actor.clone(),
        session_hash,
    };
    let result = delegation::scope(execution, execute(state, tenant, &goal)).await;
    // Flush the routing lock only after the checkpoint/state update completes.
    serial.commit().await.map_err(unavailable)?;
    result
}
async fn execute(state: &AppState, tenant: &TenantId, goal: &Value) -> Result<(), Response> {
    let headers = HeaderMap::new();
    let actor = goal["principal_id"]
        .as_str()
        .ok_or_else(|| unavailable("invalid actor"))?;
    let mut mission = if let Some(id) = goal["mission_id"].as_str() {
        let (status, Json(body)) = read::invoke(
            state,
            &headers,
            Method::GET,
            &format!("/api/v1/cockpit/missions/{id}?tenant={tenant}"),
            None,
        )
        .await?;
        if !status.is_success() {
            return Err((status, Json(body)));
        }
        body
    } else {
        let steps=goal["plan"]["steps"].as_array().ok_or_else(||unavailable("invalid approved plan"))?.iter().enumerate().map(|(index,s)|{
   let mut payload=s["input"].clone();payload["__agent_bindings_version"]=json!(1);
   json!({"step_index":index,"step_name":s["title"],"tool_name":s["tool"],"payload_input":payload,"depends_on":s["dependsOn"]})
  }).collect::<Vec<_>>();
        let body = json!({"tenant_id":tenant.as_str(),"mission_code":format!("agent-{}",goal["id"].as_str().unwrap_or_default()),"title":goal["objective"].as_str().unwrap_or("Business goal").chars().take(180).collect::<String>(),"description":"Reviewed agent goal. Current domain authority and recorded human approval remain required.","auto_launch":true,"steps":steps});
        let (status, Json(body)) = read::invoke(
            state,
            &headers,
            Method::POST,
            "/api/v1/cockpit/missions",
            Some(body),
        )
        .await?;
        if !status.is_success() {
            update(state,tenant,goal,"holding",None,Some("Mission creation is unresolved; retry preserves the original goal/creation key.")).await?;
            return Err((status, Json(body)));
        }
        body
    };
    update(state, tenant, goal, "running", Some(&mission), None).await?;
    let mission_id = id(&mission, "id")?;
    for _ in 0..24 {
        if mission["status"] == "touchdown" {
            update(state, tenant, goal, "completed", Some(&mission), None).await?;
            return Ok(());
        }
        if matches!(
            mission["status"].as_str(),
            Some("aborted" | "holding_pattern")
        ) {
            update(
                state,
                tenant,
                goal,
                "holding",
                Some(&mission),
                Some("The mission is stopped or waiting for supervisor review."),
            )
            .await?;
            return Ok(());
        }
        let steps = mission["steps"]
            .as_array()
            .ok_or_else(|| unavailable("invalid mission steps"))?;
        let ready = steps
            .iter()
            .find(|s| {
                s["status"] != "passed"
                    && s["depends_on_steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .all(|d| {
                            steps
                                .iter()
                                .any(|p| p["step_index"] == *d && p["status"] == "passed")
                        })
            })
            .cloned();
        let Some(step) = ready else {
            update(
                state,
                tenant,
                goal,
                "holding",
                Some(&mission),
                Some("No eligible step is available. Inspect dependencies and failures."),
            )
            .await?;
            return Ok(());
        };
        let index = step["step_index"]
            .as_u64()
            .ok_or_else(|| unavailable("invalid step"))?;
        if step["status"] == "requires_approval" {
            let (status, Json(body)) = read::invoke(
                state,
                &headers,
                Method::POST,
                &format!(
                    "/api/v1/cockpit/missions/{mission_id}/steps/{index}/approve?tenant={tenant}"
                ),
                Some(json!({})),
            )
            .await?;
            if !status.is_success() {
                update(state,tenant,goal,"holding",Some(&mission),Some("A current qualified human approval is required in Missions before this step can continue.")).await?;
                return Ok(());
            }
            mission = body;
            continue;
        }
        let (status, Json(existing)) = read::invoke(
            state,
            &headers,
            Method::GET,
            &status_path(tenant, mission_id, &step)?,
            None,
        )
        .await?;
        if !status.is_success() {
            update(state,tenant,goal,"holding",Some(&mission),Some("The original business outcome cannot be verified under current access. Keep its original reference.")).await?;
            return Ok(());
        }
        let plan_step = &goal["plan"]["steps"][index as usize];
        if !has_receipt(&existing) && !guards_match(state, tenant, actor, goal, plan_step).await? {
            update(state,tenant,goal,"needs_review",Some(&mission),Some("Business facts changed after review. Ask Ava to refresh the plan; a revised plan needs new approval.")).await?;
            return Ok(());
        }
        let (status, Json(body)) = read::invoke(
            state,
            &headers,
            Method::POST,
            &format!("/api/v1/cockpit/missions/{mission_id}/run-next?tenant={tenant}"),
            Some(json!({"stepIndex":index})),
        )
        .await?;
        if !status.is_success() {
            let next = if status.is_server_error() {
                "running"
            } else if status == StatusCode::CONFLICT {
                "needs_review"
            } else {
                "holding"
            };
            update(state,tenant,goal,next,Some(&mission),Some(if status.is_server_error(){"Outcome unconfirmed. Recovery will use the original mission/step key; do not create a replacement action."}else{"The current domain rules refused this step. Review the recorded partial outcome before changing the plan."})).await?;
            return Ok(());
        }
        mission = body;
        update(state, tenant, goal, "running", Some(&mission), None).await?;
    }
    if mission["status"] == "touchdown" {
        update(state, tenant, goal, "completed", Some(&mission), None).await?;
    }
    Ok(())
}
pub(super) async fn control(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    goal_id: Uuid,
    action: &str,
) -> Result<(), Response> {
    let Some(lock) = serial(state, goal_id, true).await? else {
        return Err(unavailable("agent lock unavailable"));
    };
    let goal = raw(state, tenant, goal_id).await?;
    if goal["principal_id"] != actor {
        return Err(refused(StatusCode::FORBIDDEN, "agent_goal_scope_refused"));
    }
    if goal["state"] == "completed" {
        return Ok(());
    }
    let mut stopped_mission = None;
    if let Some(mission_id) = goal["mission_id"].as_str() {
        let (status, Json(mission)) = read::invoke(
            state,
            headers,
            Method::GET,
            &format!("/api/v1/cockpit/missions/{mission_id}?tenant={tenant}"),
            None,
        )
        .await?;
        if !status.is_success() {
            return Err((status, Json(mission)));
        }
        if action == "resume" {
            if mission["status"] == "holding_pattern" {
                let (status, Json(body)) = read::invoke(
                    state,
                    headers,
                    Method::POST,
                    &format!("/api/v1/cockpit/missions/{mission_id}/resume?tenant={tenant}"),
                    Some(json!({"signal_key":format!("agent-{goal_id}")})),
                )
                .await?;
                if !status.is_success() {
                    return Err((status, Json(body)));
                }
            }
        } else {
            // A pending checkpoint may hide a committed effect. Do not abandon its key.
            for step in mission["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["status"] != "passed")
            {
                let (status, Json(receipt)) = read::invoke(
                    state,
                    headers,
                    Method::GET,
                    &status_path(tenant, id(&mission, "id")?, step)?,
                    None,
                )
                .await?;
                if !status.is_success() || has_receipt(&receipt) {
                    return Err(refused(
                        StatusCode::CONFLICT,
                        "agent_original_outcome_needs_recovery",
                    ));
                }
            }
            let endpoint = if action == "pause" { "hold" } else { "abort" };
            let body = if action == "pause" {
                json!({"reason":"Customer paused this agent goal","signal_key":format!("agent-{goal_id}")})
            } else {
                json!({"reason":"Customer cancelled or replaced this reviewed goal"})
            };
            let (status, Json(body)) = read::invoke(
                state,
                headers,
                Method::POST,
                &format!("/api/v1/cockpit/missions/{mission_id}/{endpoint}?tenant={tenant}"),
                Some(body),
            )
            .await?;
            if !status.is_success() {
                return Err((status, Json(body)));
            }
            stopped_mission = Some(body);
        }
    }
    if action == "resume" {
        let session = authenticate(state, headers, true)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?;
        let mut tx = begin(state, tenant, actor).await?;
        let _ = store::approve(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            actor,
            goal_id,
            goal["plan_hash"].as_str().unwrap_or_default(),
            &session.session_hash,
        )
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
    } else {
        update(
            state,
            tenant,
            &goal,
            if action == "pause" {
                "holding"
            } else {
                "cancelled"
            },
            stopped_mission.as_ref(),
            None,
        )
        .await?;
    }
    lock.commit().await.map_err(unavailable)?;
    Ok(())
}
pub(super) async fn stop_for_replan(
    state: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    actor: &str,
    id: Uuid,
) -> Result<(), Response> {
    control(state, headers, tenant, actor, id, "cancel").await
}
