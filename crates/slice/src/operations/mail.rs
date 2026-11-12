use super::*;
use crate::draft::uuid_string;

fn terminal_receipt(body: &Value, request: &str, invoice: &str) -> bool {
    body["requestId"]
        .as_str()
        .or_else(|| body["request_id"].as_str())
        == Some(request)
        && body["invoiceId"]
            .as_str()
            .or_else(|| body["invoice_id"].as_str())
            == Some(invoice)
        && body["id"].as_str().is_some_and(|id| !id.is_empty())
        && matches!(body["state"].as_str(), Some("accepted" | "rejected"))
}

#[derive(Clone, Copy)]
struct EmailPanelState {
    invoice: StoredValue<String>,
    work: Workbench,
    recipient: RwSignal<String>,
    subject: RwSignal<String>,
    busy: RwSignal<bool>,
    notice: RwSignal<String>,
    emails: RwSignal<Vec<Value>>,
    pending: RwSignal<Option<String>>,
}

impl EmailPanelState {
    fn scope(self) -> String {
        format!("mail_{}", self.invoice.try_get_value().unwrap_or_default())
    }

    fn refresh(self) {
        let Some(held) = session::held() else { return };
        let Some(invoice_id) = self.invoice.try_get_value() else {
            return;
        };
        let path = format!(
            "/erp/billing/invoices/{invoice_id}/emails?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer) if answer.status == 200 && answer.body.is_array() => {
                    let items = answer.body.as_array().cloned().unwrap_or_default();
                    self.emails.set_if_alive(items.clone());

                    // If a pending request is being tracked, check if the server has its terminal receipt
                    if let Some(pending_req_id) = self.pending.try_get_untracked().flatten() {
                        if let Some(found) = items.iter().find(|e| {
                            e["requestId"] == pending_req_id || e["request_id"] == pending_req_id
                        }) {
                            let state = found["state"].as_str().unwrap_or("");
                            if terminal_receipt(found, &pending_req_id, &invoice_id) {
                                pending::clear_by_request_id(
                                    &held.tenant,
                                    &held.principal,
                                    &pending_req_id,
                                );
                                self.pending.set_if_alive(None);
                                self.notice
                                    .set_if_alive(format!("Email outcome confirmed: {state}"));
                            }
                            if self
                                .recipient
                                .try_get_untracked()
                                .is_some_and(|value| value.is_empty())
                            {
                                if let Some(rec) = found["recipient"].as_str() {
                                    self.recipient.set_if_alive(rec.to_string());
                                }
                            }
                            if self
                                .subject
                                .try_get_untracked()
                                .is_some_and(|value| value.is_empty())
                            {
                                if let Some(subj) = found["subject"].as_str() {
                                    self.subject.set_if_alive(subj.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }

    fn send(self) {
        let Some(held) = session::held() else { return };
        let Some(invoice_id) = self.invoice.try_get_value() else {
            return;
        };
        let recipient = self.recipient.get_untracked();
        if recipient.trim().is_empty() || !recipient.contains('@') {
            self.notice
                .set_if_alive("Enter a valid recipient email address.".into());
            return;
        }

        let old = self.pending.get_untracked();
        let key = old.clone().unwrap_or_else(uuid_string);
        let marker = PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            self.scope(),
            held.principal.clone(),
        );
        if pending::write(&marker).is_err() {
            self.notice.set_if_alive(
                "Could not preserve this request for recovery. Email was not sent.".into(),
            );
            return;
        }

        self.pending.set_if_alive(Some(key.clone()));
        self.busy.set_if_alive(true);
        self.notice.set_if_alive(String::new());

        let path = format!(
            "/erp/billing/invoices/{invoice_id}/send-email?tenant={}",
            crate::contacts::query_encode(&held.tenant)
        );

        let subject_text = self.subject.get_untracked();
        let body = json!({
            "requestId": key,
            "recipient": recipient,
            "subject": if subject_text.trim().is_empty() { None } else { Some(subject_text) },
            "message": "Please review your invoice details below."
        })
        .to_string();

        let pending_key = key.clone();
        spawn_local(async move {
            match session::api_fetch("POST", &path, Some(body)).await {
                Ok(answer) if answer.status == 200 => {
                    let state = answer.body["state"].as_str().unwrap_or("unknown");
                    if terminal_receipt(&answer.body, &pending_key, &invoice_id) {
                        pending::clear_by_request_id(&held.tenant, &held.principal, &pending_key);
                        self.pending.set_if_alive(None);
                    }
                    self.notice.set_if_alive(format!("Email status: {state}"));
                    self.refresh();
                }
                Ok(answer)
                    if answer.status == 409
                        && answer.body["reason"] == "email_already_recorded"
                        && answer.body["requestId"].as_str() == Some(pending_key.as_str())
                        && answer.body["invoiceId"].as_str() == Some(invoice_id.as_str())
                        && answer.body["duplicateOf"]
                            .as_str()
                            .is_some_and(|id| !id.is_empty() && id != pending_key) =>
                {
                    // The backend explicitly refused this key without an
                    // invocation; the original transmission stays in history.
                    pending::clear_by_request_id(&held.tenant, &held.principal, &pending_key);
                    self.pending.set_if_alive(None);
                    self.notice.set_if_alive("These invoice details already have an email transmission. Review the history before sending another message.".into());
                    self.refresh();
                }
                Ok(answer) if (400..500).contains(&answer.status) && old.is_none() => {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &pending_key);
                    self.pending.set_if_alive(None);
                    let reason = answer.body["reason"]
                        .as_str()
                        .unwrap_or("Transmission refused");
                    self.notice.set_if_alive(format!("Email refused: {reason}"));
                }
                Ok(answer) => {
                    let reason = answer.body["reason"]
                        .as_str()
                        .unwrap_or("Transmission failed");
                    self.notice
                        .set_if_alive(format!("Email status unconfirmed: {reason}"));
                }
                Err(_) => {
                    self.notice.set_if_alive(
                        "Network error while sending email. Original request reference is preserved."
                            .into(),
                    );
                }
            }
            self.busy.set_if_alive(false);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_matching_explicit_terminal_invoice_receipt_clears_recovery() {
        let accepted =
            json!({"id":"receipt","requestId":"request","invoiceId":"invoice","state":"accepted"});
        assert!(terminal_receipt(&accepted, "request", "invoice"));
        for body in [
            json!({"id":"receipt","requestId":"request","invoiceId":"invoice"}),
            json!({"id":"receipt","requestId":"request","invoiceId":"invoice","state":"unknown"}),
            json!({"id":"receipt","requestId":"different","invoiceId":"invoice","state":"accepted"}),
            json!({"id":"receipt","requestId":"request","invoiceId":"another","state":"accepted"}),
            json!({"requestId":"request","invoiceId":"invoice","state":"accepted"}),
        ] {
            assert!(!terminal_receipt(&body, "request", "invoice"), "{body}");
        }
    }
}

#[component]
pub fn EmailPanel(work: Workbench, invoice_id: String, issued: bool) -> impl IntoView {
    let panel = EmailPanelState {
        invoice: StoredValue::new(invoice_id),
        work,
        recipient: RwSignal::new(String::new()),
        subject: RwSignal::new(String::new()),
        busy: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        emails: RwSignal::new(Vec::new()),
        pending: RwSignal::new(None),
    };

    Effect::new(move |_| {
        if let Some(held) = session::held() {
            if let Some(marker) =
                pending::read_scoped(&held.tenant, &panel.scope(), &held.principal)
            {
                panel.pending.set_if_alive(Some(marker.request_id));
            }
        }
        panel.refresh();
    });

    view! {
        <section class="ops-email-panel" data-testid="invoice-email-panel" hidden=move||!issued>
            <h4>"Email Invoice"</h4>
            {move||(!panel.notice.get().is_empty()).then(||view!{
                <p class="notice" data-testid="email-notice">{panel.notice.get()}</p>
            })}
            {move||panel.pending.get().map(|reference|view!{
                <div class="ops-recovery" data-testid="email-pending-recovery">
                    <p>"An earlier email transmission needs confirmation."</p>
                    <p class="hint">{format!("Reference: {reference}")}</p>
                    <button type="button" class="btn secondary" data-testid="email-retry" disabled=move||panel.busy.get() on:click=move|_|panel.send()>"Retry original request"</button>
                </div>
            })}
            <div hidden=move||!panel.work.permits("billing.write")>
                <div class="ops-field">
                    <label for="email-recipient">"Recipient email"</label>
                    <input
                        type="email"
                        id="email-recipient"
                        data-testid="email-recipient"
                        placeholder="customer@example.com"
                        prop:value=move||panel.recipient.get()
                        on:input=move|e|panel.recipient.set_if_alive(event_target_value(&e))
                    />
                </div>
                <div class="ops-field">
                    <label for="email-subject">"Subject (optional)"</label>
                    <input
                        type="text"
                        id="email-subject"
                        data-testid="email-subject"
                        placeholder="Invoice notification"
                        prop:value=move||panel.subject.get()
                        on:input=move|e|panel.subject.set_if_alive(event_target_value(&e))
                    />
                </div>
                <button
                    type="button"
                    class="btn secondary"
                    data-testid="email-send-button"
                    disabled=move||panel.busy.get()||panel.pending.get().is_some()
                    on:click=move|_|panel.send()
                >
                    {move||if panel.busy.get(){"Sending…"}else{"Send email"}}
                </button>
            </div>
            {move||(!panel.emails.get().is_empty()).then(||view!{
                <div class="email-history">
                    <h5>"Sent emails"</h5>
                    <ul class="ops-lines">
                        {panel.emails.get().into_iter().map(|item|view!{
                            <li>
                                <span class="recipient">{text(&item,"recipient")}</span>
                                " · "
                                <span class="badge">{text(&item,"state")}</span>
                            </li>
                        }).collect::<Vec<_>>()}
                    </ul>
                </div>
            })}
        </section>
    }
}
