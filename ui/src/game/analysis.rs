//! What the engine has *proven* about the position — and nothing else.
//!
//! Deliberately not an evaluation readout. The search does produce a number,
//! but it is in the units of the engine's own evaluation terms (queen
//! neighbours; Hive has no captures, so there is no material scale to anchor
//! it to) and nothing calibrates those against how games actually finish. A
//! panel reading "-6" tells a player nothing they can act on, and worse, it
//! looks authoritative while being a guess.
//!
//! What the search *can* state without qualification is a forced result: past
//! `MATE_THRESHOLD` it has proven a win against every defence, and the distance
//! is exact. So this module decodes that much and throws the number away.
//!
//! Pure and DOM-free, so the decoding the panel depends on is tested natively
//! rather than by squinting at a browser.

use hive_engine::{Color, MATE_SCORE, MATE_THRESHOLD};

/// Think-time budget for one pondering search, in milliseconds.
///
/// Paired with `config::MAX_DEPTH`: the ponder ends at whichever arrives first
/// — or immediately, if it proves a forced result, since the engine stops
/// deepening once it has one. It is a *cap*, not a target: analysis runs on a
/// second CPU core while the player thinks, and a player who walks away
/// mid-turn should not come back to a laptop that has been spinning for an
/// hour.
pub const ANALYSIS_BUDGET_MS: f64 = 30_000.0;

/// A forced result the search has proven: `winner` wins in `moves`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Forced {
    /// The side that can force the win against any defence.
    pub winner: Color,
    /// Distance to the surrounded queen in plies (half-moves) from the searched
    /// position.
    pub plies: u32,
    /// The same distance counted in the winner's own moves, which is how mate
    /// distance is conventionally stated: `ceil(plies / 2)`.
    pub moves: u32,
}

/// The engine's read on one position after a completed search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Assessment {
    /// Depth the search reached (its last completed iteration).
    pub depth: u8,
    /// The forced result, if the search proved one. `None` is the ordinary
    /// case and means exactly "nothing proven yet at this depth" — *not*
    /// "the position is balanced".
    pub forced: Option<Forced>,
}

impl Assessment {
    /// Interpret a raw search score.
    ///
    /// Returns `None` when `depth == 0`, which is the engine's way of saying no
    /// iteration ran — the position was terminal, or it had a single legal move
    /// and the search short-circuited with a placeholder score. There is
    /// nothing to report in that case, and it should not be confused with a
    /// completed search that found no forced win.
    pub fn from_search(score_stm: i32, side_to_move: Color, depth: u8) -> Option<Self> {
        if depth == 0 {
            return None;
        }
        // Normalise out of negamax first: the raw score belongs to whoever is
        // to move, so its sign flips every half-move. Negating is correct for
        // mate scores too — the encoding is symmetric about zero.
        let white_score = match side_to_move {
            Color::White => score_stm,
            Color::Black => -score_stm,
        };
        Some(Self {
            depth,
            forced: decode_forced(white_score),
        })
    }
}

/// Decode a White-perspective score into a forced result, if it is one.
///
/// Anything under [`MATE_THRESHOLD`] in magnitude is a heuristic judgement, not
/// a proof, and this returns `None` for all of it — however large. "White is
/// winning easily" and "White mates in 3" are different kinds of claim, and
/// only the second one belongs in this panel.
fn decode_forced(white_score: i32) -> Option<Forced> {
    if white_score.abs() < MATE_THRESHOLD {
        return None;
    }
    let winner = if white_score > 0 {
        Color::White
    } else {
        Color::Black
    };
    let plies = (MATE_SCORE - white_score.abs()).max(0) as u32;
    Some(Forced {
        winner,
        plies,
        // The winner moves on plies 1, 3, 5, … when they are to move at the
        // root, so their own move count rounds up.
        moves: plies.div_ceil(2),
    })
}
