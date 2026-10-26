//! Exact-integer transforms. Quarter-turn rotations and mirrors never touch
//! floating point, so schematic and footprint geometry stays on-grid.

use crate::units::{Coord, Point};

/// Normalizes an angle in degrees to `[0, 360)`.
pub fn normalize_deg(deg: f64) -> f64 {
    let d = deg.rem_euclid(360.0);
    // Snap float noise (e.g. 89.99999999) so quarter turns stay exact.
    let r = d.round();
    let d = if (d - r).abs() < 1e-9 { r } else { d };
    if d >= 360.0 { 0.0 } else { d }
}

/// Rotates `p` counter-clockwise by `deg` around `c` (world Y up).
pub fn rotate(p: Point, c: Point, deg: f64) -> Point {
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    let (rx, ry) = match normalize_deg(deg) {
        0.0 => (dx, dy),
        90.0 => (-dy, dx),
        180.0 => (-dx, -dy),
        270.0 => (dy, -dx),
        d => {
            let (s, cs) = d.to_radians().sin_cos();
            let (fx, fy) = (dx as f64, dy as f64);
            ((fx * cs - fy * s).round() as Coord, (fx * s + fy * cs).round() as Coord)
        }
    };
    Point::new(c.x + rx, c.y + ry)
}

/// Mirrors `p` across the vertical line `x = c.x`.
pub fn mirror_x(p: Point, c: Point) -> Point {
    Point::new(2 * c.x - p.x, p.y)
}

/// Placement of a symbol or footprint: local → world is
/// `offset + rotate(mirror(local), rotation)`; mirroring is about the local Y axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub offset: Point,
    pub rotation: f64,
    pub mirror: bool,
}

impl Placement {
    pub fn apply(&self, local: Point) -> Point {
        let m = if self.mirror { Point::new(-local.x, local.y) } else { local };
        rotate(m, Point::default(), self.rotation).translated(self.offset.x, self.offset.y)
    }

    /// World direction (degrees) of a local direction.
    pub fn apply_angle(&self, deg: f64) -> f64 {
        let d = if self.mirror { 180.0 - deg } else { deg };
        normalize_deg(d + self.rotation)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BBox {
    pub min: Point,
    pub max: Point,
}

impl BBox {
    pub fn from_points(points: impl IntoIterator<Item = Point>) -> Option<BBox> {
        let mut it = points.into_iter();
        let first = it.next()?;
        Some(it.fold(BBox { min: first, max: first }, |b, p| b.including(p)))
    }

    pub fn including(self, p: Point) -> BBox {
        BBox {
            min: Point::new(self.min.x.min(p.x), self.min.y.min(p.y)),
            max: Point::new(self.max.x.max(p.x), self.max.y.max(p.y)),
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

/// Whether `p` lies on the closed segment `a`–`b` (exact integer test).
pub fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let cross = (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
    cross == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_are_exact() {
        let c = Point::new(10, 10);
        assert_eq!(rotate(Point::new(20, 10), c, 90.0), Point::new(10, 20));
        assert_eq!(rotate(Point::new(20, 10), c, -90.0), Point::new(10, 0));
        assert_eq!(rotate(Point::new(20, 10), c, 450.0), Point::new(10, 20));
    }

    #[test]
    fn placement_mirrors_before_rotating() {
        let p = Placement { offset: Point::new(100, 0), rotation: 90.0, mirror: true };
        // local (10, 0) → mirror (-10, 0) → rotate 90 (0, -10) → offset (100, -10)
        assert_eq!(p.apply(Point::new(10, 0)), Point::new(100, -10));
        assert_eq!(p.apply_angle(0.0), 270.0);
    }

    #[test]
    fn segment_membership() {
        assert!(on_segment(Point::new(5, 5), Point::new(0, 0), Point::new(10, 10)));
        assert!(!on_segment(Point::new(11, 11), Point::new(0, 0), Point::new(10, 10)));
        assert!(!on_segment(Point::new(5, 6), Point::new(0, 0), Point::new(10, 10)));
    }
}
