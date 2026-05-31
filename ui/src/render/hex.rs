//! Pure hex geometry for a pointy-top axial layout, plus the reactive-viewBox
//! math (fit / zoom-about-point / pan-delta) and the frontier-cell helper. No
//! DOM / Leptos here — everything is native-testable so the coordinate
//! transform and view math have unit tests rather than relying on eyeballing
//! the rendered SVG.
//!
//! Pointy-top orientation: hexes have a pointed top/bottom corner and flat
//! left/right edges. For axial `(q, r)` with the engine's convention
//! (`coord.rs`) the standard pointy-top layout is:
//!
//! ```text
//! x = size * sqrt(3) * (q + r / 2)
//! y = size * 1.5 * r
//! ```
//!
//! `size` is the hex "radius" (centre to a corner). The width of a pointy-top
//! hex is `sqrt(3) * size`; the height is `2 * size`. Horizontal spacing
//! between adjacent columns is `sqrt(3) * size`; vertical spacing between rows
//! is `1.5 * size`.

use hive_engine::{Coord, State};
use std::collections::BTreeSet;

/// sqrt(3), precomputed.
pub const SQRT3: f64 = 1.732_050_807_568_877_2;

/// A 2D point in SVG user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Centre of the hex at axial `coord`, in SVG user units, for a pointy-top
/// layout with the given hex `size` (centre-to-corner radius).
pub fn axial_to_pixel(coord: Coord, size: f64) -> Point {
    let q = coord.q as f64;
    let r = coord.r as f64;
    Point {
        x: size * SQRT3 * (q + r / 2.0),
        y: size * 1.5 * r,
    }
}

/// The six corner points of a pointy-top hex centred at `centre` with the given
/// `size`. Corner `i` is at angle `60° * i + 30°`, which places the first corner
/// up-and-to-the-right and the topmost/bottommost corners on the vertical axis,
/// yielding pointed top/bottom and flat left/right edges.
pub fn hex_corners(centre: Point, size: f64) -> [Point; 6] {
    let mut out = [Point { x: 0.0, y: 0.0 }; 6];
    for (i, p) in out.iter_mut().enumerate() {
        // 30,90,150,210,270,330 degrees.
        let angle = std::f64::consts::PI / 3.0 * i as f64 + std::f64::consts::PI / 6.0;
        *p = Point {
            x: centre.x + size * angle.cos(),
            y: centre.y + size * angle.sin(),
        };
    }
    out
}

/// SVG `points` attribute string ("x,y x,y ...") for a pointy-top hex polygon.
pub fn hex_polygon_points(centre: Point, size: f64) -> String {
    let corners = hex_corners(centre, size);
    let mut s = String::new();
    for (i, c) in corners.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        // Trim to 3 decimals to keep the DOM small; SVG tolerates it fine.
        s.push_str(&format!("{:.3},{:.3}", c.x, c.y));
    }
    s
}

/// Inverse of [`axial_to_pixel`]: the axial [`Coord`] whose hex contains the
/// user-space point `p`, for a pointy-top layout with hex `size`.
///
/// Solves the forward transform for fractional `(q, r)`, then rounds to the
/// nearest hex via cube rounding (the standard hex-grid technique: round the
/// three cube coords, then fix up whichever drifted most so they re-sum to
/// zero). A click anywhere inside a hex maps to that hex's coord.
pub fn pixel_to_axial(p: Point, size: f64) -> Coord {
    // Forward: x = size*sqrt3*(q + r/2); y = size*1.5*r.
    let r = p.y / (size * 1.5);
    let q = p.x / (size * SQRT3) - r / 2.0;
    round_axial(q, r)
}

/// Round fractional axial `(q, r)` to the nearest hex using cube rounding.
fn round_axial(q: f64, r: f64) -> Coord {
    // Axial -> cube: x=q, z=r, y=-x-z.
    let x = q;
    let z = r;
    let y = -x - z;
    let mut rx = x.round();
    let ry = y.round();
    let mut rz = z.round();
    let dx = (rx - x).abs();
    let dy = (ry - y).abs();
    let dz = (rz - z).abs();
    // Re-derive whichever of the two coords we keep (`rx`, `rz`) drifted most,
    // so x+y+z == 0 holds. We only ever read `rx` and `rz` for the axial result,
    // so `ry` never needs fixing up.
    if dx > dy && dx > dz {
        rx = -ry - rz;
    } else if dy <= dz {
        rz = -rx - ry;
    }
    Coord::new(rx as i16, rz as i16)
}

/// An axis-aligned bounding box in SVG user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BBox {
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }
}

/// A viewBox: the rectangle of user space the SVG maps to its pixel box. Hex
/// `size` stays constant; this rectangle is what zoom and pan mutate. With
/// `preserveAspectRatio="xMidYMid meet"` the rendered scale is governed by the
/// ratio of this rectangle to the SVG's pixel size, so a *small* viewBox zooms
/// in and a *large* one zooms out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBox {
    pub min_x: f64,
    pub min_y: f64,
    pub w: f64,
    pub h: f64,
}

impl ViewBox {
    /// Format as an SVG `viewBox` attribute: "min_x min_y w h".
    pub fn attr(&self) -> String {
        format!(
            "{:.3} {:.3} {:.3} {:.3}",
            self.min_x, self.min_y, self.w, self.h
        )
    }

    /// The centre point of the viewBox in user units.
    pub fn center(&self) -> Point {
        Point {
            x: self.min_x + self.w / 2.0,
            y: self.min_y + self.h / 2.0,
        }
    }
}

/// Bounding box that encloses every hex tile centred at the given coords,
/// padded by `size` on each side so the tiles' corners are not clipped, plus an
/// extra `margin` (in user units) of breathing room. Returns `None` if `coords`
/// is empty (caller draws an empty-board placeholder instead).
pub fn fit_bbox(coords: &[Coord], size: f64, margin: f64) -> Option<BBox> {
    let mut iter = coords.iter().copied();
    let first = iter.next()?;
    let p0 = axial_to_pixel(first, size);
    let mut min_x = p0.x;
    let mut max_x = p0.x;
    let mut min_y = p0.y;
    let mut max_y = p0.y;
    for c in iter {
        let p = axial_to_pixel(c, size);
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    // A pointy-top hex extends `sqrt(3)/2 * size` horizontally and `size`
    // vertically from its centre. Pad by the larger (`size`) on every side for
    // simplicity, then add the requested margin.
    let pad = size + margin;
    Some(BBox {
        min_x: min_x - pad,
        min_y: min_y - pad,
        max_x: max_x + pad,
        max_y: max_y + pad,
    })
}

/// The default "fit" viewBox for a set of coords at a *fixed* hex `size`.
///
/// Crucially this does NOT stretch the hive to fill the panel: the hex size is
/// constant, so a small early-game position yields a small content bbox. We
/// center that bbox in a viewBox that is at least `min_extent` user units on
/// each axis, so a lone piece (or a few) renders at its natural size, centered,
/// rather than blown up. `preserveAspectRatio=meet` then letterboxes it into
/// whatever pixel box the panel has. Returns `None` for empty coords.
pub fn fit_viewbox(coords: &[Coord], size: f64, margin: f64, min_extent: f64) -> Option<ViewBox> {
    let b = fit_bbox(coords, size, margin)?;
    let cx = (b.min_x + b.max_x) / 2.0;
    let cy = (b.min_y + b.max_y) / 2.0;
    // Use a square viewBox sized to the larger content axis but never below
    // `min_extent`, so small positions stay at natural scale and don't zoom in.
    let extent = b.width().max(b.height()).max(min_extent);
    Some(ViewBox {
        min_x: cx - extent / 2.0,
        min_y: cy - extent / 2.0,
        w: extent,
        h: extent,
    })
}

/// Zoom the viewBox about a fixed user-space point (the point under the cursor),
/// keeping that point stationary on screen.
///
/// `factor < 1` zooms in (shrinks the viewBox), `factor > 1` zooms out. The
/// effective zoom relative to a reference `base_extent` (the fit extent) is
/// clamped to `[min_zoom, max_zoom]`, where zoom is `base_extent / viewBox.w`.
/// We keep the viewBox square (w == h driven by `factor`), which is correct here
/// because the fit viewBox is square and pan never changes the aspect.
pub fn zoom_about(
    vb: ViewBox,
    anchor: Point,
    factor: f64,
    base_extent: f64,
    min_zoom: f64,
    max_zoom: f64,
) -> ViewBox {
    let mut new_w = vb.w * factor;
    let mut new_h = vb.h * factor;
    // Clamp by zoom = base_extent / extent. Larger extent => smaller zoom.
    let min_extent = base_extent / max_zoom; // most zoomed in
    let max_extent = base_extent / min_zoom; // most zoomed out
    if new_w < min_extent {
        let s = min_extent / new_w;
        new_w *= s;
        new_h *= s;
    } else if new_w > max_extent {
        let s = max_extent / new_w;
        new_w *= s;
        new_h *= s;
    }
    // Keep `anchor` at the same fractional position within the viewBox.
    let fx = (anchor.x - vb.min_x) / vb.w;
    let fy = (anchor.y - vb.min_y) / vb.h;
    ViewBox {
        min_x: anchor.x - fx * new_w,
        min_y: anchor.y - fy * new_h,
        w: new_w,
        h: new_h,
    }
}

/// Pan the viewBox by a pixel delta. `dx_px`/`dy_px` are pointer-movement
/// pixels in the SVG element; `svg_w_px`/`svg_h_px` are the element's pixel
/// size. The delta is converted to user units via the current viewBox extent so
/// the content tracks the cursor 1:1. Dragging right should move the content
/// right (i.e. the viewBox shifts left), hence the subtraction.
pub fn pan_by_pixels(
    vb: ViewBox,
    dx_px: f64,
    dy_px: f64,
    svg_w_px: f64,
    svg_h_px: f64,
) -> ViewBox {
    let ux = if svg_w_px > 0.0 {
        dx_px * vb.w / svg_w_px
    } else {
        0.0
    };
    let uy = if svg_h_px > 0.0 {
        dy_px * vb.h / svg_h_px
    } else {
        0.0
    };
    ViewBox {
        min_x: vb.min_x - ux,
        min_y: vb.min_y - uy,
        w: vb.w,
        h: vb.h,
    }
}

/// Convert a pixel position within the SVG element into a user-space point
/// under the current viewBox, assuming `preserveAspectRatio="xMidYMid meet"`.
/// `meet` scales uniformly by the *smaller* axis ratio and centers the excess,
/// so we account for the letterbox offset on the larger pixel axis.
pub fn client_to_user(
    vb: ViewBox,
    px: f64,
    py: f64,
    svg_w_px: f64,
    svg_h_px: f64,
) -> Point {
    if svg_w_px <= 0.0 || svg_h_px <= 0.0 {
        return vb.center();
    }
    // `meet`: uniform scale = min over axes of (pixels / user-units).
    let scale = (svg_w_px / vb.w).min(svg_h_px / vb.h);
    // The drawn content occupies `vb.w * scale` x `vb.h * scale` pixels,
    // centered in the element; the rest is letterbox padding.
    let draw_w = vb.w * scale;
    let draw_h = vb.h * scale;
    let off_x = (svg_w_px - draw_w) / 2.0;
    let off_y = (svg_h_px - draw_h) / 2.0;
    Point {
        x: vb.min_x + (px - off_x) / scale,
        y: vb.min_y + (py - off_y) / scale,
    }
}

/// The frontier coordinate set for `state`: every occupied cell plus all of
/// their neighbours (empty cells adjacent to the hive). Rendering these as
/// faint outlined cells behind the occupied ones produces the honeycomb look.
///
/// In a later PR this set will be unioned with the current legal targets so the
/// highlighted move destinations also get a frontier cell. Sorted/deduped via a
/// `BTreeSet` for deterministic output.
pub fn frontier_coords(state: &State) -> Vec<Coord> {
    let mut set: BTreeSet<(i16, i16)> = BTreeSet::new();
    for (c, _) in state.entries() {
        set.insert((c.q, c.r));
        for n in c.neighbours() {
            set.insert((n.q, n.r));
        }
    }
    set.into_iter().map(|(q, r)| Coord::new(q, r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_engine::Coord;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn origin_maps_to_origin() {
        let p = axial_to_pixel(Coord::new(0, 0), 10.0);
        approx(p.x, 0.0);
        approx(p.y, 0.0);
    }

    #[test]
    fn pointy_top_row_spacing_is_1_5_size() {
        // Moving +1 in r (a row down) shifts y by 1.5*size and x by
        // sqrt(3)/2*size (half a column right because of the r/2 term).
        let a = axial_to_pixel(Coord::new(0, 0), 10.0);
        let b = axial_to_pixel(Coord::new(0, 1), 10.0);
        approx(b.y - a.y, 15.0);
        approx(b.x - a.x, 10.0 * SQRT3 / 2.0);
    }

    #[test]
    fn pointy_top_column_spacing_is_sqrt3_size() {
        // Moving +1 in q (same row) shifts x by sqrt(3)*size, y unchanged.
        let a = axial_to_pixel(Coord::new(0, 0), 10.0);
        let b = axial_to_pixel(Coord::new(1, 0), 10.0);
        approx(b.x - a.x, 10.0 * SQRT3);
        approx(b.y - a.y, 0.0);
    }

    #[test]
    fn engine_neighbours_are_equidistant() {
        // All six neighbour cells of the origin should be exactly one hex step
        // (sqrt(3)*size in pixel space) away — sanity that our transform agrees
        // with the engine's neighbour deltas.
        let size = 12.0;
        let centre = axial_to_pixel(Coord::ORIGIN, size);
        let expected = size * SQRT3;
        for n in Coord::ORIGIN.neighbours() {
            let p = axial_to_pixel(n, size);
            let d = ((p.x - centre.x).powi(2) + (p.y - centre.y).powi(2)).sqrt();
            approx(d, expected);
        }
    }

    #[test]
    fn hex_has_six_distinct_corners() {
        let corners = hex_corners(Point { x: 0.0, y: 0.0 }, 10.0);
        for i in 0..6 {
            for j in (i + 1)..6 {
                let d = ((corners[i].x - corners[j].x).powi(2)
                    + (corners[i].y - corners[j].y).powi(2))
                .sqrt();
                assert!(d > 1e-6, "corners {i} and {j} coincide");
            }
        }
    }

    #[test]
    fn pointy_top_corners_have_vertical_extremes() {
        // Pointy-top: topmost and bottommost corners lie on the vertical axis
        // through the centre. Corner 1 is at +90° (straight down in SVG's
        // y-down space) and corner 4 at -90° (straight up).
        let c = hex_corners(Point { x: 0.0, y: 0.0 }, 10.0);
        approx(c[1].x, 0.0);
        approx(c[1].y, 10.0);
        approx(c[4].x, 0.0);
        approx(c[4].y, -10.0);
        // And the left/right edges are flat: corners 0 and 5 share the same
        // (max) x; corners 2 and 3 share the same (min) x.
        approx(c[0].x, c[5].x);
        approx(c[2].x, c[3].x);
    }

    #[test]
    fn polygon_points_has_six_pairs() {
        let s = hex_polygon_points(Point { x: 0.0, y: 0.0 }, 10.0);
        assert_eq!(s.split(' ').count(), 6);
        for pair in s.split(' ') {
            assert_eq!(pair.split(',').count(), 2);
        }
    }

    #[test]
    fn empty_coords_yield_no_bbox_or_viewbox() {
        assert!(fit_bbox(&[], 10.0, 4.0).is_none());
        assert!(fit_viewbox(&[], 10.0, 4.0, 100.0).is_none());
    }

    #[test]
    fn single_cell_bbox_is_centred_and_padded() {
        let b = fit_bbox(&[Coord::ORIGIN], 10.0, 4.0).unwrap();
        // Origin maps to (0,0); pad = size+margin = 14 on every side.
        approx(b.min_x, -14.0);
        approx(b.min_y, -14.0);
        approx(b.max_x, 14.0);
        approx(b.max_y, 14.0);
        approx(b.width(), 28.0);
        approx(b.height(), 28.0);
    }

    #[test]
    fn bbox_encloses_all_centres() {
        let coords = [
            Coord::new(0, 0),
            Coord::new(2, -1),
            Coord::new(-1, 3),
            Coord::new(1, 1),
        ];
        let b = fit_bbox(&coords, 10.0, 4.0).unwrap();
        for c in coords {
            let p = axial_to_pixel(c, 10.0);
            assert!(p.x >= b.min_x && p.x <= b.max_x, "x {} out of box", p.x);
            assert!(p.y >= b.min_y && p.y <= b.max_y, "y {} out of box", p.y);
        }
    }

    #[test]
    fn fit_viewbox_respects_min_extent_for_small_positions() {
        // A single cell has a 28-unit content bbox, but min_extent forces a
        // larger square so the lone piece renders small/centered, not zoomed in.
        let vb = fit_viewbox(&[Coord::ORIGIN], 10.0, 4.0, 200.0).unwrap();
        approx(vb.w, 200.0);
        approx(vb.h, 200.0);
        // Centered on the origin.
        approx(vb.center().x, 0.0);
        approx(vb.center().y, 0.0);
    }

    #[test]
    fn fit_viewbox_grows_with_content() {
        // A spread-out position whose content exceeds min_extent uses the
        // content extent instead.
        let coords = [Coord::new(-5, 0), Coord::new(5, 0)];
        let b = fit_bbox(&coords, 10.0, 4.0).unwrap();
        let vb = fit_viewbox(&coords, 10.0, 4.0, 50.0).unwrap();
        approx(vb.w, b.width().max(b.height()));
        assert!(vb.w > 50.0);
    }

    #[test]
    fn viewbox_attr_is_four_numbers() {
        let vb = ViewBox {
            min_x: -1.0,
            min_y: -2.0,
            w: 4.0,
            h: 7.0,
        };
        assert_eq!(vb.attr(), "-1.000 -2.000 4.000 7.000");
    }

    #[test]
    fn zoom_in_keeps_anchor_fixed() {
        let vb = ViewBox { min_x: 0.0, min_y: 0.0, w: 100.0, h: 100.0 };
        let anchor = Point { x: 25.0, y: 75.0 };
        // factor 0.5 -> zoom in 2x. base_extent 100, zoom clamps wide open.
        let z = zoom_about(vb, anchor, 0.5, 100.0, 0.1, 10.0);
        approx(z.w, 50.0);
        approx(z.h, 50.0);
        // Anchor stays at the same fractional position (0.25, 0.75).
        let fx = (anchor.x - z.min_x) / z.w;
        let fy = (anchor.y - z.min_y) / z.h;
        approx(fx, 0.25);
        approx(fy, 0.75);
    }

    #[test]
    fn zoom_out_keeps_anchor_fixed() {
        let vb = ViewBox { min_x: 10.0, min_y: 20.0, w: 40.0, h: 40.0 };
        let anchor = Point { x: 30.0, y: 40.0 };
        let z = zoom_about(vb, anchor, 2.0, 40.0, 0.1, 10.0);
        approx(z.w, 80.0);
        let fx = (anchor.x - z.min_x) / z.w;
        let fy = (anchor.y - z.min_y) / z.h;
        approx(fx, (anchor.x - vb.min_x) / vb.w);
        approx(fy, (anchor.y - vb.min_y) / vb.h);
    }

    #[test]
    fn zoom_clamps_at_max_zoom_in() {
        // base_extent 100, max_zoom 4 => min extent 25. Try to zoom way past it.
        let vb = ViewBox { min_x: 0.0, min_y: 0.0, w: 100.0, h: 100.0 };
        let anchor = vb.center();
        let z = zoom_about(vb, anchor, 0.01, 100.0, 0.3, 4.0);
        approx(z.w, 25.0);
    }

    #[test]
    fn zoom_clamps_at_min_zoom_out() {
        // base_extent 100, min_zoom 0.3 => max extent ~333.33.
        let vb = ViewBox { min_x: 0.0, min_y: 0.0, w: 100.0, h: 100.0 };
        let anchor = vb.center();
        let z = zoom_about(vb, anchor, 100.0, 100.0, 0.3, 4.0);
        approx(z.w, 100.0 / 0.3);
    }

    #[test]
    fn pan_converts_pixels_to_user_units() {
        // viewBox 200 wide over a 400px svg => 0.5 user units per pixel.
        let vb = ViewBox { min_x: 0.0, min_y: 0.0, w: 200.0, h: 200.0 };
        let p = pan_by_pixels(vb, 40.0, -20.0, 400.0, 400.0);
        // Drag right 40px -> content right -> viewBox shifts left by 20 units.
        approx(p.min_x, -20.0);
        approx(p.min_y, 10.0);
        approx(p.w, 200.0);
        approx(p.h, 200.0);
    }

    #[test]
    fn client_to_user_center_roundtrips() {
        // Square viewBox over square pixels: center pixel maps to viewBox center.
        let vb = ViewBox { min_x: -50.0, min_y: -50.0, w: 100.0, h: 100.0 };
        let u = client_to_user(vb, 150.0, 150.0, 300.0, 300.0);
        approx(u.x, 0.0);
        approx(u.y, 0.0);
        // Corner pixel (0,0) maps to viewBox min corner.
        let c = client_to_user(vb, 0.0, 0.0, 300.0, 300.0);
        approx(c.x, -50.0);
        approx(c.y, -50.0);
    }

    #[test]
    fn client_to_user_accounts_for_letterbox() {
        // Square viewBox in a wide (400x200) box: meet scales by the height
        // ratio (200/100 = 2), drawn content is 200px wide, centered with 100px
        // letterbox each side. So pixel x=100 is the left edge of content.
        let vb = ViewBox { min_x: 0.0, min_y: 0.0, w: 100.0, h: 100.0 };
        let u = client_to_user(vb, 100.0, 0.0, 400.0, 200.0);
        approx(u.x, 0.0);
        approx(u.y, 0.0);
        // Center pixel maps to viewBox center.
        let c = client_to_user(vb, 200.0, 100.0, 400.0, 200.0);
        approx(c.x, 50.0);
        approx(c.y, 50.0);
    }

    #[test]
    fn pixel_to_axial_inverts_axial_to_pixel() {
        // Every cell's centre must round-trip back to that cell.
        let size = 17.0;
        for q in -6..=6 {
            for r in -6..=6 {
                let c = Coord::new(q, r);
                let centre = axial_to_pixel(c, size);
                assert_eq!(pixel_to_axial(centre, size), c, "centre of {c:?}");
            }
        }
    }

    #[test]
    fn pixel_to_axial_maps_interior_points_to_the_hex() {
        // A point nudged a fraction toward each neighbour from a hex centre must
        // still resolve to the original hex (it stays inside).
        let size = 20.0;
        let c = Coord::new(2, -1);
        let centre = axial_to_pixel(c, size);
        for n in c.neighbours() {
            let np = axial_to_pixel(n, size);
            // 30% of the way toward the neighbour: still inside `c`'s hex.
            let p = Point {
                x: centre.x + 0.3 * (np.x - centre.x),
                y: centre.y + 0.3 * (np.y - centre.y),
            };
            assert_eq!(pixel_to_axial(p, size), c, "30% toward {n:?}");
        }
    }

    #[test]
    fn frontier_includes_occupied_and_neighbours() {
        use crate::game::Session;
        // Place one piece, then check the frontier is that cell plus its 6
        // neighbours = 7 cells.
        let mut s = Session::new();
        let m = s.state().legal_moves()[0];
        s.push_move(m).unwrap();
        let f = frontier_coords(s.state());
        // The single occupied cell must be present.
        let occ: Vec<_> = s.state().entries().map(|(c, _)| c).collect();
        assert_eq!(occ.len(), 1);
        let o = occ[0];
        assert!(f.contains(&o), "frontier must include occupied cell");
        for n in o.neighbours() {
            assert!(f.contains(&n), "frontier must include neighbour {n:?}");
        }
        assert_eq!(f.len(), 7, "one piece -> 7 frontier cells, got {}", f.len());
    }

    #[test]
    fn frontier_is_empty_for_empty_board() {
        use crate::game::Session;
        let s = Session::new();
        assert!(frontier_coords(s.state()).is_empty());
    }
}
