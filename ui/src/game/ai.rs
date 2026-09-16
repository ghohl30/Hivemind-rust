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

/// A completed search: everything the Web Worker needs to fill a
/// `WorkerResponse`, and more than the main-thread path reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiSearch {
    /// Best move found, or `None` at a terminal position.
    pub best: Option<Move>,
    /// Score from the side-to-move's perspective (negamax convention).
    ///
    /// Not meaningful when `stats.depth == 0` — see [`AiSearch::was_forced`].
    pub score: i32,
    /// Engine statistics, including `depth`: the last *completed* iteration.
    pub stats: SearchStats,
}

impl AiSearch {
    /// Whether the engine short-circuited because the position had exactly one
    /// legal move. It reports `depth: 0` and a placeholder `score` of 0 in that
    /// case; neither is a search result and neither should be displayed.
    pub fn was_forced(&self) -> bool {
        self.stats.depth == 0 && self.best.is_some()
    }
}

/// Reconstruct the game position from `moves`, search it under a `budget_ms`
/// wall-clock budget measured by `now_ms`, and return the full result.
///
/// `now_ms` must be monotonically non-decreasing; only differences are used, so
/// its origin is irrelevant. It is polled every 1024 nodes by the engine, not
/// once per node, so a mildly expensive clock is fine.
///
/// The engine guarantees a `Some` move for any non-terminal position even if
/// the budget is already spent on entry — depth 1 always completes — and never
/// returns a move from a partially searched iteration.
pub fn search_with_clock(
    moves: &[Move],
    budget_ms: f64,
    now_ms: impl FnMut() -> f64,
) -> AiSearch {
    search_with_progress(moves, budget_ms, now_ms, |_depth| {})
}

/// [`search_with_clock`], reporting each completed iterative-deepening
/// iteration to `on_progress` as it happens.
///
/// This is what makes a live "thinking, depth N" indicator possible: the engine
/// hands `&SearchStats` to the stop predicate precisely so a caller can watch
/// `stats.depth` change without the engine needing a progress channel.
///
/// Two limits worth knowing. The predicate is polled every 1024 nodes, so a
/// depth is reported at the first poll *after* it completes, not the instant it
/// does. And the final iteration returns without a further poll, so the deepest
/// depth usually arrives in [`AiSearch::stats`] rather than through this
/// callback — treat it as a progress hint, not a record of every depth reached.
pub fn search_with_progress(
    moves: &[Move],
    budget_ms: f64,
    mut now_ms: impl FnMut() -> f64,
    mut on_progress: impl FnMut(u8),
) -> AiSearch {
    let mut state = replay(moves);
    let mut tt = TranspositionTable::with_capacity_log2(TT_LOG2);

    let start = now_ms();
    let mut reported = 0u8;
    let mut should_stop = |stats: &SearchStats| {
        if stats.depth != reported {
            reported = stats.depth;
            on_progress(stats.depth);
        }
        now_ms() - start >= budget_ms
    };

    let (score, best, stats) =
        search_bounded(&mut state, MAX_DEPTH, &mut tt, &mut should_stop);
    AiSearch { best, score, stats }
}

/// [`search_with_clock`], keeping only the move.
pub fn compute_ai_move_with_clock(
    moves: &[Move],
    budget_ms: f64,
    now_ms: impl FnMut() -> f64,
) -> Option<Move> {
    search_with_clock(moves, budget_ms, now_ms).best
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
