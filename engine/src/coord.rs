//! Axial hex coordinates and the six neighbour directions.
//!
//! This is the ONE place the convention lives:
//!   E (+1, 0), W (-1, 0), SE (0, +1), NW (0, -1), NE (+1, -1), SW (-1, +1).

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Coord {
    pub q: i16,
    pub r: i16,
}

impl Coord {
    pub const ORIGIN: Coord = Coord { q: 0, r: 0 };

    pub const fn new(q: i16, r: i16) -> Self {
        Coord { q, r }
    }

    pub const fn neighbour(self, d: Direction) -> Coord {
        let (dq, dr) = d.delta();
        Coord {
            q: self.q + dq,
            r: self.r + dr,
        }
    }

    pub fn neighbours(self) -> [Coord; 6] {
        let mut out = [Coord::ORIGIN; 6];
        let mut i = 0;
        while i < 6 {
            out[i] = self.neighbour(Direction::ALL[i]);
            i += 1;
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum Direction {
    East = 0,
    West = 1,
    SouthEast = 2,
    NorthWest = 3,
    NorthEast = 4,
    SouthWest = 5,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Direction::East,
        Direction::West,
        Direction::SouthEast,
        Direction::NorthWest,
        Direction::NorthEast,
        Direction::SouthWest,
    ];

    pub const fn delta(self) -> (i16, i16) {
        match self {
            Direction::East => (1, 0),
            Direction::West => (-1, 0),
            Direction::SouthEast => (0, 1),
            Direction::NorthWest => (0, -1),
            Direction::NorthEast => (1, -1),
            Direction::SouthWest => (-1, 1),
        }
    }

    /// Adjacency by index in `ALL`. Two directions share an adjacent direction
    /// on either side — used by the slide/gap rule.
    pub const fn rotated(self, steps: i8) -> Direction {
        // Angular order around a hex: E, NE, NW, W, SW, SE, E ...
        const ORDER: [Direction; 6] = [
            Direction::East,
            Direction::NorthEast,
            Direction::NorthWest,
            Direction::West,
            Direction::SouthWest,
            Direction::SouthEast,
        ];
        let idx = match self {
            Direction::East => 0,
            Direction::NorthEast => 1,
            Direction::NorthWest => 2,
            Direction::West => 3,
            Direction::SouthWest => 4,
            Direction::SouthEast => 5,
        };
        let raw = idx as i8 + steps;
        let normalized = ((raw % 6) + 6) % 6;
        ORDER[normalized as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbour_deltas_match_brief() {
        let o = Coord::ORIGIN;
        assert_eq!(o.neighbour(Direction::East), Coord::new(1, 0));
        assert_eq!(o.neighbour(Direction::West), Coord::new(-1, 0));
        assert_eq!(o.neighbour(Direction::SouthEast), Coord::new(0, 1));
        assert_eq!(o.neighbour(Direction::NorthWest), Coord::new(0, -1));
        assert_eq!(o.neighbour(Direction::NorthEast), Coord::new(1, -1));
        assert_eq!(o.neighbour(Direction::SouthWest), Coord::new(-1, 1));
    }

    #[test]
    fn all_six_directions_distinct() {
        let n = Coord::ORIGIN.neighbours();
        for i in 0..6 {
            for j in (i + 1)..6 {
                assert_ne!(n[i], n[j]);
            }
        }
    }

    #[test]
    fn directions_all_is_exhaustive() {
        assert_eq!(Direction::ALL.len(), 6);
        for d in Direction::ALL.iter() {
            let (dq, dr) = d.delta();
            assert!((dq, dr) != (0, 0));
        }
    }

    #[test]
    fn neighbour_is_symmetric() {
        // Every neighbour direction has an inverse such that c.n(d).n(d.inverse()) == c
        let inv = |d: Direction| match d {
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::SouthEast => Direction::NorthWest,
            Direction::NorthWest => Direction::SouthEast,
            Direction::NorthEast => Direction::SouthWest,
            Direction::SouthWest => Direction::NorthEast,
        };
        let c = Coord::new(3, -2);
        for &d in Direction::ALL.iter() {
            assert_eq!(c.neighbour(d).neighbour(inv(d)), c);
        }
    }

    #[test]
    fn rotated_is_periodic() {
        for &d in Direction::ALL.iter() {
            assert_eq!(d.rotated(0), d);
            assert_eq!(d.rotated(6), d);
            assert_eq!(d.rotated(-6), d);
        }
    }
}
