use super::*;

#[component]
pub fn Catalog() -> impl IntoView {
    let work = Workbench::new(Domain::Commerce);
    let sku = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let price = RwSignal::new("0".into());
    let tracked = RwSignal::new(true);
    let selected = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = work.company_key.get();
        selected.set_if_alive(String::new());
    });
    let quantity = RwSignal::new("1".into());
    let reason = RwSignal::new(String::new());
    let revised_price = RwSignal::new(String::new());
    let opening_cost = RwSignal::new(String::new());
    // A new product is priced in the selected company's trade currency, which
    // the workspace read reports (IDR without a company or a book).
    let trade = Signal::derive(move || work.trade_currency());
    Effect::new(move |_| {
        selected.track();
        opening_cost.set_if_alive(String::new());
        quantity.set_if_alive("1".into());
        reason.set_if_alive(String::new());
    });
    view! {<Toolbar work title="Products & stock"/><Feedback work/>
          <div class="ops-layout"><section class="ops-main"><Records work collection="products" columns=vec![("sku","SKU"),("title","Product"),("price","Price · IDR"),("quantity","On hand"),("available","Available"),("active","Active")] selected money=vec!["price"]/>
            {move||work.rows("products").into_iter().find(|p|p["id"]==selected.get()).map(|product|{
              let id=text(&product,"id");let revision=integer(&product,"revision");let active=product["active"].as_bool().unwrap_or(false);let stock=product["trackStock"].as_bool().unwrap_or(false);let price_id=id.clone();let stock_id=id.clone();let availability_id=id;let currency=currency_of(&product);let price_currency=currency.clone();let cost_currency=currency.clone();
              view!{<section class="ops-detail" data-testid="product-detail"><header><p class="eyebrow">"PRODUCT"</p><company_context::Identity row=product.clone()/><h3>{text(&product,"title")}</h3><p>{format!("{} · {} · revision {}",text(&product,"sku"),row_money(&product,"price"),revision)}</p></header>
                {product.get("inventoryValueMinor").is_some().then(||view!{<dl class="ops-totals"><dt>"Inventory acquisition value"</dt><dd data-testid="inventory-value">{exact_money(&product["inventoryValueMinor"],&currency)}</dd><dt>"Units without a known cost"</dt><dd>{integer(&product,"unknownCostQuantity")}</dd></dl>})}
    <div hidden=move||!work.permits("catalog.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();match trade_input(revised_price,&price_currency){Some(n)if n>=0=>work.send(json!({"operation":"product.set","id":price_id,"expectedRevision":revision,"price":n,"active":active})),_=>work.fail(format!("Enter a non-negative {} price.",amount_rule(&price_currency,"whole Rupiah")))}}><TradeField label="New price" id="product-new-price" value=revised_price currency=Signal::stored(currency.clone())/><button type="submit" class="btn secondary" disabled=move||work.blocked()>"Update price"</button></form>
                  <ActionButton work command=json!({"operation":"product.set","id":availability_id,"expectedRevision":revision,"price":integer(&product,"price"),"active":!active}) label=if active{"Deactivate"}else{"Activate"} id="product-toggle"/>
                </div>
                {(stock).then(||view!{<div hidden=move||!work.permits("stock.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();match number(quantity){Some(n)if n!=0=>{
                    let cost=if n>0&&!opening_cost.get_untracked().trim().is_empty(){match trade_input(opening_cost,&cost_currency){Some(cost)if cost>=0=>Some(cost),_=>{work.fail(format!("Enter a non-negative {} acquisition cost, or leave it blank.",amount_rule(&cost_currency,"whole Rupiah")));return}}}else{None};
                    let mut command=json!({"operation":"stock.adjust","productId":stock_id,"quantity":n,"reason":reason.get_untracked()});if let Some(cost)=cost{command["unitCostMinor"]=json!(cost);}
                    if let Err(e)=work.new_company_fields(&mut command,"stock.adjust_for_company"){work.fail(e);return}work.send(command);
                },_=>work.fail("Enter a non-zero whole stock adjustment.")}}><h4>"Adjust counted stock"</h4><div hidden=move||!work.permits("valuation.write")><TradeField label="Unit acquisition cost" id="stock-unit-cost" value=opening_cost currency=Signal::stored(currency.clone()) note=" (optional for additions)" required=false/><p class="hint">"Leave blank when the acquisition cost is unknown. Decreases use their original FIFO basis."</p></div><p class="hint">"Positive adds stock; negative removes it. Purchases belong in goods receipts."</p><Field label="Quantity change" id="stock-quantity" value=quantity kind="number"/><Field label="Count / adjustment reason" id="stock-reason" value=reason/><button type="submit" class="btn" data-testid="stock-save" disabled=move||work.blocked()>"Record adjustment"</button></form></div>})}
              </section>}
            })}
          </section><aside class="ops-aside"><div hidden=move||!work.permits("catalog.write")>
            <form class="ops-form" data-testid="product-create" on:submit=move|e|{e.prevent_default();match trade_input(price,&trade.get_untracked()){Some(n)if n>=0=>{let mut command=json!({"operation":"product.create","sku":sku.get_untracked(),"title":title.get_untracked(),"unit":"unit","trackStock":tracked.get_untracked(),"price":n});if let Err(e)=work.new_company_fields(&mut command,"product.create_for_company"){work.fail(e);return}work.send(command)},_=>work.fail(format!("Enter a non-negative {} price.",amount_rule(&trade.get_untracked(),"whole Rupiah")))}}><h3>"New product"</h3><Field label="SKU" id="product-sku" value=sku/><p class="hint">"Use an uppercase SKU: letters, numbers, underscore or dash."</p><Field label="Product or service name" id="product-title" value=title/><TradeField label="Unit price" id="product-price" value=price currency=trade/><label class="ops-check"><input type="checkbox" prop:checked=move||tracked.get() on:change=move|e|tracked.set_if_alive(event_target_checked(&e))/><span>"Track physical stock"</span></label><p class="hint">"Leave stock tracking off for services. SKU and stock type stay fixed after creation."</p><button type="submit" class="btn" data-testid="product-save" disabled=move||work.blocked()>"Create product"</button></form>
          </div></aside></div>
        }
}

#[component]
pub fn Sales() -> impl IntoView {
    let work = Workbench::new(Domain::Commerce);
    let customer = RwSignal::new(String::new());
    let use_crm = RwSignal::new(false);
    let use_party = RwSignal::new(false);
    let linked_party = RwSignal::new(None::<Value>);
    let linked_customer = RwSignal::new(None::<Value>);
    let product = RwSignal::new(String::new());
    let quantity = RwSignal::new("1".into());
    let cart = RwSignal::new(Vec::<Value>::new());
    // The tax code a new company order names, and whether its prices include
    // the tax. A tax code belongs to one company's book.
    let tax_code = RwSignal::new(String::new());
    let price_includes_tax = RwSignal::new(false);
    Effect::new(move |_| {
        let _ = work.company_key.get();
        cart.set_if_alive(Vec::new());
        product.set_if_alive(String::new());
        tax_code.set_if_alive(String::new());
        price_includes_tax.set_if_alive(false);
    });
    let selected = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = work.company_key.get();
        selected.set_if_alive(String::new());
    });
    let return_quantity = RwSignal::new("1".into());
    let reason = RwSignal::new(String::new());
    let disposition = RwSignal::new("restock".to_string());
    let options = Signal::derive(move || {
        work.rows("products")
            .into_iter()
            .filter(|p| p["active"] == true)
            .map(|p| {
                (
                    text(&p, "id"),
                    format!("{} · {}", text(&p, "title"), row_money(&p, "price")),
                )
            })
            .collect()
    });
    view! {<Toolbar work title="Sales orders"/><Feedback work/><div class="ops-layout"><section class="ops-main"><Records work collection="orders" columns=vec![("customerName","Customer"),("state","Status"),("total","Total · IDR")] selected money=vec!["total"]/>
          {move||work.rows("orders").into_iter().find(|p|p["id"]==selected.get()).map(|order|{
            let id=text(&order,"id");let revision=integer(&order,"revision");let state=text(&order,"state");let lines=order["lines"].as_array().cloned().unwrap_or_default();let can_cancel=matches!(state.as_str(),"draft"|"reserved");let cancel_id=id.clone();
            view!{<section class="ops-detail" data-testid="sales-detail"><p class="eyebrow">"SALES ORDER"</p><company_context::Identity row=order.clone()/><h3>{text(&order,"customerName")}</h3>{order["customerPartyId"].as_str().map(|id|view!{<p class="hint" data-testid="order-party-identity">{format!("Party {id} · captured version {}",integer(&order,"customerPartyRevision"))}<a href="/app/erp?module=organizations">" · Business parties"</a></p>})}{order["customerContactId"].as_str().map(|contact|view!{<p class="hint" data-testid="order-customer-link"><a href=format!("/app/contact?id={contact}")>"Open linked CRM customer"</a>" · name captured at order creation"</p>})}<p>{format!("{} · {}",state,row_money(&order,"total"))}</p><document_tax::OrderTax order=order.clone()/><ul class="ops-lines">{lines.clone().into_iter().map(|line|view!{<li>{format!("{} × {} · returned {}",integer(&line,"quantity"),text(&line,"title"),integer(&line,"returnedQuantity"))}</li>}).collect::<Vec<_>>()}</ul>
            {order.get("fulfillmentCostMinor").is_some().then(||view!{<dl class="ops-totals"><dt>"Inventory cost on fulfillment"</dt><dd data-testid="order-fulfillment-cost">{exact_money(&order["fulfillmentCostMinor"],&currency_of(&order))}</dd><dt>"Acquisition basis restocked"</dt><dd>{exact_money(&order["restockedCostMinor"],&currency_of(&order))}</dd></dl><p class="hint">"Acquisition cost of the physical goods consumed by this order."</p>})}
    <div hidden=move||!work.permits("orders.write")><div class="ops-actions">
              {(state=="draft").then(||view!{<ActionButton work command=json!({"operation":"order.reserve","id":id,"expectedRevision":revision,"holdSeconds":900}) label="Reserve stock · 15 min" id="order-reserve"/>})}
              {(state=="reserved").then(||view!{<ActionButton work command=json!({"operation":"order.fulfill","id":id,"expectedRevision":revision}) label="Fulfill order" id="order-fulfill"/>})}
            </div></div>
            {(can_cancel).then(||view!{<div hidden=move||!work.permits("orders.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();work.send(json!({"operation":"order.cancel","id":cancel_id,"expectedRevision":revision,"reason":reason.get_untracked()}));}><Field label="Cancellation reason" id="order-cancel-reason" value=reason/><button type="submit" class="btn secondary" disabled=move||work.blocked()>"Cancel order"</button></form></div>})}
            {(state=="fulfilled").then(||view!{<div hidden=move||!work.permits("returns.write")><section class="ops-form"><h4>"Return received goods"</h4><label class="ops-field"><span>"Returned goods"</span><select data-testid="return-disposition" on:change=move|e|disposition.set_if_alive(event_target_value(&e))><option value="restock">"Restock usable goods"</option><option value="discard">"Discard damaged goods"</option></select></label><Field label="Return quantity" id="return-quantity" value=return_quantity kind="number"/><Field label="Return reason" id="return-reason" value=reason/>{lines.into_iter().filter(|l|integer(l,"quantity")>integer(l,"returnedQuantity")).map(|line|{let order_id=id.clone();view!{<button type="button" class="btn secondary" data-testid="order-return" disabled=move||work.blocked() on:click=move|_|match number(return_quantity){Some(q)if q>0=>work.send(json!({"operation":"order.return","id":order_id,"expectedRevision":revision,"lineId":line["id"],"quantity":q,"disposition":disposition.get_untracked(),"reason":reason.get_untracked()})),_=>work.fail("Enter a positive whole return quantity.")}>{format!("Record return · {}",text(&line,"title"))}</button>}}).collect::<Vec<_>>()}</section></div>})}
            </section>}
          })}
        </section><aside class="ops-aside"><div hidden=move||!work.permits("orders.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();if cart.get_untracked().is_empty(){work.fail("Add at least one product to the order.");return}let mut command=if work.company.get_untracked().is_some()||use_party.get_untracked() {
        let Some(party)=linked_party.get_untracked() else {work.fail("Choose a registered customer party first.");return;};
        json!({"operation":"order.create_from_party","partyId":party["id"],"expectedPartyRevision":party["revision"],"lines":cart.get_untracked()})
    } else if use_crm.get_untracked() {
        let Some(contact)=linked_customer.get_untracked() else {work.fail("Choose a CRM customer first.");return;};
        json!({"operation":"order.create_from_contact","contactId":contact["id"],"expectedContactRevision":contact["revision"],"lines":cart.get_untracked()})
    } else {
        if customer.get_untracked().trim().is_empty(){work.fail("Enter a customer name.");return;}
        json!({"operation":"order.create","customerName":customer.get_untracked(),"lines":cart.get_untracked()})
    };if let Err(e)=work.new_company_fields(&mut command,"order.create_for_company"){work.fail(e);return}document_tax::order_tax_fields(&mut command,&tax_code.get_untracked(),price_includes_tax.get_untracked());work.send(command);}><h3>"New sales order"</h3><label class="ops-field" hidden=move||work.company.get().is_some()><span><input type="checkbox" data-testid="order-link-customer" prop:checked=move||use_crm.get() on:change=move|event|{use_crm.set_if_alive(event_target_checked(&event));use_party.set_if_alive(false);}/>" Use an existing CRM customer"</span></label>
    <label class="ops-check" hidden=move||work.company.get().is_some()><input type="checkbox" data-testid="order-link-party" prop:checked=move||use_party.get() on:change=move|event|{use_party.set_if_alive(event_target_checked(&event));use_crm.set_if_alive(false);}/><span>"Use a registered business party"</span></label>
    <Show when=move||work.company.get().is_some()||use_party.get()><super::party_selection::PartyPicker selected=linked_party/></Show>
    <Show when=move||work.company.get().is_none()&&use_crm.get()><super::customer::CustomerPicker selected=linked_customer/></Show>
    <div hidden=move||work.company.get().is_some()||use_crm.get()||use_party.get()><Field label="Customer name" id="order-customer" value=customer required=false/></div><Pick label="Product" id="order-product" value=product options/><Field label="Quantity" id="order-quantity" value=quantity kind="number"/><button type="button" class="btn secondary" data-testid="order-add-line" on:click=move|_|{let id=product.get_untracked();if id.is_empty(){work.fail("Choose a product first.");return}match number(quantity){Some(q)if q>0=>cart.update_if_alive(|lines|{if let Some(line)=lines.iter_mut().find(|l|l["productId"]==id){line["quantity"]=json!(q)}else{lines.push(json!({"productId":id,"quantity":q}))}}),_=>work.fail("Enter a positive whole quantity.")}}>"Add / update line"</button>
          <ul class="ops-lines">{move||cart.get().into_iter().map(|line|{let id=text(&line,"productId");let title=work.rows("products").into_iter().find(|p|p["id"]==id).map(|p|text(&p,"title")).unwrap_or_else(||short(&id));view!{<li>{format!("{} × {}",integer(&line,"quantity"),title)}<button type="button" aria-label="Remove line" on:click=move|_|cart.update_if_alive(|ls|ls.retain(|l|l["productId"]!=id))>"×"</button></li>}}).collect::<Vec<_>>()}</ul>
          <document_tax::OrderTaxFields work code=tax_code price_includes_tax/>
          <p class="hint">"Creating a draft does not reduce stock. Reserve, then fulfill when goods are handed over."</p><button type="submit" class="btn" data-testid="order-save" disabled=move||work.blocked()>"Create draft order"</button></form></div></aside></div>}
}
