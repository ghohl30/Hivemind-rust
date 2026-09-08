//! # hive-engine — stable public API
//!
//! **Everything re-exported from this file is the stable public API.** The UI
//! crate (`hive-ui`) and any other consumer must depend only on items listed
//! here. Everything else — module internals, `pub(crate)` items, types in
//! `src/gen/`, `src/rules.rs`, `src/zobrist.rs`, etc. — is an implementation
//! detail and may change between commits without notice.
//!
//! ## Stability contract
//!
//! - *Additive* changes (new methods, new re-exports, new optional feature
//!   flags) are cheap and can happen any time.
//! - *Breaking* changes (removing or renaming a re-exported item, changing a
//!   method signature, adding a required `Move` variant) require explicit
//!   coordination with the UI agent before landing.
//! - **Adding a new `Move` variant is a breaking change.** The UI pattern-
//!   matches on `Move`; an unexpected variant causes a compile error on the UI
//!   side. Announce it and let the UI agent update its match arms first.
//!
//! ## Feature flags
//!
//! - `serde` — opt-in `Serialize`/`Deserialize` on all stable value types
//!   (`Move`, `Coord`, `Direction`, `Color`, `PieceId`, `PieceType`,
//!   `PieceSlot`, `StackTop`, `Outcome`). Not in the default feature set; the
//!   UI crate opts in via
//!   `hive-engine = { path = "../engine", features = ["serde"] }`.
//!   `State` itself is intentionally not serialized — games persist as a
//!   `Vec<Move>` replayed through `State::new()` + `apply`.

// Internal modules — not part of the stable surface, subject to change.
pub mod board;
pub mod coord;
pub mod eval;
pub mod game_runner;
pub mod gen;
pub mod moves;
pub mod perft;
pub mod piece;
pub mod player;
pub mod rules;
pub mod search;
pub mod state;
pub mod zobrist;

// ── Stable re-exports ────────────────────────────────────────────────────────

// Board
pub use board::Board;

// Coordinates
pub use coord::{Coord, Direction};

// Moves
pub use moves::Move;

// Pieces and slots
pub use piece::{Color, PieceId, PieceSlot, PieceType, StackTop};

// Game state and outcome
pub use state::{Outcome, State};

// Search: entry points, result types, and the transposition table.
// `search_bounded` is the time-bounded form (UI engine-request #1): the caller
// supplies the stop predicate, so the engine stays clock-free and therefore
// safe to compile into the UI's WASM bundle, where `Instant::now()` panics.
pub use search::{
    search, search_bounded, search_bounded_with, SearchStats, TranspositionTable, MATE_SCORE,
    MATE_THRESHOLD,
};

// Evaluation, behind a trait so two variants can be compared head-to-head.
// `LegacyEval` is temporary A/B scaffolding — see its doc comment.
pub use eval::{CurrentEval, Eval, LegacyEval};

// Native-only: wall-clock wrapper around `search_bounded`. Absent on wasm32 by
// design — see its doc comment.
#[cfg(not(target_arch = "wasm32"))]
pub use search::{search_timed, search_timed_with};

// Phase 6: engine-vs-engine game runner
pub use game_runner::{play_game, play_game_from, GameResult};
pub use player::{FirstMovePlayer, Player, SearchPlayer};
#[cfg(not(target_arch = "wasm32"))]
pub use player::TimedSearchPlayer;
