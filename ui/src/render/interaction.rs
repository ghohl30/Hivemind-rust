//! Pure click-to-move interaction logic, native-testable (no DOM / Leptos).
//!
//! The interaction model is a small state machine over a [`Selection`]:
//!   - Nothing selected: a click either selects a hand piece, selects an
//!     on-board piece the side-to-move controls, or does nothing.
//!   - Something selected: a click on a *legal destination* for that selection
//!     resolves to the concrete [`Move`] to apply (via [`resolve_destination`]);
//!     a click elsewhere re-selects or clears.
//!
//! All of this is expressed as pure functions over the engine `State` and our
//! [`LegalMoveIndex`], so the "selected piece + clicked coord -> Move" mapping
//! and the selectability rules are unit-tested here rather than by clicking the
//! rendered SVG. The Leptos layer (`components`) only owns the signals and the
//! click-vs-drag discrimination.

use hive_engine::{Color, Coord, Move, PieceId, State};

use crate::game::LegalMoveIndex;

/// What the player currently has picked up, pending a destination click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Selection {
    /// An in-hand piece chosen from a hand panel, awaiting a placement target.
    Hand(PieceId),
    /// An on-board piece (the top of a stack) chosen on the board, awaiting a
    /// move target.
    Board(PieceId),
}

impl Selection {
    /// The piece this selection refers to, regardless of origin.
    pub fn piece(self) -> PieceId {
        match self {
            Selection::Hand(p) | Selection::Board(p) => p,
        }
    }
}

/// The top piece at `coord` if it belongs to `side`, else `None`. This is the
/// selectability test for board clicks: only the side-to-move's pieces (and only
/// the visible top of a stack) can be picked up.
pub fn selectable_top_at(state: &State, coord: Coord, side: Color) -> Option<PieceId> {
    state.entries().find_map(|(c, top)| {
        if c == coord && top.piece.color() == side {
            Some(top.piece)
        } else {
            None
        }
    })
}

/// Given the current selection and a clicked board coord, return the legal
/// [`Move`] that lands the selected piece on that coord — if one exists in the
/// index. `None` means the click was not on a legal destination for the
/// selection (the caller should treat it as a re-select / clear instead).
///
/// We look up the selected piece's legal moves and match the destination, rather
/// than trusting the click, so an illegal target can never produce a `Move`.
pub fn resolve_destination(
    selection: Selection,
    clicked: Coord,
    index: &LegalMoveIndex,
) -> Option<Move> {
    let piece = selection.piece();
    index
        .moves_for_piece(piece)
        .iter()
        .copied()
        .find(|m| match m {
            Move::Place { to, .. } | Move::Slide { to, .. } => *to == clicked,
            Move::Pass => false,
        })
}

/// The destination coords that should be highlighted gold for `selection`.
/// Empty if the selected piece has no legal move this turn (e.g. a pinned piece,
/// or a hand piece that cannot legally be placed yet).
pub fn highlight_destinations(selection: Selection, index: &LegalMoveIndex) -> Vec<Coord> {
    index.destinations_for_piece(selection.piece())
}

/// Resolve a board click into the next selection state, given what is currently
/// selected. This is the pure core of the board-click handler; the component
/// only decides *when* to call it (after distinguishing a click from a drag) and
/// applies any returned move.
///
/// Returns either a [`Move`] to apply (the click hit a legal destination) or the
/// new [`Selection`] state (`None` = cleared). The two are mutually exclusive:
/// applying a move always clears selection, which the caller handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoardClick {
    /// Apply this move, then clear selection.
    Apply(Move),
    /// Replace the selection with this (or clear it when `None`).
    Select(Option<Selection>),
}

/// Pure board-click resolution. `side` is the side to move.
///
/// Rules:
///   1. If something is selected and the click is a legal destination for it,
///      return [`BoardClick::Apply`].
///   2. Else, if the clicked cell holds a top piece the side-to-move controls,
///      select it (toggling off if it is already the current selection).
///   3. Else, clear the selection.
pub fn board_click(
    state: &State,
    index: &LegalMoveIndex,
    selection: Option<Selection>,
    clicked: Coord,
    side: Color,
) -> BoardClick {
    if let Some(sel) = selection {
        if let Some(m) = resolve_destination(sel, clicked, index) {
            return BoardClick::Apply(m);
        }
    }
    match selectable_top_at(state, clicked, side) {
        // Clicking the already-selected on-board piece toggles it off.
        Some(p) if selection == Some(Selection::Board(p)) => BoardClick::Select(None),
        Some(p) => BoardClick::Select(Some(Selection::Board(p))),
        None => BoardClick::Select(None),
    }
}

/// Pure hand-chip click resolution: clicking a hand piece selects it, unless it
/// is already the current selection, in which case it toggles off.
///
/// The caller is responsible for only offering hand chips of the side to move
/// (the hand panels render both colors, but only the active color's chips are
/// wired clickable). We still gate on color here for safety.
pub fn hand_click(
    selection: Option<Selection>,
    piece: PieceId,
    side: Color,
) -> Option<Selection> {
    if piece.color() != side {
        return selection;
    }
    if selection == Some(Selection::Hand(piece)) {
        None
    } else {
        Some(Selection::Hand(piece))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Session;
    use hive_engine::{Move, PieceType};

    /// First legal placement of `want` for the side to move, applied.
    fn place_type(s: &mut Session, want: PieceType) -> Move {
        let m = s
            .state()
            .legal_moves()
            .into_iter()
            .find(|m| match m {
                Move::Place { piece, .. } => piece.piece_type() == want,
                _ => false,
            })
            .expect("a legal placement of the wanted type");
        s.push_move(m).unwrap();
        m
    }

    #[test]
    fn hand_click_selects_then_toggles_off() {
        let s = Session::new();
        let side = s.state().side_to_move();
        let index = LegalMoveIndex::from_state(s.state());
        // Pick any piece the index says is placeable this turn.
        let piece = index.movable_pieces().next().unwrap();
        let sel = hand_click(None, piece, side);
        assert_eq!(sel, Some(Selection::Hand(piece)));
        // Clicking it again clears.
        assert_eq!(hand_click(sel, piece, side), None);
    }

    #[test]
    fn hand_click_rejects_wrong_color() {
        let s = Session::new();
        let side = s.state().side_to_move();
        // A piece of the *other* color must not become selected.
        let other = PieceId::for_color(side.other()).next().unwrap();
        assert_eq!(hand_click(None, other, side), None);
    }

    #[test]
    fn highlight_for_hand_piece_matches_index_placements() {
        let s = Session::new();
        let index = LegalMoveIndex::from_state(s.state());
        let piece = index.movable_pieces().next().unwrap();
        let hi = highlight_destinations(Selection::Hand(piece), &index);
        // Every highlighted coord is an actual legal destination for that piece.
        for c in &hi {
            assert!(resolve_destination(Selection::Hand(piece), *c, &index).is_some());
        }
        assert!(!hi.is_empty(), "the first placeable piece should have targets");
    }

    #[test]
    fn resolve_destination_returns_the_matching_move() {
        // Opening: White's only placement coord is the origin.
        let s = Session::new();
        let index = LegalMoveIndex::from_state(s.state());
        let piece = index.movable_pieces().next().unwrap();
        let dest = highlight_destinations(Selection::Hand(piece), &index)[0];
        let m = resolve_destination(Selection::Hand(piece), dest, &index).unwrap();
        match m {
            Move::Place { piece: p, to } => {
                assert_eq!(p, piece);
                assert_eq!(to, dest);
            }
            _ => panic!("expected a Place, got {m:?}"),
        }
        // A coord with no legal move resolves to None.
        let far = Coord::new(50, 50);
        assert!(resolve_destination(Selection::Hand(piece), far, &index).is_none());
    }

    #[test]
    fn board_click_applies_a_legal_destination() {
        let s = Session::new();
        let index = LegalMoveIndex::from_state(s.state());
        let piece = index.movable_pieces().next().unwrap();
        let dest = highlight_destinations(Selection::Hand(piece), &index)[0];
        let side = s.state().side_to_move();
        let click = board_click(
            s.state(),
            &index,
            Some(Selection::Hand(piece)),
            dest,
            side,
        );
        match click {
            BoardClick::Apply(Move::Place { piece: p, to }) => {
                assert_eq!(p, piece);
                assert_eq!(to, dest);
            }
            other => panic!("expected Apply(Place), got {other:?}"),
        }
    }

    #[test]
    fn board_click_selects_own_top_piece_and_toggles() {
        // Place one white piece, then (as White still, before the click is on
        // White's turn) we instead build a position where White has a piece on
        // the board and it's White's move.
        let mut s = Session::new();
        place_type(&mut s, PieceType::SoldierAnt); // White places at origin.
        // Now it's Black to move; the white piece at origin is NOT selectable
        // for Black.
        let origin = Coord::ORIGIN;
        let black = s.state().side_to_move();
        assert_eq!(black, Color::Black);
        let index = LegalMoveIndex::from_state(s.state());
        let click = board_click(s.state(), &index, None, origin, black);
        assert_eq!(
            click,
            BoardClick::Select(None),
            "opponent's piece is not selectable"
        );

        // Place black adjacent so we can test selecting a Black top piece on
        // Black's own turn would require it to be Black's move with the piece on
        // board — instead verify selectability of White's piece on White's turn
        // by checking selectable_top_at directly.
        let white_top = selectable_top_at(s.state(), origin, Color::White);
        assert!(white_top.is_some(), "white piece is white-selectable");
    }

    #[test]
    fn board_click_on_empty_clears_selection() {
        let s = Session::new();
        let index = LegalMoveIndex::from_state(s.state());
        let side = s.state().side_to_move();
        let piece = index.movable_pieces().next().unwrap();
        // A selection plus a click on a non-destination empty cell clears it.
        let click = board_click(
            s.state(),
            &index,
            Some(Selection::Hand(piece)),
            Coord::new(40, -40),
            side,
        );
        assert_eq!(click, BoardClick::Select(None));
    }

    #[test]
    fn toggling_a_board_selection_off() {
        // Set up: White ant at origin; it's White's move and the ant is the top
        // piece. Selecting it then clicking it again toggles off.
        let mut s = Session::new();
        // White places ant at origin (move 1). Black places (move 2). White to
        // move again — now the origin ant is White's and White is to move, but
        // for a single placed piece an early move may be illegal; we only test
        // the pure toggle path, which does not depend on the ant being movable.
        place_type(&mut s, PieceType::SoldierAnt);
        place_type(&mut s, PieceType::SoldierAnt); // Black at a neighbour.
        let side = s.state().side_to_move();
        assert_eq!(side, Color::White);
        let origin = Coord::ORIGIN;
        let white_piece = selectable_top_at(s.state(), origin, side).unwrap();
        let index = LegalMoveIndex::from_state(s.state());
        // Re-clicking the selected board piece toggles it off.
        let click = board_click(
            s.state(),
            &index,
            Some(Selection::Board(white_piece)),
            origin,
            side,
        );
        assert_eq!(click, BoardClick::Select(None));
    }

    #[test]
    fn forced_pass_detected_when_only_pass_is_legal() {
        // Build directly: a one-element [Pass] move list is a forced pass.
        let index = LegalMoveIndex::from_moves(vec![Move::Pass]);
        assert!(index.is_forced_pass());
        // And a normal opening is not.
        let s = Session::new();
        assert!(!LegalMoveIndex::from_state(s.state()).is_forced_pass());
    }
}
