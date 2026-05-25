//! Recursive move-count enumeration ("perft") with an optional Zobrist-keyed
//! transposition table. Provides the simplest concrete search that exercises
//! the Phase 2 TT.
//!
//! Phase 2 is still copy-make — each child state is cloned. Phase 3 will swap
//! this for `apply`/`unapply` on `&mut State`.

use std::collections::HashMap;

use crate::state::State;

/// Statistics returned by `perft_with_tt`. `count` is the number of leaves
/// reached (the standard perft answer). `tt_hits` counts how many recursive
/// calls were resolved from the table instead of expanding children — a
/// non-zero value demonstrates the Zobrist hash is identifying transpositions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PerftStats {
    pub count: u64,
    pub tt_hits: u64,
    pub tt_stores: u64,
}

/// Plain perft, no TT — useful as ground truth for asserting that the
/// TT variant computes the same count.
pub fn perft(state: &State, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut total = 0u64;
    for m in state.legal_moves() {
        let mut next = state.clone();
        next.apply(m);
        total += perft(&next, depth - 1);
    }
    total
}

/// Make-unmake perft on `&mut State`. The Phase 3 "simple search runs on
/// &mut State" deliverable. Should produce the same count as `perft`.
pub fn perft_unmake(state: &mut State, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    // `legal_moves` borrows `&self`; returns by value, so the borrow drops
    // before we take `&mut` for apply/unapply.
    let moves = state.legal_moves();
    let mut total = 0u64;
    for m in moves {
        state.apply(m);
        total += perft_unmake(state, depth - 1);
        state.unapply();
    }
    total
}

/// Perft using a `(zobrist, depth)` keyed transposition table. Two positions
/// that share a Zobrist hash at the same depth are considered equivalent;
/// the table caches their subtree count.
pub fn perft_with_tt(state: &State, depth: u32) -> PerftStats {
    let mut tt: HashMap<(u64, u32), u64> = HashMap::new();
    let mut stats = PerftStats::default();
    stats.count = inner(state, depth, &mut tt, &mut stats);
    stats
}

fn inner(state: &State, depth: u32, tt: &mut HashMap<(u64, u32), u64>, stats: &mut PerftStats) -> u64 {
    if depth == 0 {
        return 1;
    }
    let key = (state.zobrist(), depth);
    if let Some(c) = tt.get(&key) {
        stats.tt_hits += 1;
        return *c;
    }
    let mut total = 0u64;
    for m in state.legal_moves() {
        let mut next = state.clone();
        next.apply(m);
        total += inner(&next, depth - 1, tt, stats);
    }
    tt.insert(key, total);
    stats.tt_stores += 1;
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perft_initial_depth_1_is_11() {
        // White can place any of 11 pieces at ORIGIN on turn 1.
        let s = State::new();
        assert_eq!(perft(&s, 1), 11);
    }

    #[test]
    fn perft_initial_depth_2_is_726() {
        // 11 white openings × 11 black piece choices × 6 adjacency placements.
        let s = State::new();
        assert_eq!(perft(&s, 2), 11 * 11 * 6);
    }

    #[test]
    fn tt_perft_matches_plain_perft_at_low_depth() {
        let s = State::new();
        for depth in 0..=2 {
            let plain = perft(&s, depth);
            let withtt = perft_with_tt(&s, depth);
            assert_eq!(
                withtt.count, plain,
                "TT perft diverged from plain perft at depth {depth}"
            );
        }
    }

    #[test]
    fn perft_unmake_matches_plain_perft() {
        let s = State::new();
        for depth in 0..=2 {
            let plain = perft(&s, depth);
            let mut m = s.clone();
            let unmake = perft_unmake(&mut m, depth);
            assert_eq!(unmake, plain, "perft_unmake diverged at depth {depth}");
            assert_eq!(
                m.undo_depth(),
                0,
                "perft_unmake left dangling undo records at depth {depth}",
            );
        }
    }

    #[test]
    fn perft_unmake_leaves_state_unchanged() {
        let s = State::new();
        let mut m = s.clone();
        perft_unmake(&mut m, 2);
        assert_eq!(m, s, "perft_unmake mutated the starting state");
    }

    #[test]
    fn tt_perft_records_transposition_hits_at_depth_2() {
        // The two physically interchangeable beetles (and the two spiders, the
        // three grasshoppers, the three ants) produce identical Zobrist hashes
        // when placed at the same coord — the hash key is on PieceType, not
        // PieceId. So any depth-2 search of the opening must record at least
        // one TT hit.
        let s = State::new();
        let stats = perft_with_tt(&s, 2);
        assert!(
            stats.tt_hits > 0,
            "expected TT hits at depth 2, got 0 (count={}, stores={})",
            stats.count,
            stats.tt_stores
        );
    }
}
