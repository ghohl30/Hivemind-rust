//! `Player` trait and concrete implementations.
//!
//! A `Player` encapsulates a strategy for choosing a move given the current
//! game state. Implementations are expected to be deterministic; `choose_move`
//! is called only when `state.is_terminal()` returns `None` and
//! `state.legal_moves()` is non-empty.

use crate::moves::Move;
use crate::search::{search, TranspositionTable};
use crate::state::State;

/// Chooses a move for the current side-to-move in `state`.
///
/// Contract:
/// - Called only when `state.is_terminal().is_none()`.
/// - The returned `Move` must appear in `state.legal_moves()`.
/// - Implementations must not permanently mutate `state` (make-unmake is fine
///   internally as long as balance is maintained on return).
pub trait Player {
    fn choose_move(&mut self, state: &State) -> Move;
}

// ---------------------------------------------------------------------------
// SearchPlayer
// ---------------------------------------------------------------------------

/// Uses the engine's negamax alpha-beta search to pick a move. Reuses its
/// `TranspositionTable` across calls within a game for better move ordering.
pub struct SearchPlayer {
    pub depth: u8,
    pub tt: TranspositionTable,
}

impl SearchPlayer {
    /// Construct a `SearchPlayer` with the given fixed depth and a
    /// transposition table of `1 << tt_log2` slots.
    pub fn new(depth: u8, tt_log2: u32) -> Self {
        Self {
            depth,
            tt: TranspositionTable::with_capacity_log2(tt_log2),
        }
    }
}

impl Player for SearchPlayer {
    fn choose_move(&mut self, state: &State) -> Move {
        // search() takes &mut State but uses make-unmake internally, so we
        // clone the state for the search call — the original is untouched.
        let mut working = state.clone();
        let (_score, best, _stats) = search(&mut working, self.depth, &mut self.tt);
        // search() returns None only when the position is terminal, which the
        // caller guarantees it is not. Unwrap is safe.
        best.expect("SearchPlayer: search returned no move for a non-terminal position")
    }
}

// ---------------------------------------------------------------------------
// FirstMovePlayer
// ---------------------------------------------------------------------------

/// Always picks the first move from `state.legal_moves()`. Deterministic and
/// cheap; useful as a dummy opponent in tests and benchmarks.
pub struct FirstMovePlayer;

impl Player for FirstMovePlayer {
    fn choose_move(&mut self, state: &State) -> Move {
        state
            .legal_moves()
            .into_iter()
            .next()
            .expect("FirstMovePlayer: no legal moves in a non-terminal position")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_move_player_returns_legal_move() {
        let s = State::new();
        let mut p = FirstMovePlayer;
        let m = p.choose_move(&s);
        assert!(s.legal_moves().contains(&m));
    }

    #[test]
    fn search_player_returns_legal_move() {
        let s = State::new();
        let mut p = SearchPlayer::new(2, 14);
        let m = p.choose_move(&s);
        assert!(s.legal_moves().contains(&m));
    }

    #[test]
    fn search_player_does_not_mutate_state() {
        let s = State::new();
        let mut p = SearchPlayer::new(2, 14);
        // choose_move should leave the original state unchanged
        let m = p.choose_move(&s);
        assert!(s.legal_moves().contains(&m));
        assert_eq!(s.undo_depth(), 0, "choose_move left undo records on the state");
    }
}
