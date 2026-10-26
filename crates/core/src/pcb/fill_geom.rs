//! Geometry helpers for the pour engine: a local integer frame for
//! `i_overlay` (i32 engine, coordinates relative to the zone center),
//! tessellation of copper primitives, booleans and offsetting.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay::{Overlay, ShapeType};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::i_float::int::angle::Angle;
use i_overlay::i_float::int::point::IntPoint;
use i_overlay::mesh::int::arc::ArcOptions;
use i_overlay::mesh::int::outline::offset::IntOutlineOffset;
use i_overlay::mesh::int::style::{IntLineJoin, IntOutlineStyle};
use i_overlay::mesh::math::MathMode;

use crate::units::{Coord, Point};

pub type IPt = IntPoint<i32>;
pub type Contour = Vec<IPt>;
pub type Shape = Vec<Contour>;
pub type Shapes = Vec<Shape>;

/// Local coordinates are clamped to this magnitude (the i32 engine accepts ±2^30).
const LIMIT: f64 = 1_000_000_000.0;

/// Maximum deviation from the true arc (nm) when a step coarser than 5° is used (5 µm).
const ARC_TOLERANCE: f64 = 5_000.0;
const MIN_STEP_DEG: f64 = 5.0;
const MAX_STEP_DEG: f64 = 20.0;
/// Vertex thinning tolerance before the min-width dilation.
const THIN_TOLERANCE: f64 = 2_000.0;

/// Translation between world nanometers and the local i32 frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    ox: i64,
    oy: i64,
}

impl Frame {
    /// A frame centered on the bounding box of `pts`.
    pub fn around(pts: &[Point]) -> Frame {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if pts.is_empty() {
            return Frame { ox: 0, oy: 0 };
        }
        Frame { ox: x0 / 2 + x1 / 2, oy: y0 / 2 + y1 / 2 }
    }

    /// Rounds a local float point.
    pub fn pt(&self, x: f64, y: f64) -> IPt {
        let c = |v: f64| v.round().clamp(-LIMIT, LIMIT) as i32;
        IntPoint::new(c(x), c(y))
    }

    pub fn p(&self, p: Point) -> IPt {
        let c = |v: i64| v.clamp(-LIMIT as i64, LIMIT as i64) as i32;
        IntPoint::new(c(p.x - self.ox), c(p.y - self.oy))
    }

    /// World point in local float coordinates.
    pub fn f(&self, p: Point) -> (f64, f64) {
        ((p.x - self.ox) as f64, (p.y - self.oy) as f64)
    }

    pub fn world(&self, p: IPt) -> Point {
        Point::new(p.x as i64 + self.ox, p.y as i64 + self.oy)
    }
}

/// Number of segments for an arc of radius `r` (nm) sweeping `sweep_deg`:
/// 5° per step, coarser (up to 20°) only while the deviation stays ≤ 5 µm.
pub fn arc_steps(r: f64, sweep_deg: f64) -> usize {
    let sweep = sweep_deg.abs();
    if sweep <= 0.0 {
        return 1;
    }
    let by_tol = if r > 0.0 { 2.0 * (r / (r + ARC_TOLERANCE)).acos().to_degrees() } else { MAX_STEP_DEG };
    let step = by_tol.clamp(MIN_STEP_DEG, MAX_STEP_DEG);
    ((sweep / step).ceil() as usize).max(1)
}

/// Rectangle `w`×`h` centered at (`cx`, `cy`), rotated `rot_deg`, with
/// corner radius `r`, counter-clockwise. With `outer` the arcs are
/// circumscribed (polygon ⊇ true shape, for clearances), otherwise inscribed.
#[allow(clippy::too_many_arguments)]
pub fn rounded_rect(fr: &Frame, cx: f64, cy: f64, w: f64, h: f64, rot_deg: f64, r: f64, outer: bool) -> Contour {
    let (hw, hh) = ((w / 2.0).max(0.5), (h / 2.0).max(0.5));
    let r = r.min(hw).min(hh).max(0.0);
    let (s, c) = rot_deg.to_radians().sin_cos();
    let place = |x: f64, y: f64| fr.pt(cx + x * c - y * s, cy + x * s + y * c);
    if r < 1.0 {
        return vec![place(hw, hh), place(-hw, hh), place(-hw, -hh), place(hw, -hh)];
    }
    let n = arc_steps(r, 90.0);
    let step = 90.0 / n as f64;
    let mut out = Vec::with_capacity(4 * (n + 1));
    let corners = [(hw - r, hh - r, 0.0), (-hw + r, hh - r, 90.0), (-hw + r, -hh + r, 180.0), (hw - r, -hh + r, 270.0)];
    for (x, y, a0) in corners {
        if outer {
            // Vertices where consecutive tangents meet.
            let rr = r / (step / 2.0).to_radians().cos();
            for i in 0..n {
                let a = (a0 + (i as f64 + 0.5) * step).to_radians();
                out.push(place(x + rr * a.cos(), y + rr * a.sin()));
            }
        } else {
            for i in 0..=n {
                let a = (a0 + i as f64 * step).to_radians();
                out.push(place(x + r * a.cos(), y + r * a.sin()));
            }
        }
    }
    out.dedup();
    out
}

/// Stadium around segment `a`–`b` with radius `radius` (circumscribed).
pub fn capsule(fr: &Frame, a: (f64, f64), b: (f64, f64), radius: f64) -> Contour {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let rot = if len > 0.0 { dy.atan2(dx).to_degrees() } else { 0.0 };
    rounded_rect(fr, (a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, len + 2.0 * radius, 2.0 * radius, rot, radius, true)
}

/// Centerline points of an arc (world), with the chord sagitta so callers can
/// grow capsules to cover the true arc.
pub fn arc_chain(center: Point, radius: Coord, start_deg: f64, sweep_deg: f64) -> (Vec<(f64, f64)>, f64) {
    let r = radius as f64;
    let n = arc_steps(r, sweep_deg);
    let step = sweep_deg / n as f64;
    let pts = (0..=n)
        .map(|i| {
            let a = (start_deg + step * i as f64).to_radians();
            (center.x as f64 + r * a.cos(), center.y as f64 + r * a.sin())
        })
        .collect();
    let sagitta = r * (1.0 - (step.abs() / 2.0).to_radians().cos());
    (pts, sagitta)
}

/// Twice the signed area (positive = counter-clockwise).
pub fn area2(c: &[IPt]) -> f64 {
    let n = c.len();
    (0..n)
        .map(|i| {
            let (p, q) = (c[i], c[(i + 1) % n]);
            p.x as f64 * q.y as f64 - q.x as f64 * p.y as f64
        })
        .sum()
}

pub fn make_ccw(mut c: Contour) -> Contour {
    if area2(&c) < 0.0 {
        c.reverse();
    }
    c
}

// ---------- booleans ----------

fn edges(shapes: &[Shape]) -> usize {
    shapes.iter().flatten().map(|c| c.len()).sum()
}

/// Normalizes contours (nonzero or even-odd) into shapes.
pub fn normalize(contours: &[Contour], fill: FillRule) -> Shapes {
    let mut ov = Overlay::<i32>::new(contours.iter().map(|c| c.len()).sum());
    ov.add_source(contours, ShapeType::Subject);
    ov.overlay(OverlayRule::Subject, fill)
}

/// `subj ∘ clip` with the nonzero rule (clip = shapes plus loose CCW contours).
pub fn boolean(subj: &[Shape], clip_shapes: &[Shape], clip_contours: &[Contour], rule: OverlayRule) -> Shapes {
    let cap = edges(subj) + edges(clip_shapes) + clip_contours.iter().map(|c| c.len()).sum::<usize>();
    let mut ov = Overlay::<i32>::new(cap);
    ov.add_source(subj, ShapeType::Subject);
    ov.add_source(clip_shapes, ShapeType::Clip);
    ov.add_source(clip_contours, ShapeType::Clip);
    ov.overlay(rule, FillRule::NonZero)
}

/// Grows (`delta` > 0) or shrinks shapes with round joins (≤ 5° per step).
pub fn offset(shapes: &[Shape], delta: Coord) -> Shapes {
    let max_step = Angle::from_radians(MIN_STEP_DEG.to_radians()).unwrap_or(ArcOptions::MIN_STEP);
    offset_with(shapes, delta, IntLineJoin::Round(ArcOptions { max_step, rotation_precision: 5 }))
}

/// Grows or shrinks shapes with mitered corners (clipped below 90°).
pub fn offset_miter(shapes: &[Shape], delta: Coord) -> Shapes {
    offset_with(shapes, delta, IntLineJoin::Miter(Angle::QUARTER_TURN))
}

fn offset_with(shapes: &[Shape], delta: Coord, join: IntLineJoin) -> Shapes {
    if delta == 0 || shapes.is_empty() {
        return shapes.to_vec();
    }
    let d = delta.clamp(-(LIMIT as i64) / 4, LIMIT as i64 / 4) as i32;
    let style = IntOutlineStyle::new(d).line_join(join).math(MathMode::Float);
    shapes.outline(&style).unwrap_or_default()
}

// ---------- measurements ----------

#[derive(Clone, Copy, Debug)]
pub struct FBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl FBox {
    pub fn of_points(pts: impl IntoIterator<Item = (f64, f64)>) -> FBox {
        let mut b = FBox { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
        for (x, y) in pts {
            b.x0 = b.x0.min(x);
            b.y0 = b.y0.min(y);
            b.x1 = b.x1.max(x);
            b.y1 = b.y1.max(y);
        }
        b
    }

    pub fn of_shape(s: &Shape) -> FBox {
        FBox::of_points(s.first().into_iter().flatten().map(|p| (p.x as f64, p.y as f64)))
    }

    pub fn of_shapes(s: &[Shape]) -> FBox {
        FBox::of_points(s.iter().filter_map(|s| s.first()).flatten().map(|p| (p.x as f64, p.y as f64)))
    }

    pub fn grown(self, d: f64) -> FBox {
        FBox { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }

    pub fn overlaps(&self, o: &FBox) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    pub fn contains(&self, o: &FBox) -> bool {
        self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1
    }
}

/// Even-odd point containment over all contours of a shape.
pub fn point_in_shape(s: &Shape, x: f64, y: f64) -> bool {
    let mut inside = false;
    for c in s {
        let n = c.len();
        for i in 0..n {
            let (p, q) = (c[i], c[(i + 1) % n]);
            let (px, py, qx, qy) = (p.x as f64, p.y as f64, q.x as f64, q.y as f64);
            if (py > y) != (qy > y) && x < px + (y - py) * (qx - px) / (qy - py) {
                inside = !inside;
            }
        }
    }
    inside
}

fn point_seg_dist2(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (ex, ey) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    ex * ex + ey * ey
}

fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let orient = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Squared distance between segments `a`–`b` and `c`–`d`.
pub fn seg_seg_dist2(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    if segments_cross(a, b, c, d) {
        return 0.0;
    }
    point_seg_dist2(a, c, d).min(point_seg_dist2(b, c, d)).min(point_seg_dist2(c, a, b)).min(point_seg_dist2(d, a, b))
}

/// Whether the capsule `a`–`b` of radius `r` overlaps the shape.
pub fn capsule_touches_shape(s: &Shape, a: (f64, f64), b: (f64, f64), r: f64) -> bool {
    if point_in_shape(s, a.0, a.1) {
        return true;
    }
    let r2 = r * r;
    for c in s {
        let n = c.len();
        for i in 0..n {
            let (p, q) = (c[i], c[(i + 1) % n]);
            if seg_seg_dist2(a, b, (p.x as f64, p.y as f64), (q.x as f64, q.y as f64)) <= r2 {
                return true;
            }
        }
    }
    false
}

/// Drops vertices closer than `tol` to the chord of their neighbours
/// (greedy, one pass per contour). Contours that collapse are removed.
pub fn thin(shapes: &mut Shapes, tol: f64) {
    let tol2 = tol * tol;
    for s in shapes.iter_mut() {
        for c in s.iter_mut() {
            if c.len() <= 4 {
                continue;
            }
            let mut out: Contour = Vec::with_capacity(c.len());
            let n = c.len();
            out.push(c[0]);
            for i in 1..n {
                let prev = *out.last().unwrap();
                let next = c[(i + 1) % n];
                let f = |p: IPt| (p.x as f64, p.y as f64);
                if point_seg_dist2(f(c[i]), f(prev), f(next)) >= tol2 {
                    out.push(c[i]);
                }
            }
            if out.len() >= 3 {
                *c = out;
            }
        }
    }
}

/// Morphological opening by `h`: removes everything narrower than `2h`.
/// Mitered offsets keep corners sharp and vertex counts low; the result is
/// clipped back to the input so it never grows past it.
pub fn opening(shapes: &[Shape], h: Coord) -> Shapes {
    let mut eroded = offset_miter(shapes, -h);
    // Dense arcs make the dilation's self-overlap resolution expensive; the
    // final intersection keeps the thinning from adding copper.
    thin(&mut eroded, THIN_TOLERANCE);
    let opened = offset_miter(&eroded, h);
    if opened.is_empty() {
        return opened;
    }
    boolean(&opened, shapes, &[], OverlayRule::Intersect)
}
