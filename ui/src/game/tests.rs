//! Native tests for the game core. Every position is driven through the real
//! engine — no hand-rolled board state.

use super::*;
use hive_engine::{Color, Move, PieceId, State};

/// Play `n` legal plies into a fresh session by always taking the engine's first
/// reported legal move. Returns the session. Panics if a position runs out of
/// moves before `n` (terminal) — callers pick small `n`.
fn play_first_legal(n: usize) -> Session {
    let mut s = Session::new();
    for _ in 0..n {
        let m = s.state().legal_moves()[0];
        s.push_move(m).expect("engine-reported move must be legal");
    }
    s
}

// ---- session: replay / derivation correctness -----------------------------

#[test]
fn new_session_matches_initial_state() {
    let s = Session::new();
    assert_eq!(s.ply(), 0);
    assert!(s.moves().is_empty());
    assert_eq!(*s.state(), State::new());
    assert!(!s.is_over());
}

#[test]
fn derived_state_equals_independent_replay() {
    let s = play_first_legal(6);
    // The cached state must equal a from-scratch fold over the move list, and
    // also equal the free `replay` helper (the from_moves workaround seam).
    let mut independent = State::new();
    for &m in s.moves() {
        independent.apply(m);
    }
    assert_eq!(*s.state(), independent);
    assert_eq!(*s.state(), replay(s.moves()));
}

#[test]
fn from_moves_reconstructs_and_validates() {
    let played = play_first_legal(5);
    let rebuilt = Session::from_moves(played.moves()).expect("recorded moves are legal");
    assert_eq!(rebuilt.moves(), played.moves());
    assert_eq!(*rebuilt.state(), *played.state());
}

#[test]
fn from_moves_rejects_illegal_sequence() {
    // A slide of a piece that is not even on the board is never legal as move 1.
    let bogus = vec![Move::Slide {
        piece: PieceId(0),
        to: hive_engine::Coord::new(0, 0),
    }];
    let err = Session::from_moves(&bogus).unwrap_err();
    assert_eq!(err.attempted, bogus[0]);
}

#[test]
fn undo_restores_previous_position() {
    let mut s = play_first_legal(4);
    let before_moves = s.moves().to_vec();
    let snapshot = s.state().clone();

    s.push_move(s.state().legal_moves()[0]).unwrap();
    let popped = s.undo().expect("a move was just pushed");

    assert_eq!(s.moves(), before_moves.as_slice());
    assert_eq!(*s.state(), snapshot);
    // The popped move is the one we just pushed.
    assert!(matches!(popped, Move::Place { .. } | Move::Slide { .. }));
}

#[test]
fn undo_on_empty_session_is_none() {
    let mut s = Session::new();
    assert_eq!(s.undo(), None);
}

// ---- session: illegal-move rejection at push boundary ----------------------

#[test]
fn push_illegal_move_is_rejected_without_mutation() {
    let mut s = Session::new();
    let snapshot = s.state().clone();

    // Sliding the white queen with nothing on the board is not in legal_moves.
    let illegal = Move::Slide {
        piece: PieceId(0),
        to: hive_engine::Coord::new(1, 0),
    };
    let err = s.push_move(illegal).unwrap_err();

    assert_eq!(err.attempted, illegal);
    assert_eq!(s.ply(), 0);
    assert_eq!(*s.state(), snapshot, "rejected move must not mutate state");
}

#[test]
fn every_engine_reported_move_is_accepted() {
    let mut s = Session::new();
    for &m in s.state().legal_moves().clone().iter() {
        // Each candidate is legal in the *initial* position; apply to a clone.
        let mut probe = Session::new();
        assert!(probe.push_move(m).is_ok(), "engine move rejected: {m}");
    }
    // And the real session can keep playing.
    let first = s.state().legal_moves()[0];
    assert!(s.push_move(first).is_ok());
}

// ---- legal-move index ------------------------------------------------------

#[test]
fn index_groups_moves_by_piece_and_destination() {
    let s = play_first_legal(4);
    let idx = LegalMoveIndex::from_state(s.state());
    let legal: Vec<Move> = s.state().legal_moves().into_iter().collect();

    assert!(!idx.is_forced_pass());
    assert_eq!(idx.all().len(), legal.len());

    // Every non-pass legal move must be retrievable both by its piece and by
    // its destination.
    for &m in &legal {
        if let Move::Place { piece, to } | Move::Slide { piece, to } = m {
            assert!(
                idx.moves_for_piece(piece).contains(&m),
                "by_piece missing {m}"
            );
            assert!(idx.moves_to(to).contains(&m), "by_destination missing {m}");
        }
    }
}

#[test]
fn selecting_a_piece_yields_exactly_its_legal_destinations() {
    let s = play_first_legal(4);
    let idx = LegalMoveIndex::from_state(s.state());
    let legal: Vec<Move> = s.state().legal_moves().into_iter().collect();

    let piece = idx
        .movable_pieces()
        .next()
        .expect("a non-terminal position has a movable piece");

    // Expected destinations for that piece, computed directly from legal_moves.
    let mut expected: Vec<_> = legal
        .iter()
        .filter_map(|m| match m {
            Move::Place { piece: p, to } | Move::Slide { piece: p, to } if *p == piece => Some(*to),
            _ => None,
        })
        .collect();
    expected.sort();

    let mut got = idx.destinations_for_piece(piece);
    got.sort();

    assert!(!expected.is_empty());
    assert_eq!(got, expected);
}

#[test]
fn unmovable_piece_has_no_destinations() {
    let s = Session::new();
    let idx = LegalMoveIndex::from_state(s.state());
    // Black pieces cannot move on White's opening turn.
    let black = PieceId(11);
    assert!(idx.moves_for_piece(black).is_empty());
    assert!(idx.destinations_for_piece(black).is_empty());
}

#[test]
fn forced_pass_is_detected() {
    let only_pass = LegalMoveIndex::from_moves(vec![Move::Pass]);
    assert!(only_pass.is_forced_pass());
    assert!(only_pass.movable_pieces().next().is_none());

    // A real position with actual moves is not a forced pass.
    let normal = LegalMoveIndex::from_state(&State::new());
    assert!(!normal.is_forced_pass());

    // Pass alongside other moves is not "forced".
    let mixed = LegalMoveIndex::from_moves(vec![
        Move::Pass,
        Move::Place {
            piece: PieceId(0),
            to: hive_engine::Coord::new(0, 0),
        },
    ]);
    assert!(!mixed.is_forced_pass());
}

// ---- new-game config -------------------------------------------------------

#[test]
fn human_color_resolution() {
    assert_eq!(HumanColor::White.resolve(false), Color::White);
    assert_eq!(HumanColor::White.resolve(true), Color::White);
    assert_eq!(HumanColor::Black.resolve(false), Color::Black);
    assert_eq!(HumanColor::Black.resolve(true), Color::Black);
    // Random follows the coin: false -> White, true -> Black.
    assert_eq!(HumanColor::Random.resolve(false), Color::White);
    assert_eq!(HumanColor::Random.resolve(true), Color::Black);
}

#[test]
fn difficulty_depths_are_distinct_and_ordered() {
    assert_eq!(Difficulty::Easy.depth(), 2);
    assert_eq!(Difficulty::Medium.depth(), 4);
    assert_eq!(Difficulty::Hard.depth(), 6);
    assert!(Difficulty::Easy.depth() < Difficulty::Medium.depth());
    assert!(Difficulty::Medium.depth() < Difficulty::Hard.depth());
}

#[test]
fn config_resolve_assigns_opposite_ai_color() {
    let cfg = NewGameConfig {
        human: HumanColor::Black,
        difficulty: Difficulty::Hard,
    };
    let setup = cfg.resolve(false);
    assert_eq!(setup.human, Color::Black);
    assert_eq!(setup.ai, Color::White);
    assert_eq!(setup.ai_depth(), 6);
    // AI plays White, so it moves first.
    assert!(setup.ai_moves_first());
}

#[test]
fn config_resolve_human_white_means_ai_second() {
    let cfg = NewGameConfig {
        human: HumanColor::White,
        difficulty: Difficulty::Easy,
    };
    let setup = cfg.resolve(true); // coin ignored for non-random
    assert_eq!(setup.human, Color::White);
    assert_eq!(setup.ai, Color::Black);
    assert!(!setup.ai_moves_first());
}

#[test]
fn random_config_resolution_both_outcomes() {
    let cfg = NewGameConfig {
        human: HumanColor::Random,
        difficulty: Difficulty::Medium,
    };
    let white = cfg.resolve(false);
    assert_eq!(white.human, Color::White);
    assert_eq!(white.ai, Color::Black);

    let black = cfg.resolve(true);
    assert_eq!(black.human, Color::Black);
    assert_eq!(black.ai, Color::White);
}

#[test]
fn default_config_is_human_white_medium() {
    let cfg = NewGameConfig::default();
    assert_eq!(cfg.human, HumanColor::White);
    assert_eq!(cfg.difficulty, Difficulty::Medium);
}

// ---- worker message serde round-trip ---------------------------------------

#[test]
fn worker_request_round_trips_through_json() {
    let session = play_first_legal(5);
    let req = WorkerRequest::new(session.moves().to_vec(), Difficulty::Hard.depth(), 42);

    let json = serde_json::to_string(&req).expect("serialize request");
    let back: WorkerRequest = serde_json::from_str(&json).expect("deserialize request");

    assert_eq!(req, back);
    // The moves survived the trip and still rebuild the same position.
    assert_eq!(*Session::from_moves(&back.moves).unwrap().state(), *session.state());
}

#[test]
fn worker_response_round_trips_through_json() {
    let resp = WorkerResponse {
        request_id: 42,
        best_move: Some(Move::Place {
            piece: PieceId(2),
            to: hive_engine::Coord::new(-1, 1),
        }),
        score: -1234,
        stats: WorkerSearchStats {
            nodes: 9001,
            tt_hits: 12,
            tt_cutoffs: 3,
            beta_cutoffs: 7,
            tt_stores: 100,
        },
    };

    let json = serde_json::to_string(&resp).expect("serialize response");
    let back: WorkerResponse = serde_json::from_str(&json).expect("deserialize response");
    assert_eq!(resp, back);
}

#[test]
fn worker_response_handles_none_move() {
    let resp = WorkerResponse {
        request_id: 1,
        best_move: None,
        score: 0,
        stats: WorkerSearchStats::default(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: WorkerResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn config_types_serde_round_trip() {
    for human in [HumanColor::White, HumanColor::Black, HumanColor::Random] {
        for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
            let cfg = NewGameConfig { human, difficulty };
            let json = serde_json::to_string(&cfg).unwrap();
            let back: NewGameConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(cfg, back);
        }
    }
}
