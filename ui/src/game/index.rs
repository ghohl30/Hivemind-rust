//! Legal-move indexing for the interaction layer.
//!
//! The render/interaction code needs to answer two questions fast:
//!   - "I clicked this piece — where can it go?" (group by mover)
//!   - "I hovered this cell — what could land here?" (group by destination)
//!
//! and it needs to know when the only legal action is a forced [`Move::Pass`],
//! which has no piece or destination to highlight.
//!
//! This is a pure, cheap projection of `State::legal_moves()`; rebuild it
//! whenever the position changes. It owns no engine state.

use std::collections::BTreeMap;

use hive_engine::{Coord, Move, PieceId, State};

/// Indexed view over the legal moves in one position.
#[derive(Clone, Debug, Default)]
pub struct LegalMoveIndex {
    /// Every legal move, in engine order.
    all: Vec<Move>,
    /// Legal moves keyed by the piece they move (`Place`/`Slide`). `Pass` is
    /// excluded — it moves no piece.
    by_piece: BTreeMap<PieceId, Vec<Move>>,
    /// Legal moves keyed by destination coord. `Pass` is excluded.
    by_destination: BTreeMap<Coord, Vec<Move>>,
    /// True iff the only legal move is `Pass` (forced pass).
    forced_pass: bool,
}

impl LegalMoveIndex {
    /// Build the index from the current position's legal moves.
    pub fn from_state(state: &State) -> Self {
        let all: Vec<Move> = state.legal_moves().into_iter().collect();
        Self::from_moves(all)
    }

    /// Build directly from a move list (test seam; production uses
    /// [`LegalMoveIndex::from_state`]).
    pub fn from_moves(all: Vec<Move>) -> Self {
        let forced_pass = all.len() == 1 && all[0] == Move::Pass;

        let mut by_piece: BTreeMap<PieceId, Vec<Move>> = BTreeMap::new();
        let mut by_destination: BTreeMap<Coord, Vec<Move>> = BTreeMap::new();

        for &m in &all {
            match m {
                Move::Place { piece, to } | Move::Slide { piece, to } => {
                    by_piece.entry(piece).or_default().push(m);
                    by_destination.entry(to).or_default().push(m);
                }
                Move::Pass => {}
            }
        }

        Self {
            all,
            by_piece,
            by_destination,
            forced_pass,
        }
    }

    /// All legal moves, in engine order.
    pub fn all(&self) -> &[Move] {
        &self.all
    }

    /// Whether there are no legal moves at all (only happens at a terminal
    /// position — non-terminal positions always have at least `Pass`).
    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// Whether the only legal move is `Pass`. The UI should surface a
    /// pass-only affordance and not try to highlight pieces or cells.
    pub fn is_forced_pass(&self) -> bool {
        self.forced_pass
    }

    /// The legal moves for a given piece (its selectable destinations). Empty
    /// slice if the piece cannot move this turn.
    pub fn moves_for_piece(&self, piece: PieceId) -> &[Move] {
        self.by_piece
            .get(&piece)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The destination coords reachable by a given piece this turn.
    pub fn destinations_for_piece(&self, piece: PieceId) -> Vec<Coord> {
        self.moves_for_piece(piece)
            .iter()
            .filter_map(|m| match m {
                Move::Place { to, .. } | Move::Slide { to, .. } => Some(*to),
                Move::Pass => None,
            })
            .collect()
    }

    /// The legal moves that land on a given coord. Empty slice if nothing can
    /// move/place there.
    pub fn moves_to(&self, dest: Coord) -> &[Move] {
        self.by_destination
            .get(&dest)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Every piece that has at least one legal move this turn.
    pub fn movable_pieces(&self) -> impl Iterator<Item = PieceId> + '_ {
        self.by_piece.keys().copied()
    }
}
