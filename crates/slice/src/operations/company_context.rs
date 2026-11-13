//! Explicit company review for trade; history includes inactive companies.
use super::*;

#[derive(Clone, Copy)]
struct Companies {
    rows: RwSignal<Vec<Value>>,
    query: RwSignal<String>,
    applied: RwSignal<String>,
    next: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
    notice: RwSignal<String>,
    sequence: RwSignal<u64>,
}
impl Companies {
    fn load(self, cursor: Option<String>, selected: Option<RwSignal<Option<Value>>>) {
        let Some(held) = session::held() else { return };
        let reviewed = selected.and_then(|signal| signal.get_untracked());
        if selected.is_some() && reviewed.is_none() {
            return;
        }
        let query = if selected.is_some() {
            String::new()
        } else if cursor.is_some() {
            self.applied.get_untracked()
        } else {
            self.query.get_untracked()
        };
        let mut path = format!(
            "/erp/organizations/workspace?tenant={}&collection=companies&limit=100&search={}",
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&query)
        );
        if let Some(cursor) = cursor {
            path.push_str(&format!(
                "&cursor={}",
                crate::contacts::query_encode(&cursor)
            ));
        }
        if let Some(ref row) = reviewed {
            path.push_str(&format!(
                "&id={}",
                crate::contacts::query_encode(&text(row, "id"))
            ));
        }
        self.sequence.update_if_alive(|n| *n += 1);
        let sequence = self.sequence.get_untracked();
        self.loading.set_if_alive(true);
        self.notice.set_if_alive(String::new());
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if self.sequence.is_disposed() || self.sequence.get_untracked() != sequence {
                return;
            }
            self.loading.set_if_alive(false);
            match answer {
                Ok(answer) if answer.status == 200 => {
                    let Some(rows) = answer.body["companies"].as_array() else {
                        self.notice
                            .set_if_alive("The company list could not be verified.".into());
                        return;
                    };
                    if let Some(selected) = selected {
                        if selected.get_untracked() != reviewed {
                            return;
                        }
                        if let Some(row) = rows
                            .iter()
                            .find(|row| reviewed.as_ref().is_some_and(|old| row["id"] == old["id"]))
                        {
                            // A reviewed refresh also updates this page's cached choice, so
                            // selecting it again cannot silently restore an older revision.
                            self.rows.update_if_alive(|rows| {
                                if let Some(cached) =
                                    rows.iter_mut().find(|cached| cached["id"] == row["id"])
                                {
                                    *cached = row.clone();
                                }
                            });
                            selected.set_if_alive(Some(row.clone()));
                            self.notice.set_if_alive("Company refreshed. Review its official name and version before submitting.".into());
                        } else {
                            // Keep the reviewed scope when access disappears; never fall back to legacy data.
                            self.notice.set_if_alive(
                                "The selected company is unavailable. Choose an accessible company."
                                    .into(),
                            );
                        }
                    } else {
                        self.rows.set_if_alive(rows.clone());
                        self.applied.set_if_alive(query);
                        self.next.set_if_alive(
                            answer.body["collections"]
                                .as_array()
                                .and_then(|rows| rows.iter().find(|row| row["id"] == "companies"))
                                .and_then(|row| row["nextCursor"].as_str())
                                .map(str::to_owned),
                        );
                    }
                }
                _ => {
                    if let Some(selected) = selected {
                        if selected.get_untracked() == reviewed {
                            // Keep the reviewed scope when access disappears; never fall back to legacy data.
                        }
                    }
                    self.rows.set_if_alive(Vec::new());
                    self.next.set_if_alive(None);
                    self.notice.set_if_alive(
                        "Companies could not be loaded. Check your access and try again.".into(),
                    );
                }
            }
        });
    }
}

#[component]
pub(crate) fn CompanyPicker(
    selected: RwSignal<Option<Value>>,
    blocked: Signal<bool>,
    #[prop(optional)] on_change: Option<Callback<()>>,
) -> impl IntoView {
    let companies = Companies {
        rows: RwSignal::new(Vec::new()),
        query: RwSignal::new(String::new()),
        applied: RwSignal::new(String::new()),
        next: RwSignal::new(None),
        loading: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        sequence: RwSignal::new(0),
    };
    let (search, find, next, select, picked, refresh, status) = (
        "trade-company-search",
        "trade-company-find",
        "trade-company-next",
        "trade-company-select",
        "trade-company-selected",
        "trade-company-refresh",
        "trade-company-status",
    );
    Effect::new(move |_| companies.load(None, None));
    let choices = Memo::new(move |_| {
        let mut rows = companies.rows.get().into_iter().collect::<Vec<_>>();
        if let Some(current) = selected.get() {
            rows.retain(|row| row["id"] != current["id"]);
            rows.insert(0, current);
        }
        rows
    });
    view! {<section class="ops-form">
        <div on:keydown=move|event|{if event.key()=="Enter" {event.prevent_default();event.stop_propagation();if !companies.loading.get_untracked(){companies.load(None,None);}}}><Field label="Find managed companies" id=search value=companies.query required=false/></div>
        <div class="ops-search"><button type="button" class="btn secondary" data-testid=find disabled=move||companies.loading.get()||blocked.get() on:click=move|_|companies.load(None,None)>"Find companies"</button><button type="button" class="btn secondary" data-testid=next disabled=move||companies.loading.get()||blocked.get()||companies.next.get().is_none() on:click=move|_|companies.load(companies.next.get_untracked(),None)>"Next page"</button></div>
        <p class="hint">"Up to 100 accessible companies per page. Inactive companies remain available for history. Different companies use separate product IDs; SKU codes remain unique across this workspace."</p>
        <label class="ops-field"><span>"Operate as company"</span><select data-testid=select disabled=move||blocked.get() prop:value=move||selected.get().map(|row|text(&row,"id")).unwrap_or_default() on:change=move|event|{if blocked.get(){return}let id=event_target_value(&event);if let Some(on_change)=on_change{on_change.run(());}companies.notice.set_if_alive(String::new());selected.set_if_alive(choices.get_untracked().into_iter().find(|row|row["id"]==id));}>
            <option value="" prop:selected=move||selected.get().is_none()>"Unassigned legacy workspace"</option>
            {move||choices.get().into_iter().map(|row|{let id=text(&row,"id");let checked=id.clone();let caption=format!("{} · {} · {} · v{}{}",text(&row,"code"),text(&row,"legalName"),short(&id),integer(&row,"revision"),if row["active"]==true{""}else{" · inactive"});view!{<option value=id prop:selected=move||selected.get().as_ref().is_some_and(|row|row["id"]==checked)>{caption}</option>}}).collect::<Vec<_>>()}
        </select></label>
        <p class="hint" data-testid=picked>{move||selected.get().map(|row|format!("Reviewed: {} · company {} · version {} · {}",text(&row,"legalName"),text(&row,"id"),integer(&row,"revision"),if row["active"]==true{"active"}else{"inactive"})).unwrap_or_default()}</p>
        <button type="button" class="btn secondary" data-testid=refresh disabled=move||companies.loading.get()||blocked.get()||selected.get().is_none() on:click=move|_|companies.load(None,Some(selected))>"Refresh selected company"</button>
        <p class="hint" role="status" data-testid=status>{move||companies.notice.get()}</p>
        <p class="hint">"New business checks the reviewed company version. Existing documents keep their captured identity. Unassigned records have no company attribution."</p>
    </section>}
}

#[component]
pub(super) fn Identity(row: Value) -> impl IntoView {
    row["companyId"].as_str().map(|id|view!{<p class="hint" data-testid="document-company-identity">{format!("{} · {} · company {} · captured version {}",text(&row,"companyCode"),text(&row,"companyName"),id,integer(&row,"companyRevision"))}</p>})
}
