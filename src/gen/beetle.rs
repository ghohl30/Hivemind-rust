//! Beetle: one hex, slides on ground OR climbs/descends.
//!
//! Ground beetles use the ground-slide rule (gap + contact). When climbing,
//! descending, or moving across stacks, the height-aware beetle gate applies.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::PieceId;
use crate::rules::{beetle_gate_allows, can_slide_ground};
use crate::state::State;

pub fn generate(
    state: &State,
    pid: PieceId,
    from: Coord,
    h_from: u8,
    out: &mut SmallVec<[Move; 64]>,
) {
    let board = state.board();
    let h_from_total = h_from + 1; // tiles at `from` including the beetle

    for to in from.neighbours() {
        let h_to_existing = match board.top_at(to) {
            Some(top) => top.height + 1, // total tiles at `to`
            None => 0,
        };
        let climbing_or_stacked = h_from > 0 || h_to_existing > 0;
        let legal = if climbing_or_stacked {
            beetle_gate_allows(board, from, to, h_from_total, h_to_existing)
        } else {
            // Ground beetle moving to empty cell — same rule as a queen slide.
            can_slide_ground(board, from, to)
        };
        if !legal {
            continue;
        }
        out.push(Move::Slide { piece: pid, to });
    }
}
