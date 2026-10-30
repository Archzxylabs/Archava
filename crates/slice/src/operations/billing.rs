use super::*;
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
    let reason = RwSignal::new(String::new());
    let correction = RwSignal::new(String::new());
    view! {<Toolbar work title="Invoices & payment history"/><Feedback work/><div class="ops-layout"><section class="ops-main"><Records work collection="invoices" columns=vec![("code","Invoice"),("customer","Customer"),("status","Status"),("outstanding","Outstanding · minor units"),("currency","Currency")] selected/>
     {move||work.rows("invoices").into_iter().find(|p|p["id"]==selected.get()).map(|invoice|{
      let id=text(&invoice,"id");let revision=integer(&invoice,"revision");let state=text(&invoice,"status");let issue_id=id.clone();let void_id=id.clone();let correction_id=id.clone();
      view!{<section class="ops-detail" data-testid="invoice-detail"><p class="eyebrow">"INVOICE"</p><h3>{text(&invoice,"code")}</h3><p>{text(&invoice,"customer")}</p><dl class="ops-totals"><dt>"Gross"</dt><dd>{currency_amount(integer(&invoice,"gross"),&text(&invoice,"currency"))}</dd><dt>"Corrections"</dt><dd>{currency_amount(integer(&invoice,"correction"),&text(&invoice,"currency"))}</dd><dt>"Settled"</dt><dd>{currency_amount(integer(&invoice,"settled"),&text(&invoice,"currency"))}</dd><dt>"Outstanding"</dt><dd>{currency_amount(integer(&invoice,"outstanding"),&text(&invoice,"currency"))}</dd></dl>
       <ul class="ops-lines">{invoice["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|line|view!{<li>{format!("{} × {} · {}",integer(&line,"quantity"),text(&line,"description"),money(integer(&line,"total")))}</li>}).collect::<Vec<_>>()}</ul>
       <p class="hint">"Payment totals change only after a verified provider event. Recording an intent does not mark this invoice paid."</p>
       <div hidden=move||!work.permits("billing.settle")><Field label="Action reason" id="invoice-reason" value=reason/>
         {(state=="draft").then(||view!{<button type="button" class="btn" data-testid="invoice-issue" disabled=move||work.blocked() on:click=move|_|work.send(json!({"type":"issue_invoice","invoiceId":issue_id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Issue invoice"</button>})}
         {(state!="void"&&state!="settled").then(||view!{<button type="button" class="btn secondary" data-testid="invoice-void" disabled=move||work.blocked() on:click=move|_|work.send(json!({"type":"void_invoice","invoiceId":void_id,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Void invoice"</button>})}
         {(state=="issued"||state=="settled").then(||view!{<form class="ops-form" on:submit=move|e|{e.prevent_default();match number(correction){Some(n)if n!=0=>work.send(json!({"type":"append_correction","invoiceId":correction_id,"reason":reason.get_untracked(),"amountMinor":n})),_=>work.fail("Enter a non-zero whole Rupiah correction.")}}><Field label="Correction amount · IDR" id="invoice-correction" value=correction kind="number"/><button type="submit" class="btn secondary" disabled=move||work.blocked()>"Append correction"</button></form>})}
       </div>
       <super::payments::PaymentPanel work invoice_id=id.clone() issued=state=="issued" outstanding=integer(&invoice,"outstanding")/>
       <super::mail::EmailPanel work invoice_id=id issued=state=="issued"/>
      </section>}
     })}
    </section><aside class="ops-aside"><div hidden=move||!work.permits("billing.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();if lines.get_untracked().is_empty(){work.fail("Add at least one invoice line.");return}work.send(json!({"type":"create_invoice","invoiceCode":code.get_untracked(),"customerRef":customer.get_untracked(),"consumerRef":reference.get_untracked(),"currency":"IDR","lines":lines.get_untracked()}));}><h3>"New invoice"</h3><Field label="Invoice number" id="invoice-code" value=code/><Field label="Customer reference" id="invoice-customer" value=customer/><Field label="Order / contract reference" id="invoice-reference" value=reference/><Field label="Line description" id="invoice-description" value=description/><Field label="Quantity" id="invoice-quantity" value=quantity kind="number"/><Field label="Unit amount · IDR" id="invoice-price" value=price kind="number"/>
     <button type="button" class="btn secondary" data-testid="invoice-add-line" on:click=move|_|match(number(quantity),number(price)){(Some(q),Some(p))if q>0&&p>=0&&!description.get_untracked().trim().is_empty()=>lines.update_if_alive(|ls|ls.push(json!({"description":description.get_untracked(),"quantity":q,"unitMinor":p}))),_=>work.fail("Enter a description, positive quantity and non-negative whole Rupiah price.")}>"Add line"</button>
     <ul class="ops-lines">{move||lines.get().into_iter().enumerate().map(|(index,line)|view!{<li>{format!("{} × {}",integer(&line,"quantity"),text(&line,"description"))}<button type="button" aria-label="Remove line" on:click=move|_|lines.update_if_alive(|ls|{if index<ls.len(){ls.remove(index);}})>"×"</button></li>}).collect::<Vec<_>>()}</ul>
     <button class="btn" type="submit" data-testid="invoice-save" disabled=move||work.blocked()>"Create draft invoice"</button></form></div></aside></div>}
}
