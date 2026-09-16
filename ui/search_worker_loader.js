// Bootstrap for the search Web Worker.
//
// Trunk's `data-type="worker"` emits a `--target no-modules` bundle: loading
// `search_worker.js` directly defines `wasm_bindgen` but never calls it, so the
// Rust `main()` would never run. This file is the thing the main thread actually
// constructs a Worker from; it loads that bundle and initialises it.
//
// The buffering matters. `wasm_bindgen(...)` resolves asynchronously, and Rust
// only installs its `onmessage` once it does — but the main thread posts its
// first search as soon as `new Worker(...)` returns. A message arriving in that
// window has no listener and is silently dropped, which would strand the AI on
// its first move of the game. So we claim `onmessage` up front, queue anything
// that arrives, and replay it once Rust has taken over.

let pending = [];
self.onmessage = (event) => pending.push(event.data);

importScripts("./search_worker.js");

wasm_bindgen("./search_worker_bg.wasm")
  .then(() => {
    // Rust's `set_onmessage` has replaced the buffering handler above, so these
    // dispatch into the real one.
    const queued = pending;
    pending = [];
    for (const data of queued) {
      self.dispatchEvent(new MessageEvent("message", { data }));
    }
  })
  .catch((err) => {
    // Without this the failure is an invisible dead worker. The main thread
    // cannot see this, but it shows up in the devtools console.
    console.error("search worker failed to initialise:", err);
  });
