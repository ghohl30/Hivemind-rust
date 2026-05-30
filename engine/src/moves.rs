//! Engine moves. Single enum, three variants.
//!
//! `Slide` covers slide, hop, and beetle climb uniformly — the per-piece legality
//! is enforced in `gen/`, but application is uniform in `state::apply`.

use std::fmt;

use crate::coord::Coord;
use crate::piece::PieceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Move {
    Place { piece: PieceId, to: Coord },
    Slide { piece: PieceId, to: Coord },
    Pass,
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Move::Place { piece, to } => write!(f, "Place(p{} @ {},{})", piece.0, to.q, to.r),
            Move::Slide { piece, to } => write!(f, "Slide(p{} -> {},{})", piece.0, to.q, to.r),
            Move::Pass => write!(f, "Pass"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_is_copy_and_small() {
        // 8 bytes upper bound: discriminant + PieceId(u8) + Coord(i16, i16) + padding.
        assert!(std::mem::size_of::<Move>() <= 8);
    }

    #[test]
    fn display_renders_each_variant() {
        let p = Move::Place {
            piece: PieceId(3),
            to: Coord::new(1, -1),
        };
        let s = Move::Slide {
            piece: PieceId(0),
            to: Coord::new(0, 0),
        };
        assert_eq!(format!("{p}"), "Place(p3 @ 1,-1)");
        assert_eq!(format!("{s}"), "Slide(p0 -> 0,0)");
        assert_eq!(format!("{}", Move::Pass), "Pass");
    }
}
