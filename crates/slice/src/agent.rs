//! Agent-first conversation and explicit reviewed business approval.
use crate::{
    alive::AliveSignal,
    pending::{self, PendingMarker},
    session,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use serde_json::{json, Value};
#[derive(Clone, Copy)]
pub(crate) struct AgentDraft(pub RwSignal<Option<String>>);
#[derive(Clone, Copy)]
struct Panel {
    input: RwSignal<String>,
    thread: RwSignal<Option<String>>,
    turns: RwSignal<Vec<Value>>,
    threads: RwSignal<Vec<Value>>,
    goals: RwSignal<Vec<Value>>,
    busy: RwSignal<bool>,
    notice: RwSignal<String>,
    pending: RwSignal<Option<String>>,
    selected: RwSignal<Option<String>>,
    sequence: RwSignal<u64>,
    company: RwSignal<Option<Value>>,
}
fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().into()
}
fn encode(v: &str) -> String {
    crate::contacts::query_encode(v)
}
fn error(v: &Value) -> String {
    v["message"]
        .as_str()
        .or_else(|| v["error"]["message"].as_str())
        .or_else(|| v["reason"].as_str())
        .unwrap_or(
            "This request could not be completed. Check the original status before retrying.",
        )
        .into()
}
impl Panel {
    fn refresh(self) {
        let Some(held) = session::held() else { return };
        self.sequence.update_if_alive(|n| *n += 1);
        let Some(ticket) = self.sequence.try_get_untracked() else {
            return;
        };
        let mut path = format!("/api/agent/state?tenant={}", encode(&held.tenant));
        if let Some(id) = self.thread.try_get_untracked().flatten() {
            path.push_str(&format!("&threadId={}", encode(&id)));
        }
        spawn_local(async move {
            let response = session::api_fetch("GET", &path, None).await;
            if self.sequence.try_get_untracked() != Some(ticket) {
                return;
            }
            match response {
                Ok(a) if a.status == 200 => {
                    self.threads
                        .set_if_alive(a.body["threads"].as_array().cloned().unwrap_or_default());
                    self.thread
                        .set_if_alive(a.body["threadId"].as_str().map(str::to_owned));
                    self.turns
                        .set_if_alive(a.body["turns"].as_array().cloned().unwrap_or_default());
                    self.goals
                        .set_if_alive(a.body["goals"].as_array().cloned().unwrap_or_default());
                }
                Ok(a) => self.notice.set_if_alive(error(&a.body)),
                Err(e) => self
                    .notice
                    .set_if_alive(format!("Conversation unavailable: {e}")),
            }
        });
    }
    fn send(self, text: String, action: Option<(String, Value)>) {
        if self.busy.get_untracked()
            || self.pending.get_untracked().is_some()
            || text.trim().is_empty()
        {
            return;
        }
        let Some(held) = session::held() else { return };
        let request = crate::draft::uuid_string();
        let marker = PendingMarker::with_domain(
            request.clone(),
            held.tenant.clone(),
            "agent-turn",
            held.principal.clone(),
        );
        if let Err(e) = pending::write(&marker) {
            self.notice
                .set_if_alive(format!("Could not preserve this request: {e}"));
            return;
        }
        let mut body = json!({"tenant":held.tenant,"requestId":request,"threadId":self.thread.get_untracked(),"utterance":text,"goalId":self.selected.get_untracked()});
        body["companyId"] = if self.selected.get_untracked().is_some() {
            Value::Null
        } else {
            self.company
                .get_untracked()
                .map(|c| c["id"].clone())
                .unwrap_or(Value::Null)
        };
        if let Some((tool, values)) = action {
            body["action"] = json!({"tool":tool,"values":values});
        }
        self.sequence.update_if_alive(|n| *n += 1);
        self.pending.set_if_alive(Some(request.clone()));
        self.busy.set_if_alive(true);
        self.notice
            .set_if_alive("Ava is checking the business context and preparing a response…".into());
        spawn_local(async move {
            match session::api_fetch("POST","/api/agent/turns",Some(body.to_string())).await{
   Ok(a)if a.status==200&&a.body["requestId"]==request=>{pending::clear_by_request_id(&held.tenant,&held.principal,&request);self.pending.set_if_alive(None);self.input.set_if_alive(String::new());self.thread.set_if_alive(a.body["threadId"].as_str().map(str::to_owned));self.selected.set_if_alive(a.body["goalId"].as_str().map(str::to_owned));self.notice.set_if_alive(string(&a.body,"text"));self.refresh();},
   Ok(a)if (400..500).contains(&a.status)=>{pending::clear_by_request_id(&held.tenant,&held.principal,&request);self.pending.set_if_alive(None);self.notice.set_if_alive(error(&a.body));},
   _=>self.notice.set_if_alive("The planning response is unconfirmed. Check the original request before starting another.".into())};
            self.busy.set_if_alive(false);
        });
    }
    fn recover(self) {
        let (Some(held), Some(request)) = (session::held(), self.pending.get_untracked()) else {
            return;
        };
        self.busy.set_if_alive(true);
        spawn_local(async move {
            let path = format!(
                "/api/agent/state?tenant={}&requestId={}",
                encode(&held.tenant),
                encode(&request)
            );
            match session::api_fetch("GET",&path,None).await{Ok(a)if a.status==200&&a.body["requestId"]==request&&a.body["status"]=="committed"=>{pending::clear_by_request_id(&held.tenant,&held.principal,&request);self.pending.set_if_alive(None);self.thread.set_if_alive(a.body["turn"]["threadId"].as_str().map(str::to_owned));self.notice.set_if_alive("The original conversation response was recovered.".into());self.refresh();},Ok(a)if a.status==200&&a.body["requestId"]==request&&a.body["status"]=="not_committed"=>{pending::clear_by_request_id(&held.tenant,&held.principal,&request);self.pending.set_if_alive(None);self.notice.set_if_alive("The original planning request did not commit. You can send the goal again; no business action was authorized.".into());},_=>self.notice.set_if_alive("No confirmed response is available yet. Keep this original request reference and check again.".into())};
            self.busy.set_if_alive(false);
        });
    }
    fn control(self, goal: Value, action: &str) {
        if self.busy.get_untracked() || self.pending.get_untracked().is_some() {
            return;
        }
        let Some(held) = session::held() else { return };
        let action = action.to_string();
        let id = string(&goal, "id");
        let hash = string(&goal, "plan_hash");
        let path = if action == "approve" {
            "/api/agent/approve"
        } else {
            "/api/agent/control"
        };
        let mut body = json!({"tenant":held.tenant,"goalId":id,"planHash":hash,"confirmed":true});
        if action != "approve" {
            body["action"] = json!(action);
        }
        self.busy.set_if_alive(true);
        self.notice
            .set_if_alive("Checking the reviewed goal and current permissions…".into());
        spawn_local(async move {
            match session::api_fetch("POST",path,Some(body.to_string())).await{Ok(a)if a.status==200&&a.body["goal"]["id"]==id=>{self.notice.set_if_alive(format!("Goal status: {}. Recorded results and receipts are shown below.",a.body["goal"]["state"].as_str().unwrap_or("unknown")));},Ok(a)=>self.notice.set_if_alive(error(&a.body)),Err(_)=>self.notice.set_if_alive("The response is unconfirmed. The original goal and plan are retained; refresh its status before starting replacement work.".into())};
            self.busy.set_if_alive(false);
            self.refresh();
        });
    }
}
#[component]
pub(crate) fn AgentEntry() -> impl IntoView {
    let text = RwSignal::new(String::new());
    let draft = expect_context::<AgentDraft>();
    let navigate = use_navigate();
    view! {<section class="agent-entry" aria-labelledby="agent-entry-title"><h2 id="agent-entry-title">"What should Ava handle for your business?"</h2><p>"Describe the outcome. Review the plan, approve business changes, and follow the recorded results."</p><form on:submit=move|e|{e.prevent_default();let value=text.get_untracked();if !value.trim().is_empty(){draft.0.set_if_alive(Some(value));navigate("/app/desk",Default::default());}}><label class="ops-field"><span>"Business goal"</span><textarea data-testid="home-agent-goal" maxlength="1000" placeholder="For example: review low stock and prepare purchasing for my approval" prop:value=move||text.get() on:input=move|e|text.set_if_alive(event_target_value(&e))></textarea></label><button class="btn" data-testid="home-agent-start">"Work with Ava"</button></form></section>}
}
#[component]
pub(crate) fn AgentPanel() -> impl IntoView {
    let panel = Panel {
        company: RwSignal::new(None),
        input: RwSignal::new(String::new()),
        thread: RwSignal::new(None),
        turns: RwSignal::new(vec![]),
        threads: RwSignal::new(vec![]),
        goals: RwSignal::new(vec![]),
        busy: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        pending: RwSignal::new(None),
        selected: RwSignal::new(None),
        sequence: RwSignal::new(0),
    };
    let draft = expect_context::<AgentDraft>();
    Effect::new(move |_| {
        if let Some(held) = session::held() {
            if let Some(marker) = pending::read_family(&held.tenant, "agent-turn", &held.principal)
            {
                panel.pending.set_if_alive(Some(marker.request_id));
                panel
                    .notice
                    .set_if_alive("An earlier conversation request needs a status check.".into());
            }
            panel.refresh();
            if let Some(text) = draft.0.get_untracked() {
                draft.0.set_if_alive(None);
                panel.send(text, None);
            }
        }
    });
    #[cfg(target_arch = "wasm32")]
    {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let alive = Arc::new(AtomicBool::new(true));
        let cleanup = alive.clone();
        on_cleanup(move || cleanup.store(false, Ordering::Relaxed));
        spawn_local(async move {
            for _ in 0..150 {
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    if let Some(window) = web_sys::window() {
                        let _ = window
                            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 2000);
                    }
                });
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                if panel.goals.try_get_untracked().is_some_and(|g| {
                    g.iter()
                        .any(|g| matches!(g["state"].as_str(), Some("approved" | "running")))
                }) {
                    panel.refresh();
                }
            }
        });
    }
    view! {<section class="agent-panel" data-testid="agent-panel"><h2>"Ava · your business agent"</h2><p class="hint">"Tell Ava the outcome you want. Business actions wait for a reviewed plan and the required human approval."</p>
    <details class="ops-disclosure"><summary>"Company for a new trade goal"</summary><crate::operations::company_context::CompanyPicker selected=panel.company blocked=Signal::derive(move||panel.busy.get()||panel.pending.get().is_some()) on_change=Callback::new(move|()|panel.selected.set_if_alive(None))/><p class="hint">"Existing goals keep their reviewed company. Changing this selection starts a new goal."</p></details>
    <div class="ops-actions"><label class="ops-field"><span>"Your conversations"</span><select data-testid="agent-thread" disabled=move||panel.busy.get()||panel.pending.get().is_some() prop:value=move||panel.thread.get().unwrap_or_default() on:change=move|e|{panel.thread.set_if_alive(Some(event_target_value(&e)));panel.selected.set_if_alive(None);panel.refresh();}>{move||panel.threads.get().into_iter().map(|t|view!{<option value=string(&t,"id")>{format!("Conversation · {}",string(&t,"created_at"))}</option>}).collect::<Vec<_>>()}</select></label><button type="button" class="btn secondary" data-testid="agent-new-thread" disabled=move||panel.busy.get()||panel.pending.get().is_some() on:click=move|_|{panel.sequence.update_if_alive(|n|*n+=1);panel.thread.set_if_alive(None);panel.selected.set_if_alive(None);panel.turns.set_if_alive(vec![]);panel.goals.set_if_alive(vec![]);panel.input.set_if_alive(String::new());panel.notice.set_if_alive("Start a new conversation with your business goal.".into());}>"New conversation"</button></div>
    <form class="agent-goal-form" on:submit=move|e|{e.prevent_default();panel.send(panel.input.get_untracked(),None)}><label class="ops-field"><span>"Your goal or follow-up"</span><textarea data-testid="agent-prompt" maxlength="1000" prop:value=move||panel.input.get() on:input=move|e|panel.input.set_if_alive(event_target_value(&e)) placeholder="Review stock and suppliers, then prepare purchasing for my approval"></textarea></label><div class="ops-actions"><button class="btn" data-testid="agent-send" disabled=move||panel.busy.get()||panel.pending.get().is_some()>"Send to Ava"</button><button class="btn secondary" type="button" on:click=move|_|{panel.selected.set_if_alive(None);panel.input.set_if_alive(String::new());}>"New goal"</button><button class="btn secondary" type="button" data-testid="agent-refresh" disabled=move||panel.busy.get() on:click=move|_|panel.refresh()>"Refresh status"</button></div></form>
    <p class="status" role="status" data-testid="agent-status">{move||panel.notice.get()}</p>
    {move||panel.pending.get().map(|reference|view!{<div class="notice"><p>"Original request: "<code>{reference}</code></p><button type="button" class="btn secondary" data-testid="agent-recover" on:click=move|_|panel.recover()>"Check original request"</button></div>})}
    <ol class="agent-conversation" data-testid="agent-conversation">{move||panel.turns.get().into_iter().map(|t|view!{<li><p class="agent-user-message">{string(&t,"utterance")}</p><p data-testid="agent-reply">{string(&t["response"],"text")}</p>{t["response"]["planner"]["limitation"].as_str().map(|s|view!{<p class="hint">{s.to_string()}</p>})}</li>}).collect::<Vec<_>>()}</ol>
    <div class="agent-goals" data-testid="agent-goals">{move||panel.goals.get().into_iter().map(|goal|view!{<GoalCard panel goal/>}).collect::<Vec<_>>()}</div>
    </section>}
}
#[component]
fn GoalCard(panel: Panel, goal: Value) -> impl IntoView {
    let goal = StoredValue::new(goal);
    let state = string(&goal.get_value(), "state");
    let id = string(&goal.get_value(), "id");
    let financial_ready = goal.get_value()["plan"]["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .all(|s| crate::operations::agent_step_ready(&goal.get_value()["context"], s));
    let approve = state == "planned" && financial_ready;
    let module = RwSignal::new(
        goal.get_value()["modules"][0]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    );
    let active = matches!(
        state.as_str(),
        "planned"
            | "approved"
            | "running"
            | "holding"
            | "awaiting_session"
            | "needs_review"
            | "clarifying"
    );
    view! {<article class="agent-goal-card" data-testid="agent-goal" data-goal=id data-state=state.clone()><div class="panel-heading"><h3>{string(&goal.get_value(),"objective")}</h3><strong data-testid="agent-goal-state">{state.clone()}</strong></div><p>{string(&goal.get_value()["plan"],"summary")}</p>
    {goal.get_value()["failure"].as_str().map(|s|view!{<p class="notice">{s.to_string()}</p>})}
    <ol class="agent-plan-steps">{goal.get_value()["plan"]["steps"].as_array().cloned().unwrap_or_default().into_iter().enumerate().map(|(i,s)|view!{<li><strong>{format!("{}. {}",i+1,string(&s,"title"))}</strong>{if s["tool"].as_str().is_some_and(|t|t.starts_with("accounting.")){crate::operations::agent_step_review(&goal.get_value()["context"],&s)}else{view!{<Facts value=s["input"].clone()/>}.into_any()}}{s["guards"].as_array().filter(|g|!g.is_empty()).map(|g|view!{<details open=!s["tool"].as_str().is_some_and(|t|t.starts_with("accounting."))><summary>"Reviewed source facts and costs"</summary><Facts value=json!(g)/></details>})}</li>}).collect::<Vec<_>>()}</ol>
    <details class="agent-sources"><summary>"Business context used for this plan"</summary><p class="hint">"These are bounded authorized records from planning time, not complete business totals."</p>{goal.get_value()["context"]["sources"].as_array().cloned().unwrap_or_default().into_iter().map(|s|view!{<section><h4>{string(&s,"module")}</h4>{if s["available"]==true{view!{<Facts value=s["data"].clone()/>}.into_any()}else{view!{<p>"Unavailable under current access or configuration."</p>}.into_any()}}</section>}).collect::<Vec<_>>()}</details>
    {(!financial_ready).then(||view!{<p class="refusal">"Financial review is incomplete. Refresh this plan to load the exact book, journal lines and original accounts, or use Accounting to review the record."</p>})}
    <div class="ops-actions">{approve.then(||view!{<button type="button" class="btn" data-testid="agent-approve" disabled=move||panel.busy.get() on:click=move|_|panel.control(goal.get_value(),"approve")>"Approve this plan and start"</button>})}
    {active.then(||view!{<button type="button" class="btn secondary" data-testid="agent-cancel" disabled=move||panel.busy.get() on:click=move|_|panel.control(goal.get_value(),"cancel")>"Cancel remaining work"</button>})}
    {(state=="running").then(||view!{<button type="button" class="btn secondary" data-testid="agent-pause" on:click=move|_|panel.control(goal.get_value(),"pause")>"Pause"</button>})}
    {matches!(state.as_str(),"holding"|"awaiting_session").then(||view!{<button type="button" class="btn secondary" data-testid="agent-resume" on:click=move|_|panel.control(goal.get_value(),"resume")>"Resume reviewed work"</button>})}
    <button type="button" class="btn secondary" data-testid="agent-goal-report" on:click=move|_|{panel.selected.set_if_alive(Some(string(&goal.get_value(),"id")));panel.send("Report the mission status and verified outcome.".into(),None)}>"Ask Ava for the result"</button>
    {active.then(||view!{<button type="button" class="btn secondary" data-testid="agent-replan" on:click=move|_|{panel.selected.set_if_alive(Some(string(&goal.get_value(),"id")));panel.send("Refresh this plan using the latest business context and keep completed receipts.".into(),None)}>"Refresh plan for review"</button>})}</div>
    {goal.get_value()["mission_id"].as_str().map(|_|view!{<a href="/app/erp?module=missions">"Open mission approvals and checkpoints"</a>})}
    {goal.get_value()["outcome"].as_object().map(|_|view!{<section data-testid="agent-outcome"><h4>"Recorded result"</h4><Facts value=goal.get_value()["outcome"].clone()/></section>})}
    {(state=="clarifying").then(||view!{<details class="agent-action-details"><summary>"Provide action details for Ava’s plan"</summary><label class="ops-field"><span>"Business area"</span><select prop:value=move||module.get() on:change=move|e|module.set_if_alive(event_target_value(&e))>{goal.get_value()["modules"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|m|m.as_str().and_then(archava_domain::operator::module).map(|m|view!{<option value=string(m,"id")>{string(m,"label")}</option>})).collect::<Vec<_>>()}</select></label>{move||archava_domain::operator::module(&module.get()).and_then(|m|m["id"].as_str()).map(|domain|view!{<crate::operations::AgentActionEditor domain context=goal.get_value()["context"].clone() on_plan=Callback::new(move|action:(String,Value)|{panel.selected.set_if_alive(Some(string(&goal.get_value(),"id")));panel.send(format!("Prepare the reviewed {} action for this goal",action.0),Some(action));})/>})}</details>})}
    </article>}
}
#[component]
fn Facts(value: Value) -> AnyView {
    match value{
  Value::Object(map)=>{if let Some(binding)=map.get("$from"){return view!{<span>{format!("Use the verified result from step {}",binding["step"].as_u64().unwrap_or(0)+1)}</span>}.into_any()}
   view!{<dl class="agent-facts">{map.into_iter().filter(|(k,_)|!k.starts_with("__")&&!matches!(k.as_str(),"accessHash"|"session_hash"|"approval_gate")).map(|(k,v)|view!{<dt>{human_label(&k)}</dt><dd><Facts value=v/></dd>}).collect::<Vec<_>>()}</dl>}.into_any()},
  Value::Array(rows)=>view!{<ul>{rows.into_iter().map(|v|view!{<li><Facts value=v/></li>}).collect::<Vec<_>>()}</ul>}.into_any(),
  Value::String(s)=>view!{<span>{s}</span>}.into_any(),Value::Null=>view!{<span>"—"</span>}.into_any(),v=>view!{<span>{v.to_string()}</span>}.into_any()
 }
}
fn human_label(key: &str) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if c == '_' {
            out.push(' ')
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase())
        } else {
            out.push(c)
        }
    }
    out
}
