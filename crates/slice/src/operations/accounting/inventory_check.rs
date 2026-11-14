//! Inventory control accounts against the posted stock and receipt sources
//! that explain them, through a chosen date.
use super::*;

/// The one-line verdict for a report.
fn verdict(report: &Value) -> &'static str {
    let listed = report["controls"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty());
    if !listed {
        "No inventory account is declared for this book yet."
    } else if report["postedSourceControlsAgree"] == true {
        "Posted stock and receipt sources explain every inventory account balance."
    } else {
        "Some inventory account balances are not explained by posted sources. Review the differences."
    }
}

#[component]
pub(super) fn InventoryControl(work: Workbench) -> impl IntoView {
    let date = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Value>);
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let sequence = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let _ = work.data.get();
        let _ = date.get();
        report.set_if_alive(None);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|v| *v += 1);
        busy.set_if_alive(false);
    });
    let load = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(held) = session::held() else { return };
        let book = text(&work.data.get_untracked()["configuredBook"], "id");
        let chosen = date.get_untracked();
        if book.is_empty()
            || archava_domain::erp::accommodation::parse_calendar_date(&chosen).is_none()
        {
            notice.set_if_alive("Select a company book and a valid report date.".into());
            return;
        }
        let company = work.company_key.get_untracked();
        sequence.update_if_alive(|v| *v += 1);
        let current = sequence.get_untracked();
        busy.set_if_alive(true);
        notice.set_if_alive(String::new());
        let path = format!(
            "/erp/accounting/inventory-control?tenant={}&bookId={book}&asOf={chosen}",
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
                        "The inventory comparison could not be loaded. Refresh and try again."
                            .into(),
                    );
                }
            }
        });
    };
    view! {<section class="ops-detail" data-testid="accounting-inventory-control"><h3>"Inventory accounts against posted stock sources"</h3>
        <p class="hint">"Compares each inventory account's ledger balance with posted receipts, fulfillments and restocks through the chosen date. It does not count the stock on hand."</p>
        <form class="ops-form" on:submit=load>
            <label class="ops-field"><span>"Balance as of"</span><input data-testid="accounting-inventory-date" type="date" min="2000-01-01" max="2100-12-31" required prop:value=move||date.get() on:input=move|e|date.set_if_alive(event_target_value(&e))/></label>
            <button type="submit" class="btn secondary" data-testid="accounting-inventory-run" disabled=move||busy.get()||!work.permits("reports.read")>"Compare inventory accounts"</button>
        </form>
        {move||{let n=notice.get();(!n.is_empty()).then(||view!{<p class="refusal">{n}</p>})}}
        {move||report.get().map(|r|{
            let book=r["book"].clone();
            let controls=r["controls"].as_array().cloned().unwrap_or_default();
            let agree=r["postedSourceControlsAgree"]==true;
            view!{<div data-testid="accounting-inventory-result">
                <h4>{format!("{} · {}",text(&book,"code"),text(&r,"asOf"))}</h4>
                <p data-testid="accounting-inventory-verdict" class=if agree {"hint"} else {"refusal"}>{verdict(&r)}</p>
                <p class="hint" data-testid="accounting-inventory-pending">{format!("Captured receipt and stock cost sources not posted through this date: {}.",r["coverage"]["pendingMonetarySourceCount"])}</p>
                <div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-inventory-controls"><caption>"Inventory control comparison"</caption><thead><tr><th>"Account"</th><th>"Posted-source balance"</th><th>"Ledger balance"</th><th>"Difference"</th></tr></thead><tbody>
                    {controls.into_iter().map(|c|view!{<tr><td>{format!("{} · {}",text(&c,"code"),text(&c,"name"))}</td><td>{amount(&c["expectedMinor"],&book)}</td><td>{amount(&c["actualMinor"],&book)}</td><td>{amount(&c["differenceMinor"],&book)}</td></tr>}).collect_view()}
                </tbody></table></div>
                <p class="hint">{text(&r,"notice")}</p>
            </div>}
        })}
    </section>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_verdict_follows_the_report() {
        assert_eq!(
            verdict(&json!({"controls":[],"postedSourceControlsAgree":true})),
            "No inventory account is declared for this book yet."
        );
        let row = json!({"code":"INVENTORY"});
        assert_eq!(
            verdict(&json!({"controls":[row],"postedSourceControlsAgree":true})),
            "Posted stock and receipt sources explain every inventory account balance."
        );
        assert_eq!(
            verdict(&json!({"controls":[row],"postedSourceControlsAgree":false})),
            "Some inventory account balances are not explained by posted sources. Review the differences."
        );
    }
}
