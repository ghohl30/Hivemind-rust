//! Synchronous AI move computation for the in-browser engine search.
//!
//! Called from a `spawn_local` task in the render layer after the human moves.
//! Not `Send` — runs on the single WASM thread; the caller yields to the
//! browser event loop before calling so the "Thinking…" indicator renders first.

use hive_engine::{search, Move, TranspositionTable};

use crate::game::session::replay;

/// Reconstruct the game position from `moves`, run alpha-beta search to
/// `depth`, and return the best move (or `None` at a terminal position).
///
/// A fresh `TranspositionTable` is allocated per call (capacity 2^18 ≈ 2 MB).
pub fn compute_ai_move(moves: &[Move], depth: u8) -> Option<Move> {
    let mut state = replay(moves);
    let mut tt = TranspositionTable::with_capacity_log2(18);
    let (_score, best, _stats) = search(&mut state, depth, &mut tt);
    best
}
