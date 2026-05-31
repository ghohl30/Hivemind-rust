//! Fixed demo position for the static render PR. Builds a real [`Session`] by
//! selecting legal moves from the engine, so the board, a beetle stack, hands,
//! and the turn indicator all show genuine content. Pure (no DOM), and unit
//! tested for the properties the render relies on (≥1 stack, both colors on the
//! board, hands depleted).
//!
//! Selection is greedy-by-target rather than a hard-coded `Move` list: a
//! hard-coded list would silently break if the engine's move encoding or
//! opening legality shifted, whereas asking the engine for legal moves and
//! picking one that matches a goal stays correct against the public API.

use hive_engine::{Move, PieceId, PieceType};

use crate::game::Session;

/// Build the demo session: a handful of opening placements for both colors
/// plus a beetle climb so a stack badge is exercised. Falls back gracefully —
/// every step only applies a move the engine reports as legal, so the result is
/// always a valid position even if a preferred move isn't available.
pub fn demo_session() -> Session {
    let mut s = Session::new();

    // Phase 1: place pieces for both colors to grow a small hive. Prefer
    // placing a beetle and a queen early (so the climb and the queen-hint logic
    // have something to show), then fill with whatever is legal.
    let placement_prefs = [
        PieceType::QueenBee,
        PieceType::Beetle,
        PieceType::SoldierAnt,
        PieceType::Spider,
        PieceType::Grasshopper,
    ];
    for _ in 0..6 {
        if !place_preferred(&mut s, &placement_prefs) {
            break;
        }
    }

    // Phase 2: try to make a beetle climb onto an adjacent occupied cell so a
    // stack (height > 1) exists for the count badge.
    let _ = try_beetle_climb(&mut s);

    s
}

/// Apply the first legal `Place` whose piece type matches the earliest entry in
/// `prefs` that has a legal placement right now. Returns whether a move was
/// applied.
fn place_preferred(s: &mut Session, prefs: &[PieceType]) -> bool {
    let legal = s.state().legal_moves();
    for &want in prefs {
        if let Some(&m) = legal.iter().find(|m| match m {
            Move::Place { piece, .. } => piece.piece_type() == want,
            _ => false,
        }) {
            s.push_move(m).expect("selected from legal_moves");
            return true;
        }
    }
    // No preferred type placeable; take any legal placement.
    if let Some(&m) = legal.iter().find(|m| matches!(m, Move::Place { .. })) {
        s.push_move(m).expect("selected from legal_moves");
        return true;
    }
    false
}

/// Attempt a beetle `Slide` (the engine encodes climbs as `Slide`) that lands
/// on an already-occupied cell, producing a height-2 stack. Returns whether a
/// climb was applied.
fn try_beetle_climb(s: &mut Session) -> bool {
    // Look a few plies ahead opportunistically: on the side-to-move's turn,
    // find a beetle slide onto an occupied cell.
    for _ in 0..4 {
        let state = s.state();
        let occupied: Vec<_> = state.entries().map(|(c, _)| c).collect();
        let legal = state.legal_moves();
        let climb = legal.iter().copied().find(|m| match m {
            Move::Slide { piece, to } => {
                piece.piece_type() == PieceType::Beetle && occupied.contains(to)
            }
            _ => false,
        });
        if let Some(m) = climb {
            s.push_move(m).expect("selected from legal_moves");
            return true;
        }
        // Otherwise advance with any legal non-pass move to change whose turn it
        // is and open up a climb next ply. Bail if only Pass is available.
        let advance = legal
            .iter()
            .copied()
            .find(|m| !matches!(m, Move::Pass));
        match advance {
            Some(m) => s.push_move(m).expect("selected from legal_moves"),
            None => return false,
        };
    }
    false
}

/// Silence an otherwise-unused import in builds that don't reference `PieceId`
/// directly; kept because tests below use it.
#[allow(dead_code)]
fn _uses_piece_id(p: PieceId) -> u8 {
    p.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::view::{board_tiles, hand_entries};
    use hive_engine::Color;

    #[test]
    fn demo_has_pieces_on_the_board() {
        let s = demo_session();
        let tiles = board_tiles(s.state());
        assert!(tiles.len() >= 4, "expected several tiles, got {}", tiles.len());
    }

    #[test]
    fn demo_has_both_colors_on_the_board() {
        let s = demo_session();
        let tiles = board_tiles(s.state());
        assert!(tiles.iter().any(|t| t.color() == Color::White));
        assert!(tiles.iter().any(|t| t.color() == Color::Black));
    }

    #[test]
    fn demo_has_a_beetle_stack() {
        let s = demo_session();
        let tiles = board_tiles(s.state());
        assert!(
            tiles.iter().any(|t| t.is_stack()),
            "demo should exercise the stack badge with a height>1 cell"
        );
    }

    #[test]
    fn demo_depletes_hands_partially() {
        let s = demo_session();
        // Each color started with 11; the demo placed some, so both hands
        // should be below 11.
        let w: usize = hand_entries(s.state(), Color::White)
            .iter()
            .map(|e| e.count)
            .sum();
        let b: usize = hand_entries(s.state(), Color::Black)
            .iter()
            .map(|e| e.count)
            .sum();
        assert!(w < 11 && b < 11, "white={w} black={b}");
    }

    #[test]
    fn demo_session_is_internally_consistent() {
        // Reconstructing from the recorded move list reproduces the same state.
        let s = demo_session();
        let rebuilt = Session::from_moves(s.moves()).expect("demo moves are legal");
        assert_eq!(rebuilt.state(), s.state());
    }
}
