//! The browser's entry point into the hydrated app — wasm32 only.
//!
//! `hydrate` takes the markup the SSR pass rendered and ties this tree's
//! boots into it: the same `App`, compiled with Leptos's `hydrate` feature,
//! finds the server-rendered DOM and wires the event handlers and signals the
//! interactive pages own. Nothing is re-rendered from scratch.

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;

#[wasm_bindgen]
pub fn hydrate() {
    // Route panics into the console first: a hydrated tree that panics is a
    // bug the browser proof must SEE, not lose.
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&format!("slice panic: {info}")));
    }));

    // Leptos hydrates the tree the server rendered; the SSR shell embedded
    // the hydration state LeptosOptions produced.
    use crate::App;
    leptos::mount::hydrate_body(App);

    // Stamp THIS document's hydration. It is how `wasm-bindgen`'s single
    // `hydrate()` export proves it ran per document: the browser proof gate
    // reads the timestamp strictly after each navigation (and zero — zero is
    // the pre-hydration SSR value) as the exact boundary between "markup on
    // screen" and "signals live".
    let window = web_sys::window().expect("hydration runs in a browser window");
    js_sys::Reflect::set(
        &window,
        &JsValue::from_str("archavaSliceHydratedAt"),
        &JsValue::from(js_sys::Date::now()),
    )
    .expect("stamp the hydration");
}
