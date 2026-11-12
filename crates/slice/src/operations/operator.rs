//! Native forms share input descriptions; persistence and authorization remain
//! in each existing domain transaction. No arbitrary JSON editor is exposed.
use super::*;
use archava_domain::operator as contracts;

fn pointer(path: &[String]) -> String {
    format!("/{}", path.join("/"))
}
fn at(value: &Value, path: &[String]) -> Value {
    value
        .pointer(&pointer(path))
        .cloned()
        .unwrap_or(Value::Null)
}
fn put(value: &mut Value, path: &[String], next: Value) {
    if path.is_empty() {
        *value = next;
        return;
    }
    if let Ok(index) = path[0].parse::<usize>() {
        if !value.is_array() {
            *value = json!([]);
        }
        let a = value.as_array_mut().unwrap();
        while a.len() <= index {
            a.push(Value::Null);
        }
        put(&mut a[index], &path[1..], next);
    } else {
        if !value.is_object() {
            *value = json!({});
        }
        let child = value
            .as_object_mut()
            .unwrap()
            .entry(path[0].clone())
            .or_insert(Value::Null);
        put(child, &path[1..], next);
    }
}
fn caption(value: &Value) -> String {
    for key in [
        "displayName",
        "title",
        "name",
        "fullName",
        "planName",
        "subject",
        "code",
        "orderCode",
        "jobCode",
        "sessionCode",
        "studentRef",
        "summary",
    ] {
        if let Some(v) = value[key].as_str().filter(|v| !v.is_empty()) {
            return v.to_string();
        }
    }
    value["id"]
        .as_str()
        .map(|v| format!("Record {}", short(v)))
        .unwrap_or_else(|| "Record".into())
}
fn string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "Yes".into()
            } else {
                "No".into()
            }
        }
        _ => String::new(),
    }
}
fn label(key: &str) -> String {
    let mut text = String::new();
    for c in key.chars() {
        if c == '_' {
            text.push(' ');
        } else if c.is_uppercase() {
            text.push(' ');
            text.extend(c.to_lowercase());
        } else {
            text.push(c);
        }
    }
    if let Some(c) = text.chars().next() {
        text.replace_range(..c.len_utf8(), &c.to_uppercase().to_string());
    }
    text
}
fn reference(name: &str, action: &str, domain: &str) -> Option<(&'static str, &'static str)> {
    contracts::reference(name, action, domain)
}
fn fetch_reference(domain: &str, collection: &str, query: String, refs: RwSignal<Value>) {
    let Some(held) = session::held() else {
        return;
    };
    let mut path = match domain {
        "crm" => format!(
            "/contacts?tenant={}&search={}",
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&query)
        ),
        "support" if collection == "assignees" => format!(
            "/erp/support/assignees?tenant={}&limit=100",
            crate::contacts::query_encode(&held.tenant)
        ),
        "support" => format!(
            "/erp/support/cases?tenant={}&limit=100",
            crate::contacts::query_encode(&held.tenant)
        ),
        "accounting" => format!(
            "/erp/accounting/workspace?tenant={}&collection={}&search={}",
            crate::contacts::query_encode(&held.tenant),
            collection,
            crate::contacts::query_encode(&query)
        ),
        _ => format!(
            "/erp/{domain}/workspace?tenant={}&search={}",
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&query)
        ),
    };
    let company = text(&refs.get_untracked(), "__companyId");
    if !company.is_empty() && matches!(domain, "commerce" | "purchasing" | "billing" | "accounting")
    {
        path.push_str(&format!(
            "&companyId={}",
            crate::contacts::query_encode(&company)
        ));
    }
    let key = format!("{domain}/{collection}");
    let generation_key = format!("__fetch/{key}");
    let generation = integer(&refs.get_untracked(), &generation_key) + 1;
    refs.update_if_alive(|v| v[&generation_key] = json!(generation));
    let collection = collection.to_string();
    let domain = domain.to_string();
    spawn_local(async move {
        let answer = session::api_fetch("GET", &path, None).await;
        let Some(now) = refs.try_get_untracked() else {
            return;
        };
        if text(&now, "__companyId") != company || integer(&now, &generation_key) != generation {
            return;
        }
        if let Ok(answer) = answer {
            if answer.status == 200 {
                let rows = if domain == "crm" || domain == "support" && collection == "assignees" {
                    answer.body["items"].clone()
                } else {
                    answer.body[&collection].clone()
                };
                if rows.is_array() {
                    refs.update_if_alive(|v| v[&key] = rows);
                }
            } else {
                refs.update_if_alive(|v| v[&key] = json!([]));
            }
        } else {
            refs.update_if_alive(|v| v[&key] = json!([]));
        }
    });
}
fn optional_group_update(current: &Value, defaults: &Value, enabled: bool) -> Option<Value> {
    if enabled {
        current.is_null().then(|| defaults.clone())
    } else {
        (!current.is_null()).then_some(Value::Null)
    }
}
fn field_view(
    schema: Value,
    path: Vec<String>,
    values: RwSignal<Value>,
    refs: RwSignal<Value>,
    domain: &'static str,
    action: String,
) -> AnyView {
    if schema["automatic"] == "requestId" {
        return view! {<span/>}.into_any();
    }
    let name = schema["name"].as_str().unwrap_or_default().to_string();
    let title = schema["label"].as_str().unwrap_or("Value").to_string();
    let id = format!("operator-{domain}-{}", path.join("-"));
    let read_path = path.clone();
    let value = Signal::derive(move || at(&values.get(), &read_path));
    let write_path = path.clone();
    let setter = Callback::new(move |next: Value| {
        values.update_if_alive(|data| put(data, &write_path, next))
    });
    if domain == "accounting" && schema["automatic"] == "bankCheckHash" {
        return view! {<accounting::BankCheckField values refs set=setter/>}.into_any();
    }
    if domain == "accounting"
        && matches!(
            schema["automatic"].as_str(),
            Some("termsDocumentRevision" | "termsRevision")
        )
    {
        let property = if schema["automatic"] == "termsDocumentRevision" {
            "documentRevision"
        } else {
            "termsRevision"
        };
        let terms_action = action.clone();
        Effect::new(move |_| {
            let draft = values.get();
            let loaded = refs.get();
            let revision = accounting::terms_source(&loaded, &terms_action, &draft["sourceId"])
                .map(|r| r[property].clone())
                .unwrap_or(Value::Null);
            if at(&draft, &path) != revision {
                setter.run(revision);
            }
        });
        return view!{<p class="hint" data-testid=id>{move||value.get().as_i64().map(|v|format!("Reviewed version {v}.")).unwrap_or_else(||"Choose the original document to load its current version.".into())}</p>}.into_any();
    }
    if domain == "accounting"
        && matches!(action.as_str(), "invoice_terms.set" | "supplier_terms.set")
        && name == "condition"
    {
        return view! {<accounting::ConditionField value set=setter/>}.into_any();
    }
    if domain == "accounting" && schema["automatic"] == "payableDocumentRevision" {
        Effect::new(move |_| {
            let draft = values.get();
            let loaded = refs.get();
            let revision = loaded["accounting/payables"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["id"] == draft["sourceId"])
                .map(|r| r["documentRevision"].clone())
                .unwrap_or(Value::Null);
            if at(&draft, &path) != revision {
                setter.run(revision);
            }
        });
        return view!{<p class="hint" data-testid=id>{move||value.get().as_i64().map(|v|format!("Reviewed matched bill version {v}.")).unwrap_or_else(||"Choose a supplier bill to load its matched version.".into())}</p>}.into_any();
    }
    if domain == "accounting" && schema["automatic"] == "bankPolicyRevision" {
        Effect::new(move |_| {
            let draft = values.get();
            let loaded = refs.get();
            let revision = loaded["accounting/bank_accounts"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["id"] == draft["bankAccountId"])
                .and_then(|r| r["bankPolicyRevision"].as_i64())
                .map(|v| json!(v))
                .unwrap_or(Value::Null);
            if at(&draft, &path) != revision {
                setter.run(revision);
            }
        });
        return view!{<p class="hint" data-testid=id>{move||match value.get().as_i64(){Some(0)=>"First reconciliation basis for this bank.",Some(_)=>"Current reconciliation setup loaded for review.",None=>"Choose a bank account to load its current reconciliation setup."}}</p>}.into_any();
    }
    if domain == "accounting" && schema["minorAmount"] == true {
        let signed = schema["signedMinorAmount"] == true;
        return view! {<accounting::MoneyField label=title id value set=setter refs signed/>}
            .into_any();
    }
    if domain == "accounting"
        && matches!(
            schema["automatic"].as_str(),
            Some("invoicePolicyRevision" | "purchasePolicyRevision" | "stockPolicyRevision")
        )
    {
        let automatic = schema["automatic"].as_str().unwrap_or_default();
        let (book_key, revision_key) = match automatic {
            "purchasePolicyRevision" => ("__purchasePolicyBook", "__purchasePolicyRevision"),
            "stockPolicyRevision" => ("__stockPolicyBook", "__stockPolicyRevision"),
            _ => ("__invoicePolicyBook", "__invoicePolicyRevision"),
        };
        let invoice = automatic == "invoicePolicyRevision";
        Effect::new(move |_| {
            let loaded = refs.get();
            // A reviewed agent proposal carries its own optimistic version.
            // Only the native book workspace has authoritative policy metadata;
            // do not erase a proposal's version while its references are fetched.
            if loaded.get(book_key).is_none() {
                return;
            }
            let draft = values.get();
            let revision = if invoice {
                invoice_policy_revision(&loaded, &draft["bookId"])
            } else if draft["bookId"].is_string() && draft["bookId"] == loaded[book_key] {
                loaded[revision_key].clone()
            } else {
                Value::Null
            };
            if at(&draft, &path) != revision {
                setter.run(revision);
            }
        });
        return view! {<p class="hint" data-testid=id>{move || if refs.get().get(book_key).is_none() {
            "The proposed policy version is included in this review."
        } else { match value.get().as_u64() {
            Some(0) => "First posting policy for this book.",
            Some(_) => "Current posting policy loaded for review.",
            None => "Choose the current company book to load its posting policy.",
        }}}</p>}
        .into_any();
    }
    if schema["automatic"] == "revision" {
        return view! {<p class="hint">"Record version "{move||string(&value.get())}</p>}
            .into_any();
    }
    if schema["kind"] == "object" {
        if domain == "accounting" && action == "clearing_settlement.draft" && name == "feeAccount" {
            let fields = schema["fields"].as_array().cloned().unwrap_or_default();
            let defaults = contracts::field_defaults(&fields);
            let enabled = Memo::new(move |_| {
                values.get()["feeMinor"]
                    .as_str()
                    .is_some_and(|s| archava_domain::erp::accounting::minor(s).is_ok_and(|n| n > 0))
            });
            Effect::new(move |_| {
                if let Some(next) = optional_group_update(&value.get(), &defaults, enabled.get()) {
                    setter.run(next);
                }
            });
            return view!{<Show when=move||enabled.get() fallback=||view!{<p class="hint">"No fee expense account is needed for a zero fee."</p>}>
                <fieldset class="operator-group"><legend>{title.clone()}</legend>{fields.iter().cloned().map(|field|{let mut p=path.clone();p.push(field["name"].as_str().unwrap_or_default().into());field_view(field,p,values,refs,domain,action.clone())}).collect::<Vec<_>>()}</fieldset>
            </Show>}.into_any();
        }
        let fields = schema["fields"].as_array().cloned().unwrap_or_default();
        return view!{<fieldset class="operator-group"><legend>{title}</legend>{fields.into_iter().map(|field|{let mut p=path.clone();p.push(field["name"].as_str().unwrap_or_default().into());field_view(field,p,values,refs,domain,action.clone())}).collect::<Vec<_>>()}</fieldset>}.into_any();
    }
    if schema["kind"] == "array" {
        let maximum = schema["maxItems"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(100);
        let item = schema["items"].clone();
        let display_path = path.clone();
        let add_path = path.clone();
        let item_for_add = item.clone();
        let action_for_rows = action.clone();
        return view!{<fieldset class="operator-group" data-testid=id><legend>{title}</legend>
            <For each=move||value.get().as_array().map(|a|(0..a.len()).collect::<Vec<_>>()).unwrap_or_default() key=|i|*i children=move|index|{
                let mut p=display_path.clone();p.push(index.to_string());let field=item.clone();let remove_path=display_path.clone();
                view!{<div class="operator-array-item">{field_view(field,p,values,refs,domain,action_for_rows.clone())}<button type="button" class="btn secondary" on:click=move|_|values.update_if_alive(|data|{let mut a=at(data,&remove_path).as_array().cloned().unwrap_or_default();if index<a.len(){a.remove(index);}put(data,&remove_path,json!(a));})>"Remove item"</button></div>}
            }/>
            <button type="button" class="btn secondary" disabled=move||value.get().as_array().is_some_and(|v|v.len()>=maximum) on:click=move|_|values.update_if_alive(|data|{let mut a=at(data,&add_path).as_array().cloned().unwrap_or_default();if a.len()>=maximum{return;}let next=if item_for_add["kind"]=="object"{contracts::field_defaults(item_for_add["fields"].as_array().map(Vec::as_slice).unwrap_or_default())}else{Value::Null};a.push(next);put(data,&add_path,json!(a));})>"Add item"</button><p class="hint">{format!("Maximum {maximum} items.")}</p>
        </fieldset>}.into_any();
    }
    let optional = schema["optional"] == true;
    let nullable = schema["nullable"] == true;
    if schema["kind"] == "boolean" {
        if optional {
            return view!{<label class="ops-field"><span>{title}</span><select data-testid=id prop:value=move||match value.get(){Value::Bool(v)=>v.to_string(),_=>String::new()} on:change=move|e|{let v=event_target_value(&e);setter.run(if v.is_empty(){Value::Null}else{json!(v=="true")});}><option value="">"Keep unchanged"</option><option value="true">"Yes"</option><option value="false">"No"</option></select></label>}.into_any();
        }
        return view!{<label class="ops-check"><input data-testid=id type="checkbox" prop:checked=move||value.get().as_bool().unwrap_or(false) on:change=move|e|setter.run(json!(event_target_checked(&e)))/><span>{title}</span></label>}.into_any();
    }
    if let Some(options) = schema["options"]
        .as_array()
        .or_else(|| schema["enum"].as_array())
    {
        let options = options.clone();
        return view!{<label class="ops-field"><span>{title}</span><select data-testid=id prop:value=move||string(&value.get()) on:change=move|e|{let v=event_target_value(&e);setter.run(if v.is_empty(){Value::Null}else{json!(v)});} required=!optional><option value="">{if optional{"Keep unchanged"}else{"Choose…"}}</option>{options.into_iter().map(|v|{let s=v.as_str().unwrap_or_default().to_string();view!{<option value=s.clone()>{label(&s)}</option>}}).collect::<Vec<_>>()}</select></label>}.into_any();
    }
    if let Some((target, collection)) = reference(&name, &action, domain) {
        let key = format!("{target}/{collection}");
        let change_key = key.clone();
        let term = RwSignal::new(String::new());
        let title2 = title.clone();
        let target_name = name.clone();
        let parent_path = path[..path.len().saturating_sub(1)].to_vec();
        return view!{<div class="operator-reference"><label class="ops-field"><span>{title}</span><select data-testid=id prop:value=move||string(&value.get())
            on:focus=move|_|fetch_reference(target,collection,String::new(),refs)
            on:change=move|e|{let v=event_target_value(&e);if domain=="accounting" && target_name=="sourceId" && matches!(action.as_str(),"invoice_terms.set"|"supplier_terms.set") && v!=string(&value.get_untracked()) {values.update_if_alive(|draft|{draft["condition"]=json!({"kind":null});draft["effectiveFrom"]=Value::Null;});}let row=refs.get_untracked()[&change_key].as_array().and_then(|a|a.iter().find(|r|r["id"]==v)).cloned();setter.run(if v.is_empty(){Value::Null}else{json!(v)});if let Some(row)=row{values.update_if_alive(|draft|{bind_path_revisions(draft,&row,&target_name,&parent_path);if parent_path.is_empty()&&domain=="projects"&&target_name=="id"&&row["projectId"].is_string(){draft["projectId"]=row["projectId"].clone();}});if domain=="projects"{hydrate_project_revision(values);}}} required=!optional&&!nullable>
            <option value="">{if optional{"Keep unchanged"}else if nullable{"None"}else{"Choose a record…"}}</option>
            {move||{let current=string(&value.get());(!current.is_empty()).then(||view!{<option value=current>"Current linked record"</option>})}}
            {move||refs.get()[&key].as_array().cloned().unwrap_or_default().into_iter().map(|row|{let id=row["id"].as_str().or_else(||row["enrollmentId"].as_str()).or_else(||row["jobId"].as_str()).unwrap_or_default().to_string();view!{<option value=id>{caption(&row)}</option>}}).collect::<Vec<_>>()}
            </select></label><div class="ops-search"><input aria-label=format!("Find {title2}") placeholder="Search available records" prop:value=move||term.get() on:input=move|e|term.set_if_alive(event_target_value(&e))/><button type="button" class="btn secondary" on:click=move|_|fetch_reference(target,collection,term.get_untracked(),refs)>"Find"</button></div></div>}.into_any();
    }
    let kind = match schema["kind"].as_str() {
        Some("integer" | "number") => "number",
        _ => match schema["format"].as_str() {
            Some("date") => "date",
            Some("time") => "time",
            Some("datetime") => "datetime-local",
            _ => "text",
        },
    };
    let datetime = schema["format"] == "datetime";
    let max = schema["maxLength"].as_u64().unwrap_or(2000);
    let step = if schema["kind"] == "integer" {
        "1"
    } else {
        "any"
    };
    view!{<label class="ops-field"><span>{title}{optional.then_some(" (optional)")}</span><input data-testid=id type=kind step=step maxlength=max required=!optional&&!nullable
        prop:value=move||{let s=string(&value.get());if datetime{display_instant(&s)}else{s}}
        on:input=move|e|{let mut raw=event_target_value(&e);if datetime&&!raw.is_empty(){raw=local_instant(&raw);}setter.run(if raw.is_empty(){Value::Null}else{json!(raw)});}/>
        {nullable.then(||view!{<button type="button" class="btn secondary" on:click=move|_|setter.run(Value::Null)>"Clear value"</button>})}
    </label>}.into_any()
}
#[cfg(target_arch = "wasm32")]
fn display_instant(raw: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(raw));
    if date.get_time().is_finite() {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}",
            date.get_full_year(),
            date.get_month() + 1,
            date.get_date(),
            date.get_hours(),
            date.get_minutes()
        )
    } else {
        String::new()
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn display_instant(raw: &str) -> String {
    raw.chars().take(16).collect()
}
#[cfg(target_arch = "wasm32")]
fn local_instant(raw: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(raw));
    if date.get_time().is_finite() {
        date.to_iso_string()
            .as_string()
            .unwrap_or_else(|| raw.to_string())
    } else {
        raw.to_string()
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn local_instant(raw: &str) -> String {
    raw.to_string()
}

fn hydrate_project_revision(values: RwSignal<Value>) {
    let Some(held) = session::held() else {
        return;
    };
    let Some(id) = values.get_untracked()["projectId"]
        .as_str()
        .map(str::to_owned)
    else {
        return;
    };
    let path = format!(
        "/erp/projects/workspace?tenant={}&collection=projects&id={}",
        crate::contacts::query_encode(&held.tenant),
        crate::contacts::query_encode(&id)
    );
    spawn_local(async move {
        if let Ok(answer) = session::api_fetch("GET", &path, None).await {
            if answer.status == 200 && answer.body["projects"][0]["id"] == id {
                let revision = answer.body["projects"][0]["revision"].clone();
                values.update_if_alive(|draft| {
                    if draft["projectId"] == id && draft.get("projectRevision").is_some() {
                        draft["projectRevision"] = revision;
                    }
                });
            }
        }
    });
}
fn record_context(mut row: Value, collection: &str) -> Value {
    row["_collection"] = json!(collection);
    let key = match collection {
        "organizations" => "organizationId",
        "companies" => "companyId",
        "proposals" => "proposalId",
        "projects" => "projectId",
        "properties" => "propertyId",
        "room_types" => "roomTypeId",
        "rooms" => "roomId",
        "stays" => "stayId",
        "assets" => "assetId",
        "subscriptions" => "subscriptionId",
        "sessions" => "sessionId",
        "orders" => "orderId",
        "jobs" => "jobId",
        "enrollments" => "enrollmentId",
        "bank_accounts" => "bankAccountId",
        "bank_rows" => "bankRowId",
        "bank_policies" => "bankPolicyId",
        "periods" => "periodId",
        _ => "id",
    };
    if row["id"].is_string() {
        row[key] = row["id"].clone();
    }
    row
}
fn bind_revisions(values: &mut Value, row: &Value, target: &str) {
    let Some(revision) = row.get("revision") else {
        return;
    };
    for (field, reference) in [
        ("projectRevision", "projectId"),
        ("contactRevision", "contactId"),
        ("expectedOrderRevision", "orderId"),
        ("expectedAccountRevision", "accountId"),
        ("expectedJournalRevision", "journalId"),
        ("expectedJournalRevision", "originalPaymentId"),
        ("expectedPolicyRevision", "bankPolicyId"),
        ("expectedPeriodRevision", "periodId"),
        ("expectedCompanyRevision", "companyId"),
    ] {
        if target == reference && values.get(field).is_some() {
            values[field] = revision.clone();
        }
    }
    // The aggregate version belongs to its primary reference. Selecting an
    // offer, assignee, supplier or other linked record must not replace it.
    let primary = [
        "id",
        "changeRequestId",
        "proposalId",
        "invoiceId",
        "roomId",
        "assetId",
        "subscriptionId",
        "rosterId",
        "sessionId",
        "orderId",
        "jobId",
        "resourceId",
    ]
    .into_iter()
    .find(|field| values.get(*field).is_some());
    if target == "id"
        && values.get("expectedOrderRevision").is_some()
        && row["sourceOrder"]["revision"].is_i64()
    {
        values["expectedOrderRevision"] = row["sourceOrder"]["revision"].clone();
    }
    if primary == Some(target) {
        for field in ["revision", "expectedRevision"] {
            if values.get(field).is_some() {
                values[field] = revision.clone();
            }
        }
    }
}
fn bind_path_revisions(values: &mut Value, row: &Value, target: &str, parent: &[String]) {
    if parent.is_empty() {
        bind_revisions(values, row, target);
    } else {
        let mut nested = at(values, parent);
        bind_revisions(&mut nested, row, target);
        put(values, parent, nested);
    }
}
fn invoice_policy_revision(refs: &Value, book: &Value) -> Value {
    if book.is_string() && book == &refs["__invoicePolicyBook"] {
        refs["__invoicePolicyRevision"]
            .as_u64()
            .map_or(Value::Null, |v| json!(v))
    } else {
        Value::Null
    }
}
fn prefills(fields: &[Value], row: &Value, values: &mut Value) {
    for f in fields {
        let Some(name) = f["name"].as_str() else {
            continue;
        };
        if f["kind"] == "object" {
            let mut nested = values.get(name).cloned().unwrap_or_else(|| json!({}));
            prefills(
                f["fields"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                row,
                &mut nested,
            );
            values[name] = nested;
            continue;
        }
        if f["automatic"] == "revision" {
            if let Some(reference) = f["revisionOf"].as_str() {
                if !values[reference].is_string() || values[reference] != row[reference] {
                    continue;
                }
            }
            if let Some(revision) = row.get(name).or_else(|| row.get("revision")) {
                values[name] = revision.clone();
            }
            continue;
        }
        if let Some(v) = row.get(name) {
            if name == "id" || name.ends_with("Id") {
                values[name] = v.clone();
            }
        }
    }
}
fn command(body: &Value, module: &Value) -> Value {
    match module["envelope"].as_str() {
        Some("command" | "action-command") => body["command"].clone(),
        Some("nested-command") => body["mutation"]["command"].clone(),
        Some("organization" | "data-mutation") => {
            let mut c = body["mutation"]["data"].clone();
            c["type"] = body["mutation"]["type"].clone();
            c
        }
        _ => body["mutation"].clone(),
    }
}
#[component]
pub(super) fn Actions(work: Workbench, selected: RwSignal<Value>) -> impl IntoView {
    let module = StoredValue::new(
        contracts::module(work.domain.slug())
            .cloned()
            .unwrap_or(Value::Null),
    );
    let first = module.get_value()["actions"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["providerOnly"] != true))
        .and_then(|a| a["id"].as_str())
        .unwrap_or_default()
        .to_string();
    let chosen = RwSignal::new(first);
    let values = RwSignal::new(json!({}));
    let plan = RwSignal::new(None::<Value>);
    let refs = RwSignal::new(json!({}));
    let action = Signal::derive(move || {
        module.get_value()["actions"]
            .as_array()
            .and_then(|a| a.iter().find(|a| a["id"] == chosen.get()))
            .cloned()
            .unwrap_or(Value::Null)
    });
    Effect::new(move |_| {
        let a = action.get();
        let company = work.company.get();
        let mut row = selected.get();
        if let Some((_, expected)) = reference(
            "id",
            a["id"].as_str().unwrap_or_default(),
            work.domain.slug(),
        ) {
            if row["_collection"]
                .as_str()
                .is_some_and(|collection| collection != expected)
            {
                if let Some(object) = row.as_object_mut() {
                    object.remove("id");
                }
            }
        }
        let fields = a["fields"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut draft = contracts::field_defaults(fields);
        prefills(fields, &row, &mut draft);
        if let Some(c) = company {
            for f in fields {
                match f["name"].as_str() {
                    Some("companyId") => draft["companyId"] = c["id"].clone(),
                    Some("expectedCompanyRevision") => {
                        draft["expectedCompanyRevision"] = c["revision"].clone()
                    }
                    _ => {}
                }
            }
        }
        values.set_if_alive(draft);
        if work.domain.slug() == "projects" {
            hydrate_project_revision(values);
        }
        plan.set_if_alive(None);
    });
    Effect::new(move |_| {
        let company = work.company_key.get();
        refs.update_if_alive(|v| {
            if text(v, "__companyId") != company {
                *v = json!({"__companyId":company});
            }
        });
        let data = work.data.get();
        if work.domain == Domain::Other("accounting") {
            refs.update_if_alive(|v| {
                v["__currency"] = data["configuredBook"]["currency"].clone();
                v["__minorUnits"] = data["configuredBook"]["minorUnits"].clone();
                v["__purchasePolicyBook"] = data["configuredBook"]["id"].clone();
                v["__purchasePolicyRevision"] = match data.get("purchasePolicy") {
                    Some(Value::Null) => json!(0),
                    Some(policy) => policy["revision"].clone(),
                    None => Value::Null,
                };
                v["__stockPolicyBook"] = data["configuredBook"]["id"].clone();
                v["__stockPolicyRevision"] = match data.get("stockPolicy") {
                    Some(Value::Null) => json!(0),
                    Some(policy) => policy["revision"].clone(),
                    None => Value::Null,
                };
                v["__invoicePolicyBook"] = data["configuredBook"]["id"].clone();
                v["__invoicePolicyRevision"] = match data.get("invoicePolicy") {
                    Some(Value::Null) => json!(0),
                    Some(policy) => policy["revision"].clone(),
                    None => Value::Null,
                };
            });
        }
        if let Some(detail) = data["detail"].as_object() {
            for (key, rows) in detail {
                if rows.is_array() {
                    refs.update_if_alive(|v| {
                        v[format!("{}/{}", work.domain.slug(), key)] = rows.clone()
                    });
                }
            }
        }
        for (key, rows) in data.as_object().into_iter().flatten() {
            if rows.is_array() && key != "permissions" && key != "collections" {
                refs.update_if_alive(|v| {
                    let scoped =
                        matches!(work.domain.slug(), "commerce" | "purchasing" | "billing")
                            && matches!(
                                key.as_str(),
                                "products" | "orders" | "offers" | "invoices" | "documents"
                            );
                    v[format!("{}/{}", work.domain.slug(), key)] = if scoped {
                        json!(rows
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|r| text(r, "companyId") == company)
                            .cloned()
                            .collect::<Vec<_>>())
                    } else {
                        rows.clone()
                    };
                });
            }
        }
    });
    let preview = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(held) = session::held() else {
            return;
        };
        match contracts::envelope(
            work.domain.slug(),
            &held.tenant,
            "preview",
            &chosen.get_untracked(),
            &values.get_untracked(),
        ) {
            Ok(body) => {
                let body = command(&body, &module.get_value());
                if work.domain == Domain::Other("accounting")
                    && matches!(
                        body["type"].as_str(),
                        Some("invoice_terms.set" | "supplier_terms.set")
                    )
                    && !accounting::source_review_accounts_match(
                        &body,
                        &work.data.get_untracked()["configuredBook"],
                        &refs.get_untracked(),
                    )
                {
                    work.fail("Review the original document, current versions and explicit agreed due terms before saving.");
                    return;
                }
                if work.domain == Domain::Other("accounting")
                    && matches!(
                        body["type"].as_str(),
                        Some(
                            "payable_payment.draft"
                                | "payable_return.draft"
                                | "clearing_settlement.draft"
                        )
                    )
                    && !accounting::source_review_accounts_match(
                        &body,
                        &work.data.get_untracked()["configuredBook"],
                        &refs.get_untracked(),
                    )
                {
                    work.fail("Refresh and review the original supplier bill or payment, actual bank transaction and current accounts before saving.");
                    return;
                }
                if work.domain == Domain::Other("accounting")
                    && matches!(
                        body["type"].as_str(),
                        Some(
                            "invoice_source.draft" | "purchase_source.draft" | "stock_source.draft"
                        )
                    )
                    && !body["sourceId"].as_str().is_some_and(|id| {
                        accounting::source_reference(
                            &refs.get_untracked(),
                            id,
                            body["type"].as_str().unwrap_or_default(),
                        )
                        .is_some()
                    })
                {
                    work.fail("Refresh and select the captured source so its exact amount can be reviewed.");
                    return;
                }
                if work.domain == Domain::Other("accounting")
                    && matches!(
                        body["type"].as_str(),
                        Some("purchase_source.draft" | "stock_source.draft")
                    )
                    && !accounting::source_review_accounts_match(
                        &body,
                        &work.data.get_untracked()["configuredBook"],
                        &refs.get_untracked(),
                    )
                {
                    work.fail("Review the source amounts and select its original mapped accounts before saving.");
                    return;
                }
                plan.set_if_alive(Some(body));
                work.notice.set_if_alive(String::new());
            }
            Err(reason) => work.fail(reason),
        }
    };
    view! {<section class="operator-actions"><h2>"Work with this module"</h2><p class="hint">"Choose an action, enter its details, then review before saving. The server checks your current permissions and record versions."</p>
        <label class="ops-field"><span>"Action"</span><select data-testid="operator-action" prop:value=move||chosen.get() on:change=move|e|chosen.set_if_alive(event_target_value(&e))>
            {module.get_value()["actions"].as_array().cloned().unwrap_or_default().into_iter().filter(|a|a["providerOnly"]!=true).map(|a|{let id=text(&a,"id");view!{<option value=id>{text(&a,"label")}</option>}}).collect::<Vec<_>>()}
        </select></label>
        {move||{let a=action.get();let permission=StoredValue::new(text(&a,"permission"));let op=text(&a,"id");let housekeeping=work.domain.slug()=="accommodation"&&op=="room.housekeeping";let allowed=work.permits(&permission.get_value())||housekeeping&&work.permits("rooms.inspect");let fields=a["fields"].as_array().cloned().unwrap_or_default();
            view!{<form class="ops-form" on:submit=preview><fieldset disabled=move||!allowed||work.blocked()><legend>{text(&a,"label")}</legend>
                {fields.into_iter().map(|field|{let name=text(&field,"name");field_view(field,vec![name],values,refs,work.domain.slug(),op.clone())}).collect::<Vec<_>>()}
                <button class="btn" type="submit" disabled=move||housekeeping&&!work.permits(if values.get()["state"]=="inspected"{"rooms.inspect"}else{"housekeeping.write"}) data-testid=format!("module-{}-create",work.domain.slug())>"Review action"</button>
            </fieldset>{(!allowed).then(||view!{<p class="hint">"Your current role cannot perform this action."</p>})}</form>}
        }}
        {move||plan.get().map(|body|{let decision=body.clone();view!{<section class="operator-review" data-testid="operator-review"><h3>"Review action"</h3><p>{action.get()["label"].as_str().unwrap_or_default().to_string()}</p><p class="hint">"Saving submits these details once. An uncertain response keeps its original reference for recovery."</p>
            {if work.domain==Domain::Other("accounting"){accounting::review(body.clone(),&work.data.get()["configuredBook"],&refs.get())}else{details(body.clone())}}<MissionFromAction work command=body.clone()/><button type="button" class="btn" data-testid="operator-confirm" disabled=move||work.blocked()||(work.domain==Domain::Other("accounting")&&matches!(decision["type"].as_str(),Some("invoice_source.draft"|"purchase_source.draft"|"stock_source.draft"|"payable_payment.draft"|"payable_return.draft"|"clearing_settlement.draft"|"invoice_terms.set"|"supplier_terms.set"))&&!accounting::source_review_accounts_match(&decision,&work.data.get()["configuredBook"],&refs.get())) on:click=move|_|{work.send(body.clone());plan.set_if_alive(None);}>"Confirm and save"</button>
        </section>}})}
    </section>}
}
#[component]
fn MissionFromAction(work: Workbench, command: Value) -> impl IntoView {
    let command = StoredValue::new(command);
    let busy = RwSignal::new(false);
    let navigate = use_navigate();
    let create = move |_| {
        if busy.get_untracked() || work.blocked() {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let domain = format!("missions:{}", held.principal);
        if pending::read_scoped(&held.tenant, &domain, &held.principal).is_some() {
            work.notice.set_if_alive(
                "An earlier mission needs a status check in Missions before creating another."
                    .into(),
            );
            return;
        }
        let key = uuid_string();
        let payload = command.get_value();
        let action = payload["operation"]
            .as_str()
            .or_else(|| payload["type"].as_str())
            .or_else(|| payload["action"].as_str())
            .unwrap_or_default();
        let target = format!("{}.{action}", work.domain.slug());
        if let Err(error) = pending::write(&PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            domain,
            held.principal.clone(),
        )) {
            work.notice.set_if_alive(error);
            return;
        }
        let body=json!({"tenant_id":held.tenant,"mission_code":key,"title":format!("Review {target}"),"description":"Reviewed native business action. A human approval is required before execution.","auto_launch":true,"steps":[{"step_index":0,"step_name":target,"tool_name":target,"payload_input":payload,"depends_on":[]}]}).to_string();
        busy.set_if_alive(true);
        let navigate = navigate.clone();
        spawn_local(async move {
            match session::api_fetch("POST", "/api/v1/cockpit/missions", Some(body)).await {
                Ok(answer)
                    if matches!(answer.status, 200 | 201)
                        && answer.body["mission_code"] == key
                        && answer.body["id"].is_string() =>
                {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    navigate("/app/erp?module=missions", Default::default());
                }
                Ok(answer) if (400..500).contains(&answer.status) => {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    work.notice.set_if_alive(message(&answer));
                }
                _ => {
                    navigate("/app/erp?module=missions", Default::default());
                }
            }
            busy.set_if_alive(false);
        });
    };
    view! {<button type="button" class="btn secondary" data-testid="operator-create-mission" disabled=move||busy.get()||work.blocked() on:click=create>"Create mission for approval"</button>}
}
pub(super) fn details(value: Value) -> AnyView {
    match value {
        Value::Object(fields)=>view!{<dl class="operator-details">{fields.into_iter().filter(|(key,_)|!key.starts_with('_')&&!matches!(key.as_str(),"tenantId"|"commandId"|"operation"|"type"|"action")).map(|(key,value)|view!{<dt>{label(&key)}</dt><dd>{details(value)}</dd>}).collect::<Vec<_>>()}</dl>}.into_any(),
        Value::Array(items)=>view!{<ul class="ops-lines">{items.into_iter().map(|v|view!{<li>{details(v)}</li>}).collect::<Vec<_>>()}</ul>}.into_any(),
        value=>view!{<span>{if value.is_null(){"None".into()}else{string(&value)}}</span>}.into_any(),
    }
}
fn load_page(
    work: Workbench,
    collection: Option<String>,
    cursor: Option<String>,
    id: Option<String>,
) {
    let typed = matches!(
        work.domain.slug(),
        "subscriptions"
            | "attendance"
            | "laundry"
            | "field"
            | "education"
            | "fulfillment"
            | "accounting"
    );
    if id.is_some() && !typed {
        return;
    }
    let Some(held) = session::held() else {
        return;
    };
    let mut path = format!(
        "/erp/{}/workspace?tenant={}&search={}",
        work.domain.slug(),
        crate::contacts::query_encode(&held.tenant),
        crate::contacts::query_encode(&work.search.get_untracked())
    );
    for (name, value) in [
        ("collection", collection),
        ("cursor", cursor),
        ("id", id.clone()),
    ] {
        if let Some(v) = value {
            path.push_str(&format!("&{name}={}", crate::contacts::query_encode(&v)));
        }
    }
    if work.domain == Domain::Other("accounting") {
        let company = work.company_key.get_untracked();
        if !company.is_empty() {
            path.push_str(&format!(
                "&companyId={}",
                crate::contacts::query_encode(&company)
            ));
        }
    }
    work.sequence.update_if_alive(|s| *s += 1);
    let sequence = work.sequence.get_untracked();
    work.loading.set_if_alive(true);
    spawn_local(async move {
        let response = session::api_fetch("GET", &path, None).await;
        if work.sequence.is_disposed() || work.sequence.get_untracked() != sequence {
            return;
        }
        match response {
            Ok(answer) if answer.status == 200 => {
                work.data.update_if_alive(|data| {
                    if id.is_some() {
                        data["detail"] = answer.body["detail"].clone();
                    } else if let Some(object) = answer.body.as_object() {
                        for (key, value) in object {
                            if key == "collections" {
                                if let Some(existing) = data[key].as_array_mut() {
                                    for next in value.as_array().into_iter().flatten() {
                                        if let Some(row) =
                                            existing.iter_mut().find(|r| r["id"] == next["id"])
                                        {
                                            *row = next.clone();
                                        }
                                    }
                                }
                            } else {
                                data[key] = value.clone();
                            }
                        }
                    }
                });
            }
            Ok(answer) => work.notice.set_if_alive(message(&answer)),
            Err(e) => work
                .notice
                .set_if_alive(format!("Records could not be loaded: {e}")),
        }
        work.loading.set_if_alive(false);
    });
}
#[component]
pub(super) fn Workspace(domain: Domain) -> impl IntoView {
    let work = Workbench::new(domain);
    let title = archava_domain::operator::module(domain.slug())
        .and_then(|m| m["label"].as_str())
        .unwrap_or("Records");
    let collection = RwSignal::new(String::new());
    let selected = RwSignal::new(json!({}));
    let prior_company = RwSignal::new(String::new());
    Effect::new(move |_| {
        let company = work.company_key.get();
        if company != prior_company.get_untracked() {
            prior_company.set_if_alive(company);
            selected.set_if_alive(json!({}));
            collection.set_if_alive(String::new());
        }
    });
    Effect::new(move |_| {
        let data = work.data.get();
        if collection.get_untracked().is_empty() {
            if let Some(id) = data["collections"][0]["id"].as_str() {
                collection.set_if_alive(id.into());
            }
        }
        if let Some(id) = selected.get_untracked()["id"].as_str() {
            for rows in data
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, v)| v.as_array())
            {
                if let Some(row) = rows.iter().find(|r| r["id"] == id) {
                    selected.set_if_alive(record_context(row.clone(), &collection.get_untracked()));
                    break;
                }
            }
        }
    });
    view! {<div class="operator-workspace" data-testid=format!("operator-workspace-{}",domain.slug())><Toolbar work title/><Feedback work/>
        <label class="ops-field"><span>"Collection"</span><select data-testid="operator-collection" prop:value=move||collection.get() on:change=move|e|{collection.set_if_alive(event_target_value(&e));selected.set_if_alive(json!({}));}>
            {move||work.data.get()["collections"].as_array().cloned().unwrap_or_default().into_iter().map(|c|view!{<option value=text(&c,"id")>{text(&c,"label")}</option>}).collect::<Vec<_>>()}
        </select></label>
        <div class="ops-actions"><button type="button" class="btn secondary" disabled=move||work.loading.get() on:click=move|_|work.refresh()>"First page"</button><button type="button" class="btn secondary" data-testid="operator-next" disabled=move||work.loading.get()||!work.data.get()["collections"].as_array().is_some_and(|cs|cs.iter().any(|c|c["id"]==collection.get()&&c["hasMore"]==true)) on:click=move|_|{let c=collection.get_untracked();let cursor=work.data.get_untracked()["collections"].as_array().and_then(|cs|cs.iter().find(|v|v["id"]==c)).and_then(|v|v["nextCursor"].as_str()).map(str::to_owned);load_page(work,Some(c),cursor,None);}>"Next page"</button></div><p class="hint">"Search filters the current bounded page; use Next page to continue through matches."</p>{(domain==Domain::Other("accounting")).then(||view!{<accounting::Reports work/>})}<div class="ops-layout"><section class="ops-main"><ul class="operator-records">{move||work.rows(&collection.get()).into_iter().map(|row|{let name=caption(&row);let state=row["state"].as_str().or_else(||row["status"].as_str()).unwrap_or_default().to_string();view!{<li><button type="button" class="btn secondary" data-testid="operator-open" on:click=move|_|{selected.set_if_alive(record_context(row.clone(),&collection.get_untracked()));load_page(work,if domain==Domain::Other("accounting"){Some(collection.get_untracked())}else{None},None,Some(text(&selected.get_untracked(),"id")));}>{name}</button><span>{state}</span></li>}}).collect::<Vec<_>>()}</ul>
            {move||(!selected.get().is_null()&&selected.get().as_object().is_some_and(|o|!o.is_empty())).then(||view!{<section class="ops-detail" data-testid="operator-detail"><h2>{caption(&selected.get())}</h2>{details(selected.get())}{move||details(work.data.get()["detail"].clone())}</section>})}
        </section><aside class="ops-aside">{(domain==Domain::Other("organizations")).then(||view!{<Show when=move||work.permits("companies.read")><companies::Registry work/></Show><parties::Registry work/><organizations::Affiliations work/>})}<Actions work selected/></aside></div>
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_term_forms_need_an_explicit_condition_and_roundtrip_both_source_namespaces() {
        let module = contracts::module("accounting").unwrap();
        for op in ["invoice_terms.set", "supplier_terms.set"] {
            let action = module["actions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == op)
                .unwrap();
            let mut values = contracts::field_defaults(action["fields"].as_array().unwrap());
            assert!(values["condition"]["kind"].is_null());
            values["sourceId"] = json!("a0000000-0000-4000-8000-000000000001");
            values["expectedDocumentRevision"] = json!(4);
            values["expectedRevision"] = json!(0);
            values["effectiveFrom"] = json!("2026-11-12");
            values["reason"] = json!("Review the actual original document and agreed dates");
            let key = "a0000000-0000-4000-8000-000000000002";
            assert!(contracts::envelope("accounting", "acme", key, op, &values).is_err());
            for condition in [
                json!({"kind":"due_on","date":"2026-11-30"}),
                json!({"kind":"unknown"}),
            ] {
                values["condition"] = condition.clone();
                let wire = contracts::envelope("accounting", "acme", key, op, &values).unwrap();
                assert_eq!(wire["command"]["condition"], condition);
                assert_eq!(wire["command"]["expectedDocumentRevision"], 4);
                assert!(archava_domain::erp::accounting::parse(
                    &json!({"requestId":wire["requestId"],"command":wire["command"]})
                )
                .is_ok());
            }
        }
    }
    #[test]
    fn positive_gateway_fee_selection_binds_its_current_revision_and_zero_clears_the_optional_account(
    ) {
        let module = contracts::module("accounting").unwrap();
        let action = module["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "clearing_settlement.draft")
            .unwrap();
        let fields = action["fields"].as_array().unwrap();
        let fee_fields = fields.iter().find(|f| f["name"] == "feeAccount").unwrap()["fields"]
            .as_array()
            .unwrap();
        let defaults = contracts::field_defaults(fee_fields);
        let mut values = contracts::field_defaults(fields);
        assert!(optional_group_update(&values["feeAccount"], &defaults, false).is_none());
        values["sourceId"] = json!("a0000000-0000-4000-8000-000000000001");
        values["bankRowId"] = json!("a0000000-0000-4000-8000-000000000002");
        values["code"] = json!("FEE-REVIEW");
        values["clearingAccount"] =
            json!({"accountId":"a0000000-0000-4000-8000-000000000003","expectedAccountRevision":2});
        values["cashAccount"] =
            json!({"accountId":"a0000000-0000-4000-8000-000000000004","expectedAccountRevision":3});
        values["grossMinor"] = json!("3000000");
        values["feeMinor"] = json!("20000");
        values["reason"] = json!("Review actual gross, explicit fee and original current accounts");
        values["feeAccount"] =
            optional_group_update(&values["feeAccount"], &defaults, true).unwrap();
        values["feeAccount"]["accountId"] = json!("a0000000-0000-4000-8000-000000000005");
        bind_path_revisions(
            &mut values,
            &json!({"revision":7}),
            "accountId",
            &["feeAccount".into()],
        );
        assert_eq!(values["feeAccount"]["expectedAccountRevision"], 7);
        assert_eq!(values["clearingAccount"]["expectedAccountRevision"], 2);
        assert_eq!(values["cashAccount"]["expectedAccountRevision"], 3);
        assert!(optional_group_update(&values["feeAccount"], &defaults, true).is_none());
        let key = "a0000000-0000-4000-8000-000000000006";
        let wire = contracts::envelope(
            "accounting",
            "acme",
            key,
            "clearing_settlement.draft",
            &values,
        )
        .unwrap();
        assert_eq!(wire["command"]["feeAccount"]["expectedAccountRevision"], 7);
        assert!(archava_domain::erp::accounting::parse(
            &json!({"requestId":wire["requestId"],"command":wire["command"]})
        )
        .is_ok());
        values["feeMinor"] = json!("0");
        values["feeAccount"] =
            optional_group_update(&values["feeAccount"], &defaults, false).unwrap();
        let wire = contracts::envelope(
            "accounting",
            "acme",
            key,
            "clearing_settlement.draft",
            &values,
        )
        .unwrap();
        assert!(wire["command"].get("feeAccount").is_none());
        assert!(archava_domain::erp::accounting::parse(
            &json!({"requestId":wire["requestId"],"command":wire["command"]})
        )
        .is_ok());
        let reenabled = optional_group_update(&values["feeAccount"], &defaults, true).unwrap();
        assert!(reenabled["accountId"].is_null());
        assert!(reenabled["expectedAccountRevision"].is_null());
    }
    #[test]
    fn invoice_policy_revision_requires_the_loaded_book_and_current_policy() {
        let refs = json!({"__invoicePolicyBook":"book-a","__invoicePolicyRevision":0});
        assert_eq!(invoice_policy_revision(&refs, &json!("book-a")), 0);
        assert_eq!(
            invoice_policy_revision(&refs, &json!("book-b")),
            Value::Null
        );
        assert_eq!(invoice_policy_revision(&refs, &Value::Null), Value::Null);
        assert_eq!(
            invoice_policy_revision(&json!({}), &json!("book-a")),
            Value::Null
        );
        assert_eq!(
            invoice_policy_revision(
                &json!({"__invoicePolicyBook":"book-a","__invoicePolicyRevision":7}),
                &json!("book-a")
            ),
            7
        );
    }
    #[test]
    fn linked_records_do_not_replace_the_aggregate_revision() {
        for (mut values, linked, primary) in [
            (
                json!({"id":"order","expectedRevision":1,"lines":[]}),
                "offerId",
                "id",
            ),
            (
                json!({"id":"case","expectedRevision":1,"assigneeId":null}),
                "assigneeId",
                "id",
            ),
            (
                json!({"proposalId":"proposal","organizationId":null,"expectedRevision":1}),
                "organizationId",
                "proposalId",
            ),
            (
                json!({"assetId":"asset","toCustodianId":null,"expectedRevision":1}),
                "toCustodianId",
                "assetId",
            ),
        ] {
            bind_revisions(&mut values, &json!({"revision":7}), linked);
            assert_eq!(values["expectedRevision"], 1);
            bind_revisions(&mut values, &json!({"revision":2}), primary);
            assert_eq!(values["expectedRevision"], 2);
        }
    }
    #[test]
    fn parent_and_child_versions_are_bound_independently() {
        let mut values =
            json!({"projectId":"project","id":"task","projectRevision":9,"expectedRevision":1});
        bind_revisions(&mut values, &json!({"revision":10}), "projectId");
        assert_eq!(values["projectRevision"], 10);
        assert_eq!(values["expectedRevision"], 1);
        bind_revisions(&mut values, &json!({"revision":2}), "id");
        assert_eq!(values["projectRevision"], 10);
        assert_eq!(values["expectedRevision"], 2);
        bind_revisions(&mut values, &json!({"revision":99}), "milestoneId");
        assert_eq!(values["expectedRevision"], 2);
        assert_eq!(values["projectRevision"], 10);
    }
    #[test]
    fn journal_line_account_versions_bind_only_the_selected_line() {
        let mut v = json!({"expectedRevision":7,"lines":[{"accountId":"A","expectedAccountRevision":1},{"accountId":"B","expectedAccountRevision":3}]});
        bind_path_revisions(
            &mut v,
            &json!({"revision":5}),
            "accountId",
            &["lines".into(), "0".into()],
        );
        assert_eq!(v["expectedRevision"], 7);
        assert_eq!(v["lines"][0]["expectedAccountRevision"], 5);
        assert_eq!(v["lines"][1]["expectedAccountRevision"], 3);
    }
    #[test]
    fn bank_match_cash_and_journal_versions_bind_to_their_own_reference() {
        let mut v = json!({"journalId":"journal","expectedJournalRevision":null,"account":{"accountId":"cash","expectedAccountRevision":null}});
        bind_revisions(&mut v, &json!({"revision":2}), "journalId");
        bind_path_revisions(
            &mut v,
            &json!({"revision":4}),
            "accountId",
            &["account".into()],
        );
        assert_eq!(v["expectedJournalRevision"], 2);
        assert_eq!(v["account"]["expectedAccountRevision"], 4);
        bind_revisions(&mut v, &json!({"revision":99}), "bankRowId");
        assert_eq!(v["expectedJournalRevision"], 2);
    }
    #[test]
    fn selecting_a_bank_policy_never_prefills_an_unselected_period_version() {
        let fields = json!([{"name":"bankPolicyId","kind":"string"},{"name":"expectedPolicyRevision","kind":"integer","automatic":"revision","revisionOf":"bankPolicyId"},{"name":"periodId","kind":"string"},{"name":"expectedPeriodRevision","kind":"integer","automatic":"revision","revisionOf":"periodId"}]);
        let fields = fields.as_array().unwrap();
        let mut draft = contracts::field_defaults(fields);
        prefills(
            fields,
            &json!({"bankPolicyId":"bank-policy","revision":2}),
            &mut draft,
        );
        assert_eq!(draft["expectedPolicyRevision"], 2);
        assert!(draft["expectedPeriodRevision"].is_null());
    }
}

/// The same native fields can supply missing agent plan details, without executing a command.
#[component]
pub(crate) fn AgentActionEditor(
    domain: &'static str,
    on_plan: Callback<(String, Value)>,
    #[prop(optional)] context: Value,
) -> impl IntoView {
    let context = StoredValue::new(context);
    let module = StoredValue::new(contracts::module(domain).cloned().unwrap_or(Value::Null));
    let first = module.get_value()["actions"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["providerOnly"] != true))
        .and_then(|a| a["id"].as_str())
        .unwrap_or_default()
        .to_string();
    let chosen = RwSignal::new(first);
    let values = RwSignal::new(json!({}));
    let refs = RwSignal::new(accounting::agent_references(&context.get_value()));
    let error = RwSignal::new(String::new());
    let action = Signal::derive(move || {
        module.get_value()["actions"]
            .as_array()
            .and_then(|a| a.iter().find(|a| a["id"] == chosen.get()))
            .cloned()
            .unwrap_or(Value::Null)
    });
    Effect::new(move |_| {
        let a = action.get();
        values.set_if_alive(contracts::field_defaults(
            a["fields"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
        ));
        error.set_if_alive(String::new());
    });
    view! {<form class="ops-form agent-action-editor" on:submit=move|e|{e.prevent_default();let Some(held)=session::held()else{return};let a=chosen.get_untracked();match contracts::envelope(domain,&held.tenant,"preview",&a,&values.get_untracked()){Ok(body)=>{let mut data=command(&body,&module.get_value());if let Some(o)=data.as_object_mut(){o.remove(module.get_value()["tag"].as_str().unwrap_or("operation"));for f in action.get_untracked()["fields"].as_array().into_iter().flatten(){if f["automatic"]=="requestId"{if let Some(name)=f["name"].as_str(){o.remove(name);}}}}on_plan.run((format!("{domain}.{a}"),data));},Err(reason)=>error.set_if_alive(reason)}}>
        <label class="ops-field"><span>"Action to prepare"</span><select data-testid="agent-action" prop:value=move||chosen.get() on:change=move|e|chosen.set_if_alive(event_target_value(&e))>{module.get_value()["actions"].as_array().cloned().unwrap_or_default().into_iter().filter(|a|a["providerOnly"]!=true).map(|a|view!{<option value=text(&a,"id")>{text(&a,"label")}</option>}).collect::<Vec<_>>()}</select></label>
        {move||{let a=action.get();let op=text(&a,"id");a["fields"].as_array().cloned().unwrap_or_default().into_iter().map(|f|{let name=text(&f,"name");field_view(f,vec![name],values,refs,domain,op.clone())}).collect::<Vec<_>>()}}
        <p role="status">{move||error.get()}</p><button class="btn secondary" data-testid="agent-prepare-action">"Add these details to the plan"</button>
    </form>}
}
