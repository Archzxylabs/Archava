//! What still stands between a financial period and its close, before the
//! close is submitted.
use super::*;

/// The sentence shown for one outstanding condition.
fn blocker(row: &Value) -> String {
    let count = row["count"].as_i64().unwrap_or(0);
    let what = match row["reason"].as_str() {
        Some("draft_journals") => "draft journals to post or void",
        Some("unposted_invoice_sources") => "invoice sources to post",
        Some("unposted_purchase_sources") => "receipt and supplier claim sources to post",
        Some("unposted_stock_sources") => "stock cost sources to post",
        Some("unmatched_bank_rows") => "imported statement amounts to match",
        Some("banks_without_policy") => "banks without a reconciliation opening basis",
        Some("bank_reviews_incomplete") => "banks without complete, freshly reviewed evidence",
        _ => "other conditions to resolve",
    };
    format!("{count} {what}")
}

/// The one-line verdict for a report.
fn verdict(report: &Value) -> &'static str {
    if report["period"]["state"] == "closed" {
        "This period is already closed."
    } else if report["ready"] != true {
        "This period cannot be closed yet."
    } else if report["canClose"] == true {
        "Nothing is outstanding. This period can be closed."
    } else {
        "Nothing is outstanding. Closing needs the period close permission."
    }
}

#[component]
pub(super) fn CloseReadiness(work: Workbench) -> impl IntoView {
    let period = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Value>);
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let data = work.data.get();
        report.set_if_alive(None);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|v| *v += 1);
        busy.set_if_alive(false);
        let known = data["periods"].as_array().cloned().unwrap_or_default();
        let chosen = period.get_untracked();
        if !known.iter().any(|row| text(row, "id") == chosen) {
            period.set_if_alive(known.first().map(|row| text(row, "id")).unwrap_or_default());
        }
    });
    let load = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(held) = session::held() else { return };
        let chosen = period.get_untracked();
        if chosen.is_empty() {
            notice.set_if_alive("Open a financial period first.".into());
            return;
        }
        let company = work.company_key.get_untracked();
        sequence.update_if_alive(|v| *v += 1);
        let current = sequence.get_untracked();
        busy.set_if_alive(true);
        notice.set_if_alive(String::new());
        let path = format!(
            "/erp/accounting/close-readiness?tenant={}&periodId={chosen}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if sequence.is_disposed()
                || sequence.get_untracked() != current
                || work.company_key.get_untracked() != company
            {
                return;
            }
            busy.set_if_alive(false);
            match answer {
                Ok(v) if v.status == 200 => report.set_if_alive(Some(v.body)),
                Ok(v) => {
                    report.set_if_alive(None);
                    notice.set_if_alive(message(&v));
                }
                Err(_) => {
                    report.set_if_alive(None);
                    notice.set_if_alive(
                        "The close check could not be loaded. Refresh and try again.".into(),
                    );
                }
            }
        });
    };
    view! {<section class="ops-detail" data-testid="accounting-close-check"><h3>"Period close check"</h3>
        <p class="hint">"Lists what must be resolved before a period can be closed. The close itself is still checked when it is submitted."</p>
        <form class="ops-form" on:submit=load>
            <label class="ops-field"><span>"Financial period"</span><select data-testid="accounting-close-period" prop:value=move||period.get() on:change=move|e|{period.set_if_alive(event_target_value(&e));report.set_if_alive(None);}>
                {move||work.data.get()["periods"].as_array().cloned().unwrap_or_default().into_iter().map(|row|{let id=text(&row,"id");let chosen=id==period.get_untracked();view!{<option value=id selected=chosen>{format!("{} · {}",text(&row,"code"),text(&row,"state"))}</option>}}).collect_view()}
            </select></label>
            <button type="submit" class="btn secondary" data-testid="accounting-close-run" disabled=move||busy.get()||!work.permits("reports.read")>"Check period"</button>
        </form>
        {move||{let n=notice.get();(!n.is_empty()).then(||view!{<p class="refusal">{n}</p>})}}
        {move||report.get().map(|r|{
            let blockers=r["blockers"].as_array().cloned().unwrap_or_default();
            let ready=r["ready"]==true;
            view!{<div data-testid="accounting-close-result">
                <h4>{format!("{} · {} to {}",text(&r["period"],"code"),text(&r["period"],"startDate"),text(&r["period"],"endDate"))}</h4>
                <p data-testid="accounting-close-verdict" class=if ready {"hint"} else {"refusal"}>{verdict(&r)}</p>
                <ul data-testid="accounting-close-blockers">{blockers.iter().map(|row|view!{<li>{blocker(row)}</li>}).collect_view()}</ul>
            </div>}
        })}
    </section>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_reported_reason_has_its_own_sentence_and_the_verdict_follows_the_report() {
        let reasons = [
            "draft_journals",
            "unposted_invoice_sources",
            "unposted_purchase_sources",
            "unposted_stock_sources",
            "unmatched_bank_rows",
            "banks_without_policy",
            "bank_reviews_incomplete",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for reason in reasons {
            let line = blocker(&json!({"reason":reason,"count":2}));
            assert!(line.starts_with("2 "), "{line}");
            assert!(!line.contains("other conditions"), "{reason}");
            assert!(seen.insert(line), "{reason} repeats another sentence");
        }
        assert_eq!(
            blocker(&json!({"reason":"new_guard","count":1})),
            "1 other conditions to resolve"
        );
        let open = json!({"state":"open"});
        assert_eq!(
            verdict(&json!({"period":open,"ready":false,"canClose":false})),
            "This period cannot be closed yet."
        );
        assert_eq!(
            verdict(&json!({"period":open,"ready":true,"canClose":true})),
            "Nothing is outstanding. This period can be closed."
        );
        assert_eq!(
            verdict(&json!({"period":open,"ready":true,"canClose":false})),
            "Nothing is outstanding. Closing needs the period close permission."
        );
        assert_eq!(
            verdict(&json!({"period":{"state":"closed"},"ready":false,"canClose":false})),
            "This period is already closed."
        );
    }
}
