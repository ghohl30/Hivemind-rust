//! Engine-vs-engine game runner.
//!
//! `play_game` drives a full game between two `Player` implementations.
//! It is deterministic (no randomness), stateless between games (callers manage
//! player state), and suitable for win-rate testing, weight tuning, and
//! self-play data generation.

use crate::player::Player;
use crate::piece::Color;
use crate::state::{Outcome, State};

/// The outcome of a completed (or truncated) game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameResult {
    /// Game outcome. If the game was truncated (`truncated == true`), this is
    /// `Outcome::Draw` — no winner is declared for games that hit the ply cap.
    pub outcome: Outcome,
    /// Number of plies actually played (0 if the initial position is terminal).
    pub plies: u32,
    /// `true` if `max_plies` was reached before a terminal position.
    pub truncated: bool,
}

/// Play a game from the initial position using `white` and `black` as the
/// respective players.
///
/// Terminates when:
/// - `state.is_terminal()` returns `Some(outcome)` — normal finish.
/// - `plies == max_plies` — game is truncated and treated as a draw.
///
/// Neither player is reset between calls; reuse or replace them to control
/// whether TT state carries across games.
pub fn play_game(white: &mut dyn Player, black: &mut dyn Player, max_plies: u32) -> GameResult {
    let mut state = State::new();
    let mut plies = 0u32;

    loop {
        // Check for terminal position before asking a player to move.
        if let Some(outcome) = state.is_terminal() {
            return GameResult {
                outcome,
                plies,
                truncated: false,
            };
        }

        // Hit the ply cap — declare a draw.
        if plies >= max_plies {
            return GameResult {
                outcome: Outcome::Draw,
                plies,
                truncated: true,
            };
        }

        let m = match state.side_to_move() {
            Color::White => white.choose_move(&state),
            Color::Black => black.choose_move(&state),
        };

        state.apply(m);
        plies += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::FirstMovePlayer;

    #[test]
    fn play_game_terminates_within_max_plies() {
        let mut w = FirstMovePlayer;
        let mut b = FirstMovePlayer;
        let result = play_game(&mut w, &mut b, 300);
        assert!(result.plies <= 300);
    }

    #[test]
    fn play_game_truncated_is_draw() {
        let mut w = FirstMovePlayer;
        let mut b = FirstMovePlayer;
        // Force truncation with max_plies = 5.
        let result = play_game(&mut w, &mut b, 5);
        if result.truncated {
            assert_eq!(result.outcome, Outcome::Draw);
            assert_eq!(result.plies, 5);
        }
        // If it finished early, truncated must be false.
        if !result.truncated {
            assert!(result.plies <= 5);
        }
    }

    #[test]
    fn play_game_result_plies_matches_moves_made() {
        // Run a short game and verify the ply count is consistent with the
        // outcome (non-truncated games must have plies > 0).
        let mut w = FirstMovePlayer;
        let mut b = FirstMovePlayer;
        let result = play_game(&mut w, &mut b, 300);
        if !result.truncated {
            // A terminal outcome requires at least enough moves to place all queens.
            assert!(result.plies > 0, "a finished game must have at least 1 ply");
        }
    }
}
