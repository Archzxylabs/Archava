//! Human whole-book totals with separately paged original-document detail.
use super::*;

#[derive(Clone, Copy)]
struct Reader {
    work: Workbench,
    date: RwSignal<String>,
    side: RwSignal<String>,
    report: RwSignal<Option<Value>>,
    notice: RwSignal<String>,
    busy: RwSignal<bool>,
    sequence: RwSignal<u64>,
}
impl Reader {
    fn load(self, cursor: Option<String>) {
        let Some(held) = session::held() else {
            return;
        };
        let book = text(&self.work.data.get_untracked()["configuredBook"], "id");
        let date = self.date.get_untracked();
        let side = self.side.get_untracked();
        if book.is_empty()
            || archava_domain::erp::accommodation::parse_calendar_date(&date).is_none()
        {
            self.notice
                .set_if_alive("Select a company book and a valid report date.".into());
            return;
        }
        let company = self.work.company_key.get_untracked();
        self.sequence.update_if_alive(|v| *v += 1);
        let sequence = self.sequence.get_untracked();
        self.busy.set_if_alive(true);
        self.notice.set_if_alive(String::new());
        let mut path=format!("/erp/accounting/subledger-report?tenant={}&bookId={book}&asOf={date}&side={side}&limit=25",crate::contacts::query_encode(&held.tenant));
        if let Some(cursor) = cursor {
            path.push_str(&format!(
                "&cursor={}",
                crate::contacts::query_encode(&cursor)
            ));
        }
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if self.sequence.is_disposed()
                || self.sequence.get_untracked() != sequence
                || self.work.company_key.get_untracked() != company
            {
                return;
            }
            self.busy.set_if_alive(false);
            match answer {
                Ok(v) if v.status == 200 => self.report.set_if_alive(Some(v.body)),
                Ok(v) => {
                    self.report.set_if_alive(None);
                    self.notice.set_if_alive(message(&v));
                }
                Err(_) => {
                    self.report.set_if_alive(None);
                    self.notice.set_if_alive(
                        "The report could not be loaded. Refresh and try again.".into(),
                    );
                }
            }
        });
    }
}
fn bucket(row: &Value) -> String {
    match row["bucket"].as_str() {
        Some("settled") => "Settled".into(),
        Some("credit") => "Credit balance".into(),
        Some("unknown_terms") => "Due date unknown".into(),
        Some("current") => "Not overdue".into(),
        Some("overdue1_to30") => "1–30 days overdue".into(),
        Some("overdue31_to60") => "31–60 days overdue".into(),
        Some("overdue61_to90") => "61–90 days overdue".into(),
        Some("overdue_over90") => "Over 90 days overdue".into(),
        _ => "Unavailable".into(),
    }
}

#[component]
pub(super) fn AgingReports(work: Workbench) -> impl IntoView {
    let reader = Reader {
        work,
        date: RwSignal::new(String::new()),
        side: RwSignal::new("receivable".into()),
        report: RwSignal::new(None),
        notice: RwSignal::new(String::new()),
        busy: RwSignal::new(false),
        sequence: RwSignal::new(0),
    };
    Effect::new(move |_| {
        let _ = work.data.get();
        let _ = reader.date.get();
        let _ = reader.side.get();
        reader.report.set_if_alive(None);
        reader.notice.set_if_alive(String::new());
        reader.sequence.update_if_alive(|v| *v += 1);
        reader.busy.set_if_alive(false);
    });
    view! {<section class="ops-detail" data-testid="accounting-aging-reports"><h3>"Receivables and payables by due date"</h3>
        <p class="hint">"Balances use all posted effects through the chosen date. Unknown due dates and credit balances are shown separately. Changing the page does not change the whole-book totals."</p>
        <form class="ops-form" on:submit=move|e:leptos::ev::SubmitEvent|{e.prevent_default();reader.load(None);}>
            <label class="ops-field"><span>"Report"</span><select data-testid="accounting-aging-side" prop:value=move||reader.side.get() on:change=move|e|reader.side.set_if_alive(event_target_value(&e))>
                <option value="receivable">"Customer receivables"</option><option value="payable" disabled=move||!work.permits("payables.read")>"Supplier payables"</option>
            </select></label>
            <label class="ops-field"><span>"Balance as of"</span><input data-testid="accounting-aging-date" type="date" min="2000-01-01" max="2100-12-31" required prop:value=move||reader.date.get() on:input=move|e|reader.date.set_if_alive(event_target_value(&e))/></label>
            <button type="submit" class="btn secondary" data-testid="accounting-aging-run" disabled=move||reader.busy.get()||!work.permits("reports.read")||!work.permits("terms.read")>"Refresh due-date report"</button>
        </form>
        {move||{let notice=reader.notice.get();(!notice.is_empty()).then(||view!{<p class="refusal">{notice}</p>})}}
        {move||reader.report.get().map(|r|{
            let book=r["book"].clone();let totals=r["totals"].clone();
            let documents=r["positions"].as_array().cloned().unwrap_or_default();let document_book=book.clone();
            let controls=r["controls"].as_array().cloned().unwrap_or_default();let control_book=book.clone();
            view!{<div data-testid="accounting-aging-result"><h4>{format!("{} · {}",text(&book,"code"),text(&r,"asOf"))}</h4>
                <p class="hint">{format!("Whole book: {} qualifying documents. This page: {} documents.",r["coverage"]["qualifyingDocumentCount"],documents.len())}</p>
                <dl class="operator-details" data-testid="accounting-aging-totals">
                    {[("Unknown due date","unknownTermsMinor"),("Not overdue","currentMinor"),("1–30 days overdue","overdue1To30Minor"),("31–60 days overdue","overdue31To60Minor"),("61–90 days overdue","overdue61To90Minor"),("Over 90 days overdue","overdueOver90Minor"),("Positive outstanding","positiveOutstandingMinor"),("Credit balances","creditMinor"),("Net outstanding","netOutstandingMinor")].into_iter().map(|(label,key)|view!{<dt>{label}</dt><dd>{amount(&totals[key],&book)}</dd>}).collect::<Vec<_>>()}
                </dl>
                <p data-testid="accounting-aging-control-status" class=if r["postedSourceControlsAgree"]==true {"hint"} else {"refusal"}>{if r["postedSourceControlsAgree"]==true {"Posted source amounts agree with their declared ledger control accounts."} else {"Control differences need review. Unattributed entries have not been added to customer or supplier balances."}}</p>
                <p class="hint">{format!("Captured monetary sources not posted through this date: {}.",r["coverage"]["pendingMonetarySourceCount"])}</p>
                <div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-aging-controls"><caption>"Independent control comparison"</caption><thead><tr><th>"Account"</th><th>"Original-source balance"</th><th>"Ledger balance"</th><th>"Difference"</th></tr></thead><tbody>{controls.into_iter().map(|c|view!{<tr><td>{format!("{} · {}",text(&c,"code"),text(&c,"name"))}</td><td>{amount(&c["expectedMinor"],&control_book)}</td><td>{amount(&c["actualMinor"],&control_book)}</td><td>{amount(&c["differenceMinor"],&control_book)}</td></tr>}).collect::<Vec<_>>()}</tbody></table></div>
                <div class="ops-table-wrap"><table class="ops-table" data-testid="accounting-aging-documents"><caption>"Original document balances — current page"</caption><thead><tr><th>"Document"</th><th>"Source date"</th><th>"Agreed due date"</th><th>"Age"</th><th>"Balance"</th></tr></thead><tbody>{documents.into_iter().map(|d|view!{<tr><td>{text(&d,"code")}</td><td>{text(&d,"sourceDate")}</td><td>{d["terms"]["condition"]["date"].as_str().unwrap_or("Unknown").to_owned()}</td><td>{bucket(&d)}</td><td>{amount(&d["balanceMinor"],&document_book)}</td></tr>}).collect::<Vec<_>>()}</tbody></table></div>
                <div class="ops-actions"><button type="button" class="btn secondary" disabled=move||reader.busy.get() on:click=move|_|reader.load(None)>"First page"</button><button type="button" class="btn secondary" data-testid="accounting-aging-next" disabled=move||reader.busy.get()||!reader.report.get().is_some_and(|v|v["page"]["hasMore"]==true) on:click=move|_|reader.load(reader.report.get_untracked().and_then(|v|v["page"]["nextCursor"].as_str().map(str::to_owned)))>"Next page"</button></div>
                <p class="hint">{text(&r,"notice")}</p><p class="hint">"Terms use the latest reviewed history effective at this business date. Later authorized date corrections can change that view; this is not a record of what was known at a historical moment."</p>
            </div>}
        })}
    </section>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use archava_domain::erp::accounting::aging::Bucket;
    #[test]
    fn every_domain_bucket_serializes_to_its_actual_customer_label() {
        for (value, label) in [
            (Bucket::Settled, "Settled"),
            (Bucket::Credit, "Credit balance"),
            (Bucket::UnknownTerms, "Due date unknown"),
            (Bucket::Current, "Not overdue"),
            (Bucket::Overdue1To30, "1–30 days overdue"),
            (Bucket::Overdue31To60, "31–60 days overdue"),
            (Bucket::Overdue61To90, "61–90 days overdue"),
            (Bucket::OverdueOver90, "Over 90 days overdue"),
        ] {
            assert_eq!(bucket(&json!({"bucket":value})), label);
        }
        assert_eq!(bucket(&json!({"bucket":"unrecognized"})), "Unavailable");
    }
}
