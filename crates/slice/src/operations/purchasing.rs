use super::*;
#[component]
pub fn Purchasing() -> impl IntoView {
    let work = Workbench::new(Domain::Purchasing);
    let catalog = Workbench::with_company(Domain::Commerce, work.company);
    let supplier = RwSignal::new(String::new());
    let product = RwSignal::new(String::new());
    let cost = RwSignal::new("0".into());
    let offer = RwSignal::new(String::new());
    let quantity = RwSignal::new("1".into());
    let cart = RwSignal::new(Vec::<Value>::new());
    let selected = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = work.company_key.get();
        cart.set_if_alive(Vec::new());
        selected.set_if_alive(String::new());
        product.set_if_alive(String::new());
        offer.set_if_alive(String::new());
    });
    let reason = RwSignal::new(String::new());
    let findings = RwSignal::new(Vec::<Value>::new());
    let landed_fee = RwSignal::new("0".to_string());
    let allocation_basis = RwSignal::new("value".to_string());
    Effect::new(move |_| {
        supplier.track();
        cart.set_if_alive(Vec::new());
        offer.set_if_alive(String::new());
    });
    let suppliers = Signal::derive(move || {
        work.rows("suppliers")
            .into_iter()
            .filter(|p| p["active"] == true)
            .map(|p| {
                (
                    text(&p, "id"),
                    format!("{} · {}", text(&p, "code"), text(&p, "name")),
                )
            })
            .collect()
    });
    let products = Signal::derive(move || {
        catalog
            .rows("products")
            .into_iter()
            .filter(|p| p["active"] == true && p["trackStock"] == true)
            .map(|p| (text(&p, "id"), text(&p, "title")))
            .collect()
    });
    let offers = Signal::derive(move || {
        work.rows("offers")
            .into_iter()
            .filter(|p| p["active"] == true && p["supplierId"] == supplier.get())
            .map(|p| {
                (
                    text(&p, "id"),
                    format!(
                        "{} · {}",
                        text(&p, "productTitle"),
                        money(integer(&p, "unitCost"))
                    ),
                )
            })
            .collect()
    });
    Effect::new(move |_| {
        selected.track();
        findings.set_if_alive(Vec::new());
    });
    let action_record = RwSignal::new(json!({}));
    Effect::new(move |_| {
        action_record.set_if_alive(
            work.rows("orders")
                .into_iter()
                .find(|r| r["id"] == selected.get())
                .map(|mut row| {
                    row["_collection"] = json!("orders");
                    row
                })
                .unwrap_or_else(|| json!({})),
        );
    });
    view! {<Toolbar work title="Purchasing & receiving"/><Feedback work/><div class="ops-layout"><section class="ops-main"><Records work collection="orders" columns=vec![("supplierName","Supplier"),("state","Status"),("total","Total · IDR")] selected/>
     {move||work.rows("orders").into_iter().find(|p|p["id"]==selected.get()).map(|order|{
      let id=text(&order,"id");let revision=integer(&order,"revision");let state=text(&order,"state");let lines=order["lines"].as_array().cloned().unwrap_or_default();let receive_id=id.clone();let cancel_id=id.clone();
      view!{<section class="ops-detail" data-testid="purchase-detail"><p class="eyebrow">"PURCHASE ORDER"</p><company_context::Identity row=order.clone()/><h3>{text(&order,"supplierName")}</h3>{order["supplierPartyId"].as_str().map(|id|view!{<p class="hint" data-testid="purchase-party-identity">{format!("Party {id} · captured version {} · historical purchase identity",integer(&order,"supplierPartyRevision"))}</p>})}{order["supplierContactId"].as_str().map(|contact|view!{<p class="hint"><a data-testid="purchase-supplier-contact-link" href=format!("/app/contact?id={contact}")>"View linked CRM supplier contact"</a>" · Historical purchase snapshot, revision "{integer(&order,"supplierContactRevision")}</p>})}<p>{format!("{} · {}",state,money(integer(&order,"total")))}</p><Field label="Reason / delivery reference" id="purchase-reason" value=reason/>
       <ul class="ops-lines">{lines.clone().into_iter().map(|line|view!{<li>{format!("{} × {} · accepted {} · rejected {}",integer(&line,"quantity"),text(&line,"title"),integer(&line,"acceptedQuantity"),integer(&line,"rejectedQuantity"))}</li>}).collect::<Vec<_>>()}</ul>
       <div hidden=move||!work.permits("orders.write")>{(state=="draft").then(||view!{<button type="button" class="btn" data-testid="purchase-confirm" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"order.confirm","id":id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Confirm purchase"</button>})}</div>
       {(matches!(state.as_str(),"confirmed"|"partial")).then(||view!{<div hidden=move||!work.permits("receipts.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();let lines=findings.get_untracked().into_iter().filter(|l|integer(l,"accepted")+integer(l,"rejected")>0).collect::<Vec<_>>();if lines.is_empty(){work.fail("Enter received quantities for at least one line.");return}match number(landed_fee){Some(fee)if fee>=0=>work.send(json!({"operation":"order.receive","id":receive_id,"expectedRevision":revision,"reason":reason.get_untracked(),"lines":lines,"landedFeeMinor":fee,"allocationBasis":allocation_basis.get_untracked()})),_=>work.fail("Enter a non-negative whole Rupiah landed fee.")}}><h4>"Inspect delivery"</h4><Field label="Delivery / landed fee allocated to accepted goods · IDR" id="receive-landed-fee" value=landed_fee kind="number"/><label class="ops-field"><span>"Allocate fee by"</span><select data-testid="receive-allocation-basis" on:change=move|e|allocation_basis.set_if_alive(event_target_value(&e))><option value="value">"Accepted goods value"</option><option value="quantity">"Accepted quantity"</option></select></label><p class="hint">"Record only this delivery. Accepted goods add stock; rejected goods do not."</p>
        {lines.into_iter().filter(|l|integer(l,"quantity")>integer(l,"acceptedQuantity")+integer(l,"rejectedQuantity")).map(|line|{let line_id=text(&line,"id");let accepted_id=line_id.clone();let rejected_id=line_id;view!{<fieldset class="ops-receipt-line"><legend>{text(&line,"title")}</legend><label>"Accepted"<input data-testid="receive-accepted" type="number" min="0" value="0" on:input=move|e|{if let Ok(n)=event_target_value(&e).parse::<i64>(){set_finding(findings,&accepted_id,"accepted",n);}}/></label><label>"Rejected"<input data-testid="receive-rejected" type="number" min="0" value="0" on:input=move|e|{if let Ok(n)=event_target_value(&e).parse::<i64>(){set_finding(findings,&rejected_id,"rejected",n);}}/></label></fieldset>}}).collect::<Vec<_>>()}
        <button type="submit" class="btn" data-testid="purchase-receive" disabled=move||work.blocked()>"Record goods receipt"</button></form></div>})}
       {(matches!(state.as_str(),"draft"|"confirmed"|"partial")).then(||view!{<div hidden=move||!work.permits("orders.write")><button type="button" class="btn secondary" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":if state=="partial"{"order.short-close"}else{"order.cancel"},"id":cancel_id,"expectedRevision":revision,"reason":reason.get_untracked()}))>{if state=="partial"{"Close remaining quantity"}else{"Cancel purchase"}}</button></div>})}
      </section>}
     })}
    <supplier_documents::Documents work order_id=selected/>
    </section><aside class="ops-aside">
     <suppliers::Registration work/><suppliers::Profile work/>
     <details class="ops-disclosure"><summary>"2. Register supplier price"</summary><div hidden=move||!work.permits("offers.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();match number(cost){Some(c)if c>=0=>work.send(json!({"operation":"offer.create","supplierId":supplier.get_untracked(),"productId":product.get_untracked(),"unitCost":c})),_=>work.fail("Enter a non-negative whole Rupiah unit cost.")}}><Pick label="Supplier" id="offer-supplier" value=supplier options=suppliers/><Pick label="Stock product" id="offer-product" value=product options=products/><Field label="Unit cost · IDR" id="offer-cost" value=cost kind="number"/><button class="btn" type="submit" data-testid="offer-save" disabled=move||work.blocked()>"Save supplier price"</button></form></div></details>
     <div hidden=move||!work.permits("orders.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();if cart.get_untracked().is_empty(){work.fail("Add at least one supplier offer.");return}let mut command=json!({"operation":"order.create","supplierId":supplier.get_untracked(),"lines":cart.get_untracked()});if let Err(e)=work.new_company_fields(&mut command,"order.create-for-company"){work.fail(e);return}work.send(command);}><h3>"3. New purchase order"</h3><Pick label="Supplier" id="purchase-supplier" value=supplier options=suppliers/><Pick label="Supplier offer" id="purchase-offer" value=offer options=offers/><Field label="Quantity" id="purchase-quantity" value=quantity kind="number"/>
      <button type="button" class="btn secondary" data-testid="purchase-add-line" on:click=move|_|{let id=offer.get_untracked();if id.is_empty(){work.fail("Choose a supplier offer.");return}match number(quantity){Some(q)if q>0=>cart.update_if_alive(|lines|{if let Some(l)=lines.iter_mut().find(|l|l["offerId"]==id){l["quantity"]=json!(q)}else{lines.push(json!({"offerId":id,"quantity":q}))}}),_=>work.fail("Enter a positive whole quantity.")}}>"Add / update line"</button>
      <ul class="ops-lines">{move||cart.get().into_iter().map(|line|{let id=text(&line,"offerId");let label=work.rows("offers").into_iter().find(|o|o["id"]==id).map(|o|text(&o,"productTitle")).unwrap_or_else(||short(&id));view!{<li>{format!("{} × {}",integer(&line,"quantity"),label)}<button type="button" aria-label="Remove line" on:click=move|_|cart.update_if_alive(|ls|ls.retain(|l|l["offerId"]!=id))>"×"</button></li>}}).collect::<Vec<_>>()}</ul>
      <button class="btn" type="submit" data-testid="purchase-save" disabled=move||work.blocked()>"Create draft purchase"</button></form></div>
    <details class="ops-disclosure"><summary>"All purchasing actions"</summary><operator::Actions work selected=action_record/></details></aside></div>}
}
fn set_finding(findings: RwSignal<Vec<Value>>, id: &str, key: &str, n: i64) {
    findings.update_if_alive(|lines| {
        if let Some(line) = lines.iter_mut().find(|l| l["lineId"] == id) {
            line[key] = json!(n)
        } else {
            let mut line = json!({"lineId":id,"accepted":0,"rejected":0});
            line[key] = json!(n);
            lines.push(line);
        }
    });
}
