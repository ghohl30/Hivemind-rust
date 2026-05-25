//! Coord-keyed top-of-stack lookup. Dumb on purpose — no rule knowledge.
//!
//! Mutation is `pub(crate)` so only `state::apply` may write. The full per-coord
//! stack is reconstructed from the `[PieceSlot; 22]` array in `state` when
//! needed.
//!
//! Phase 5 (perf branch): backing storage is a **flat 64×64 array indexed by
//! packed `(q, r)`**, plus a sorted `SmallVec<[Coord; 22]>` for iteration.
//! `is_occupied` / `top_at` become a single memory access — no binary search,
//! no hashing. The flat array is 8 KB inline in `State`; cloning is one memcpy
//! and is only done in tests (search uses make-unmake). The previous sorted-
//! SmallVec implementation made membership a binary search of ~22 entries
//! (~5 i16 comparisons each), called millions of times per `apply` inside
//! `membership_triple` and the slide checks.
//!
//! Coord range is `q, r ∈ [-32, 31]`. A hive starting at origin can reach at
//! most ~22 cells in any direction, so this is comfortably safe; debug builds
//! assert the bound.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::piece::{PieceId, StackTop};

/// Side length of the flat grid. Power of two so the pack multiplication
/// collapses to a shift in release builds.
const BOARD_SIDE: usize = 64;
const BOARD_OFFSET: i16 = 32;
const BOARD_LEN: usize = BOARD_SIDE * BOARD_SIDE;

/// 2-byte cell: `piece == 0xFF` means empty (no `StackTop`). Otherwise piece
/// is a PieceId byte and height is the stack height of the top.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
struct Cell {
    piece: u8,
    height: u8,
}

const EMPTY: Cell = Cell {
    piece: 0xFF,
    height: 0,
};

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Board {
    /// Flat grid indexed by `pack(coord)`. 8 KB inline.
    cells: [Cell; BOARD_LEN],
    /// Sorted-ascending list of currently-occupied coords. Kept in sync with
    /// `cells` so iteration is cheap (no scanning 4 KB of cells looking for
    /// non-empties) and `PartialEq` is correct without ambiguity.
    occupied: SmallVec<[Coord; 22]>,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            cells: [EMPTY; BOARD_LEN],
            occupied: SmallVec::new(),
        }
    }
}

#[inline(always)]
fn pack(c: Coord) -> usize {
    debug_assert!(
        c.q >= -BOARD_OFFSET && c.q < BOARD_OFFSET && c.r >= -BOARD_OFFSET && c.r < BOARD_OFFSET,
        "coord {:?} outside board range [-{},{})",
        c,
        BOARD_OFFSET,
        BOARD_OFFSET
    );
    ((c.q + BOARD_OFFSET) as usize) * BOARD_SIDE + ((c.r + BOARD_OFFSET) as usize)
}

impl Board {
    pub fn new() -> Self {
        Board::default()
    }

    #[inline]
    pub fn top_at(&self, c: Coord) -> Option<StackTop> {
        let cell = self.cells[pack(c)];
        if cell.piece == 0xFF {
            None
        } else {
            Some(StackTop {
                piece: PieceId(cell.piece),
                height: cell.height,
            })
        }
    }

    #[inline]
    pub fn is_occupied(&self, c: Coord) -> bool {
        self.cells[pack(c)].piece != 0xFF
    }

    pub fn occupied_coords(&self) -> impl Iterator<Item = Coord> + '_ {
        self.occupied.iter().copied()
    }

    pub fn entries(&self) -> impl Iterator<Item = (Coord, StackTop)> + '_ {
        self.occupied.iter().map(move |c| {
            let cell = self.cells[pack(*c)];
            (
                *c,
                StackTop {
                    piece: PieceId(cell.piece),
                    height: cell.height,
                },
            )
        })
    }

    pub fn len(&self) -> usize {
        self.occupied.len()
    }

    pub fn is_empty(&self) -> bool {
        self.occupied.is_empty()
    }

    pub(crate) fn set_top(&mut self, c: Coord, t: StackTop) {
        let idx = pack(c);
        let was_empty = self.cells[idx].piece == 0xFF;
        self.cells[idx] = Cell {
            piece: t.piece.0,
            height: t.height,
        };
        if was_empty {
            // Sorted insert into the occupied list.
            let pos = self.occupied.partition_point(|x| *x < c);
            self.occupied.insert(pos, c);
        }
    }

    pub(crate) fn clear(&mut self, c: Coord) {
        let idx = pack(c);
        if self.cells[idx].piece != 0xFF {
            self.cells[idx] = EMPTY;
            if let Ok(pos) = self.occupied.binary_search(&c) {
                self.occupied.remove(pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(idx: u8, height: u8) -> StackTop {
        StackTop {
            piece: PieceId(idx),
            height,
        }
    }

    #[test]
    fn set_then_get_roundtrip() {
        let mut b = Board::new();
        let c = Coord::new(2, -1);
        assert_eq!(b.top_at(c), None);
        b.set_top(c, st(3, 0));
        assert_eq!(b.top_at(c), Some(st(3, 0)));
        assert!(b.is_occupied(c));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn clear_removes() {
        let mut b = Board::new();
        let c = Coord::ORIGIN;
        b.set_top(c, st(0, 0));
        b.clear(c);
        assert_eq!(b.top_at(c), None);
        assert!(!b.is_occupied(c));
        assert!(b.is_empty());
    }

    #[test]
    fn occupied_coords_iterates_all() {
        let mut b = Board::new();
        b.set_top(Coord::new(0, 0), st(0, 0));
        b.set_top(Coord::new(1, 0), st(1, 0));
        b.set_top(Coord::new(0, 1), st(2, 0));
        let mut v: Vec<Coord> = b.occupied_coords().collect();
        v.sort();
        assert_eq!(v, vec![Coord::new(0, 0), Coord::new(0, 1), Coord::new(1, 0)]);
    }

    #[test]
    fn equal_after_different_insert_orders() {
        let mut a = Board::new();
        let mut b = Board::new();
        a.set_top(Coord::new(1, 0), st(0, 0));
        a.set_top(Coord::new(-1, 0), st(1, 0));
        a.set_top(Coord::new(0, 1), st(2, 0));
        b.set_top(Coord::new(0, 1), st(2, 0));
        b.set_top(Coord::new(1, 0), st(0, 0));
        b.set_top(Coord::new(-1, 0), st(1, 0));
        assert_eq!(a, b);
    }

    #[test]
    fn set_top_overwrites_existing() {
        let mut b = Board::new();
        let c = Coord::new(0, 0);
        b.set_top(c, st(0, 0));
        b.set_top(c, st(5, 1));
        assert_eq!(b.top_at(c), Some(st(5, 1)));
        assert_eq!(b.len(), 1);
    }
}
