//! Synchronous AI move computation for the in-browser engine search.
//!
//! Called from a `spawn_local` task in the render layer after the human moves.
//! Not `Send` — runs on the single WASM thread; the caller yields to the
//! browser event loop before calling so the "Thinking…" indicator renders first.
//!
//! Strength is a wall-clock budget, not a ply count. The engine's
//! `search_bounded` is deliberately clock-free (`std::time::Instant` compiles on
//! wasm32 but panics at runtime), so the clock is ours to supply. That split is
//! mirrored here: [`compute_ai_move_with_clock`] takes the clock as a parameter
//! and is testable natively, and [`compute_ai_move`] is the thin browser wrapper
//! that drives it from `performance.now()`.

use hive_engine::{search_bounded, Move, SearchStats, TranspositionTable};

use crate::game::config::MAX_DEPTH;
use crate::game::session::replay;

/// Transposition table size, as a power of two entries (2^18 ≈ 2 MB).
const TT_LOG2: u32 = 18;

/// Depth used if the browser exposes no `performance` clock. Equivalent to the
/// old fixed-depth `Medium` preset: a search that is certainly bounded without
/// needing a clock at all, which is the safe way to degrade.
const CLOCKLESS_FALLBACK_DEPTH: u8 = 4;

/// Reconstruct the game position from `moves`, search it under a `budget_ms`
/// wall-clock budget measured by `now_ms`, and return the best move (or `None`
/// at a terminal position).
///
/// `now_ms` must be monotonically non-decreasing; only differences are used, so
/// its origin is irrelevant. It is polled every 1024 nodes by the engine, not
/// once per node, so a mildly expensive clock is fine.
///
/// The engine guarantees a `Some` result for any non-terminal position even if
/// the budget is already spent on entry — depth 1 always completes — and never
/// returns a move from a partially searched iteration.
pub fn compute_ai_move_with_clock(
    moves: &[Move],
    budget_ms: f64,
    mut now_ms: impl FnMut() -> f64,
) -> Option<Move> {
    let mut state = replay(moves);
    let mut tt = TranspositionTable::with_capacity_log2(TT_LOG2);

    let start = now_ms();
    let mut should_stop = |_stats: &SearchStats| now_ms() - start >= budget_ms;

    let (_score, best, _stats) =
        search_bounded(&mut state, MAX_DEPTH, &mut tt, &mut should_stop);
    best
}

/// Browser entry point: [`compute_ai_move_with_clock`] driven by
/// `performance.now()`.
///
/// Falls back to a fixed-depth search if the `performance` API is unavailable,
/// rather than panicking or searching unbounded.
pub fn compute_ai_move(moves: &[Move], budget_ms: f64) -> Option<Move> {
    match web_sys::window().and_then(|w| w.performance()) {
        Some(perf) => compute_ai_move_with_clock(moves, budget_ms, move || perf.now()),
        None => {
            let mut state = replay(moves);
            let mut tt = TranspositionTable::with_capacity_log2(TT_LOG2);
            let (_score, best, _stats) = search_bounded(
                &mut state,
                CLOCKLESS_FALLBACK_DEPTH,
                &mut tt,
                &mut |_stats: &SearchStats| false,
            );
            best
        }
    }
}
