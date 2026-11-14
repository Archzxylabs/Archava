use super::*;
#[component]
pub fn Support() -> impl IntoView {
    let work = Workbench::new(Domain::Support);
    let requester = RwSignal::new(String::new());
    let subject = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let selected = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let detail = RwSignal::new(json!({}));
    Effect::new(move |_| {
        let id = selected.get();
        let _ = work.data.get();
        if id.is_empty() {
            return;
        }
        let Some(held) = session::held() else { return };
        let path = format!(
            "/erp/support/cases/detail?tenant={}&id={}",
            crate::contacts::query_encode(&held.tenant),
            id
        );
        spawn_local(async move {
            match session::api_fetch("GET", &path, None).await {
                Ok(answer)
                    if answer.status == 200 && selected.try_get_untracked() == Some(id.clone()) =>
                {
                    detail.set_if_alive(answer.body)
                }
                _ => detail.set_if_alive(json!({})),
            }
        });
    });
    let action_record = RwSignal::new(json!({}));
    Effect::new(move |_| {
        action_record.set_if_alive(
            work.rows("items")
                .into_iter()
                .find(|r| r["id"] == selected.get())
                .map(|mut row| {
                    row["_collection"] = json!("items");
                    row
                })
                .unwrap_or_else(|| json!({})),
        );
    });
    view! {<Toolbar work title="Support cases"/><Feedback work/><div class="ops-layout"><section class="ops-main"><Records work collection="items" columns=vec![("subject","Subject"),("requesterName","Requester"),("priority","Priority"),("state","Status")] selected/>
    {move||work.rows("items").into_iter().find(|p|p["id"]==selected.get()).map(|case|{
     let id=text(&case,"id");let revision=integer(&case,"revision");let state=text(&case,"state");let start=id.clone();let wait=id.clone();let resume=id.clone();let resolve=id.clone();let close=id.clone();let reopen=id.clone();let note_id=id;
     view!{<section class="ops-detail" data-testid="case-detail"><p class="eyebrow">"SUPPORT CASE"</p><h3>{text(&case,"subject")}</h3><p>{text(&case,"description")}</p><p>{format!("{} · {}",text(&case,"requesterName"),state)}</p>
      <div hidden=move||!work.permits("cases.write")><Field label="Transition reason / resolution" id="case-reason" value=reason/><div class="ops-actions">
       {(state=="open").then(||view!{<button type="button" class="btn" data-testid="case-start" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.start","id":start,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Start work"</button>})}
       {(state=="in_progress").then(||view!{<button type="button" class="btn secondary" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.wait","id":wait,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Waiting for reply"</button>})}
       {(state=="waiting").then(||view!{<button type="button" class="btn secondary" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.resume","id":resume,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Resume work"</button>})}
       {(matches!(state.as_str(),"open"|"in_progress"|"waiting")).then(||view!{<button type="button" class="btn" data-testid="case-resolve" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.resolve","id":resolve,"expectedRevision":revision,"resolution":reason.get_untracked()}))>"Resolve case"</button>})}
       {(state=="resolved").then(||view!{<button type="button" class="btn secondary" data-testid="case-close" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.close","id":close,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Close case"</button>})}
       {(state=="resolved"||state=="closed").then(||view!{<button type="button" class="btn secondary" data-testid="case-reopen" disabled=move||work.blocked() on:click=move|_|work.send(json!({"operation":"case.reopen","id":reopen,"expectedRevision":revision,"reason":reason.get_untracked()}))>"Reopen case"</button>})}
      </div></div>
      <div hidden=move||!work.permits("notes.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();work.send(json!({"operation":"note.add","id":note_id,"expectedRevision":revision,"body":note.get_untracked()}));}><Field label="Internal note" id="case-note" value=note/><button class="btn secondary" type="submit" data-testid="case-note-save" disabled=move||work.blocked()>"Add note"</button></form></div>
      <h4>"History"</h4>{move||{let data=detail.get();view!{<ul class="ops-lines">{data["notes"].as_array().cloned().unwrap_or_default().into_iter().map(|n|view!{<li><strong>{text(&n,"authorId")}</strong>" · "{text(&n,"body")}</li>}).collect::<Vec<_>>()}{data["transitions"].as_array().cloned().unwrap_or_default().into_iter().map(|t|view!{<li>{format!("{} · {} → {}",text(&t,"occurredAt"),text(&t,"from"),text(&t,"to"))}</li>}).collect::<Vec<_>>()}</ul>}}}<p class="hint">"History is bounded; the API exposes continuation flags for longer records."</p>
     </section>}
    })}
    </section><aside class="ops-aside"><div hidden=move||!work.permits("cases.write")><form class="ops-form" on:submit=move|e|{e.prevent_default();work.send(json!({"operation":"case.create","requesterName":requester.get_untracked(),"subject":subject.get_untracked(),"description":description.get_untracked(),"priority":"normal"}));}><h3>"New support case"</h3><Field label="Requester name" id="case-requester" value=requester/><Field label="Subject" id="case-subject" value=subject/><label class="ops-field"><span>"Description"</span><textarea data-testid="case-description" prop:value=move||description.get() on:input=move|e|description.set_if_alive(event_target_value(&e)) required></textarea></label><button class="btn" type="submit" data-testid="case-save" disabled=move||work.blocked()>"Create case"</button></form></div><details class="ops-disclosure"><summary>"All support actions"</summary><operator::Actions work selected=action_record/></details></aside></div>}
}
