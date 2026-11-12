//! Native operator workspace. Views share receipt/recovery mechanics, not business authority.
use crate::alive::AliveSignal;
mod accounting;
pub(crate) use accounting::agent_step_ready;
pub(crate) use accounting::agent_step_review;
mod billing;
mod commerce;
mod companies;
pub(crate) mod company_context;
mod customer;
mod mail;
mod missions;
mod operator;
mod organizations;
mod parties;
mod party_selection;
pub(crate) use operator::AgentActionEditor;
mod payments;
mod purchasing;
mod supplier_documents;
mod suppliers;
mod support;
mod voice;
mod workflows;
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
    Other(&'static str),
}
impl Domain {
    fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "commerce" => Self::Commerce,
            "purchasing" => Self::Purchasing,
            "billing" => Self::Billing,
            "support" => Self::Support,
            "crm" => return None,
            _ => Self::Other(archava_domain::operator::module(slug)?["id"].as_str()?),
        })
    }
    fn status_key(self) -> &'static str {
        archava_domain::operator::module(self.slug())
            .and_then(|m| m["statusKey"].as_str())
            .unwrap_or("requestId")
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Commerce => "commerce",
            Self::Purchasing => "purchasing",
            Self::Billing => "billing",
            Self::Support => "support",
            Self::Other(id) => id,
        }
    }
    fn collection(self) -> &'static str {
        match self {
            Self::Commerce | Self::Purchasing | Self::Billing => "workspace",
            Self::Support => "cases",
            Self::Other(_) => "workspace",
        }
    }
    fn expected(self) -> &'static str {
        match self {
            Self::Commerce => "products",
            Self::Purchasing => "orders",
            Self::Billing => "invoices",
            Self::Support => "items",
            Self::Other(_) => "collections",
        }
    }
    fn envelope(self, tenant: &str, key: &str, mut command: Value) -> Result<Value, String> {
        if let Self::Other(id) = self {
            let action = command["operation"]
                .as_str()
                .or_else(|| command["type"].as_str())
                .or_else(|| command["action"].as_str())
                .ok_or("Choose an action")?;
            return archava_domain::operator::envelope(id, tenant, key, action, &command);
        }
        Ok(match self {
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
            Self::Other(_) => unreachable!("handled above"),
        })
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
    pub company: RwSignal<Option<Value>>,
    pub company_key: Memo<String>,
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
    } else if (body["commandId"].is_string() || body["requestId"].is_string())
        && resource_id(body).is_some()
    {
        Some(body)
    } else {
        None
    }
}
fn request_reference(body: &Value) -> Option<&str> {
    body["requestId"]
        .as_str()
        .or_else(|| body["commandId"].as_str())
        .or_else(|| body["request_id"].as_str())
        .or_else(|| {
            receipt(body).and_then(|r| r["requestId"].as_str().or_else(|| r["commandId"].as_str()))
        })
}
fn matching_receipt(body: &Value, key: &str) -> bool {
    let Some(r) = receipt(body) else { return false };
    let nested = r["requestId"].as_str().or_else(|| r["commandId"].as_str());
    request_reference(body) == Some(key)
        && nested.is_none_or(|id| id == key)
        && resource_id(r).is_some()
}
const RESOURCE_KEYS: &[&str] = &[
    "id",
    "invoiceId",
    "partyId",
    "companyId",
    "organizationId",
    "proposalId",
    "projectId",
    "ticketId",
    "resourceId",
    "reservationId",
    "propertyId",
    "roomTypeId",
    "roomId",
    "assetId",
    "subscriptionId",
    "entitlementId",
    "sessionId",
    "orderId",
    "jobId",
    "enrollmentId",
    "changeRequestId",
    "affiliationId",
    "usageEventId",
    "rosterId",
    "checkInId",
    "lineId",
    "processId",
    "findingId",
    "custodyId",
];
fn resource_id(receipt: &Value) -> Option<&str> {
    if receipt["resourceKind"].as_str().is_some_and(|k| {
        matches!(
            k,
            "book"
                | "account"
                | "period"
                | "journal"
                | "invoice_policy"
                | "purchase_policy"
                | "stock_policy"
                | "bank_account"
                | "bank_statement"
                | "bank_allocation"
                | "bank_unmatch"
                | "bank_policy"
                | "bank_reconciliation"
                | "document_terms"
        )
    }) {
        return receipt["resourceId"].as_str().filter(|v| !v.is_empty());
    }
    RESOURCE_KEYS
        .iter()
        .find_map(|k| receipt[*k].as_str().filter(|s| !s.is_empty()))
}
fn matches_command(body: &Value, command: &Value) -> bool {
    let Some(r) = receipt(body) else {
        return false;
    };
    if matches!(
        command["type"].as_str(),
        Some("invoice_terms.set" | "supplier_terms.set")
    ) {
        let terms = &r["documentTerms"];
        let kind = if command["type"] == "invoice_terms.set" {
            "receivable"
        } else {
            "payable"
        };
        let revision = command["expectedRevision"]
            .as_i64()
            .and_then(|v| v.checked_add(1));
        if r["resourceKind"] != "document_terms"
            || r["resourceId"].as_str().is_none_or(str::is_empty)
            || command["sourceId"].as_str().is_none()
            || command["sourceId"] != r["sourceId"]
            || terms["sourceId"] != command["sourceId"]
            || terms["documentKind"] != kind
            || terms["documentId"].as_str().is_none_or(str::is_empty)
            || command["expectedDocumentRevision"]
                .as_i64()
                .is_none_or(|v| v <= 0)
            || terms["documentRevision"] != command["expectedDocumentRevision"]
            || revision.is_none()
            || r["revision"].as_i64() != revision
            || terms["termsRevision"].as_i64() != revision
            || terms["condition"] != command["condition"]
            || terms["effectiveFrom"] != command["effectiveFrom"]
        {
            return false;
        }
    }
    if matches!(
        command["type"].as_str(),
        Some("invoice_source.draft" | "purchase_source.draft" | "stock_source.draft")
    ) && (command["sourceId"].as_str().is_none()
        || command["sourceId"] != r["sourceId"]
        || r["resourceKind"] != "journal")
    {
        return false;
    }
    if matches!(
        command["type"].as_str(),
        Some("payable_payment.draft" | "payable_return.draft" | "clearing_settlement.draft")
    ) && (r["resourceKind"] != "journal"
        || command["bankRowId"].as_str().is_none()
        || command["bankRowId"] != r["bankRowId"]
        || (matches!(
            command["type"].as_str(),
            Some("payable_payment.draft" | "clearing_settlement.draft")
        ) && (command["sourceId"].as_str().is_none() || command["sourceId"] != r["sourceId"]))
        || (command["type"] == "payable_return.draft"
            && (command["originalPaymentId"].as_str().is_none()
                || command["originalPaymentId"] != r["originalPayablePaymentId"])))
    {
        return false;
    }
    if command["type"] == "bank_reconciliation.confirm"
        && (r["resourceKind"] != "bank_reconciliation"
            || command["expectedCheckHash"].as_str().is_none()
            || r["bankCheckHash"] != command["expectedCheckHash"])
    {
        return false;
    }
    if let Some(expected) = command["id"].as_str() {
        let reversal = match command["type"].as_str() {
            Some("journal.reverse") => Some(
                r["type"] == "journal.reverse"
                    && r["originalJournalId"].as_str() == Some(expected)
                    && r["resourceKind"] == "journal",
            ),
            Some("bank_allocation.reverse") => Some(
                r["type"] == "bank_allocation.reverse"
                    && r["originalBankAllocationId"].as_str() == Some(expected)
                    && r["resourceKind"] == "bank_unmatch",
            ),
            _ => None,
        };
        if reversal == Some(false) {
            return false;
        }
        if reversal.is_none()
            && !RESOURCE_KEYS
                .iter()
                .any(|k| r[*k].as_str() == Some(expected))
        {
            return false;
        }
    }
    for key in RESOURCE_KEYS.iter().filter(|k| **k != "id") {
        if let (Some(a), Some(b)) = (command[*key].as_str(), r[*key].as_str()) {
            if a != b {
                return false;
            }
        }
    }
    let expected = command["operation"]
        .as_str()
        .or_else(|| command["type"].as_str())
        .or_else(|| command["action"].as_str());
    for key in ["operation", "type", "action"] {
        if let Some(actual) = r[key].as_str() {
            if expected.is_some_and(|v| v != actual) {
                return false;
            }
        }
    }
    true
}
fn record_id(body: &Value) -> String {
    receipt(body)
        .and_then(resource_id)
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
        Self::with_company(domain, RwSignal::new(None))
    }
    pub fn with_company(domain: Domain, company: RwSignal<Option<Value>>) -> Self {
        let company_key =
            Memo::new(move |_| company.get().map(|c| text(&c, "id")).unwrap_or_default());
        let work = Self {
            company,
            company_key,
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
            work.company.track();
            work.data.set_if_alive(json!({}));
            work.ready.set_if_alive(false);
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
        let path = if let Some(company) = self.company.get_untracked() {
            format!(
                "{path}&companyId={}",
                crate::contacts::query_encode(&text(&company, "id"))
            )
        } else {
            path
        };
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
                                self.data.set_if_alive(json!({}));
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
                    self.data.set_if_alive(json!({}));
                    self.ready.set_if_alive(false);
                    let notice = if answer.status == 403 && self.company.get_untracked().is_some() {
                        "Access to the selected company or workspace was refused. Check your permissions or choose an accessible company.".to_string()
                    } else {
                        message(&answer)
                    };
                    self.notice.set_if_alive(notice);
                }
                Err(error) => {
                    self.data.set_if_alive(json!({}));
                    self.ready.set_if_alive(false);
                    self.notice
                        .set_if_alive(format!("Workspace unavailable: {error}"));
                }
            }
        });
    }
    pub fn new_company_fields(self, command: &mut Value, operation: &str) -> Result<(), String> {
        if let Some(company) = self.company.get_untracked() {
            if company["active"] != true {
                return Err("This company is inactive. Existing documents remain available; select an active company for new business.".into());
            }
            command["operation"] = json!(operation);
            command["companyId"] = company["id"].clone();
            command["expectedCompanyRevision"] = company["revision"].clone();
        }
        Ok(())
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
        let body = match self.domain.envelope(&held.tenant, &key, command.clone()) {
            Ok(v) => v.to_string(),
            Err(reason) => {
                pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                self.fail(reason);
                return;
            }
        };
        self.action.set_if_alive(ActionState::Busy);
        spawn_local(async move {
            let mut response = session::api_fetch(
                "POST",
                &format!("/erp/{}/commands", self.domain.slug()),
                Some(body),
            )
            .await;
            // Accommodation preserves its bare receipt contract. Bind that
            // receipt to this exact request through the authoritative status.
            if self.domain == Domain::Other("accommodation") {
                if let Ok(answer) = &response {
                    if answer.status == 200 {
                        let path = format!(
                            "/erp/accommodation/commands/status?tenant={}&requestId={}",
                            crate::contacts::query_encode(&held.tenant),
                            key
                        );
                        if let Ok(status) = session::api_fetch("GET", &path, None).await {
                            if status.status == 200
                                && matching_receipt(&status.body, &key)
                                && status.body["receipt"] == answer.body
                                && matches_command(&status.body, &command)
                            {
                                response = Ok(status);
                            }
                        }
                    }
                }
            }
            match response{
                Ok(answer)if answer.status==200&&matching_receipt(&answer.body,&key)&&matches_command(&answer.body,&command)=>{
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
            "/erp/{}/commands/status?tenant={}&{}={}",
            self.domain.slug(),
            crate::contacts::query_encode(&held.tenant),
            self.domain.status_key(),
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
    view! {<label class="ops-field"><span>{label}</span><select data-testid=id prop:value=move||value.get() on:change=move|e|value.set_if_alive(event_target_value(&e)) required><option value="" prop:selected=move||value.get().is_empty()>"Choose…"</option>{move||options.get().into_iter().map(|(id,label)|{let selected=id.clone();view!{<option value=id prop:selected=move||value.get()==selected>{label}</option>}}).collect::<Vec<_>>()}</select></label>}
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
    view! {<Show when=move||matches!(work.domain,Domain::Commerce|Domain::Purchasing|Domain::Billing|Domain::Other("accounting"))><company_context::CompanyPicker selected=work.company blocked=Signal::derive(move||work.blocked()) on_change=Callback::new(move|()|work.new_intent())/></Show><header class="ops-section-heading"><div><h2>{title}</h2><p class="hint">"Search and refresh records available to your workspace role. Up to 100 recent matches are shown."</p></div><form class="ops-search" on:submit=move|e|{e.prevent_default();work.refresh();}><input aria-label="Search records" placeholder="Search records…" data-testid="ops-search" prop:value=move||work.search.get() on:input=move|e|work.search.set_if_alive(event_target_value(&e))/><button class="btn secondary" type="submit" data-testid="ops-refresh">"Refresh"</button></form></header>}
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
    // Tabs with a dedicated view, then one tab per module the generic operator
    // workspace serves. The second group comes from the module registry
    // (`config/modules.v1.json`), so a new module's tab is its descriptor.
    let mut tabs = vec![
        ("catalog", "Catalog"),
        ("sales", "Sales orders"),
        ("purchasing", "Purchasing"),
        ("invoices", "Invoices"),
        ("accounting", "Accounting"),
        ("support", "Support"),
        ("missions", "Missions"),
        ("feed", "Suggestions"),
        ("import", "Import data"),
        ("voice", "Voice"),
    ];
    tabs.extend(
        archava_domain::modules::all()
            .iter()
            .filter_map(|module| module.tab.map(|label| (module.id, label))),
    );
    view! {<section class="operations-page" data-testid="operations-workspace"><header class="page-heading"><p class="eyebrow">"BUSINESS WORKSPACE"</p><h1>"Operations"</h1><p>"Manage the records behind your daily work."</p></header><nav class="ops-tabs" aria-label="Operations modules">{tabs.into_iter().map(|(id,label)|{let navigate=navigate.clone();view!{<button type="button" class:active=move||active.get()==id data-testid=format!("ops-tab-{id}") on:click=move|_|navigate(&format!("/app/erp?module={id}"),Default::default())>{label}</button>}}).collect::<Vec<_>>()}</nav>
        {move||match active.get().as_str(){
            "voice"=>view!{<voice::Voice/>}.into_any(),"feed"=>view!{<workflows::Feed/>}.into_any(),"import"=>view!{<workflows::Import/>}.into_any(),
            "catalog"|"commerce"=>view!{<commerce::Catalog/>}.into_any(),
            "sales"=>view!{<commerce::Sales/>}.into_any(),"purchasing"=>view!{<purchasing::Purchasing/>}.into_any(),"invoices"|"billing"=>view!{<billing::Invoices/>}.into_any(),"support"=>view!{<support::Support/>}.into_any(),"missions"=>view!{<missions::Missions/>}.into_any(),
            slug=>match Domain::from_slug(slug){Some(domain)=>view!{<operator::Workspace domain/>}.into_any(),None=>view!{<p class="refusal">"This module is unavailable. Choose an installed module above."</p>}.into_any()},
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
    fn document_term_acknowledgement_binds_the_exact_reviewed_condition_and_both_versions() {
        for op in ["invoice_terms.set", "supplier_terms.set"] {
            let kind = if op == "invoice_terms.set" {
                "receivable"
            } else {
                "payable"
            };
            let command = json!({"type":op,"sourceId":"source","expectedDocumentRevision":3,"expectedRevision":1,"effectiveFrom":"2026-11-12","condition":{"kind":"unknown"}});
            let response = json!({"requestId":"request","receipt":{"requestId":"request","type":op,"resourceKind":"document_terms","resourceId":"terms","sourceId":"source","bookId":"book","companyId":"company","revision":2,"documentTerms":{"documentKind":kind,"sourceId":"source","documentId":"document","documentRevision":3,"termsRevision":2,"effectiveFrom":"2026-11-12","condition":{"kind":"unknown"}}}});
            assert!(matches_command(&response, &command));
            assert_eq!(record_id(&response), "terms");
            for (field, value) in [
                ("documentKind", json!("wrong")),
                ("sourceId", json!("another")),
                ("documentRevision", json!(2)),
                ("termsRevision", json!(3)),
                ("effectiveFrom", json!("2026-11-13")),
                ("condition", json!({"kind":"due_on","date":"2026-11-30"})),
            ] {
                let mut bad = response.clone();
                bad["receipt"]["documentTerms"][field] = value;
                assert!(!matches_command(&bad, &command), "{op} {field}");
            }
            let mut bad = response.clone();
            bad["receipt"]["resourceKind"] = json!("journal");
            assert!(!matches_command(&bad, &command));
        }
    }
    #[test]
    fn payable_acknowledgement_binds_both_the_bill_or_original_payment_and_bank_evidence() {
        for (operation, field, receipt_field) in [
            ("payable_payment.draft", "sourceId", "sourceId"),
            (
                "payable_return.draft",
                "originalPaymentId",
                "originalPayablePaymentId",
            ),
        ] {
            let command = json!({"type":operation,"bankRowId":"row",field:"original"});
            let response = json!({"requestId":"request","receipt":{"requestId":"request","type":operation,"resourceKind":"journal","resourceId":"draft","bankRowId":"row",receipt_field:"original"}});
            assert!(matches_command(&response, &command));
            for changed in ["bankRowId", receipt_field] {
                let mut wrong = response.clone();
                wrong["receipt"][changed] = json!("different");
                assert!(!matches_command(&wrong, &command));
                wrong["receipt"].as_object_mut().unwrap().remove(changed);
                assert!(!matches_command(&wrong, &command));
            }
        }
    }
    fn bank_unmatch_receipt() -> Value {
        json!({"requestId":"request","receipt":{"requestId":"request","type":"bank_allocation.reverse","resourceKind":"bank_unmatch","resourceId":"new-unmatch","originalBankAllocationId":"original-match","companyId":"company","bookId":"book","revision":1}})
    }
    #[test]
    fn bank_unmatch_acknowledgement_displays_the_new_resource() {
        assert_eq!(record_id(&bank_unmatch_receipt()), "new-unmatch");
    }
    #[test]
    fn bank_unmatch_acknowledgement_binds_the_original_match() {
        let response = bank_unmatch_receipt();
        let command =
            json!({"type":"bank_allocation.reverse","id":"original-match","expectedRevision":1});
        assert!(matching_receipt(&response, "request"));
        assert!(matches_command(&response, &command));
        let mut wrong = response.clone();
        wrong["receipt"]["originalBankAllocationId"] = json!("other-match");
        wrong["receipt"]["companyId"] = json!("original-match");
        assert!(!matches_command(&wrong, &command));
        wrong["receipt"]
            .as_object_mut()
            .unwrap()
            .remove("originalBankAllocationId");
        assert!(!matches_command(&wrong, &command));
        let mut wrong_kind = response;
        wrong_kind["receipt"]["resourceKind"] = json!("bank_allocation");
        assert!(!matches_command(&wrong_kind, &command));
    }
    #[test]
    fn source_and_bank_confirmation_receipts_cannot_acknowledge_different_reviewed_evidence() {
        for operation in [
            "invoice_source.draft",
            "purchase_source.draft",
            "stock_source.draft",
        ] {
            let command = json!({"type":operation,"sourceId":"reviewed-source"});
            let mut response = json!({"requestId":"request","receipt":{"requestId":"request","type":operation,"resourceKind":"journal","resourceId":"journal","sourceId":"reviewed-source"}});
            assert!(matches_command(&response, &command));
            response["receipt"]["sourceId"] = json!("another-source");
            assert!(!matches_command(&response, &command));
        }
        let command =
            json!({"type":"bank_reconciliation.confirm","expectedCheckHash":"a".repeat(64)});
        let mut response = json!({"requestId":"request","receipt":{"requestId":"request","type":"bank_reconciliation.confirm","resourceKind":"bank_reconciliation","resourceId":"review","bankCheckHash":"a".repeat(64)}});
        assert!(matches_command(&response, &command));
        response["receipt"]["bankCheckHash"] = json!("b".repeat(64));
        assert!(!matches_command(&response, &command));
        response["receipt"]
            .as_object_mut()
            .unwrap()
            .remove("bankCheckHash");
        assert!(!matches_command(&response, &command));
    }
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
        let source = json!({"requestId":"source-request","receipt":{"requestId":"source-request","type":"invoice_source.draft","resourceKind":"journal","resourceId":"draft","sourceId":"source-a","bookId":"book","companyId":"company","revision":1}});
        assert!(matches_command(
            &source,
            &json!({"type":"invoice_source.draft","sourceId":"source-a"})
        ));
        assert!(!matches_command(
            &source,
            &json!({"type":"invoice_source.draft","sourceId":"source-b"})
        ));
        let reversal = json!({"requestId":"a","receipt":{"requestId":"a","type":"journal.reverse","resourceKind":"journal","resourceId":"new-draft","companyId":"company","bookId":"book","originalJournalId":"original","revision":1}});
        assert_eq!(resource_id(&reversal["receipt"]), Some("new-draft"));
        assert!(matching_receipt(&reversal, "a"));
        assert!(matches_command(
            &reversal,
            &json!({"type":"journal.reverse","id":"original","expectedRevision":2})
        ));
        assert!(!matches_command(
            &reversal,
            &json!({"type":"journal.reverse","id":"other","expectedRevision":2})
        ));
        let commerce =
            json!({"requestId":"a","receipt":{"kind":"product","id":"product-id","revision":1}});
        let purchasing = json!({"requestId":"a","receipt":{"requestId":"a","operation":"order.create","id":"order-id","revision":1}});
        let billing = json!({"commandId":"a","action":"create_invoice","invoiceId":"invoice-id"});
        let support =
            json!({"requestId":"a","operation":"case.create","id":"case-id","revision":1});
        let proposal = json!({"commandId":"a","changeRequestId":"change-id"});
        let accommodation = json!({"requestId":"a","status":"committed","receipt":{"kind":"Property","id":"property-id","revision":1}});
        assert!(matches_command(
            &proposal,
            &json!({"changeRequestId":"change-id"})
        ));
        assert!(!matches_command(
            &proposal,
            &json!({"changeRequestId":"wrong"})
        ));
        assert!(!matching_receipt(&accommodation["receipt"], "a"));
        for body in [
            commerce,
            purchasing,
            billing,
            support,
            proposal,
            accommodation,
        ] {
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
