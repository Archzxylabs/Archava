//! Review an organization's official profile before declaring a managed company.
use super::*;
#[component]
pub(super) fn Registry(work: Workbench) -> impl IntoView {
    let selected = RwSignal::new(None::<Value>);
    let code = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    view! {<Show when=move||work.permits("companies.write")><details class="ops-disclosure"><summary>"Register a managed company"</summary><div hidden=move||!work.permits("companies.write")><form class="ops-form" on:submit=move|e|{
        e.prevent_default();let Some(source)=selected.get_untracked()else{work.fail("Choose an organization your workspace manages.");return};
        if text(&source,"legalName").trim().is_empty(){work.fail("Add the official name to the organization profile, then refresh your selection.");return}
        work.send(json!({"operation":"company.create_from_organization","code":code.get_untracked(),"organizationId":source["id"],"expectedSourceRevision":source["revision"],"reason":reason.get_untracked()}));
    }>
    <p class="hint">"Register a company your workspace operates. Review its official name and reference below. Registration records your declaration; it does not verify legal status or assign existing transactions or accounting books."</p>
    <Field label="Company code · A–Z, 0–9, _ or -" id="company-code" value=code/>
    <customer::CustomerPicker selected organization=true company=true/>
    <p class="hint" data-testid="company-registration-review">{move||selected.get().map(|s|format!("Official name: {} · reference: {} · source revision {}",text(&s,"legalName"),text(&s,"taxId"),integer(&s,"revision"))).unwrap_or_default()}</p>
    <Field label="Reason for registering this company" id="company-create-reason" value=reason/>
    <button type="submit" class="btn" data-testid="company-save" disabled=move||work.blocked()>"Register reviewed company"</button>
    </form></div></details></Show><Profile work/>}
}
/// Said wherever a company's tax id or its registration statement is shown.
const TAX_REGISTRATION_NOTICE: &str =
    "Entered by your company; Archava does not verify tax ids or tax registration.";

/// The company's tax id and its own statement about being registered for
/// value-added tax, as stored.
fn tax_registration_line(row: &Value) -> String {
    let tax_id = row["taxId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map_or_else(
            || "no tax id entered".to_string(),
            |value| format!("tax id {value}"),
        );
    let stated = if row["vatRegistered"] == true {
        "the company states it is registered for value-added tax"
    } else {
        "no statement that the company is registered for value-added tax"
    };
    format!("Tax registration: {tax_id} · {stated}.")
}
/// The stored statement of the chosen company with the notice; nothing
/// without a company.
fn tax_registration_summary(row: Option<&Value>) -> AnyView {
    match row {
        Some(row) => view! {<p data-testid="company-tax-registration">{tax_registration_line(row)}</p>
            <p class="hint" data-testid="company-tax-registration-notice">{TAX_REGISTRATION_NOTICE}" The tax id is the reviewed copy of the source organization's tax id: change it on the organization, then apply the reviewed company profile. A company can be marked registered only while its tax id is digits with spaces, dots, slashes or hyphens."</p>}
        .into_any(),
        None => ().into_any(),
    }
}
/// The company's tax registration statement and its reviewed form.
#[component]
fn TaxRegistration(work: Workbench, current: Memo<Option<Value>>) -> impl IntoView {
    let stated = RwSignal::new(false);
    let reason = RwSignal::new(String::new());
    Effect::new(move |_| {
        stated.set_if_alive(
            current
                .get()
                .is_some_and(|row| row["vatRegistered"] == true),
        );
        reason.set_if_alive(String::new());
    });
    let save = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(row) = current.get_untracked() else {
            return;
        };
        work.send(json!({"operation":"company.set_tax_registration","id":row["id"],"expectedRevision":row["revision"],"vatRegistered":stated.get_untracked(),"reason":reason.get_untracked()}));
    };
    view! {<details class="ops-disclosure" data-testid="company-tax-registration-panel"><summary>"Tax registration"</summary>
    {move||tax_registration_summary(current.get().as_ref())}
    <form class="ops-form" on:submit=save>
    <label class="ops-check"><input type="checkbox" data-testid="company-vat-registered" prop:checked=move||stated.get() on:change=move|e|stated.set_if_alive(event_target_checked(&e))/><span>"The company states it is registered for value-added tax"</span></label>
    <Field label="Reason for the tax registration change" id="company-tax-registration-reason" value=reason/>
    <button type="submit" class="btn" data-testid="company-tax-registration-save" disabled=move||work.blocked()||current.get().is_none_or(|row|(row["vatRegistered"]==true)==stated.get())>"Save tax registration"</button>
    </form></details>}
}
#[component]
fn Profile(work: Workbench) -> impl IntoView {
    let selected = RwSignal::new(String::new());
    let options = Signal::derive(move || {
        work.rows("companies")
            .into_iter()
            .map(|r| {
                (
                    text(&r, "id"),
                    format!("{} · {}", text(&r, "code"), text(&r, "legalName")),
                )
            })
            .collect()
    });
    let current = Memo::new(move |_| {
        work.rows("companies")
            .into_iter()
            .find(|r| r["id"] == selected.get())
    });
    let active = RwSignal::new(false);
    let reason = RwSignal::new(String::new());
    let reviewed = RwSignal::new(None::<Value>);
    let loading = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let sequence = RwSignal::new(0u64);
    Effect::new(move |_| {
        let row = current.get();
        active.set_if_alive(row.as_ref().is_some_and(|r| r["active"] == true));
        reason.set_if_alive(String::new());
        reviewed.set_if_alive(None);
        loading.set_if_alive(false);
        notice.set_if_alive(String::new());
        sequence.update_if_alive(|n| *n += 1);
    });
    let can_refresh = move || {
        current.get().zip(reviewed.get()).is_some_and(|(c, s)| {
            integer(&s, "revision") > integer(&c, "sourceRevision")
                && s["state"] == "active"
                && !text(&s, "legalName").trim().is_empty()
        })
    };
    view! {<details class="ops-disclosure"><summary>"Managed-company profiles"</summary><div class="ops-form">
    <p class="hint">"Uses the loaded Managed companies collection page. Search and Next page locate older profiles. Each profile retains its stable ID, company code and source organization."</p>
    <Pick label="Managed company" id="company-profile" value=selected options/>
    {move||current.get().map(|row|view!{<p data-testid="company-profile-current">{format!("{} · company {} · revision {} · {}",text(&row,"legalName"),text(&row,"id"),integer(&row,"revision"),if row["active"]==true{"active"}else{"inactive"})}</p><p class="hint">{format!("{} · source {} · captured revision {} · reference {}",text(&row,"code"),text(&row,"sourceOrganizationId"),integer(&row,"sourceRevision"),text(&row,"taxId"))}</p>})}
    <div hidden=move||!work.permits("companies.write")>
    <form class="ops-form" on:submit=move|e|{e.prevent_default();let Some(row)=current.get_untracked()else{return};work.send(json!({"operation":"company.set_active","id":row["id"],"expectedRevision":row["revision"],"active":active.get_untracked(),"reason":reason.get_untracked()}));}>
    <label class="ops-check"><input type="checkbox" data-testid="company-profile-active" prop:checked=move||active.get() on:change=move|e|active.set_if_alive(event_target_checked(&e))/><span>"Company profile active"</span></label>
    <Field label="Reason for profile change" id="company-profile-reason" value=reason/>
    <button type="submit" class="btn" data-testid="company-active-save" disabled=move||work.blocked()||current.get().is_none_or(|r|r["active"]==active.get())>"Save company activity"</button></form>
    <p class="hint">"Reactivation checks the current organization profile. If it changed, review and refresh this company first. Deactivation preserves its history."</p>
    <TaxRegistration work current/>
    <button type="button" class="btn secondary" data-testid="company-source-load" disabled=move||work.blocked()||loading.get()||current.get().is_none() on:click=move|_|{
        let Some(row)=current.get_untracked()else{return};let Some(held)=session::held()else{return};let source=text(&row,"sourceOrganizationId");
        let path=format!("/erp/organizations/workspace?tenant={}&collection=organizations&id={}",crate::contacts::query_encode(&held.tenant),crate::contacts::query_encode(&source));
        sequence.update_if_alive(|n|*n+=1);let request=sequence.get_untracked();loading.set_if_alive(true);reviewed.set_if_alive(None);notice.set_if_alive(String::new());
        spawn_local(async move{let answer=session::api_fetch("GET",&path,None).await;if sequence.is_disposed()||sequence.get_untracked()!=request||current.get_untracked()!=Some(row.clone()){return}loading.set_if_alive(false);match answer{Ok(answer)if answer.status==200=>{let value=answer.body["organizations"].get(0).cloned().unwrap_or(Value::Null);if value["id"]==source {if value["state"]!="active"||text(&value,"legalName").trim().is_empty(){notice.set_if_alive("The source needs an active organization with an official name.".into());}reviewed.set_if_alive(Some(value));}else{notice.set_if_alive("The organization is unavailable.".into());}},_=>notice.set_if_alive("The organization could not be loaded. Check your access and try again.".into())}});
    }>"Load current company source"</button>
    {move||reviewed.get().map(|s|view!{<p class="hint" data-testid="company-source-preview">{format!("Review: {} · official name {} · reference {} · source revision {} · {}",text(&s,"name"),text(&s,"legalName"),text(&s,"taxId"),integer(&s,"revision"),text(&s,"state"))}</p>})}
    <button type="button" class="btn" data-testid="company-source-save" disabled=move||work.blocked()||loading.get()||!can_refresh() on:click=move|_|{let(Some(row),Some(source))=(current.get_untracked(),reviewed.get_untracked())else{return};if reason.get_untracked().trim().is_empty(){work.fail("Enter a reason before applying the reviewed company profile.");return}work.send(json!({"operation":"company.refresh_source","id":row["id"],"expectedRevision":row["revision"],"expectedSourceRevision":source["revision"],"reason":reason.get_untracked()}));}>"Apply reviewed company profile"</button>
    </div><p class="hint" role="status" data-testid="company-profile-status">{move||notice.get()}</p></div></details>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn the_company_profile_states_its_tax_id_and_registration_as_entered() {
        let row =
            json!({"id":"c","taxId":"01.234.567.8-999.000","vatRegistered":true,"revision":3});
        let html = tax_registration_summary(Some(&row)).to_html();
        assert!(html.contains("data-testid=\"company-tax-registration\""));
        assert!(html.contains(
            "Tax registration: tax id 01.234.567.8-999.000 · the company states it is registered for value-added tax."
        ));
        assert!(html.contains("Archava does not verify tax ids or tax registration."));
        assert!(html.contains("change it on the organization"));
        // A company from before the field, or without a tax id.
        assert_eq!(
            tax_registration_line(&json!({"id":"c","taxId":null})),
            "Tax registration: no tax id entered · no statement that the company is registered for value-added tax."
        );
        assert_eq!(
            tax_registration_line(&json!({"taxId":"<b>","vatRegistered":false})),
            "Tax registration: tax id <b> · no statement that the company is registered for value-added tax."
        );
        let html = tax_registration_summary(Some(&json!({"taxId":"<b>"}))).to_html();
        assert!(html.contains("tax id &lt;b&gt;") && !html.contains("<b>"));
        assert!(!tax_registration_summary(None)
            .to_html()
            .contains("Tax registration"));
        for word in ["verified by", "compliant", "official", "NPWP", "PKP"] {
            assert!(!html.contains(word), "{word}");
        }
    }
}
