//! Hive game engine — base game only.
//!
//! Phase 1: copy-make, on-demand cache recomputation, full rules + tests.
//! See `/Users/gregor30/.claude/plans/hive-engine-project-eventual-charm.md`.

pub mod board;
pub mod coord;
pub mod gen;
pub mod moves;
pub mod perft;
pub mod piece;
pub mod rules;
pub mod search;
pub mod state;
pub mod zobrist;

pub use board::Board;
pub use coord::{Coord, Direction};
pub use moves::Move;
pub use piece::{Color, PieceId, PieceSlot, PieceType, StackTop};
pub use state::{Outcome, State};
