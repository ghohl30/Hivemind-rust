//! Queen Bee: exactly one perimeter slide.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::can_slide_ground;
use crate::state::State;

pub fn generate(state: &State, pid: PieceId, from: Coord, out: &mut SmallVec<[Move; 64]>) {
    let board = state.board();
    for to in from.neighbours() {
        if board.is_occupied(to) {
            continue;
        }
        if !can_slide_ground(board, from, to) {
            continue;
        }
        out.push(Move::Slide { piece: pid, to });
    }
}
