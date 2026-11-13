//! Select a published business-party snapshot; a list refresh never changes the review.
use super::*;

#[derive(Clone, Copy)]
struct Parties {
    rows: RwSignal<Vec<Value>>,
    query: RwSignal<String>,
    applied: RwSignal<String>,
    next: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
    notice: RwSignal<String>,
    sequence: RwSignal<u64>,
}
impl Parties {
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
            "/erp/organizations/workspace?tenant={}&collection=parties&limit=100&search={}",
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
                    let Some(rows) = answer.body["parties"].as_array() else {
                        self.notice
                            .set_if_alive("The party list could not be verified.".into());
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
                            selected.set_if_alive(Some(row.clone()));
                            self.notice.set_if_alive("Party refreshed. Review its name, role and version before submitting.".into());
                        } else {
                            selected.set_if_alive(None);
                            self.notice.set_if_alive(
                                "The selected party is unavailable. Choose an accessible party."
                                    .into(),
                            );
                        }
                    } else {
                        self.rows.set_if_alive(rows.clone());
                        self.applied.set_if_alive(query);
                        self.next.set_if_alive(
                            answer.body["collections"]
                                .as_array()
                                .and_then(|rows| rows.iter().find(|row| row["id"] == "parties"))
                                .and_then(|row| row["nextCursor"].as_str())
                                .map(str::to_owned),
                        );
                    }
                }
                _ => {
                    if let Some(selected) = selected {
                        if selected.get_untracked() == reviewed {
                            selected.set_if_alive(None);
                        }
                    }
                    self.rows.set_if_alive(Vec::new());
                    self.next.set_if_alive(None);
                    self.notice.set_if_alive(
                        "Parties could not be loaded. Check your access and try again.".into(),
                    );
                }
            }
        });
    }
}

#[component]
pub(super) fn PartyPicker(
    selected: RwSignal<Option<Value>>,
    #[prop(default = false)] supplier: bool,
) -> impl IntoView {
    let parties = Parties {
        rows: RwSignal::new(Vec::new()),
        query: RwSignal::new(String::new()),
        applied: RwSignal::new(String::new()),
        next: RwSignal::new(None),
        loading: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        sequence: RwSignal::new(0),
    };
    let (search, find, next, select, picked, refresh, status) = if supplier {
        (
            "supplier-party-search",
            "supplier-party-find",
            "supplier-party-next",
            "supplier-party-select",
            "supplier-party-selected",
            "supplier-party-refresh",
            "supplier-party-status",
        )
    } else {
        (
            "customer-party-search",
            "customer-party-find",
            "customer-party-next",
            "customer-party-select",
            "customer-party-selected",
            "customer-party-refresh",
            "customer-party-status",
        )
    };
    Effect::new(move |_| parties.load(None, None));
    let role = if supplier { "supplier" } else { "customer" };
    let choices = Memo::new(move |_| {
        let mut rows = parties
            .rows
            .get()
            .into_iter()
            .filter(|row| row[role] == true)
            .collect::<Vec<_>>();
        if let Some(current) = selected.get() {
            rows.retain(|row| row["id"] != current["id"]);
            rows.insert(0, current);
        }
        rows
    });
    view! {<section class="ops-form">
        <div on:keydown=move|event|{if event.key()=="Enter" {event.prevent_default();event.stop_propagation();if !parties.loading.get_untracked(){parties.load(None,None);}}}><Field label="Find registered business parties" id=search value=parties.query required=false/></div>
        <div class="ops-search"><button type="button" class="btn secondary" data-testid=find disabled=move||parties.loading.get() on:click=move|_|parties.load(None,None)>"Find parties"</button><button type="button" class="btn secondary" data-testid=next disabled=move||parties.loading.get()||parties.next.get().is_none() on:click=move|_|parties.load(parties.next.get_untracked(),None)>"Next page"</button></div>
        <p class="hint">{format!("Up to 100 accessible parties per page; only active {role} roles are offered. Next page continues through the registry. Review the ID when names match.")}</p>
        <label class="ops-field"><span>"Registered business party"</span><select data-testid=select required prop:value=move||selected.get().map(|row|text(&row,"id")).unwrap_or_default() on:change=move|event|{let id=event_target_value(&event);selected.set_if_alive(choices.get_untracked().into_iter().find(|row|row["id"]==id));}>
            <option value="" prop:selected=move||selected.get().is_none()>"Choose a business party…"</option>
            {move||choices.get().into_iter().map(|row|{let id=text(&row,"id");let checked=id.clone();let caption=format!("{} · {} · v{}{}",text(&row,"name"),short(&id),integer(&row,"revision"),if row[role]==true{""}else{" · role inactive"});view!{<option value=id prop:selected=move||selected.get().as_ref().is_some_and(|row|row["id"]==checked)>{caption}</option>}}).collect::<Vec<_>>()}
        </select></label>
        <p class="hint" data-testid=picked>{move||selected.get().map(|row|format!("Reviewed: {} · party {} · version {} · {} role {}",text(&row,"name"),text(&row,"id"),integer(&row,"revision"),role,if row[role]==true{"active"}else{"inactive"})).unwrap_or_default()}</p>
        <button type="button" class="btn secondary" data-testid=refresh disabled=move||parties.loading.get()||selected.get().is_none() on:click=move|_|parties.load(None,Some(selected))>"Refresh selected party"</button>
        <p class="hint" role="status" data-testid=status>{move||parties.notice.get()}</p>
        <p class="hint">"The reviewed version is checked when you submit. Existing documents preserve their captured party identity."</p>
    </section>}
}
