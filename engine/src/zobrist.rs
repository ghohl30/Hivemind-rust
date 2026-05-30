//! Zobrist hashing keyed on (color, piece_type, coord, stack_height).
//!
//! The brief's specification: "a beetle on a queen hashes differently than a
//! beetle on the ground" — i.e., the *physical position* (coord + height in
//! the stack) is what matters, not the PieceId. Two beetles of the same color
//! sitting on the same square at the same height would hash identically. They
//! cannot, because at most one piece sits at any (coord, height); but it does
//! mean the hash is invariant under permutation of physically interchangeable
//! pieces, which is what we want for transposition detection.
//!
//! Implementation: a deterministic mixing function over the packed tuple,
//! rather than a precomputed table. No allocation, no global state, no
//! initialization. The function is pure and compile-time constant.
//!
//! Per the project brief, the hash is keyed on *board state + side to move
//! only*. Turn counters and `placements_so_far` are intentionally NOT in the
//! hash — collisions on those fields are accepted in exchange for higher
//! transposition rates.

use crate::coord::Coord;
use crate::piece::{Color, PieceId, PieceSlot, PieceType};
use crate::state::State;

/// XOR'd into the hash whenever it is Black's turn. White's turn contributes
/// nothing, so an empty board with White to move has hash 0 — a useful
/// invariant for `State::new()`.
pub const SIDE_TO_MOVE_KEY: u64 = 0xA5A5_A5A5_A5A5_A5A5;

/// Hash key for "this piece sits at this coord at this height in the stack".
pub const fn piece_key(color: Color, ptype: PieceType, coord: Coord, height: u8) -> u64 {
    // Pack the four inputs into a u64 and run a SplitMix64 mixer.
    let packed: u64 = (color as u64)
        | ((ptype as u64) << 4)
        | (((coord.q as u16) as u64) << 8)
        | (((coord.r as u16) as u64) << 24)
        | ((height as u64) << 40)
        // Bias so packed == 0 doesn't trivially hash to 0.
        | (0xC3 << 48);
    splitmix64(packed)
}

const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Compute the Zobrist hash of a state from scratch by XOR-summing the keys
/// of every on-board tile (including covered ones — they still contribute at
/// their original height) plus the side-to-move bit.
///
/// `State::zobrist()` maintains this incrementally; the proptest invariant
/// `state.zobrist() == zobrist::from_scratch(&state)` is the safety net for
/// future cache-related refactors.
pub fn from_scratch(state: &State) -> u64 {
    let mut h: u64 = 0;
    for pid in PieceId::all() {
        match state.piece_slot(pid) {
            PieceSlot::OnBoard { coord, stack_height }
            | PieceSlot::Covered { coord, stack_height } => {
                h ^= piece_key(pid.color(), pid.piece_type(), coord, stack_height);
            }
            PieceSlot::InHand => {}
        }
    }
    if matches!(state.side_to_move(), Color::Black) {
        h ^= SIDE_TO_MOVE_KEY;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_differ_on_each_input_axis() {
        let k0 = piece_key(Color::White, PieceType::Beetle, Coord::ORIGIN, 0);
        let k_color = piece_key(Color::Black, PieceType::Beetle, Coord::ORIGIN, 0);
        let k_type = piece_key(Color::White, PieceType::QueenBee, Coord::ORIGIN, 0);
        let k_coord = piece_key(Color::White, PieceType::Beetle, Coord::new(1, 0), 0);
        let k_height = piece_key(Color::White, PieceType::Beetle, Coord::ORIGIN, 1);
        assert_ne!(k0, k_color);
        assert_ne!(k0, k_type);
        assert_ne!(k0, k_coord);
        assert_ne!(k0, k_height);
    }

    #[test]
    fn beetle_on_queen_distinct_from_beetle_on_ground() {
        let on_ground = piece_key(Color::White, PieceType::Beetle, Coord::new(2, -1), 0);
        let on_queen = piece_key(Color::White, PieceType::Beetle, Coord::new(2, -1), 1);
        assert_ne!(on_ground, on_queen);
    }

    #[test]
    fn empty_initial_state_hashes_to_zero() {
        let s = State::new();
        assert_eq!(from_scratch(&s), 0);
    }

    #[test]
    fn deterministic_across_calls() {
        let k1 = piece_key(Color::Black, PieceType::SoldierAnt, Coord::new(-3, 2), 2);
        let k2 = piece_key(Color::Black, PieceType::SoldierAnt, Coord::new(-3, 2), 2);
        assert_eq!(k1, k2);
    }
}
