//! Pure presentation helpers: mapping engine types to filenames, colors, and
//! the derived view-models the Leptos components render. No DOM dependency, so
//! the type→asset mapping and hand-grouping logic are unit-tested natively.

use hive_engine::{Color, Coord, PieceId, PieceType, State};

/// Asset filename (under `assets/pieces/`) for a piece type's glyph. Returns
/// `None` for expansion types the base game never produces — callers fall back
/// to no glyph rather than a broken image.
pub fn glyph_file(t: PieceType) -> Option<&'static str> {
    Some(match t {
        PieceType::QueenBee => "queen.svg",
        PieceType::Beetle => "beetle.svg",
        PieceType::Grasshopper => "grasshopper.svg",
        PieceType::Spider => "spider.svg",
        PieceType::SoldierAnt => "ant.svg",
        // Expansions — not in the base game; no asset.
        PieceType::Mosquito | PieceType::Ladybug | PieceType::Pillbug => return None,
    })
}

/// Full URL (relative to the served root) for a piece type's glyph.
pub fn glyph_url(t: PieceType) -> Option<String> {
    glyph_file(t).map(|f| format!("assets/pieces/{f}"))
}

/// Short human label for a piece type (used in tooltips / hand counts).
pub fn type_label(t: PieceType) -> &'static str {
    match t {
        PieceType::QueenBee => "Queen",
        PieceType::Beetle => "Beetle",
        PieceType::Grasshopper => "Grasshopper",
        PieceType::Spider => "Spider",
        PieceType::SoldierAnt => "Ant",
        PieceType::Mosquito => "Mosquito",
        PieceType::Ladybug => "Ladybug",
        PieceType::Pillbug => "Pillbug",
    }
}

/// Tile fill color for a player's pieces (light theme).
pub fn tile_fill(c: Color) -> &'static str {
    match c {
        Color::White => "#f7f4ec",
        Color::Black => "#2b2b2b",
    }
}

/// Tile border/stroke color for a player's pieces.
pub fn tile_stroke(c: Color) -> &'static str {
    match c {
        Color::White => "#b9b09a",
        Color::Black => "#101010",
    }
}

/// Glyph color for a piece on a player-colored tile. The glyph SVGs are
/// `currentColor` silhouettes, so this drives the CSS `color:` of the wrapper.
/// Dark glyph on the light (white) tile, light glyph on the dark (black) tile.
pub fn glyph_color(c: Color) -> &'static str {
    match c {
        Color::White => "#1c1b18",
        Color::Black => "#f2efe6",
    }
}

/// Human label for a color.
pub fn color_label(c: Color) -> &'static str {
    match c {
        Color::White => "White",
        Color::Black => "Black",
    }
}

/// One rendered hex tile (the top of a stack at a cell).
#[derive(Clone, Debug, PartialEq)]
pub struct TileView {
    pub coord: Coord,
    /// The piece sitting on top at this cell.
    pub piece: PieceId,
    /// Number of pieces stacked at this cell (1 = flat, >1 = beetle stack).
    pub stack_height: u8,
}

impl TileView {
    pub fn color(&self) -> Color {
        self.piece.color()
    }
    pub fn piece_type(&self) -> PieceType {
        self.piece.piece_type()
    }
    pub fn is_stack(&self) -> bool {
        self.stack_height > 1
    }
}

/// Every occupied cell as a `TileView`, top-of-stack only, in the engine's
/// coord-sorted order. `stack_height` is the full stack depth at that cell
/// (from `stack_at`), so the count badge can be shown for beetle stacks.
pub fn board_tiles(state: &State) -> Vec<TileView> {
    state
        .entries()
        .map(|(coord, top)| TileView {
            coord,
            piece: top.piece,
            // `StackTop.height` is the top piece's 0-based layer; the true
            // stack depth is the number of pieces sitting at the cell.
            stack_height: state.stack_at(coord).len() as u8,
        })
        .collect()
}

/// All occupied coords (for viewBox fitting), in the engine's sorted order.
pub fn occupied_coords(state: &State) -> Vec<Coord> {
    state.entries().map(|(c, _)| c).collect()
}

/// One grouped in-hand entry for a color: a piece type and how many of that
/// type remain in that color's hand.
#[derive(Clone, Debug, PartialEq)]
pub struct HandEntry {
    pub piece_type: PieceType,
    pub color: Color,
    /// How many of this type are still in hand for this color.
    pub count: usize,
    /// A representative PieceId of this type/color (for the glyph + key).
    pub example: PieceId,
}

/// In-hand pieces for `color`, grouped by type in canonical type order
/// (Queen, Beetle, Grasshopper, Spider, Ant). Types with zero remaining are
/// omitted. Uses `piece_slot(...).is_in_hand()` per the brief.
pub fn hand_entries(state: &State, color: Color) -> Vec<HandEntry> {
    // Canonical display order.
    const ORDER: [PieceType; 5] = [
        PieceType::QueenBee,
        PieceType::Beetle,
        PieceType::Grasshopper,
        PieceType::Spider,
        PieceType::SoldierAnt,
    ];
    let mut out = Vec::new();
    for &t in ORDER.iter() {
        let mut count = 0usize;
        let mut example = None;
        for pid in PieceId::for_color(color) {
            if pid.piece_type() == t && state.piece_slot(pid).is_in_hand() {
                count += 1;
                example.get_or_insert(pid);
            }
        }
        if let Some(example) = example {
            out.push(HandEntry {
                piece_type: t,
                color,
                count,
                example,
            });
        }
    }
    out
}

/// Whether `color` still has its queen in hand and is at/after the turn by
/// which the queen must be placed (turn 4). Drives the "queen must be placed"
/// hint. Uses `turn_for` + `piece_slot` only.
pub fn queen_must_be_placed(state: &State, color: Color) -> bool {
    let queen = PieceId::for_color(color)
        .find(|p| p.piece_type() == PieceType::QueenBee)
        .expect("each color has a queen");
    state.piece_slot(queen).is_in_hand() && state.turn_for(color) >= 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_engine::{Move, State};

    #[test]
    fn every_base_type_has_a_glyph() {
        for t in [
            PieceType::QueenBee,
            PieceType::Beetle,
            PieceType::Grasshopper,
            PieceType::Spider,
            PieceType::SoldierAnt,
        ] {
            assert!(glyph_file(t).is_some(), "{t:?} missing glyph");
        }
    }

    #[test]
    fn expansion_types_have_no_glyph() {
        for t in [PieceType::Mosquito, PieceType::Ladybug, PieceType::Pillbug] {
            assert!(glyph_file(t).is_none());
        }
    }

    #[test]
    fn glyph_url_is_under_assets() {
        assert_eq!(
            glyph_url(PieceType::SoldierAnt).as_deref(),
            Some("assets/pieces/ant.svg")
        );
    }

    #[test]
    fn fresh_hand_has_full_complement_per_color() {
        let state = State::new();
        for color in [Color::White, Color::Black] {
            let entries = hand_entries(&state, color);
            // Five distinct types in hand at game start.
            assert_eq!(entries.len(), 5);
            let total: usize = entries.iter().map(|e| e.count).sum();
            assert_eq!(total, 11, "{color:?} should have 11 pieces in hand");
            // Spot-check a couple of counts.
            let by = |t: PieceType| entries.iter().find(|e| e.piece_type == t).unwrap().count;
            assert_eq!(by(PieceType::QueenBee), 1);
            assert_eq!(by(PieceType::SoldierAnt), 3);
            assert_eq!(by(PieceType::Spider), 2);
        }
    }

    #[test]
    fn placing_a_piece_removes_it_from_hand_and_adds_a_tile() {
        let mut state = State::new();
        // Play the first legal move (some white placement at the origin).
        let m = state.legal_moves()[0];
        state.apply(m);
        // One tile on the board now.
        let tiles = board_tiles(&state);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].stack_height, 1);
        assert!(!tiles[0].is_stack());
        // The placed piece left white's hand: white now has 10.
        let white_total: usize = hand_entries(&state, Color::White)
            .iter()
            .map(|e| e.count)
            .sum();
        assert_eq!(white_total, 10);
        // Black untouched.
        let black_total: usize = hand_entries(&state, Color::Black)
            .iter()
            .map(|e| e.count)
            .sum();
        assert_eq!(black_total, 11);
        // Silence unused warning on Move import in some configs.
        let _ = Move::Pass;
    }

    #[test]
    fn queen_hint_false_at_game_start() {
        let state = State::new();
        // Turn 1 for both, queens in hand → no hint yet.
        assert!(!queen_must_be_placed(&state, Color::White));
        assert!(!queen_must_be_placed(&state, Color::Black));
    }

    #[test]
    fn tile_and_glyph_colors_differ_per_player() {
        assert_ne!(tile_fill(Color::White), tile_fill(Color::Black));
        assert_ne!(glyph_color(Color::White), glyph_color(Color::Black));
    }
}
