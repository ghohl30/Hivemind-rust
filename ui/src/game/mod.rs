//! Engine-facing game core for the Hive UI.
//!
//! No DOM or Leptos dependencies — testable on the native target
//! (`cargo test -p hive-ui`). The one browser touch-point is
//! [`ai::compute_ai_move`], which reads `performance.now()`; its logic lives in
//! the clock-free [`ai::compute_ai_move_with_clock`], which is what the tests
//! drive.
//!
//! Pieces:
//!   - [`session`]  — `Session`: authoritative `Vec<Move>` + derived `State`.
//!   - [`index`]    — `LegalMoveIndex`: legal moves grouped by mover/destination,
//!                    plus forced-pass detection, for the interaction layer.
//!   - [`config`]   — new-game configuration (human color, AI think-time budget).
//!   - [`ai`]       — time-bounded engine search behind the difficulty presets.
//!   - [`analysis`] — decoding a raw search score into the one thing the
//!                    engine can state without qualification: a forced win.
//!   - [`worker`]   — serde message contract for the future search Web Worker.

pub mod ai;
pub mod analysis;
pub mod config;
pub mod index;
pub mod session;
pub mod worker;

pub use ai::{
    compute_ai_move, compute_ai_move_with_clock, search_with_clock, search_with_progress, AiSearch,
};
pub use analysis::{Assessment, Forced, ANALYSIS_BUDGET_MS};
pub use config::{Difficulty, GameSetup, HumanColor, NewGameConfig, MAX_DEPTH};
pub use index::LegalMoveIndex;
pub use session::{replay, IllegalMove, Session};
pub use worker::{
    handle_request, handle_request_with_progress, WorkerMessage, WorkerRequest, WorkerResponse,
    WorkerSearchStats,
};

#[cfg(test)]
mod tests;
