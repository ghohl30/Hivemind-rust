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

/// Danger to a queen, indexed by how many of its six neighbours are occupied.
///
/// Convex, because the danger is not linear: the sixth neighbour ends the game,
/// the first is barely a threat. A linear schedule — which is what the engine
/// shipped with — values two half-surrounded queens exactly as highly as one
/// nearly-finished one, so it spreads pressure and never closes anything out.
///
/// **Owner-agnostic on purpose, and this is a correction.** An earlier version
/// of this table split neighbours into enemy and own, weighting an own piece at
/// roughly a quarter of an enemy one, on the reasoning that you can usually move
/// your own piece away. Measured over 300 games it scored 48.5% +/- 2.9% against
/// the plain linear term — no better, and slightly worse. The likely reason is
/// that Hive's win condition counts *all six* neighbours regardless of who owns
/// them, so discounting your own pieces makes the evaluation less aligned with
/// the thing that actually ends the game: an opponent whose own pieces crowd
/// their queen really is losing, and that version could barely see it.
///
/// Scaled to stay near the old linear term's magnitude at low counts so this
/// changes the shape of the evaluation without changing its units.
///
/// Index 6 is unreachable — `negamax` checks for a terminal position before
/// calling `evaluate` — but is filled defensively, and must stay well below
/// `MATE_THRESHOLD` so a heuristic score is never read as a forced mate.
const QUEEN_ADJ: [i32; 7] = [0, 6, 15, 30, 56, 96, 160];

const ENEMY_COVER: i32 = 90;

/// Danger from your *own* beetle sitting on your queen.
///
/// Still bad — the queen cannot move — but you control when it steps off, so
/// it is a self-inflicted tempo problem rather than a hostage situation.
const OWN_COVER: i32 = 35;

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
    QUEEN_ADJ[(enemy + own) as usize] + cover_danger(state, color)
}

/// Danger to `color`'s queen from being buried under a beetle.
///
/// This is a boolean term, which is the shape that wrecked the opponent-mobility
/// experiment on the earlier eval branch (a 4x node-count blowup). It is
/// tolerable here for two reasons the mobility term could not claim: it flips
/// only on a deliberate, rare climb onto one specific stack rather than as a
/// side effect of an unrelated game-phase condition, and its magnitude is in
/// scale with the ring tables rather than orders of magnitude larger. Node count
/// is checked on every change regardless.
fn cover_danger(state: &State, color: Color) -> i32 {
    let q = queen_of(color);
    // `Covered` is precisely "something is stacked on this piece" — no board
    // lookup needed to know that much.
    let coord = match state.piece_slot(q) {
        PieceSlot::Covered { coord, .. } => coord,
        _ => return 0,
    };
    match state.board().top_at(coord) {
        Some(top) if top.piece.color() == color => OWN_COVER,
        Some(_) => ENEMY_COVER,
        // Unreachable: a Covered queen has something above it by definition.
        None => 0,
    }
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
        let deltas: Vec<i32> = QUEEN_ADJ.windows(2).map(|w| w[1] - w[0]).collect();
        for pair in deltas.windows(2) {
            assert!(
                pair[1] > pair[0],
                "QUEEN_ADJ must be convex, got deltas {deltas:?}"
            );
        }
    }

    /// Apply a move, asserting it is legal first.
    ///
    /// `State::apply` only debug-asserts legality, so a release-mode test can
    /// happily build an impossible position and assert things about it. This
    /// makes the setup fail loudly in both profiles.
    fn play(s: &mut State, m: Move) {
        assert!(
            s.legal_moves().contains(&m),
            "test setup played an illegal move {m:?}"
        );
        s.apply(m);
    }

    /// Play the first legal move, for plies where the test does not care.
    fn play_any(s: &mut State) {
        let m = s.legal_moves()[0];
        s.apply(m);
    }

    #[test]
    fn beetle_cover_is_detected_and_owner_aware() {
        let mut s = State::new();
        play(&mut s, Move::Place { piece: PieceId(0), to: Coord::ORIGIN });      // WQ
        play(&mut s, Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });  // BQ
        play(&mut s, Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });  // W beetle
        play(&mut s, Move::Place { piece: PieceId(12), to: Coord::new(2, 0) });  // B beetle

        assert_eq!(cover_danger(&s, Color::White), 0, "nothing on the white queen yet");

        // White's beetle climbs onto its own queen: immobilised, but by choice.
        play(&mut s, Move::Slide { piece: PieceId(1), to: Coord::ORIGIN });
        assert_eq!(
            cover_danger(&s, Color::White),
            OWN_COVER,
            "own beetle on own queen is the milder penalty"
        );

        play_any(&mut s); // black, don't care

        // That same beetle steps across onto the black queen: now it is the
        // hostage case, and must score strictly worse.
        play(&mut s, Move::Slide { piece: PieceId(1), to: Coord::new(1, 0) });
        assert_eq!(
            cover_danger(&s, Color::Black),
            ENEMY_COVER,
            "enemy beetle on a queen is the severe penalty"
        );
        assert_eq!(
            cover_danger(&s, Color::White),
            0,
            "white's queen is uncovered again once the beetle leaves"
        );
    }

    #[test]
    fn enemy_cover_outweighs_own_cover() {
        // A hostage queen must always be worse than a self-blocked one,
        // otherwise the engine would volunteer to bury its own queen.
        assert!(ENEMY_COVER > OWN_COVER);
    }

    #[test]
    fn cover_is_priced_between_the_fourth_and_fifth_attacker() {
        // Anchors the weight against the ring schedule rather than leaving it a
        // free-floating magic number.
        assert!(ENEMY_COVER > QUEEN_ADJ[4] && ENEMY_COVER < QUEEN_ADJ[5]);
    }

    #[test]
    fn uncovered_queen_has_no_cover_penalty() {
        let s = State::new();
        assert_eq!(cover_danger(&s, Color::White), 0);
        assert_eq!(cover_danger(&s, Color::Black), 0);
    }

    #[test]
    fn ring_counts_every_neighbour_equally() {
        // Regression guard on a measured result: an owner-weighted version of
        // this table scored 48.5% over 300 games against the plain linear term.
        // Hive's win condition counts all six neighbours whoever owns them, so
        // the danger must depend only on how many cells are filled.
        let mut a = State::new();
        play(&mut a, Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        play(&mut a, Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });

        let (e, o) = queen_ring(&a, Color::White);
        assert_eq!(
            QUEEN_ADJ[(e + o) as usize],
            QUEEN_ADJ[1],
            "one neighbour is one neighbour, regardless of colour"
        );
    }

    #[test]
    fn heuristic_scores_stay_below_mate() {
        // A heuristic score reaching MATE_THRESHOLD would be read as a forced
        // win and would cut iterative deepening short.
        let worst = QUEEN_ADJ[6] + ENEMY_COVER;
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

        let d1 = QUEEN_ADJ[1];
        let d2 = QUEEN_ADJ[2];
        let d3 = QUEEN_ADJ[3];
        assert!(d2 - d1 < d3 - d2, "pressure must accelerate, not just accumulate");
    }
}
