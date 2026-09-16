//! Main-thread handle for the search Web Worker.
//!
//! The engine search is CPU-bound and synchronous. Run on the UI thread it
//! freezes the tab for the whole think — up to 20 s on `Hard` — so it runs in a
//! Worker instead and the result arrives as a message.
//!
//! One worker is created lazily and reused: spawning one costs a fresh download
//! and instantiation of the WASM module, which would be a large fraction of an
//! `Easy` budget if paid per move.
//!
//! **Failure is asynchronous.** `new Worker(url)` does *not* throw when the
//! script is missing or broken — the constructor returns fine and an `error`
//! event arrives later. Without handling that, a bad deploy leaves the game
//! wedged on "Thinking…" forever rather than falling back. So an outstanding
//! request is completed with `None` on `onerror`, and the worker is marked dead
//! so later searches skip it entirely.
//!
//! **Stale replies.** The worker cannot be interrupted mid-search — the engine
//! blocks its thread — so a "New Game" pressed while it is thinking still gets
//! the old reply eventually. Every request carries a `request_id` and only the
//! most recently issued one is honoured; anything else is dropped. [`cancel`]
//! invalidates the outstanding request without waiting for it.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, Worker};

use crate::game::{WorkerMessage, WorkerRequest, WorkerResponse};

/// Worker bootstrap in the Trunk `dist/` output.
///
/// Not `search_worker.js` itself: Trunk emits that as a `--target no-modules`
/// bundle which defines `wasm_bindgen` without calling it, so a Worker built
/// straight from it would never run `main`. The loader initialises it.
///
/// Stable rather than content-hashed because this code has to name it —
/// `filehash = false` in `Trunk.toml` is what keeps the two in step.
const WORKER_URL: &str = "./search_worker_loader.js";

/// What the caller is told about a search in flight.
pub enum SearchEvent {
    /// An iteration completed at this depth. More events will follow.
    Progress(u8),
    /// The search finished.
    Done(WorkerResponse),
    /// The worker failed after accepting the request; run the search yourself.
    Failed,
}

type ResponseHandler = Box<dyn FnMut(SearchEvent)>;

struct Inner {
    worker: Worker,
    /// Callback for the outstanding request, if any.
    handler: Rc<RefCell<Option<ResponseHandler>>>,
    /// Id of the most recent request. Replies carrying anything else are stale.
    latest_id: Rc<RefCell<u64>>,
    /// Kept alive for as long as the worker is: dropping these detaches the
    /// JS event handlers.
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onerror: Closure<dyn FnMut(ErrorEvent)>,
}

enum Slot {
    /// No worker created yet.
    Uninit,
    Live(Inner),
    /// Tried and failed. Never retried: if the script is missing it will still
    /// be missing next move, and each retry costs the player a stalled turn.
    Dead,
}

thread_local! {
    static WORKER: RefCell<Slot> = const { RefCell::new(Slot::Uninit) };
}

/// Deliver a non-terminal event, leaving the handler in place for the next one.
fn notify_pending(handler: &Rc<RefCell<Option<ResponseHandler>>>, event: SearchEvent) {
    if let Some(f) = handler.borrow_mut().as_mut() {
        f(event);
    }
}

/// Deliver a terminal event, taking the handler first.
///
/// Taken before calling because the callback re-enters this module — it starts
/// the next search — and leaving the borrow open would panic.
fn complete_pending(handler: &Rc<RefCell<Option<ResponseHandler>>>, event: SearchEvent) {
    let taken = handler.borrow_mut().take();
    if let Some(mut f) = taken {
        f(event);
    }
}

fn spawn() -> Option<Inner> {
    let worker = Worker::new(WORKER_URL).ok()?;

    let handler: Rc<RefCell<Option<ResponseHandler>>> = Rc::new(RefCell::new(None));
    let latest_id = Rc::new(RefCell::new(0u64));

    let msg_handler = Rc::clone(&handler);
    let msg_latest = Rc::clone(&latest_id);
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(payload) = event.data().as_string() else {
            return;
        };
        let Ok(message) = serde_json::from_str::<WorkerMessage>(&payload) else {
            return;
        };
        // Drop anything belonging to a superseded request.
        match message {
            WorkerMessage::Progress { request_id, depth } => {
                if request_id == *msg_latest.borrow() {
                    notify_pending(&msg_handler, SearchEvent::Progress(depth));
                }
            }
            WorkerMessage::Done(response) => {
                if response.request_id == *msg_latest.borrow() {
                    complete_pending(&msg_handler, SearchEvent::Done(response));
                }
            }
        }
    });
    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    let err_handler = Rc::clone(&handler);
    let onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |_event: ErrorEvent| {
        WORKER.with(|cell| {
            // Replace rather than mutate: this drops the Worker and its
            // closures, and every later `request_search` now returns false.
            *cell.borrow_mut() = Slot::Dead;
        });
        complete_pending(&err_handler, SearchEvent::Failed);
    });
    worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

    Some(Inner {
        worker,
        handler,
        latest_id,
        _onmessage: onmessage,
        _onerror: onerror,
    })
}

/// Post a search to the worker.
///
/// `on_event` receives zero or more [`SearchEvent::Progress`] events followed by
/// exactly one terminal [`SearchEvent::Done`] or [`SearchEvent::Failed`].
///
/// Returns `false` if the request could not be handed over at all, which is the
/// same signal `Failed` carries, delivered synchronously. Supersedes any
/// outstanding request.
pub fn request_search(
    request: &WorkerRequest,
    on_event: impl FnMut(SearchEvent) + 'static,
) -> bool {
    WORKER.with(|cell| {
        {
            let mut slot = cell.borrow_mut();
            if matches!(*slot, Slot::Uninit) {
                *slot = match spawn() {
                    Some(inner) => Slot::Live(inner),
                    None => Slot::Dead,
                };
            }
        }

        let slot = cell.borrow();
        let Slot::Live(inner) = &*slot else {
            return false;
        };
        let Ok(payload) = serde_json::to_string(request) else {
            return false;
        };
        *inner.latest_id.borrow_mut() = request.request_id;
        *inner.handler.borrow_mut() = Some(Box::new(on_event));
        inner
            .worker
            .post_message(&JsValue::from_str(&payload))
            .is_ok()
    })
}

/// Abandon the outstanding request. Its reply, whenever it lands, is ignored.
///
/// The search itself keeps running to completion inside the worker: it cannot
/// be interrupted. This only guarantees it will not touch the new game.
pub fn cancel() {
    let id = next_request_id();
    WORKER.with(|cell| {
        if let Slot::Live(inner) = &*cell.borrow() {
            *inner.handler.borrow_mut() = None;
            // Burn a fresh id from the same counter as real requests: that is
            // what guarantees nothing in flight — and nothing issued later —
            // can collide with it.
            *inner.latest_id.borrow_mut() = id;
        }
    });
}

/// Monotonic request ids. Wrapping is not a concern: at one search per move,
/// `u64` outlasts the universe.
pub fn next_request_id() -> u64 {
    thread_local! {
        static COUNTER: RefCell<u64> = const { RefCell::new(0) };
    }
    COUNTER.with(|c| {
        let mut c = c.borrow_mut();
        *c += 1;
        *c
    })
}
