//! Alpha-beta + TT search benchmark.
//!
//!     cargo run --release --example search_bench [max_depth] [tt_log2]
//!
//! Defaults: max_depth=4, tt_log2=18 (1<<18 = 262144 entries).
//! Each depth prints (score, nodes, tt_hits, tt_cutoffs, beta_cutoffs, elapsed, nodes/sec).

use std::env;
use std::time::Instant;

use hive_engine::search::{search, TranspositionTable};
use hive_engine::State;

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
}
