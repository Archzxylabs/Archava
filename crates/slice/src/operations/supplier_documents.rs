use super::*;
#[component]
pub(super) fn Documents(work: Workbench, order_id: RwSignal<String>) -> impl IntoView {
    let selected = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let reason = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = work.company_key.get();
        selected.set_if_alive(String::new());
        editing.set_if_alive(false)
    });
    Effect::new(move |_| {
        selected.track();
        editing.set_if_alive(false)
    });
    view! {<section class="ops-detail" hidden=move||work.company.get().is_none()||!work.permits("documents.read") data-testid="supplier-documents"><p class="eyebrow">"SUPPLIER DOCUMENTS · UNPOSTED"</p><h3>"Check supplier claims against accepted goods"</h3><p class="hint">"Capture the supplier’s actual goods claim. Matching checks PO prices and unallocated accepted receipts. This does not post accounts, calculate tax, or pay the supplier. Tax, freight and other charges require separate review."</p>
     <Records work collection="documents" columns=vec![("vendorReference","Vendor reference"),("state","Check"),("declaredTotal","Claim · IDR")] selected/>
     {move||work.rows("documents").into_iter().find(|d|d["id"]==selected.get()).map(|d|{let id=text(&d,"id");let match_id=id.clone();let revision=integer(&d,"revision");let state=text(&d,"state");let po_revision=integer(&d["sourceOrder"],"revision");let source=d["sourceOrder"].clone();let edit=d.clone();
      view!{<section data-testid="supplier-document-detail"><company_context::Identity row=d.clone()/><h4>{text(&d,"vendorReference")}</h4><p data-testid="supplier-document-state">{format!("{} · UNPOSTED · {} · allocated accepted quantity {}",state,money(integer(&d,"declaredTotal")),integer(&d,"activeAllocatedQuantity"))}</p><p>{format!("Supplier {} · source PO {} · reviewed order version {}",text(&d,"supplierName"),short(&text(&d,"orderId")),integer(&d,"observedOrderRevision"))}</p>
       <ul class="ops-lines" data-testid="supplier-document-findings">{d["matchEvidence"]["reasons"].as_array().cloned().unwrap_or_default().into_iter().map(|r|view!{<li>{format!("{} · claimed {} · observed {}",text(&r,"code").replace('_'," "),r.get("claimed").unwrap_or(&Value::Null),r.get("available").or_else(||r.get("ordered")).or_else(||r.get("computed")).unwrap_or(&Value::Null))}</li>}).collect::<Vec<_>>()}</ul>
       <ul class="ops-lines">{d["matchEvidence"]["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|l|view!{<li>{format!("Line {} · claimed {} · accepted {} · rejected {} · available {}",short(&text(&l,"lineId")),integer(&l,"claimedQuantity"),integer(&l,"accepted"),integer(&l,"rejected"),integer(&l,"available"))}</li>}).collect::<Vec<_>>()}</ul>
       <div hidden=move||!work.permits("documents.write")><Field label="Review / correction reason" id="supplier-doc-reason" value=reason/>
        {matches!(state.as_str(),"captured"|"held").then(||view!{<button class="btn" type="button" data-testid="supplier-doc-match" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"document.match","id":match_id,"expectedRevision":revision,"expectedOrderRevision":po_revision,"reason":reason.get_untracked()}))>"Check exact goods match"</button>})}
        {(state!="void").then(||view!{<button class="btn secondary" type="button" data-testid="supplier-doc-void" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"document.void","id":id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Void claim and release allocations"</button>})}
        {(state!="matched").then(||view!{<button class="btn secondary" type="button" data-testid="supplier-doc-edit" disabled=move||work.blocked() on:click=move|_|editing.set_if_alive(true)>"Review corrected supplier claim"</button>})}
        {move||(editing.get()&&edit["state"]!="matched").then(||view!{<Composer work order=source.clone() existing=Some(edit.clone())/>})}
       </div><details><summary>"Recent document history (up to 20 versions)"</summary><ul>{d["versions"].as_array().cloned().unwrap_or_default().into_iter().map(|v|view!{<li>{format!("Version {} · {} · receipt {}",integer(&v,"revision"),text(&v,"state"),text(&v,"requestId"))}</li>}).collect::<Vec<_>>()}</ul></details>
      </section>}
     })}
     {move||work.rows("orders").into_iter().find(|o|o["id"]==order_id.get()&&o["companyId"].is_string()&&matches!(o["state"].as_str(),Some("confirmed"|"partial"|"received"|"short_closed"))).map(|o|view!{<details class="ops-disclosure" data-testid="supplier-doc-capture"><summary>"Capture a supplier goods document for selected PO"</summary><div hidden=move||!work.permits("documents.write")><Composer work order=o existing=None/></div></details>})}
    </section>}
}
#[component]
fn Composer(work: Workbench, order: Value, existing: Option<Value>) -> impl IntoView {
    let vendor = RwSignal::new(
        existing
            .as_ref()
            .map(|d| text(d, "vendorReference"))
            .unwrap_or_default(),
    );
    let day = RwSignal::new(
        existing
            .as_ref()
            .map(|d| text(d, "documentDate"))
            .unwrap_or_default(),
    );
    let total = RwSignal::new(
        existing
            .as_ref()
            .map(|d| integer(d, "declaredTotal").to_string())
            .unwrap_or_default(),
    );
    let reason = RwSignal::new(String::new());
    let source = StoredValue::new(order.clone());
    let prior = StoredValue::new(existing.clone());
    let edit = existing.is_some();
    let drafts=RwSignal::new(order["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|line|{let found=existing.as_ref().and_then(|d|d["lines"].as_array()).and_then(|ls|ls.iter().find(|l|l["lineId"]==line["id"]));json!({"lineId":line["id"],"title":line["title"],"accepted":line["acceptedQuantity"],"rejected":line["rejectedQuantity"],"orderedUnitCost":line["unitCost"],"quantity":found.map(|l|integer(l,"quantity").to_string()).unwrap_or_default(),"unitCost":found.map(|l|integer(l,"unitCost").to_string()).unwrap_or_default()})}).collect::<Vec<_>>());
    view! {<form class="ops-form" data-testid=if edit{"supplier-doc-revise-form"}else{"supplier-doc-capture-form"} on:submit=move|e|{
    e.prevent_default();let Some(amount)=number(total).filter(|n|*n>=0)else{work.fail("Enter the supplier’s declared whole IDR goods total.");return};let mut claims=Vec::new();for l in drafts.get_untracked(){let q=text(&l,"quantity");let price=text(&l,"unitCost");if q.trim().is_empty()&&price.trim().is_empty(){continue}let(Ok(q),Ok(price))=(q.parse::<i64>(),price.parse::<i64>())else{work.fail("Enter whole claimed quantity and unit price, or leave both blank to omit a line.");return};if q<=0||price<0{work.fail("Claim quantities must be positive and unit prices non-negative.");return}claims.push(json!({"lineId":l["lineId"],"quantity":q,"unitCost":price}))}if claims.is_empty(){work.fail("Enter at least one actual supplier claim line.");return}
    let o=source.get_value();let mut command=json!({"operation":"document.capture","orderId":o["id"],"expectedOrderRevision":o["revision"],"supplierId":o["supplierId"],"vendorReference":vendor.get_untracked(),"documentDate":day.get_untracked(),"currency":"IDR","declaredTotal":amount,"lines":claims});if let Some(d)=prior.get_value(){command.as_object_mut().unwrap().remove("orderId");command.as_object_mut().unwrap().remove("supplierId");command.as_object_mut().unwrap().remove("vendorReference");command["operation"]=json!("document.revise");command["id"]=d["id"].clone();command["expectedRevision"]=d["revision"].clone();command["reason"]=json!(reason.get_untracked());}work.send(command);
    }><h4>{if edit{"Correct supplier claim · new version"}else{"Actual supplier goods claim"}}</h4>
    {(!edit).then(||view!{<Field label="Vendor document reference" id="supplier-doc-reference" value=vendor/>})}<Field label="Vendor document date" id=if edit{"supplier-doc-edit-date"}else{"supplier-doc-date"} value=day kind="date"/><Field label="Declared goods total · IDR" id=if edit{"supplier-doc-edit-total"}else{"supplier-doc-total"} value=total kind="number"/>
    <p class="hint">"Enter what the supplier document actually claims. Leave both fields blank for a line that is not claimed. Prices and accepted goods are checked after capture."</p>
    {drafts.get_untracked().into_iter().map(|l|{let id=text(&l,"lineId");let qid=id.clone();let cid=id.clone();let qread=id.clone();let cread=id;view!{<fieldset class="ops-receipt-line"><legend>{text(&l,"title")}</legend><p class="hint">{format!("PO unit price {} · accepted {} · rejected {}",money(integer(&l,"orderedUnitCost")),integer(&l,"accepted"),integer(&l,"rejected"))}</p><label>"Claimed quantity"<input type="number" min="1" data-testid=if edit{"supplier-doc-edit-quantity"}else{"supplier-doc-quantity"} prop:value=move||drafts.get().iter().find(|l|l["lineId"]==qread).map(|l|text(l,"quantity")).unwrap_or_default() on:input=move|e|set_claim(drafts,&qid,"quantity",event_target_value(&e))/></label><label>"Claimed unit price · IDR"<input type="number" min="0" data-testid=if edit{"supplier-doc-edit-price"}else{"supplier-doc-price"} prop:value=move||drafts.get().iter().find(|l|l["lineId"]==cread).map(|l|text(l,"unitCost")).unwrap_or_default() on:input=move|e|set_claim(drafts,&cid,"unitCost",event_target_value(&e))/></label></fieldset>}}).collect::<Vec<_>>()}
    {edit.then(||view!{<Field label="Reason for correction" id="supplier-doc-edit-reason" value=reason/>})}<button class="btn" type="submit" data-testid=if edit{"supplier-doc-revise"}else{"supplier-doc-save"} disabled=move||work.blocked()>{if edit{"Save corrected claim version"}else{"Capture supplier claim"}}</button></form>}
}
fn set_claim(drafts: RwSignal<Vec<Value>>, id: &str, key: &str, value: String) {
    drafts.update_if_alive(|ls| {
        if let Some(l) = ls.iter_mut().find(|l| l["lineId"] == id) {
            l[key] = json!(value)
        }
    })
}
