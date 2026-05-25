//! Spider: exactly three perimeter slides, no revisits, no backtracking.
//!
//! Phase 5: removed the explicit `touches_hive` post-filter — same argument
//! as in `gen/ant.rs`: slide-gap already implies hive contact at the new
//! cell (the occupied flank from the XOR check is itself a neighbour of
//! `next` and not the lifted piece). Proptest + perft counts unchanged.

use smallvec::SmallVec;
use std::collections::HashSet;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::can_slide_with_lifted;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    let mut destinations: HashSet<Coord> = HashSet::new();
    let mut visited: HashSet<Coord> = HashSet::new();
    visited.insert(from);
    dfs(board, from, from, 0, 3, &mut visited, &mut destinations);
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
    visited: &mut HashSet<Coord>,
    out: &mut HashSet<Coord>,
) {
    if depth == target {
        out.insert(current);
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
        visited.insert(next);
        dfs(board, lifted_from, next, depth + 1, target, visited, out);
        visited.remove(&next);
    }
}
