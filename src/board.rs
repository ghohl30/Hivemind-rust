//! Coord-keyed top-of-stack lookup. Dumb on purpose — no rule knowledge.
//!
//! Mutation is `pub(crate)` so only `state::apply` may write. The full per-coord
//! stack is reconstructed from the `[PieceSlot; 22]` array in `state` when
//! needed.
//!
//! Phase 5: backing storage switched from `HashMap<Coord, StackTop>` to a
//! sorted `SmallVec<[(Coord, StackTop); 22]>`. Since the board can hold at most
//! 22 distinct ground-level coords, entries live inline on the stack — no
//! heap, no hashing, no pointer chase. Lookups are a linear scan; with N ≤ 22,
//! that beats a HashMap lookup on a cache miss. Sorted-by-coord invariant
//! keeps `derive(PartialEq)` correct.

use smallvec::SmallVec;

use crate::coord::Coord;
use crate::piece::StackTop;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Board {
    /// (Coord, StackTop) entries, kept sorted ascending by Coord. The ordering
    /// invariant exists for the derived PartialEq (set-equal boards must
    /// compare equal regardless of how they were built).
    entries: SmallVec<[(Coord, StackTop); 22]>,
}

impl Board {
    pub fn new() -> Self {
        Board::default()
    }

    #[inline]
    fn search(&self, c: Coord) -> Result<usize, usize> {
        self.entries.binary_search_by_key(&c, |(k, _)| *k)
    }

    #[inline]
    pub fn top_at(&self, c: Coord) -> Option<StackTop> {
        match self.search(c) {
            Ok(i) => Some(self.entries[i].1),
            Err(_) => None,
        }
    }

    #[inline]
    pub fn is_occupied(&self, c: Coord) -> bool {
        self.search(c).is_ok()
    }

    pub fn occupied_coords(&self) -> impl Iterator<Item = Coord> + '_ {
        self.entries.iter().map(|(c, _)| *c)
    }

    pub fn entries(&self) -> impl Iterator<Item = (Coord, StackTop)> + '_ {
        self.entries.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn set_top(&mut self, c: Coord, t: StackTop) {
        match self.search(c) {
            Ok(i) => self.entries[i].1 = t,
            Err(i) => self.entries.insert(i, (c, t)),
        }
    }

    pub(crate) fn clear(&mut self, c: Coord) {
        if let Ok(i) = self.search(c) {
            self.entries.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::PieceId;

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
        // Sorted-by-Coord invariant: same set of entries ⇒ derive(PartialEq) holds.
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
