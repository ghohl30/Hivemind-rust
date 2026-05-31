//! Web Worker AI protocol — message contract only.
//!
//! The future search worker (PR 6) runs `hive_engine::search::search` off the UI
//! thread. The main thread posts a [`WorkerRequest`] (the game's move list plus
//! the search depth); the worker replies with a [`WorkerResponse`] carrying the
//! chosen move and some lightweight stats.
//!
//! These are plain serde structs so they round-trip through `serde_json` and are
//! native-testable. No `web-sys` / wasm dependency lives here — the worker glue
//! that actually `postMessage`s these lands in PR 6.
//!
//! Why the move list rather than a serialized `State`: the move list is the only
//! portable representation of a game today (the engine's `serde` feature covers
//! the leaf types and `Move` but not `State`). The worker reconstructs the
//! position by replaying the moves, exactly as the main thread does. See
//! `ui/engine-requests.md` #2.

use hive_engine::Move;

use serde::{Deserialize, Serialize};

/// Main thread → worker: "search this position to this depth."
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRequest {
    /// Authoritative game record. The worker replays it from `State::new()` to
    /// reconstruct the position to search.
    pub moves: Vec<Move>,
    /// Negamax search depth (from the difficulty preset).
    pub depth: u8,
    /// Opaque id echoed back in the response so the main thread can match a
    /// reply to the request it sent and discard stale ones.
    pub request_id: u64,
}

impl WorkerRequest {
    /// Construct a request for the given record and depth.
    pub fn new(moves: Vec<Move>, depth: u8, request_id: u64) -> Self {
        Self {
            moves,
            depth,
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
