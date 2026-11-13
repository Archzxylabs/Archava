//! Business-party source registration, independent roles and reviewed refresh.
use super::*;
#[component]
pub(super) fn Registry(work: Workbench) -> impl IntoView {
    let organization = RwSignal::new(false);
    let selected = RwSignal::new(None::<Value>);
    let customer = RwSignal::new(true);
    let supplier = RwSignal::new(false);
    Effect::new(move |_| {
        organization.track();
        selected.set_if_alive(None);
    });
    view! {<details class="ops-disclosure"><summary>"Register a business party"</summary><div hidden=move||!work.permits("parties.write")><form class="ops-form" on:submit=move|event|{
     event.prevent_default();let Some(source)=selected.get_untracked()else{work.fail("Choose and review a source record.");return};
     let mut command=json!({"operation":if organization.get_untracked(){"party.create_from_organization"}else{"party.create_from_contact"},"expectedSourceRevision":source["revision"],"customer":customer.get_untracked(),"supplier":supplier.get_untracked()});
     command[if organization.get_untracked(){"organizationId"}else{"contactId"}]=source["id"].clone();work.send(command);
    }>
    <p class="hint">"Register one stable business identity from a reviewed source. A contact and an organization remain separate identities, even when their names match."</p>
    <label class="ops-check"><input type="checkbox" data-testid="party-organization-source" prop:checked=move||organization.get() on:change=move|e|organization.set_if_alive(event_target_checked(&e))/><span>"Use an organization source"</span></label>
    {move||if organization.get(){view!{<customer::CustomerPicker selected organization=true party=true/>}.into_any()}else{view!{<customer::CustomerPicker selected affiliation=true party=true/>}.into_any()}}
    <label class="ops-check"><input type="checkbox" data-testid="party-customer" prop:checked=move||customer.get() on:change=move|e|customer.set_if_alive(event_target_checked(&e))/><span>"Customer role"</span></label>
    <label class="ops-check"><input type="checkbox" data-testid="party-supplier" prop:checked=move||supplier.get() on:change=move|e|supplier.set_if_alive(event_target_checked(&e))/><span>"Supplier role"</span></label>
    <p class="hint">"Choose at least one role. Existing sales and purchasing records keep their current links; this registers the master identity."</p>
    <button type="submit" class="btn" data-testid="party-save" disabled=move||work.blocked()||(!customer.get()&&!supplier.get())>"Register reviewed party"</button>
    </form></div></details><Profile work/>}
}
#[component]
fn Profile(work: Workbench) -> impl IntoView {
    let selected = RwSignal::new(String::new());
    let options = Signal::derive(move || {
        work.rows("parties")
            .into_iter()
            .map(|row| {
                (
                    text(&row, "id"),
                    format!("{} · {}", text(&row, "name"), short(&text(&row, "id"))),
                )
            })
            .collect()
    });
    let current = Memo::new(move |_| {
        work.rows("parties")
            .into_iter()
            .find(|row| row["id"] == selected.get())
    });
    let customer = RwSignal::new(false);
    let supplier = RwSignal::new(false);
    let reason = RwSignal::new(String::new());
    let reviewed = RwSignal::new(None::<Value>);
    let loading = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let sequence = RwSignal::new(0u64);
    Effect::new(move |_| {
        let row = current.get();
        customer.set_if_alive(row.as_ref().is_some_and(|r| r["customer"] == true));
        supplier.set_if_alive(row.as_ref().is_some_and(|r| r["supplier"] == true));
        reason.set_if_alive(String::new());
        reviewed.set_if_alive(None);
        loading.set_if_alive(false);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|n| *n += 1);
    });
    view! {<details class="ops-disclosure"><summary>"Business-party profiles"</summary><div class="ops-form">
    <p class="hint">"Uses the loaded Business parties page. Use collection search and Next page for older records. Role changes keep the same party ID; clearing both roles makes it inactive for this master."</p>
    <Pick label="Registered business party" id="party-profile" value=selected options/>
    {move||current.get().map(|row|{let contact=row["sourceContactId"].as_str().map(str::to_owned);let source=contact.clone().unwrap_or_else(||text(&row,"sourceOrganizationId"));view!{<p data-testid="party-profile-current">{format!("{} · party revision {} · captured source revision {}",text(&row,"name"),integer(&row,"revision"),integer(&row,"sourceRevision"))}</p><p class="hint">{if contact.is_some(){"CRM contact source"}else{"Organization source"}}" · "<code>{source.clone()}</code></p>{contact.map(|id|view!{<a href=format!("/app/contact?id={id}")>"Open CRM source"</a>})}}})}
    <div hidden=move||!work.permits("parties.write")>
    <form class="ops-form" on:submit=move|e|{e.prevent_default();let Some(row)=current.get_untracked()else{return};work.send(json!({"operation":"party.set_roles","id":row["id"],"expectedRevision":row["revision"],"customer":customer.get_untracked(),"supplier":supplier.get_untracked(),"reason":reason.get_untracked()}));}>
    <label class="ops-check"><input type="checkbox" data-testid="party-profile-customer" prop:checked=move||customer.get() on:change=move|e|customer.set_if_alive(event_target_checked(&e))/><span>"Customer role"</span></label>
    <label class="ops-check"><input type="checkbox" data-testid="party-profile-supplier" prop:checked=move||supplier.get() on:change=move|e|supplier.set_if_alive(event_target_checked(&e))/><span>"Supplier role"</span></label>
    <Field label="Change reason" id="party-profile-reason" value=reason/>
    <button type="submit" class="btn" data-testid="party-roles-save" disabled=move||work.blocked()||current.get().is_none_or(|r|r["customer"]==customer.get()&&r["supplier"]==supplier.get())>"Save reviewed roles"</button></form>
    <button type="button" class="btn secondary" data-testid="party-source-load" disabled=move||work.blocked()||loading.get()||current.get().is_none() on:click=move|_|{
     let Some(row)=current.get_untracked()else{return};let Some(held)=session::held()else{return};let contact=row["sourceContactId"].as_str().map(str::to_owned);let source=contact.clone().unwrap_or_else(||text(&row,"sourceOrganizationId"));
     let path=if contact.is_some(){format!("/crm/records?tenant={}&contact={}",crate::contacts::query_encode(&held.tenant),crate::contacts::query_encode(&source))}else{format!("/erp/organizations/workspace?tenant={}&collection=organizations&id={}",crate::contacts::query_encode(&held.tenant),crate::contacts::query_encode(&source))};
     sequence.update_if_alive(|n|*n+=1);let request=sequence.get_untracked();loading.set_if_alive(true);reviewed.set_if_alive(None);notice.set_if_alive(String::new());
     spawn_local(async move{let answer=session::api_fetch("GET",&path,None).await;if sequence.is_disposed()||sequence.get_untracked()!=request||current.get_untracked()!=Some(row.clone()){return}loading.set_if_alive(false);match answer{Ok(answer)if answer.status==200=>{let value=if contact.is_some(){answer.body["contact"].clone()}else{answer.body["organizations"].get(0).cloned().unwrap_or(Value::Null)};if value["id"]==source{reviewed.set_if_alive(Some(value));}else{notice.set_if_alive("The source is unavailable.".into());}},_=>notice.set_if_alive("The source could not be loaded. Check your access and try again.".into())}});
    }>"Load current source for review"</button>
    {move||reviewed.get().map(|source|view!{<p class="hint" data-testid="party-source-preview">{format!("Review: {} · source revision {}",source["fullName"].as_str().or_else(||source["name"].as_str()).unwrap_or(""),integer(&source,"revision"))}</p>})}
    <p class="hint">"Refreshing changes the master profile and preserves its previous versions in history. It does not rewrite existing transactions."</p>
    <button type="button" class="btn" data-testid="party-source-save" disabled=move||work.blocked()||loading.get()||current.get().zip(reviewed.get()).is_none_or(|(p,s)|integer(&s,"revision")<=integer(&p,"sourceRevision")) on:click=move|_|{let(Some(row),Some(source))=(current.get_untracked(),reviewed.get_untracked())else{return};work.send(json!({"operation":"party.refresh_source","id":row["id"],"expectedRevision":row["revision"],"expectedSourceRevision":source["revision"],"reason":reason.get_untracked()}));}>"Apply reviewed source profile"</button>
    </div><p class="hint" role="status" data-testid="party-profile-status">{move||notice.get()}</p></div></details>}
}
