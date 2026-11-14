//! Native selection and semantic review of the exact tenant AI sharing policy.
use super::*;

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
fn flip(values: RwSignal<Vec<String>>, id: &str) {
    values.update_if_alive(|values| {
        if values.iter().any(|v| v == id) {
            values.retain(|v| v != id);
        } else {
            values.push(id.to_string());
        }
        values.sort();
    });
}
#[component]
pub(super) fn SharingSettings(
    config: Value,
    change: RwSignal<Option<Value>>,
    on_edit: Callback<()>,
    on_preview: Callback<()>,
) -> impl IntoView {
    let saved = &config["config"]["aiDisclosure"];
    let enabled = RwSignal::new(saved.is_object());
    let areas = RwSignal::new(strings(&saved["modules"]));
    let classes = RwSignal::new(strings(&saved["dataClasses"]));
    let companies = RwSignal::new(strings(&saved["companies"]));
    let workspace = RwSignal::new(saved["includeWorkspaceRecords"] == true);
    let company_rows = RwSignal::new(Vec::<Value>::new());
    let query = RwSignal::new(String::new());
    let cursor = RwSignal::new(None::<String>);
    let notice = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    let binding = StoredValue::new(
        config["aiProvider"]["binding"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    );
    let can_apply = config["grants"]["apply"] == true;
    let provider_ready = config["aiProvider"]["provider"].is_object();
    let provider = config["aiProvider"]["provider"].clone();
    let matches = config["aiProvider"]["approvedForCurrentProvider"] == true;
    let update = move || {
        let value = if enabled.get_untracked() {
            json!({"kind":"replace","policy":{"providerBinding":binding.get_value(),"modules":areas.get_untracked(),"dataClasses":classes.get_untracked(),"companies":companies.get_untracked(),"includeWorkspaceRecords":workspace.get_untracked()}})
        } else {
            json!({"kind":"revoke"})
        };
        change.set_if_alive(Some(value));
        on_edit.run(());
    };
    let load = move |next: Option<String>| {
        let Some(held) = session::held() else { return };
        sequence.update_if_alive(|n| *n += 1);
        let version = sequence.get_untracked();
        loading.set_if_alive(true);
        let mut path = format!(
            "/erp/organizations/workspace?tenant={}&collection=companies&limit=100&search={}",
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&query.get_untracked())
        );
        if let Some(next) = next {
            path.push_str(&format!("&cursor={}", crate::contacts::query_encode(&next)));
        }
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed()
                || sequence.get_untracked() != version
                || session::held()
                    .is_none_or(|now| now.tenant != held.tenant || now.principal != held.principal)
            {
                return;
            }
            loading.set_if_alive(false);
            match answer {
                Ok(Answer { status: 200, body }) if body["companies"].is_array() => {
                    company_rows
                        .set_if_alive(body["companies"].as_array().cloned().unwrap_or_default());
                    cursor.set_if_alive(
                        body["collections"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|c| c["id"] == "companies")
                            .and_then(|c| c["nextCursor"].as_str().map(str::to_owned)),
                    );
                    notice.set_if_alive(String::new());
                }
                _ => {
                    company_rows.set_if_alive(Vec::new());
                    cursor.set_if_alive(None);
                    notice.set_if_alive("Company choices are unavailable under your current company access. You can still stop sharing or review workspace records.".into());
                }
            }
        });
    };
    Effect::new(move |_| load(None));
    let area_rows = config["disclosureAreas"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let category_rows = config["disclosureCategories"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    view! {
        <section class="settings-section" id="settings-ai" tabindex="-1" aria-labelledby="settings-ai-heading" data-testid="owner-ai-settings">
            <h2 id="settings-ai-heading">"AI data sharing"</h2>
            <p>"Choose what Ava may send to your configured AI provider. Current record permissions still apply to every user."</p>
            <p data-testid="owner-ai-provider">{if provider_ready {format!("Provider: {} · model {}",provider["provider"].as_str().unwrap_or("Configured provider"),provider["model"].as_str().unwrap_or("Configured model"))}else{"No AI planning provider is configured. Local work remains available.".into()}}</p>
            <p class="hint" data-testid="owner-ai-current-state">{if matches {"A sharing selection is approved for this provider. Current messages must also be selected for AI planning."}else{"AI planning needs a sharing selection reviewed for the current provider. Local work remains available."}}</p>
            <p class="hint">"Provider retention and data location depend on your provider agreement; Archava has not verified those terms. Stopping sharing prevents future requests and cannot recall data already sent."</p>
            <label class="sharing-option"><span>"Include this selection in the sharing review"</span><input type="checkbox" data-testid="owner-ai-enabled" disabled=!can_apply||!provider_ready prop:checked=move||enabled.get() on:change=move |_|{enabled.update_if_alive(|v|*v = !*v);update();}/></label>
            <fieldset class="settings-sharing-options" disabled=move||!can_apply||!provider_ready||!enabled.get()>
                <legend>"Business areas"</legend>
                {area_rows.into_iter().map(|row|{
                    let id=row["id"].as_str().unwrap_or_default().to_string();let selected=id.clone();let edit=id.clone();
                    view!{<label class="sharing-option"><span>{row["label"].as_str().unwrap_or("Business area").to_string()}</span><input type="checkbox" data-testid=format!("owner-ai-area-{id}") prop:checked=move||areas.get().contains(&selected) on:change=move |_|{flip(areas,&edit);update();}/></label>}
                }).collect_view()}
            </fieldset>
            <fieldset class="settings-sharing-options" disabled=move||!can_apply||!provider_ready||!enabled.get()>
                <legend>"Data categories"</legend>
                {category_rows.into_iter().map(|row|{
                    let id=row["id"].as_str().unwrap_or_default().to_string();let selected=id.clone();let edit=id.clone();
                    view!{<label class="sharing-option"><span>{row["label"].as_str().unwrap_or("Data category").to_string()}</span><input type="checkbox" data-testid=format!("owner-ai-class-{id}") prop:checked=move||classes.get().contains(&selected) on:change=move |_|{flip(classes,&edit);update();}/><span class="hint">{row["description"].as_str().unwrap_or_default().to_string()}</span></label>}
                }).collect_view()}
            </fieldset>
            <fieldset disabled=move||!can_apply||!provider_ready||!enabled.get()>
                <legend>"Company scope"</legend>
                <label class="sharing-option"><span>"Include workspace records without a company"</span><input type="checkbox" data-testid="owner-ai-workspace" prop:checked=move||workspace.get() on:change=move |_|{workspace.update_if_alive(|v|*v = !*v);update();}/></label>
                <p class="hint">"Company-owned records require their company to be selected separately. Financial records always require an explicitly selected company."</p>
                <label class="ops-field"><span>"Find a company"</span><input data-testid="owner-ai-company-search" prop:value=move||query.get() on:input=move|e|query.set_if_alive(event_target_value(&e))/></label>
                <button type="button" data-testid="owner-ai-company-find" disabled=move||loading.get() on:click=move |_|load(None)>"Find companies"</button>
                <button type="button" data-testid="owner-ai-company-next" disabled=move||loading.get()||cursor.get().is_none() on:click=move |_|load(cursor.get_untracked())>"Next companies"</button>
                <p class="hint" role="status">{move||notice.get()}</p>
                {move||company_rows.get().into_iter().map(|row|{
                    let id=row["id"].as_str().unwrap_or_default().to_string();let selected=id.clone();let edit=id.clone();
                    view!{<label class="sharing-option"><span>{format!("{} · {}",row["legalName"].as_str().or_else(||row["name"].as_str()).unwrap_or("Company"),row["code"].as_str().unwrap_or_default())}</span><input type="checkbox" data-testid=format!("owner-ai-company-{id}") prop:checked=move||companies.get().contains(&selected) on:change=move |_|{flip(companies,&edit);update();}/></label>}
                }).collect_view()}
                <p class="hint">{move||format!("{} companies selected. Search or page to review more choices.",companies.get().len())}</p>
                <button type="button" data-testid="owner-ai-companies-clear" on:click=move |_|{companies.set_if_alive(Vec::new());update();}>"Clear company selection"</button>
            </fieldset>
            <button type="button" class="btn" data-testid="owner-ai-preview" disabled=!can_apply||!provider_ready on:click=move |_|{update();on_preview.run(());}>"Review sharing selection"</button>
            <button type="button" data-testid="owner-ai-revoke" disabled=!can_apply on:click=move |_|{enabled.set_if_alive(false);update();on_preview.run(());}>"Review stopping AI sharing"</button>
            <p class="hint">"These controls prepare a review. Sharing changes only after you confirm the plan below."</p>
        </section>
    }
}

pub(super) fn review(policy: &Value, provider: &Value, metadata: &Value) -> AnyView {
    if policy.is_null() {
        return view!{<div data-testid="owner-ai-review"><h3>"AI sharing will stop"</h3><p>"Future planner requests use local work until a new sharing selection is reviewed and confirmed. Data already sent cannot be recalled."</p></div>}.into_any();
    }
    let labels = |key: &str, definitions: &str| {
        strings(&policy[key])
            .into_iter()
            .map(|id| {
                metadata[definitions]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| r["id"] == id)
                    .and_then(|r| r["label"].as_str())
                    .unwrap_or(&id)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let areas = labels("modules", "disclosureAreas");
    let classes = labels("dataClasses", "disclosureCategories");
    let companies = strings(&policy["companies"]);
    view!{<div data-testid="owner-ai-review">
        <h3>"Review AI data sharing"</h3>
        <p>{format!("Provider: {} · model {}",provider["provider"]["provider"].as_str().unwrap_or("Unavailable"),provider["provider"]["model"].as_str().unwrap_or("Unavailable"))}</p>
        <p>{format!("Business areas: {areas}")}</p><p>{format!("Data categories: {classes}")}</p>
        <p>{if policy["includeWorkspaceRecords"]==true {"Includes workspace records without a company."}else{"Workspace records without a company are withheld."}}</p>
        <p>{format!("{} explicitly selected companies. Company read and financial permissions still apply.",companies.len())}</p>
        <ul>{companies.iter().map(|id|{
            let row=metadata["disclosureCompanies"].as_array().into_iter().flatten().find(|r|r["id"]==*id);
            let label=row.filter(|r|r["available"]==true).map(|r|format!("{} · {}",r["name"].as_str().unwrap_or("Company"),r["code"].as_str().unwrap_or_default())).unwrap_or_else(||"Previously selected company; current access is unavailable".into());
            view!{<li>{label}</li>}
        }).collect_view()}</ul>
        <details><summary>"Selected company references"</summary><ul>{companies.into_iter().map(|id|view!{<li><code>{id}</code></li>}).collect_view()}</ul></details>
        <p class="hint">"The current messages you type are a separate category from automatically retrieved record fields. Historical conversation text and saved preferences are withheld. Known credential fields and supported credential patterns are filtered."</p>
    </div>}.into_any()
}
