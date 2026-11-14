//! Supplier role registration and deliberate refresh from its immutable CRM source.
use super::*;

#[component]
pub(super) fn Registration(work: Workbench) -> impl IntoView {
    let code = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let linked = RwSignal::new(false);
    let use_party = RwSignal::new(false);
    let party = RwSignal::new(None::<Value>);
    let contact = RwSignal::new(None::<Value>);
    view! {<details class="ops-disclosure"><summary>"1. Register supplier"</summary>
    <div hidden=move||!work.permits("suppliers.write")>
    <form class="ops-form" on:submit=move|event|{
        event.prevent_default();
        if use_party.get_untracked() {
            let Some(source)=party.get_untracked() else {work.fail("Choose a registered supplier party.");return};
            work.send(json!({"operation":"supplier.create-from-party","code":code.get_untracked(),"partyId":source["id"],"expectedPartyRevision":source["revision"]}));
        } else if linked.get_untracked() {
            let Some(source)=contact.get_untracked() else {work.fail("Choose a CRM supplier contact.");return};
            work.send(json!({"operation":"supplier.create-from-contact","code":code.get_untracked(),"contactId":source["id"],"expectedContactRevision":source["revision"]}));
        } else {
            if name.get_untracked().trim().is_empty(){work.fail("Enter the supplier name.");return}
            work.send(json!({"operation":"supplier.create","code":code.get_untracked(),"name":name.get_untracked()}));
        }
    }>
        <Field label="Supplier code" id="supplier-code" value=code/>
        <label class="ops-check"><input type="checkbox" data-testid="supplier-link-contact" prop:checked=move||linked.get() on:change=move|event|{linked.set(event_target_checked(&event));use_party.set(false);}/><span>"Link a CRM supplier contact"</span></label>
        <label class="ops-check"><input type="checkbox" data-testid="supplier-link-party" prop:checked=move||use_party.get() on:change=move|event|{use_party.set(event_target_checked(&event));linked.set(false);}/><span>"Use a registered business party"</span></label>
        {move||if use_party.get(){view!{<party_selection::PartyPicker selected=party supplier=true/>}.into_any()}else if linked.get(){view!{<customer::CustomerPicker selected=contact supplier=true/>}.into_any()}else{view!{<Field label="Supplier name" id="supplier-name" value=name/>}.into_any()}}
        <p class="hint">"A contact can have both customer and supplier roles. A shared contact does not establish a legal entity or bank account."</p>
        <button class="btn" type="submit" data-testid="supplier-save" disabled=move||work.blocked()>"Create supplier"</button>
    </form></div></details>}
}

#[component]
pub(super) fn Profile(work: Workbench) -> impl IntoView {
    let selected = RwSignal::new(String::new());
    let options = Signal::derive(move || {
        work.rows("suppliers")
            .into_iter()
            .map(|row| {
                (
                    text(&row, "id"),
                    format!("{} · {}", text(&row, "code"), text(&row, "name")),
                )
            })
            .collect()
    });
    let current = Memo::new(move |_| {
        work.rows("suppliers")
            .into_iter()
            .find(|row| row["id"] == selected.get())
    });
    let reviewed = RwSignal::new(None::<Value>);
    let reason = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let sequence = RwSignal::new(0u64);
    Effect::new(move |_| {
        current.track();
        sequence.update_if_alive(|n| *n += 1);
        reviewed.set_if_alive(None);
        reason.set_if_alive(String::new());
        notice.set_if_alive(String::new());
        loading.set_if_alive(false);
    });
    let can_apply = move || {
        let Some(supplier) = current.get() else {
            return false;
        };
        let Some(contact) = reviewed.get() else {
            return false;
        };
        if supplier["supplierPartyId"].is_string() {
            supplier["supplierPartyId"] == contact["id"]
                && contact["supplier"] == true
                && integer(&contact, "revision") > integer(&supplier, "supplierPartyRevision")
        } else {
            supplier["supplierContactId"] == contact["id"]
                && integer(&contact, "revision") > integer(&supplier, "supplierContactRevision")
        }
    };
    view! {<details class="ops-disclosure"><summary>"Supplier profiles"</summary>
    <div class="ops-form">
        <p class="hint">"Showing up to 100 matching suppliers. Use the workspace search to find older records. Refresh changes the supplier profile; existing purchases and receipts keep their original history."</p>
        <Pick label="Registered supplier" id="supplier-profile" value=selected options/>
        {move||current.get().map(|supplier|{
            let source=text(&supplier,"supplierContactId");
            view!{<p data-testid="supplier-profile-current">{format!("Registered: {} · supplier revision {}",text(&supplier,"name"),integer(&supplier,"revision"))}</p>
                {if supplier["supplierPartyId"].is_string(){view!{<p class="hint" data-testid="supplier-party-identity">{format!("Party {} · captured version {}. New offers and purchases require the current active supplier role and version. Review and refresh this profile after a party change.",text(&supplier,"supplierPartyId"),integer(&supplier,"supplierPartyRevision"))}</p>}.into_any()}else if source.is_empty(){view!{<p class="hint">"This supplier has no CRM source. A matching name does not link an existing record."</p>}.into_any()}else{view!{<p class="hint"><a href=format!("/app/contact?id={source}")>"View linked CRM supplier contact"</a>{format!(" · captured CRM revision {}",integer(&supplier,"supplierContactRevision"))}</p>}.into_any()}}
            }
        })}
        <div hidden=move||!work.permits("suppliers.write")>
        <button type="button" class="btn secondary" data-testid="supplier-profile-load" disabled=move||loading.get()||work.blocked()||current.get().is_none_or(|row|!row["supplierContactId"].is_string()&&!row["supplierPartyId"].is_string()) on:click=move|_|{
            let Some(supplier)=current.get_untracked() else{return};
            let is_party=supplier["supplierPartyId"].is_string();
            let source=text(&supplier,if is_party{"supplierPartyId"}else{"supplierContactId"});
            if source.is_empty(){return}
            let Some(held)=session::held() else{return};
            sequence.update_if_alive(|n|*n+=1);let request=sequence.get_untracked();
            loading.set_if_alive(true);reviewed.set_if_alive(None);notice.set_if_alive(String::new());
            let path=if is_party{format!("/erp/organizations/workspace?tenant={}&collection=parties&id={}",crate::contacts::query_encode(&held.tenant),crate::contacts::query_encode(&source))}else{format!("/crm/records?tenant={}&contact={}",crate::contacts::query_encode(&held.tenant),crate::contacts::query_encode(&source))};
            spawn_local(async move{
                let answer=session::api_fetch("GET",&path,None).await;
                if sequence.is_disposed()||sequence.get_untracked()!=request||current.get_untracked()!=Some(supplier.clone()){return}
                loading.set_if_alive(false);
                match answer {
                    Ok(answer) if answer.status==200=>{
                        let contact=if is_party{answer.body["parties"].get(0).cloned().unwrap_or(Value::Null)}else{answer.body["contact"].clone()};
                        if contact["id"]!=source {notice.set_if_alive("The source is unavailable.".into());return}
                        if integer(&contact,"revision")<=integer(&supplier,if is_party{"supplierPartyRevision"}else{"supplierContactRevision"}) {notice.set_if_alive("The supplier already has this source revision.".into());}
                        if is_party && contact["supplier"]!=true {notice.set_if_alive("The party supplier role is inactive.".into());}
                        reviewed.set_if_alive(Some(contact));
                    }
                    _=>notice.set_if_alive("The source could not be loaded. Check your access and try again.".into()),
                }
            });
        }>{move||if current.get().is_some_and(|row|row["supplierPartyId"].is_string()){"Load current party profile"}else{"Load current CRM profile"}}</button>
        {move||reviewed.get().map(|contact|view!{<p class="hint" data-testid="supplier-profile-preview">{if contact["name"].is_string(){format!("Review: {} · party {} · version {} · supplier role {}",text(&contact,"name"),text(&contact,"id"),integer(&contact,"revision"),if contact["supplier"]==true{"active"}else{"inactive"})}else{format!("Review: {} · {} · {} · CRM revision {}",text(&contact,"fullName"),text(&contact,"organization"),text(&contact,"email"),integer(&contact,"revision"))}}</p>})}
        <form class="ops-form" on:submit=move|event|{
            event.prevent_default();if !can_apply(){work.fail("Load and review a newer active source profile first.");return}
            let Some(supplier)=current.get_untracked() else{return};
            let Some(contact)=reviewed.get_untracked() else{return};
            work.send(if supplier["supplierPartyId"].is_string(){json!({"operation":"supplier.refresh-party","id":supplier["id"],"expectedRevision":supplier["revision"],"expectedPartyRevision":contact["revision"],"reason":reason.get_untracked()})}else{json!({"operation":"supplier.refresh-contact","id":supplier["id"],"expectedRevision":supplier["revision"],"expectedContactRevision":contact["revision"],"reason":reason.get_untracked()})});
        }>
            <Field label="Reason for profile refresh" id="supplier-profile-reason" value=reason/>
            <button class="btn" type="submit" data-testid="supplier-profile-apply" disabled=move||loading.get()||work.blocked()||!can_apply()>"Apply reviewed profile"</button>
        </form></div>
        <p class="hint" role="status" data-testid="supplier-profile-status">{move||notice.get()}</p>
    </div></details>}
}
