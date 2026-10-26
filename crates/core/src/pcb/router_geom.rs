//! Router geometry primitives: float vectors (nm), segment/polygon distances,
//! octilinear helpers and corner-style paths. Board data stays integer; the
//! router converts at its boundary and rounds results back to nm.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::{Add, Mul, Neg, Sub};

use crate::units::{Coord, Point};

use super::CornerStyle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct V {
    pub x: f64,
    pub y: f64,
}

impl V {
    pub const fn new(x: f64, y: f64) -> V {
        V { x, y }
    }
    pub fn of(p: Point) -> V {
        V::new(p.x as f64, p.y as f64)
    }
    pub fn round(self) -> Point {
        Point::new(self.x.round() as Coord, self.y.round() as Coord)
    }
    pub fn dot(self, o: V) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: V) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn norm(self) -> V {
        let l = self.len();
        if l == 0.0 { V::default() } else { V::new(self.x / l, self.y / l) }
    }
    /// Left normal (rotated +90°).
    pub fn perp(self) -> V {
        V::new(-self.y, self.x)
    }
}

impl Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V {
    type Output = V;
    fn mul(self, k: f64) -> V {
        V::new(self.x * k, self.y * k)
    }
}
impl Neg for V {
    type Output = V;
    fn neg(self) -> V {
        V::new(-self.x, -self.y)
    }
}

// ---------- fast hashing for the search maps ----------

#[derive(Default, Clone, Copy)]
pub(super) struct Fx(u64);

impl Fx {
    #[inline]
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.add(*b as u64);
        }
    }
    fn write_u8(&mut self, v: u8) {
        self.add(v as u64);
    }
    fn write_u32(&mut self, v: u32) {
        self.add(v as u64);
    }
    fn write_i32(&mut self, v: i32) {
        self.add(v as u32 as u64);
    }
    fn write_u64(&mut self, v: u64) {
        self.add(v);
    }
    fn write_usize(&mut self, v: usize) {
        self.add(v as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(super) type FxMap<K, T> = HashMap<K, T, BuildHasherDefault<Fx>>;
pub(super) type FxSet<K> = HashSet<K, BuildHasherDefault<Fx>>;

// ---------- distances ----------

pub(super) fn point_seg_dist(p: V, a: V, b: V) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 == 0.0 {
        return (p - a).len();
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).len()
}

fn orient(a: V, b: V, c: V) -> f64 {
    (b - a).cross(c - a)
}

fn on_seg(a: V, b: V, p: V) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Whether the closed segments intersect (touching counts).
pub(super) fn segs_intersect(a0: V, a1: V, b0: V, b1: V) -> bool {
    let d1 = orient(b0, b1, a0);
    let d2 = orient(b0, b1, a1);
    let d3 = orient(a0, a1, b0);
    let d4 = orient(a0, a1, b1);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && on_seg(b0, b1, a0)) || (d2 == 0.0 && on_seg(b0, b1, a1)) || (d3 == 0.0 && on_seg(a0, a1, b0)) || (d4 == 0.0 && on_seg(a0, a1, b1))
}

pub(super) fn seg_seg_dist(a0: V, a1: V, b0: V, b1: V) -> f64 {
    if segs_intersect(a0, a1, b0, b1) {
        return 0.0;
    }
    point_seg_dist(a0, b0, b1).min(point_seg_dist(a1, b0, b1)).min(point_seg_dist(b0, a0, a1)).min(point_seg_dist(b1, a0, a1))
}

/// Convex polygon (either winding) containment, boundary included.
pub(super) fn convex_contains(poly: &[V], p: V) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let (mut pos, mut neg) = (false, false);
    for i in 0..n {
        let c = orient(poly[i], poly[(i + 1) % n], p);
        if c > 0.0 {
            pos = true;
        } else if c < 0.0 {
            neg = true;
        }
        if pos && neg {
            return false;
        }
    }
    true
}

pub(super) fn point_poly_dist(p: V, poly: &[V]) -> f64 {
    if convex_contains(poly, p) {
        return 0.0;
    }
    let n = poly.len();
    (0..n).map(|i| point_seg_dist(p, poly[i], poly[(i + 1) % n])).fold(f64::INFINITY, f64::min)
}

pub(super) fn seg_poly_dist(a: V, b: V, poly: &[V]) -> f64 {
    if convex_contains(poly, a) || convex_contains(poly, b) {
        return 0.0;
    }
    let n = poly.len();
    (0..n).map(|i| seg_seg_dist(a, b, poly[i], poly[(i + 1) % n])).fold(f64::INFINITY, f64::min)
}

/// Liang–Barsky clip of segment `a`–`b` to the box `[x0,x1]×[y0,y1]`.
pub(super) fn clip_box(a: V, b: V, x0: f64, x1: f64, y0: f64, y1: f64) -> Option<(V, V)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d.x, a.x - x0), (d.x, x1 - a.x), (-d.y, a.y - y0), (d.y, y1 - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((a + d * t0, a + d * t1))
}

// ---------- octilinear helpers ----------

/// The eight compass directions, counter-clockwise from +X.
pub(super) const DIRS: [(i64, i64); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// Octant index of an exactly octilinear vector.
pub(super) fn octant(dx: Coord, dy: Coord) -> Option<u8> {
    if dx == 0 && dy == 0 {
        return None;
    }
    if dx != 0 && dy != 0 && dx.abs() != dy.abs() {
        return None;
    }
    let s = (dx.signum(), dy.signum());
    DIRS.iter().position(|&d| d == s).map(|i| i as u8)
}

/// Octant distance between two directions (0..=4).
pub(super) fn octant_diff(a: u8, b: u8) -> u8 {
    let d = (a as i16 - b as i16).rem_euclid(8) as u8;
    d.min(8 - d)
}

/// Whether two consecutive segment vectors form an allowed corner.
pub(super) fn turn_ok(prev: (Coord, Coord), next: (Coord, Coord), style: CornerStyle) -> bool {
    if prev == (0, 0) || next == (0, 0) {
        return true;
    }
    match style {
        CornerStyle::Diagonal45 | CornerStyle::Arc45 => match (octant(prev.0, prev.1), octant(next.0, next.1)) {
            (Some(a), Some(b)) => octant_diff(a, b) <= 1,
            _ => false,
        },
        CornerStyle::Orthogonal90 => {
            let axis = |v: (Coord, Coord)| v.0 == 0 || v.1 == 0;
            axis(prev) && axis(next) && (prev.0 as i128 * next.0 as i128 + prev.1 as i128 * next.1 as i128) >= 0
        }
        CornerStyle::Any => (prev.0 as i128 * next.0 as i128 + prev.1 as i128 * next.1 as i128) >= 0,
    }
}

pub(super) fn vec_of(a: Point, b: Point) -> (Coord, Coord) {
    (b.x - a.x, b.y - a.y)
}

/// Corner-style connection from `a` to `b` (2 or 3 points). Diagonal: the
/// straight run comes first unless `flip`; orthogonal: horizontal first
/// unless `flip`; any-angle: a single straight segment.
pub(super) fn corner_path(a: Point, b: Point, style: CornerStyle, flip: bool) -> Vec<Point> {
    let (dx, dy) = vec_of(a, b);
    if dx == 0 && dy == 0 {
        return vec![a];
    }
    match style {
        CornerStyle::Any => vec![a, b],
        CornerStyle::Orthogonal90 => {
            if dx == 0 || dy == 0 {
                vec![a, b]
            } else if flip {
                vec![a, Point::new(a.x, b.y), b]
            } else {
                vec![a, Point::new(b.x, a.y), b]
            }
        }
        CornerStyle::Diagonal45 | CornerStyle::Arc45 => {
            let m = dx.abs().min(dy.abs());
            if m == 0 || dx.abs() == dy.abs() {
                return vec![a, b];
            }
            let diag = (dx.signum() * m, dy.signum() * m);
            let mid = if flip { a.translated(diag.0, diag.1) } else { a.translated(dx - diag.0, dy - diag.1) };
            vec![a, mid, b]
        }
    }
}

/// Removes repeated points and merges collinear same-direction runs.
pub(super) fn simplify(pts: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let (u, v) = (vec_of(a, b), vec_of(b, p));
            let cross = u.0 as i128 * v.1 as i128 - u.1 as i128 * v.0 as i128;
            let dot = u.0 as i128 * v.0 as i128 + u.1 as i128 * v.1 as i128;
            if cross == 0 && dot > 0 {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}


/// Offsets a polyline sideways by `o` (positive = left of travel) with
/// mitered joints, so parallel copies stay at constant distance.
pub(super) fn offset_polyline(pts: &[Point], o: f64) -> Vec<Point> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let normals: Vec<V> = pts.windows(2).map(|w| (V::of(w[1]) - V::of(w[0])).norm().perp()).collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = V::of(pts[i]);
        let q = if i == 0 {
            p + normals[0] * o
        } else if i == n - 1 {
            p + normals[n - 2] * o
        } else {
            let (n0, n1) = (normals[i - 1], normals[i]);
            let m = (n0 + n1).norm();
            let c = m.dot(n1);
            if c.abs() < 1e-6 { p + n1 * o } else { p + m * (o / c) }
        };
        out.push(q.round());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        let (a, b) = (V::new(0.0, 0.0), V::new(10.0, 0.0));
        assert_eq!(point_seg_dist(V::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(point_seg_dist(V::new(13.0, 4.0), a, b), 5.0);
        assert_eq!(seg_seg_dist(a, b, V::new(5.0, -1.0), V::new(5.0, 1.0)), 0.0);
        assert_eq!(seg_seg_dist(a, b, V::new(0.0, 2.0), V::new(10.0, 2.0)), 2.0);
        let sq = [V::new(0.0, 0.0), V::new(4.0, 0.0), V::new(4.0, 4.0), V::new(0.0, 4.0)];
        assert_eq!(point_poly_dist(V::new(2.0, 2.0), &sq), 0.0);
        assert_eq!(point_poly_dist(V::new(7.0, 2.0), &sq), 3.0);
        assert_eq!(seg_poly_dist(V::new(-1.0, 2.0), V::new(5.0, 2.0), &sq), 0.0);
    }

    #[test]
    fn corner_paths_are_octilinear() {
        let p = corner_path(Point::new(0, 0), Point::new(10, 4), CornerStyle::Diagonal45, false);
        assert_eq!(p, vec![Point::new(0, 0), Point::new(6, 0), Point::new(10, 4)]);
        let p = corner_path(Point::new(0, 0), Point::new(10, 4), CornerStyle::Diagonal45, true);
        assert_eq!(p, vec![Point::new(0, 0), Point::new(4, 4), Point::new(10, 4)]);
        assert!(turn_ok((6, 0), (4, 4), CornerStyle::Diagonal45));
        assert!(!turn_ok((6, 0), (0, 4), CornerStyle::Diagonal45));
        assert!(turn_ok((6, 0), (0, 4), CornerStyle::Orthogonal90));
    }

    #[test]
    fn clipping_and_simplify() {
        let c = clip_box(V::new(-5.0, 0.0), V::new(5.0, 0.0), -1.0, 1.0, -1.0, 1.0).unwrap();
        assert_eq!((c.0.x, c.1.x), (-1.0, 1.0));
        assert!(clip_box(V::new(-5.0, 3.0), V::new(5.0, 3.0), -1.0, 1.0, -1.0, 1.0).is_none());
        let s = simplify(&[Point::new(0, 0), Point::new(1, 0), Point::new(2, 0), Point::new(2, 0), Point::new(3, 1)]);
        assert_eq!(s, vec![Point::new(0, 0), Point::new(2, 0), Point::new(3, 1)]);
    }
}
