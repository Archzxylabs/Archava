use super::*;

#[derive(Clone, Copy)]
struct Panel {
    invoice: StoredValue<String>,
    work: Workbench,
    data: RwSignal<Value>,
    busy: RwSignal<bool>,
    notice: RwSignal<String>,
    pending: RwSignal<Option<String>>,
    review: RwSignal<bool>,
    integration: RwSignal<String>,
}
impl Panel {
    fn scope(self, principal: &str) -> String {
        format!("billing-payment:{principal}:{}", self.invoice.get_value())
    }
    fn refresh(self) {
        if self.data.try_get_untracked().is_none() {
            return;
        }
        let Some(held) = session::held() else { return };
        let path = format!(
            "/erp/billing/payments?tenant={}&invoiceId={}",
            crate::contacts::query_encode(&held.tenant),
            self.invoice.get_value()
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer)
                    if answer.status == 200
                        && answer.body["links"].is_array()
                        && answer.body["integrations"].is_array() =>
                {
                    if self
                        .integration
                        .try_get_untracked()
                        .is_some_and(|v| v.is_empty())
                    {
                        self.integration.set_if_alive(
                            answer.body["integrations"][0].as_str().unwrap_or("").into(),
                        );
                    }
                    self.data.set_if_alive(answer.body);
                }
                _ => self.notice.set_if_alive(
                    "Payment details could not be loaded. Refresh before continuing.".into(),
                ),
            }
        });
    }
    fn recover(self) {
        let (Some(held), Some(key)) = (session::held(), self.pending.get_untracked()) else {
            return;
        };
        self.busy.set_if_alive(true);
        self.review.set_if_alive(false);
        let invoice = self.invoice.get_value();
        let path = format!(
            "/erp/billing/payments/status?tenant={}&requestId={key}",
            crate::contacts::query_encode(&held.tenant)
        );
        spawn_local(async move {
            match session::api_fetch("GET",&path,None).await {
                Ok(answer) if answer.status==200 && answer.body["status"]=="recorded" && answer.body["requestId"]==key && answer.body["link"]["invoiceId"]==invoice=>{
                    pending::clear_by_request_id(&held.tenant,&held.principal,&key);
                    self.pending.set_if_alive(None);self.notice.set_if_alive("Original payment request found. Its gateway status appears below.".into());self.refresh();
                },
                Ok(answer) if answer.status==200 && answer.body["status"]=="unknown" && answer.body["requestId"]==key=>{
                    self.review.set_if_alive(true);self.notice.set_if_alive("No receipt was found yet. Review the provider below, then retry with the original request reference.".into());
                },
                _=>self.notice.set_if_alive("The original request could not be confirmed. Keep its reference and check again.".into()),
            }
            self.busy.set_if_alive(false);
        });
    }
    fn create(self) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(held) = session::held() else { return };
        let old = self.pending.get_untracked();
        if old.is_some() && !self.review.get_untracked() {
            return;
        }
        let key = old.clone().unwrap_or_else(uuid_string);
        let provider = self.integration.get_untracked();
        if provider.is_empty() {
            self.notice
                .set_if_alive("Select a configured payment provider.".into());
            return;
        }
        let marker = PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            self.scope(&held.principal),
            held.principal.clone(),
        );
        if pending::write(&marker).is_err() {
            self.notice.set_if_alive(
                "Could not preserve this request for recovery. Payment request was not sent."
                    .into(),
            );
            return;
        }
        self.pending.set_if_alive(Some(key.clone()));
        self.busy.set_if_alive(true);
        self.review.set_if_alive(false);
        let invoice = self.invoice.get_value();
        let body=json!({"tenant":held.tenant,"invoiceId":invoice,"requestId":key,"integrationId":provider}).to_string();
        spawn_local(async move {
            match session::api_fetch("POST","/erp/billing/payments",Some(body)).await {
                Ok(answer) if answer.status==200 && answer.body["status"]=="recorded" && answer.body["requestId"]==key && answer.body["link"]["invoiceId"]==invoice=>{
                    pending::clear_by_request_id(&held.tenant,&held.principal,&key);self.pending.set_if_alive(None);
                    self.notice.set_if_alive("Payment request saved. Share its checkout link when available; payment is confirmed separately.".into());self.refresh();
                },
                Ok(answer) if (400..500).contains(&answer.status)&&old.is_none()=>{
                    pending::clear_by_request_id(&held.tenant,&held.principal,&key);self.pending.set_if_alive(None);
                    self.notice.set_if_alive("The request was refused. Refresh the invoice and check for an existing payment request.".into());self.refresh();
                },
                _=>self.notice.set_if_alive("The request outcome is unconfirmed. Check the original reference before trying again.".into()),
            }
            self.busy.set_if_alive(false);
        });
    }
    fn check(self, id: String) {
        if self.busy.get_untracked() || self.pending.get_untracked().is_some() {
            return;
        }
        let Some(held) = session::held() else { return };
        self.busy.set_if_alive(true);
        let body = json!({"tenant":held.tenant,"invoiceId":self.invoice.get_value(),"linkId":id})
            .to_string();
        spawn_local(async move {
            match session::api_fetch("POST","/erp/billing/payments/check",Some(body)).await {
                Ok(answer) if answer.status==200 && answer.body["status"]=="verified"=>{
                    self.notice.set_if_alive("Status checked directly with the gateway.".into());self.refresh();
                    if self.work.ready.try_get_untracked().is_some(){self.work.refresh();}
                },
                Ok(answer) if answer.status==429=>self.notice.set_if_alive("A status check is already in progress. Wait a moment, then refresh.".into()),
                _=>self.notice.set_if_alive("The gateway status could not be confirmed. The existing payment reference has been kept.".into()),
            }
            self.busy.set_if_alive(false);
        });
    }
}

#[component]
pub fn PaymentPanel(
    work: Workbench,
    invoice_id: String,
    issued: bool,
    outstanding: i64,
) -> impl IntoView {
    let panel = Panel {
        invoice: StoredValue::new(invoice_id),
        work,
        data: RwSignal::new(json!({})),
        busy: RwSignal::new(false),
        notice: RwSignal::new(String::new()),
        pending: RwSignal::new(None),
        review: RwSignal::new(false),
        integration: RwSignal::new(String::new()),
    };
    Effect::new(move |_| {
        if let Some(held) = session::held() {
            if let Some(marker) =
                pending::read_scoped(&held.tenant, &panel.scope(&held.principal), &held.principal)
            {
                panel.pending.set_if_alive(Some(marker.request_id));
            }
            panel.refresh();
        }
    });
    view! {<section class="ops-payment" data-testid="invoice-payment"><div class="ops-actions"><h4>"Online payment"</h4><button type="button" class="btn secondary" on:click=move|_|panel.refresh() disabled=move||panel.busy.get()>"Refresh payment details"</button></div>
      <p class="hint">"Checkout links are created by the configured gateway. Invoice totals change only after its status has been verified."</p>
      <p role="status">{move||panel.notice.get()}</p>
      {move||panel.pending.get().map(|reference|view!{<div class="ops-recovery"><p>"An earlier payment request needs a receipt check."</p><p class="hint">{format!("Reference: {reference}")}</p><button type="button" class="btn secondary" data-testid="payment-recover" disabled=move||panel.busy.get() on:click=move|_|panel.recover()>"Check original request"</button>
        <Show when=move||panel.review.get()><button type="button" class="btn" data-testid="payment-retry" disabled=move||panel.busy.get() on:click=move|_|panel.create()>"Retry reviewed request"</button></Show></div>})}
      {move||panel.data.get()["links"].as_array().cloned().unwrap_or_default().into_iter().map(|link|{
        let id=text(&link,"id");let state=text(&link,"state");let detail=match state.as_str(){"creating"|"creation_unknown"=>"Gateway creation is unconfirmed. Keep this reference and check the merchant dashboard; creating another link is blocked.","open"|"pending"=>"Waiting for customer payment. Opening checkout does not confirm payment.","failed"=>"Gateway reports this payment failed or expired.","refunded"=>"A verified refund has been recorded.","settled"=>"Gateway payment verified.",_=>"This payment needs reconciliation in the merchant dashboard."};
        view!{<article class="ops-payment-record"><strong>{state.clone()}</strong><p>{detail}</p><dl class="ops-totals"><dt>"Requested"</dt><dd>{money(integer(&link,"amountMinor"))}</dd><dt>"Received"</dt><dd>{money(integer(&link,"receivedMinor"))}</dd><dt>"Refunded"</dt><dd>{money(integer(&link,"refundedMinor"))}</dd></dl>
          {(integer(&link,"unappliedMinor")>0).then(||view!{<p role="status">{format!("{} remains unapplied. Review or refund it in the merchant dashboard, then check status again.",money(integer(&link,"unappliedMinor")))}</p>})}
          <p class="hint">{text(&link,"orderId")}</p>
          {link["redirectUrl"].as_str().filter(|_|matches!(state.as_str(),"open"|"pending")).map(|url|view!{<a class="btn" data-testid="payment-checkout" href=url.to_string() target="_blank" rel="noopener noreferrer">"Open checkout"</a>})}
          <button type="button" class="btn secondary" data-testid="payment-check" disabled=move||panel.busy.get()||panel.pending.get().is_some() on:click=move|_|panel.check(id.clone())>"Check gateway status"</button></article>}
      }).collect::<Vec<_>>()}
      <Show when=move||panel.data.get()["integrations"].as_array().is_some_and(|a|a.is_empty())><p class="hint">"Online payment is not configured for this workspace. Ask the workspace administrator to configure a gateway."</p></Show>
      <Show when=move||issued&&(outstanding>0)&&panel.data.get()["canCreate"]==true&&panel.data.get()["integrations"].as_array().is_some_and(|a|!a.is_empty())>
        <label>"Payment provider"<select prop:value=move||panel.integration.get() on:change=move|e|panel.integration.set_if_alive(event_target_value(&e))>{move||panel.data.get()["integrations"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|i|i.as_str().map(str::to_string)).map(|id|view!{<option value=id.clone()>{id.clone()}</option>}).collect::<Vec<_>>()}</select></label>
        <button type="button" class="btn" data-testid="payment-create" disabled=move||panel.busy.get()||panel.pending.get().is_some()||panel.data.get()["links"].as_array().is_some_and(|links|links.iter().any(|l|integer(l,"receivedMinor")==0&&matches!(l["state"].as_str(),Some("creating"|"creation_unknown"|"open"|"pending"|"review")))) on:click=move|_|panel.create()>"Create payment link"</button>
      </Show>
    </section>}
}
