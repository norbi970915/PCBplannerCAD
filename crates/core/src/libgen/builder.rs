//! Footprint construction helpers shared by the wizard and the hand-made
//! starter footprints.
//!
//! All geometry is given in millimeters (`f64`) and converted to integer
//! nanometers when items are emitted. The builder enforces the house rules:
//!
//! * SMD pads are rounded rectangles on `TOP_COPPER` (paste/mask implied),
//!   corner radius 25 % of the shorter side but at most 0.25 mm (the same
//!   cap used for rounded corners so big thermal pads don't become blobs).
//! * THT pads live on `MULTI_LAYER` with a drill.
//! * Silk-screen line work is clipped against every pad's bounding box
//!   grown by `SILK_CLEARANCE + SILK_WIDTH / 2` (+10 µm rounding margin), so
//!   the stroke never comes closer than 0.15 mm to copper. Fragments shorter
//!   than `MIN_SILK_SEGMENT` are dropped. Circles/arcs/dots are not clipped;
//!   they are kept only if they are already clear.
//! * The courtyard is the bounding box of pads, body and silk, grown by
//!   0.25 mm and rounded outward to a 0.01 mm grid.
//! * `.Designator` sits on `TOP_SILK` just above the courtyard, `.Value` on
//!   `TOP_ASSEMBLY` just below it.

use crate::document::LayerId;
use crate::item::{Arc, Circle, DESIGNATOR_TOKEN, Drill, FillStyle, HAlign, Item, Line, Pad, PadKind, PadShape, Polyline, Rect, Text, VAlign, VALUE_TOKEN};
use crate::layer::{MULTI_LAYER, TOP_ASSEMBLY, TOP_COPPER, TOP_COURTYARD, TOP_PASTE, TOP_SILK};
use crate::library::Footprint;
use crate::units::{Coord, Point, mm};

pub(crate) const SILK_WIDTH: f64 = 0.12;
pub(crate) const SILK_CLEARANCE: f64 = 0.15;
const SILK_MARGIN: f64 = 0.01;
pub(crate) const ASSEMBLY_WIDTH: f64 = 0.10;
pub(crate) const COURTYARD_WIDTH: f64 = 0.05;
pub(crate) const COURTYARD_EXCESS: f64 = 0.25;
pub(crate) const TEXT_HEIGHT: f64 = 1.0;
pub(crate) const CORNER_RATIO: f64 = 0.25;
const MAX_CORNER_RADIUS: f64 = 0.25;
const MIN_SILK_SEGMENT: f64 = 0.2;
/// Radius of the filled pin-1 dot (its stroke adds `SILK_WIDTH / 2`).
pub(crate) const DOT_RADIUS: f64 = 0.1;

/// Distance from a pad edge to the centerline of a silk stroke that is just clear.
pub(crate) fn silk_keepout() -> f64 {
    SILK_CLEARANCE + SILK_WIDTH / 2.0 + SILK_MARGIN
}

/// Center offset of a pin-1 dot placed just outside a pad edge.
pub(crate) fn dot_offset() -> f64 {
    SILK_CLEARANCE + DOT_RADIUS + SILK_WIDTH / 2.0 + 2.0 * SILK_MARGIN
}

/// Which body corner the assembly outline bevels to mark pin 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pin1Corner {
    TopLeft,
    BottomLeft,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Box2 {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Box2 {
    pub fn centered(cx: f64, cy: f64, w: f64, h: f64) -> Self {
        Box2 { x0: cx - w / 2.0, y0: cy - h / 2.0, x1: cx + w / 2.0, y1: cy + h / 2.0 }
    }

    pub fn inflate(self, d: f64) -> Self {
        Box2 { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }

    pub fn union(self, o: Box2) -> Self {
        Box2 { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    /// Euclidean distance from a point to the box (0 inside).
    pub fn distance(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x0 - x).max(0.0).max(x - self.x1);
        let dy = (self.y0 - y).max(0.0).max(y - self.y1);
        dx.hypot(dy)
    }

    /// Distance from a point to the farthest corner.
    pub fn far_distance(&self, x: f64, y: f64) -> f64 {
        let dx = (x - self.x0).abs().max((x - self.x1).abs());
        let dy = (y - self.y0).abs().max((y - self.y1).abs());
        dx.hypot(dy)
    }
}

fn union_opt(a: Option<Box2>, b: Box2) -> Option<Box2> {
    Some(a.map_or(b, |a| a.union(b)))
}

type P2 = (f64, f64);

fn pt(p: P2) -> Point {
    Point::new(mm(p.0), mm(p.1))
}

/// Parameter range `[t0, t1]` of segment `a`–`b` inside `bx` (Liang–Barsky).
fn clip_t(a: P2, b: P2, bx: &Box2) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, a.0 - bx.x0), (dx, bx.x1 - a.0), (-dy, a.1 - bx.y0), (dy, bx.y1 - a.1)] {
        if p.abs() < 1e-12 {
            // Parallel and outside, or running exactly along the boundary: no cut.
            if q <= 1e-9 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    (t1 > t0).then_some((t0, t1))
}

pub(crate) struct FpBuilder {
    name: String,
    description: String,
    pads: Vec<Pad>,
    pad_boxes: Vec<Box2>,
    silk_segs: Vec<(P2, P2)>,
    silk_other: Vec<Item>,
    silk_extent: Option<Box2>,
    assembly: Vec<Item>,
    paste: Vec<Item>,
    body: Option<Box2>,
}

#[allow(clippy::too_many_arguments)]
impl FpBuilder {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        FpBuilder {
            name: name.into(),
            description: description.into(),
            pads: Vec::new(),
            pad_boxes: Vec::new(),
            silk_segs: Vec::new(),
            silk_other: Vec::new(),
            silk_extent: None,
            assembly: Vec::new(),
            paste: Vec::new(),
            body: None,
        }
    }

    fn push_pad(&mut self, pad: Pad, x: f64, y: f64, w: f64, h: f64) {
        self.pad_boxes.push(Box2::centered(x, y, w, h));
        self.pads.push(pad);
    }

    /// Rounded-rectangle SMD pad on the top copper layer.
    pub fn smd(&mut self, number: &str, x: f64, y: f64, w: f64, h: f64) {
        // Quantized to 4 decimals so the value survives a JSON round trip exactly.
        let ratio = (CORNER_RATIO.min(MAX_CORNER_RADIUS / w.min(h)) * 1e4).floor() / 1e4;
        self.smd_shape(number, x, y, w, h, PadShape::RoundRect, ratio);
    }

    pub fn smd_shape(&mut self, number: &str, x: f64, y: f64, w: f64, h: f64, shape: PadShape, corner_ratio: f64) {
        let pad = Pad {
            number: number.into(),
            pos: pt((x, y)),
            rotation: 0.0,
            shape,
            width: mm(w),
            height: mm(h),
            corner_ratio: if shape == PadShape::RoundRect { corner_ratio } else { 0.0 },
            kind: PadKind::Smd,
            layer: TOP_COPPER,
            drill: None,
        };
        self.push_pad(pad, x, y, w, h);
    }

    /// Plated through-hole pad with a round drill.
    pub fn tht(&mut self, number: &str, x: f64, y: f64, w: f64, h: f64, shape: PadShape, drill: f64) {
        let pad = Pad {
            number: number.into(),
            pos: pt((x, y)),
            rotation: 0.0,
            shape,
            width: mm(w),
            height: mm(h),
            corner_ratio: 0.0,
            kind: PadKind::Tht,
            layer: MULTI_LAYER,
            drill: Some(Drill { width: mm(drill), height: mm(drill) }),
        };
        self.push_pad(pad, x, y, w, h);
    }

    pub fn silk_line(&mut self, a: P2, b: P2) {
        self.silk_segs.push((a, b));
    }

    pub fn silk_path(&mut self, points: &[P2], closed: bool) {
        for w in points.windows(2) {
            self.silk_line(w[0], w[1]);
        }
        if closed && points.len() > 2 {
            self.silk_line(points[points.len() - 1], points[0]);
        }
    }

    pub fn silk_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.silk_path(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], true);
    }

    fn add_silk_extent(&mut self, b: Box2) {
        self.silk_extent = union_opt(self.silk_extent, b);
    }

    /// Whether a circle stroke of radius `r` (stroke width included by the
    /// caller) stays clear of every pad.
    fn ring_clear(&self, cx: f64, cy: f64, r_outer: f64, r_inner: f64) -> bool {
        let clr = SILK_CLEARANCE + SILK_MARGIN;
        self.pad_boxes.iter().all(|b| b.distance(cx, cy) >= r_outer + clr || b.far_distance(cx, cy) <= r_inner - clr)
    }

    /// Filled pin-1 dot. Returns false (and draws nothing) if it would touch a pad.
    pub fn silk_dot(&mut self, x: f64, y: f64) -> bool {
        let outer = DOT_RADIUS + SILK_WIDTH / 2.0;
        if !self.ring_clear(x, y, outer, f64::NEG_INFINITY) {
            return false;
        }
        self.silk_other.push(Item::Circle(Circle {
            center: pt((x, y)),
            radius: mm(DOT_RADIUS),
            width: mm(SILK_WIDTH),
            layer: TOP_SILK,
            fill: FillStyle::Solid,
        }));
        self.add_silk_extent(Box2::centered(x, y, 2.0 * outer, 2.0 * outer));
        true
    }

    /// Circle outline on silk; skipped (returns false) if it would touch a pad.
    pub fn silk_circle(&mut self, x: f64, y: f64, r: f64) -> bool {
        let hw = SILK_WIDTH / 2.0;
        if !self.ring_clear(x, y, r + hw, r - hw) {
            return false;
        }
        self.silk_other.push(Item::Circle(Circle { center: pt((x, y)), radius: mm(r), width: mm(SILK_WIDTH), layer: TOP_SILK, fill: FillStyle::None }));
        self.add_silk_extent(Box2::centered(x, y, 2.0 * (r + hw), 2.0 * (r + hw)));
        true
    }

    /// Arc on silk (degrees, CCW); skipped (returns false) if any part would touch a pad.
    pub fn silk_arc(&mut self, cx: f64, cy: f64, r: f64, start: f64, sweep: f64) -> bool {
        let steps = (sweep.abs() / 2.0).ceil().max(2.0) as usize;
        let pts: Vec<P2> = (0..=steps)
            .map(|i| {
                let a = (start + sweep * i as f64 / steps as f64).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        // Chord sag between samples is tiny (2° steps) and covered by the margin.
        let need = silk_keepout();
        if pts.iter().any(|&(x, y)| self.pad_boxes.iter().any(|b| b.distance(x, y) < need)) {
            return false;
        }
        self.silk_other.push(Item::Arc(Arc { center: pt((cx, cy)), radius: mm(r), start_angle: start, sweep, width: mm(SILK_WIDTH), layer: TOP_SILK }));
        let hw = SILK_WIDTH / 2.0;
        for (x, y) in pts {
            self.add_silk_extent(Box2::centered(x, y, 2.0 * hw, 2.0 * hw));
        }
        true
    }

    /// Body outline on the assembly layer with a bevel at the pin-1 corner;
    /// also records the body for the courtyard.
    pub fn body(&mut self, cx: f64, cy: f64, w: f64, h: f64, corner: Pin1Corner) {
        let (x0, x1, y0, y1) = (cx - w / 2.0, cx + w / 2.0, cy - h / 2.0, cy + h / 2.0);
        let c = (0.25 * w.min(h)).min(1.0);
        let points = match corner {
            Pin1Corner::TopLeft => vec![(x0 + c, y1), (x1, y1), (x1, y0), (x0, y0), (x0, y1 - c)],
            Pin1Corner::BottomLeft => vec![(x0, y1), (x1, y1), (x1, y0), (x0 + c, y0), (x0, y0 + c)],
        };
        self.assembly_path(&points, true);
        self.body = union_opt(self.body, Box2 { x0, y0, x1, y1 });
    }

    /// Extends the region the courtyard must cover (e.g. a round body).
    pub fn body_extent(&mut self, b: Box2) {
        self.body = union_opt(self.body, b);
    }

    pub fn assembly_path(&mut self, points: &[P2], closed: bool) {
        self.assembly.push(Item::Polyline(Polyline {
            points: points.iter().copied().map(pt).collect(),
            closed,
            width: mm(ASSEMBLY_WIDTH),
            layer: TOP_ASSEMBLY,
            fill: FillStyle::None,
        }));
    }

    pub fn assembly_line(&mut self, a: P2, b: P2) {
        self.assembly.push(Item::Line(Line { a: pt(a), b: pt(b), width: mm(ASSEMBLY_WIDTH), layer: TOP_ASSEMBLY }));
    }

    pub fn assembly_circle(&mut self, x: f64, y: f64, r: f64) {
        self.assembly.push(Item::Circle(Circle { center: pt((x, y)), radius: mm(r), width: mm(ASSEMBLY_WIDTH), layer: TOP_ASSEMBLY, fill: FillStyle::None }));
    }

    /// Explicit solder-paste aperture (used for subdivided exposed pads).
    pub fn paste_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.paste.push(Item::Rect(Rect {
            a: pt((x - w / 2.0, y - h / 2.0)),
            b: pt((x + w / 2.0, y + h / 2.0)),
            width: 0,
            layer: TOP_PASTE,
            fill: FillStyle::Solid,
        }));
    }

    /// Silk segments with every pad keep-out cut away.
    fn clipped_silk(&self) -> Vec<(P2, P2)> {
        let keepouts: Vec<Box2> = self.pad_boxes.iter().map(|b| b.inflate(silk_keepout())).collect();
        let mut out = Vec::new();
        for &(a, b) in &self.silk_segs {
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            if len < 1e-9 {
                continue;
            }
            let mut cuts: Vec<(f64, f64)> = keepouts.iter().filter_map(|k| clip_t(a, b, k)).collect();
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut t = 0.0;
            let lerp = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let emit = |t0: f64, t1: f64, out: &mut Vec<(P2, P2)>| {
                if (t1 - t0) * len >= MIN_SILK_SEGMENT {
                    out.push((lerp(t0), lerp(t1)));
                }
            };
            for (c0, c1) in cuts {
                if c0 > t {
                    emit(t, c0, &mut out);
                }
                t = t.max(c1);
            }
            if t < 1.0 {
                emit(t, 1.0, &mut out);
            }
        }
        out
    }

    pub fn build(mut self) -> Footprint {
        let silk = self.clipped_silk();
        let hw = SILK_WIDTH / 2.0;
        for &(a, b) in &silk {
            let bx = Box2 { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }.inflate(hw);
            self.add_silk_extent(bx);
        }

        let mut extent: Option<Box2> = None;
        for b in &self.pad_boxes {
            extent = union_opt(extent, *b);
        }
        if let Some(b) = self.body {
            extent = union_opt(extent, b);
        }
        if let Some(b) = self.silk_extent {
            extent = union_opt(extent, b);
        }
        let e = extent.unwrap_or(Box2::centered(0.0, 0.0, 1.0, 1.0)).inflate(COURTYARD_EXCESS);
        const GRID: Coord = 10_000; // 0.01 mm
        let down = |v: f64| mm(v).div_euclid(GRID) * GRID;
        let up = |v: f64| -(-mm(v)).div_euclid(GRID) * GRID;
        let (cx0, cy0, cx1, cy1) = (down(e.x0), down(e.y0), up(e.x1), up(e.y1));

        let mut items: Vec<Item> = Vec::new();
        items.extend(self.pads.into_iter().map(Item::Pad));
        items.extend(self.paste);
        for (a, b) in silk {
            items.push(Item::Line(Line { a: pt(a), b: pt(b), width: mm(SILK_WIDTH), layer: TOP_SILK }));
        }
        items.extend(self.silk_other);
        items.extend(self.assembly);
        items.push(Item::Rect(Rect {
            a: Point::new(cx0, cy0),
            b: Point::new(cx1, cy1),
            width: mm(COURTYARD_WIDTH),
            layer: TOP_COURTYARD,
            fill: FillStyle::None,
        }));
        let text = |s: &str, y: Coord, v_align: VAlign, layer: LayerId| {
            Item::Text(Text {
                pos: Point::new(0, y),
                text: s.into(),
                height: mm(TEXT_HEIGHT),
                rotation: 0.0,
                h_align: HAlign::Center,
                v_align,
                layer,
                mirrored: false,
            })
        };
        items.push(text(DESIGNATOR_TOKEN, cy1 + mm(0.2), VAlign::Bottom, TOP_SILK));
        items.push(text(VALUE_TOKEN, cy0 - mm(0.2), VAlign::Top, TOP_ASSEMBLY));

        Footprint { name: self.name, description: self.description, items, model: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liang_barsky_clips_through_box() {
        let b = Box2 { x0: -1.0, y0: -1.0, x1: 1.0, y1: 1.0 };
        let (t0, t1) = clip_t((-2.0, 0.0), (2.0, 0.0), &b).unwrap();
        assert!((t0 - 0.25).abs() < 1e-12 && (t1 - 0.75).abs() < 1e-12);
        assert!(clip_t((-2.0, 2.0), (2.0, 2.0), &b).is_none());
    }
}
