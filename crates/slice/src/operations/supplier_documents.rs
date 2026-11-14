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
    view! {<section class="ops-detail" hidden=move||work.company.get().is_none()||!work.permits("documents.read") data-testid="supplier-documents"><p class="eyebrow">"SUPPLIER DOCUMENTS · UNPOSTED"</p><h3>"Check supplier claims against accepted goods"</h3><p class="hint">"Capture the supplier’s actual goods claim. Matching checks PO prices and unallocated accepted receipts. This does not post accounts or pay the supplier. A claim may carry one of your company’s input tax codes with the tax amount printed on the supplier’s document; freight and other charges require separate review."</p>
     <Records work collection="documents" columns=vec![("vendorReference","Vendor reference"),("state","Check"),("declaredTotal","Claim · IDR")] selected money=vec!["declaredTotal"]/>
     {move||work.rows("documents").into_iter().find(|d|d["id"]==selected.get()).map(|d|{let id=text(&d,"id");let match_id=id.clone();let revision=integer(&d,"revision");let state=text(&d,"state");let po_revision=integer(&d["sourceOrder"],"revision");let source=d["sourceOrder"].clone();let edit=d.clone();
      view!{<section data-testid="supplier-document-detail"><company_context::Identity row=d.clone()/><h4>{text(&d,"vendorReference")}</h4><p data-testid="supplier-document-state">{format!("{} · UNPOSTED · {} · allocated accepted quantity {}",state,row_money(&d,"declaredTotal"),integer(&d,"activeAllocatedQuantity"))}</p><p>{format!("Supplier {} · source PO {} · reviewed order version {}",text(&d,"supplierName"),short(&text(&d,"orderId")),integer(&d,"observedOrderRevision"))}</p><document_tax::SupplierTax document=d.clone()/>
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
    // The claim is in its purchase order's currency: a correction keeps the
    // document's, a new claim takes the selected order's (both are returned
    // by the workspace read; the embedded source order of a document is not).
    let currency = StoredValue::new(currency_of(existing.as_ref().unwrap_or(&order)));
    let shown = move |amount: i64| {
        trade_money::trade_decimal(&currency.get_value(), amount)
            .unwrap_or_else(|| amount.to_string())
    };
    let total = RwSignal::new(
        existing
            .as_ref()
            .map(|d| shown(integer(d, "declaredTotal")))
            .unwrap_or_default(),
    );
    // A correction starts from the tax the document carries; a new claim
    // from none. Saving without a code saves a claim without tax.
    let tax_code = RwSignal::new(
        existing
            .as_ref()
            .map(|d| text(&d["tax"], "code"))
            .unwrap_or_default(),
    );
    let tax_amount = RwSignal::new(
        existing
            .as_ref()
            .filter(|d| d["tax"].is_object())
            .map(|d| shown(integer(&d["tax"], "enteredMinor")))
            .unwrap_or_default(),
    );
    let reason = RwSignal::new(String::new());
    let source = StoredValue::new(order.clone());
    let prior = StoredValue::new(existing.clone());
    let edit = existing.is_some();
    let drafts=RwSignal::new(order["lines"].as_array().cloned().unwrap_or_default().into_iter().map(|line|{let found=existing.as_ref().and_then(|d|d["lines"].as_array()).and_then(|ls|ls.iter().find(|l|l["lineId"]==line["id"]));json!({"lineId":line["id"],"title":line["title"],"accepted":line["acceptedQuantity"],"rejected":line["rejectedQuantity"],"orderedUnitCost":line["unitCost"],"quantity":found.map(|l|integer(l,"quantity").to_string()).unwrap_or_default(),"unitCost":found.map(|l|shown(integer(l,"unitCost"))).unwrap_or_default()})}).collect::<Vec<_>>());
    view! {<form class="ops-form" data-testid=if edit{"supplier-doc-revise-form"}else{"supplier-doc-capture-form"} on:submit=move|e|{
    e.prevent_default();let cur=currency.get_value();let Some(amount)=trade_input(total,&cur).filter(|n|*n>=0)else{work.fail(format!("Enter the supplier’s declared {} goods total.",amount_rule(&cur,"whole IDR")));return};let mut claims=Vec::new();for l in drafts.get_untracked(){let q=text(&l,"quantity");let price=text(&l,"unitCost");if q.trim().is_empty()&&price.trim().is_empty(){continue}let(Ok(q),Ok(price))=(q.parse::<i64>(),trade_money::parse_trade(&cur,&price))else{work.fail(if cur==trade_money::IDR{"Enter whole claimed quantity and unit price, or leave both blank to omit a line.".to_string()}else{format!("Enter a whole claimed quantity and a unit price in {}, or leave both blank to omit a line.",amount_rule(&cur,""))});return};if q<=0||price<0{work.fail("Claim quantities must be positive and unit prices non-negative.");return}claims.push(json!({"lineId":l["lineId"],"quantity":q,"unitCost":price}))}if claims.is_empty(){work.fail("Enter at least one actual supplier claim line.");return}
    let code=tax_code.get_untracked();let tax=if code.is_empty(){0}else{let Some(tax)=trade_input(tax_amount,&cur).filter(|n|*n>=0)else{work.fail(format!("Enter the tax amount printed on the supplier’s document in {}, or choose no tax code.",amount_rule(&cur,"whole IDR")));return};tax};
    let o=source.get_value();let mut command=json!({"operation":"document.capture","orderId":o["id"],"expectedOrderRevision":o["revision"],"supplierId":o["supplierId"],"vendorReference":vendor.get_untracked(),"documentDate":day.get_untracked(),"currency":cur,"declaredTotal":amount,"lines":claims});if let Some(d)=prior.get_value(){command.as_object_mut().unwrap().remove("orderId");command.as_object_mut().unwrap().remove("supplierId");command.as_object_mut().unwrap().remove("vendorReference");command["operation"]=json!("document.revise");command["id"]=d["id"].clone();command["expectedRevision"]=d["revision"].clone();command["reason"]=json!(reason.get_untracked());}document_tax::supplier_tax_fields(&mut command,&code,tax);work.send(command);
    }><h4>{if edit{"Correct supplier claim · new version"}else{"Actual supplier goods claim"}}</h4>
    {(!edit).then(||view!{<Field label="Vendor document reference" id="supplier-doc-reference" value=vendor/>})}<Field label="Vendor document date" id=if edit{"supplier-doc-edit-date"}else{"supplier-doc-date"} value=day kind="date"/><TradeField label="Declared goods total" id=if edit{"supplier-doc-edit-total"}else{"supplier-doc-total"} value=total currency=Signal::stored(currency.get_value())/>
    <document_tax::SupplierTaxFields work code=tax_code amount=tax_amount total day currency=currency.get_value() edit/>
    <p class="hint">"Enter what the supplier document actually claims. Leave both fields blank for a line that is not claimed. Prices and accepted goods are checked after capture."</p>
    {drafts.get_untracked().into_iter().map(|l|{let id=text(&l,"lineId");let qid=id.clone();let cid=id.clone();let qread=id.clone();let cread=id;view!{<fieldset class="ops-receipt-line"><legend>{text(&l,"title")}</legend><p class="hint">{format!("PO unit price {} · accepted {} · rejected {}",currency_amount(integer(&l,"orderedUnitCost"),&currency.get_value()),integer(&l,"accepted"),integer(&l,"rejected"))}</p><label>"Claimed quantity"<input type="number" min="1" data-testid=if edit{"supplier-doc-edit-quantity"}else{"supplier-doc-quantity"} prop:value=move||drafts.get().iter().find(|l|l["lineId"]==qread).map(|l|text(l,"quantity")).unwrap_or_default() on:input=move|e|set_claim(drafts,&qid,"quantity",event_target_value(&e))/></label><label>{format!("Claimed unit price · {}",currency.get_value())}<input type="number" min="0" step=trade_step(&currency.get_value()) data-testid=if edit{"supplier-doc-edit-price"}else{"supplier-doc-price"} prop:value=move||drafts.get().iter().find(|l|l["lineId"]==cread).map(|l|text(l,"unitCost")).unwrap_or_default() on:input=move|e|set_claim(drafts,&cid,"unitCost",event_target_value(&e))/></label></fieldset>}}).collect::<Vec<_>>()}
    {edit.then(||view!{<Field label="Reason for correction" id="supplier-doc-edit-reason" value=reason/>})}<button class="btn" type="submit" data-testid=if edit{"supplier-doc-revise"}else{"supplier-doc-save"} disabled=move||work.blocked()>{if edit{"Save corrected claim version"}else{"Capture supplier claim"}}</button></form>}
}
fn set_claim(drafts: RwSignal<Vec<Value>>, id: &str, key: &str, value: String) {
    drafts.update_if_alive(|ls| {
        if let Some(l) = ls.iter_mut().find(|l| l["lineId"] == id) {
            l[key] = json!(value)
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;
    fn order(currency: Option<&str>, unit_cost: i64) -> Value {
        let mut order = json!({"id":"po","revision":3,"supplierId":"supplier","lines":[{"id":"line","title":"Green tea","unitCost":unit_cost,"acceptedQuantity":4,"rejectedQuantity":0}]});
        if let Some(currency) = currency {
            order["currency"] = json!(currency);
        }
        order
    }
    #[test]
    fn a_supplier_claim_is_entered_and_shown_in_its_purchase_orders_currency() {
        Owner::new().with(|| {
            let work = Workbench::new(Domain::Purchasing);
            let idr = view! {<Composer work order=order(Some("IDR"), 150_000) existing=None/>}.to_html();
            assert!(idr.contains("Declared goods total · IDR"), "{idr}");
            assert!(idr.contains("PO unit price Rp 150.000"), "{idr}");
            assert!(idr.contains("Claimed unit price · IDR"), "{idr}");
            assert!(!idr.contains("step="), "{idr}");
            // A row without a currency is IDR.
            let legacy = view! {<Composer work order=order(None, 150_000) existing=None/>}.to_html();
            assert!(legacy.contains("PO unit price Rp 150.000"), "{legacy}");
            let usd = view! {<Composer work order=order(Some("USD"), 1_250) existing=None/>}.to_html();
            assert!(usd.contains("Declared goods total · USD"), "{usd}");
            assert!(usd.contains("PO unit price USD 12.50"), "{usd}");
            assert!(usd.contains("Claimed unit price · USD"), "{usd}");
            assert!(usd.contains("step=\"0.01\""), "{usd}");
            // A correction keeps the document's currency (its embedded source
            // order carries none) and shows the stored minor units as decimals.
            let source = order(None, 1_250);
            let document = json!({"id":"doc","revision":2,"currency":"USD","declaredTotal":5_000,"documentDate":"2026-11-14","lines":[{"lineId":"line","quantity":4,"unitCost":1_250}]});
            let revise = view! {<Composer work order=source existing=Some(document)/>}.to_html();
            assert!(revise.contains("Declared goods total · USD"), "{revise}");
            assert!(revise.contains("PO unit price USD 12.50"), "{revise}");
        });
    }
    fn loaded(codes: Value) -> Workbench {
        let work = Workbench::new(Domain::Purchasing);
        work.data.set(json!({"taxCodes":codes}));
        work.ready.set(true);
        work
    }
    fn input_codes() -> Value {
        json!([
            {"code":"VAT-IN","version":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},"effectiveFrom":"2020-01-01","effectiveTo":null},
            {"code":"GST","version":1,"rate":{"numerator":43,"denominator":400},"base":{"numerator":1,"denominator":1},"effectiveFrom":"2020-01-01","effectiveTo":null}])
    }
    #[test]
    fn the_claim_form_offers_the_companys_input_codes_and_the_suppliers_tax_amount_in_its_currency()
    {
        Owner::new().with(|| {
            // A company without input codes: the form is the one it was.
            let plain = view! {<Composer work=Workbench::new(Domain::Purchasing) order=order(Some("IDR"), 7_000) existing=None/>}.to_html();
            for absent in ["supplier-doc-tax-code", "supplier-doc-tax\"", "supplier-doc-tax-notice", "Input tax code"] {
                assert!(!plain.contains(absent), "{absent}: {plain}");
            }
            assert_eq!(
                plain,
                view! {<Composer work=loaded(json!([])) order=order(Some("IDR"), 7_000) existing=None/>}.to_html()
            );

            let idr = view! {<Composer work=loaded(input_codes()) order=order(Some("IDR"), 7_000) existing=None/>}.to_html();
            assert!(idr.contains("data-testid=\"supplier-doc-tax-code\""), "{idr}");
            assert!(idr.contains(">No tax<") && idr.contains(">VAT-IN<") && idr.contains(">GST<"), "{idr}");
            assert!(idr.contains("data-testid=\"supplier-doc-tax\""), "{idr}");
            assert!(idr.contains("s document · IDR"), "{idr}");
            assert!(idr.contains("data-testid=\"supplier-doc-tax-notice\""), "{idr}");
            assert!(idr.contains("Tax codes are entered by the company; Archava does not verify tax rates or rules."), "{idr}");
            assert!(idr.contains("that amount is the one owed and posted"), "{idr}");
            // Nothing is previewed before a code is chosen.
            assert!(!idr.contains("supplier-doc-tax-preview"), "{idr}");

            // A correction of a taxed USD claim starts from the stored code and
            // entered amount and shows computed, entered and the difference.
            let document = json!({"id":"doc","revision":3,"currency":"USD","declaredTotal":7_000,"documentDate":"2026-11-14","lines":[{"lineId":"line","quantity":1,"unitCost":7_000}],
                "tax":{"code":"GST","version":1,"rate":{"numerator":43,"denominator":400},"base":{"numerator":1,"denominator":1},"enteredMinor":752,"computedMinor":753,"differenceMinor":-1,"totalMinor":7_752}});
            let usd = view! {<Composer work=loaded(input_codes()) order=order(None, 7_000) existing=Some(document.clone())/>}.to_html();
            assert!(usd.contains("data-testid=\"supplier-doc-edit-tax-code\""), "{usd}");
            assert!(usd.contains("data-testid=\"supplier-doc-edit-tax\""), "{usd}");
            assert!(usd.contains("document · USD"), "{usd}");
            assert!(usd.contains("step=\"0.01\""), "{usd}");
            assert!(usd.contains("data-testid=\"supplier-doc-tax-preview\""), "{usd}");
            assert!(usd.contains("GST 10.75% · version 1 applies on 2026-11-14 · computed tax USD 7.53 · entered tax USD 7.52 · difference USD -0.01 · owed to the supplier USD 77.52"), "{usd}");
            // The stored code stays selectable even when the list no longer has it.
            let kept = view! {<Composer work=loaded(json!([])) order=order(None, 7_000) existing=Some(document)/>}.to_html();
            assert!(kept.contains(">GST<"), "{kept}");
            assert!(kept.contains("data-testid=\"supplier-doc-tax-refusal\""), "{kept}");
        });
    }
}
