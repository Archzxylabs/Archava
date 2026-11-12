use super::*;
#[derive(Clone, Copy)]
struct MissionUi {
    rows: RwSignal<Vec<Value>>,
    mission: RwSignal<Value>,
    selected: RwSignal<String>,
    title: RwSignal<String>,
    code: RwSignal<String>,
    busy: RwSignal<bool>,
    notice: RwSignal<String>,
    uncertain: RwSignal<bool>,
    can_write: RwSignal<bool>,
    refresh: RwSignal<u64>,
    signal_key: RwSignal<String>,
}
impl MissionUi {
    fn load(self) {
        let Some(held) = session::held() else { return };
        let domain = format!("missions:{}", held.principal);
        if let Some(marker) = pending::read_scoped(&held.tenant, &domain, &held.principal) {
            self.code.set_if_alive(marker.request_id);
            self.uncertain.set_if_alive(true);
            self.notice
                .set_if_alive("An earlier mission creation needs a status check.".into());
        }
        let path = format!(
            "/api/v1/cockpit/missions?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer) if answer.status == 200 && answer.body["items"].is_array() => {
                    let items = answer.body["items"].as_array().cloned().unwrap_or_default();
                    self.can_write.set_if_alive(answer.body["canWrite"] == true);
                    if let Some(found) = items.iter().find(|m| {
                        self.code
                            .try_get_untracked()
                            .is_some_and(|code| m["mission_code"] == code)
                    }) {
                        self.selected.set_if_alive(text(found, "id"));
                        pending::clear_by_request_id(
                            &held.tenant,
                            &held.principal,
                            &text(found, "mission_code"),
                        );
                        self.uncertain.set_if_alive(false);
                    }
                    self.rows.set_if_alive(items);
                }
                _ => self.notice.set_if_alive(
                    "Mission records could not be loaded. Refresh to try again.".into(),
                ),
            }
        });
    }
    fn create(self) {
        if self.busy.get_untracked() || self.uncertain.get_untracked() {
            return;
        }
        let Some(held) = session::held() else { return };
        let key = uuid_string();
        let domain = format!("missions:{}", held.principal);
        if pending::write(&PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            domain.clone(),
            held.principal.clone(),
        ))
        .is_err()
        {
            self.notice
                .set_if_alive("The mission recovery reference could not be saved.".into());
            return;
        }
        self.code.set_if_alive(key.clone());
        self.busy.set_if_alive(true);
        let body=json!({"tenant_id":held.tenant,"mission_code":key,"title":self.title.get_untracked(),"description":"Read current business workspace summaries. No business records are changed.","auto_launch":true,"steps":[
   {"step_index":0,"step_name":"Review sales workspace","tool_name":"commerce.workspace.read","payload_input":{},"depends_on":[]},
   {"step_index":1,"step_name":"Review purchasing workspace","tool_name":"purchasing.workspace.read","payload_input":{},"depends_on":[0]},
   {"step_index":2,"step_name":"Review invoicing workspace","tool_name":"billing.workspace.read","payload_input":{},"depends_on":[1]}]}).to_string();
        spawn_local(async move {
            match session::api_fetch("POST", "/api/v1/cockpit/missions", Some(body)).await {
                Ok(answer)
                    if matches!(answer.status, 200 | 201)
                        && answer.body["mission_code"] == key
                        && answer.body["id"].is_string() =>
                {
                    self.selected.set_if_alive(text(&answer.body, "id"));
                    self.mission.set_if_alive(answer.body);
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    self.notice.set_if_alive(
                        "Mission created. Run each ready step to collect a current observation."
                            .into(),
                    );
                    self.refresh.update_if_alive(|n| *n += 1);
                }
                Ok(answer) if (400..500).contains(&answer.status) => {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    self.notice.set_if_alive(message(&answer));
                }
                _ => {
                    self.uncertain.set_if_alive(true);
                    self.notice.set_if_alive("The mission outcome is unconfirmed. Refresh its status before creating another.".into());
                }
            }
            self.busy.set_if_alive(false);
        });
    }
    fn run(self, id: String, index: i64) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(held) = session::held() else { return };
        self.busy.set_if_alive(true);
        let path = format!(
            "/api/v1/cockpit/missions/{id}/run-next?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("POST",&path,Some(json!({"stepIndex":index}).to_string())).await{
    Ok(answer)if answer.status==200&&answer.body["id"].is_string()=>{self.mission.set_if_alive(answer.body);self.notice.set_if_alive("The tool observation is saved.".into());},
    Ok(answer)=>self.notice.set_if_alive(message(&answer)),Err(_)=>self.notice.set_if_alive("Step outcome unconfirmed. Refresh, then retry this same step if it is still pending.".into())
   }
            self.busy.set_if_alive(false);
            self.refresh.update_if_alive(|n| *n += 1);
        });
    }
    fn resume(self, id: String) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(held) = session::held() else { return };
        let signal_key = self.signal_key.get_untracked();
        if signal_key.trim().is_empty() {
            self.notice
                .set_if_alive("A signal key is required to resume the mission.".into());
            return;
        }
        self.busy.set_if_alive(true);
        let path = format!(
            "/api/v1/cockpit/missions/{id}/resume?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch(
                "POST",
                &path,
                Some(json!({"signal_key": signal_key}).to_string()),
            )
            .await
            {
                Ok(answer) if answer.status == 200 && answer.body["id"].is_string() => {
                    self.mission.set_if_alive(answer.body);
                    self.signal_key.set_if_alive(String::new());
                    self.notice
                        .set_if_alive("Mission resumed. Steps are now active.".into());
                }
                Ok(answer) => self.notice.set_if_alive(message(&answer)),
                Err(_) => self
                    .notice
                    .set_if_alive("Resume outcome unconfirmed. Refresh to verify.".into()),
            }
            self.busy.set_if_alive(false);
            self.refresh.update_if_alive(|n| *n += 1);
        });
    }
    fn approve_step(self, id: String, index: i64) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(held) = session::held() else { return };
        self.busy.set_if_alive(true);
        let path = format!(
            "/api/v1/cockpit/missions/{id}/steps/{index}/approve?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("POST", &path, Some(json!({}).to_string())).await {
                Ok(answer) if answer.status == 200 && answer.body["id"].is_string() => {
                    self.mission.set_if_alive(answer.body);
                    self.notice.set_if_alive(
                        "Step approved by supervisor. Step is now ready to run.".into(),
                    );
                }
                Ok(answer) => self.notice.set_if_alive(message(&answer)),
                Err(_) => self
                    .notice
                    .set_if_alive("Approval outcome unconfirmed. Refresh to verify.".into()),
            }
            self.busy.set_if_alive(false);
            self.refresh.update_if_alive(|n| *n += 1);
        });
    }
}
#[component]
pub fn Missions() -> impl IntoView {
    let ui = MissionUi {
        rows: RwSignal::new(Vec::new()),
        mission: RwSignal::new(json!({})),
        selected: RwSignal::new(String::new()),
        title: RwSignal::new(String::new()),
        code: RwSignal::new(String::new()),
        busy: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        uncertain: RwSignal::new(false),
        can_write: RwSignal::new(false),
        refresh: RwSignal::new(0),
        signal_key: RwSignal::new(String::new()),
    };
    Effect::new(move |_| {
        ui.refresh.track();
        ui.load();
    });
    Effect::new(move |_| {
        let id = ui.selected.get();
        ui.refresh.track();
        if id.is_empty() {
            return;
        }
        let Some(held) = session::held() else { return };
        let path = format!(
            "/api/v1/cockpit/missions/{id}?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer)
                    if answer.status == 200
                        && ui.selected.try_get_untracked() == Some(id.clone()) =>
                {
                    ui.mission.set_if_alive(answer.body)
                }
                _ => ui
                    .notice
                    .set_if_alive("The selected mission is unavailable.".into()),
            }
        });
    });
    view! {
     <header class="ops-section-heading"><div><h2>"Missions"</h2><p class="hint">"Private, durable workflows. Each tool uses your current domain access."</p></div><button type="button" class="btn secondary" data-testid="missions-refresh" on:click=move|_|ui.refresh.update_if_alive(|n|*n+=1)>"Refresh status"</button></header>
     <p class="status" aria-live="polite" data-testid="missions-notice">{move||ui.notice.get()}</p>
     <div class="ops-layout"><section class="ops-main"><ul class="ops-mission-list">
      {move||ui.rows.get().into_iter().map(|row|{let id=text(&row,"id");let auth=text(&row,"required_authority");view!{<li><button type="button" data-testid="mission-open" on:click=move|_|ui.selected.set_if_alive(id.clone())><strong>{text(&row,"title")}</strong><span>{text(&row,"status")}</span>{(!auth.is_empty()).then(|| view!{<small class="pill">{auth.clone()}</small>})}</button></li>}}).collect::<Vec<_>>()}
     </ul>{move||{let data=ui.mission.get();data["id"].is_string().then(||view!{<MissionDetail ui data/>})}}</section>
     <aside class="ops-aside"><div hidden=move||!ui.can_write.get()><form class="ops-form" on:submit=move|e|{e.prevent_default();ui.create();}><h3>"New business review"</h3><Field label="Mission title" id="mission-title" value=ui.title/><p class="hint">"Creates three dependent read steps for sales, purchasing and invoicing. Your grants are checked again for every step."</p><button type="submit" class="btn" data-testid="mission-create" disabled=move||ui.busy.get()||ui.uncertain.get()>"Create review mission"</button></form></div></aside></div>
    }
}
#[component]
fn MissionDetail(ui: MissionUi, data: Value) -> impl IntoView {
    let steps = data["steps"].as_array().cloned().unwrap_or_default();
    let is_holding = data["status"] == "holding_pattern";
    let holding_reason = text(&data, "holding_reason");
    let mission_id = text(&data, "id");
    let rows=steps.into_iter().map(|step|{
  let id=text(&data,"id");let index=integer(&step,"step_index");let state=text(&step,"status");
  let needs_approval = state == "requires_approval";
  let ready=!is_holding && state=="pending"&&step["depends_on_steps"].as_array().into_iter().flatten().all(|dep|data["steps"].as_array().into_iter().flatten().any(|s|s["step_index"]==*dep&&s["status"]=="passed"));
  let approve_id = id.clone();
  let run_id = id;
  view!{<li><strong>{text(&step,"step_name")}</strong><span>{state}</span>
   {needs_approval.then(move||view!{<button type="button" class="btn secondary warning" data-testid="mission-step-approve" disabled=move||ui.busy.get()||!ui.can_write.get() on:click=move|_|ui.approve_step(approve_id.clone(),index)>"Approve step"</button>})}
   {ready.then(move||view!{<button type="button" class="btn secondary" data-testid="mission-run" disabled=move||ui.busy.get()||!ui.can_write.get() on:click=move|_|ui.run(run_id.clone(),index)>"Run step"</button>})}
   <p class="hint">{text(&step,"isolated_context_summary")}</p><details><summary>"Reviewed step details"</summary><p>{text(&step,"tool_target")}</p>{operator::details(step["payload_input"].clone())}{operator::details(step["payload_output"].clone())}</details></li>}
 }).collect::<Vec<_>>();
    view! {
        <section class="ops-detail" data-testid="mission-detail">
            <p class="eyebrow">"BUSINESS REVIEW"</p>
            <h3>{text(&data,"title")}</h3>
            <p data-testid="mission-status">{text(&data,"status")}</p>
            {is_holding.then(move || view! {
                <div class="callout warning" data-testid="mission-holding-banner">
                    <h4>"Mission In Holding Pattern"</h4>
                    <p class="hint">{if holding_reason.is_empty() { "Holding pattern awaiting supervisor approval signal.".to_string() } else { holding_reason }}</p>
                    <div class="row">
                        <Field label="Signal Key" id="mission-resume-signal" value=ui.signal_key/>
                        <button type="button" class="btn secondary" data-testid="mission-resume" disabled=move||ui.busy.get()||!ui.can_write.get() on:click=move|_|ui.resume(mission_id.clone())>"Resume mission"</button>
                    </div>
                </div>
            })}
            <ol class="ops-mission-steps">{rows}</ol>
            <p class="hint">"Completed means these read tools finished. It does not confirm payment, dispatch, or other external effects."</p>
        </section>
    }
}
