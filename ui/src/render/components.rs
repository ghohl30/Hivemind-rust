//! Leptos components for the playable Hive board (hot-seat).
//!
//! The single source of truth is a [`Session`] held in a reactive signal; the
//! board, hands, and status panel all *derive* from the session's current
//! `State`, so applying a move re-renders everything. A separate [`Selection`]
//! signal tracks the piece the player has picked up; legal destinations for that
//! selection are highlighted gold and clicking one applies the corresponding
//! `Move` through the session (which validates it).
//!
//! Structure:
//!   `App`         — owns the `session` and `selection` signals; the shell.
//!   `Board`       — reactive SVG; click-to-select / click-to-move plus the
//!                   PR-7 zoom / pan / fit controls. Discriminates click vs drag.
//!   `HexTile`     — one hex; carries `legal` / `selected` classes and, when
//!                   interactive, a click handler that routes through the board.
//!   `FrontierCell`— faint empty-cell outline (also a click target so clicking
//!                   an empty legal landing spot applies the move).
//!   `StatusPanel` — whose turn, turn number, placements, queen hint, outcome,
//!                   plus the forced-`Pass` affordance.
//!   `HandPanel`   — one color's in-hand pieces; the side-to-move's chips are
//!                   clickable and show a selected state.
//!
//! The pure geometry/view math lives in `hex` / `view`, and the click-resolution
//! logic in `interaction`, all native-tested. These components only own signals,
//! DOM events, and the click-vs-drag threshold.

use std::collections::BTreeSet;

use hive_engine::{Color, Coord, Move, Outcome, PieceId};
use leptos::*;
use web_sys::{PointerEvent, WheelEvent};

use crate::game::{LegalMoveIndex, Session};
use crate::render::demo::demo_session;
use crate::render::hex::{
    axial_to_pixel, client_to_user, fit_viewbox, frontier_coords, hex_polygon_points,
    pan_by_pixels, pixel_to_axial, zoom_about, Point, ViewBox,
};
use crate::render::interaction::{board_click, hand_click, highlight_destinations, BoardClick, Selection};
use crate::render::view::{
    board_tiles, color_label, glyph_color, glyph_url, hand_entries, occupied_coords,
    queen_must_be_placed, tile_fill, tile_stroke, type_label, HandEntry, TileView,
};

const HEX_SIZE: f64 = 58.0;
const BOARD_MARGIN: f64 = 24.0;
const MIN_FIT_EXTENT: f64 = HEX_SIZE * 9.0;
const EMPTY_EXTENT: f64 = MIN_FIT_EXTENT;

const MIN_ZOOM: f64 = 0.3;
const MAX_ZOOM: f64 = 4.0;
const WHEEL_STEP: f64 = 0.88;
const BUTTON_STEP: f64 = 0.8;

/// Pointer movement (in client pixels) beyond which a press is treated as a pan
/// drag rather than a click. Below it, pointer-up resolves as a select/move
/// click. Squared, so the handler compares against a squared distance and skips
/// a sqrt per move event.
const CLICK_SLOP_PX: f64 = 5.0;

/// Whether to start from a real new game (default) or the dev demo position.
/// Flip to `true` while iterating on rendering of a populated board.
const START_FROM_DEMO: bool = false;

/// Root component: owns the live session + selection signals and the shell.
#[component]
pub fn App() -> impl IntoView {
    let initial = if START_FROM_DEMO {
        demo_session()
    } else {
        Session::new()
    };
    let session = create_rw_signal(initial);
    let selection = create_rw_signal::<Option<Selection>>(None);

    // Whenever the position changes, a stale selection (a piece that is no
    // longer the side-to-move's, or no longer placeable/movable) is dropped by
    // the board's reactive highlight derivation; we also clear it on every
    // applied move explicitly in the click handler.

    let white_hand = move || hand_entries(session.get().state(), Color::White);
    let black_hand = move || hand_entries(session.get().state(), Color::Black);

    view! {
        <main class="app">
            <h1>"Hive"</h1>
            <StatusPanel session=session selection=selection />
            <div class="play-area">
                <HandPanel
                    color=Color::White
                    entries=Signal::derive(white_hand)
                    session=session
                    selection=selection
                />
                <Board session=session selection=selection />
                <HandPanel
                    color=Color::Black
                    entries=Signal::derive(black_hand)
                    session=session
                    selection=selection
                />
            </div>
            {move || {
                session
                    .get()
                    .outcome()
                    .map(|o| view! { <OutcomeBanner outcome=o /> })
            }}
        </main>
    }
}

/// Default-fit viewBox for a session's occupied cells at the fixed hex size.
fn default_viewbox(session: &Session) -> ViewBox {
    let coords = occupied_coords(session.state());
    fit_viewbox(&coords, HEX_SIZE, BOARD_MARGIN, MIN_FIT_EXTENT).unwrap_or(ViewBox {
        min_x: -EMPTY_EXTENT / 2.0,
        min_y: -EMPTY_EXTENT / 2.0,
        w: EMPTY_EXTENT,
        h: EMPTY_EXTENT,
    })
}

/// The board SVG. Reactive over `session`; selection-aware highlighting; pan /
/// zoom / fit; click-vs-drag discrimination so a small drag pans without
/// selecting and a click selects without panning.
#[component]
fn Board(session: RwSignal<Session>, selection: RwSignal<Option<Selection>>) -> impl IntoView {
    // Derived, reactive view-models. These re-run whenever the session changes.
    let tiles = move || board_tiles(session.get().state());
    let index = move || LegalMoveIndex::from_state(session.get().state());

    // Highlighted gold destinations for the current selection (empty when
    // nothing is selected or the selection has no legal target).
    let highlighted: Memo<BTreeSet<(i16, i16)>> = create_memo(move |_| {
        match selection.get() {
            Some(sel) => highlight_destinations(sel, &index())
                .into_iter()
                .map(|c| (c.q, c.r))
                .collect(),
            None => BTreeSet::new(),
        }
    });

    // The selected on-board coord (for the accent outline), if the selection is
    // a board piece that is actually sitting somewhere.
    let selected_coord: Memo<Option<(i16, i16)>> = create_memo(move |_| match selection.get() {
        Some(Selection::Board(p)) => session
            .get()
            .state()
            .entries()
            .find(|(_, top)| top.piece == p)
            .map(|(c, _)| (c.q, c.r)),
        _ => None,
    });

    // Frontier cells, unioned with any highlighted destinations that are not
    // already occupied, so an empty legal landing spot always has a clickable
    // outline behind it.
    let frontier = move || {
        let st = session.get();
        let mut set: BTreeSet<(i16, i16)> = frontier_coords(st.state())
            .into_iter()
            .map(|c| (c.q, c.r))
            .collect();
        for hc in highlighted.get() {
            set.insert(hc);
        }
        set.into_iter()
            .map(|(q, r)| Coord::new(q, r))
            .collect::<Vec<_>>()
    };

    // viewBox state. The fit is computed once from the initial position; the
    // user's zoom/pan persist across moves (we do NOT auto-refit on every move,
    // which would fight the player's view). The Fit button recomputes from the
    // *current* board.
    let fit0 = default_viewbox(&session.get_untracked());
    let view_box = create_rw_signal(fit0);
    let base_extent = fit0.w;

    let svg_ref = create_node_ref::<leptos::svg::Svg>();

    let svg_size = move || -> (f64, f64, f64, f64) {
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
        let notches = (ev.delta_y() / 100.0).clamp(-3.0, 3.0);
        let factor = WHEEL_STEP.powf(-notches);
        view_box.set(zoom_about(vb, anchor, factor, base_extent, MIN_ZOOM, MAX_ZOOM));
    };

    // --- Pointer drag-pan with click discrimination ---
    // We track the press origin and whether movement crossed the slop
    // threshold. On pointer-up: if it never crossed, resolve as a click.
    let press_origin = store_value::<Option<(f64, f64)>>(None);
    let last_pos = store_value::<Option<(f64, f64)>>(None);
    let is_drag = store_value::<bool>(false);

    let on_pointer_down = move |ev: PointerEvent| {
        if let Some(el) = svg_ref.get_untracked() {
            let el: &web_sys::Element = el.as_ref();
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        let p = (ev.client_x() as f64, ev.client_y() as f64);
        press_origin.set_value(Some(p));
        last_pos.set_value(Some(p));
        is_drag.set_value(false);
    };

    let on_pointer_move = move |ev: PointerEvent| {
        let Some((lx, ly)) = last_pos.get_value() else {
            return;
        };
        let Some((ox, oy)) = press_origin.get_value() else {
            return;
        };
        let cx = ev.client_x() as f64;
        let cy = ev.client_y() as f64;
        // Once total displacement from the press origin crosses the slop, this
        // gesture is a pan for the rest of its life.
        let tot_dx = cx - ox;
        let tot_dy = cy - oy;
        if !is_drag.get_value() && tot_dx * tot_dx + tot_dy * tot_dy > CLICK_SLOP_PX * CLICK_SLOP_PX {
            is_drag.set_value(true);
        }
        if is_drag.get_value() {
            let dx = cx - lx;
            let dy = cy - ly;
            let (_l, _t, w, h) = svg_size();
            let vb = view_box.get_untracked();
            view_box.set(pan_by_pixels(vb, dx, dy, w, h));
        }
        last_pos.set_value(Some((cx, cy)));
    };

    let on_pointer_up = move |ev: PointerEvent| {
        if let Some(el) = svg_ref.get_untracked() {
            let el: &web_sys::Element = el.as_ref();
            let _ = el.release_pointer_capture(ev.pointer_id());
        }
        let was_drag = is_drag.get_value();
        let origin = press_origin.get_value();
        press_origin.set_value(None);
        last_pos.set_value(None);
        is_drag.set_value(false);

        // A drag pans only; a click (no significant movement) selects/moves.
        if was_drag || origin.is_none() {
            return;
        }
        let (left, top, w, h) = svg_size();
        let px = ev.client_x() as f64 - left;
        let py = ev.client_y() as f64 - top;
        let vb = view_box.get_untracked();
        let user = client_to_user(vb, px, py, w, h);
        let clicked = pixel_to_axial(user, HEX_SIZE);
        handle_board_click(session, selection, clicked);
    };

    // --- Buttons ---
    let zoom_at_center = move |factor: f64| {
        let vb = view_box.get_untracked();
        let c = vb.center();
        view_box.set(zoom_about(vb, c, factor, base_extent, MIN_ZOOM, MAX_ZOOM));
    };
    let on_zoom_in = move |_| zoom_at_center(BUTTON_STEP);
    let on_zoom_out = move |_| zoom_at_center(1.0 / BUTTON_STEP);
    // Fit recomputes from the *current* board so it works after moves.
    let on_fit = move |_| view_box.set(default_viewbox(&session.get_untracked()));

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
                    {move || {
                        let hl = highlighted.get();
                        frontier()
                            .into_iter()
                            .map(|c| {
                                let legal = hl.contains(&(c.q, c.r));
                                view! { <FrontierCell coord=c legal=legal /> }
                            })
                            .collect_view()
                    }}
                </g>
                <g class="tile-layer">
                    {move || {
                        let hl = highlighted.get();
                        let sel = selected_coord.get();
                        tiles()
                            .into_iter()
                            .map(|t| {
                                let key = (t.coord.q, t.coord.r);
                                let legal = hl.contains(&key);
                                let is_selected = sel == Some(key);
                                view! {
                                    <HexTile tile=t legal=legal selected=is_selected />
                                }
                            })
                            .collect_view()
                    }}
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

/// Apply a resolved board click to the signals: select / clear / apply-move.
/// Centralized so the pointer-up handler stays thin. `clicked` is the axial
/// coord under the pointer.
fn handle_board_click(
    session: RwSignal<Session>,
    selection: RwSignal<Option<Selection>>,
    clicked: Coord,
) {
    // Don't accept moves once the game is over.
    if session.with(|s| s.is_over()) {
        return;
    }
    let (decision, side) = session.with(|s| {
        let index = LegalMoveIndex::from_state(s.state());
        let side = s.state().side_to_move();
        (
            board_click(s.state(), &index, selection.get_untracked(), clicked, side),
            side,
        )
    });
    let _ = side;
    match decision {
        BoardClick::Apply(m) => {
            apply_move(session, selection, m);
        }
        BoardClick::Select(next) => {
            selection.set(next);
        }
    }
}

/// Push a move through the session and clear selection. The move came from the
/// legal index, so `push_move` should always succeed; on the off chance it does
/// not, we leave the position untouched and clear the selection.
fn apply_move(
    session: RwSignal<Session>,
    selection: RwSignal<Option<Selection>>,
    m: Move,
) {
    session.update(|s| {
        let _ = s.push_move(m);
    });
    selection.set(None);
}

/// A faint empty frontier cell. Carries `legal` so an empty legal landing spot
/// is highlighted gold; clicks are handled by the parent SVG (event delegation
/// via the shared pointer handlers), so this is purely presentational.
#[component]
fn FrontierCell(coord: Coord, legal: bool) -> impl IntoView {
    let centre = axial_to_pixel(coord, HEX_SIZE);
    let points = hex_polygon_points(centre, HEX_SIZE);
    let class = if legal { "hex frontier legal" } else { "hex frontier" };
    view! { <polygon class=class points=points /> }
}

/// One hex cell: pointy-top polygon (player-colored), the top piece's glyph, a
/// stack badge, and `legal` / `selected` styling. Clicks are handled by the
/// parent SVG via the shared pointer handlers (the pointer-up coord is mapped
/// back to a hex), so tiles need no per-element click handler.
#[component]
fn HexTile(tile: TileView, legal: bool, selected: bool) -> impl IntoView {
    let centre = axial_to_pixel(tile.coord, HEX_SIZE);
    let points = hex_polygon_points(centre, HEX_SIZE);
    let color = tile.color();
    let fill = tile_fill(color);
    let stroke = tile_stroke(color);
    let glyph_col = glyph_color(color);
    let glyph = glyph_url(tile.piece_type());

    let g = HEX_SIZE * 0.95;
    let gx = centre.x - g / 2.0;
    let gy = centre.y - g / 2.0;

    let filter_id = format!("glyph-{}-{}", tile.coord.q, tile.coord.r);
    let filter_ref = format!("url(#{filter_id})");

    let badge_cx = centre.x + HEX_SIZE * 0.42;
    let badge_cy = centre.y - HEX_SIZE * 0.5;
    let height = tile.stack_height;
    let is_stack = tile.is_stack();

    // `legal` (a slide landing on this occupied cell, e.g. a beetle climb) and
    // `selected` (this cell holds the picked-up piece) both modulate the tile.
    let mut tile_class = String::from("hex tile");
    if legal {
        tile_class.push_str(" legal");
    }
    if selected {
        tile_class.push_str(" selected");
    }

    view! {
        <g class="hex-tile">
            <polygon
                class=tile_class
                points=points
                fill=fill
                stroke=stroke
                stroke-width="2"
            />
            {glyph
                .map(|url| {
                    view! {
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

/// Whose turn, turn number, placements, the queen hint, and the forced-`Pass`
/// affordance. All reactive over the session.
#[component]
fn StatusPanel(
    session: RwSignal<Session>,
    selection: RwSignal<Option<Selection>>,
) -> impl IntoView {
    let side = move || session.get().state().side_to_move();
    let side_text = move || color_label(side());
    let turn = move || session.get().state().turn_for(side()).to_string();
    let placements = move || session.get().state().placements_so_far().to_string();
    let show_queen_hint = move || queen_must_be_placed(session.get().state(), side());
    let is_over = move || session.get().is_over();

    // Forced pass: the only legal move is `Pass`. Surface a button rather than
    // auto-passing, so the player sees the situation explicitly.
    let forced_pass = move || {
        !is_over() && LegalMoveIndex::from_state(session.get().state()).is_forced_pass()
    };
    let on_pass = move |_| {
        apply_move(session, selection, Move::Pass);
    };

    view! {
        <section class="status" aria-label="game status">
            <span class="status-item">
                <span class="eyebrow">"To move"</span>
                <strong class="status-value">{side_text}</strong>
            </span>
            <span class="status-item">
                <span class="eyebrow">"Turn"</span>
                <strong class="status-value">{turn}</strong>
            </span>
            <span class="status-item">
                <span class="eyebrow">"Placements"</span>
                <strong class="status-value">{placements}</strong>
            </span>
            {move || {
                show_queen_hint()
                    .then(|| {
                        view! {
                            <span class="status-hint">"Queen must be placed this turn"</span>
                        }
                    })
            }}
            {move || {
                forced_pass()
                    .then(|| {
                        view! {
                            <span class="status-item">
                                <span class="status-hint">"No moves available"</span>
                                <button class="ctrl-btn pass-btn" on:click=on_pass>
                                    "Pass"
                                </button>
                            </span>
                        }
                    })
            }}
        </section>
    }
}

/// One color's in-hand pieces, grouped by type. For the side to move, each chip
/// is a clickable button that selects that piece (and shows a selected state);
/// for the inactive color the chips are inert.
#[component]
fn HandPanel(
    color: Color,
    entries: Signal<Vec<HandEntry>>,
    session: RwSignal<Session>,
    selection: RwSignal<Option<Selection>>,
) -> impl IntoView {
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
                {move || {
                    let active = session.get().state().side_to_move() == color
                        && !session.get().is_over();
                    let sel = selection.get();
                    entries
                        .get()
                        .into_iter()
                        .map(|e| {
                            let glyph = glyph_url(e.piece_type);
                            let title = format!("{} \u{00d7}{}", type_label(e.piece_type), e.count);
                            let piece: PieceId = e.example;
                            let is_selected = sel == Some(Selection::Hand(piece));
                            // Clickable only for the side to move.
                            let on_click = move |_| {
                                if !session.get_untracked().is_over() {
                                    let side = session.with_untracked(|s| s.state().side_to_move());
                                    let next = hand_click(selection.get_untracked(), piece, side);
                                    selection.set(next);
                                }
                            };
                            let mut chip_class = String::from("hand-piece");
                            if active {
                                chip_class.push_str(" selectable");
                            }
                            if is_selected {
                                chip_class.push_str(" selected");
                            }
                            let mut tile_class = String::from("hand-tile");
                            if is_selected {
                                tile_class.push_str(" selected");
                            }
                            let glyph = glyph.clone();
                            view! {
                                <button
                                    class=chip_class
                                    title=title
                                    disabled=!active
                                    on:click=on_click
                                >
                                    <div
                                        class=tile_class
                                        style=format!(
                                            "background:{fill};border-color:{stroke};color:{glyph_col};",
                                        )
                                    >
                                        {glyph
                                            .map(|url| {
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
                                </button>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </section>
    }
}

/// Read-only outcome line shown when the game is over.
#[component]
fn OutcomeBanner(outcome: Outcome) -> impl IntoView {
    let text = match outcome {
        Outcome::Win(Color::White) => "White wins".to_string(),
        Outcome::Win(Color::Black) => "Black wins".to_string(),
        Outcome::Draw => "Draw".to_string(),
    };
    view! { <section class="outcome" aria-live="polite">{text}</section> }
}

// Keep `Point` referenced even if a future refactor drops the only use; the
// pixel→user conversion above returns it.
#[allow(dead_code)]
fn _uses_point(p: Point) -> f64 {
    p.x
}
