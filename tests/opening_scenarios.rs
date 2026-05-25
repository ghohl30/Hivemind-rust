//! Scripted opening-phase scenarios. Regression anchors for Phase 2+ refactors.

use hive_engine::piece::{BLACK_QUEEN, WHITE_QUEEN};
use hive_engine::{Color, Coord, Move, PieceId, PieceSlot, State};

fn place(s: &mut State, piece: PieceId, to: Coord) {
    let m = Move::Place { piece, to };
    let legal = s.legal_moves();
    assert!(
        legal.iter().any(|x| *x == m),
        "Place {{ piece: {piece:?}, to: {to:?} }} not in legal moves: {legal:?}",
    );
    s.apply(m);
}


#[test]
fn first_two_placements_are_special() {
    let mut s = State::new();
    // Only origin placements are legal first.
    for m in s.legal_moves() {
        match m {
            Move::Place { to, .. } => assert_eq!(to, Coord::ORIGIN),
            _ => panic!("first move should only be Place"),
        }
    }
    // Place white spider at origin.
    place(&mut s, PieceId(6), Coord::ORIGIN);
    // Black's first placement must be adjacent to origin.
    for m in s.legal_moves() {
        match m {
            Move::Place { to, .. } => {
                let neighbours = Coord::ORIGIN.neighbours();
                assert!(neighbours.contains(&to), "black first placement {to:?} not adjacent to origin");
            }
            _ => panic!("black's first move should only be Place"),
        }
    }
}

#[test]
fn movements_blocked_until_queen_placed() {
    let mut s = State::new();
    place(&mut s, PieceId(6), Coord::ORIGIN); // white spider
    place(&mut s, PieceId(17), Coord::new(1, 0)); // black spider (id 11+6=17)
    place(&mut s, PieceId(7), Coord::new(-1, 0)); // white 2nd spider
    place(&mut s, PieceId(18), Coord::new(2, 0)); // black 2nd spider

    // Now turn 2 for each player. No Slide moves should appear yet — queens still in hand.
    let legal = s.legal_moves();
    assert!(legal.iter().all(|m| !matches!(m, Move::Slide { .. })));
}

#[test]
fn queen_must_be_placed_on_fourth_turn() {
    let mut s = State::new();
    // Turn 1: each places a non-queen.
    place(&mut s, PieceId(6), Coord::ORIGIN);
    place(&mut s, PieceId(17), Coord::new(1, 0));
    // Turn 2: each places a non-queen.
    place(&mut s, PieceId(7), Coord::new(-1, 0));
    place(&mut s, PieceId(18), Coord::new(2, 0));
    // Turn 3: each places a non-queen.
    place(&mut s, PieceId(8), Coord::new(-2, 0)); // white ant
    place(&mut s, PieceId(19), Coord::new(3, 0)); // black ant

    // White is now on turn 4 with queen still in hand. Only Place(queen, ...) legal.
    assert_eq!(s.turn_for(Color::White), 4);
    assert!(matches!(s.piece_slot(WHITE_QUEEN), PieceSlot::InHand));
    for m in s.legal_moves() {
        match m {
            Move::Place { piece, .. } => assert_eq!(piece, WHITE_QUEEN),
            Move::Pass => {} // permitted if no legal queen spot, though unlikely here
            Move::Slide { .. } => panic!("Slide on 4th turn with queen in hand: {m:?}"),
        }
    }
}

#[test]
fn pinned_middle_cannot_move() {
    // Line of three white pieces: middle is articulation.
    // (We can't easily build a legal mid-game position; instead, drive a short game
    //  where after some placements the middle piece is genuinely an articulation,
    //  and verify it does not appear in legal Slide moves.)
    let mut s = State::new();
    // Turn 1
    place(&mut s, WHITE_QUEEN, Coord::ORIGIN);
    place(&mut s, BLACK_QUEEN, Coord::new(1, 0));
    // Turn 2
    place(&mut s, PieceId(6), Coord::new(-1, 0)); // white spider attached to white queen
    place(&mut s, PieceId(17), Coord::new(2, 0)); // black spider attached to black queen
    // White queen is the only ground connection between (-1,0) and (1,0)+black-side, so removing
    // it would disconnect the hive. White queen should NOT have any Slide moves.
    let legal = s.legal_moves();
    let white_queen_slides: Vec<_> = legal
        .iter()
        .filter(|m| matches!(m, Move::Slide { piece, .. } if *piece == WHITE_QUEEN))
        .collect();
    assert!(
        white_queen_slides.is_empty(),
        "pinned white queen should not be movable; got {white_queen_slides:?}",
    );
}

#[test]
fn beetle_climbs_and_pins() {
    let mut s = State::new();
    // Build: W-Queen at origin, B-Queen at (1,0), W-Beetle at (-1,0), B-Beetle at (2,0).
    place(&mut s, WHITE_QUEEN, Coord::ORIGIN);
    place(&mut s, BLACK_QUEEN, Coord::new(1, 0));
    place(&mut s, PieceId(1), Coord::new(-1, 0)); // W beetle
    place(&mut s, PieceId(12), Coord::new(2, 0)); // B beetle
    // Turn 3 for both — neither has played turn 3 yet. Each gets to move now.
    // Verify the W beetle has a climb option onto the white queen.
    let legal = s.legal_moves();
    let climb = Move::Slide {
        piece: PieceId(1),
        to: Coord::ORIGIN,
    };
    assert!(legal.contains(&climb), "white beetle should be able to climb onto its queen: {legal:?}");
    s.apply(climb);
    // After the climb, white queen is Covered, beetle is OnBoard at origin with height 1.
    assert!(matches!(
        s.piece_slot(WHITE_QUEEN),
        PieceSlot::Covered { coord, stack_height: 0 } if coord == Coord::ORIGIN
    ));
    assert!(matches!(
        s.piece_slot(PieceId(1)),
        PieceSlot::OnBoard { coord, stack_height: 1 } if coord == Coord::ORIGIN
    ));
}

#[test]
fn grasshopper_jumps_over_a_chain() {
    let mut s = State::new();
    // Build a short chain so a grasshopper can demonstrate a jump.
    place(&mut s, WHITE_QUEEN, Coord::ORIGIN);
    place(&mut s, BLACK_QUEEN, Coord::new(1, 0));
    // Turn 2: place white grasshopper at (-1, 0), black at (2, 0).
    place(&mut s, PieceId(3), Coord::new(-1, 0)); // white grasshopper 1
    place(&mut s, PieceId(14), Coord::new(2, 0)); // black grasshopper 1 (11 + 3 = 14)
    // After both queens are on the board, movements unlock. White's turn 3.
    // The white grasshopper at (-1, 0) can jump East over WQ@(0,0), BQ@(1,0),
    // BG@(2,0) and land at (3, 0).
    let legal = s.legal_moves();
    let jump = Move::Slide {
        piece: PieceId(3),
        to: Coord::new(3, 0),
    };
    assert!(
        legal.contains(&jump),
        "expected grasshopper jump to (3,0) in {legal:?}",
    );
}
