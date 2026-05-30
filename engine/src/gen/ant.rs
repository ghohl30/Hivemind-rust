//! Soldier Ant: any number of perimeter slides, no revisits.
//!
//! Phase 5: removed the explicit `touches_hive` post-filter. For any
//! `current → next` step the ant takes, `can_slide_with_lifted` requires
//! exactly one of the two shared-neighbour cells (excluding `lifted_from`)
//! to be occupied. That occupied flank is itself a neighbour of `next` and
//! is `!= lifted_from`, so reaching `next` via slide-gap already implies
//! `next` touches the hive (excluding `lifted_from`). The previous
//! `touches_hive` check was redundant — proven by the proptest still
//! passing and by perft node counts being identical.
//!
//! Phase 5 (perf branch): `visited` is now a sorted SmallVec with
//! binary-search contains/insert, and `reachable` is a push-only SmallVec
//! deduplicated by the same visited check. No heap, no SipHash. Called once
//! per ant per move-gen call, so previously allocated 2 HashSets + 1 Vec on
//! every node touched by perft — a big chunk of the total allocations.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::can_slide_with_lifted;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    // Worst case the ant reaches every perimeter cell of a 22-piece hive
    // (≤ ~50 cells). 64 inline slots covers it.
    let mut reachable: SmallVec<[Coord; 64]> = SmallVec::new();
    let mut frontier: SmallVec<[Coord; 64]> = SmallVec::new();
    // `visited` kept sorted for binary-search membership tests.
    let mut visited: SmallVec<[Coord; 64]> = SmallVec::new();
    visited.push(from);
    frontier.push(from);
    while let Some(current) = frontier.pop() {
        for next in current.neighbours() {
            if visited.binary_search(&next).is_ok() {
                continue;
            }
            if board.is_occupied(next) && next != from {
                continue;
            }
            if !can_slide_with_lifted(board, current, next, from) {
                continue;
            }
            // Sorted insert into visited.
            let pos = visited.partition_point(|c| *c < next);
            visited.insert(pos, next);
            reachable.push(next);
            frontier.push(next);
        }
    }
    // `from` is in visited from the start, never enters reachable.
    for d in reachable {
        out.push(Move::Slide { piece: pid, to: d });
    }
}
