//! Grasshopper: jumps in a straight line over ≥1 contiguous occupied hexes,
//! landing on the first empty hex beyond them. Not subject to slide/gap rules.

use smallvec::SmallVec;

use crate::coord::{Coord, Direction};
use crate::moves::Move;
use crate::piece::PieceId;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    for d in Direction::ALL.iter() {
        let mut cur = from.neighbour(*d);
        if !board.is_occupied(cur) {
            // Must jump over at least one piece — bail if the immediate neighbour is empty.
            continue;
        }
        // Walk in direction d through occupied cells until we hit an empty one.
        while board.is_occupied(cur) {
            cur = cur.neighbour(*d);
        }
        out.push(Move::Slide { piece: pid, to: cur });
    }
}
