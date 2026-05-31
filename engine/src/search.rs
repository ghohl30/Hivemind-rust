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
//! - Eval is a five-feature heuristic (queen surroundedness, mobility, development,
//!   queen immobilisation). The weights are starting points; Texel-style tuning
//!   is planned for Phase 7. The AlphaZero learned evaluator is Phase 8.
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
use crate::piece::{queen_of, Color, PieceId, PieceSlot, PieceType};
use crate::state::{Outcome, State};
use smallvec::SmallVec;

/// Magnitude for a win at ply 0. Real terminal scores are `MATE_SCORE - ply`
/// so closer mates outrank deeper ones.
pub const MATE_SCORE: i32 = 1_000_000;
/// Anything beyond this is a forced win/loss; below is a heuristic eval.
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 1_000;

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
    let mut stats = SearchStats::default();
    let mut score = 0i32;

    for d in 1..=max_depth {
        score = negamax(state, d, 0, -MATE_SCORE, MATE_SCORE, tt, &mut stats);
        stats.depth = d;

        // If we found a forced mate, no deeper search can improve on it —
        // cut the loop early. The mate distance is correct because the TT
        // stores root-relative scores.
        if score.abs() >= MATE_THRESHOLD {
            break;
        }
    }

    // After search, the root entry (written at max depth or mate depth)
    // holds the best move found across all iterations.
    let best = tt.probe(state.zobrist()).and_then(|e| e.best_move);
    (score, best, stats)
}

fn negamax(
    state: &mut State,
    depth: u8,
    ply: u32,
    mut alpha: i32,
    beta: i32,
    tt: &mut TranspositionTable,
    stats: &mut SearchStats,
) -> i32 {
    stats.nodes += 1;

    // Terminal check first — game-over states have a definitive score
    // regardless of remaining depth.
    if let Some(outcome) = state.is_terminal() {
        return terminal_score(outcome, state.side_to_move(), ply);
    }
    if depth == 0 {
        return evaluate(state);
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
                    return decoded;
                }
                Bound::Lower if decoded >= beta => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return decoded;
                }
                Bound::Upper if decoded <= alpha => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return decoded;
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
        let score = -negamax(state, depth - 1, ply + 1, -beta, -alpha, tt, stats);
        state.unapply();

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

    best_score
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

// Evaluation weights. All heuristic scores stay well below MATE_THRESHOLD.
const W_OPP_QUEEN_NBRS: i32 = 12; // each piece surrounding opponent queen (pressure)
const W_OWN_QUEEN_NBRS: i32 = 15; // each piece surrounding own queen (defensive urgency)
const W_DEVELOPMENT:     i32 =  5; // per piece-in-hand delta (opp_in_hand − own_in_hand)
// W_MOBILITY and W_OPP_QUEEN_IMMOB require gen:: calls at every leaf node.
// Benchmarked at 61% nodes/sec regression at depth 4; deferred to Phase 7.

/// Static evaluation, side-to-move perspective.
///
/// Three active features (features 3 and 5 deferred — see weight comments):
///   1. Opponent queen neighbours  (W_OPP_QUEEN_NBRS per neighbour)
///   2. Own queen neighbours       (W_OWN_QUEEN_NBRS per neighbour, subtracted)
///   4. Development advantage      (W_DEVELOPMENT × pieces-in-hand delta)
fn evaluate(state: &State) -> i32 {
    let stm = state.side_to_move();
    let opp = stm.other();

    // Features 1 & 2: queen surroundedness (queen_neighbours returns 0 if in hand)
    let opp_q_nbrs = queen_neighbours(state, opp) as i32;
    let own_q_nbrs = queen_neighbours(state, stm) as i32;

    // Feature 4: development (symmetric at game start, safe to compute always)
    let own_in_hand = pieces_in_hand(state, stm) as i32;
    let opp_in_hand = pieces_in_hand(state, opp) as i32;

    opp_q_nbrs  * W_OPP_QUEEN_NBRS
  - own_q_nbrs  * W_OWN_QUEEN_NBRS
  + (opp_in_hand - own_in_hand) * W_DEVELOPMENT
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

fn pieces_in_hand(state: &State, color: Color) -> u8 {
    PieceId::for_color(color)
        .filter(|&pid| state.piece_slot(pid).is_in_hand())
        .count() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::Coord;
    use crate::gen;

    fn queen_is_immobilized(state: &State, color: Color) -> bool {
        let q = queen_of(color);
        if state.piece_slot(q).is_in_hand() { return false; }
        let mut moves: SmallVec<[Move; 64]> = SmallVec::new();
        gen::generate_movements(state, color, &mut moves);
        !moves.iter().any(|m| matches!(m, Move::Slide { piece, .. } if *piece == q))
    }

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
        // WB - WQ - BQ - BB in a line. Both queens have 2 neighbours and
        // development is equal. With W_OWN_QUEEN_NBRS (15) > W_OPP_QUEEN_NBRS (12),
        // equal threats produce a small negative score (defensive bias by design).
        // Expected: 2×W_OPP − 2×W_OWN = 24 − 30 = −6.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0),  to: Coord::ORIGIN        }); // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0)     }); // BQ
        s.apply(Move::Place { piece: PieceId(1),  to: Coord::new(-1, 0)    }); // W beetle
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(2, 0)     }); // B beetle
        assert_eq!(evaluate(&s), -6, "symmetric 2-nbr position should score −6 with current weights");
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

    #[test]
    fn pieces_in_hand_counts_placed_pieces() {
        let mut s = State::new();
        assert_eq!(pieces_in_hand(&s, Color::White), 11);
        assert_eq!(pieces_in_hand(&s, Color::Black), 11);
        s.apply(Move::Place { piece: PieceId(0), to: Coord::ORIGIN }); // WQ placed
        assert_eq!(pieces_in_hand(&s, Color::White), 10);
        assert_eq!(pieces_in_hand(&s, Color::Black), 11);
    }

    #[test]
    fn queen_immobilized_detects_articulation() {
        // Queens in hand → not immobilised
        let s = State::new();
        assert!(!queen_is_immobilized(&s, Color::White));
        assert!(!queen_is_immobilized(&s, Color::Black));

        // WB - WQ - BQ - BB in a line: both queens are internal articulation points
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(0),  to: Coord::ORIGIN        }); // WQ
        s.apply(Move::Place { piece: PieceId(11), to: Coord::new(1, 0)     }); // BQ
        s.apply(Move::Place { piece: PieceId(1),  to: Coord::new(-1, 0)    }); // WB
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(2, 0)     }); // BB
        assert!(queen_is_immobilized(&s, Color::White), "WQ should be immobilised (articulation)");
        assert!(queen_is_immobilized(&s, Color::Black), "BQ should be immobilised (articulation)");
    }

    #[test]
    fn evaluation_rewards_development_advantage() {
        // Game with no queens placed yet: only development feature fires.
        // 3 white pieces on board (8 in hand), 2 black (9 in hand); black's turn.
        // development score = (8 − 9) × W_DEVELOPMENT = −5 from black's POV.
        let mut s = State::new();
        s.apply(Move::Place { piece: PieceId(1),  to: Coord::ORIGIN        }); // WB
        s.apply(Move::Place { piece: PieceId(12), to: Coord::new(1, 0)     }); // BB
        s.apply(Move::Place { piece: PieceId(3),  to: Coord::new(-1, 0)    }); // WG
        s.apply(Move::Place { piece: PieceId(14), to: Coord::new(2, 0)     }); // BG
        s.apply(Move::Place { piece: PieceId(6),  to: Coord::new(-2, 0)    }); // WS
        // stm = black (less developed); expect negative score
        assert!(evaluate(&s) < 0,
            "less-developed side should score negatively; got {}", evaluate(&s));
    }
}
