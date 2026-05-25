//! Pieces, colors, and per-piece slot state.
//!
//! PieceId layout (canonical):
//!   0..11  = white in order [Q, B, B, G, G, G, S, S, A, A, A]
//!   11..22 = black in same order.

use crate::coord::Coord;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    pub const fn other(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum PieceType {
    QueenBee = 0,
    Beetle = 1,
    Grasshopper = 2,
    Spider = 3,
    SoldierAnt = 4,
    // Reserved for expansions; never produced in Phase 1.
    Mosquito = 5,
    Ladybug = 6,
    Pillbug = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PieceId(pub u8);

impl PieceId {
    pub const COUNT: usize = 22;
    pub const PER_SIDE: usize = 11;

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn color(self) -> Color {
        if (self.0 as usize) < Self::PER_SIDE {
            Color::White
        } else {
            Color::Black
        }
    }

    pub const fn piece_type(self) -> PieceType {
        CANONICAL_TYPES[(self.0 as usize) % Self::PER_SIDE]
    }

    /// All 22 piece IDs.
    pub fn all() -> impl Iterator<Item = PieceId> {
        (0..Self::COUNT as u8).map(PieceId)
    }

    /// All piece IDs for one color.
    pub fn for_color(c: Color) -> impl Iterator<Item = PieceId> {
        let start = match c {
            Color::White => 0u8,
            Color::Black => Self::PER_SIDE as u8,
        };
        let end = start + Self::PER_SIDE as u8;
        (start..end).map(PieceId)
    }
}

/// Order within one side's eleven pieces.
const CANONICAL_TYPES: [PieceType; PieceId::PER_SIDE] = [
    PieceType::QueenBee,
    PieceType::Beetle,
    PieceType::Beetle,
    PieceType::Grasshopper,
    PieceType::Grasshopper,
    PieceType::Grasshopper,
    PieceType::Spider,
    PieceType::Spider,
    PieceType::SoldierAnt,
    PieceType::SoldierAnt,
    PieceType::SoldierAnt,
];

/// White queen's PieceId.
pub const WHITE_QUEEN: PieceId = PieceId(0);
/// Black queen's PieceId.
pub const BLACK_QUEEN: PieceId = PieceId(PieceId::PER_SIDE as u8);

pub const fn queen_of(c: Color) -> PieceId {
    match c {
        Color::White => WHITE_QUEEN,
        Color::Black => BLACK_QUEEN,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PieceSlot {
    InHand,
    OnBoard { coord: Coord, stack_height: u8 },
    Covered { coord: Coord, stack_height: u8 },
}

impl PieceSlot {
    pub const fn coord(self) -> Option<Coord> {
        match self {
            PieceSlot::InHand => None,
            PieceSlot::OnBoard { coord, .. } | PieceSlot::Covered { coord, .. } => Some(coord),
        }
    }

    pub const fn is_on_board_top(self) -> bool {
        matches!(self, PieceSlot::OnBoard { .. })
    }

    pub const fn is_in_hand(self) -> bool {
        matches!(self, PieceSlot::InHand)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StackTop {
    pub piece: PieceId,
    pub height: u8,
}

// Compile-time invariants.
const _: () = {
    assert!(std::mem::size_of::<PieceSlot>() <= 8);
    assert!(PieceId::PER_SIDE * 2 == PieceId::COUNT);
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn counts_per_color_and_type() {
        for color in [Color::White, Color::Black] {
            let mut counts: HashMap<PieceType, usize> = HashMap::new();
            for pid in PieceId::for_color(color) {
                assert_eq!(pid.color(), color);
                *counts.entry(pid.piece_type()).or_insert(0) += 1;
            }
            assert_eq!(counts.get(&PieceType::QueenBee), Some(&1));
            assert_eq!(counts.get(&PieceType::Beetle), Some(&2));
            assert_eq!(counts.get(&PieceType::Grasshopper), Some(&3));
            assert_eq!(counts.get(&PieceType::Spider), Some(&2));
            assert_eq!(counts.get(&PieceType::SoldierAnt), Some(&3));
        }
    }

    #[test]
    fn total_count_is_22() {
        let all: Vec<_> = PieceId::all().collect();
        assert_eq!(all.len(), 22);
    }

    #[test]
    fn queens_are_at_expected_indices() {
        assert_eq!(WHITE_QUEEN, PieceId(0));
        assert_eq!(BLACK_QUEEN, PieceId(11));
        assert_eq!(queen_of(Color::White), WHITE_QUEEN);
        assert_eq!(queen_of(Color::Black), BLACK_QUEEN);
    }

    #[test]
    fn color_other_is_involutive() {
        assert_eq!(Color::White.other().other(), Color::White);
        assert_eq!(Color::Black.other().other(), Color::Black);
        assert_eq!(Color::White.other(), Color::Black);
    }
}
