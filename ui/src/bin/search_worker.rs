//! Web Worker binary: runs the engine search off the UI thread.
//!
//! This file is plumbing only. It owns no game logic — it decodes a
//! [`WorkerRequest`], hands it to [`handle_request`], and posts the
//! [`WorkerResponse`] back. Everything worth testing lives in
//! `hive_ui::game::worker` and is exercised on the native target.
//!
//! Messages cross as JSON strings rather than structured-clone objects: the
//! engine's `serde` feature already gives us `Move`, so one `serde_json` call
//! per side is simpler than mirroring the types in JS, and a search that takes
//! seconds is not going to notice the encoding cost.

use hive_ui::game::{handle_request, WorkerRequest};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

fn main() {
    // Route Rust panics to the devtools console. A panic in here is otherwise
    // an opaque silent worker death.
    console_error_panic_hook::set_once();

    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let post_scope = scope.clone();

    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(payload) = event.data().as_string() else {
            return;
        };
        let request: WorkerRequest = match serde_json::from_str(&payload) {
            Ok(request) => request,
            Err(_) => return,
        };

        // `performance.now()` is monotonic and high-resolution; `Date::now()` is
        // the universally available fallback. Either is fine — the engine polls
        // the predicate every 1024 nodes, so millisecond resolution is ample.
        let response = match post_scope.performance() {
            Some(perf) => handle_request(&request, move || perf.now()),
            None => handle_request(&request, js_sys::Date::now),
        };

        if let Ok(encoded) = serde_json::to_string(&response) {
            let _ = post_scope.post_message(&JsValue::from_str(&encoded));
        }
    });

    scope.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    // The closure must outlive this function: it is the worker's whole purpose.
    onmessage.forget();
}
