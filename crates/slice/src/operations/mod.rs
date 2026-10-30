//! Native operator workspace. Views share receipt/recovery mechanics, not business authority.
mod billing;
mod commerce;
mod mail;
mod missions;
mod payments;
mod purchasing;
mod support;
use crate::{
    pending::{self, PendingMarker},
    session::{self, Answer},
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_query_map};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Commerce,
    Purchasing,
    Billing,
    Support,
}
impl Domain {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Commerce => "commerce",
            Self::Purchasing => "purchasing",
            Self::Billing => "billing",
            Self::Support => "support",
        }
    }
    fn collection(self) -> &'static str {
        match self {
            Self::Commerce | Self::Purchasing | Self::Billing => "workspace",
            Self::Support => "cases",
        }
    }
    fn expected(self) -> &'static str {
        match self {
            Self::Commerce => "products",
            Self::Purchasing => "orders",
            Self::Billing => "invoices",
            Self::Support => "items",
        }
    }
    fn envelope(self, tenant: &str, key: &str, mut command: Value) -> Value {
        match self {
            Self::Commerce | Self::Purchasing => {
                json!({"tenant":tenant,"requestId":key,"command":command})
            }
            Self::Support => {
                json!({"tenant":tenant,"mutation":{"requestId":key,"command":command}})
            }
            Self::Billing => {
                command["commandId"] = json!(key);
                json!({"tenant":tenant,"mutation":command})
            }
        }
    }
}
#[derive(Clone, Debug)]
pub enum ActionState {
    Idle,
    Busy,
    Saved { reference: String, record: String },
    Refused(String),
    Uncertain { reference: String, message: String },
}
#[derive(Clone, Copy)]
pub struct Workbench {
    pub domain: Domain,
    pub data: RwSignal<Value>,
    pub ready: RwSignal<bool>,
    pub loading: RwSignal<bool>,
    pub notice: RwSignal<String>,
    pub search: RwSignal<String>,
    pub action: RwSignal<ActionState>,
    retry: RwSignal<Option<String>>,
    last: RwSignal<Option<(String, String)>>,
    sequence: RwSignal<u64>,
}
fn storage_domain(domain: Domain, principal: &str) -> String {
    format!("erp-{}:{principal}", domain.slug())
}
fn receipt(body: &Value) -> Option<&Value> {
    if body["receipt"].is_object() {
        Some(&body["receipt"])
    } else if body["commandId"].is_string() && body["action"].is_string()
        || body["requestId"].is_string() && body["operation"].is_string()
    {
        Some(body)
    } else {
        None
    }
}
fn request_reference(body: &Value) -> Option<&str> {
    body["requestId"].as_str().or_else(|| {
        receipt(body).and_then(|r| r["requestId"].as_str().or_else(|| r["commandId"].as_str()))
    })
}
fn matching_receipt(body: &Value, key: &str) -> bool {
    let Some(r) = receipt(body) else { return false };
    let nested = r["requestId"].as_str().or_else(|| r["commandId"].as_str());
    request_reference(body) == Some(key)
        && nested.is_none_or(|id| id == key)
        && (r["id"].as_str().is_some_and(|id| !id.is_empty())
            || r["invoiceId"].as_str().is_some_and(|id| !id.is_empty()))
}
fn record_id(body: &Value) -> String {
    receipt(body)
        .and_then(|r| r["id"].as_str().or_else(|| r["invoiceId"].as_str()))
        .unwrap_or("")
        .to_string()
}
fn message(answer: &Answer) -> String {
    answer.body["message"]
        .as_str()
        .or_else(|| answer.body["error"]["message"].as_str())
        .unwrap_or("The operation could not be completed. Refresh the records before trying again.")
        .to_string()
}
impl Workbench {
    pub fn new(domain: Domain) -> Self {
        let work = Self {
            domain,
            data: RwSignal::new(json!({})),
            ready: RwSignal::new(false),
            loading: RwSignal::new(false),
            notice: RwSignal::new(String::new()),
            search: RwSignal::new(String::new()),
            action: RwSignal::new(ActionState::Idle),
            last: RwSignal::new(None),
            retry: RwSignal::new(None),
            sequence: RwSignal::new(0),
        };
        Effect::new(move |_| {
            if let Some(held) = session::held() {
                let key = storage_domain(domain, &held.principal);
                if let Some(marker) = pending::read_scoped(&held.tenant, &key, &held.principal) {
                    work.action.set_if_alive(ActionState::Uncertain{reference:marker.request_id,message:"An earlier action needs a receipt check before another action can start.".into()});
                }
                work.refresh();
            } else {
                work.notice
                    .set_if_alive("Sign in to use this workspace.".into());
            }
        });
        work
    }
    pub fn blocked(self) -> bool {
        matches!(
            self.action.get(),
            ActionState::Busy | ActionState::Uncertain { .. }
        )
    }
    pub fn permits(self, permission: &str) -> bool {
        self.data.get()["permissions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|p| p == permission))
    }
    pub fn rows(self, key: &str) -> Vec<Value> {
        self.data.get()[key].as_array().cloned().unwrap_or_default()
    }
    pub fn fail(self, message: impl Into<String>) {
        self.action
            .set_if_alive(ActionState::Refused(message.into()));
    }
    pub fn refresh(self) {
        if self.sequence.is_disposed() {
            return;
        }
        let Some(held) = session::held() else { return };
        self.sequence.update_if_alive(|s| *s += 1);
        let seq = self.sequence.get_untracked();
        let path = format!(
            "/erp/{}/{}?tenant={}&search={}",
            self.domain.slug(),
            self.domain.collection(),
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&self.search.get_untracked())
        );
        // Support has its own bounded collection query and accepts no search field.
        let path = if self.domain == Domain::Support {
            format!(
                "/erp/support/cases?tenant={}&limit=100",
                crate::contacts::query_encode(&held.tenant)
            )
        } else {
            path
        };
        self.loading.set_if_alive(true);
        self.notice.set_if_alive(String::new());
        spawn_local(async move {
            let response = session::api_fetch("GET", &path, None).await;
            if self.sequence.try_get_untracked() != Some(seq) {
                return;
            }
            self.loading.set_if_alive(false);
            match response {
                Ok(mut answer)
                    if answer.status == 200 && answer.body[self.domain.expected()].is_array() =>
                {
                    if self.domain == Domain::Support {
                        let access_path = format!(
                            "/erp/support/access?tenant={}",
                            crate::contacts::query_encode(&held.tenant)
                        );
                        match session::api_fetch("GET", &access_path, None).await {
                            Ok(access) if access.status == 200 => {
                                answer.body["permissions"] = access.body["permissions"].clone();
                            }
                            _ => {
                                self.ready.set_if_alive(false);
                                self.notice.set_if_alive(
                                    "Case access could not be verified. Refresh to try again."
                                        .into(),
                                );
                                return;
                            }
                        }
                    }
                    self.data.set_if_alive(answer.body);
                    self.ready.set_if_alive(true);
                }
                Ok(answer) => {
                    self.ready.set_if_alive(false);
                    self.notice.set_if_alive(message(&answer));
                }
                Err(error) => {
                    self.ready.set_if_alive(false);
                    self.notice
                        .set_if_alive(format!("Workspace unavailable: {error}"));
                }
            }
        });
    }
    pub fn send(self, command: Value) {
        if self.blocked() {
            return;
        }
        for field in ["reason", "resolution"] {
            if command
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|v| v.trim().is_empty())
            {
                self.fail("Add an action reason or resolution before saving.");
                return;
            }
        }
        let Some(held) = session::held() else {
            self.fail("Sign in before saving an action.");
            return;
        };
        let signature =
            json!({"tenant":held.tenant,"principal":held.principal.clone(),"command":command})
                .to_string();
        let resuming = self.retry.get_untracked();
        let key = match (resuming.clone(), self.last.get_untracked()) {
            (Some(key), _) => key,
            (None, Some((old, key))) if old == signature => key,
            _ => uuid_string(),
        };
        self.last.set_if_alive(Some((signature, key.clone())));
        let domain = storage_domain(self.domain, &held.principal);
        let marker = PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            domain.clone(),
            held.principal.clone(),
        );
        if let Err(error) = pending::write(&marker) {
            self.fail(format!(
                "Could not preserve this action for recovery: {error}"
            ));
            return;
        }
        let body = self
            .domain
            .envelope(&held.tenant, &key, command)
            .to_string();
        self.action.set_if_alive(ActionState::Busy);
        spawn_local(async move {
            match session::api_fetch("POST",&format!("/erp/{}/commands",self.domain.slug()),Some(body)).await{
                Ok(answer)if answer.status==200&&matching_receipt(&answer.body,&key)=>{
                    pending::clear_by_request_id(&held.tenant,&held.principal,&key);self.retry.set_if_alive(None);let record=record_id(&answer.body);
                    self.action.set_if_alive(ActionState::Saved{reference:key,record});self.refresh();
                },
                Ok(answer)if (400..500).contains(&answer.status)&&resuming.is_none()=>{pending::clear_by_request_id(&held.tenant,&held.principal,&key);self.action.set_if_alive(ActionState::Refused(message(&answer)));},
                Ok(answer)if (400..500).contains(&answer.status)=>self.action.set_if_alive(ActionState::Uncertain{reference:key,message:format!("The retry was refused: {}. The original action remains unresolved; keep its reference and review the original details.",message(&answer))}),
                Ok(_)=>self.action.set_if_alive(ActionState::Uncertain{reference:key,message:"A matching committed receipt was not received. Check this action before starting another.".into()}),
                Err(error)=>self.action.set_if_alive(ActionState::Uncertain{reference:key,message:format!("The outcome is unconfirmed: {error}")}),
            }
        });
    }
    pub fn new_intent(self) {
        if self.blocked() || self.retry.get_untracked().is_some() {
            return;
        }
        self.last.set_if_alive(None);
        self.action.set_if_alive(ActionState::Idle);
    }
    pub fn review_retry(self) {
        let ActionState::Uncertain { reference, .. } = self.action.get_untracked() else {
            return;
        };
        self.retry.set_if_alive(Some(reference));
        self.action.set_if_alive(ActionState::Idle);
    }
    pub fn recover(self) {
        let ActionState::Uncertain { reference, .. } = self.action.get_untracked() else {
            return;
        };
        let Some(held) = session::held() else { return };
        let path = format!(
            "/erp/{}/commands/status?tenant={}&requestId={}",
            self.domain.slug(),
            crate::contacts::query_encode(&held.tenant),
            reference
        );
        spawn_local(async move {
            match session::api_fetch("GET",&path,None).await{
                Ok(answer)if answer.status==200&&matching_receipt(&answer.body,&reference)=>{
                    pending::clear_by_request_id(&held.tenant,&held.principal,&reference);self.retry.set_if_alive(None);let record=record_id(&answer.body);self.action.set_if_alive(ActionState::Saved{reference,record});self.refresh();
                },
                _=>self.action.set_if_alive(ActionState::Uncertain{reference,message:"No matching receipt is visible yet. The action may still be running; check again before retrying.".into()}),
            }
        });
    }
}
use crate::draft::uuid_string;

#[component]
pub fn Field(
    label: &'static str,
    id: &'static str,
    value: RwSignal<String>,
    #[prop(default = "text")] kind: &'static str,
    #[prop(default = true)] required: bool,
) -> impl IntoView {
    view! {<label class="ops-field"><span>{label}</span><input data-testid=id type=kind prop:value=move||value.get() on:input=move|e|value.set_if_alive(event_target_value(&e)) required=required /></label>}
}
#[component]
pub fn Pick(
    label: &'static str,
    id: &'static str,
    value: RwSignal<String>,
    options: Signal<Vec<(String, String)>>,
) -> impl IntoView {
    view! {<label class="ops-field"><span>{label}</span><select data-testid=id prop:value=move||value.get() on:change=move|e|value.set_if_alive(event_target_value(&e)) required><option value="">"Choose…"</option>{move||options.get().into_iter().map(|(id,label)|view!{<option value=id>{label}</option>}).collect::<Vec<_>>()}</select></label>}
}
#[component]
pub fn ActionButton(
    work: Workbench,
    command: Value,
    label: &'static str,
    id: &'static str,
) -> impl IntoView {
    view! {<button type="button" class="btn secondary" data-testid=id disabled=move||work.blocked() on:click=move|_|work.send(command.clone())>{label}</button>}
}
#[component]
pub fn Feedback(work: Workbench) -> impl IntoView {
    view! {<div aria-live="polite" class="ops-feedback" data-testid="ops-feedback">
        {move||match work.action.get(){
            ActionState::Idle=>view!{<span></span>}.into_any(),
            ActionState::Busy=>view!{<p class="status">"Saving…"</p>}.into_any(),
            ActionState::Saved{reference,record}=>view!{<div class="status ops-saved" data-testid="ops-saved">"Saved. "<span>{if record.is_empty(){String::new()}else{format!("Record {}. ",short(&record))}}</span><details><summary>"Receipt reference"</summary><code>{reference}</code></details><button type="button" class="ops-new-intent" data-testid="ops-new-intent" on:click=move|_|work.new_intent()>"Start another action"</button></div>}.into_any(),
            ActionState::Refused(reason)=>view!{<p class="refusal" data-testid="ops-refused">{reason}</p>}.into_any(),
            ActionState::Uncertain{reference,message}=>view!{<div class="refusal" data-testid="ops-uncertain"><p>{message}</p><p>"Reference: "<code>{reference}</code></p><button type="button" class="btn secondary" data-testid="ops-recover" on:click=move|_|work.recover()>"Check action status"</button><button type="button" class="btn secondary" data-testid="ops-review-retry" on:click=move|_|work.review_retry()>"Review and retry original action"</button></div>}.into_any(),
        }}
        {move||work.retry.get().map(|key|view!{<p class="status" data-testid="ops-retry-review">"Review the original action details below, then save to retry under reference "<code>{key}</code>". This reuses the original key; a conflicting payload will be refused. No action is sent by opening this review."</p>})}
        {move||{let notice=work.notice.get();(!notice.is_empty()).then(||view!{<p class="refusal" data-testid="ops-load-error">{notice}</p>})}}
        {move||work.loading.get().then(||view!{<p class="status">"Refreshing records…"</p>})}
    </div>}
}
#[component]
pub fn Toolbar(work: Workbench, title: &'static str) -> impl IntoView {
    view! {<header class="ops-section-heading"><div><h2>{title}</h2><p class="hint">"Search and refresh records available to your workspace role. Up to 100 recent matches are shown."</p></div><form class="ops-search" on:submit=move|e|{e.prevent_default();work.refresh();}><input aria-label="Search records" placeholder="Search records…" data-testid="ops-search" prop:value=move||work.search.get() on:input=move|e|work.search.set_if_alive(event_target_value(&e))/><button class="btn secondary" type="submit" data-testid="ops-refresh">"Refresh"</button></form></header>}
}
pub fn number(value: RwSignal<String>) -> Option<i64> {
    value.get_untracked().trim().parse().ok()
}
pub fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}
pub fn integer(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
}
pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}
pub fn money(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.')
        }
        out.push(c)
    }
    format!("{}Rp {}", if amount < 0 { "−" } else { "" }, out)
}

#[component]
pub fn Operations() -> impl IntoView {
    let query = use_query_map();
    let active = RwSignal::new("catalog".to_string());
    let navigate = use_navigate();
    Effect::new(move |_| {
        active.set_if_alive(
            query
                .get()
                .get("module")
                .unwrap_or_else(|| "catalog".into()),
        );
    });
    let tabs = [
        ("catalog", "Catalog"),
        ("sales", "Sales orders"),
        ("purchasing", "Purchasing"),
        ("invoices", "Invoices"),
        ("support", "Support"),
        ("missions", "Missions"),
    ];
    view! {<section class="operations-page" data-testid="operations-workspace"><header class="page-heading"><p class="eyebrow">"BUSINESS WORKSPACE"</p><h1>"Operations"</h1><p>"Manage the records behind your daily work."</p></header><nav class="ops-tabs" aria-label="Operations modules">{tabs.into_iter().map(|(id,label)|{let navigate=navigate.clone();view!{<button type="button" class:active=move||active.get()==id data-testid=format!("ops-tab-{id}") on:click=move|_|navigate(&format!("/app/erp?module={id}"),Default::default())>{label}</button>}}).collect::<Vec<_>>()}</nav>
        {move||match active.get().as_str(){
            "sales"=>view!{<commerce::Sales/>}.into_any(),"purchasing"=>view!{<purchasing::Purchasing/>}.into_any(),"invoices"=>view!{<billing::Invoices/>}.into_any(),"support"=>view!{<support::Support/>}.into_any(),"missions"=>view!{<missions::Missions/>}.into_any(),_=>view!{<commerce::Catalog/>}.into_any(),
        }}
    </section>}
}

#[component]
pub fn Records(
    work: Workbench,
    collection: &'static str,
    columns: Vec<(&'static str, &'static str)>,
    selected: RwSignal<String>,
) -> impl IntoView {
    let headings = columns.clone();
    view! {<div class="ops-records">
      {move||if !work.ready.get(){view!{<p class="hint">"Waiting for workspace records…"</p>}.into_any()}
      else{let rows=work.rows(collection);if rows.is_empty(){view!{<div class="ops-empty"><h3>"No records yet"</h3><p>"Create a record to start this workflow, or adjust your search."</p></div>}.into_any()}
      else{let columns=columns.clone();view!{<div class="ops-table-wrap"><table class="ops-table"><thead><tr>{headings.iter().map(|(_,label)|view!{<th>{*label}</th>}).collect::<Vec<_>>()}<th>"Details"</th></tr></thead><tbody>{rows.into_iter().map(|row|{let id=text(&row,"id");let chosen=id.clone();view!{<tr class:selected=move||selected.get()==chosen>{columns.iter().map(|(key,_)|{let v=&row[*key];let display=if let Some(s)=v.as_str(){s.to_string()}else if v.is_null(){"—".into()}else{v.to_string()};view!{<td>{display}</td>}}).collect::<Vec<_>>()}<td><button type="button" class="ops-open" data-testid=format!("ops-open-{}",id) on:click=move|_|selected.set_if_alive(id.clone())>"Open"</button></td></tr>}}).collect::<Vec<_>>()}</tbody></table></div>}.into_any()}}}
    </div>}
}

pub fn currency_amount(amount: i64, currency: &str) -> String {
    if currency == "IDR" {
        money(amount)
    } else {
        format!("{currency} {amount} minor units")
    }
}

// A fetch may finish after its route owner has been disposed. Preserve durable
// marker handling, but never write into a destroyed view or spawn another read.
trait AliveSignal<T> {
    fn set_if_alive(&self, value: T);
    fn update_if_alive(&self, update: impl FnOnce(&mut T));
}
impl<T: Send + Sync + 'static> AliveSignal<T> for RwSignal<T> {
    fn set_if_alive(&self, value: T) {
        let _ = self.try_set(value);
    }
    fn update_if_alive(&self, update: impl FnOnce(&mut T)) {
        let _ = self.try_update(update);
    }
}

pub fn exact_money(value: &Value) -> String {
    let Some(digits) = value
        .as_str()
        .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
    else {
        return "Unknown cost".into();
    };
    let mut out = String::from("Rp ");
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.')
        }
        out.push(c)
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_money_keeps_large_values_and_unknown_cost_distinct_from_zero() {
        assert_eq!(
            exact_money(&json!("1000000000000000000")),
            "Rp 1.000.000.000.000.000.000"
        );
        assert_eq!(exact_money(&json!("0")), "Rp 0");
        assert_eq!(exact_money(&Value::Null), "Unknown cost");
    }
    #[test]
    fn receipts_match_wire_shape_original_key_and_resource() {
        let commerce =
            json!({"requestId":"a","receipt":{"kind":"product","id":"product-id","revision":1}});
        let purchasing = json!({"requestId":"a","receipt":{"requestId":"a","operation":"order.create","id":"order-id","revision":1}});
        let billing = json!({"commandId":"a","action":"create_invoice","invoiceId":"invoice-id"});
        let support =
            json!({"requestId":"a","operation":"case.create","id":"case-id","revision":1});
        for body in [commerce, purchasing, billing, support] {
            assert!(matching_receipt(&body, "a"));
            assert!(!matching_receipt(&body, "b"));
        }
        assert!(!matching_receipt(
            &json!({"requestId":"a","receipt":{"requestId":"b","id":"id"}}),
            "a"
        ));
        assert!(!matching_receipt(
            &json!({"requestId":"a","receipt":{}}),
            "a"
        ));
        assert!(!matching_receipt(
            &json!({"requestId":"a","status":"unknown"}),
            "a"
        ));
    }
}
