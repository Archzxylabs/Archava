use super::*;

#[component]
fn InvoiceFromOrder(work: Workbench) -> impl IntoView {
    let orders = Workbench::with_company(Domain::Commerce, work.company);
    let selected = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let eligible = Memo::new(move |_| {
        orders
            .rows("orders")
            .into_iter()
            .filter(|order| {
                order["state"] == "fulfilled"
                    && order["lines"].as_array().is_some_and(|lines| {
                        !lines.is_empty()
                            && lines
                                .iter()
                                .all(|line| integer(line, "returnedQuantity") == 0)
                    })
                    && !work
                        .rows("invoices")
                        .iter()
                        .any(|invoice| invoice["sourceOrderId"] == order["id"])
            })
            .collect::<Vec<_>>()
    });
    let options = Memo::new(move |_| {
        eligible
            .get()
            .iter()
            .map(|order| {
                (
                    text(order, "id"),
                    format!(
                        "{} · {} · {}",
                        text(order, "customerName"),
                        row_money(order, "total"),
                        short(&text(order, "id"))
                    ),
                )
            })
            .collect::<Vec<_>>()
    });
    Effect::new(move |_| {
        let current = selected.get();
        if !current.is_empty() && !eligible.get().iter().any(|order| order["id"] == current) {
            selected.set_if_alive(String::new());
        }
    });
    view! {<section class="ops-form" data-testid="invoice-from-order">
        <h3>"Invoice a fulfilled order"</h3>
        <p class="hint">"Copy the original customer, quantities and prices into one draft. Requires access to the sales order. Orders with returns need a separate review."</p>
        <Field label="Find sales orders by customer" id="invoice-order-search" value=orders.search required=false/>
        <p class="hint">"Up to 100 recent matching orders are shown."</p>
        <Feedback work=orders/>
        <button type="button" class="btn secondary" data-testid="invoice-order-refresh" on:click=move|_|orders.refresh()>"Search / refresh sales orders"</button>
        <form class="ops-form" on:submit=move|event|{
            event.prevent_default();
            let Some(order)=eligible.get_untracked().into_iter().find(|order|order["id"]==selected.get_untracked()) else {
                work.fail("Choose an available fulfilled sales order, or refresh the list.");return;
            };
            work.send(json!({"type":"create_invoice_from_order","invoiceCode":code.get_untracked(),"orderId":order["id"],"expectedOrderRevision":order["revision"]}));
        }>
            <Pick label="Fulfilled sales order" id="invoice-source-order" value=selected options=options.into()/>
            <Field label="Invoice number" id="invoice-order-code" value=code/>
            {move||eligible.get().into_iter().find(|order|order["id"]==selected.get()).map(|order|{let currency=currency_of(&order);view!{
                <ul class="ops-lines">{order["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|line|view!{<li>{format!("{} × {} · {}",integer(&line,"quantity"),text(&line,"title"),currency_amount(integer(&line,"quantity")*integer(&line,"price"),&currency))}</li>}).collect::<Vec<_>>()}</ul>
                <document_tax::OrderTaxPreview order=order.clone() codes=orders.rows("taxCodes")/>
            }})}
            <button type="submit" class="btn" data-testid="invoice-from-order-save" disabled=move||work.blocked()||!orders.permits("orders.read")>"Create invoice from order"</button>
        </form>
    </section>}
}

#[component]
pub fn Invoices() -> impl IntoView {
    let work = Workbench::new(Domain::Billing);
    let code = RwSignal::new(String::new());
    let customer = RwSignal::new(String::new());
    let reference = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let quantity = RwSignal::new("1".into());
    let price = RwSignal::new("0".into());
    let lines = RwSignal::new(Vec::<Value>::new());
    let selected = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = work.company_key.get();
        lines.set_if_alive(Vec::new());
        selected.set_if_alive(String::new());
    });
    let reason = RwSignal::new(String::new());
    let correction = RwSignal::new(String::new());
    view! {<Toolbar work title="Invoices & payment history"/><Feedback work/><div class="ops-layout"><section class="ops-main"><Records work collection="invoices" columns=vec![("code","Invoice"),("customer","Customer"),("status","Status"),("outstanding","Outstanding · minor units"),("currency","Currency")] selected/>
     {move||work.rows("invoices").into_iter().find(|p|p["id"]==selected.get()).map(|invoice|{
      let id=text(&invoice,"id");let revision=integer(&invoice,"revision");let state=text(&invoice,"status");let issue_id=id.clone();let void_id=id.clone();let correction_id=id.clone();let currency=currency_of(&invoice);let correction_currency=currency.clone();let line_currency=currency.clone();
      view!{<section class="ops-detail" data-testid="invoice-detail"><p class="eyebrow">"INVOICE"</p><company_context::Identity row=invoice.clone()/><h3>{text(&invoice,"code")}</h3><p>{text(&invoice,"customer")}</p>{invoice["customerPartyId"].as_str().map(|id|view!{<p class="hint" data-testid="invoice-party-identity">{format!("Party {id} · captured version {} · historical order identity",integer(&invoice,"customerPartyRevision"))}</p>})}{invoice["customerContactId"].as_str().map(|contact|view!{<p class="hint" data-testid="invoice-customer-link"><a href=format!("/app/contact?id={contact}")>"Open linked CRM customer"</a>" · name captured on the source order"</p>})}
       {invoice["sourceOrderId"].as_str().map(|order|view!{<p class="hint" data-testid="invoice-source">{format!("Created from sales order {}",short(order))}</p>})}
       <dl class="ops-totals"><dt>"Gross"</dt><dd>{row_money(&invoice,"gross")}</dd><dt>"Corrections"</dt><dd>{row_money(&invoice,"correction")}</dd><dt>"Settled"</dt><dd>{row_money(&invoice,"settled")}</dd><dt>"Outstanding"</dt><dd>{row_money(&invoice,"outstanding")}</dd></dl>
       <document_tax::InvoiceTax invoice=invoice.clone()/>
       <ul class="ops-lines">{invoice["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|line|view!{<li>{format!("{} × {} · {}",integer(&line,"quantity"),text(&line,"description"),currency_amount(integer(&line,"total"),&line_currency))}</li>}).collect::<Vec<_>>()}</ul>
       <p class="hint">"Payment totals change only after a verified provider event. Recording an intent does not mark this invoice paid."</p>
       <div hidden=move||!work.permits("billing.settle")><Field label="Action reason" id="invoice-reason" value=reason/>
         {(state=="draft").then(||view!{<button type="button" class="btn" data-testid="invoice-issue" disabled=move||work.blocked() on:click=move|_|work.send(json!({"type":"issue_invoice","invoiceId":issue_id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Issue invoice"</button>})}
         {(state!="void"&&state!="settled").then(||view!{<button type="button" class="btn secondary" data-testid="invoice-void" disabled=move||work.blocked() on:click=move|_|work.send(json!({"type":"void_invoice","invoiceId":void_id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Void invoice"</button>})}
         {(state=="issued"||state=="settled").then(||view!{<form class="ops-form" on:submit=move|e|{e.prevent_default();match trade_input(correction,&correction_currency){Some(n)if n!=0=>work.send(json!({"type":"append_correction","invoiceId":correction_id,"reason":reason.get_untracked(),"amountMinor":n})),_=>work.fail(format!("Enter a non-zero {} correction.",amount_rule(&correction_currency,"whole Rupiah")))}}><TradeField label="Correction amount" id="invoice-correction" value=correction currency=Signal::stored(currency.clone())/><button type="submit" class="btn secondary" disabled=move||work.blocked()>"Append correction"</button></form>})}
       </div>
       <super::payments::PaymentPanel work invoice_id=id.clone() issued=state=="issued" outstanding=integer(&invoice,"outstanding")/>
       <super::mail::EmailPanel work invoice_id=id issued=state=="issued"/>
      </section>}
     })}
    </section><aside class="ops-aside"><Show when=move||work.permits("billing.write")><InvoiceFromOrder work/></Show><div hidden=move||work.company.get().is_some()||!work.permits("billing.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();if lines.get_untracked().is_empty(){work.fail("Add at least one invoice line.");return}work.send(json!({"type":"create_invoice","invoiceCode":code.get_untracked(),"customerRef":customer.get_untracked(),"consumerRef":reference.get_untracked(),"currency":"IDR","lines":lines.get_untracked()}));}><h3>"New invoice"</h3><Field label="Invoice number" id="invoice-code" value=code/><Field label="Customer reference" id="invoice-customer" value=customer/><Field label="Order / contract reference" id="invoice-reference" value=reference/><Field label="Line description" id="invoice-description" value=description/><Field label="Quantity" id="invoice-quantity" value=quantity kind="number"/><Field label="Unit amount · IDR" id="invoice-price" value=price kind="number"/>
     <button type="button" class="btn secondary" data-testid="invoice-add-line" on:click=move|_|match(number(quantity),number(price)){(Some(q),Some(p))if q>0&&p>=0&&!description.get_untracked().trim().is_empty()=>lines.update_if_alive(|ls|ls.push(json!({"description":description.get_untracked(),"quantity":q,"unitMinor":p}))),_=>work.fail("Enter a description, positive quantity and non-negative whole Rupiah price.")}>"Add line"</button>
     <ul class="ops-lines">{move||lines.get().into_iter().enumerate().map(|(index,line)|view!{<li>{format!("{} × {}",integer(&line,"quantity"),text(&line,"description"))}<button type="button" aria-label="Remove line" on:click=move|_|lines.update_if_alive(|ls|{if index<ls.len(){ls.remove(index);}})>"×"</button></li>}).collect::<Vec<_>>()}</ul>
     <button class="btn" type="submit" data-testid="invoice-save" disabled=move||work.blocked()>"Create draft invoice"</button></form></div></aside></div>}
}
