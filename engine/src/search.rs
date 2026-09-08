//! Negamax + alpha-beta search with a Zobrist-keyed transposition table.
//!
//! This is the first real search in the engine and the concrete payoff for
//! Phases 2–4: Zobrist gives us TT keys, make-unmake means we don't clone the
//! state per child, the perimeter / placement-legality caches keep move
//! generation cheap.
//!
//! Conventions:
//! - Scores are from the side-to-move's perspective (standard negamax).
//! - `+MATE_SCORE - ply` ⇒ side-to-move can force a win in `ply` plies.
//!   Smaller `ply` (mate-in-fewer) ⇒ larger score ⇒ preferred.
//! - The TT is direct-mapped (no buckets/chaining). Entries are unconditionally
//!   replaced on collision; this is the simplest scheme that demonstrates the
//!   payoff and is fine for the depth-of-search we exercise here.
//! - Eval is intentionally trivial — queen-surroundedness — because the brief
//!   carves the AlphaZero-style learned evaluator out as a separate future
//!   phase. The search machinery is the deliverable here; the eval can swap
//!   later without touching anything in this file.
//!
//! ## TT mate-score encoding
//!
//! Mate scores are stored in the TT as "distance from root" (DFR), not
//! "distance from the node where they were found". This matters for iterative
//! deepening: a mate-in-3 found during depth-6 iteration should still read as
//! mate-in-3 when probed during depth-4 iteration, regardless of what ply the
//! probe happens at.
//!
//! Encoding (on store):
//!   positive mate  → stored = score + ply   (shifts score up; root-relative)
//!   negative mate  → stored = score - ply   (shifts score down)
//!
//! Decoding (on probe):
//!   positive mate  → score  = stored - ply  (reverses the shift)
//!   negative mate  → score  = stored + ply

use crate::moves::Move;
use crate::piece::{queen_of, Color, PieceSlot, PieceType};
use crate::state::{Outcome, State};
use smallvec::SmallVec;

/// Magnitude for a win at ply 0. Real terminal scores are `MATE_SCORE - ply`
/// so closer mates outrank deeper ones.
pub const MATE_SCORE: i32 = 1_000_000;
/// Anything beyond this is a forced win/loss; below is a heuristic eval.
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 1_000;

/// Poll `should_stop` once every this many nodes (mask of a power of two minus
/// one). Checking every node would put a vtable call in the hottest loop in the
/// program; at ~1.2M nodes/sec this bounds overshoot past the deadline to well
/// under a millisecond, which is noise against any realistic time budget.
const NODE_CHECK_MASK: u64 = 1023;

/// Returned by a subtree that was cut off by `should_stop`. Its score is
/// meaningless and must never reach the TT or a caller — routing aborts through
/// the error channel of a `Result` is what makes that structurally impossible
/// rather than a matter of remembering to check a flag.
struct Aborted;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchStats {
    pub nodes: u64,
    pub tt_hits: u64,
    pub tt_cutoffs: u64,
    pub beta_cutoffs: u64,
    pub tt_stores: u64,
    /// Deepest fully-completed iteration (always equals `max_depth` for a
    /// non-iterative call, equals the last completed depth for an ID search).
    pub depth: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bound {
    Exact,
    /// `score` is a lower bound (we got a beta cutoff — the true value is ≥ score).
    Lower,
    /// `score` is an upper bound (we never beat alpha — true value is ≤ score).
    Upper,
}

#[derive(Clone, Copy, Debug)]
struct TtEntry {
    key: u64,
    depth: u8,
    score: i32,
    bound: Bound,
    best_move: Option<Move>,
}

/// Direct-mapped transposition table. Capacity is a power of two; index is the
/// low bits of the Zobrist key.
pub struct TranspositionTable {
    entries: Vec<Option<TtEntry>>,
    mask: u64,
}

impl TranspositionTable {
    /// Allocate a table with `1 << log2_capacity` slots.
    pub fn with_capacity_log2(log2_capacity: u32) -> Self {
        let size = 1usize << log2_capacity;
        Self {
            entries: vec![None; size],
            mask: (size as u64) - 1,
        }
    }

    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        for e in &mut self.entries {
            *e = None;
        }
    }

    fn probe(&self, key: u64) -> Option<TtEntry> {
        let idx = (key & self.mask) as usize;
        match self.entries[idx] {
            Some(e) if e.key == key => Some(e),
            _ => None,
        }
    }

    fn store(&mut self, e: TtEntry) {
        let idx = (e.key & self.mask) as usize;
        self.entries[idx] = Some(e);
    }
}

/// Top-level entry point. Runs iterative deepening negamax up to `max_depth`,
/// returns `(score, best_move, stats)` from the root side-to-move's
/// perspective. `stats.depth` reports the deepest completed iteration.
///
/// The TT is **not** cleared between iterations: entries from shallower
/// iterations provide move-ordering hints that prune the deeper search.
/// Callers are responsible for clearing the TT between game moves if they
/// want to avoid stale entries from a prior position (though the key-match
/// guard makes false hits rare).
pub fn search(
    state: &mut State,
    max_depth: u8,
    tt: &mut TranspositionTable,
) -> (i32, Option<Move>, SearchStats) {
    search_bounded(state, max_depth, tt, &mut |_| false)
}

/// Time-bounded search. Identical to [`search`] except that `should_stop` is
/// polled periodically; when it returns `true` the search unwinds and reports
/// the best move from the deepest **fully completed** iteration.
///
/// `should_stop` rather than a `Duration` because the engine must stay
/// clock-free: `std::time::Instant::now()` compiles on `wasm32-unknown-unknown`
/// but panics at runtime, and this crate is compiled into the browser UI's WASM
/// bundle. The caller owns the clock — natively via [`search_timed`], in the
/// browser via `performance.now()`.
///
/// The predicate receives `&SearchStats` so a caller can implement adaptive
/// allocation (e.g. declining to start an iteration that cannot finish) without
/// the engine needing a clock of its own. `stats.depth` is the last completed
/// iteration, so a change in it marks an iteration boundary.
///
/// Guarantees:
/// - Returns `Some` move for any non-terminal position, even if `should_stop`
///   is already true on entry: depth 1 always runs to completion.
/// - Never returns a move from a partially searched iteration.
/// - Leaves `state` exactly as it was found, aborted or not.
pub fn search_bounded(
    state: &mut State,
    max_depth: u8,
    tt: &mut TranspositionTable,
    should_stop: &mut dyn FnMut(&SearchStats) -> bool,
) -> (i32, Option<Move>, SearchStats) {
    let mut stats = SearchStats::default();

    // A forced move needs no deliberation. `depth: 0` truthfully reports that
    // no iteration ran; the returned score is not a search result and callers
    // should not read meaning into it.
    if state.is_terminal().is_none() {
        let root_moves = state.legal_moves();
        if root_moves.len() == 1 {
            return (0, Some(root_moves[0]), stats);
        }
    }

    let mut score = 0i32;
    let mut best: Option<Move> = None;

    for d in 1..=max_depth {
        // Depth 1 is unconditional so we always have a move to return.
        if d > 1 && should_stop(&stats) {
            break;
        }

        match negamax(
            state,
            d,
            0,
            -MATE_SCORE,
            MATE_SCORE,
            tt,
            &mut stats,
            &mut *should_stop,
        ) {
            Ok(s) => {
                score = s;
                stats.depth = d;
                // Read the root move now, while this iteration's root entry is
                // the freshest thing in the table. Deferring the probe to after
                // the loop would risk a later aborted iteration evicting it —
                // the TT is direct-mapped and always-replace.
                if let Some(m) = tt.probe(state.zobrist()).and_then(|e| e.best_move) {
                    best = Some(m);
                }
                // A forced mate cannot be improved on by searching deeper. The
                // distance is correct because the TT stores root-relative scores.
                if score.abs() >= MATE_THRESHOLD {
                    break;
                }
            }
            // Discard the partial iteration entirely: it may have examined only
            // the first few root moves, so its "best so far" is an artefact of
            // move ordering rather than a judgement.
            Err(Aborted) => break,
        }
    }

    (score, best, stats)
}

/// Native convenience wrapper: search under a wall-clock budget.
///
/// Two mechanisms combine to keep the budget. The hard cap comes from polling
/// the deadline every [`NODE_CHECK_MASK`]`+1` nodes, so an overrunning iteration
/// is cut off wherever it happens to be. The adaptive part is declining to
/// *start* an iteration that the previous iteration's cost says cannot finish —
/// without it, the engine would routinely burn the tail of its budget on work it
/// then throws away.
///
/// Deliberately absent on `wasm32`: this would compile there and then panic at
/// runtime inside `Instant::now()`. A compile error is the better failure.
#[cfg(not(target_arch = "wasm32"))]
pub fn search_timed(
    state: &mut State,
    max_depth: u8,
    tt: &mut TranspositionTable,
    budget: std::time::Duration,
) -> (i32, Option<Move>, SearchStats) {
    use std::time::Instant;

    let start = Instant::now();
    let mut iter_start = start;
    let mut last_seen_depth = 0u8;

    let mut should_stop = |stats: &SearchStats| -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(start);
        if elapsed >= budget {
            return true;
        }
        // `stats.depth` only advances when an iteration completes, so this is
        // the iteration boundary and `now - iter_start` is what that iteration
        // cost. Extrapolate the next one and stop if it cannot fit.
        if stats.depth != last_seen_depth {
            let this_iter = now.duration_since(iter_start);
            last_seen_depth = stats.depth;
            iter_start = now;
            if this_iter.mul_f32(EFFECTIVE_BRANCHING_FACTOR) > budget - elapsed {
                return true;
            }
        }
        false
    };

    search_bounded(state, max_depth, tt, &mut should_stop)
}

/// Cost multiplier from one iterative-deepening iteration to the next, used to
/// predict whether the next iteration fits in the remaining budget.
///
/// Measured from `examples/search_bench` on the opening position, where
/// iteration-over-iteration time ratios run ~3.5x early and ~10x by depth 6.
/// Set below the observed worst case on purpose: guessing low costs some wasted
/// work at the end of the budget, guessing high forfeits a whole ply. Re-measure
/// if move ordering or eval cost changes materially.
#[cfg(not(target_arch = "wasm32"))]
const EFFECTIVE_BRANCHING_FACTOR: f32 = 5.0;

fn negamax(
    state: &mut State,
    depth: u8,
    ply: u32,
    mut alpha: i32,
    beta: i32,
    tt: &mut TranspositionTable,
    stats: &mut SearchStats,
    should_stop: &mut dyn FnMut(&SearchStats) -> bool,
) -> Result<i32, Aborted> {
    stats.nodes += 1;

    // Cooperative abort. Interior nodes only: a leaf returns immediately, so
    // checking there would only add cost to the most-executed path.
    if depth > 0 && stats.nodes & NODE_CHECK_MASK == 0 && should_stop(stats) {
        return Err(Aborted);
    }

    // Terminal check first — game-over states have a definitive score
    // regardless of remaining depth.
    if let Some(outcome) = state.is_terminal() {
        return Ok(terminal_score(outcome, state.side_to_move(), ply));
    }
    if depth == 0 {
        return Ok(evaluate(state));
    }

    let key = state.zobrist();
    let original_alpha = alpha;

    // TT probe. A sufficient-depth entry may cut off the whole subtree.
    let mut tt_move: Option<Move> = None;
    if let Some(entry) = tt.probe(key) {
        tt_move = entry.best_move;
        if entry.depth >= depth {
            // Decode the stored score from root-relative to node-relative
            // before using it for cutoffs or returning it.
            let decoded = tt_decode_score(entry.score, ply);
            match entry.bound {
                Bound::Exact => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return Ok(decoded);
                }
                Bound::Lower if decoded >= beta => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return Ok(decoded);
                }
                Bound::Upper if decoded <= alpha => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return Ok(decoded);
                }
                _ => {
                    stats.tt_hits += 1;
                }
            }
        }
    }

    let moves = state.legal_moves();
    // Move ordering pass 1: TT move first (if still legal).
    // Move ordering pass 2: ant Slide moves tiered by proximity to opponent queen.
    //   Tier 1 — `to` adjacent to opponent queen (on board).
    //   Tier 2 — `to` adjacent to any opponent piece.
    //   Tier 3 — ant moves not adjacent to any opponent piece.
    // Final order: [TT move] [tier-1 ant slides] [tier-2 ant slides]
    //              [non-ant non-TT moves in gen order] [tier-3 ant slides]
    //
    // This is a reorder only — every legal move is searched.

    // Determine the opponent's queen coord for tier-1 classification.
    let opp = state.side_to_move().other();
    let opp_queen_coord = match state.piece_slot(queen_of(opp)) {
        PieceSlot::OnBoard { coord, .. } => Some(coord),
        _ => None,
    };

    // Classify each non-TT move.
    let mut tier1: SmallVec<[Move; 8]> = SmallVec::new();
    let mut tier2: SmallVec<[Move; 8]> = SmallVec::new();
    let mut non_ant: SmallVec<[Move; 32]> = SmallVec::new();
    let mut tier3: SmallVec<[Move; 8]> = SmallVec::new();

    let tt_pos = tt_move.and_then(|tm| moves.iter().position(|m| *m == tm));

    for (i, &m) in moves.iter().enumerate() {
        // Skip the TT move — it goes first unconditionally.
        if tt_pos == Some(i) {
            continue;
        }
        if let Move::Slide { piece, to } = m {
            if piece.piece_type() == PieceType::SoldierAnt {
                // Check tier 1: `to` adjacent to opponent queen.
                if let Some(qc) = opp_queen_coord {
                    if qc.neighbours().contains(&to) {
                        tier1.push(m);
                        continue;
                    }
                }
                // Check tier 2: `to` adjacent to any opponent piece (board top).
                let board = state.board();
                let adj_opp = to.neighbours().iter().any(|&nbr| {
                    board.top_at(nbr).map_or(false, |top| top.piece.color() == opp)
                });
                if adj_opp {
                    tier2.push(m);
                } else {
                    tier3.push(m);
                }
                continue;
            }
        }
        non_ant.push(m);
    }

    // Assemble final ordering.
    let mut ordered: SmallVec<[Move; 64]> = SmallVec::with_capacity(moves.len());
    if let Some(pos) = tt_pos {
        ordered.push(moves[pos]);
    }
    ordered.extend_from_slice(&tier1);
    ordered.extend_from_slice(&tier2);
    ordered.extend_from_slice(&non_ant);
    ordered.extend_from_slice(&tier3);

    let mut best_score = -MATE_SCORE - 1;
    let mut best_move: Option<Move> = None;

    for m in ordered.iter().copied() {
        state.apply(m);
        let child = negamax(
            state,
            depth - 1,
            ply + 1,
            -beta,
            -alpha,
            tt,
            stats,
            &mut *should_stop,
        );
        // Unconditional, and before `?` inspects the result: this is what keeps
        // every apply paired with an unapply at every frame no matter which
        // depth the abort originated at.
        state.unapply();
        let score = -child?;

        if score > best_score {
            best_score = score;
            best_move = Some(m);
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            stats.beta_cutoffs += 1;
            break;
        }
    }

    // Determine bound flag for the stored entry.
    let bound = if best_score <= original_alpha {
        Bound::Upper
    } else if best_score >= beta {
        Bound::Lower
    } else {
        Bound::Exact
    };
    // Encode the score as root-relative before storing so probes from any ply
    // can reconstruct the correct node-relative value via tt_decode_score.
    tt.store(TtEntry {
        key,
        depth,
        score: tt_encode_score(best_score, ply),
        bound,
        best_move,
    });
    stats.tt_stores += 1;

    Ok(best_score)
}

/// Encode a score for TT storage.
///
/// Mate scores are stored as "distance from root": a mate-in-3 found at
/// ply 2 is stored as `MATE_SCORE - 3 + 2 = MATE_SCORE - 1`, not as
/// `MATE_SCORE - 3`. When probed at a different ply, `tt_decode_score`
/// reverses the shift so the caller always sees distance-from-current-node.
#[inline(always)]
fn tt_encode_score(score: i32, ply: u32) -> i32 {
    let p = ply as i32;
    if score >= MATE_THRESHOLD {
        score + p
    } else if score <= -MATE_THRESHOLD {
        score - p
    } else {
        score
    }
}

/// Decode a score retrieved from the TT back to node-relative (distance from
/// the current node, not from the root).
#[inline(always)]
fn tt_decode_score(stored: i32, ply: u32) -> i32 {
    let p = ply as i32;
    if stored >= MATE_THRESHOLD {
        stored - p
    } else if stored <= -MATE_THRESHOLD {
        stored + p
    } else {
        stored
    }
}

fn terminal_score(outcome: Outcome, side_to_move: Color, ply: u32) -> i32 {
    match outcome {
        Outcome::Draw => 0,
        Outcome::Win(winner) => {
            let mag = MATE_SCORE - ply as i32;
            if winner == side_to_move {
                mag
            } else {
                -mag
            }
        }
    }
}

/// Static evaluation, side-to-move perspective.
///
/// Hive's loss condition is "queen surrounded". The crude-but-load-bearing
/// heuristic is the difference in own-queen and opponent-queen neighbour
/// counts: getting the opponent closer to surrounded is good, getting your
/// own queen closer to surrounded is bad. A queen still in hand contributes 0
/// neighbours — it's safe but means the player hasn't started threats yet.
fn evaluate(state: &State) -> i32 {
    let stm = state.side_to_move();
    let opp = stm.other();
    let own = queen_neighbours(state, stm) as i32;
    let theirs = queen_neighbours(state, opp) as i32;
    // Each neighbour ≈ 1/6 of a mate threat. Weight 10 so scores have headroom.
    (theirs - own) * 10
}

fn queen_neighbours(state: &State, color: Color) -> u8 {
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
    use crate::piece::PieceId;

    #[test]
    fn search_depth_1_returns_some_move() {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(12);
        let (_score, best, stats) = search(&mut s, 1, &mut tt);
        assert!(best.is_some(), "depth-1 search must produce a move");
        assert!(stats.nodes > 0);
    }

    #[test]
    fn search_depth_3_completes_and_picks_a_legal_move() {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(14);
        let (_score, best, _stats) = search(&mut s, 3, &mut tt);
        let best = best.expect("depth-3 search must produce a move");
        let legal = s.legal_moves();
        assert!(legal.iter().any(|m| *m == best), "search returned an illegal move: {best:?}");
    }

    #[test]
    fn search_returns_state_to_original_via_make_unmake() {
        // Search must leave State exactly where it found it (balanced apply/unapply).
        let s = State::new();
        let mut probe = s.clone();
        let mut tt = TranspositionTable::with_capacity_log2(12);
        let _ = search(&mut probe, 2, &mut tt);
        assert_eq!(probe, s, "search left state mutated");
        assert_eq!(probe.undo_depth(), 0, "search left undo records on the stack");
    }

    #[test]
    fn tt_hits_accrue_at_depth_3() {
        // Two interchangeable beetles + commutative opening placements
        // guarantee transpositions show up by depth 3.
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(14);
        let (_, _, stats) = search(&mut s, 3, &mut tt);
        assert!(
            stats.tt_hits > 0,
            "expected TT hits at depth 3; got 0 (nodes={}, stores={})",
            stats.nodes,
            stats.tt_stores
        );
    }

    #[test]
    fn evaluation_is_symmetric_at_initial_state() {
        // Empty board, both queens in hand ⇒ eval is 0.
        let s = State::new();
        assert_eq!(evaluate(&s), 0);
    }

    #[test]
    fn evaluation_rewards_attacking_opponent_queen() {
        // Build a position where white has placed pieces next to black's queen.
        // Side-to-move is whoever it ends up being; we check the sign relative
        // to that.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });           // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });        // BQ
        s.apply(Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });        // W beetle
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(2, 0) });        // B beetle
        // Black has 1 neighbour on its queen; white has 1 neighbour on its queen.
        // Eval should be 0 here (symmetric).
        assert_eq!(evaluate(&s), 0);
    }

    #[test]
    fn tt_mate_score_encode_decode_roundtrip() {
        // encode then decode must be identity for both polarities.
        for ply in [0u32, 1, 3, 10] {
            let pos_mate = MATE_SCORE - ply as i32 - 1;
            let neg_mate = -(MATE_SCORE - ply as i32 - 1);
            let heuristic = 42i32;

            assert_eq!(tt_decode_score(tt_encode_score(pos_mate, ply), ply), pos_mate,
                "positive mate roundtrip failed at ply {ply}");
            assert_eq!(tt_decode_score(tt_encode_score(neg_mate, ply), ply), neg_mate,
                "negative mate roundtrip failed at ply {ply}");
            assert_eq!(tt_decode_score(tt_encode_score(heuristic, ply), ply), heuristic,
                "heuristic roundtrip failed at ply {ply}");
        }
    }

    #[test]
    fn tt_mate_score_ply_independence() {
        let original = MATE_SCORE - 3;
        let encoded = tt_encode_score(original, 3);
        assert_eq!(tt_decode_score(encoded, 3), original);
        assert_eq!(tt_decode_score(encoded, 5), MATE_SCORE - 5);
    }

    #[test]
    fn iterative_deepening_stats_depth_equals_max_depth() {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(14);
        let (_, _, stats) = search(&mut s, 3, &mut tt);
        assert_eq!(stats.depth, 3, "stats.depth should equal max_depth when no forced mate");
    }

    #[test]
    fn search_near_terminal_returns_non_pass_move() {
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0),  to: Coord::ORIGIN        }); // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0)     }); // BQ
        s.apply(Move::Place { piece: PieceId(8),  to: Coord::new(-1, 0)    }); // WA1
        s.apply(Move::Place { piece: PieceId(19), to: Coord::new(2, 0)     }); // BA1

        let undo_depth_before = s.undo_depth();
        let mut tt = TranspositionTable::with_capacity_log2(16);
        let (score, best_move, stats) = search(&mut s, 4, &mut tt);

        let bm = best_move.expect("search at depth 4 must return a move");
        assert!(
            !matches!(bm, Move::Pass),
            "search returned Pass in an open position (score={score}); \
             legal_moves={:?}", s.legal_moves()
        );
        assert!(stats.nodes > 0);
        assert_eq!(stats.depth, 4, "stats.depth must equal max_depth");
        assert_eq!(s.undo_depth(), undo_depth_before, "search left undo records on the stack");
    }

    // --- time-bounded search -------------------------------------------

    /// A predicate that fires once a node budget is spent, standing in for a
    /// clock so the test is deterministic rather than timing-dependent.
    fn node_budget(limit: u64) -> impl FnMut(&SearchStats) -> bool {
        move |stats: &SearchStats| stats.nodes >= limit
    }

    #[test]
    fn search_bounded_with_never_stop_matches_search() {
        let mut a = State::new();
        let mut b = State::new();
        let mut tt_a = TranspositionTable::with_capacity_log2(14);
        let mut tt_b = TranspositionTable::with_capacity_log2(14);

        let (score_a, best_a, stats_a) = search(&mut a, 4, &mut tt_a);
        let (score_b, best_b, stats_b) =
            search_bounded(&mut b, 4, &mut tt_b, &mut |_| false);

        assert_eq!(score_a, score_b);
        assert_eq!(best_a, best_b);
        assert_eq!(stats_a, stats_b, "a never-firing predicate must not perturb search");
    }

    #[test]
    fn search_bounded_stops_short_of_max_depth() {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(16);
        // Budget far below what depth 8 costs (baseline: depth 6 alone is ~106k
        // nodes), so iterative deepening must be cut off partway.
        let (_score, best, stats) = search_bounded(&mut s, 8, &mut tt, &mut node_budget(3_000));

        assert!(best.is_some(), "must still return a move when the budget runs out");
        assert!(
            s.legal_moves().contains(&best.unwrap()),
            "returned move must be legal"
        );
        assert!(
            stats.depth < 8,
            "expected an early stop, got a completed depth of {}",
            stats.depth
        );
        assert!(stats.depth >= 1, "depth 1 must always complete");
    }

    #[test]
    fn search_bounded_aborts_without_corrupting_state() {
        let mut s = State::new();
        // Play a few plies so the position is non-trivial and the undo stack
        // has real depth to get wrong.
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN });
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0) });
        s.apply(Move::Place { piece: PieceId(1), to: Coord::new(-1, 0) });

        let before = s.clone();
        let undo_before = s.undo_depth();
        let mut tt = TranspositionTable::with_capacity_log2(16);

        // Stop as early as the poll interval allows, so the abort unwinds from
        // deep inside the tree rather than at a tidy boundary.
        let (_score, best, _stats) = search_bounded(&mut s, 10, &mut tt, &mut node_budget(1));

        assert!(best.is_some(), "depth 1 completes before any abort can fire");
        assert_eq!(s.undo_depth(), undo_before, "abort left undo records on the stack");
        assert_eq!(s, before, "abort did not restore the state bit-for-bit");
    }

    #[test]
    fn search_bounded_returns_a_move_even_if_already_stopped() {
        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(12);
        let (_score, best, stats) = search_bounded(&mut s, 6, &mut tt, &mut |_| true);

        assert!(best.is_some(), "an already-expired budget must not yield None");
        assert_eq!(stats.depth, 1, "exactly the mandatory depth-1 iteration should run");
    }

    #[test]
    fn search_timed_respects_its_budget() {
        use std::time::{Duration, Instant};

        let mut s = State::new();
        let mut tt = TranspositionTable::with_capacity_log2(18);
        let budget = Duration::from_millis(150);

        let started = Instant::now();
        let (_score, best, stats) = search_timed(&mut s, 40, &mut tt, budget);
        let elapsed = started.elapsed();

        assert!(best.is_some());
        // Generous ceiling: this asserts the cap engages at all, not a tight
        // latency bound, so it stays reliable on a loaded CI runner.
        assert!(
            elapsed < budget * 20,
            "budget {budget:?} overrun: took {elapsed:?} to depth {}",
            stats.depth
        );
        assert!(stats.depth >= 1);
    }

    #[test]
    fn ant_ordering_searches_all_moves() {
        // Verify ant ordering doesn't drop moves: search completes without
        // panicking and apply/unapply are balanced.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0),  to: Coord::ORIGIN     }); // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0)  }); // BQ
        s.apply(Move::Place { piece: PieceId(8),  to: Coord::new(-1, 0) }); // WA1
        s.apply(Move::Place { piece: PieceId(19), to: Coord::new(2, 0)  }); // BA1
        let depth_before = s.undo_depth();
        let mut tt = TranspositionTable::with_capacity_log2(14);
        let (_, _, stats) = search(&mut s, 3, &mut tt);
        assert!(stats.nodes > 0);
        assert_eq!(s.undo_depth(), depth_before, "search left undo records on the stack");
    }
}
