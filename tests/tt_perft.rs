//! Phase 2 deliverable: a simple search using the Zobrist-keyed TT shows
//! non-zero hit count, and matches plain perft for the same depth.

use hive_engine::perft::{perft, perft_with_tt};
use hive_engine::{Coord, Move, PieceId, State};

#[test]
fn tt_perft_matches_plain_perft_depth_2() {
    let s = State::new();
    let plain = perft(&s, 2);
    let stats = perft_with_tt(&s, 2);
    assert_eq!(stats.count, plain);
}

#[test]
fn interchangeable_beetles_collide_in_tt() {
    // Phase 2 hash key uses PieceType, not PieceId, so placing either of the
    // two white beetles at ORIGIN produces the same Zobrist hash. The first
    // such placement we explore stores into the TT; the second is a hit.
    let s = State::new();
    let stats = perft_with_tt(&s, 2);
    assert!(stats.tt_hits > 0, "expected TT hits, got {:?}", stats);
}

#[test]
fn placement_order_transposition_collides() {
    // White places W-Spider1 then advances to a state with one piece at
    // ORIGIN. White places W-Spider2 at ORIGIN (later turn would be different
    // game state) — but here we just verify that after the very first move,
    // a board with W-Spider1 at ORIGIN and a board with W-Spider2 at ORIGIN
    // share the same Zobrist hash. (Two spiders of the same color are
    // physically interchangeable for hash purposes.)
    let mut a = State::new();
    a.apply(Move::Place {
        piece: PieceId(6), // W-Spider1
        to: Coord::ORIGIN,
    });
    let mut b = State::new();
    b.apply(Move::Place {
        piece: PieceId(7), // W-Spider2
        to: Coord::ORIGIN,
    });
    assert_eq!(
        a.zobrist(),
        b.zobrist(),
        "interchangeable spiders should hash identically; got {:#x} vs {:#x}",
        a.zobrist(),
        b.zobrist(),
    );
}
