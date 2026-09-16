//! Web Worker AI protocol — message contract only.
//!
//! The future search worker runs `hive_engine::search::search_bounded` off the
//! UI thread. The main thread posts a [`WorkerRequest`] (the game's move list
//! plus the think-time budget); the worker replies with a [`WorkerResponse`]
//! carrying the chosen move and some lightweight stats.
//!
//! These are plain serde structs so they round-trip through `serde_json` and are
//! native-testable. No `web-sys` / wasm dependency lives here — the worker glue
//! that actually `postMessage`s these has not been written yet.
//!
//! Why the move list rather than a serialized `State`: the move list is the only
//! portable representation of a game today (the engine's `serde` feature covers
//! the leaf types and `Move` but not `State`). The worker reconstructs the
//! position by replaying the moves, exactly as the main thread does. See
//! `ui/engine-requests.md` #2.

use hive_engine::{Move, SearchStats};

use serde::{Deserialize, Serialize};

use crate::game::ai::search_with_progress;

/// Main thread → worker: "search this position within this budget."
///
/// `Eq` is not derived: `budget_ms` is an `f64`. Compare with `PartialEq`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkerRequest {
    /// Authoritative game record. The worker replays it from `State::new()` to
    /// reconstruct the position to search.
    pub moves: Vec<Move>,
    /// Wall-clock think-time budget in milliseconds (from the difficulty
    /// preset). The worker owns the clock and drives `search_bounded` with it.
    pub budget_ms: f64,
    /// Opaque id echoed back in the response so the main thread can match a
    /// reply to the request it sent and discard stale ones.
    pub request_id: u64,
}

impl WorkerRequest {
    /// Construct a request for the given record and think-time budget.
    pub fn new(moves: Vec<Move>, budget_ms: f64, request_id: u64) -> Self {
        Self {
            moves,
            budget_ms,
            request_id,
        }
    }
}

/// Search statistics surfaced to the UI (thinking indicator, debugging). Mirrors
/// the fields of `hive_engine::search::SearchStats`; kept as its own type so the
/// wire contract does not depend on that struct deriving serde.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSearchStats {
    pub nodes: u64,
    pub tt_hits: u64,
    pub tt_cutoffs: u64,
    pub beta_cutoffs: u64,
    pub tt_stores: u64,
    /// Last *completed* iterative-deepening iteration. `0` means no iteration
    /// ran: either the position was terminal, or it had exactly one legal move
    /// and the engine short-circuited. The score is meaningless in that case.
    pub depth: u8,
}

impl From<SearchStats> for WorkerSearchStats {
    fn from(s: SearchStats) -> Self {
        Self {
            nodes: s.nodes,
            tt_hits: s.tt_hits,
            tt_cutoffs: s.tt_cutoffs,
            beta_cutoffs: s.beta_cutoffs,
            tt_stores: s.tt_stores,
            depth: s.depth,
        }
    }
}

/// Worker → main thread: the search result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResponse {
    /// Echo of [`WorkerRequest::request_id`].
    pub request_id: u64,
    /// The chosen move. `None` only at a terminal/no-move position, which the
    /// main thread would not normally search.
    pub best_move: Option<Move>,
    /// Search score from the side-to-move's perspective (negamax convention).
    pub score: i32,
    /// Search statistics.
    pub stats: WorkerSearchStats,
}

/// Run the search a [`WorkerRequest`] asks for and build its [`WorkerResponse`].
///
/// This is the whole of the worker's behaviour. The worker binary
/// (`src/bin/search_worker.rs`) is only the `postMessage` plumbing around it, so
/// everything worth testing is testable here on the native target — `now_ms` is
/// the worker's clock, injected for the same reason it is injected in `ai.rs`.
pub fn handle_request(req: &WorkerRequest, now_ms: impl FnMut() -> f64) -> WorkerResponse {
    handle_request_with_progress(req, now_ms, |_depth| {})
}

/// [`handle_request`], reporting each completed search depth as it happens.
///
/// The worker posts these as [`WorkerMessage::Progress`] so the UI can show a
/// live depth while it thinks; the final [`WorkerMessage::Done`] carries the
/// authoritative depth.
pub fn handle_request_with_progress(
    req: &WorkerRequest,
    now_ms: impl FnMut() -> f64,
    on_progress: impl FnMut(u8),
) -> WorkerResponse {
    let result = search_with_progress(&req.moves, req.budget_ms, now_ms, on_progress);
    WorkerResponse {
        request_id: req.request_id,
        best_move: result.best,
        score: result.score,
        stats: result.stats.into(),
    }
}

/// Worker → main thread. Several `Progress` messages may precede one `Done`.
///
/// Tagged rather than two bare structs so the main thread can decode without
/// guessing, and so adding a third kind later does not break the wire format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkerMessage {
    /// An iteration finished; the search continues.
    Progress {
        /// Echoes the request, so stale progress is discarded like stale replies.
        request_id: u64,
        /// Depth just completed.
        depth: u8,
    },
    /// The search finished. No further messages for this request.
    Done(WorkerResponse),
}
