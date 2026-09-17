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
fn difficulty_budgets_are_distinct_and_ordered() {
    assert_eq!(Difficulty::Easy.budget_ms(), 500.0);
    assert_eq!(Difficulty::Medium.budget_ms(), 3_000.0);
    assert_eq!(Difficulty::Hard.budget_ms(), 20_000.0);
    assert!(Difficulty::Easy.budget_ms() < Difficulty::Medium.budget_ms());
    assert!(Difficulty::Medium.budget_ms() < Difficulty::Hard.budget_ms());
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
    assert_eq!(setup.ai_budget_ms(), 20_000.0);
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
    let req = WorkerRequest::new(session.moves().to_vec(), Difficulty::Hard.budget_ms(), 42);

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
            depth: 5,
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

// ---- time-bounded AI search ------------------------------------------------

/// A deterministic stand-in for `performance.now()`: starts at zero and advances
/// `STEP_MS` on every call. Deterministic so these tests cannot flake on a busy
/// machine, and so their cost is fixed rather than dependent on how fast the
/// host happens to be.
///
/// **Budget arithmetic matters here.** The engine polls the predicate every 1024
/// nodes, and each poll advances this clock by one step, so a budget of N steps
/// aborts after roughly `N * 1024` nodes. These tests run in a debug build where
/// the engine is an order of magnitude slower than release, so keep N small —
/// a budget of 1000 steps is ~1M nodes and will appear to hang.
const STEP_MS: f64 = 1.0;

fn fake_clock() -> impl FnMut() -> f64 {
    let mut t = 0.0;
    move || {
        let now = t;
        t += STEP_MS;
        now
    }
}

/// Budget covering roughly `polls` predicate checks (~`polls * 1024` nodes).
fn budget_for(polls: u32) -> f64 {
    f64::from(polls) * STEP_MS
}

#[test]
fn ai_returns_a_legal_move_for_the_position() {
    let session = play_first_legal(6);
    let best = compute_ai_move_with_clock(session.moves(), budget_for(8), fake_clock())
        .expect("non-terminal position must yield a move");
    assert!(session.state().legal_moves().contains(&best));
}

#[test]
fn ai_honours_an_already_spent_budget() {
    let session = play_first_legal(6);
    // Budget zero: the predicate is true at the very first poll. The engine
    // still owes us a completed depth-1 result rather than `None`.
    let best = compute_ai_move_with_clock(session.moves(), 0.0, fake_clock())
        .expect("depth 1 always completes, even on an expired budget");
    assert!(session.state().legal_moves().contains(&best));
}

#[test]
fn ai_search_leaves_the_session_record_untouched() {
    let session = play_first_legal(6);
    let before = session.moves().to_vec();
    let _ = compute_ai_move_with_clock(session.moves(), budget_for(4), fake_clock());
    assert_eq!(session.moves(), before.as_slice());
}

#[test]
fn ai_searches_deeper_with_a_larger_budget() {
    // The claim this whole PR rests on: budget controls strength. Uses the
    // engine directly to read `stats.depth`, which `compute_ai_move_with_clock`
    // deliberately does not surface.
    use hive_engine::{search_bounded, SearchStats, TranspositionTable};

    let session = play_first_legal(6);
    let depth_for = |budget_ms: f64| -> u8 {
        let mut state = replay(session.moves());
        let mut tt = TranspositionTable::with_capacity_log2(18);
        let mut clock = fake_clock();
        let start = clock();
        let mut should_stop = |_: &SearchStats| clock() - start >= budget_ms;
        let (_score, _best, stats) =
            search_bounded(&mut state, MAX_DEPTH, &mut tt, &mut should_stop);
        stats.depth
    };

    let shallow = depth_for(budget_for(1));
    let generous = depth_for(budget_for(32));
    assert!(
        generous > shallow,
        "a 32x budget should buy depth: {shallow} -> {generous}"
    );
}

// ---- worker request handling -----------------------------------------------

#[test]
fn handle_request_echoes_the_request_id() {
    let session = play_first_legal(6);
    let req = WorkerRequest::new(session.moves().to_vec(), budget_for(4), 7);
    let resp = handle_request(&req, fake_clock());
    assert_eq!(resp.request_id, 7);
}

#[test]
fn handle_request_returns_a_legal_move_and_real_stats() {
    let session = play_first_legal(6);
    let req = WorkerRequest::new(session.moves().to_vec(), budget_for(8), 1);
    let resp = handle_request(&req, fake_clock());

    let best = resp.best_move.expect("non-terminal position must yield a move");
    assert!(session.state().legal_moves().contains(&best));
    assert!(resp.stats.nodes > 0, "a real search must visit nodes");
    assert!(resp.stats.depth >= 1, "at least one iteration must complete");
}

#[test]
fn handle_request_response_survives_the_wire() {
    let session = play_first_legal(6);
    let req = WorkerRequest::new(session.moves().to_vec(), budget_for(4), 99);

    // The worker posts JSON and the main thread parses it; assert the whole
    // round trip, since that is what actually crosses the thread boundary.
    let req_json = serde_json::to_string(&req).unwrap();
    let decoded: WorkerRequest = serde_json::from_str(&req_json).unwrap();
    let resp = handle_request(&decoded, fake_clock());
    let resp_json = serde_json::to_string(&resp).unwrap();
    let back: WorkerResponse = serde_json::from_str(&resp_json).unwrap();

    assert_eq!(resp, back);
    assert_eq!(back.request_id, 99);
}

#[test]
fn was_forced_distinguishes_a_short_circuit_from_a_real_search() {
    use hive_engine::SearchStats;

    let some_move = Move::Place {
        piece: PieceId(0),
        to: hive_engine::Coord::new(0, 0),
    };

    // The engine short-circuits a position with exactly one legal move: it
    // reports `depth: 0` and a placeholder score of 0, neither of which is a
    // search result. The UI must not render that as an evaluation. (The
    // short-circuit itself is the engine's behaviour and is tested there; what
    // is ours is recognising it.)
    let forced = AiSearch {
        best: Some(some_move),
        score: 0,
        stats: SearchStats {
            depth: 0,
            ..Default::default()
        },
    };
    assert!(forced.was_forced());

    // A completed search at any depth is a real result.
    let searched = AiSearch {
        best: Some(some_move),
        score: -42,
        stats: SearchStats {
            depth: 1,
            ..Default::default()
        },
    };
    assert!(!searched.was_forced());

    // A terminal position yields no move at all, which is not a forced move.
    let terminal = AiSearch {
        best: None,
        score: 0,
        stats: SearchStats {
            depth: 0,
            ..Default::default()
        },
    };
    assert!(!terminal.was_forced());
}

// ---- live search progress --------------------------------------------------

#[test]
fn progress_reports_completed_depths_in_order() {
    let session = play_first_legal(6);
    let seen = std::cell::RefCell::new(Vec::new());

    let result = search_with_progress(
        session.moves(),
        budget_for(32),
        fake_clock(),
        |depth| seen.borrow_mut().push(depth),
    );

    let seen = seen.into_inner();
    assert!(!seen.is_empty(), "a multi-iteration search must report progress");
    assert!(
        seen.windows(2).all(|w| w[0] < w[1]),
        "depths must be strictly increasing, got {seen:?}"
    );
    // Progress is a hint: the last iteration returns without a further poll, so
    // the authoritative depth is in the result and is never behind the hint.
    assert!(result.stats.depth >= *seen.last().unwrap());
}

#[test]
fn an_expired_budget_reports_at_most_the_guaranteed_iteration() {
    let session = play_first_legal(6);
    let seen = std::cell::RefCell::new(Vec::new());

    // Budget zero: the predicate is true at the first poll. The engine still
    // runs depth 1 unconditionally, so that one iteration may be reported —
    // but nothing deeper, or the indicator would promise a search that never
    // happened.
    let result = search_with_progress(session.moves(), 0.0, fake_clock(), |depth| {
        seen.borrow_mut().push(depth)
    });

    let seen = seen.into_inner();
    assert!(seen.is_empty() || seen.as_slice() == [1u8], "got {seen:?}");
    assert_eq!(result.stats.depth, 1, "depth 1 is the engine's guarantee");
}

#[test]
fn handle_request_with_progress_reports_and_still_answers() {
    let session = play_first_legal(6);
    let req = WorkerRequest::new(session.moves().to_vec(), budget_for(32), 3);
    let seen = std::cell::RefCell::new(Vec::new());

    let resp = handle_request_with_progress(&req, fake_clock(), |d| seen.borrow_mut().push(d));

    assert!(!seen.into_inner().is_empty());
    assert_eq!(resp.request_id, 3);
    assert!(resp.best_move.is_some());
}

#[test]
fn worker_messages_round_trip_through_json() {
    // Both variants cross the thread boundary as JSON, and the main thread has
    // to tell them apart without guessing.
    let progress = WorkerMessage::Progress {
        request_id: 12,
        depth: 4,
    };
    let json = serde_json::to_string(&progress).unwrap();
    assert_eq!(
        serde_json::from_str::<WorkerMessage>(&json).unwrap(),
        progress
    );

    let done = WorkerMessage::Done(WorkerResponse {
        request_id: 12,
        best_move: None,
        score: 7,
        stats: WorkerSearchStats::default(),
    });
    let json = serde_json::to_string(&done).unwrap();
    assert_eq!(serde_json::from_str::<WorkerMessage>(&json).unwrap(), done);

    // A Progress must never decode as a Done, or a live update would be taken
    // for a final answer and end the turn early.
    let progress_json = serde_json::to_string(&progress).unwrap();
    let decoded: WorkerMessage = serde_json::from_str(&progress_json).unwrap();
    assert!(matches!(decoded, WorkerMessage::Progress { .. }));
}

// ---- analysis: what the engine has proven ----------------------------------

#[test]
fn no_completed_iteration_is_not_an_assessment() {
    // The engine reports `depth: 0` with a placeholder score when a move was
    // forced. That is not a search result, and must not be confused with a
    // completed search that found no forced win.
    assert_eq!(Assessment::from_search(0, Color::White, 0), None);
    assert_eq!(Assessment::from_search(500, Color::Black, 0), None);
}

#[test]
fn an_ordinary_score_proves_nothing_however_large() {
    use hive_engine::MATE_THRESHOLD;

    // Everything below the threshold is a heuristic judgement. The panel says
    // "no forced win found", which is a fact about the search — not "equal".
    for score in [0, -40, 250, MATE_THRESHOLD - 1, -(MATE_THRESHOLD - 1)] {
        let a = Assessment::from_search(score, Color::White, 8).unwrap();
        assert_eq!(a.forced, None, "score {score} should prove nothing");
        assert_eq!(a.depth, 8);
    }

    // The threshold itself is a mate.
    assert!(Assessment::from_search(MATE_THRESHOLD, Color::White, 8)
        .unwrap()
        .forced
        .is_some());
}

#[test]
fn a_mate_score_names_the_winner_and_the_distance() {
    use hive_engine::MATE_SCORE;

    // White to move, mate in 3 plies: White wins, stated as 2 of its own moves
    // (it moves on plies 1 and 3).
    let forced = Assessment::from_search(MATE_SCORE - 3, Color::White, 6)
        .unwrap()
        .forced
        .expect("a mate score is a forced result");
    assert_eq!(forced.winner, Color::White);
    assert_eq!(forced.plies, 3);
    assert_eq!(forced.moves, 2);

    // The same magnitude with Black to move belongs to Black: the raw score is
    // always from the mover's side, so the perspective has to be undone first.
    let forced = Assessment::from_search(MATE_SCORE - 3, Color::Black, 6)
        .unwrap()
        .forced
        .unwrap();
    assert_eq!(forced.winner, Color::Black);

    // A losing score names the *other* side as the winner, and an even ply
    // count halves exactly.
    let forced = Assessment::from_search(-(MATE_SCORE - 4), Color::White, 6)
        .unwrap()
        .forced
        .unwrap();
    assert_eq!(forced.winner, Color::Black);
    assert_eq!(forced.plies, 4);
    assert_eq!(forced.moves, 2);
}

#[test]
fn mate_distance_rounds_up_to_whole_moves() {
    use hive_engine::MATE_SCORE;

    // Mate in 1 ply is one move, not half a one; 5 plies is 3.
    let moves_for = |plies: i32| {
        Assessment::from_search(MATE_SCORE - plies, Color::White, 9)
            .unwrap()
            .forced
            .unwrap()
            .moves
    };
    assert_eq!(moves_for(1), 1);
    assert_eq!(moves_for(2), 1);
    assert_eq!(moves_for(3), 2);
    assert_eq!(moves_for(5), 3);
}

#[test]
fn a_real_search_produces_a_readable_assessment() {
    // End to end through the engine rather than synthetic scores: whatever a
    // real search returns has to decode without panicking, and an opening
    // position must not claim a forced win.
    let session = play_first_legal(6);
    let result = search_with_clock(session.moves(), budget_for(16), fake_clock());
    let assessment = Assessment::from_search(
        result.score,
        session.state().side_to_move(),
        result.stats.depth,
    )
    .expect("a completed search is an assessment");
    assert!(assessment.depth >= 1);
    assert_eq!(
        assessment.forced, None,
        "no side has a forced win six plies into the game"
    );
}

// ---- analysis: a forced win in a real position -----------------------------

/// A complete game the engine actually played (a depth-4 search against a
/// deterministic random opponent), ending in a win for White. Frozen as a move
/// list because that is the only portable representation of a position — and
/// because `Session::from_moves` re-validates every move, so the fixture cannot
/// silently rot into an illegal sequence.
const FINISHED_GAME: &str = include_str!("fixtures/forced_win.json");

fn finished_game() -> Vec<Move> {
    serde_json::from_str(FINISHED_GAME).expect("fixture parses")
}

#[test]
fn the_fixture_is_a_legal_game_that_white_wins() {
    let moves = finished_game();
    let session = Session::from_moves(&moves).expect("every move must still be legal");
    assert_eq!(
        session.outcome(),
        Some(hive_engine::Outcome::Win(Color::White))
    );
}

#[test]
fn a_losing_position_is_reported_as_the_opponents_forced_win() {
    // One ply before the end: Black to move and lost. This is the case the
    // panel exists for — a warning aimed at the player who is about to be
    // surrounded — and it exercises the perspective inversion, since the raw
    // score is a *negative* mate from Black's point of view.
    let moves = finished_game();
    let session = Session::from_moves(&moves[..moves.len() - 1]).expect("prefix is legal");
    assert!(!session.is_over());
    assert_eq!(session.state().side_to_move(), Color::Black);

    let mut state = replay(session.moves());
    let mut tt = hive_engine::TranspositionTable::with_capacity_log2(16);
    let (score, _best, stats) = hive_engine::search(&mut state, 4, &mut tt);

    let assessment = Assessment::from_search(score, session.state().side_to_move(), stats.depth)
        .expect("a completed search is an assessment");
    let forced = assessment
        .forced
        .expect("a mate four plies out must be proven at depth 4");
    assert_eq!(forced.winner, Color::White, "Black is the one losing here");
    assert_eq!(forced.plies, 4);
    assert_eq!(forced.moves, 2, "stated in the winner's own moves");
}
