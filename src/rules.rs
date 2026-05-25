//! Pure predicates over (`[PieceSlot; 22]`, `&Board`).
//!
//! No mutation, no allocation beyond bounded scratch. These are the primitive
//! checks reused across all five move generators and the terminal check.

use std::collections::HashSet;

use crate::board::Board;
use crate::coord::{Coord, Direction};
use crate::piece::{queen_of, Color, PieceId, PieceSlot, PieceType};

/// Returns true if the moving piece would not violate the One Hive rule by
/// leaving its current coord.
///
/// - `Covered` pieces can never be lifted (a beetle sits on top).
/// - Pieces stacked above ground (height > 0) are always removable from One
///   Hive's perspective: lifting them does not change which coords are occupied.
/// - Ground-level pieces are removable iff their coord is not an articulation
///   point of the hive's adjacency graph.
pub fn one_hive_holds_if_removed(pieces: &[PieceSlot; PieceId::COUNT], board: &Board, pid: PieceId) -> bool {
    match pieces[pid.index()] {
        PieceSlot::InHand => true,
        PieceSlot::Covered { .. } => false,
        PieceSlot::OnBoard { stack_height, coord } => {
            if stack_height > 0 {
                return true;
            }
            // Remove `coord` from the set of occupied coords and check connectivity.
            let occupied: HashSet<Coord> = board.occupied_coords().filter(|c| *c != coord).collect();
            connected(&occupied)
        }
    }
}

/// True if `occupied` is empty OR forms a single 6-connected component.
pub fn connected(occupied: &HashSet<Coord>) -> bool {
    let Some(start) = occupied.iter().next().copied() else {
        return true;
    };
    let mut seen = HashSet::with_capacity(occupied.len());
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        for n in c.neighbours() {
            if occupied.contains(&n) && !seen.contains(&n) {
                stack.push(n);
            }
        }
    }
    seen.len() == occupied.len()
}

/// Ground-level slide check: a single-step slide from `from` to an empty
/// adjacent cell `to` is legal iff *exactly one* of the two shared-neighbour
/// cells is occupied. Equivalently: not both occupied (physical gap) AND not
/// both empty (would lose contact with the hive during the slide).
///
/// `from` is treated as if vacated (the moving piece doesn't block itself).
pub fn can_slide_ground(board: &Board, from: Coord, to: Coord) -> bool {
    can_slide_with_lifted(board, from, to, from)
}

/// As `can_slide_ground` but the piece is considered lifted from `lifted_from`,
/// which may differ from `from` (multi-step spider/ant moves).
pub fn can_slide_with_lifted(board: &Board, from: Coord, to: Coord, lifted_from: Coord) -> bool {
    let (n1, n2) = shared_neighbours(from, to);
    let occ = |c: Coord| board.is_occupied(c) && c != lifted_from;
    occ(n1) ^ occ(n2)
}

/// The two cells adjacent to both `a` and `b`. `a` and `b` must be neighbours
/// — undefined behaviour otherwise.
pub fn shared_neighbours(a: Coord, b: Coord) -> (Coord, Coord) {
    let dq = b.q - a.q;
    let dr = b.r - a.r;
    // For each of the six directions, the two perpendicular directions on
    // either side are the rotations by ±1 in angular order.
    let d = direction_from_delta(dq, dr).expect("a and b must be adjacent");
    (a.neighbour(d.rotated(-1)), a.neighbour(d.rotated(1)))
}

fn direction_from_delta(dq: i16, dr: i16) -> Option<Direction> {
    for d in Direction::ALL.iter() {
        let (ddq, ddr) = d.delta();
        if (ddq, ddr) == (dq, dr) {
            return Some(*d);
        }
    }
    None
}

/// Beetle climbing rule (gate). A beetle moving from `from` (currently at the
/// top of a stack of height `h_from_total`) to `to` (with `h_to_existing` tiles
/// already there — 0 if empty) is blocked iff both shared-neighbour stacks are
/// strictly taller than max(h_from_total - 1, h_to_existing).
///
/// `h_from_total` is the full stack height at `from` *including* the beetle, so
/// `h_from_total - 1` is the floor the beetle is leaving from.
pub fn beetle_gate_allows(
    board: &Board,
    from: Coord,
    to: Coord,
    h_from_total: u8,
    h_to_existing: u8,
) -> bool {
    let (n1, n2) = shared_neighbours(from, to);
    let h1 = stack_height_at(board, n1, from);
    let h2 = stack_height_at(board, n2, from);
    let squeeze_floor = std::cmp::max(h_from_total.saturating_sub(1), h_to_existing);
    !(h1 > squeeze_floor && h2 > squeeze_floor)
}

fn stack_height_at(board: &Board, c: Coord, ignore_if: Coord) -> u8 {
    if c == ignore_if {
        return 0;
    }
    match board.top_at(c) {
        // A stack of n tiles has top height (n-1); the *number of tiles* is height+1.
        Some(top) => top.height + 1,
        None => 0,
    }
}

/// Placement-legality predicate.
///
/// `placements_so_far` is the total number of placements *by either player*
/// already on the board — used to gate the two opening special cases:
///   - the very first placement (count == 0): anywhere.
///   - the second placement (count == 1): adjacent to the first piece.
///   - all subsequent placements: must touch at least one own-color tile and
///     no enemy-color tile.
pub fn can_place_for(
    pieces: &[PieceSlot; PieceId::COUNT],
    board: &Board,
    color: Color,
    coord: Coord,
    placements_so_far: usize,
) -> bool {
    if board.is_occupied(coord) {
        return false;
    }
    if placements_so_far == 0 {
        return coord == Coord::ORIGIN;
    }
    if placements_so_far == 1 {
        // Must be adjacent to the single existing piece.
        return coord
            .neighbours()
            .iter()
            .any(|n| board.is_occupied(*n));
    }
    let mut touches_own = false;
    for n in coord.neighbours() {
        if let Some(top) = board.top_at(n) {
            let neighbour_color = top_color_at(pieces, top.piece);
            if neighbour_color == color {
                touches_own = true;
            } else {
                return false;
            }
        }
    }
    touches_own
}

/// The color visible at the top of the stack at `pid`'s coord. For beetles on
/// top of opposing pieces, this is the beetle's color.
fn top_color_at(pieces: &[PieceSlot; PieceId::COUNT], pid: PieceId) -> Color {
    let _ = pieces; // PieceId already encodes color
    pid.color()
}

/// True if `color`'s queen must be placed *now* — i.e., it is still in hand
/// and `color` is on their 4th turn.
pub fn queen_must_be_placed_now(pieces: &[PieceSlot; PieceId::COUNT], color: Color, current_turn: u8) -> bool {
    if current_turn < 4 {
        return false;
    }
    let q = queen_of(color);
    matches!(pieces[q.index()], PieceSlot::InHand)
}

/// True if `color`'s queen has already been placed (on the board, possibly
/// covered by a beetle). Required for any movement by `color`.
pub fn queen_is_placed(pieces: &[PieceSlot; PieceId::COUNT], color: Color) -> bool {
    !matches!(pieces[queen_of(color).index()], PieceSlot::InHand)
}

/// True iff the given color's queen is fully surrounded (all six neighbours
/// occupied). Used for win detection.
pub fn queen_surrounded(pieces: &[PieceSlot; PieceId::COUNT], board: &Board, color: Color) -> bool {
    let q = queen_of(color);
    match pieces[q.index()] {
        PieceSlot::InHand => false,
        PieceSlot::OnBoard { coord, .. } | PieceSlot::Covered { coord, .. } => {
            coord.neighbours().iter().all(|n| board.is_occupied(*n))
        }
    }
}

/// Helper for `gen/`: piece type of a given PieceId. Lookup is constant.
pub fn piece_type_of(pid: PieceId) -> PieceType {
    pid.piece_type()
}

/// Membership predicate for the perimeter set: an empty cell with at least
/// one occupied neighbour. Used by `State`'s incremental cache and by the
/// from-scratch builder in tests.
pub fn is_in_perimeter(board: &Board, c: Coord) -> bool {
    if board.is_occupied(c) {
        return false;
    }
    c.neighbours().iter().any(|n| board.is_occupied(*n))
}

/// Membership predicate for `color`'s placement-legality set, post-opening:
/// the cell is empty, has ≥1 own-color top neighbour, and 0 enemy-color top
/// neighbours. The opening special cases (placements_so_far == 0 or 1) are
/// handled in `gen`, not here.
pub fn is_legal_placement_for(board: &Board, color: Color, c: Coord) -> bool {
    if board.is_occupied(c) {
        return false;
    }
    let mut touches_own = false;
    for n in c.neighbours() {
        if let Some(top) = board.top_at(n) {
            if top.piece.color() == color {
                touches_own = true;
            } else {
                return false;
            }
        }
    }
    touches_own
}

/// Single-pass per-cell membership: returns
/// `(in_perimeter, in_white_legality, in_black_legality)` after one
/// 6-neighbour scan. Used by `State::apply` to halve cache-snapshot cost.
pub fn membership_triple(board: &Board, c: Coord) -> (bool, bool, bool) {
    if board.is_occupied(c) {
        return (false, false, false);
    }
    let mut any = false;
    let mut has_white = false;
    let mut has_black = false;
    for n in c.neighbours() {
        if let Some(top) = board.top_at(n) {
            any = true;
            match top.piece.color() {
                Color::White => has_white = true,
                Color::Black => has_black = true,
            }
        }
    }
    let in_perim = any;
    let in_white = in_perim && has_white && !has_black;
    let in_black = in_perim && has_black && !has_white;
    (in_perim, in_white, in_black)
}

/// From-scratch perimeter set. Used by tests to validate the incremental cache.
pub fn perimeter_from_scratch(board: &Board) -> std::collections::HashSet<Coord> {
    let mut out = std::collections::HashSet::new();
    for c in board.occupied_coords() {
        for n in c.neighbours() {
            if !board.is_occupied(n) {
                out.insert(n);
            }
        }
    }
    out
}

/// From-scratch placement-legality set for one color.
pub fn placement_legality_from_scratch(board: &Board, color: Color) -> std::collections::HashSet<Coord> {
    perimeter_from_scratch(board)
        .into_iter()
        .filter(|c| is_legal_placement_for(board, color, *c))
        .collect()
}

/// Tarjan articulation-point detection over the hive (≤22 vertices, max 6
/// edges per vertex). Returns the set of coords whose removal would
/// disconnect the hive — these are the ground-level pieces that cannot move
/// without violating the One Hive rule.
///
/// Used by `gen::generate_movements` to classify all pieces in one pass
/// instead of running an independent connectivity check per piece.
pub fn articulation_points(board: &Board) -> std::collections::HashSet<Coord> {
    use std::collections::HashMap;
    let coords: Vec<Coord> = board.occupied_coords().collect();
    let n = coords.len();
    if n <= 1 {
        return std::collections::HashSet::new();
    }
    let mut idx: HashMap<Coord, usize> = HashMap::with_capacity(n);
    for (i, c) in coords.iter().enumerate() {
        idx.insert(*c, i);
    }
    // Neighbour adjacency in vertex-index space.
    let mut adj: Vec<smallvec::SmallVec<[usize; 6]>> = vec![smallvec::SmallVec::new(); n];
    for (i, c) in coords.iter().enumerate() {
        for n_c in c.neighbours() {
            if let Some(&j) = idx.get(&n_c) {
                adj[i].push(j);
            }
        }
    }
    // Iterative Tarjan with an explicit stack — keeps the call depth bounded
    // even though n ≤ 22.
    let mut visited = vec![false; n];
    let mut disc = vec![0u32; n];
    let mut low = vec![0u32; n];
    let mut parent = vec![usize::MAX; n];
    let mut is_art = vec![false; n];
    let mut timer: u32 = 0;
    // Stack frame: (node, child-iterator-index)
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(n);
    let root = 0;
    visited[root] = true;
    timer += 1;
    disc[root] = timer;
    low[root] = timer;
    stack.push((root, 0));
    let mut root_children = 0u32;
    while let Some(&(u, _)) = stack.last() {
        let i = stack.last().unwrap().1;
        if i < adj[u].len() {
            stack.last_mut().unwrap().1 += 1;
            let v = adj[u][i];
            if !visited[v] {
                visited[v] = true;
                parent[v] = u;
                timer += 1;
                disc[v] = timer;
                low[v] = timer;
                if u == root {
                    root_children += 1;
                }
                stack.push((v, 0));
            } else if v != parent[u] {
                low[u] = low[u].min(disc[v]);
            }
        } else {
            // Done with u's children. Propagate low to parent.
            stack.pop();
            if let Some(&(p, _)) = stack.last() {
                low[p] = low[p].min(low[u]);
                // Non-root articulation criterion.
                if p != root && low[u] >= disc[p] {
                    is_art[p] = true;
                }
            }
        }
    }
    // Root articulation criterion.
    if root_children > 1 {
        is_art[root] = true;
    }
    coords
        .iter()
        .enumerate()
        .filter(|(i, _)| is_art[*i])
        .map(|(_, c)| *c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::StackTop;

    fn pieces_init() -> [PieceSlot; PieceId::COUNT] {
        [PieceSlot::InHand; PieceId::COUNT]
    }

    fn place(pieces: &mut [PieceSlot; PieceId::COUNT], board: &mut Board, pid: PieceId, c: Coord) {
        pieces[pid.index()] = PieceSlot::OnBoard {
            coord: c,
            stack_height: 0,
        };
        board.set_top(c, StackTop { piece: pid, height: 0 });
    }

    #[test]
    fn connected_empty_and_singleton() {
        assert!(connected(&HashSet::new()));
        let one = [Coord::ORIGIN].into_iter().collect();
        assert!(connected(&one));
    }

    #[test]
    fn connected_chain() {
        let cs: HashSet<Coord> = [Coord::new(0, 0), Coord::new(1, 0), Coord::new(2, 0)]
            .into_iter()
            .collect();
        assert!(connected(&cs));
    }

    #[test]
    fn disconnected_pair() {
        let cs: HashSet<Coord> = [Coord::new(0, 0), Coord::new(3, 0)].into_iter().collect();
        assert!(!connected(&cs));
    }

    #[test]
    fn one_hive_bridge() {
        // A line of three: removing the middle splits the hive.
        let mut pieces = pieces_init();
        let mut board = Board::new();
        place(&mut pieces, &mut board, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut board, PieceId(1), Coord::new(1, 0));
        place(&mut pieces, &mut board, PieceId(2), Coord::new(2, 0));
        // Middle (1,0) is the articulation.
        assert!(!one_hive_holds_if_removed(&pieces, &board, PieceId(1)));
        // Leaves are removable.
        assert!(one_hive_holds_if_removed(&pieces, &board, PieceId(0)));
        assert!(one_hive_holds_if_removed(&pieces, &board, PieceId(2)));
    }

    #[test]
    fn shared_neighbours_basic() {
        let a = Coord::new(0, 0);
        let b = Coord::new(1, 0); // East
        let (n1, n2) = shared_neighbours(a, b);
        let set: HashSet<Coord> = [n1, n2].into_iter().collect();
        // The two flanking cells for an east step are NE (1,-1) and SE (0,1).
        assert!(set.contains(&Coord::new(1, -1)));
        assert!(set.contains(&Coord::new(0, 1)));
    }

    #[test]
    fn slide_gap_blocks() {
        // Pieces at (1,-1) and (0,1) form a gate on the (0,0) -> (1,0) slide.
        let mut pieces = pieces_init();
        let mut board = Board::new();
        place(&mut pieces, &mut board, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut board, PieceId(1), Coord::new(1, -1));
        place(&mut pieces, &mut board, PieceId(2), Coord::new(0, 1));
        // (1,0) is the open destination but both flanks are occupied → blocked.
        assert!(!can_slide_ground(&board, Coord::new(0, 0), Coord::new(1, 0)));
    }

    #[test]
    fn slide_requires_contact() {
        // A lone piece at origin tries to slide east into open space — both
        // flanks empty, contact would be lost. Block.
        let mut pieces = pieces_init();
        let mut board = Board::new();
        place(&mut pieces, &mut board, PieceId(0), Coord::new(0, 0));
        assert!(!can_slide_ground(&board, Coord::new(0, 0), Coord::new(1, 0)));
    }

    #[test]
    fn slide_legal_with_one_flank() {
        let mut pieces = pieces_init();
        let mut board = Board::new();
        place(&mut pieces, &mut board, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut board, PieceId(1), Coord::new(1, -1));
        // From (0,0) to (1,0): one flank (1,-1) occupied, the other (0,1) empty.
        assert!(can_slide_ground(&board, Coord::new(0, 0), Coord::new(1, 0)));
    }

    #[test]
    fn queen_by_turn_4() {
        let mut pieces = pieces_init();
        // White queen still in hand on turn 4 → must be placed.
        assert!(queen_must_be_placed_now(&pieces, Color::White, 4));
        // Place the queen.
        pieces[0] = PieceSlot::OnBoard {
            coord: Coord::ORIGIN,
            stack_height: 0,
        };
        assert!(!queen_must_be_placed_now(&pieces, Color::White, 4));
    }

    #[test]
    fn placement_first_two_are_special() {
        let pieces = pieces_init();
        let board = Board::new();
        // First placement only at origin.
        assert!(can_place_for(&pieces, &board, Color::White, Coord::ORIGIN, 0));
        assert!(!can_place_for(&pieces, &board, Color::White, Coord::new(1, 0), 0));
    }

    #[test]
    fn articulation_empty_and_singleton() {
        let board = Board::new();
        assert!(articulation_points(&board).is_empty());
        let mut b = Board::new();
        let mut pieces = pieces_init();
        place(&mut pieces, &mut b, PieceId(0), Coord::ORIGIN);
        assert!(articulation_points(&b).is_empty());
    }

    #[test]
    fn articulation_line_of_three_picks_middle() {
        // (0,0) - (1,0) - (2,0): middle is the articulation point.
        let mut b = Board::new();
        let mut pieces = pieces_init();
        place(&mut pieces, &mut b, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut b, PieceId(1), Coord::new(1, 0));
        place(&mut pieces, &mut b, PieceId(2), Coord::new(2, 0));
        let arts = articulation_points(&b);
        assert!(arts.contains(&Coord::new(1, 0)));
        assert!(!arts.contains(&Coord::new(0, 0)));
        assert!(!arts.contains(&Coord::new(2, 0)));
    }

    #[test]
    fn articulation_triangle_has_none() {
        // (0,0), (1,0), (1,-1) form a triangle — no articulation points.
        let mut b = Board::new();
        let mut pieces = pieces_init();
        place(&mut pieces, &mut b, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut b, PieceId(1), Coord::new(1, 0));
        place(&mut pieces, &mut b, PieceId(2), Coord::new(1, -1));
        assert!(articulation_points(&b).is_empty());
    }

    #[test]
    fn articulation_matches_per_piece_check() {
        // Cross-check against one_hive_holds_if_removed on a hand-built position.
        let mut b = Board::new();
        let mut pieces = pieces_init();
        place(&mut pieces, &mut b, PieceId(0), Coord::new(0, 0));
        place(&mut pieces, &mut b, PieceId(1), Coord::new(1, 0));
        place(&mut pieces, &mut b, PieceId(2), Coord::new(2, 0));
        place(&mut pieces, &mut b, PieceId(3), Coord::new(2, -1));
        place(&mut pieces, &mut b, PieceId(4), Coord::new(-1, 1));
        let arts = articulation_points(&b);
        for pid in [PieceId(0), PieceId(1), PieceId(2), PieceId(3), PieceId(4)] {
            let coord = match pieces[pid.index()] {
                PieceSlot::OnBoard { coord, .. } => coord,
                _ => unreachable!(),
            };
            let in_articulation = arts.contains(&coord);
            let movable = one_hive_holds_if_removed(&pieces, &b, pid);
            // movable iff NOT articulation.
            assert_eq!(
                in_articulation, !movable,
                "mismatch at {coord:?} (pid {pid:?}): art={in_articulation}, movable={movable}"
            );
        }
    }
}
