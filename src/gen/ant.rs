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

use smallvec::SmallVec;
use std::collections::HashSet;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::can_slide_with_lifted;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    let mut reachable: HashSet<Coord> = HashSet::new();
    let mut frontier: Vec<Coord> = vec![from];
    let mut visited: HashSet<Coord> = HashSet::new();
    visited.insert(from);
    while let Some(current) = frontier.pop() {
        for next in current.neighbours() {
            if visited.contains(&next) {
                continue;
            }
            if board.is_occupied(next) && next != from {
                continue;
            }
            if !can_slide_with_lifted(board, current, next, from) {
                continue;
            }
            visited.insert(next);
            reachable.insert(next);
            frontier.push(next);
        }
    }
    for d in reachable {
        if d == from {
            continue;
        }
        out.push(Move::Slide { piece: pid, to: d });
    }
}
