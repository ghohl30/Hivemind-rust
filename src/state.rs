//! `State`: source-of-truth game state. All mutation funnels through `apply`.
//!
//! Phase 3 introduces make-unmake: `apply` pushes an `UndoRecord` onto
//! `undo_stack`, and `unapply` pops and reverses. Phase 4 extends each
//! `UndoRecord` with `SetDelta`s for the perimeter and per-color
//! placement-legality caches. Round-trip equality is guarded by the property
//! test in `tests/proptest_invariants.rs`.

use std::collections::HashSet;

use smallvec::SmallVec;

use crate::board::Board;
use crate::coord::Coord;
use crate::gen;
use crate::moves::Move;
use crate::piece::{Color, PieceId, PieceSlot, StackTop};
use crate::rules::{
    membership_triple, queen_is_placed, queen_must_be_placed_now, queen_surrounded,
};
use crate::zobrist::{piece_key, SIDE_TO_MOVE_KEY};

/// Add/remove pair describing how a HashSet<Coord> changed across one `apply`.
/// Inline up to 8 entries per direction — typical moves change very few cells.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SetDelta {
    added: SmallVec<[Coord; 8]>,
    removed: SmallVec<[Coord; 8]>,
}

impl SetDelta {
    #[allow(dead_code)]
    fn apply(&self, set: &mut HashSet<Coord>) {
        for c in &self.removed {
            set.remove(c);
        }
        for c in &self.added {
            set.insert(*c);
        }
    }

    fn reverse(&self, set: &mut HashSet<Coord>) {
        for c in &self.added {
            set.remove(c);
        }
        for c in &self.removed {
            set.insert(*c);
        }
    }

    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Cells whose membership in `perimeter` / `placement_legality_*` might change
/// across `m`. Conservative: includes `from`, `to`, and each of their six
/// neighbours; duplicates are removed via the HashSet.
fn affected_coords_for(m: Move, from: Option<Coord>) -> HashSet<Coord> {
    let mut out: HashSet<Coord> = HashSet::with_capacity(16);
    match m {
        Move::Pass => {}
        Move::Place { to, .. } => {
            out.insert(to);
            out.extend(to.neighbours().iter().copied());
        }
        Move::Slide { to, .. } => {
            if let Some(f) = from {
                out.insert(f);
                out.extend(f.neighbours().iter().copied());
            }
            out.insert(to);
            out.extend(to.neighbours().iter().copied());
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Outcome {
    Win(Color),
    Draw,
}

/// Record needed to reverse a single `apply`. Stores board + zobrist + meta
/// restoration plus the per-cache deltas added in Phase 4.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoRecord {
    move_made: Move,
    /// For `Slide`: the moving piece's slot before the move. Unused for
    /// `Place` / `Pass`.
    prev_slot: PieceSlot,
    /// For `Slide` onto an occupied cell: the piece that became `Covered` at
    /// the destination. Restoring it means demoting back to `OnBoard`.
    covered_piece: Option<PieceId>,
    /// For `Slide` from a stacked position: the piece that was un-covered at
    /// the origin. Restoring it means re-covering it.
    revealed_piece: Option<PieceId>,
    prev_side: Color,
    prev_white_turn: u8,
    prev_black_turn: u8,
    prev_placements_so_far: u16,
    prev_zobrist: u64,
    perimeter_delta: SetDelta,
    white_legality_delta: SetDelta,
    black_legality_delta: SetDelta,
}

/// Game state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    pieces: [PieceSlot; PieceId::COUNT],
    board: Board,
    side_to_move: Color,
    /// 1-indexed: the number of the turn `side_to_move` is *about* to play if
    /// `side_to_move == White`. Otherwise unchanged from white's last value.
    white_turn: u8,
    black_turn: u8,
    /// Total placements made so far by either player. Drives the two opening
    /// placement special cases.
    placements_so_far: u16,
    /// Incrementally maintained Zobrist hash. Invariant guarded by the
    /// `tests/proptest_invariants.rs::zobrist_matches_from_scratch` property
    /// test: `state.zobrist == zobrist::from_scratch(&state)` after any apply.
    zobrist: u64,
    /// Phase 4: empty cells adjacent to the hive. Maintained incrementally
    /// via deltas in `UndoRecord`. Invariant: equals
    /// `rules::perimeter_from_scratch(&self.board)` after every apply/unapply.
    perimeter: HashSet<Coord>,
    /// Per-color general placement-legality (post-opening). A cell is in the
    /// set iff it is empty, touches ≥1 own-color top neighbour, and 0
    /// enemy-color top neighbours. Opening special cases (placements_so_far
    /// < 2) are handled in `gen`, not in this cache.
    placement_legality_white: HashSet<Coord>,
    placement_legality_black: HashSet<Coord>,
    /// Make-unmake history. `apply` pushes, `unapply` pops. Cloning a state
    /// clones its undo stack, which is rarely what callers want in a search —
    /// prefer make-unmake on `&mut State`.
    undo_stack: Vec<UndoRecord>,
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

impl State {
    pub fn new() -> Self {
        State {
            pieces: [PieceSlot::InHand; PieceId::COUNT],
            board: Board::new(),
            side_to_move: Color::White,
            white_turn: 1,
            black_turn: 1,
            placements_so_far: 0,
            // Empty board + White to move ⇒ hash 0. SIDE_TO_MOVE_KEY is only
            // XOR'd when side_to_move is Black.
            zobrist: 0,
            perimeter: HashSet::new(),
            placement_legality_white: HashSet::new(),
            placement_legality_black: HashSet::new(),
            undo_stack: Vec::new(),
        }
    }

    /// Cached empty-cells-adjacent-to-hive set. Phase 4 incremental cache.
    pub fn perimeter(&self) -> &HashSet<Coord> {
        &self.perimeter
    }

    /// Cached placement-legality set for `color`, post-opening. Phase 4
    /// incremental cache. NOTE: callers in the very first two placements of
    /// the game must use the opening-special-case logic in `gen`, not this
    /// cache — the cache reflects the general rule only.
    pub fn placement_legality(&self, color: Color) -> &HashSet<Coord> {
        match color {
            Color::White => &self.placement_legality_white,
            Color::Black => &self.placement_legality_black,
        }
    }

    /// Depth of the make-unmake stack. Mostly a diagnostic: a balanced search
    /// should leave this at the same value it found.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// The incrementally-maintained Zobrist hash of this state.
    pub fn zobrist(&self) -> u64 {
        self.zobrist
    }

    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    pub fn turn_for(&self, color: Color) -> u8 {
        match color {
            Color::White => self.white_turn,
            Color::Black => self.black_turn,
        }
    }

    pub fn piece_slot(&self, id: PieceId) -> PieceSlot {
        self.pieces[id.index()]
    }

    pub fn pieces(&self) -> &[PieceSlot; PieceId::COUNT] {
        &self.pieces
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn placements_so_far(&self) -> usize {
        self.placements_so_far as usize
    }

    /// Per the brief: a Win records the *winning* color; a Draw means both
    /// queens are surrounded simultaneously.
    pub fn is_terminal(&self) -> Option<Outcome> {
        let white_dead = queen_surrounded(&self.pieces, &self.board, Color::White);
        let black_dead = queen_surrounded(&self.pieces, &self.board, Color::Black);
        match (white_dead, black_dead) {
            (true, true) => Some(Outcome::Draw),
            (true, false) => Some(Outcome::Win(Color::Black)),
            (false, true) => Some(Outcome::Win(Color::White)),
            (false, false) => None,
        }
    }

    pub fn legal_moves(&self) -> SmallVec<[Move; 64]> {
        let mut out: SmallVec<[Move; 64]> = SmallVec::new();
        if self.is_terminal().is_some() {
            return out;
        }
        let color = self.side_to_move;
        let current_turn = self.turn_for(color);

        if queen_must_be_placed_now(&self.pieces, color, current_turn) {
            gen::generate_queen_placement(self, color, &mut out);
        } else {
            gen::generate_placements(self, color, &mut out);
            if queen_is_placed(&self.pieces, color) {
                gen::generate_movements(self, color, &mut out);
            }
        }
        if out.is_empty() {
            out.push(Move::Pass);
        }
        out
    }

    /// Apply a move. In debug builds, asserts the move is legal. Pushes an
    /// `UndoRecord` so `unapply` can reverse the change exactly.
    ///
    /// Phase 5: the record is built directly in the `undo_stack`'s spare slot
    /// via `MaybeUninit`. Previously we constructed a stub `UndoRecord` on the
    /// stack, mutated its fields during apply, then `push`'d it (a ~300-byte
    /// stack→heap memcpy). The MaybeUninit path skips that final copy: per-
    /// variant fields land in locals, the SetDelta accumulators live on the
    /// stack while we walk `pre`, and `MaybeUninit::write` emplaces the whole
    /// record once at the end.
    pub fn apply(&mut self, m: Move) {
        if cfg!(debug_assertions) {
            let legal = self.legal_moves();
            debug_assert!(
                legal.iter().any(|x| *x == m),
                "illegal move {:?}; legal: {:?}",
                m,
                legal
            );
        }
        // Snapshot meta needed for reversal.
        let prev_side = self.side_to_move;
        let prev_white_turn = self.white_turn;
        let prev_black_turn = self.black_turn;
        let prev_placements_so_far = self.placements_so_far;
        let prev_zobrist = self.zobrist;
        // Per-variant data; populated by the match below.
        let mut prev_slot = PieceSlot::InHand;
        let mut covered_piece: Option<PieceId> = None;
        let mut revealed_piece: Option<PieceId> = None;

        // Phase 4: snapshot per-coord cache membership BEFORE mutating board/pieces.
        let current_from: Option<Coord> = match m {
            Move::Slide { piece, .. } => match self.pieces[piece.index()] {
                PieceSlot::OnBoard { coord, .. } => Some(coord),
                _ => unreachable!("Slide must originate from OnBoard top"),
            },
            _ => None,
        };
        // Sort the affected coords so delta vec ordering is deterministic
        // across runs (HashSet iteration order is randomized, which would
        // otherwise break `apply_deterministic`).
        let affected = affected_coords_for(m, current_from);
        let mut affected_sorted: SmallVec<[Coord; 16]> = affected.into_iter().collect();
        affected_sorted.sort();
        let pre: SmallVec<[(Coord, bool, bool, bool); 16]> = affected_sorted
            .iter()
            .map(|c| {
                let (p, w, b) = membership_triple(&self.board, *c);
                (*c, p, w, b)
            })
            .collect();
        match m {
            Move::Pass => { /* board unchanged */ }
            Move::Place { piece, to } => {
                self.pieces[piece.index()] = PieceSlot::OnBoard {
                    coord: to,
                    stack_height: 0,
                };
                self.board.set_top(to, StackTop { piece, height: 0 });
                self.placements_so_far += 1;
                self.zobrist ^= piece_key(piece.color(), piece.piece_type(), to, 0);
            }
            Move::Slide { piece, to } => {
                let slot_before = self.pieces[piece.index()];
                let (from, h_from) = match slot_before {
                    PieceSlot::OnBoard { coord, stack_height } => (coord, stack_height),
                    _ => unreachable!("Slide must originate from OnBoard top"),
                };
                prev_slot = slot_before;
                // Hash out the moving piece's old position.
                self.zobrist ^= piece_key(piece.color(), piece.piece_type(), from, h_from);
                // Land at destination, possibly covering existing top. Pieces
                // that become Covered keep their coord + stack_height, so their
                // zobrist contribution is unchanged.
                let new_height = match self.board.top_at(to) {
                    Some(top) => {
                        self.pieces[top.piece.index()] = PieceSlot::Covered {
                            coord: to,
                            stack_height: top.height,
                        };
                        covered_piece = Some(top.piece);
                        top.height + 1
                    }
                    None => 0,
                };
                self.pieces[piece.index()] = PieceSlot::OnBoard {
                    coord: to,
                    stack_height: new_height,
                };
                self.board.set_top(to, StackTop { piece, height: new_height });
                // Hash in the moving piece's new position.
                self.zobrist ^= piece_key(piece.color(), piece.piece_type(), to, new_height);

                // Restore stack at `from`. If beetle was at ground level, clear.
                // Else promote the piece directly below to OnBoard. The piece
                // below kept its coord + stack_height — no zobrist change.
                if h_from == 0 {
                    self.board.clear(from);
                } else {
                    let below_pid = self
                        .find_covered_at(from, h_from - 1)
                        .expect("piece must exist directly below the moving beetle");
                    self.pieces[below_pid.index()] = PieceSlot::OnBoard {
                        coord: from,
                        stack_height: h_from - 1,
                    };
                    self.board.set_top(
                        from,
                        StackTop {
                            piece: below_pid,
                            height: h_from - 1,
                        },
                    );
                    revealed_piece = Some(below_pid);
                }
            }
        }
        // Advance turn for the player who just moved.
        match self.side_to_move {
            Color::White => self.white_turn = self.white_turn.saturating_add(1),
            Color::Black => self.black_turn = self.black_turn.saturating_add(1),
        }
        self.side_to_move = self.side_to_move.other();
        self.zobrist ^= SIDE_TO_MOVE_KEY;
        // Phase 4: recompute cache membership for affected coords and build deltas.
        let mut perimeter_delta = SetDelta::default();
        let mut white_legality_delta = SetDelta::default();
        let mut black_legality_delta = SetDelta::default();
        for (c, was_p, was_w, was_b) in pre {
            let (is_p, is_w, is_b) = membership_triple(&self.board, c);
            if was_p != is_p {
                if is_p {
                    self.perimeter.insert(c);
                    perimeter_delta.added.push(c);
                } else {
                    self.perimeter.remove(&c);
                    perimeter_delta.removed.push(c);
                }
            }
            if was_w != is_w {
                if is_w {
                    self.placement_legality_white.insert(c);
                    white_legality_delta.added.push(c);
                } else {
                    self.placement_legality_white.remove(&c);
                    white_legality_delta.removed.push(c);
                }
            }
            if was_b != is_b {
                if is_b {
                    self.placement_legality_black.insert(c);
                    black_legality_delta.added.push(c);
                } else {
                    self.placement_legality_black.remove(&c);
                    black_legality_delta.removed.push(c);
                }
            }
        }
        // Emplace the record directly into the undo_stack's spare slot.
        // SAFETY: `reserve(1)` guarantees `spare_capacity_mut()` has ≥1 slot.
        // We write a fully-initialized UndoRecord into that slot, then bump
        // the length. No double-init, no leaks.
        self.undo_stack.reserve(1);
        let new_len = self.undo_stack.len() + 1;
        self.undo_stack.spare_capacity_mut()[0].write(UndoRecord {
            move_made: m,
            prev_slot,
            covered_piece,
            revealed_piece,
            prev_side,
            prev_white_turn,
            prev_black_turn,
            prev_placements_so_far,
            prev_zobrist,
            perimeter_delta,
            white_legality_delta,
            black_legality_delta,
        });
        unsafe { self.undo_stack.set_len(new_len) };
    }

    /// Reverse the most recent `apply`. Panics if the undo stack is empty.
    pub fn unapply(&mut self) {
        let rec = self
            .undo_stack
            .pop()
            .expect("unapply called with no prior apply");
        // Restore meta in one shot. Board / pieces restoration is per-variant.
        self.side_to_move = rec.prev_side;
        self.white_turn = rec.prev_white_turn;
        self.black_turn = rec.prev_black_turn;
        self.placements_so_far = rec.prev_placements_so_far;
        self.zobrist = rec.prev_zobrist;
        // Reverse Phase 4 cache deltas.
        rec.perimeter_delta.reverse(&mut self.perimeter);
        rec.white_legality_delta
            .reverse(&mut self.placement_legality_white);
        rec.black_legality_delta
            .reverse(&mut self.placement_legality_black);
        match rec.move_made {
            Move::Pass => { /* no board/pieces change */ }
            Move::Place { piece, to } => {
                self.pieces[piece.index()] = PieceSlot::InHand;
                self.board.clear(to);
            }
            Move::Slide { piece, to } => {
                let (from, h_from) = match rec.prev_slot {
                    PieceSlot::OnBoard { coord, stack_height } => (coord, stack_height),
                    _ => unreachable!("UndoRecord.prev_slot must be OnBoard for a Slide"),
                };
                // Restore the moving piece's previous slot.
                self.pieces[piece.index()] = rec.prev_slot;
                // Restore stack at `to`.
                match rec.covered_piece {
                    Some(covered_pid) => {
                        // Demote-back: Covered → OnBoard at the same height.
                        let h_at_to = match self.pieces[covered_pid.index()] {
                            PieceSlot::Covered { stack_height, .. } => stack_height,
                            other => unreachable!(
                                "covered_piece {covered_pid:?} not Covered: {other:?}"
                            ),
                        };
                        self.pieces[covered_pid.index()] = PieceSlot::OnBoard {
                            coord: to,
                            stack_height: h_at_to,
                        };
                        self.board.set_top(
                            to,
                            StackTop {
                                piece: covered_pid,
                                height: h_at_to,
                            },
                        );
                    }
                    None => {
                        self.board.clear(to);
                    }
                }
                // Restore stack at `from`: re-cover the revealed piece if any,
                // and put the moving piece back as the top.
                if let Some(revealed_pid) = rec.revealed_piece {
                    let h_at_from = match self.pieces[revealed_pid.index()] {
                        PieceSlot::OnBoard { stack_height, .. } => stack_height,
                        other => unreachable!(
                            "revealed_piece {revealed_pid:?} not OnBoard: {other:?}"
                        ),
                    };
                    self.pieces[revealed_pid.index()] = PieceSlot::Covered {
                        coord: from,
                        stack_height: h_at_from,
                    };
                }
                self.board.set_top(from, StackTop { piece, height: h_from });
            }
        }
    }

    fn find_covered_at(&self, coord: Coord, height: u8) -> Option<PieceId> {
        for (i, slot) in self.pieces.iter().enumerate() {
            if let PieceSlot::Covered {
                coord: c,
                stack_height: h,
            } = slot
            {
                if *c == coord && *h == height {
                    return Some(PieceId(i as u8));
                }
            }
        }
        None
    }

    /// Reconstruct a fresh `Board` from the `pieces` array. Used by tests to
    /// assert coherence between the two state representations.
    pub fn board_from_pieces(&self) -> Board {
        let mut b = Board::new();
        // For each coord, find the piece with maximum stack_height (that's the top).
        // O(22²) — fine for Phase 1.
        for i in 0..PieceId::COUNT {
            if let PieceSlot::OnBoard { coord, stack_height } = self.pieces[i] {
                b.set_top(
                    coord,
                    StackTop {
                        piece: PieceId(i as u8),
                        height: stack_height,
                    },
                );
            }
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::WHITE_QUEEN;

    #[test]
    fn new_state_invariants() {
        let s = State::new();
        assert_eq!(s.side_to_move(), Color::White);
        assert_eq!(s.turn_for(Color::White), 1);
        assert_eq!(s.turn_for(Color::Black), 1);
        assert_eq!(s.placements_so_far(), 0);
        assert!(s.is_terminal().is_none());
        for slot in s.pieces().iter() {
            assert!(matches!(slot, PieceSlot::InHand));
        }
        assert!(s.board().is_empty());
    }

    #[test]
    fn first_move_only_origin_placements() {
        let s = State::new();
        let moves = s.legal_moves();
        // First placement must be at origin for any non-queen piece (or queen too).
        assert!(!moves.is_empty());
        for m in moves.iter() {
            match m {
                Move::Place { to, .. } => assert_eq!(*to, Coord::ORIGIN),
                _ => panic!("first move should only be Place: got {m}"),
            }
        }
    }

    #[test]
    fn apply_place_then_state_advances() {
        let mut s = State::new();
        s.apply(Move::Place {
            piece: PieceId(0),
            to: Coord::ORIGIN,
        });
        assert_eq!(s.side_to_move(), Color::Black);
        assert_eq!(s.turn_for(Color::White), 2);
        assert_eq!(s.turn_for(Color::Black), 1);
        assert_eq!(s.placements_so_far(), 1);
        assert_eq!(
            s.piece_slot(WHITE_QUEEN),
            PieceSlot::OnBoard {
                coord: Coord::ORIGIN,
                stack_height: 0,
            }
        );
    }

    #[test]
    fn board_from_pieces_matches_board() {
        let mut s = State::new();
        s.apply(Move::Place {
            piece: PieceId(0),
            to: Coord::ORIGIN,
        });
        s.apply(Move::Place {
            piece: PieceId(11),
            to: Coord::new(1, 0),
        });
        assert_eq!(s.board_from_pieces(), *s.board());
    }

    #[test]
    fn zobrist_starts_at_zero_and_changes_on_apply() {
        let mut s = State::new();
        assert_eq!(s.zobrist(), 0);
        s.apply(Move::Place {
            piece: PieceId(0),
            to: Coord::ORIGIN,
        });
        assert_ne!(s.zobrist(), 0);
    }

    #[test]
    fn zobrist_matches_from_scratch_after_a_few_moves() {
        use crate::zobrist;
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        assert_eq!(s.zobrist(), zobrist::from_scratch(&s));
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });
        assert_eq!(s.zobrist(), zobrist::from_scratch(&s));
        s.apply(Move::Place { piece: PieceId(6), to: Coord::new(-1, 0) });
        assert_eq!(s.zobrist(), zobrist::from_scratch(&s));
    }

    #[test]
    fn round_trip_place_at_origin() {
        let mut s = State::new();
        let snap = s.clone();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        s.unapply();
        assert_eq!(s, snap);
    }

    #[test]
    fn round_trip_beetle_climb_and_descend() {
        // Build a state with a beetle climbing onto a queen, then verify
        // round-trip for both climb and descend.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });          // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });      // BQ
        s.apply(Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });      // WB
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(2, 0) });      // BB
        // Now W turn 3, queens placed → movements legal.
        let snap = s.clone();
        // Climb: white beetle onto white queen.
        s.apply(Move::Slide { piece: PieceId(1), to: Coord::ORIGIN });
        s.unapply();
        assert_eq!(s, snap, "round-trip diverged after beetle climb");
    }

    #[test]
    fn round_trip_multi_move_unwinds_to_start() {
        let mut s = State::new();
        let snap = s.clone();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });
        s.apply(Move::Place { piece: PieceId(6), to: Coord::new(-1, 0) });
        s.unapply();
        s.unapply();
        s.unapply();
        assert_eq!(s, snap);
        assert_eq!(s.undo_depth(), 0);
    }

    #[test]
    fn undo_stack_balanced_after_search_pattern() {
        // Simulate a 2-ply tree expansion with full unwinding.
        let mut s = State::new();
        let starting_depth = s.undo_depth();
        let moves = s.legal_moves();
        for m in moves.iter().take(3) {
            s.apply(*m);
            for m2 in s.legal_moves().iter().take(2) {
                s.apply(*m2);
                s.unapply();
            }
            s.unapply();
        }
        assert_eq!(s.undo_depth(), starting_depth);
    }
}
