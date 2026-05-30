//! Property-based invariants on the engine.
//!
//! Strategy: drive `State` through short random *legal* play sequences. After
//! each step, assert the invariants from the Phase 1 plan + the Phase 2
//! Zobrist invariant + the Phase 3 round-trip invariant.

use std::collections::HashMap;

use hive_engine::piece::{Color, PieceType};
use hive_engine::rules::{perimeter_from_scratch, placement_legality_from_scratch};
use hive_engine::{zobrist, Move, PieceId, PieceSlot, State};
use proptest::prelude::*;

/// (1) The on-board top map is exactly reconstructible from the piece array.
fn assert_board_pieces_coherent(s: &State) {
    let rebuilt = s.board_from_pieces();
    prop_assert_eq_helper(&rebuilt, s.board());
}

fn prop_assert_eq_helper<T: std::fmt::Debug + PartialEq>(a: &T, b: &T) {
    assert_eq!(a, b, "board_from_pieces vs board");
}

/// (2) Inventory: starting tables hold 1Q, 2B, 3G, 2S, 3A per color, AND
/// `placements_so_far` equals the number of on-board pieces (placed pieces
/// never return to hand in Hive).
fn assert_inventory(s: &State) {
    let mut counts: HashMap<(Color, PieceType), usize> = HashMap::new();
    for pid in PieceId::all() {
        *counts.entry((pid.color(), pid.piece_type())).or_insert(0) += 1;
    }
    for color in [Color::White, Color::Black] {
        assert_eq!(counts.get(&(color, PieceType::QueenBee)).copied().unwrap_or(0), 1);
        assert_eq!(counts.get(&(color, PieceType::Beetle)).copied().unwrap_or(0), 2);
        assert_eq!(counts.get(&(color, PieceType::Grasshopper)).copied().unwrap_or(0), 3);
        assert_eq!(counts.get(&(color, PieceType::Spider)).copied().unwrap_or(0), 2);
        assert_eq!(counts.get(&(color, PieceType::SoldierAnt)).copied().unwrap_or(0), 3);
    }
    let on_board = PieceId::all()
        .filter(|pid| !matches!(s.piece_slot(*pid), PieceSlot::InHand))
        .count();
    assert_eq!(
        on_board,
        s.placements_so_far(),
        "placements_so_far drift: {on_board} on-board vs {} counter",
        s.placements_so_far()
    );
}

/// (3) The set of occupied coords is 6-connected (or empty).
fn assert_one_hive(s: &State) {
    use hive_engine::rules::connected;
    let occ: std::collections::HashSet<_> = s.board().occupied_coords().collect();
    assert!(connected(&occ), "hive not connected: {occ:?}");
}

/// (4) `legal_moves` is empty iff `is_terminal` is Some.
fn assert_terminal_consistency(s: &State) {
    let term = s.is_terminal().is_some();
    let moves_empty = s.legal_moves().is_empty();
    assert_eq!(
        term, moves_empty,
        "terminal/moves mismatch: terminal={term}, moves_empty={moves_empty}"
    );
    if !term {
        // Pass must appear iff there are no other moves. We don't enforce that
        // here directly (the engine inserts Pass only when other generators
        // produced nothing) — but we DO check that non-terminal states have at
        // least one move.
        assert!(!moves_empty);
    }
}

/// (5) `apply` is deterministic.
fn assert_apply_deterministic(s: &State, m: Move) {
    let mut a = s.clone();
    let mut b = s.clone();
    a.apply(m);
    b.apply(m);
    assert_eq!(a, b, "non-deterministic apply for {m:?}");
}

/// (6) `Clone` is a true copy.
fn assert_clone_independence(s: &State, m: Move) {
    let mut c = s.clone();
    let original = s.clone();
    c.apply(m);
    assert_eq!(*s, original, "clone+apply mutated original");
}

/// (7) Queen-by-turn-4: if the side-to-move's queen is still in hand at their
/// 4th turn, every legal move must be a Place of that queen.
fn assert_queen_by_turn_4(s: &State) {
    use hive_engine::piece::queen_of;
    let color = s.side_to_move();
    let queen = queen_of(color);
    if s.turn_for(color) == 4 && matches!(s.piece_slot(queen), PieceSlot::InHand) {
        for m in s.legal_moves() {
            match m {
                Move::Place { piece, .. } => {
                    assert_eq!(piece, queen, "non-queen placement on 4th turn with queen in hand");
                }
                Move::Pass => { /* allowed if no legal queen placement exists */ }
                Move::Slide { .. } => {
                    panic!("Slide emitted on 4th turn with queen in hand: {m:?}");
                }
            }
        }
    }
}

/// Phase 3 invariant: apply(m) followed by unapply() restores the exact
/// pre-state. This is the central guard for the make-unmake refactor and the
/// reason undo records exist. Must hold *bit-for-bit* on the full `State`
/// (including the Zobrist hash and the undo stack).
fn assert_round_trip(s: &mut State, m: hive_engine::Move) {
    let snap = s.clone();
    s.apply(m);
    s.unapply();
    assert_eq!(
        *s, snap,
        "round-trip apply/unapply diverged for {m:?}",
    );
}

/// Phase 4 invariant: the incrementally-maintained `perimeter` and
/// `placement_legality_*` caches equal their from-scratch builds. Both sides
/// of the comparison are sorted slices of Coord (Phase 5 changed the caches
/// from HashSet<Coord> to a sorted CoordSet).
fn assert_caches_match_from_scratch(s: &State) {
    let perim_built = perimeter_from_scratch(s.board());
    assert_eq!(
        s.perimeter(),
        perim_built.as_slice(),
        "perimeter cache drift: incremental={:?}, from_scratch={:?}",
        s.perimeter(),
        perim_built,
    );
    for color in [Color::White, Color::Black] {
        let built = placement_legality_from_scratch(s.board(), color);
        assert_eq!(
            s.placement_legality(color),
            built.as_slice(),
            "placement_legality({color:?}) drift: incremental={:?}, from_scratch={:?}",
            s.placement_legality(color),
            built,
        );
    }
}

/// Phase 2 invariant: the incrementally-maintained Zobrist hash equals the
/// from-scratch computation. The hash is the same safety net for Phase 3's
/// make-unmake refactor that the round-trip property test will be for piece
/// state.
fn assert_zobrist_matches_from_scratch(s: &State) {
    let computed = zobrist::from_scratch(s);
    assert_eq!(
        s.zobrist(),
        computed,
        "zobrist drift: incremental={:#x} from_scratch={:#x}",
        s.zobrist(),
        computed
    );
}

/// (8, 9) `legal_moves` and `is_terminal` are pure — calling twice yields
/// equal results (modulo SmallVec ordering for legal_moves).
fn assert_purity(s: &State) {
    let mut a = s.legal_moves();
    let mut b = s.legal_moves();
    a.sort();
    b.sort();
    assert_eq!(a, b, "legal_moves not pure");
    assert_eq!(s.is_terminal(), s.is_terminal(), "is_terminal not pure");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn legal_play_preserves_invariants(seq in proptest::collection::vec(0u32..1024, 0..40)) {
        let mut s = State::new();
        // Invariants on the fresh state.
        assert_board_pieces_coherent(&s);
        assert_inventory(&s);
        assert_one_hive(&s);
        assert_terminal_consistency(&s);
        assert_purity(&s);
        assert_zobrist_matches_from_scratch(&s);
        assert_caches_match_from_scratch(&s);

        for step in seq {
            if s.is_terminal().is_some() {
                break;
            }
            let moves = s.legal_moves();
            prop_assert!(!moves.is_empty(), "non-terminal state with no moves");

            let pick = (step as usize) % moves.len();
            let m = moves[pick];

            assert_apply_deterministic(&s, m);
            assert_clone_independence(&s, m);
            assert_queen_by_turn_4(&s);
            // Phase 3: round-trip check, run on every step. This is what the
            // brief calls out as the guard for the make-unmake refactor.
            assert_round_trip(&mut s, m);

            s.apply(m);

            assert_board_pieces_coherent(&s);
            assert_inventory(&s);
            assert_one_hive(&s);
            assert_terminal_consistency(&s);
            assert_purity(&s);
            assert_zobrist_matches_from_scratch(&s);
            assert_caches_match_from_scratch(&s);
        }
    }
}
