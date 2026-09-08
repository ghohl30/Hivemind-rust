//! Alpha-beta + TT search benchmark.
//!
//!     cargo run --release --example search_bench [max_depth] [tt_log2]
//!
//! Defaults: max_depth=4, tt_log2=18 (1<<18 = 262144 entries).
//! Each depth prints (score, nodes, tt_hits, tt_cutoffs, beta_cutoffs, elapsed, nodes/sec).
//!
//! A second section exercises `search_timed` from a midgame position, where the
//! branching factor is high enough for the wall-clock cap to actually bind. It
//! reports the depth reached and the time actually spent against the budget —
//! the cap is only meaningful if the second stays under the first.

use std::env;
use std::time::{Duration, Instant};

use hive_engine::search::{search, search_timed, TranspositionTable};
use hive_engine::{Move, State};

/// Walk into a midgame position deterministically by always taking the first
/// legal move. Not good play — the point is a crowded board with ants out, so
/// the timing numbers reflect the hard case rather than the opening.
fn midgame(plies: usize) -> State {
    let mut s = State::new();
    for _ in 0..plies {
        if s.is_terminal().is_some() {
            break;
        }
        let moves = s.legal_moves();
        let m = moves.iter().copied().find(|m| !matches!(m, Move::Pass));
        match m {
            Some(m) => s.apply(m),
            None => break,
        }
    }
    s
}

fn main() {
    let max_depth: u8 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let tt_log2: u32 = env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(18);

    println!("alpha-beta + TT search from initial state");
    println!("TT capacity: 2^{tt_log2} = {} entries", 1u64 << tt_log2);
    println!(
        "{:>5} {:>8} {:>14} {:>10} {:>11} {:>11} {:>12} {:>14}",
        "depth", "score", "nodes", "tt_hits", "tt_cuts", "beta_cuts", "elapsed", "nodes/sec"
    );
    println!("{:->90}", "");

    for depth in 1..=max_depth {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(tt_log2);
        let start = Instant::now();
        let (score, _best, stats) = search(&mut s, depth, &mut tt);
        let elapsed = start.elapsed();
        let nps = if elapsed.as_secs_f64() > 0.0 {
            (stats.nodes as f64 / elapsed.as_secs_f64()) as u64
        } else {
            0
        };
        println!(
            "{:>5} {:>8} {:>14} {:>10} {:>11} {:>11} {:>12} {:>14}",
            depth,
            score,
            stats.nodes,
            stats.tt_hits,
            stats.tt_cutoffs,
            stats.beta_cutoffs,
            format!("{:.3?}", elapsed),
            nps
        );
    }

    // --- wall-clock bounded search -------------------------------------
    let mut mid = midgame(24);
    println!();
    println!(
        "search_timed from a {}-ply midgame position ({} legal moves)",
        mid.undo_depth(),
        mid.legal_moves().len()
    );
    println!(
        "{:>10} {:>8} {:>14} {:>12} {:>10}",
        "budget", "depth", "nodes", "elapsed", "over?"
    );
    println!("{:->60}", "");

    for ms in [100u64, 500, 2_000, 20_000] {
        let budget = Duration::from_millis(ms);
        let mut tt = TranspositionTable::with_capacity_log2(tt_log2);
        let start = Instant::now();
        let (_score, best, stats) = search_timed(&mut mid, 64, &mut tt, budget);
        let elapsed = start.elapsed();
        assert!(best.is_some(), "timed search must always return a move");
        println!(
            "{:>10} {:>8} {:>14} {:>12} {:>10}",
            format!("{ms}ms"),
            stats.depth,
            stats.nodes,
            format!("{:.3?}", elapsed),
            if elapsed > budget { "YES" } else { "no" }
        );
    }
}
