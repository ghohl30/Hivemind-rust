//! Spider: exactly three perimeter slides, no revisits, no backtracking.
//!
//! Phase 5: removed the explicit `touches_hive` post-filter — same argument
//! as in `gen/ant.rs`: slide-gap already implies hive contact at the new
//! cell (the occupied flank from the XOR check is itself a neighbour of
//! `next` and not the lifted piece). Proptest + perft counts unchanged.
//!
//! Phase 5 (perf branch): `visited` is the DFS path stack (push on descend,
//! pop on return), and `destinations` is push-only with a final sort+dedup.
//! Max recursion depth is 3, so `visited` holds at most 4 entries — linear
//! `contains` is faster than binary search at that size.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::can_slide_with_lifted;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    let mut destinations: SmallVec<[Coord; 16]> = SmallVec::new();
    let mut visited: SmallVec<[Coord; 8]> = SmallVec::new();
    visited.push(from);
    dfs(board, from, from, 0, 3, &mut visited, &mut destinations);
    destinations.sort();
    destinations.dedup();
    for d in destinations {
        out.push(Move::Slide { piece: pid, to: d });
    }
}

fn dfs(
    board: &crate::board::Board,
    lifted_from: Coord,
    current: Coord,
    depth: u8,
    target: u8,
    visited: &mut SmallVec<[Coord; 8]>,
    out: &mut SmallVec<[Coord; 16]>,
) {
    if depth == target {
        out.push(current);
        return;
    }
    for next in current.neighbours() {
        if visited.contains(&next) {
            continue;
        }
        if board.is_occupied(next) && next != lifted_from {
            continue;
        }
        if !can_slide_with_lifted(board, current, next, lifted_from) {
            continue;
        }
        visited.push(next);
        dfs(board, lifted_from, next, depth + 1, target, visited, out);
        visited.pop();
    }
}
