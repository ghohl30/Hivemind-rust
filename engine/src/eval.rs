//! Static evaluation, behind a trait so two evaluations can be compared
//! head-to-head in one process.
//!
//! Hive has **no captures**: material is fixed from the first placement to the
//! last. Every bit of playing strength therefore lives in positional terms, which
//! makes the evaluation — not the search — the dominant lever on strength.
//!
//! [`Eval`] is a static-dispatch trait: implementations are zero-sized and
//! `negamax` is generic over them, so a variant costs a monomorphisation rather
//! than a branch in the hottest path in the program. [`LegacyEval`] is a frozen
//! snapshot used only as the gauntlet's control arm; it is scaffolding and
//! should be deleted once the current evaluation has been shown to beat it.

use crate::piece::{queen_of, Color, PieceSlot};
use crate::state::State;

/// A static evaluation function, scored from the side-to-move's perspective
/// (standard negamax convention: positive is good for whoever moves next).
///
/// Implementations must be pure functions of the position. In particular they
/// must not depend on search state, or the transposition table will return
/// scores that disagree with a fresh evaluation of the same position.
pub trait Eval {
    fn evaluate(state: &State) -> i32;
}

/// The evaluation the engine actually plays with.
pub struct CurrentEval;

/// Frozen control arm for A/B testing — the single-term evaluation the engine
/// shipped with before this series of changes.
///
/// Deliberately duplicated rather than shared: `CurrentEval` is meant to move,
/// and a shared helper would drag this along with it, leaving the gauntlet
/// comparing a thing to itself. **Temporary** — delete once the current
/// evaluation has demonstrably beaten it.
pub struct LegacyEval;

impl Eval for LegacyEval {
    fn evaluate(state: &State) -> i32 {
        let stm = state.side_to_move();
        let opp = stm.other();
        let own = queen_neighbours(state, stm) as i32;
        let theirs = queen_neighbours(state, opp) as i32;
        // Each neighbour ≈ 1/6 of a mate threat. Weight 10 so scores have headroom.
        (theirs - own) * 10
    }
}

impl Eval for CurrentEval {
    fn evaluate(state: &State) -> i32 {
        // Identical to LegacyEval for now — this is the baseline the gauntlet
        // must first show itself unable to distinguish. It diverges in the
        // evaluation PRs that follow.
        LegacyEval::evaluate(state)
    }
}

/// Count of occupied cells adjacent to `color`'s queen.
///
/// A queen still in hand counts 0 — safe, but no threats generated yet. A
/// covered queen (a beetle on top of it) still counts its own neighbours; the
/// cover itself is a separate fact and not scored here.
pub(crate) fn queen_neighbours(state: &State, color: Color) -> u8 {
    let q = queen_of(color);
    let coord = match state.piece_slot(q) {
        PieceSlot::OnBoard { coord, .. } | PieceSlot::Covered { coord, .. } => coord,
        PieceSlot::InHand => return 0,
    };
    let board = state.board();
    let mut n = 0u8;
    for nbr in coord.neighbours() {
        if board.is_occupied(nbr) {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::Coord;
    use crate::moves::Move;
    use crate::piece::PieceId;

    #[test]
    fn evaluation_is_symmetric_at_initial_state() {
        // Empty board, both queens in hand ⇒ eval is 0.
        let s = State::new();
        assert_eq!(CurrentEval::evaluate(&s), 0);
    }

    #[test]
    fn evaluation_rewards_attacking_opponent_queen() {
        // Build a position where each side has one piece next to the other's
        // queen. Symmetric threats ⇒ eval is 0.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });
        s.apply(Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(2, 0) });
        assert_eq!(CurrentEval::evaluate(&s), 0);
    }

    #[test]
    fn queen_in_hand_has_no_neighbours() {
        let s = State::new();
        assert_eq!(queen_neighbours(&s, Color::White), 0);
        assert_eq!(queen_neighbours(&s, Color::Black), 0);
    }

    #[test]
    fn legacy_and_current_agree_before_divergence() {
        // Guards the gauntlet's control arm: while CurrentEval delegates to
        // LegacyEval, a Legacy-vs-Current run must be a true null experiment.
        // This test is expected to be deleted when they intentionally diverge.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });
        assert_eq!(CurrentEval::evaluate(&s), LegacyEval::evaluate(&s));
    }
}
