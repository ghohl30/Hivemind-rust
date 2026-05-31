//! Engine-facing game core for the Hive UI.
//!
//! Pure Rust, no DOM / Leptos / wasm dependencies — fully testable on the native
//! target (`cargo test -p hive-ui`). The render and interaction layers (later
//! PRs) build on these types; this PR introduces no rendering.
//!
//! Pieces:
//!   - [`session`]  — `Session`: authoritative `Vec<Move>` + derived `State`.
//!   - [`index`]    — `LegalMoveIndex`: legal moves grouped by mover/destination,
//!                    plus forced-pass detection, for the interaction layer.
//!   - [`config`]   — new-game configuration (human color, AI difficulty/depth).
//!   - [`worker`]   — serde message contract for the future search Web Worker.

pub mod config;
pub mod index;
pub mod session;
pub mod worker;

pub use config::{Difficulty, GameSetup, HumanColor, NewGameConfig};
pub use index::LegalMoveIndex;
pub use session::{replay, IllegalMove, Session};
pub use worker::{WorkerRequest, WorkerResponse, WorkerSearchStats};

#[cfg(test)]
mod tests;
