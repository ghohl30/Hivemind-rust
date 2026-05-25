//! Move generation. Each piece type has its own submodule.
//!
//! Top-level entry points are called from `state::legal_moves`.

use smallvec::SmallVec;
use std::collections::HashSet;

use crate::coord::Coord;
use crate::moves::Move;
use crate::piece::{queen_of, Color, PieceId, PieceSlot, PieceType};
use crate::rules::articulation_points;
use crate::state::State;

pub mod ant;
pub mod beetle;
pub mod grasshopper;
pub mod queen;
pub mod spider;

/// Empty cells adjacent to the hive. For the initial empty board this is
/// `{ORIGIN}` only when handled at the call site.
pub fn perimeter_coords(state: &State) -> HashSet<Coord> {
    let board = state.board();
    let mut out: HashSet<Coord> = HashSet::new();
    for c in board.occupied_coords() {
        for n in c.neighbours() {
            if !board.is_occupied(n) {
                out.insert(n);
            }
        }
    }
    out
}

/// Candidate placement coords for `color`. After the opening's two special
/// cases, we just read `state.placement_legality(color)` — the Phase 4 cache
/// that holds exactly the "touches own, no enemy" cells.
fn placement_candidates(state: &State, color: Color) -> Vec<Coord> {
    let placements = state.placements_so_far();
    if placements == 0 {
        return vec![Coord::ORIGIN];
    }
    if placements == 1 {
        // Adjacent to the unique existing piece (which is at ORIGIN by the
        // opening rule, but we don't assume — read it off the board).
        let existing = state
            .board()
            .occupied_coords()
            .next()
            .expect("placements_so_far==1 implies at least one occupied coord");
        return existing.neighbours().to_vec();
    }
    state.placement_legality(color).iter().copied().collect()
}

/// Distinct in-hand pieces of `color`, deduplicated by piece type — placing
/// either of the two beetles at coord X is the same outcome modulo PieceId,
/// but we keep each as a distinct Move so the action space stays stable for a
/// future RL layer (per project decision). For Phase 1 correctness, we just
/// emit one Place per (PieceId, coord).
fn in_hand_pieces(state: &State, color: Color) -> Vec<PieceId> {
    PieceId::for_color(color)
        .filter(|pid| matches!(state.piece_slot(*pid), PieceSlot::InHand))
        .collect()
}

pub fn generate_placements(state: &State, color: Color, out: &mut SmallVec<[Move; 64]>) {
    let candidates = placement_candidates(state, color);
    if candidates.is_empty() {
        return;
    }
    for pid in in_hand_pieces(state, color) {
        for c in candidates.iter() {
            out.push(Move::Place { piece: pid, to: *c });
        }
    }
}

pub fn generate_queen_placement(state: &State, color: Color, out: &mut SmallVec<[Move; 64]>) {
    let queen = queen_of(color);
    if !matches!(state.piece_slot(queen), PieceSlot::InHand) {
        return;
    }
    for c in placement_candidates(state, color) {
        out.push(Move::Place { piece: queen, to: c });
    }
}

pub fn generate_movements(state: &State, color: Color, out: &mut SmallVec<[Move; 64]>) {
    // Phase 5: one Tarjan pass classifies every piece's pinned-by-one-hive
    // status, replacing the previous N independent connectivity checks.
    let articulations = articulation_points(state.board());
    for pid in PieceId::for_color(color) {
        match state.piece_slot(pid) {
            PieceSlot::OnBoard { coord, stack_height } => {
                // Beetles on top of a stack (stack_height > 0) are always
                // movable regardless of articulation — lifting them doesn't
                // change which coords are occupied.
                if stack_height == 0 && articulations.binary_search(&coord).is_ok() {
                    continue;
                }
                let from = coord;
                let h_from = stack_height;
                match pid.piece_type() {
                    PieceType::QueenBee => queen::generate(state, pid, from, out),
                    PieceType::Beetle => beetle::generate(state, pid, from, h_from, out),
                    PieceType::Grasshopper => grasshopper::generate(state, pid, from, out),
                    PieceType::Spider => spider::generate(state, pid, from, out),
                    PieceType::SoldierAnt => ant::generate(state, pid, from, out),
                    PieceType::Mosquito | PieceType::Ladybug | PieceType::Pillbug => {
                        // Reserved for expansions; never produced in Phase 1.
                    }
                }
            }
            PieceSlot::InHand | PieceSlot::Covered { .. } => continue,
        }
    }
}
