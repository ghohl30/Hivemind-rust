//! Game session: the authoritative move list and the `State` derived from it.
//!
//! The single source of truth is the `Vec<Move>` recorded for this game. The
//! live `hive_engine::State` is *derived* by folding `apply` over that list from
//! `State::new()`. We cache the derived state for cheap reads but the move list
//! stays authoritative: every mutation goes through the move list first, then the
//! cache is brought in sync.
//!
//! The replay fold here is exactly the workaround for engine-request #2
//! (`State::from_moves`). It is isolated in [`replay`] so it can be swapped for
//! the engine constructor without touching callers.

use hive_engine::{Move, Outcome, State};

/// Error returned when an attempted move is not legal in the current position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IllegalMove {
    /// The move the caller tried to apply.
    pub attempted: Move,
}

impl core::fmt::Display for IllegalMove {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "illegal move: {}", self.attempted)
    }
}

impl std::error::Error for IllegalMove {}

/// Replay a move list into a `State` by folding `apply` from the initial
/// position. Assumes the moves are legal in sequence (callers that build a
/// `Session` via [`Session::push_move`] guarantee this). Isolated so it can be
/// replaced by `State::from_moves` if/when the engine adds it.
pub fn replay(moves: &[Move]) -> State {
    let mut state = State::new();
    for &m in moves {
        state.apply(m);
    }
    state
}

/// A single Hive game in progress, recorded as the moves played so far.
///
/// The `Vec<Move>` is authoritative; `state` is a cache kept in lock-step.
#[derive(Clone, Debug)]
pub struct Session {
    moves: Vec<Move>,
    state: State,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// A fresh game at the initial position with no moves played.
    pub fn new() -> Self {
        Self {
            moves: Vec::new(),
            state: State::new(),
        }
    }

    /// Reconstruct a session from a previously recorded move list (e.g. loaded
    /// from disk or received over the wire). Validates that every move is legal
    /// in sequence; returns the move that first failed, if any.
    pub fn from_moves(moves: &[Move]) -> Result<Self, IllegalMove> {
        let mut session = Session::new();
        for &m in moves {
            session.push_move(m)?;
        }
        Ok(session)
    }

    /// The authoritative move list for this game.
    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    /// The live derived position. Always in sync with [`Session::moves`].
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Number of moves (plies, including any `Pass`) played so far.
    pub fn ply(&self) -> usize {
        self.moves.len()
    }

    /// Validate `m` against the current legal-move list, then apply it. Returns
    /// [`IllegalMove`] without mutating anything if the move is not legal.
    ///
    /// The UI is expected never to feed an illegal move; this boundary turns a
    /// logic bug into a surfaced error rather than an engine debug-assert panic.
    pub fn push_move(&mut self, m: Move) -> Result<(), IllegalMove> {
        if !self.state.legal_moves().iter().any(|&legal| legal == m) {
            return Err(IllegalMove { attempted: m });
        }
        self.state.apply(m);
        self.moves.push(m);
        Ok(())
    }

    /// Undo the most recently played move, returning it. The derived state is
    /// rebuilt from the (now shorter) authoritative move list.
    pub fn undo(&mut self) -> Option<Move> {
        let last = self.moves.pop()?;
        self.state = replay(&self.moves);
        Some(last)
    }

    /// Outcome if the game is over, else `None`. Thin pass-through to the engine.
    pub fn outcome(&self) -> Option<Outcome> {
        self.state.is_terminal()
    }

    /// Whether the game has ended.
    pub fn is_over(&self) -> bool {
        self.state.is_terminal().is_some()
    }
}
