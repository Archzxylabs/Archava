//! Payments on the invoice detail: every payment event of the invoice with
//! its origin, and the forms that record a payment received by hand and
//! reverse one. A manual receipt is a person's statement; it is labelled
//! manual everywhere and never as verified. Built from small components that
//! return `AnyView`, so the release WASM build stays inside the compiler's
//! query depth.
use super::*;

const MANUAL_NOTICE: &str = "A payment recorded here is manual: a person's statement that the money arrived. Archava does not check it with a bank or a payment provider. It is dated the day the money was received and waits in the payment clearing account until a clearing settlement matches it to a bank or cash statement row.";

/// Where a payment event came from: the payment gateway (a provider event the
/// server verified) or a person, by bank transfer or in cash.
pub fn origin_label(event: &Value) -> String {
    match (event["origin"].as_str(), event["method"].as_str()) {
        (Some("manual"), Some("bank_transfer")) => "Manual · bank transfer".into(),
        (Some("manual"), Some("cash")) => "Manual · cash".into(),
        (Some("manual"), _) => "Manual".into(),
        _ => format!("Payment gateway · {}", text(event, "providerId")),
    }
}
/// What the event did, in the words of its origin.
fn outcome_label(event: &Value) -> &'static str {
    let manual = event["origin"] == "manual";
    match (event["outcome"].as_str(), manual) {
        (Some("succeeded"), true) => "Payment received",
        (Some("refunded"), true) => "Reversal",
        (Some("succeeded"), false) => "Payment verified by the gateway",
        (Some("refunded"), false) => "Refund verified by the gateway",
        _ => "Failed attempt, no money moved",
    }
}
/// The day the event is dated: the received-on date of a manual receipt, the
/// UTC day it was recorded otherwise.
fn event_date(event: &Value) -> String {
    event["receivedOn"]
        .as_str()
        .or_else(|| event["recordedOn"].as_str())
        .unwrap_or("—")
        .to_string()
}
/// Who recorded the event: the person's name when the workspace knows one.
fn recorded_by(event: &Value) -> String {
    event["recordedByName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .or_else(|| event["recordedBy"].as_str())
        .unwrap_or("—")
        .to_string()
}
/// What is left of a manual receipt after its reversals.
pub fn unreversed(event: &Value) -> i64 {
    integer(event, "amount") - integer(event, "reversedAmount")
}
/// Whether the event is a manual receipt with something left to reverse.
fn reversible(event: &Value) -> bool {
    event["origin"] == "manual" && event["outcome"] == "succeeded" && unreversed(event) > 0
}
/// Whether the receipt form is offered: an issued invoice that still owes.
fn takes_receipt(invoice: &Value) -> bool {
    invoice["status"] == "issued" && integer(invoice, "outstanding") > 0
}
/// The command that records a payment received by hand.
pub fn receipt_command(
    invoice: &Value,
    amount: i64,
    method: &str,
    received_on: &str,
    reference: &str,
    reason: &str,
) -> Value {
    json!({
        "type": "record_manual_receipt",
        "invoiceId": invoice["id"],
        "expectedRevision": invoice["revision"],
        "amountMinor": amount,
        "method": method,
        "receivedOn": received_on.trim(),
        "reference": reference.trim(),
        "reason": reason.trim(),
    })
}
/// The command that reverses all or part of one manual receipt.
pub fn reversal_command(invoice: &Value, event: &Value, amount: i64, reason: &str) -> Value {
    json!({
        "type": "reverse_manual_receipt",
        "invoiceId": invoice["id"],
        "expectedRevision": invoice["revision"],
        "receiptEventId": event["id"],
        "amountMinor": amount,
        "reason": reason.trim(),
    })
}

/// One row of the payments table.
fn payment_row(event: &Value, currency: &str) -> AnyView {
    let manual = event["origin"] == "manual";
    let reversed = integer(event, "reversedAmount");
    let amount = currency_amount(integer(event, "amount"), currency);
    let amount = if reversed > 0 {
        format!(
            "{amount} · {} reversed",
            currency_amount(reversed, currency)
        )
    } else {
        amount
    };
    let note = match (event["reference"].as_str(), event["reason"].as_str()) {
        (Some(reference), Some(reason)) => format!("{reference} · {reason}"),
        (None, Some(reason)) => reason.to_string(),
        (Some(reference), None) => reference.to_string(),
        (None, None) => "—".into(),
    };
    view! {<tr data-testid="invoice-payment-row" data-origin=if manual {"manual"} else {"gateway"}>
        <td>{event_date(event)}</td>
        <td data-testid="invoice-payment-origin">{origin_label(event)}</td>
        <td>{outcome_label(event)}</td>
        <td>{amount}</td>
        <td>{note}</td>
        <td>{recorded_by(event)}</td>
    </tr>}
    .into_any()
}
/// Every payment event of the invoice, oldest first, with its origin.
fn payment_list(invoice: &Value) -> AnyView {
    let events = invoice["payments"].as_array().cloned().unwrap_or_default();
    if events.is_empty() {
        return view! {<p class="hint" data-testid="invoice-payments-empty">"No payment has been recorded on this invoice."</p>}.into_any();
    }
    let currency = currency_of(invoice);
    let rows = events
        .iter()
        .map(|event| payment_row(event, &currency))
        .collect::<Vec<_>>();
    view! {<div class="ops-table-wrap"><table class="ops-table" data-testid="invoice-payments">
        <thead><tr><th>"Date (UTC)"</th><th>"Origin"</th><th>"Event"</th><th>"Amount"</th><th>"Reference / reason"</th><th>"Recorded by"</th></tr></thead>
        <tbody>{rows}</tbody>
    </table></div>}
    .into_any()
}

/// The method choice of the receipt form.
fn method_pick(method: RwSignal<String>) -> AnyView {
    view! {<label class="ops-field"><span>"How the money was received"</span>
        <select data-testid="invoice-receipt-method" prop:value=move||method.get() on:change=move|e|method.set_if_alive(event_target_value(&e))>
            <option value="bank_transfer">"Bank transfer"</option>
            <option value="cash">"Cash"</option>
        </select></label>}
    .into_any()
}

/// "Record a payment received": the amount in the invoice's currency
/// (defaulting to what is outstanding), the method, the day the money was
/// received (defaulting to today, UTC), the reference and the reason.
#[component]
fn ReceiptForm(work: Workbench, invoice: Value) -> impl IntoView {
    let currency = currency_of(&invoice);
    let outstanding = integer(&invoice, "outstanding");
    let amount = RwSignal::new(
        trade_money::trade_decimal(&currency, outstanding)
            .unwrap_or_else(|| outstanding.to_string()),
    );
    let method = RwSignal::new("bank_transfer".to_string());
    let received = RwSignal::new(text(&work.data.get_untracked(), "today"));
    let reference = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    let entered = currency.clone();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(value) = trade_input(amount, &entered).filter(|value| *value > 0) else {
            work.fail(format!(
                "Enter the {} amount received, more than zero.",
                amount_rule(&entered, "whole Rupiah")
            ));
            return;
        };
        if value > outstanding {
            work.fail("Enter at most the outstanding amount of this invoice.");
            return;
        }
        if received.get_untracked().trim().is_empty()
            || reference.get_untracked().trim().is_empty()
            || reason.get_untracked().trim().is_empty()
        {
            work.fail("Enter the date the money was received, its reference and a reason.");
            return;
        }
        work.send(receipt_command(
            &invoice,
            value,
            &method.get_untracked(),
            &received.get_untracked(),
            &reference.get_untracked(),
            &reason.get_untracked(),
        ));
    };
    view! {<form class="ops-form" data-testid="invoice-receipt-form" on:submit=submit>
        <h4>"Record a payment received"</h4>
        <TradeField label="Amount received" id="invoice-receipt-amount" value=amount currency=Signal::stored(currency)/>
        {method_pick(method)}
        <Field label="Date the money was received (UTC)" id="invoice-receipt-date" value=received kind="date"/>
        <Field label="Payer's or bank's reference, or cash receipt number" id="invoice-receipt-reference" value=reference/>
        <Field label="Reason / what it was checked against" id="invoice-receipt-reason" value=reason/>
        <button type="submit" class="btn secondary" data-testid="invoice-receipt-save" disabled=move||work.blocked()>"Record manual payment"</button>
        <p class="hint" data-testid="invoice-receipt-notice">{MANUAL_NOTICE}</p>
    </form>}
    .into_any()
}

/// Reverses all or part of one manual receipt, with a reason.
#[component]
fn ReversalForm(work: Workbench, invoice: Value, event: Value) -> impl IntoView {
    let currency = currency_of(&invoice);
    let left = unreversed(&event);
    let amount = RwSignal::new(
        trade_money::trade_decimal(&currency, left).unwrap_or_else(|| left.to_string()),
    );
    let reason = RwSignal::new(String::new());
    let summary = format!(
        "Reverse the {} of {} · {}",
        origin_label(&event).to_lowercase(),
        event_date(&event),
        text(&event, "reference")
    );
    let entered = currency.clone();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        match trade_input(amount, &entered) {
            Some(value)
                if value > 0 && value <= left && !reason.get_untracked().trim().is_empty() =>
            {
                work.send(reversal_command(
                    &invoice,
                    &event,
                    value,
                    &reason.get_untracked(),
                ));
            }
            _ => work.fail(format!(
                "Enter a reason and an amount of at most {} to reverse.",
                currency_amount(left, &entered)
            )),
        }
    };
    view! {<details class="ops-disclosure" data-testid="invoice-receipt-reversal"><summary>{summary}</summary>
        <form class="ops-form" on:submit=submit>
            <TradeField label="Amount to reverse" id="invoice-reversal-amount" value=amount currency=Signal::stored(currency)/>
            <Field label="Reason for the reversal" id="invoice-reversal-reason" value=reason/>
            <button type="submit" class="btn secondary" data-testid="invoice-reversal-save" disabled=move||work.blocked()>"Reverse manual payment"</button>
            <p class="hint">"A reversal reopens the invoice by this amount and is dated today. The original payment stays in the history."</p>
        </form></details>}
    .into_any()
}

/// The forms a person who may settle invoices gets: the receipt form on an
/// issued invoice that still owes, and one reversal per manual receipt with
/// something left to reverse.
fn settle_forms(work: Workbench, invoice: &Value) -> AnyView {
    let receipt = takes_receipt(invoice)
        .then(|| view! {<ReceiptForm work invoice=invoice.clone()/>}.into_any());
    let reversals = invoice["payments"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(reversible)
        .map(|event| view! {<ReversalForm work invoice=invoice.clone() event/>}.into_any())
        .collect::<Vec<_>>();
    view! { {receipt} {reversals} }.into_any()
}

/// The payments of an invoice and, for a person with `billing.settle`, the
/// forms that record a manual receipt and reverse one. Without the permission
/// the forms are not rendered; the server refuses the commands either way.
#[component]
pub fn InvoicePayments(work: Workbench, invoice: Value) -> impl IntoView {
    let list = payment_list(&invoice);
    let row = StoredValue::new(invoice);
    view! {<section class="ops-payment" data-testid="invoice-payments-panel">
        <h4>"Payments on this invoice"</h4>
        {list}
        <Show when=move||work.permits("billing.settle")>{move||settle_forms(work, &row.get_value())}</Show>
    </section>}
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(permissions: Value) -> Workbench {
        let work = Workbench::new(Domain::Billing);
        work.data
            .set(json!({"permissions":permissions,"today":"2026-11-14"}));
        work.ready.set(true);
        work
    }
    fn events() -> Value {
        json!([
            {"id":"e1","origin":"gateway","method":null,"providerId":"midtrans:main","outcome":"succeeded","amount":10000,"recordedOn":"2026-11-10","receivedOn":null,"reference":null,"reason":null,"recordedBy":"payment-gateway","recordedByName":null,"reversedAmount":0},
            {"id":"e2","origin":"manual","method":"bank_transfer","providerId":"manual:bank_transfer","outcome":"succeeded","amount":15000,"recordedOn":"2026-11-13","receivedOn":"2026-11-12","reference":"TRF/8842","reason":"Seen on the statement","recordedBy":"alice","recordedByName":"Alice <Finance>","reversedAmount":5000},
            {"id":"e3","origin":"manual","method":"cash","providerId":"manual:cash","outcome":"succeeded","amount":2000,"recordedOn":"2026-11-13","receivedOn":"2026-11-13","reference":"KW-0007","reason":"Paid at the counter","recordedBy":"alice","recordedByName":"","reversedAmount":0},
            {"id":"e4","origin":"manual","method":"bank_transfer","providerId":"manual:bank_transfer","outcome":"refunded","amount":5000,"recordedOn":"2026-11-14","receivedOn":null,"reference":null,"reason":"Part of the transfer bounced","recordedBy":"alice","recordedByName":"Alice <Finance>","reversesEventId":"e2","reversedAmount":0}
        ])
    }

    #[test]
    fn the_receipt_form_defaults_to_the_outstanding_amount_and_today_in_the_invoice_currency() {
        Owner::new().with(|| {
            let work = loaded(json!(["billing.read", "billing.settle"]));
            let idr = json!({"id":"i1","revision":2,"status":"issued","currency":"IDR","outstanding":30000,"payments":[]});
            let html = view! {<InvoicePayments work invoice=idr.clone()/>}.to_html();
            assert!(html.contains("data-testid=\"invoice-receipt-form\""), "{html}");
            assert!(html.contains("Record a payment received"), "{html}");
            assert!(html.contains("Amount received · IDR"), "{html}");
            assert!(html.contains("data-testid=\"invoice-receipt-date\" type=\"date\""), "{html}");
            assert!(html.contains(">Bank transfer<") && html.contains(">Cash<"), "{html}");
            assert!(html.contains("No payment has been recorded on this invoice."), "{html}");
            assert!(html.contains("A payment recorded here is manual"), "{html}");
            assert!(html.contains("does not check it with a bank or a payment provider"), "{html}");
            assert!(!html.to_lowercase().contains("verified"), "{html}");
            // IDR is entered in whole Rupiah without a step.
            assert!(!html.contains("step=\"0.01\""), "{html}");
            assert_eq!(
                receipt_command(&idr, 30000, "bank_transfer", " 2026-11-13 ", " TRF/8842 ", " Seen on the statement "),
                json!({"type":"record_manual_receipt","invoiceId":"i1","expectedRevision":2,"amountMinor":30000,"method":"bank_transfer","receivedOn":"2026-11-13","reference":"TRF/8842","reason":"Seen on the statement"})
            );

            // USD is entered with its two decimals; the stored amount is cents.
            let usd = json!({"id":"i2","revision":3,"status":"issued","currency":"USD","outstanding":1250,"payments":[]});
            let html = view! {<InvoicePayments work invoice=usd.clone()/>}.to_html();
            assert!(html.contains("Amount received · USD"), "{html}");
            assert!(html.contains("step=\"0.01\""), "{html}");
            assert_eq!(trade_money::trade_decimal("USD", 1250).as_deref(), Some("12.50"));
            assert_eq!(trade_money::trade_decimal("IDR", 30000).as_deref(), Some("30000"));
            assert_eq!(
                receipt_command(&usd, 1250, "cash", "2026-11-14", "KW-1", "Counter")["amountMinor"],
                1250
            );

            // No form on a draft, settled or void invoice, or when nothing is owed.
            for (status, outstanding) in [("draft", 30000), ("settled", 0), ("void", 30000), ("issued", 0)] {
                let invoice = json!({"id":"i1","revision":2,"status":status,"currency":"IDR","outstanding":outstanding,"payments":[]});
                let html = view! {<InvoicePayments work invoice/>}.to_html();
                assert!(html.contains("invoice-payments-panel"), "{html}");
                assert!(!html.contains("invoice-receipt-form"), "{status} {html}");
            }
        });
    }

    #[test]
    fn the_payments_list_shows_the_three_origins_and_offers_reversal_for_manual_receipts_only() {
        Owner::new().with(|| {
            let work = loaded(json!(["billing.read", "billing.settle"]));
            let invoice = json!({"id":"i1","revision":6,"status":"issued","currency":"IDR","outstanding":8000,"payments":events()});
            let html = view! {<InvoicePayments work invoice=invoice.clone()/>}.to_html();
            assert_eq!(html.matches("data-testid=\"invoice-payment-row\"").count(), 4, "{html}");
            assert!(html.contains(">Payment gateway · midtrans:main<"), "{html}");
            assert!(html.contains(">Manual · bank transfer<"), "{html}");
            assert!(html.contains(">Manual · cash<"), "{html}");
            assert!(html.contains(">Payment verified by the gateway<"), "{html}");
            assert!(html.contains(">Payment received<"), "{html}");
            assert!(html.contains(">Reversal<"), "{html}");
            // The manual receipt is dated the day the money was received.
            assert!(html.contains(">2026-11-12<"), "{html}");
            assert!(html.contains(">Rp 15.000 · Rp 5.000 reversed<"), "{html}");
            assert!(html.contains(">TRF/8842 · Seen on the statement<"), "{html}");
            assert!(html.contains(">Part of the transfer bounced<"), "{html}");
            // The recorder's name is text, never markup; the id stands in for a blank name.
            assert!(html.contains(">Alice &lt;Finance&gt;<"), "{html}");
            assert!(html.contains(">payment-gateway<"), "{html}");
            // Only a manual row says manual; only a gateway row says verified.
            let rows: Vec<&str> = html.split("data-testid=\"invoice-payment-row\"").skip(1).collect();
            assert!(rows[0].contains("data-origin=\"gateway\"") && !rows[0].split("</tr>").next().unwrap().contains("Manual"), "{html}");
            for manual in &rows[1..] {
                let row = manual.split("</tr>").next().unwrap();
                assert!(row.contains("data-origin=\"manual\""), "{row}");
                assert!(!row.to_lowercase().contains("verified"), "{row}");
            }
            // Two manual receipts have something left to reverse; the gateway
            // payment and the reversal offer nothing.
            assert_eq!(html.matches("data-testid=\"invoice-receipt-reversal\"").count(), 2, "{html}");
            assert!(html.contains("Reverse the manual · bank transfer of 2026-11-12 · TRF/8842"), "{html}");
            assert!(html.contains("Reverse the manual · cash of 2026-11-13 · KW-0007"), "{html}");
            let payments = events();
            assert_eq!(unreversed(&payments[1]), 10000);
            assert!(!reversible(&payments[0]) && reversible(&payments[1]) && !reversible(&payments[3]));
            let mut spent = payments[2].clone();
            spent["reversedAmount"] = json!(2000);
            assert!(!reversible(&spent));
            assert_eq!(
                reversal_command(&invoice, &payments[1], 10000, " Bounced "),
                json!({"type":"reverse_manual_receipt","invoiceId":"i1","expectedRevision":6,"receiptEventId":"e2","amountMinor":10000,"reason":"Bounced"})
            );
        });
    }

    #[test]
    fn without_the_settle_permission_the_payments_are_listed_and_no_form_is_rendered() {
        Owner::new().with(|| {
            let work = loaded(json!(["billing.read", "billing.write"]));
            let invoice = json!({"id":"i1","revision":6,"status":"issued","currency":"IDR","outstanding":8000,"payments":events()});
            let html = view! {<InvoicePayments work invoice/>}.to_html();
            assert_eq!(html.matches("data-testid=\"invoice-payment-row\"").count(), 4, "{html}");
            assert!(!html.contains("invoice-receipt-form"), "{html}");
            assert!(!html.contains("invoice-receipt-save"), "{html}");
            assert!(!html.contains("invoice-receipt-reversal"), "{html}");
            assert!(!html.contains("invoice-reversal-save"), "{html}");
        });
    }
}
