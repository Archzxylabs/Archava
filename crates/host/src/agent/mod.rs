//! Ava's goal-driven conversation. Plans request work; existing domain handlers authorize it.
mod aggregate;
pub(crate) mod bindings;
pub(crate) mod delegation;
mod disclosure;
mod lookups;
mod planner;
mod read;
mod runner;
mod schema;
mod units;
use crate::{
    business::{refused, Response},
    router::{authenticate, AppState},
    workflows,
};
use archava_adapters::{agent as store, rls::TenantTransaction};
use archava_core::tenant::TenantId;
use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    Json,
};
pub use runner::AgentWorker;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    pub planner: Option<archava_providers::agent::GeminiPlanner>,
}
impl AgentConfig {
    pub fn from_env() -> Result<Self, &'static str> {
        let model = std::env::var("GEMINI_AGENT_MODEL").unwrap_or_default();
        if model.trim().is_empty() {
            return Ok(Self::default());
        }
        let key = std::env::var("GEMINI_AGENT_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .unwrap_or_default();
        let endpoint = std::env::var("GEMINI_AGENT_ENDPOINT")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".into());
        let mut planner = archava_providers::agent::GeminiPlanner::new(&endpoint, &model, &key)
            .map_err(|_| "invalid agent planner configuration")?;
        if let Ok(level) = std::env::var("GEMINI_AGENT_THINKING_LEVEL") {
            if !level.is_empty() {
                planner = planner.with_thinking_level(&level).map_err(|_| {
                    "invalid agent thinking level; choose a supported Gemini 3 level"
                })?;
            }
        }
        Ok(Self {
            planner: Some(planner),
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnInput {
    pub tenant: TenantId,
    pub request_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub utterance: String,
    pub goal_id: Option<Uuid>,
    pub action: Option<ActionInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_id: Option<Uuid>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionInput {
    pub tool: String,
    pub values: Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadInput {
    pub tenant: TenantId,
    pub thread_id: Option<Uuid>,
    pub goal_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlInput {
    pub tenant: TenantId,
    pub goal_id: Uuid,
    pub plan_hash: String,
    pub confirmed: bool,
    pub action: Option<String>,
}
fn unavailable(e: impl std::fmt::Display) -> Response {
    workflows::unavailable(e)
}
async fn begin<'a>(
    state: &'a AppState,
    tenant: &TenantId,
    actor: &str,
) -> Result<TenantTransaction<'a>, Response> {
    let mut tx = workflows::begin(state, tenant).await?;
    let ledger = *state.ledger();
    let member = crate::authority::resolve_membership(
        tx.checked_executor().await.map_err(unavailable)?,
        ledger.namespace,
        ledger.ledger,
        tenant,
        actor,
    )
    .await
    .map_err(unavailable)?;
    if member.is_none_or(|m| m.kind != crate::authority::MembershipKind::Human) {
        return Err(refused(StatusCode::FORBIDDEN, "tenant_membership_required"));
    }
    Ok(tx)
}
fn modules(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
fn id(value: &Value, key: &str) -> Result<Uuid, Response> {
    value[key]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| unavailable("invalid stored agent reference"))
}
fn public_goal(mut goal: Value) -> Value {
    // The catalogued reason of a refused step travels in the stored mission
    // snapshot. It describes the current failure only: once the failure is
    // cleared (a new approval) it is no longer the goal's reason.
    let refusal = goal["outcome"]
        .as_object_mut()
        .and_then(|outcome| outcome.remove(runner::REFUSAL_KEY))
        .filter(|_| goal["failure"].is_string());
    if let Some(g) = goal.as_object_mut() {
        g.remove("session_hash");
        g.remove("principal_id");
        if let Some(refusal) = refusal {
            g.insert("failureDetail".into(), refusal);
        }
    }
    goal
}
async fn visible_goal(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    goal: Value,
) -> Result<Value, Response> {
    let mut tx = begin(state, tenant, actor).await?;
    let fp = store::access_fingerprint(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        &modules(&goal["modules"]),
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    if goal["context"]["accessHash"] != fp {
        return Err(refused(
            StatusCode::FORBIDDEN,
            "agent_context_access_changed",
        ));
    }
    Ok(public_goal(goal))
}
pub(crate) async fn turn(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<TurnInput>, JsonRejection>,
) -> Response {
    let input = match body {
        Ok(Json(v)) => v,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_agent_turn"),
    };
    turn_impl(&state, &headers, input)
        .await
        .unwrap_or_else(|e| e)
}
async fn turn_impl(
    state: &AppState,
    headers: &HeaderMap,
    input: TurnInput,
) -> Result<Response, Response> {
    if input.utterance.trim().is_empty() || input.utterance.chars().count() > 1000 {
        return Err(refused(StatusCode::BAD_REQUEST, "invalid_agent_utterance"));
    }
    let session = authenticate(state, headers, true)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let input_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&input).map_err(unavailable)?)
    );
    let actor = &session.principal_id;
    let tenant = &input.tenant;
    // The serialization lock holds no grant/mission rows across nested handlers or provider calls.
    let mut serial = state.pool().begin().await.map_err(unavailable)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "agent-turn:{}:{actor}:{}",
            tenant.as_str(),
            input.request_id
        ))
        .execute(&mut *serial)
        .await
        .map_err(unavailable)?;
    let mut tx = begin(state, tenant, actor).await?;
    if let Some(old) = store::turn_by_request(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        input.request_id,
    )
    .await
    .map_err(unavailable)?
    {
        if old["utterance"] != input.utterance || old["response"]["inputHash"] != input_hash {
            return Err(refused(StatusCode::CONFLICT, "agent_request_reused"));
        }
        tx.commit().await.map_err(unavailable)?;
        serial.commit().await.map_err(unavailable)?;
        let mut response = old["response"].clone();
        if let Some(goal) = response["goalId"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            response["goal"] = load_visible(state, tenant, actor, goal).await?;
        }
        return Ok((StatusCode::OK, Json(response)));
    }
    let thread = input.thread_id.unwrap_or_else(|| {
        crate::business::stable_key(
            "agent-thread",
            &format!("{}:{actor}:{}", tenant.as_str(), input.request_id),
        )
    });
    if !store::thread(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        thread,
    )
    .await
    .map_err(unavailable)?
    {
        return Err(refused(StatusCode::NOT_FOUND, "unknown_agent_thread"));
    }
    let mut history = store::turns(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        thread,
    )
    .await
    .map_err(unavailable)?;
    let preferences = store::preferences(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    for turn in &mut history {
        if let Some(id) = turn["response"]["goalId"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            if load_visible(state, tenant, actor, id).await.is_err() {
                turn["response"] = json!({"text":"Earlier business context is no longer available under current access."});
            }
        }
    }
    let text = input.utterance.to_lowercase();
    let mut reply = if text.starts_with("ingat ")
        || text.starts_with("ingat untuk")
        || text.starts_with("remember ")
        || text.starts_with("remember:")
    {
        let mut tx = begin(state, tenant, actor).await?;
        store::remember(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            actor,
            thread,
            input.request_id,
            &input.utterance,
        )
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        json!({"text":"I saved this as your preference for this business. It does not change permissions or approval policy.","source":"agent-memory","data":{"kind":"memory_saved","content":input.utterance},"modules":[]})
    } else if planner::asks_memory(&text) {
        json!({"text":if preferences.is_empty(){"You have not saved a preference for this business yet.".into()}else{preferences.iter().filter_map(|p|p["content"].as_str()).collect::<Vec<_>>().join("\n")},"source":"agent-memory","data":{"kind":"memory_recall","items":preferences},"modules":[]})
    } else if planner::asks_report(&text) || planner::asks_control(&text) {
        let goal = resolve_goal(
            state,
            tenant,
            actor,
            thread,
            input.goal_id,
            &input.utterance,
        )
        .await?;
        if goal.is_null() {
            json!({"text":"There is no recorded goal in this conversation yet. Tell me the business outcome you want, and I will prepare a plan for review.","source":"business","modules":[],"data":{"kind":"agent_report","verifiedCompletedSteps":0}})
        } else if planner::asks_control(&text) {
            control_from_chat(goal, &text)
        } else {
            report_reply(&load_visible(state, tenant, actor, id(&goal, "id")?).await?)
        }
    } else {
        let parent = if let Some(goal) = input.goal_id {
            let g = load_visible(state, tenant, actor, goal).await?;
            if id(&g, "thread_id")? != thread {
                return Err(refused(StatusCode::CONFLICT, "agent_thread_mismatch"));
            }
            Some(g)
        } else {
            None
        };
        let mut proposal = planner::propose(
            state,
            headers,
            tenant,
            actor,
            &input.utterance,
            &history,
            &preferences,
            parent.as_ref(),
            input.action.as_ref(),
            input.company_id,
            input.request_id,
        )
        .await?;
        proposal.context["inputHash"] = json!(input_hash);
        proposal.context["submittedUtterance"] = json!(input.utterance);
        let goal_id = crate::business::stable_key(
            "agent-goal",
            &format!("{}:{actor}:{}", tenant.as_str(), input.request_id),
        );
        let hash=format!("{:x}",Sha256::digest(json!({"tenant":tenant,"actor":actor,"goal":goal_id,"objective":input.utterance,"plan":proposal.plan,"context":proposal.context}).to_string().as_bytes()));
        let state_name = if proposal.plan["steps"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
        {
            "planned"
        } else if proposal.plan["questions"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
        {
            "clarifying"
        } else {
            "completed"
        };
        if let Some(parent) = &parent {
            runner::stop_for_replan(state, headers, tenant, actor, id(parent, "id")?).await?;
        }
        let mut tx = begin(state, tenant, actor).await?;
        let module_json = json!(proposal.modules);
        store::insert_goal(
            tx.checked_executor().await.map_err(unavailable)?,
            store::NewGoal {
                tenant: tenant.as_str(),
                actor,
                id: goal_id,
                thread,
                request: input.request_id,
                parent: parent.as_ref().map(|g| id(g, "id")).transpose()?,
                objective: &input.utterance,
                state: state_name,
                modules: &module_json,
                context: &proposal.context,
                plan: &proposal.plan,
                hash: &hash,
            },
        )
        .await
        .map_err(unavailable)?;
        let goal = store::goal(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            actor,
            goal_id,
        )
        .await
        .map_err(unavailable)?
        .ok_or_else(|| unavailable("missing newly planned goal"))?;
        let reply = json!({"text":planner::proposal_text(&proposal.plan,&proposal.context,state_name),"source":"business","planner":proposal.planner,"modules":module_json,"accessHash":proposal.context["accessHash"],"goalId":goal_id,"goal":visible_goal(state,tenant,actor,goal).await?,"inputHash":input_hash,"requestId":input.request_id,"threadId":thread,"data":{"kind":if state_name=="planned"{"agent_plan"}else if state_name=="clarifying"{"agent_clarification"}else{"agent_report"}}});
        // Plan and recoverable turn commit together: no orphaned plan after a lost response.
        store::record_turn(
            tx.checked_executor().await.map_err(unavailable)?,
            tenant.as_str(),
            actor,
            thread,
            input.request_id,
            &input.utterance,
            &reply,
        )
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        serial.commit().await.map_err(unavailable)?;
        return Ok((StatusCode::OK, Json(reply)));
    };
    reply["inputHash"] = json!(input_hash);
    reply["requestId"] = json!(input.request_id);
    reply["threadId"] = json!(thread);
    let mut tx = begin(state, tenant, actor).await?;
    store::record_turn(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        thread,
        input.request_id,
        &input.utterance,
        &reply,
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    serial.commit().await.map_err(unavailable)?;
    Ok((StatusCode::OK, Json(reply)))
}
async fn load_visible(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    goal: Uuid,
) -> Result<Value, Response> {
    let mut tx = begin(state, tenant, actor).await?;
    let goal = store::goal(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        goal,
    )
    .await
    .map_err(unavailable)?
    .ok_or_else(|| refused(StatusCode::NOT_FOUND, "unknown_agent_goal"))?;
    tx.commit().await.map_err(unavailable)?;
    visible_goal(state, tenant, actor, goal).await
}
async fn resolve_goal(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    thread: Uuid,
    selected: Option<Uuid>,
    utterance: &str,
) -> Result<Value, Response> {
    if let Some(id) = selected.or_else(|| {
        utterance.split_whitespace().find_map(|s| {
            Uuid::parse_str(s.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != '-')).ok()
        })
    }) {
        return load_visible(state, tenant, actor, id).await;
    }
    let mut tx = begin(state, tenant, actor).await?;
    let goals = store::goals(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        actor,
        thread,
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    let Some(goal) = goals.into_iter().next() else {
        return Ok(Value::Null);
    };
    visible_goal(state, tenant, actor, goal).await
}
fn report_reply(goal: &Value) -> Value {
    let mission = &goal["outcome"];
    let completed = mission["steps"]
        .as_array()
        .map(|s| s.iter().filter(|s| s["status"] == "passed").count())
        .unwrap_or(0);
    json!({"text":format!("Goal status: {}. {} business steps have verified completion records. {}",goal["state"].as_str().unwrap_or("unknown"),completed,goal["failure"].as_str().unwrap_or("Review the recorded plan, source facts and receipts below.")),"source":"business","modules":goal["modules"],"goalId":goal["id"],"goal":goal,"data":{"kind":"agent_report","verifiedCompletedSteps":completed,"refusal":goal["failureDetail"]}})
}
fn control_from_chat(goal: Value, text: &str) -> Value {
    let action = if text.contains("cancel") || text.contains("batalkan") {
        "cancel"
    } else if text.contains("pause")
        || text.contains("tunda")
        || text.contains("hentikan sementara")
    {
        "pause"
    } else {
        "resume"
    };
    // Language may quote or negate a command. Only the explicit, hash-bound
    // confirmation endpoint may mutate a goal or authorize continuation.
    let mut reply = report_reply(&goal);
    reply["text"] = json!("Review the current goal below. Use its Pause, Resume or Cancel button to confirm the action. Conversation text alone does not change or authorize this work.");
    reply["data"]["kind"] = json!("agent_control_review");
    reply["data"]["requestedAction"] = json!(action);
    reply
}
pub(crate) async fn inspect(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ReadInput>, QueryRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        q: ReadInput,
    ) -> Result<Response, Response> {
        let s = authenticate(state, headers, false)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?;
        if let Some(goal) = q.goal_id {
            return Ok((
                StatusCode::OK,
                Json(json!({"goal":load_visible(state,&q.tenant,&s.principal_id,goal).await?})),
            ));
        }
        let mut tx = begin(state, &q.tenant, &s.principal_id).await?;
        if let Some(request) = q.request_id {
            // A missing turn is definitive only after its original planner releases this lock.
            let mut serial = state.pool().begin().await.map_err(unavailable)?;
            let locked: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
                    .bind(format!(
                        "agent-turn:{}:{}:{request}",
                        q.tenant.as_str(),
                        s.principal_id
                    ))
                    .fetch_one(&mut *serial)
                    .await
                    .map_err(unavailable)?;
            if !locked {
                return Ok((
                    StatusCode::OK,
                    Json(json!({"status":"pending","requestId":request})),
                ));
            }
            let turn = store::turn_by_request(
                tx.checked_executor().await.map_err(unavailable)?,
                q.tenant.as_str(),
                &s.principal_id,
                request,
            )
            .await
            .map_err(unavailable)?;
            tx.commit().await.map_err(unavailable)?;
            serial.commit().await.map_err(unavailable)?;
            if let Some(ref turn) = turn {
                if let Some(id) = turn["response"]["goalId"]
                    .as_str()
                    .and_then(|v| Uuid::parse_str(v).ok())
                {
                    load_visible(state, &q.tenant, &s.principal_id, id).await?;
                }
            }
            return Ok((
                StatusCode::OK,
                Json(
                    json!({"status":if turn.is_some(){"committed"}else{"not_committed"},"requestId":request,"turn":turn}),
                ),
            ));
        }
        let threads = store::threads(
            tx.checked_executor().await.map_err(unavailable)?,
            q.tenant.as_str(),
            &s.principal_id,
        )
        .await
        .map_err(unavailable)?;
        let selected = q.thread_id.or_else(|| {
            threads
                .first()
                .and_then(|t| t["id"].as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
        });
        let mut turns = if let Some(thread) = selected {
            store::turns(
                tx.checked_executor().await.map_err(unavailable)?,
                q.tenant.as_str(),
                &s.principal_id,
                thread,
            )
            .await
            .map_err(unavailable)?
        } else {
            vec![]
        };
        let mut goals = if let Some(thread) = selected {
            store::goals(
                tx.checked_executor().await.map_err(unavailable)?,
                q.tenant.as_str(),
                &s.principal_id,
                thread,
            )
            .await
            .map_err(unavailable)?
        } else {
            vec![]
        };
        tx.commit().await.map_err(unavailable)?;
        for turn in &mut turns {
            if let Some(id) = turn["response"]["goalId"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                if load_visible(state, &q.tenant, &s.principal_id, id)
                    .await
                    .is_err()
                {
                    turn["response"] = json!({"text":"Earlier business context is unavailable under your current access.","source":"none","data":{"kind":"access_changed"}});
                }
            }
        }
        for goal in &mut goals {
            if visible_goal(state, &q.tenant, &s.principal_id, goal.clone())
                .await
                .is_err()
            {
                *goal = json!({"id":goal["id"],"state":"access_changed"});
            }
        }
        Ok((
            StatusCode::OK,
            Json(json!({"threads":threads,"threadId":selected,"turns":turns,"goals":goals})),
        ))
    }
    let q = match query {
        Ok(Query(q)) => q,
        Err(rejection) => {
            let (status, Json(body)) = crate::router::query_rejection(rejection);
            return (status, Json(json!(body)));
        }
    };
    run(&state, &headers, q).await.unwrap_or_else(|e| e)
}
pub(crate) async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ControlInput>, JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        q: ControlInput,
    ) -> Result<Response, Response> {
        if !q.confirmed {
            return Err(refused(StatusCode::BAD_REQUEST, "confirmation_required"));
        }
        let s = authenticate(state, headers, true)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?;
        let goal = load_visible(state, &q.tenant, &s.principal_id, q.goal_id).await?;
        if goal["plan_hash"] != q.plan_hash {
            return Err(refused(StatusCode::CONFLICT, "agent_plan_changed"));
        }
        if matches!(
            goal["state"].as_str(),
            Some("completed" | "running" | "approved")
        ) {
            return Ok((StatusCode::OK, Json(json!({"goal":goal}))));
        }
        let mut tx = begin(state, &q.tenant, &s.principal_id).await?;
        let ledger = *state.ledger();
        let member = crate::authority::resolve_membership(
            tx.checked_executor().await.map_err(unavailable)?,
            ledger.namespace,
            ledger.ledger,
            &q.tenant,
            &s.principal_id,
        )
        .await
        .map_err(unavailable)?;
        if member.is_none_or(|m| m.capability == archava_core::roles::CapabilityTier::Assist) {
            return Err(refused(
                StatusCode::FORBIDDEN,
                "agent_approval_requires_write_capability",
            ));
        }
        let approved = store::approve(
            tx.checked_executor().await.map_err(unavailable)?,
            q.tenant.as_str(),
            &s.principal_id,
            q.goal_id,
            &q.plan_hash,
            &s.session_hash,
        )
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        if !approved {
            return Err(refused(StatusCode::CONFLICT, "agent_goal_not_approvable"));
        }
        // The durable worker owns continuation. This bounded drive also gives immediate feedback.
        let _ = runner::drive(state, &q.tenant, q.goal_id).await;
        Ok((
            StatusCode::OK,
            Json(json!({"goal":load_visible(state,&q.tenant,&s.principal_id,q.goal_id).await?})),
        ))
    }
    let q = match body {
        Ok(Json(q)) => q,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_agent_approval"),
    };
    run(&state, &headers, q).await.unwrap_or_else(|e| e)
}
pub(crate) async fn control(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ControlInput>, JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        q: ControlInput,
    ) -> Result<Response, Response> {
        let s = authenticate(state, headers, true)
            .await
            .map_err(|(s, b)| (s, Json(json!(b.0))))?;
        let goal = load_visible(state, &q.tenant, &s.principal_id, q.goal_id).await?;
        if !q.confirmed || goal["plan_hash"] != q.plan_hash {
            return Err(refused(StatusCode::CONFLICT, "agent_control_not_confirmed"));
        }
        if q.action
            .as_ref()
            .is_some_and(|a| !matches!(a.as_str(), "cancel" | "pause" | "resume"))
        {
            return Err(refused(StatusCode::BAD_REQUEST, "invalid_agent_control"));
        }
        runner::control(
            state,
            headers,
            &q.tenant,
            &s.principal_id,
            q.goal_id,
            q.action.as_deref().unwrap_or("cancel"),
        )
        .await?;
        Ok((
            StatusCode::OK,
            Json(json!({"goal":load_visible(state,&q.tenant,&s.principal_id,q.goal_id).await?})),
        ))
    }
    let q = match body {
        Ok(Json(q)) => q,
        Err(_) => return refused(StatusCode::BAD_REQUEST, "invalid_agent_control"),
    };
    run(&state, &headers, q).await.unwrap_or_else(|e| e)
}
