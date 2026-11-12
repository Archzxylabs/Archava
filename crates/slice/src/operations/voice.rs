//! Leptos owns the view and session; the browser binding owns finite media tracks.
use super::*;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=ArchavaVoice,js_name=start)]
    fn start_voice(tenant: &str, csrf: &str, camera: bool);
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=ArchavaVoice,js_name=stop)]
    fn stop_voice();
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=ArchavaVoice,js_name=getInput)]
    fn voice_input() -> String;
}
#[component]
pub(super) fn Voice() -> impl IntoView {
    let camera = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    #[cfg(target_arch = "wasm32")]
    let draft = expect_context::<crate::agent::AgentDraft>();
    #[cfg(target_arch = "wasm32")]
    let navigate = use_navigate();
    #[cfg(target_arch = "wasm32")]
    on_cleanup(stop_voice);
    let start = move |_| {
        #[cfg(target_arch = "wasm32")]
        if let Some(held) = session::held() {
            start_voice(&held.tenant, &held.csrf, camera.get_untracked());
        }
    };
    let stop = move |_| {
        #[cfg(target_arch = "wasm32")]
        stop_voice();
    };
    let plan = move |_| {
        #[cfg(target_arch = "wasm32")]
        {
            let text = voice_input();
            if text.trim().is_empty() {
                notice.set_if_alive(
                    "Speak your business request first, then use it as a goal.".into(),
                );
                return;
            }
            draft.0.set_if_alive(Some(text));
            stop_voice();
            navigate("/app/desk", Default::default());
        }
    };
    view! {<section data-testid="voice-workspace"><h2>"Talk with Ava"</h2><p>"Start a conversation using your microphone. Enable the camera to share visual context. Business changes use the review and confirmation screens."</p>
        <label class="ops-check"><input type="checkbox" data-testid="voice-camera" prop:checked=move||camera.get() on:change=move|e|camera.set_if_alive(event_target_checked(&e))/><span>"Share camera during this session"</span></label>
        <div class="ops-actions"><button class="btn" type="button" data-testid="voice-start" on:click=start>"Start conversation"</button><button class="btn secondary" type="button" data-testid="voice-stop" on:click=stop>"Stop microphone and camera"</button><button class="btn secondary" type="button" data-testid="voice-plan-goal" on:click=plan>"Plan from my spoken request"</button><a href="/app/desk">"Continue in chat"</a></div>
        <p role="status" aria-live="polite" data-testid="voice-status" data-state="idle">"Microphone and camera are off."</p><video data-testid="voice-preview" autoplay muted playsinline aria-label="Your camera preview"></video>
        <p class="hint">"Your recorded words can start a business plan. Business changes still require review and approval."</p><p role="status">{move||notice.get()}</p>
        <pre class="voice-transcript" aria-live="polite" data-testid="voice-transcript"></pre>
    </section>}
}
