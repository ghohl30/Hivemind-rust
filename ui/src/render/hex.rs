//! Pure hex geometry for a flat-top axial layout. No DOM / Leptos — fully
//! native-testable so the coordinate transform and viewBox math have unit tests
//! rather than relying on eyeballing the rendered SVG.
//!
//! Flat-top orientation: hexes have a flat edge on top/bottom and pointed
//! left/right corners. For axial `(q, r)` with the engine's convention
//! (`coord.rs`), the standard flat-top layout is:
//!
//! ```text
//! x = size * 1.5 * q
//! y = size * sqrt(3) * (r + q / 2)
//! ```
//!
//! `size` is the hex "radius" (centre to a corner). The width of a flat-top hex
//! is `2 * size`; the height is `sqrt(3) * size`. Horizontal spacing between
//! adjacent columns is `1.5 * size`; vertical spacing is `sqrt(3) * size`.

use hive_engine::Coord;

/// sqrt(3), precomputed.
pub const SQRT3: f64 = 1.732_050_807_568_877_2;

/// A 2D point in SVG user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Centre of the hex at axial `coord`, in SVG user units, for a flat-top layout
/// with the given hex `size` (centre-to-corner radius).
pub fn axial_to_pixel(coord: Coord, size: f64) -> Point {
    let q = coord.q as f64;
    let r = coord.r as f64;
    Point {
        x: size * 1.5 * q,
        y: size * SQRT3 * (r + q / 2.0),
    }
}

/// The six corner points of a flat-top hex centred at `centre` with the given
/// `size`. Corner `i` is at angle `60° * i` (0° = +x axis), which places the
/// first corner at the rightmost point and yields flat top/bottom edges.
pub fn hex_corners(centre: Point, size: f64) -> [Point; 6] {
    let mut out = [Point { x: 0.0, y: 0.0 }; 6];
    for (i, p) in out.iter_mut().enumerate() {
        let angle = std::f64::consts::PI / 3.0 * i as f64; // 0,60,120,...
        *p = Point {
            x: centre.x + size * angle.cos(),
            y: centre.y + size * angle.sin(),
        };
    }
    out
}

/// SVG `points` attribute string ("x,y x,y ...") for a flat-top hex polygon.
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

/// Bounding box that encloses every hex tile centred at the given coords,
/// padded by `size` on each side so the tiles' corners are not clipped, plus an
/// extra `margin` (in user units) of breathing room. Returns `None` if `coords`
/// is empty (caller draws an empty-board placeholder instead).
pub fn fit_viewbox(coords: &[Coord], size: f64, margin: f64) -> Option<BBox> {
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
    // A flat-top hex extends `size` horizontally and `sqrt(3)/2 * size`
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

/// Format a `BBox` as an SVG `viewBox` attribute: "min_x min_y width height".
pub fn viewbox_attr(b: &BBox) -> String {
    format!(
        "{:.3} {:.3} {:.3} {:.3}",
        b.min_x,
        b.min_y,
        b.width(),
        b.height()
    )
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
    fn flat_top_column_spacing_is_1_5_size() {
        // Moving +1 in q (East-ish column) shifts x by 1.5*size and y by
        // sqrt(3)/2*size (half a row down because of the q/2 term).
        let a = axial_to_pixel(Coord::new(0, 0), 10.0);
        let b = axial_to_pixel(Coord::new(1, 0), 10.0);
        approx(b.x - a.x, 15.0);
        approx(b.y - a.y, 10.0 * SQRT3 / 2.0);
    }

    #[test]
    fn flat_top_row_spacing_is_sqrt3_size() {
        // Moving +1 in r (same column) shifts y by sqrt(3)*size, x unchanged.
        let a = axial_to_pixel(Coord::new(0, 0), 10.0);
        let b = axial_to_pixel(Coord::new(0, 1), 10.0);
        approx(b.x - a.x, 0.0);
        approx(b.y - a.y, 10.0 * SQRT3);
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
    fn flat_top_corners_have_horizontal_extremes() {
        // Flat-top: leftmost and rightmost corners lie on the horizontal axis
        // through the centre (corner 0 at +x, corner 3 at -x).
        let c = hex_corners(Point { x: 0.0, y: 0.0 }, 10.0);
        approx(c[0].x, 10.0);
        approx(c[0].y, 0.0);
        approx(c[3].x, -10.0);
        approx(c[3].y, 0.0);
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
    fn empty_coords_yield_no_viewbox() {
        assert!(fit_viewbox(&[], 10.0, 4.0).is_none());
    }

    #[test]
    fn single_cell_viewbox_is_centred_and_padded() {
        let b = fit_viewbox(&[Coord::ORIGIN], 10.0, 4.0).unwrap();
        // Origin maps to (0,0); pad = size+margin = 14 on every side.
        approx(b.min_x, -14.0);
        approx(b.min_y, -14.0);
        approx(b.max_x, 14.0);
        approx(b.max_y, 14.0);
        approx(b.width(), 28.0);
        approx(b.height(), 28.0);
    }

    #[test]
    fn viewbox_encloses_all_centres() {
        let coords = [
            Coord::new(0, 0),
            Coord::new(2, -1),
            Coord::new(-1, 3),
            Coord::new(1, 1),
        ];
        let b = fit_viewbox(&coords, 10.0, 4.0).unwrap();
        for c in coords {
            let p = axial_to_pixel(c, 10.0);
            assert!(p.x >= b.min_x && p.x <= b.max_x, "x {} out of box", p.x);
            assert!(p.y >= b.min_y && p.y <= b.max_y, "y {} out of box", p.y);
        }
    }

    #[test]
    fn viewbox_attr_is_four_numbers() {
        let b = BBox {
            min_x: -1.0,
            min_y: -2.0,
            max_x: 3.0,
            max_y: 5.0,
        };
        assert_eq!(viewbox_attr(&b), "-1.000 -2.000 4.000 7.000");
    }
}
