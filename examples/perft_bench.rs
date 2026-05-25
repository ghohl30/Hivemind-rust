//! Tiny perft benchmark. Run with:
//!
//!     cargo run --release --example perft_bench [max_depth]
//!
//! Default max_depth = 3. Each depth prints (nodes, elapsed, nodes/sec).

use std::env;
use std::time::Instant;

use hive_engine::perft::perft_unmake;
use hive_engine::State;

fn main() {
    let max_depth: u32 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);

    println!("perft_unmake from initial state");
    println!("{:>5} {:>14} {:>12} {:>14}", "depth", "nodes", "elapsed", "nodes/sec");
    println!("{:->50}", "");

    for depth in 1..=max_depth {
        let mut s = State::new();
        let start = Instant::now();
        let nodes = perft_unmake(&mut s, depth);
        let elapsed = start.elapsed();
        let nps = if elapsed.as_secs_f64() > 0.0 {
            (nodes as f64 / elapsed.as_secs_f64()) as u64
        } else {
            0
        };
        println!(
            "{:>5} {:>14} {:>12} {:>14}",
            depth,
            nodes,
            format!("{:.3?}", elapsed),
            nps
        );
    }
}
