//! Native reviewed imports and real action cards. Browser storage keeps only
//! actor-scoped recovery references; all reviewed business input stays on the server.
use super::*;
#[derive(Clone, Copy)]
struct Flow {
    kind: &'static str,
    plan: RwSignal<Value>,
    notice: RwSignal<String>,
    busy: RwSignal<bool>,
    uncertain: RwSignal<bool>,
    result: RwSignal<Value>,
    pending_ref: RwSignal<Option<String>>,
}
impl Flow {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            plan: RwSignal::new(Value::Null),
            notice: RwSignal::new(String::new()),
            busy: RwSignal::new(false),
            uncertain: RwSignal::new(false),
            result: RwSignal::new(Value::Null),
            pending_ref: RwSignal::new(None),
        }
    }
    fn id(self) -> String {
        text(
            &self.plan.get_untracked(),
            if self.kind == "import" {
                "batch_id"
            } else {
                "id"
            },
        )
    }
    fn recover(self) {
        let Some(held) = session::held() else {
            return;
        };
        let Some(marker) = pending::read_family(&held.tenant, self.kind, &held.principal) else {
            return;
        };
        self.pending_ref
            .set_if_alive(Some(marker.request_id.clone()));
        self.uncertain.set_if_alive(true);
        self.busy.set_if_alive(true);
        let path = format!(
            "/api/workflows/status?tenant={}&kind={}&id={}",
            crate::contacts::query_encode(&held.tenant),
            self.kind,
            marker.request_id
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer) if answer.status == 200 && answer.body["id"] == marker.request_id => {
                    if matches!(
                        answer.body["status"].as_str(),
                        Some("committed" | "cancelled")
                    ) && answer.body["receipt"].is_object()
                    {
                        self.result.set_if_alive(answer.body["receipt"].clone());
                        self.notice
                            .set_if_alive(if answer.body["status"] == "cancelled" {
                                "The original review was cancelled without a business effect."
                                    .into()
                            } else {
                                "The original action is committed. Its receipt was recovered."
                                    .into()
                            });
                        self.uncertain.set_if_alive(false);
                        self.pending_ref.set_if_alive(None);
                        pending::clear_by_request_id(
                            &held.tenant,
                            &held.principal,
                            &marker.request_id,
                        );
                    } else if answer.body["status"] == "pending"
                        && answer.body["plan_hash"].is_string()
                    {
                        let mut plan = answer.body["plan"].clone();
                        plan["plan_hash"] = answer.body["plan_hash"].clone();
                        plan[if self.kind == "import" {
                            "batch_id"
                        } else {
                            "id"
                        }] = json!(marker.request_id);
                        self.plan.set_if_alive(plan);
                        self.notice.set_if_alive("Review the original server-bound details below. Confirming retries the same action reference.".into());
                        self.uncertain.set_if_alive(false);
                        self.pending_ref
                            .set_if_alive(Some(marker.request_id.clone()));
                    }
                }
                Ok(answer) => self.notice.set_if_alive(message(&answer)),
                Err(_) => self.notice.set_if_alive(
                    "The original outcome is still unknown. Keep this reference and check again."
                        .into(),
                ),
            }
            self.busy.set_if_alive(false);
        });
    }
    fn cancel(self) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let mut id = self.id();
        let mut hash = text(&self.plan.get_untracked(), "plan_hash");
        if id.is_empty() {
            if let Some(marker) = pending::read_family(&held.tenant, self.kind, &held.principal) {
                id = marker.request_id;
                hash = marker
                    .domain
                    .strip_prefix(&format!("{}:", self.kind))
                    .unwrap_or_default()
                    .to_string();
            }
        }
        if id.is_empty() || hash.is_empty() {
            self.notice
                .set_if_alive("Choose or recover a reviewed plan first.".into());
            return;
        }
        self.busy.set_if_alive(true);
        let body =
            json!({"tenant":held.tenant,"id":id,"kind":self.kind,"plan_hash":hash}).to_string();
        spawn_local(async move {
            match session::api_fetch("POST","/api/workflows/cancel",Some(body)).await{
            Ok(answer)if answer.status==200&&answer.body["id"]==id&&answer.body["plan_hash"]==hash&&matches!(answer.body["status"].as_str(),Some("committed"|"cancelled"))=>{pending::clear_by_request_id(&held.tenant,&held.principal,&id);self.result.set_if_alive(answer.body["receipt"].clone());self.plan.set_if_alive(Value::Null);self.uncertain.set_if_alive(false);self.pending_ref.set_if_alive(None);self.notice.set_if_alive(if answer.body["status"]=="cancelled"{"Review cancelled. The server verified that no business effect committed.".into()}else{"The action had already committed; its receipt was recovered.".into()});},
            Ok(answer)=>self.notice.set_if_alive(message(&answer)),Err(_)=>self.notice.set_if_alive("Cancellation is unconfirmed. Check the original status before starting another.".into()),
        }
            self.busy.set_if_alive(false);
        });
    }
    fn commit(self) {
        if self.busy.get_untracked() || self.uncertain.get_untracked() {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let plan = self.plan.get_untracked();
        let id = self.id();
        let hash = text(&plan, "plan_hash");
        if id.is_empty() || hash.is_empty() {
            return;
        }
        let marker = PendingMarker::with_domain(
            id.clone(),
            held.tenant.clone(),
            format!("{}:{hash}", self.kind),
            held.principal.clone(),
        );
        if let Err(error) = pending::write(&marker) {
            self.notice
                .set_if_alive(format!("Recovery reference could not be saved: {error}"));
            return;
        }
        let (path, body) = if self.kind == "import" {
            (
                "/api/migration/commit".into(),
                json!({"tenant":held.tenant,"batch_id":id,"plan_hash":hash,"confirmed":true}),
            )
        } else {
            (
                format!("/api/v1/feed/action-cards/{id}/execute"),
                json!({"tenant":held.tenant,"plan_hash":hash,"confirmed":true}),
            )
        };
        self.pending_ref.set_if_alive(Some(id.clone()));
        self.busy.set_if_alive(true);
        spawn_local(async move {
            match session::api_fetch("POST", &path, Some(body.to_string())).await {
                Ok(answer)
                    if answer.status == 200
                        && answer.body[if self.kind == "import" {
                            "batch_id"
                        } else {
                            "card_id"
                        }] == id
                        && answer.body["plan_hash"] == hash
                        && matches!(
                            answer.body["status"].as_str(),
                            Some("committed" | "committed_atomic")
                        ) =>
                {
                    self.result.set_if_alive(answer.body);
                    self.plan.set_if_alive(Value::Null);
                    self.pending_ref.set_if_alive(None);
                    self.notice
                        .set_if_alive("Committed. The server receipt confirms this action.".into());
                    pending::clear_by_request_id(&held.tenant, &held.principal, &id);
                }
                Ok(answer) => {
                    self.uncertain.set_if_alive(true);
                    self.notice.set_if_alive(format!(
                        "{}. Check the original action status before starting another.",
                        message(&answer)
                    ));
                }
                Err(_) => {
                    self.uncertain.set_if_alive(true);
                    self.notice.set_if_alive("Outcome unconfirmed. Check the original action status before starting another.".into());
                }
            }
            self.busy.set_if_alive(false);
        });
    }
}
fn csv(raw: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let delimiter = if raw.lines().next().is_some_and(|l| l.contains('\t')) {
        '\t'
    } else {
        ','
    };
    let mut quoted = false;
    let mut field = String::new();
    let mut row = Vec::new();
    let mut rows = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => {
                row.push(field.trim().to_string());
                field.clear();
            }
            '\n' if !quoted => {
                row.push(field.trim().to_string());
                field.clear();
                if row.iter().any(|c| !c.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            '\r' if !quoted => {}
            c => field.push(c),
        }
        if rows.len() > 501 || field.len() > 8000 {
            return Err("Import up to 500 rows with bounded cells.".into());
        }
    }
    if quoted {
        return Err("A quoted CSV field is incomplete.".into());
    }
    row.push(field.trim().to_string());
    if row.iter().any(|c| !c.is_empty()) {
        rows.push(row);
    }
    if rows.len() < 2 {
        return Err("Include a header row and at least one record.".into());
    }
    let columns = rows.remove(0);
    Ok((columns, rows))
}
#[component]
fn Review(flow: Flow) -> impl IntoView {
    view! {<div aria-live="polite" data-testid="workflow-notice">{move||flow.notice.get()}</div><button class="btn secondary" type="button" data-testid="workflow-cancel" disabled=move||flow.busy.get() on:click=move|_|flow.cancel()>"Cancel reviewed action"</button><button class="btn secondary" type="button" data-testid="workflow-recover" disabled=move||flow.busy.get() on:click=move|_|flow.recover()>"Check original action status"</button>
        {move||flow.plan.get().is_object().then(||view!{<section class="operator-review" data-testid="workflow-review"><h3>"Review before confirming"</h3><p>"Every displayed record belongs to the server-issued plan. Confirmation applies this reference once."</p>{operator::details(flow.plan.get())}<button class="btn" type="button" data-testid="workflow-confirm" disabled=move||flow.busy.get()||flow.uncertain.get() on:click=move|_|flow.commit()>"Confirm and apply"</button></section>})}
        {move||flow.result.get().is_object().then(||view!{<section data-testid="workflow-receipt"><h3>"Action result"</h3>{operator::details(flow.result.get())}</section>})}
    }
}
#[component]
pub(super) fn Import() -> impl IntoView {
    let flow = Flow::new("import");
    let kind = RwSignal::new("crm_contacts".to_string());
    let content = RwSignal::new(String::new());
    Effect::new(move |_| {
        if session::held().is_some() {
            flow.recover();
        }
    });
    let review = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if flow.busy.get_untracked()
            || flow.uncertain.get_untracked()
            || flow.pending_ref.get_untracked().is_some()
        {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let (columns, rows) = match csv(&content.get_untracked()) {
            Ok(v) => v,
            Err(e) => {
                flow.notice.set_if_alive(e);
                return;
            }
        };
        flow.busy.set_if_alive(true);
        let body=json!({"tenant":held.tenant,"entity_kind":kind.get_untracked(),"table":{"columns":columns,"rows":rows},"custom_overrides":null}).to_string();
        spawn_local(async move {
            match session::api_fetch("POST", "/api/migration/reconcile", Some(body)).await {
                Ok(answer) if answer.status == 200 && answer.body["plan_hash"].is_string() => {
                    flow.plan.set_if_alive(answer.body);
                    flow.result.set_if_alive(Value::Null);
                    flow.notice.set_if_alive("Review all records, whole IDR amounts and any omitted fields before confirming.".into());
                }
                Ok(answer) => flow.notice.set_if_alive(message(&answer)),
                Err(_) => flow.notice.set_if_alive(
                    "The preview could not be loaded. No business records were changed.".into(),
                ),
            }
            flow.busy.set_if_alive(false);
        });
    };
    view! {<section data-testid="import-workspace"><h2>"Import business data"</h2><p>"Paste CSV or tab-separated rows, including column headers. Preview up to 500 rows before committing the entire batch."</p><form class="ops-form" on:submit=review><label class="ops-field"><span>"Record type"</span><select data-testid="import-kind" on:change=move|e|{kind.set_if_alive(event_target_value(&e));flow.plan.set_if_alive(Value::Null);} disabled=move||flow.busy.get()||flow.uncertain.get()||flow.pending_ref.get().is_some()><option value="crm_contacts">"CRM contacts · name, email, company"</option><option value="catalog_products">"Products · sku, title, price"</option><option value="inventory_stock">"Opening inventory · sku, quantity, unit_cost_minor"</option></select></label><label class="ops-field"><span>"CSV data"</span><textarea data-testid="import-csv" maxlength="1000000" rows="10" required prop:value=move||content.get() on:input=move|e|{content.set_if_alive(event_target_value(&e));flow.plan.set_if_alive(Value::Null);} disabled=move||flow.busy.get()||flow.uncertain.get()||flow.pending_ref.get().is_some()></textarea></label><p class="hint">"Prices and acquisition costs are whole Rupiah. Opening inventory requires a zero stock balance. Fields absent from the native schema appear under Omitted fields for review."</p><button class="btn" type="submit" data-testid="import-preview" disabled=move||flow.busy.get()||flow.uncertain.get()||flow.pending_ref.get().is_some()>"Preview import"</button></form><Review flow/></section>}
}
#[component]
pub(super) fn Feed() -> impl IntoView {
    let flow = Flow::new("feed");
    let cards = RwSignal::new(Vec::<Value>::new());
    let refresh = move || {
        let Some(held) = session::held() else {
            return;
        };
        flow.busy.set_if_alive(true);
        let path = format!(
            "/api/v1/feed/action-cards?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer) if answer.status == 200 => {
                    cards.set_if_alive(
                        answer.body["feed"]["cards"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default(),
                    );
                    flow.notice.set_if_alive("Cards reflect current accessible stock and supplier offers. Existing purchase drafts suppress another reorder.".into());
                }
                Ok(answer) => flow.notice.set_if_alive(message(&answer)),
                Err(_) => flow
                    .notice
                    .set_if_alive("Action cards could not be loaded.".into()),
            }
            flow.busy.set_if_alive(false);
        });
    };
    Effect::new(move |_| {
        if session::held().is_some() {
            refresh();
            flow.recover();
        }
    });
    view! {<section data-testid="feed-workspace"><h2>"Suggested business actions"</h2><p>"Review a suggestion before creating a purchase draft. Confirming the purchase remains a separate action in Purchasing."</p><button class="btn secondary" type="button" data-testid="feed-refresh" disabled=move||flow.busy.get()||flow.uncertain.get()||flow.pending_ref.get().is_some() on:click=move|_|refresh()>"Refresh suggestions"</button><ul class="operator-records">{move||cards.get().into_iter().map(|card|view!{<li><h3>{text(&card,"title")}</h3><p>{format!("{} · {} units · estimated unit cost {} IDR",text(&card,"supplierName"),integer(&card,"quantity"),integer(&card,"estimatedUnitCost"))}</p><button type="button" class="btn" data-testid="feed-review" disabled=move||flow.busy.get()||flow.uncertain.get()||flow.pending_ref.get().is_some() on:click=move|_|flow.plan.set_if_alive(card.clone())>"Review purchase draft"</button></li>}).collect::<Vec<_>>()}</ul><Review flow/></section>}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_preserves_quoted_commas_lines_and_rejects_unclosed_quotes() {
        let (c, r) =
            csv("name,email\n\"Doe, Jane\",jane@example.test\n\"Multi\nLine\",multi@example.test")
                .unwrap();
        assert_eq!(c, vec!["name", "email"]);
        assert_eq!(r[0][0], "Doe, Jane");
        assert_eq!(r[1][0], "Multi\nLine");
        assert!(csv("name,email\n\"broken").is_err());
    }
}
