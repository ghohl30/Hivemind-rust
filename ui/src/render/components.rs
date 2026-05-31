//! Leptos components that draw a [`Session`]'s current `State`. Presentational
//! only apart from the view controls (zoom / pan / fit): no move application, no
//! selection, no AI — those are later PRs.
//!
//! Structure:
//!   `App`         — the responsive shell: status panel, board, two hand panels.
//!   `Board`       — the SVG of hex tiles with a reactive viewBox (zoom/pan/fit).
//!   `HexTile`     — one pointy-top hex polygon + glyph (+ stack badge).
//!   `FrontierCell`— a faint empty-cell outline behind the hive (honeycomb look).
//!   `StatusPanel` — whose turn, turn number, placements, queen hint, outcome.
//!   `HandPanel`   — one color's in-hand pieces grouped by type with counts.
//!
//! The geometry and view math (axial→pixel, viewBox fit/zoom/pan, frontier) live
//! in the pure `hex` module and the view-model logic in `view`, so the
//! components stay thin: they wire DOM events to the pure math and emit markup.

use hive_engine::{Color, Outcome};
use leptos::*;
use web_sys::{PointerEvent, WheelEvent};

use crate::game::Session;
use crate::render::demo::demo_session;
use crate::render::hex::{
    axial_to_pixel, client_to_user, fit_viewbox, frontier_coords, hex_polygon_points,
    pan_by_pixels, zoom_about, ViewBox,
};
use crate::render::view::{
    board_tiles, color_label, glyph_color, glyph_url, hand_entries, occupied_coords,
    queen_must_be_placed, tile_fill, tile_stroke, type_label, HandEntry, TileView,
};

/// Hex radius (centre-to-corner) in SVG user units. Constant: pieces render at a
/// natural, comfortable size and are never blown up to fill the panel. The
/// viewBox (not the hex) handles fit/zoom/pan. ~58px is hive-gpt's comfortable
/// tile; in user-space terms this radius gives that feel at the default fit.
const HEX_SIZE: f64 = 58.0;
/// Extra breathing room around the hive, in user units.
const BOARD_MARGIN: f64 = 24.0;
/// Minimum viewBox extent (user units) for the default fit, so small early-game
/// positions stay centered at natural scale instead of zooming in huge. ~9 hex
/// widths across.
const MIN_FIT_EXTENT: f64 = HEX_SIZE * 9.0;
/// Empty-board fallback extent.
const EMPTY_EXTENT: f64 = MIN_FIT_EXTENT;

/// Wheel-zoom clamp, expressed as zoom relative to the fit extent.
const MIN_ZOOM: f64 = 0.3;
const MAX_ZOOM: f64 = 4.0;
/// Per-notch wheel zoom factor (a deltaY notch multiplies the viewBox by this;
/// <1 zooms in, applied as `factor.powf(sign)`).
const WHEEL_STEP: f64 = 0.88;
/// Button zoom factor (about the viewBox center).
const BUTTON_STEP: f64 = 0.8;

/// Root component. Builds the fixed demo session once and renders it.
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

/// Compute the default-fit viewBox for a session's occupied cells at the fixed
/// hex size (centered, natural scale, not stretched). Empty board falls back to
/// a small neutral box.
fn default_viewbox(session: &Session) -> ViewBox {
    let coords = occupied_coords(session.state());
    fit_viewbox(&coords, HEX_SIZE, BOARD_MARGIN, MIN_FIT_EXTENT).unwrap_or(ViewBox {
        min_x: -EMPTY_EXTENT / 2.0,
        min_y: -EMPTY_EXTENT / 2.0,
        w: EMPTY_EXTENT,
        h: EMPTY_EXTENT,
    })
}

/// The board: an `<svg>` whose reactive `viewBox` signal drives zoom / pan /
/// fit. The hex size is constant; only the viewBox changes.
#[component]
fn Board(session: Session) -> impl IntoView {
    let state = session.state();
    let tiles = board_tiles(state);
    let frontier = frontier_coords(state);

    // The fit viewBox and its extent (used as the zoom reference) are derived
    // once from the static demo position. base_extent is the fit width.
    let fit = default_viewbox(&session);
    let base_extent = fit.w;

    let (view_box, set_view_box) = create_signal(fit);

    // Element ref so wheel/pointer handlers can read the SVG's pixel size and
    // bounding rect for accurate pixel→user conversion.
    let svg_ref = create_node_ref::<leptos::svg::Svg>();

    // Drag state: last pointer position in client pixels while a drag is active.
    let drag = store_value::<Option<(f64, f64)>>(None);

    let svg_size = move || -> (f64, f64, f64, f64) {
        // Returns (left, top, width, height) of the SVG element in client px.
        if let Some(el) = svg_ref.get_untracked() {
            let el: &web_sys::Element = el.as_ref();
            let r = el.get_bounding_client_rect();
            (r.left(), r.top(), r.width(), r.height())
        } else {
            (0.0, 0.0, 1.0, 1.0)
        }
    };

    // --- Wheel zoom about the cursor ---
    let on_wheel = move |ev: WheelEvent| {
        ev.prevent_default();
        let (left, top, w, h) = svg_size();
        let px = ev.client_x() as f64 - left;
        let py = ev.client_y() as f64 - top;
        let vb = view_box.get_untracked();
        let anchor = client_to_user(vb, px, py, w, h);
        // deltaY > 0 (scroll down) zooms out; < 0 zooms in.
        let notches = (ev.delta_y() / 100.0).clamp(-3.0, 3.0);
        let factor = WHEEL_STEP.powf(-notches); // scroll up (neg) -> factor<1 -> zoom in
        let next = zoom_about(vb, anchor, factor, base_extent, MIN_ZOOM, MAX_ZOOM);
        set_view_box.set(next);
    };

    // --- Pointer-drag pan ---
    let on_pointer_down = move |ev: PointerEvent| {
        // Capture the pointer so we keep getting move/up even if it leaves.
        if let Some(el) = svg_ref.get_untracked() {
            let el: &web_sys::Element = el.as_ref();
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        drag.set_value(Some((ev.client_x() as f64, ev.client_y() as f64)));
    };
    let on_pointer_move = move |ev: PointerEvent| {
        if let Some((lx, ly)) = drag.get_value() {
            let cx = ev.client_x() as f64;
            let cy = ev.client_y() as f64;
            let dx = cx - lx;
            let dy = cy - ly;
            let (_l, _t, w, h) = svg_size();
            let vb = view_box.get_untracked();
            set_view_box.set(pan_by_pixels(vb, dx, dy, w, h));
            drag.set_value(Some((cx, cy)));
        }
    };
    let on_pointer_up = move |ev: PointerEvent| {
        if let Some(el) = svg_ref.get_untracked() {
            let el: &web_sys::Element = el.as_ref();
            let _ = el.release_pointer_capture(ev.pointer_id());
        }
        drag.set_value(None);
    };

    // --- Buttons: zoom about center, and fit ---
    let zoom_at_center = move |factor: f64| {
        let vb = view_box.get_untracked();
        let c = vb.center();
        set_view_box.set(zoom_about(vb, c, factor, base_extent, MIN_ZOOM, MAX_ZOOM));
    };
    let on_zoom_in = move |_| zoom_at_center(BUTTON_STEP);
    let on_zoom_out = move |_| zoom_at_center(1.0 / BUTTON_STEP);
    let on_fit = move |_| set_view_box.set(fit);

    let vb_attr = move || view_box.get().attr();

    view! {
        <section class="board" aria-label="game board">
            <svg
                node_ref=svg_ref
                class="board-svg"
                viewBox=vb_attr
                preserveAspectRatio="xMidYMid meet"
                xmlns="http://www.w3.org/2000/svg"
                on:wheel=on_wheel
                on:pointerdown=on_pointer_down
                on:pointermove=on_pointer_move
                on:pointerup=on_pointer_up
                on:pointercancel=on_pointer_up
            >
                <g class="frontier-layer">
                    {frontier
                        .into_iter()
                        .map(|c| view! { <FrontierCell coord=c /> })
                        .collect_view()}
                </g>
                <g class="tile-layer">
                    {tiles
                        .into_iter()
                        .map(|t| view! { <HexTile tile=t /> })
                        .collect_view()}
                </g>
            </svg>
            <div class="board-controls" aria-label="view controls">
                <button class="ctrl-btn" on:click=on_zoom_out title="Zoom out">"\u{2212}"</button>
                <button class="ctrl-btn" on:click=on_fit title="Fit board">"Fit"</button>
                <button class="ctrl-btn" on:click=on_zoom_in title="Zoom in">"+"</button>
            </div>
        </section>
    }
}

/// A faint empty frontier cell: just the pointy-top outline, behind the hive.
#[component]
fn FrontierCell(coord: hive_engine::Coord) -> impl IntoView {
    let centre = axial_to_pixel(coord, HEX_SIZE);
    let points = hex_polygon_points(centre, HEX_SIZE);
    view! { <polygon class="hex frontier" points=points /> }
}

/// One hex cell: the pointy-top polygon (player-colored), the top piece's glyph,
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

    // Glyph sits in a square centred on the hex, scaled to ~95% of the hex
    // width so it stays inside the polygon.
    let g = HEX_SIZE * 0.95;
    let gx = centre.x - g / 2.0;
    let gy = centre.y - g / 2.0;

    // The glyph SVGs are `currentColor` silhouettes, but an externally
    // referenced `<image>` renders in its own document context, so neither the
    // host's CSS `color` nor `fill` reach its `currentColor` — it always paints
    // the SVG's intrinsic default (black). That made the black player's glyph
    // black-on-dark and unreadable. Recolor the image by its alpha via a
    // per-tile `feFlood`+`feComposite(in)` filter: flood the desired color and
    // keep it only where the glyph is opaque, so the host fully controls color.
    let filter_id = format!("glyph-{}-{}", tile.coord.q, tile.coord.r);
    let filter_ref = format!("url(#{filter_id})");

    // Badge in the upper-right corner of the hex.
    let badge_cx = centre.x + HEX_SIZE * 0.42;
    let badge_cy = centre.y - HEX_SIZE * 0.5;
    let height = tile.stack_height;
    let is_stack = tile.is_stack();

    view! {
        <g class="hex-tile">
            <polygon
                class="hex tile"
                points=points
                fill=fill
                stroke=stroke
                stroke-width="2"
            />
            {glyph
                .map(|url| {
                    view! {
                        // Recolor the silhouette via its alpha (see note above).
                        <filter id=filter_id.clone() color-interpolation-filters="sRGB">
                            <feFlood flood-color=glyph_col result="c" />
                            <feComposite in="c" in2="SourceGraphic" operator="in" />
                        </filter>
                        <image
                            href=url
                            x=gx
                            y=gy
                            width=g
                            height=g
                            filter=filter_ref.clone()
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
                                r=HEX_SIZE * 0.24
                                fill="#b55d24"
                                stroke="#f6efe3"
                                stroke-width="2"
                            />
                            <text
                                x=badge_cx
                                y=badge_cy
                                fill="#f6efe3"
                                font-size=HEX_SIZE * 0.3
                                text-anchor="middle"
                                dominant-baseline="central"
                                font-family="Georgia, 'Times New Roman', serif"
                                font-weight="700"
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
                <span class="eyebrow">"To move"</span>
                <strong class="status-value">{side_text}</strong>
            </span>
            <span class="status-item">
                <span class="eyebrow">"Turn"</span>
                <strong class="status-value">{turn.to_string()}</strong>
            </span>
            <span class="status-item">
                <span class="eyebrow">"Placements"</span>
                <strong class="status-value">{placements.to_string()}</strong>
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
            <h2 class="hand-title"><span class="eyebrow">{label}" hand"</span></h2>
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
                                            // Recolor via CSS mask: the `<span>`'s
                                            // background paints in the host glyph
                                            // color, masked to the SVG silhouette's
                                            // alpha. (An `<img>` would render the
                                            // SVG's intrinsic black instead.)
                                            view! {
                                                <span
                                                    class="hand-glyph"
                                                    style=format!(
                                                        "background-color:{glyph_col};\
                                                         -webkit-mask:url({url}) center/contain no-repeat;\
                                                         mask:url({url}) center/contain no-repeat;",
                                                    )
                                                ></span>
                                            }
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
