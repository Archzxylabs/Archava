//! Explicit date review from the current original-document reference.
use super::*;
use archava_domain::erp::accounting::terms as rules;

pub(in crate::operations) fn source<'a>(
    refs: &'a Value,
    action: &str,
    id: &Value,
) -> Option<&'a Value> {
    let collection = match action {
        "invoice_terms.set" => "accounting/invoice_term_sources",
        "supplier_terms.set" => "accounting/supplier_term_sources",
        _ => return None,
    };
    refs[collection].as_array()?.iter().find(|r| &r["id"] == id)
}
pub(super) fn condition_kind(current: &Value, kind: &str) -> Value {
    match kind {
        "unknown" => json!({"kind":"unknown"}),
        "due_on" => {
            json!({"kind":"due_on","date":if current["kind"]=="due_on" {current["date"].clone()} else {Value::Null}})
        }
        _ => json!({"kind":null}),
    }
}
#[component]
pub(in crate::operations) fn ConditionField(
    value: Signal<Value>,
    set: Callback<Value>,
) -> impl IntoView {
    view! {<fieldset class="operator-group"><legend>"Agreed due terms"</legend>
        <label class="ops-field"><span>"Due condition"</span><select data-testid="operator-accounting-condition-kind" required
            prop:value=move||text(&value.get(),"kind")
            on:change=move|e|set.run(condition_kind(&value.get_untracked(),&event_target_value(&e)))>
            <option value="">"Choose…"</option><option value="due_on">"Agreed due date"</option><option value="unknown">"Due date explicitly unknown"</option>
        </select></label>
        <Show when=move||value.get()["kind"]=="due_on">
            <label class="ops-field"><span>"Agreed due date"</span><input data-testid="operator-accounting-condition-date" type="date" required
                prop:value=move||text(&value.get(),"date") on:input=move|e|set.run(json!({"kind":"due_on","date":event_target_value(&e)}))/></label>
        </Show>
        <p class="hint">"Choose the agreed date or explicitly mark it unknown. Changing the document clears this proposal."</p>
    </fieldset>}
}
pub(super) fn valid_review(body: &Value, book: &Value, refs: &Value) -> bool {
    let Some(row) = body["type"]
        .as_str()
        .and_then(|op| source(refs, op, &body["sourceId"]))
    else {
        return false;
    };
    if !book["id"].is_string()
        || row["bookId"] != book["id"]
        || row["documentRevision"] != body["expectedDocumentRevision"]
        || row["termsRevision"] != body["expectedRevision"]
    {
        return false;
    }
    let Some(date) = row["sourceDate"].as_str() else {
        return false;
    };
    let previous = if row["termsRevision"] == 0 {
        None
    } else {
        let Some(revision) = row["termsRevision"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
        else {
            return false;
        };
        let Some(effective_from) = row["latestEffectiveFrom"].as_str() else {
            return false;
        };
        let Ok(condition) = serde_json::from_value(row["latestCondition"].clone()) else {
            return false;
        };
        Some(rules::Terms {
            revision,
            effective_from: effective_from.into(),
            condition,
        })
    };
    let Some(revision) = body["expectedRevision"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
    else {
        return false;
    };
    let Some(effective) = body["effectiveFrom"].as_str() else {
        return false;
    };
    let Ok(condition) = serde_json::from_value(body["condition"].clone()) else {
        return false;
    };
    rules::next(date, previous.as_ref(), revision, effective, condition).is_ok()
}
fn condition_text(condition: &Value) -> String {
    match condition["kind"].as_str() {
        Some("due_on") => format!("Due on {}", text(condition, "date")),
        Some("unknown") => "Due date explicitly unknown".into(),
        _ => "No reviewed due terms recorded".into(),
    }
}
pub(super) fn review(body: &Value, book: &Value, refs: &Value) -> Option<AnyView> {
    let op = body["type"].as_str()?;
    if !matches!(op, "invoice_terms.set" | "supplier_terms.set") {
        return None;
    }
    let Some(row) = source(refs, op, &body["sourceId"]) else {
        return Some(view!{<p class="refusal">"Refresh and select the original document before reviewing due terms."</p>}.into_any());
    };
    Some(view!{<section data-testid="document-terms-summary"><h4>{text(row,"code")}</h4>
        <dl class="operator-details"><dt>"Document"</dt><dd>{text(row,"documentKind")}</dd><dt>"Source date"</dt><dd>{text(row,"sourceDate")}</dd>
        <dt>"Reviewed document version"</dt><dd>{body["expectedDocumentRevision"].to_string()}</dd>
        <dt>"Latest reviewed terms"</dt><dd>{condition_text(&row["latestCondition"])}</dd>
        <dt>"Latest terms effective from"</dt><dd>{row["latestEffectiveFrom"].as_str().unwrap_or("No reviewed terms").to_owned()}</dd>
        <dt>"Proposed terms"</dt><dd>{condition_text(&body["condition"])}</dd><dt>"Apply from"</dt><dd>{text(body,"effectiveFrom")}</dd></dl>
        <p class="hint">"Saving changes the reviewed dates. Amounts and payment balances stay unchanged."</p>
        {(!valid_review(body,book,refs)).then(||view!{<p class="refusal">"The document version, terms version or dates need review. Refresh before saving."</p>})}
    </section>}.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_unknown_removes_the_date_and_review_pins_the_document_and_terms() {
        let due = json!({"kind":"due_on","date":"2026-11-30"});
        let unknown = condition_kind(&due, "unknown");
        assert_eq!(unknown, json!({"kind":"unknown"}));
        assert!(condition_kind(&unknown, "due_on")["date"].is_null());
        assert!(condition_kind(&due, "")["kind"].is_null());
        let book = json!({"id":"book"});
        let refs = json!({"accounting/invoice_term_sources":[{"id":"source","bookId":"book","documentRevision":3,"termsRevision":1,"sourceDate":"2026-11-01","latestEffectiveFrom":"2026-11-01","latestCondition":due}]});
        let body = json!({"type":"invoice_terms.set","sourceId":"source","expectedDocumentRevision":3,"expectedRevision":1,"effectiveFrom":"2026-11-12","condition":unknown});
        assert!(valid_review(&body, &book, &refs));
        for (field, value) in [
            ("type", json!("supplier_terms.set")),
            ("expectedDocumentRevision", json!(2)),
            ("expectedRevision", json!(0)),
            ("effectiveFrom", json!("2026-10-31")),
            ("condition", json!({"kind":"unknown","date":"2026-11-30"})),
            ("condition", json!({"kind":"due_on","date":"2026-02-29"})),
        ] {
            let mut bad = body.clone();
            bad[field] = value;
            assert!(!valid_review(&bad, &book, &refs), "{field}");
        }
    }
}
