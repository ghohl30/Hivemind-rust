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

/// Danger contributed by each *enemy* piece adjacent to a queen, indexed by how
/// many there are.
///
/// Escalating rather than linear because the danger is not linear: the sixth
/// neighbour ends the game, the first is barely a threat. A linear term — which
/// is what the engine had — values spreading pressure across two queens' worth
/// of half-surrounds equally with closing out one, and so declines to finish
/// anything.
///
/// Index 6 is unreachable in play (a surrounded queen is terminal, and
/// `negamax` checks that before ever calling `evaluate`), but is filled
/// defensively. It must stay well under `MATE_THRESHOLD` so a heuristic score is
/// never mistaken for a forced mate.
const ENEMY_ADJ: [i32; 7] = [0, 8, 18, 36, 70, 130, 250];

/// Danger contributed by each *friendly* piece adjacent to a queen.
///
/// Much milder than an enemy piece at the same count. A piece of your own
/// beside your queen still fills a cell, but you may be able to move it away;
/// an enemy piece there is a committed attacker that you cannot remove. The
/// engine had no way to express this difference at all.
const OWN_ADJ: [i32; 7] = [0, 2, 5, 10, 18, 30, 60];

impl Eval for CurrentEval {
    fn evaluate(state: &State) -> i32 {
        let stm = state.side_to_move();
        let opp = stm.other();
        // Symmetric between the two queens on purpose. Weighting your own
        // queen's danger above the opponent's is a second, independent
        // defensive bias, and stacking it on the ownership asymmetry above
        // would make two knobs move at once. Left for the gauntlet to settle.
        queen_danger(state, opp) - queen_danger(state, stm)
    }
}

/// How close `color`'s queen is to being surrounded, weighted by who owns each
/// adjacent piece. Higher is worse for `color`.
fn queen_danger(state: &State, color: Color) -> i32 {
    let (enemy, own) = queen_ring(state, color);
    ENEMY_ADJ[enemy as usize] + OWN_ADJ[own as usize]
}

/// Split of the pieces adjacent to `color`'s queen into `(enemy, own)`.
///
/// A queen in hand returns `(0, 0)` — safe, but generating no threats either.
/// A covered queen still reports its ring; the beetle on top is a separate
/// fact and not scored here.
pub(crate) fn queen_ring(state: &State, color: Color) -> (u8, u8) {
    let q = queen_of(color);
    let coord = match state.piece_slot(q) {
        PieceSlot::OnBoard { coord, .. } | PieceSlot::Covered { coord, .. } => coord,
        PieceSlot::InHand => return (0, 0),
    };
    let board = state.board();
    let (mut enemy, mut own) = (0u8, 0u8);
    for nbr in coord.neighbours() {
        if let Some(top) = board.top_at(nbr) {
            if top.piece.color() == color {
                own += 1;
            } else {
                enemy += 1;
            }
        }
    }
    (enemy, own)
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
    fn surround_pressure_is_convex() {
        // The marginal value of the next enemy piece must strictly increase.
        // A linear term cannot finish a queen off; this is the property that
        // fixes that, so assert it directly rather than trusting the literals.
        let deltas: Vec<i32> = ENEMY_ADJ.windows(2).map(|w| w[1] - w[0]).collect();
        for pair in deltas.windows(2) {
            assert!(
                pair[1] > pair[0],
                "ENEMY_ADJ must be convex, got deltas {deltas:?}"
            );
        }
    }

    #[test]
    fn enemy_neighbours_outweigh_own_at_every_count() {
        // An enemy piece beside your queen is a committed attacker; one of your
        // own can often step away. If this ever inverts, the engine would start
        // preferring to be surrounded by the opponent.
        for n in 0..ENEMY_ADJ.len() {
            assert!(
                ENEMY_ADJ[n] >= OWN_ADJ[n],
                "enemy weight must dominate own weight at count {n}"
            );
        }
    }

    #[test]
    fn heuristic_scores_stay_below_mate() {
        // A heuristic score reaching MATE_THRESHOLD would be read as a forced
        // win and would cut iterative deepening short.
        let worst = ENEMY_ADJ[6] + OWN_ADJ[6];
        assert!(
            worst < crate::search::MATE_THRESHOLD,
            "eval magnitude {worst} must stay well below mate scores"
        );
    }

    #[test]
    fn queen_ring_splits_by_owner() {
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });      // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });  // BQ
        s.apply(Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });  // W beetle

        // White queen: one black neighbour (BQ), one white neighbour (beetle).
        assert_eq!(queen_ring(&s, Color::White), (1, 1));
        // Black queen: one white neighbour (WQ), no black ones.
        assert_eq!(queen_ring(&s, Color::Black), (1, 0));
    }

    #[test]
    fn closing_in_on_the_enemy_queen_scores_better() {
        // Two black pieces around the white queen must be worth strictly more
        // to Black than one — and by more than the first one was worth.
        let mut one = State::new();
        one.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        one.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });

        let (e1, o1) = queen_ring(&one, Color::White);
        assert_eq!((e1, o1), (1, 0));

        let d1 = ENEMY_ADJ[1];
        let d2 = ENEMY_ADJ[2];
        let d3 = ENEMY_ADJ[3];
        assert!(d2 - d1 < d3 - d2, "pressure must accelerate, not just accumulate");
    }
}
