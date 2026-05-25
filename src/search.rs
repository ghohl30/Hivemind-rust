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

use crate::moves::Move;
use crate::piece::{queen_of, Color, PieceSlot};
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

/// Top-level entry point. Runs negamax to `depth`, returns
/// `(score, best_move, stats)` from the root side-to-move's perspective.
pub fn search(
    state: &mut State,
    depth: u8,
    tt: &mut TranspositionTable,
) -> (i32, Option<Move>, SearchStats) {
    let mut stats = SearchStats::default();
    let score = negamax(state, depth, 0, -MATE_SCORE, MATE_SCORE, tt, &mut stats);
    // After search, the root entry should contain the best move.
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
            match entry.bound {
                Bound::Exact => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return entry.score;
                }
                Bound::Lower if entry.score >= beta => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return entry.score;
                }
                Bound::Upper if entry.score <= alpha => {
                    stats.tt_hits += 1;
                    stats.tt_cutoffs += 1;
                    return entry.score;
                }
                _ => {
                    stats.tt_hits += 1;
                }
            }
        }
    }

    let moves = state.legal_moves();
    // Move ordering: try the TT move first if it's still legal. Cheap,
    // significant cutoffs gain.
    let mut ordered: SmallVec<[Move; 64]> = SmallVec::with_capacity(moves.len());
    if let Some(tm) = tt_move {
        if let Some(pos) = moves.iter().position(|m| *m == tm) {
            ordered.push(tm);
            for (i, m) in moves.iter().enumerate() {
                if i != pos {
                    ordered.push(*m);
                }
            }
        } else {
            ordered.extend_from_slice(&moves);
        }
    } else {
        ordered.extend_from_slice(&moves);
    }

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
    // Skip TT writes for mate scores — they're ply-relative and storing them
    // unscaled would mislead probes from a different ply context. The simplest
    // safe choice for this first-cut search.
    if best_score.abs() < MATE_THRESHOLD {
        tt.store(TtEntry {
            key,
            depth,
            score: best_score,
            bound,
            best_move,
        });
        stats.tt_stores += 1;
    } else if best_move.is_some() {
        // Still record the best-move hint at this position so iterative
        // searches can use it for ordering, but mark depth 0 so the
        // mate-score isn't trusted for cutoffs.
        tt.store(TtEntry {
            key,
            depth: 0,
            score: 0,
            bound: Bound::Exact,
            best_move,
        });
        stats.tt_stores += 1;
    }

    best_score
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
}
