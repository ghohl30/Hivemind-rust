//! Leptos components that draw a [`Session`]'s current `State`. Presentational
//! only: no move application, no selection, no AI — that is later PRs.
//!
//! Structure:
//!   `App`         — the responsive shell: status panel, board, two hand panels.
//!   `Board`       — the auto-fitting SVG of hex tiles.
//!   `HexTile`     — one flat-top hex polygon + glyph (+ stack badge).
//!   `StatusPanel` — whose turn, turn number, placements, queen hint, outcome.
//!   `HandPanel`   — one color's in-hand pieces grouped by type with counts.
//!
//! The geometry and view-model logic live in the pure `hex` / `view` modules so
//! the components stay thin; the components only translate those into markup.

use hive_engine::{Color, Outcome};
use leptos::*;

use crate::game::Session;
use crate::render::demo::demo_session;
use crate::render::hex::{axial_to_pixel, fit_viewbox, hex_polygon_points, viewbox_attr};
use crate::render::view::{
    board_tiles, color_label, glyph_color, glyph_url, hand_entries, occupied_coords,
    queen_must_be_placed, tile_fill, tile_stroke, type_label, HandEntry, TileView,
};

/// Hex radius (centre-to-corner) in SVG user units. The viewBox auto-fit makes
/// the absolute value cosmetic — it only sets the internal resolution.
const HEX_SIZE: f64 = 40.0;
/// Extra breathing room around the hive, in user units.
const BOARD_MARGIN: f64 = 8.0;

/// Root component. Builds the fixed demo session once and renders it. No
/// reactive state yet — this PR is a static snapshot.
#[component]
pub fn App() -> impl IntoView {
    let session = demo_session();
    let state = session.state().clone();

    let outcome = state.is_terminal();
    let white_hand = hand_entries(&state, Color::White);
    let black_hand = hand_entries(&state, Color::Black);

    view! {
        <main class="app">
            <h1>"Hive"</h1>
            <StatusPanel session=session.clone() />
            <div class="play-area">
                <HandPanel color=Color::White entries=white_hand />
                <Board session=session.clone() />
                <HandPanel color=Color::Black entries=black_hand />
            </div>
            {move || outcome.map(|o| view! { <OutcomeBanner outcome=o /> })}
        </main>
    }
}

/// The board: an `<svg>` that scales to its container via a fitted viewBox.
#[component]
fn Board(session: Session) -> impl IntoView {
    let state = session.state();
    let coords = occupied_coords(state);
    let tiles = board_tiles(state);

    let view_box = fit_viewbox(&coords, HEX_SIZE, BOARD_MARGIN)
        .map(|b| viewbox_attr(&b))
        // Empty board: a small neutral box so the SVG still renders.
        .unwrap_or_else(|| "-50 -50 100 100".to_string());

    view! {
        <section class="board" aria-label="game board">
            <svg
                class="board-svg"
                viewBox=view_box
                preserveAspectRatio="xMidYMid meet"
                xmlns="http://www.w3.org/2000/svg"
            >
                {tiles
                    .into_iter()
                    .map(|t| view! { <HexTile tile=t /> })
                    .collect_view()}
            </svg>
        </section>
    }
}

/// One hex cell: the flat-top polygon (player-colored), the top piece's glyph,
/// and — for a stack — a small height badge in the corner.
#[component]
fn HexTile(tile: TileView) -> impl IntoView {
    let centre = axial_to_pixel(tile.coord, HEX_SIZE);
    let points = hex_polygon_points(centre, HEX_SIZE);
    let color = tile.color();
    let fill = tile_fill(color);
    let stroke = tile_stroke(color);
    let glyph_col = glyph_color(color);
    let glyph = glyph_url(tile.piece_type());

    // Glyph sits in a square centred on the hex, scaled to ~80% of the hex
    // width so it stays inside the polygon.
    let g = HEX_SIZE * 1.1;
    let gx = centre.x - g / 2.0;
    let gy = centre.y - g / 2.0;

    // Badge in the upper-right corner of the hex.
    let badge_cx = centre.x + HEX_SIZE * 0.55;
    let badge_cy = centre.y - HEX_SIZE * 0.55;
    let height = tile.stack_height;
    let is_stack = tile.is_stack();

    view! {
        <g class="hex-tile">
            <polygon points=points fill=fill stroke=stroke stroke-width="1.5" />
            {glyph
                .map(|url| {
                    view! {
                        // CSS `color` drives the currentColor silhouette.
                        <image
                            href=url
                            x=gx
                            y=gy
                            width=g
                            height=g
                            style=format!("color:{glyph_col};")
                            preserveAspectRatio="xMidYMid meet"
                        />
                    }
                })}
            {is_stack
                .then(|| {
                    view! {
                        <g class="stack-badge">
                            <circle
                                cx=badge_cx
                                cy=badge_cy
                                r=HEX_SIZE * 0.26
                                fill="#c0392b"
                                stroke="#ffffff"
                                stroke-width="1"
                            />
                            <text
                                x=badge_cx
                                y=badge_cy
                                fill="#ffffff"
                                font-size=HEX_SIZE * 0.34
                                text-anchor="middle"
                                dominant-baseline="central"
                                font-family="system-ui, sans-serif"
                            >
                                {height.to_string()}
                            </text>
                        </g>
                    }
                })}
        </g>
    }
}

/// Whose turn, turn number, placements so far, and the queen-must-be-placed
/// hint for the side to move. Read-only.
#[component]
fn StatusPanel(session: Session) -> impl IntoView {
    let state = session.state();
    let side = state.side_to_move();
    let side_text = color_label(side);
    let turn = state.turn_for(side);
    let placements = state.placements_so_far();
    let show_queen_hint = queen_must_be_placed(state, side);

    view! {
        <section class="status" aria-label="game status">
            <span class="status-item">
                <strong>"To move: "</strong>
                {side_text}
            </span>
            <span class="status-item">
                <strong>"Turn: "</strong>
                {turn.to_string()}
            </span>
            <span class="status-item">
                <strong>"Placements: "</strong>
                {placements.to_string()}
            </span>
            {show_queen_hint
                .then(|| {
                    view! { <span class="status-hint">"Queen must be placed this turn"</span> }
                })}
        </section>
    }
}

/// One color's in-hand pieces, grouped by type, each as a glyph on its
/// player-colored tile with a remaining-count badge.
#[component]
fn HandPanel(color: Color, entries: Vec<HandEntry>) -> impl IntoView {
    let label = color_label(color);
    let fill = tile_fill(color);
    let stroke = tile_stroke(color);
    let glyph_col = glyph_color(color);

    view! {
        <section
            class="hand"
            data-color=color_label(color)
            aria-label=format!("{label} pieces in hand")
        >
            <h2 class="hand-title">{label}" hand"</h2>
            <div class="hand-grid">
                {entries
                    .into_iter()
                    .map(|e| {
                        let glyph = glyph_url(e.piece_type);
                        let title = format!("{} ×{}", type_label(e.piece_type), e.count);
                        view! {
                            <div class="hand-piece" title=title>
                                <div
                                    class="hand-tile"
                                    style=format!(
                                        "background:{fill};border-color:{stroke};color:{glyph_col};",
                                    )
                                >
                                    {glyph
                                        .map(|url| {
                                            view! { <img class="hand-glyph" src=url alt="" /> }
                                        })}
                                    <span class="hand-count">{e.count.to_string()}</span>
                                </div>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </section>
    }
}

/// Read-only outcome line shown when the game is over. The full end-of-game
/// banner flow is a later PR; this is just text.
#[component]
fn OutcomeBanner(outcome: Outcome) -> impl IntoView {
    let text = match outcome {
        Outcome::Win(Color::White) => "White wins".to_string(),
        Outcome::Win(Color::Black) => "Black wins".to_string(),
        Outcome::Draw => "Draw".to_string(),
    };
    view! { <section class="outcome" aria-live="polite">{text}</section> }
}
