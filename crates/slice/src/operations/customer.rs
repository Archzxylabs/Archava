//! CRM selection retains the exact reviewed contact identity and revision.
use super::*;

#[derive(Clone, Copy)]
struct Customers {
    organization: bool,
    rows: RwSignal<Vec<Value>>,
    query: RwSignal<String>,
    applied_query: RwSignal<String>,
    next: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
    notice: RwSignal<String>,
    sequence: RwSignal<u64>,
}
impl Customers {
    fn refresh_selected(self, selected: RwSignal<Option<Value>>) {
        let Some(contact) = selected.get_untracked() else {
            return;
        };
        let Some(held) = session::held() else { return };
        let id = text(&contact, "id");
        self.sequence.update_if_alive(|sequence| *sequence += 1);
        let sequence = self.sequence.get_untracked();
        self.loading.set_if_alive(true);
        self.notice.set_if_alive(String::new());
        let path = if self.organization {
            format!(
                "/erp/organizations/workspace?tenant={}&collection=organizations&id={}",
                crate::contacts::query_encode(&held.tenant),
                crate::contacts::query_encode(&id)
            )
        } else {
            format!(
                "/crm/records?tenant={}&contact={}",
                crate::contacts::query_encode(&held.tenant),
                crate::contacts::query_encode(&id)
            )
        };
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if self.sequence.is_disposed() || self.sequence.get_untracked() != sequence {
                return;
            }
            self.loading.set_if_alive(false);
            if selected.get_untracked().as_ref().map(|row| text(row, "id")) != Some(id.clone()) {
                return;
            }
            match answer {
                Ok(answer) if answer.status==200 => {
                    let row=if self.organization {answer.body["organizations"].get(0)}else{answer.body.get("contact")};
                    if let Some(row)=row.filter(|row|row["id"]==id) {
                        selected.set_if_alive(Some(row.clone()));
                        self.notice.set_if_alive("Record refreshed. Review the updated details before submitting.".into());
                    } else {self.notice.set_if_alive("The selected record is unavailable.".into());}
                },
                _ => self.notice.set_if_alive("The selected record could not be refreshed. Check your access or choose another record.".into()),
            }
        });
    }
    fn load(self, after: Option<String>) {
        let Some(held) = session::held() else { return };
        let query = if after.is_some() {
            self.applied_query.get_untracked()
        } else {
            self.query.get_untracked()
        };
        self.sequence.update_if_alive(|sequence| *sequence += 1);
        let sequence = self.sequence.get_untracked();
        self.loading.set_if_alive(true);
        self.notice.set_if_alive(String::new());
        let mut path = format!(
            "{}?tenant={}&search={}&limit=100{}",
            if self.organization {
                "/erp/organizations/workspace"
            } else {
                "/contacts"
            },
            crate::contacts::query_encode(&held.tenant),
            crate::contacts::query_encode(&query),
            if self.organization {
                "&collection=organizations"
            } else {
                ""
            }
        );
        if let Some(after) = after {
            path.push_str(&format!(
                "&{}={}",
                if self.organization { "cursor" } else { "after" },
                crate::contacts::query_encode(&after)
            ));
        }
        spawn_local(async move {
            let answer = session::api_fetch("GET", &path, None).await;
            if self.sequence.is_disposed() || self.sequence.get_untracked() != sequence {
                return;
            }
            self.loading.set_if_alive(false);
            match answer {
                Ok(answer) if answer.status == 200 => {
                    if let Some(rows) = answer.body[if self.organization {
                        "organizations"
                    } else {
                        "items"
                    }]
                    .as_array()
                    {
                        self.rows.set_if_alive(rows.clone());
                        self.applied_query.set_if_alive(query);
                        let next = if self.organization {
                            answer.body["collections"]
                                .as_array()
                                .and_then(|rows| {
                                    rows.iter().find(|row| row["id"] == "organizations")
                                })
                                .and_then(|row| row["nextCursor"].as_str())
                        } else {
                            answer.body["next"].as_str()
                        };
                        self.next.set_if_alive(next.map(str::to_owned));
                    } else {
                        self.notice.set_if_alive(
                            "The record list could not be verified. Try again.".into(),
                        );
                    }
                }
                _ => {
                    self.rows.set_if_alive(Vec::new());
                    self.next.set_if_alive(None);
                    self.notice.set_if_alive(
                        "The record list is unavailable. Check your access and try again.".into(),
                    );
                }
            }
        });
    }
}
fn label(contact: &Value) -> String {
    let name = if contact["fullName"].is_string() {
        text(contact, "fullName")
    } else {
        text(contact, "name")
    };
    let details = [
        text(contact, "organization"),
        text(contact, "email"),
        text(contact, "legalName"),
        text(contact, "taxId"),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    if details.is_empty() {
        format!("{name} · {}", short(&text(contact, "id")))
    } else {
        format!("{name} · {details}")
    }
}

#[component]
pub(super) fn CustomerPicker(
    selected: RwSignal<Option<Value>>,
    #[prop(default = false)] supplier: bool,
    #[prop(default = false)] affiliation: bool,
    #[prop(default = false)] organization: bool,
    #[prop(default = false)] party: bool,
    #[prop(default = false)] company: bool,
) -> impl IntoView {
    let (root, search, find, next, status, select, picked, refresh) = if company {
        (
            "company-org-picker",
            "company-org-search",
            "company-org-find",
            "company-org-next",
            "company-org-status",
            "company-org-select",
            "company-org-selected",
            "company-org-refresh",
        )
    } else if party && organization {
        (
            "party-org-picker",
            "party-org-search",
            "party-org-find",
            "party-org-next",
            "party-org-status",
            "party-org-select",
            "party-org-selected",
            "party-org-refresh",
        )
    } else if party {
        (
            "party-contact-picker",
            "party-contact-search",
            "party-contact-find",
            "party-contact-next",
            "party-contact-status",
            "party-contact-select",
            "party-contact-selected",
            "party-contact-refresh",
        )
    } else if organization {
        (
            "affiliation-org-picker",
            "affiliation-org-search",
            "affiliation-org-find",
            "affiliation-org-next",
            "affiliation-org-status",
            "affiliation-org-select",
            "affiliation-org-selected",
            "affiliation-org-refresh",
        )
    } else if affiliation {
        (
            "affiliation-contact-picker",
            "affiliation-contact-search",
            "affiliation-contact-find",
            "affiliation-contact-next",
            "affiliation-contact-status",
            "affiliation-contact-select",
            "affiliation-contact-selected",
            "affiliation-contact-refresh",
        )
    } else if supplier {
        (
            "supplier-contact-picker",
            "supplier-contact-search",
            "supplier-contact-find",
            "supplier-contact-next",
            "supplier-contact-status",
            "supplier-contact-select",
            "supplier-contact-selected",
            "supplier-contact-refresh",
        )
    } else {
        (
            "customer-picker",
            "customer-search",
            "customer-find",
            "customer-next",
            "customer-search-status",
            "order-crm-customer",
            "customer-selected",
            "customer-refresh-selected",
        )
    };
    let customers = Customers {
        organization,
        rows: RwSignal::new(Vec::new()),
        query: RwSignal::new(String::new()),
        applied_query: RwSignal::new(String::new()),
        next: RwSignal::new(None),
        loading: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        sequence: RwSignal::new(0),
    };
    Effect::new(move |_| customers.load(None));
    let choices = Memo::new(move |_| {
        let mut rows = customers.rows.get();
        if let Some(current) = selected.get() {
            rows.retain(|row| row["id"] != current["id"]);
            rows.insert(0, current);
        }
        rows
    });
    view! {<section data-testid=root class="ops-form">
        <div on:keydown=move|event|{
            if event.key()=="Enter" {
                event.prevent_default();event.stop_propagation();
                if !customers.loading.get_untracked(){customers.load(None);}
            }
        }><Field label=if organization {"Find organizations"}else if affiliation {"Find CRM contacts by name"}else if supplier {"Find CRM supplier contacts by name"} else {"Find CRM customers by name"} id=search value=customers.query required=false/></div>
        <div class="ops-search">
            <button type="button" class="btn secondary" data-testid=find disabled=move||customers.loading.get() on:click=move|_|customers.load(None)>{if organization {"Find organizations"}else{"Find contacts"}}</button>
            <button type="button" class="btn secondary" data-testid=next disabled=move||customers.loading.get()||customers.next.get().is_none() on:click=move|_|customers.load(customers.next.get_untracked())>"Next page"</button>
        </div>
        <p class="hint">{if organization {"Up to 100 matching organizations per page. Review the legal name or tax reference when names match."}else{"Up to 100 matches per page. Use the organization or email to distinguish contacts with the same name."}}</p>
        <p class="hint" role="status" data-testid=status>{move||customers.notice.get()}</p>
        <label class="ops-field"><span>{if organization {"Organization"}else if affiliation {"CRM contact"}else if supplier {"CRM supplier contact"} else {"CRM customer"}}</span>
            <select data-testid=select prop:value=move||selected.get().map(|row|text(&row,"id")).unwrap_or_default()
                on:change=move|event|{let id=event_target_value(&event);customers.notice.set_if_alive(String::new());selected.set_if_alive(choices.get_untracked().into_iter().find(|row|row["id"]==id));} required>
                <option value="" prop:selected=move||selected.get().is_none()>{if organization {"Choose an organization…"}else{"Choose a CRM contact…"}}</option>
                {move||choices.get().into_iter().map(|row|{let id=text(&row,"id");let checked=id.clone();let caption=label(&row);view!{<option value=id prop:selected=move||selected.get().as_ref().is_some_and(|row|row["id"]==checked)>{caption}</option>}}).collect::<Vec<_>>()}
            </select>
        </label>
        <p class="hint">{if company {"Review the official name and reference. The source version is checked when you register this managed company; refresh the selected organization if it changes."}else if party {"The selected source is linked by ID. Its reviewed revision is checked when this party is registered. Refresh it if the details change."}else if organization || affiliation {"The selected organization and CRM contact are checked against the revisions you reviewed. Refresh a selected record if its details change."}else if supplier {"The selected contact is linked by ID. Its name is captured on registration. Supplier profile refresh is explicit; existing purchase orders keep their original snapshot."} else {"The selected CRM record is linked by ID. Its name is captured when the order is created; later profile edits preserve that history."}}</p>
        <p class="hint" data-testid=picked>{move||selected.get().as_ref().map(|row|format!("Selected record: {}",label(row))).unwrap_or_default()}</p>
        <button type="button" class="btn secondary" data-testid=refresh disabled=move||customers.loading.get()||selected.get().is_none() on:click=move|_|customers.refresh_selected(selected)>{if organization {"Refresh selected organization"}else{"Refresh selected contact"}}</button>
    </section>}
}
