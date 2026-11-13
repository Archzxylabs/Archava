//! Reviewed organization/contact relationships and explicit legacy verification.
use super::*;

#[component]
pub(super) fn Affiliations(work: Workbench) -> impl IntoView {
    let organization = RwSignal::new(None::<Value>);
    let contact = RwSignal::new(None::<Value>);
    let role = RwSignal::new(String::new());
    let primary = RwSignal::new(false);
    view! {<details class="ops-disclosure"><summary>"Link a CRM contact"</summary>
    <div hidden=move||!work.permits("affiliations.write")><form class="ops-form" on:submit=move|event|{
        event.prevent_default();
        let (Some(org),Some(person))=(organization.get_untracked(),contact.get_untracked())else{work.fail("Choose and review an organization and a CRM contact.");return};
        work.send(json!({"operation":"create_reviewed_affiliation","organizationId":org["id"],"expectedOrganizationRevision":org["revision"],"contactId":person["id"],"expectedContactRevision":person["revision"],"role":role.get_untracked(),"isPrimary":primary.get_untracked()}));
    }>
        <p class="hint">"Choose the organization and the person separately. Matching names do not establish a relationship."</p>
        <customer::CustomerPicker selected=organization organization=true/>
        <customer::CustomerPicker selected=contact affiliation=true/>
        <Field label="Relationship role" id="affiliation-role" value=role/>
        <label class="ops-check"><input type="checkbox" data-testid="affiliation-primary" prop:checked=move||primary.get() on:change=move|event|primary.set_if_alive(event_target_checked(&event))/><span>"Primary contact for this organization"</span></label>
        <button type="submit" class="btn" data-testid="affiliation-save" disabled=move||work.blocked()>"Save reviewed relationship"</button>
    </form></div></details><Legacy work/>}
}

#[component]
fn Legacy(work: Workbench) -> impl IntoView {
    let selected = RwSignal::new(String::new());
    let rows = Memo::new(move |_| {
        work.rows("affiliations")
            .into_iter()
            .filter(|row| row["contactRevision"].is_null())
            .collect::<Vec<_>>()
    });
    let options = Signal::derive(move || {
        rows.get()
            .into_iter()
            .map(|row| {
                (
                    text(&row, "id"),
                    format!(
                        "Unverified · {} · {}",
                        text(&row, "role"),
                        short(&text(&row, "contactId"))
                    ),
                )
            })
            .collect()
    });
    let current = Memo::new(move |_| {
        rows.get()
            .into_iter()
            .find(|row| row["id"] == selected.get())
    });
    let reviewed = RwSignal::new(None::<(Value, Value)>);
    let reason = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let sequence = RwSignal::new(0u64);
    Effect::new(move |_| {
        current.track();
        sequence.update_if_alive(|n| *n += 1);
        reviewed.set_if_alive(None);
        reason.set_if_alive(String::new());
        notice.set_if_alive(String::new());
        loading.set_if_alive(false);
    });
    view! {<details class="ops-disclosure"><summary>"Verify legacy contact relationships"</summary>
    <div class="ops-form">
        <p class="hint">"Older relationships have no verified CRM snapshot. This list uses the loaded Affiliations page; use its search and Next page to locate older records. Verification preserves the stored contact ID. If that contact does not exist, remove the old relationship and create a reviewed one."</p>
        <Pick label="Unverified relationship" id="affiliation-legacy-select" value=selected options/>
        <div hidden=move||!work.permits("affiliations.write")>
        <button type="button" class="btn secondary" data-testid="affiliation-legacy-load" disabled=move||loading.get()||work.blocked()||current.get().is_none() on:click=move|_|{
            let Some(row)=current.get_untracked() else{return};let Some(held)=session::held() else{return};
            sequence.update_if_alive(|n|*n+=1);let request=sequence.get_untracked();
            loading.set_if_alive(true);reviewed.set_if_alive(None);notice.set_if_alive(String::new());
            let tenant=crate::contacts::query_encode(&held.tenant);
            let org_path=format!("/erp/organizations/workspace?tenant={tenant}&collection=organizations&id={}",crate::contacts::query_encode(&text(&row,"organizationId")));
            let contact_path=format!("/crm/records?tenant={tenant}&contact={}",crate::contacts::query_encode(&text(&row,"contactId")));
            spawn_local(async move{
                let org=session::api_fetch("GET",&org_path,None).await;
                let contact=session::api_fetch("GET",&contact_path,None).await;
                if sequence.is_disposed()||sequence.get_untracked()!=request||current.get_untracked()!=Some(row.clone()){return}
                loading.set_if_alive(false);
                match (org,contact) {
                    (Ok(org),Ok(contact)) if org.status==200&&contact.status==200 => {
                        let organization=org.body["organizations"].get(0).cloned().unwrap_or(Value::Null);
                        let contact=contact.body["contact"].clone();
                        if organization["id"]==row["organizationId"]&&contact["id"]==row["contactId"] {
                            reviewed.set_if_alive(Some((organization,contact)));
                        }else{notice.set_if_alive("The stored source could not be verified. Review the old relationship before replacing it.".into());}
                    }
                    _=>notice.set_if_alive("The organization or CRM contact is unavailable. Check your access and the stored source.".into()),
                }
            });
        }>"Load organization and CRM source"</button>
        {move||reviewed.get().map(|(org,person)|view!{<p class="hint" data-testid="affiliation-legacy-preview">{format!("Review: {} · {} · {} · organization revision {} · CRM revision {}",text(&org,"name"),text(&person,"fullName"),text(&person,"email"),integer(&org,"revision"),integer(&person,"revision"))}</p>})}
        <form class="ops-form" on:submit=move|event|{
            event.prevent_default();let Some(row)=current.get_untracked() else{return};
            let Some((org,person))=reviewed.get_untracked() else{work.fail("Load and review both source records first.");return};
            work.send(json!({"operation":"verify_affiliation_contact","id":row["id"],"organizationId":org["id"],"expectedOrganizationRevision":org["revision"],"expectedContactRevision":person["revision"],"reason":reason.get_untracked()}));
        }>
            <Field label="Verification reason" id="affiliation-legacy-reason" value=reason/>
            <button type="submit" class="btn" data-testid="affiliation-legacy-save" disabled=move||work.blocked()||loading.get()||reviewed.get().is_none()>"Verify reviewed relationship"</button>
        </form></div><p class="hint" role="status" data-testid="affiliation-legacy-status">{move||notice.get()}</p>
    </div></details>}
}
