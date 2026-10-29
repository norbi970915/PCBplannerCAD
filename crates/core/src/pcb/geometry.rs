//! Shared board geometry: footprint pads and graphics in world coordinates,
//! copper shapes as polygons, and board outline extraction. Used by DRC,
//! pour filling, Gerber output and the 3D model.

use crate::document::{Document, ItemId, LayerId};
use crate::geom::{normalize_deg, rotate};
use crate::item::{Arc, Drill, Item, PadKind, PadShape};
use crate::layer;
use crate::library::Footprint;
use crate::units::{Coord, Point};

use super::board::BoardSettings;
use super::items::PlacedFootprint;

/// A footprint pad placed on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldPad {
    /// The placed footprint item it belongs to.
    pub owner: ItemId,
    pub designator: String,
    pub number: String,
    pub net: String,
    pub pos: Point,
    /// World rotation in degrees.
    pub rotation: f64,
    pub shape: PadShape,
    pub width: Coord,
    pub height: Coord,
    pub corner_ratio: f64,
    pub kind: PadKind,
    /// Copper layers the pad occupies, top to bottom.
    pub copper: Vec<LayerId>,
    pub drill: Option<Drill>,
    /// Board side for SMD pads (mask/paste openings); THT pads open both.
    pub bottom: bool,
}

impl WorldPad {
    pub fn on_layer(&self, l: LayerId) -> bool {
        self.copper.contains(&l)
    }

    /// Outline polygon grown by `inflate` (use a negative value to shrink).
    pub fn polygon(&self, inflate: Coord) -> Vec<Point> {
        let w = (self.width + 2 * inflate).max(1);
        let h = (self.height + 2 * inflate).max(1);
        match self.shape {
            PadShape::Circle => circle_polygon(self.pos, w / 2, 32),
            PadShape::Oval => {
                let (long, short) = (w.max(h), w.min(h));
                let along = if w >= h { 0.0 } else { 90.0 } + self.rotation;
                let half = (long - short) / 2;
                let a = rotate(self.pos.translated(-half, 0), self.pos, along);
                let b = rotate(self.pos.translated(half, 0), self.pos, along);
                capsule_polygon(a, b, short, 0)
            }
            PadShape::RoundRect => {
                let r = ((self.width.min(self.height) as f64 * self.corner_ratio) as Coord + inflate).max(0);
                round_rect_polygon(self.pos, w, h, self.rotation, r)
            }
            PadShape::Rect => round_rect_polygon(self.pos, w, h, self.rotation, inflate.max(0)),
        }
    }
}

/// Copper layers a pad occupies given the board stack.
pub fn pad_copper(kind: PadKind, world_layer: LayerId, board: &BoardSettings) -> Vec<LayerId> {
    match kind {
        PadKind::Smd => vec![world_layer],
        PadKind::Tht => board.stackup.copper_layers(),
        PadKind::Npth => vec![],
    }
}

/// All pads of a placed footprint in world coordinates.
pub fn world_pads(owner: ItemId, fp: &PlacedFootprint, footprint: &Footprint, board: &BoardSettings) -> Vec<WorldPad> {
    let pl = fp.placement();
    footprint
        .items
        .iter()
        .filter_map(|i| if let Item::Pad(p) = i { Some(p) } else { None })
        .map(|p| {
            let world_layer = fp.world_layer(p.layer);
            WorldPad {
                owner,
                designator: fp.designator.clone(),
                number: p.number.clone(),
                net: fp.net_of(&p.number).to_string(),
                pos: pl.apply(p.pos),
                rotation: normalize_deg(if pl.mirror { -p.rotation } else { p.rotation } + pl.rotation),
                shape: p.shape,
                width: p.width,
                height: p.height,
                corner_ratio: p.corner_ratio,
                kind: p.kind,
                copper: pad_copper(p.kind, world_layer, board),
                drill: p.drill,
                bottom: world_layer == layer::BOTTOM_COPPER,
            }
        })
        .collect()
}

/// Footprint graphics (everything but pads) moved to world coordinates with
/// layers flipped for bottom-side parts. `.Designator`/`.Value` texts are
/// substituted.
pub fn world_graphics(fp: &PlacedFootprint, footprint: &Footprint) -> Vec<Item> {
    let pl = fp.placement();
    let origin = Point::default();
    let mut out = Vec::new();
    for item in &footprint.items {
        if matches!(item, Item::Pad(_)) {
            continue;
        }
        // Mirror (bottom side) first, then rotate, then translate.
        let mut it = if pl.mirror { item.mirrored(origin) } else { item.clone() };
        it = it.rotated(origin, pl.rotation).translated(pl.offset.x, pl.offset.y);
        set_layer(&mut it, |l| fp.world_layer(l));
        if let Item::Text(t) = &mut it {
            if t.text == crate::item::DESIGNATOR_TOKEN {
                t.text = fp.designator.clone();
            } else if t.text == crate::item::VALUE_TOKEN {
                t.text = fp.value.clone();
            }
            if pl.mirror {
                t.mirrored = !t.mirrored;
            }
        }
        out.push(it);
    }
    out
}

fn set_layer(item: &mut Item, f: impl Fn(LayerId) -> LayerId) {
    match item {
        Item::Line(x) => x.layer = f(x.layer),
        Item::Arc(x) => x.layer = f(x.layer),
        Item::Circle(x) => x.layer = f(x.layer),
        Item::Rect(x) => x.layer = f(x.layer),
        Item::Polyline(x) => x.layer = f(x.layer),
        Item::Text(x) => x.layer = f(x.layer),
        _ => {}
    }
}

// ---------- polygons ----------

pub fn circle_polygon(c: Point, r: Coord, n: usize) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new(c.x + (r as f64 * a.cos()).round() as Coord, c.y + (r as f64 * a.sin()).round() as Coord)
        })
        .collect()
}

/// Stadium (round-capped segment) of total `width`, grown by `inflate`.
pub fn capsule_polygon(a: Point, b: Point, width: Coord, inflate: Coord) -> Vec<Point> {
    let r = (width / 2 + inflate).max(1) as f64;
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let base = if dx == 0.0 && dy == 0.0 { 0.0 } else { dy.atan2(dx) };
    let steps = 12;
    let mut out = Vec::with_capacity(2 * steps + 2);
    for (c, start) in [(b, base - std::f64::consts::FRAC_PI_2), (a, base + std::f64::consts::FRAC_PI_2)] {
        for i in 0..=steps {
            let t = start + std::f64::consts::PI * i as f64 / steps as f64;
            out.push(Point::new(c.x + (r * t.cos()).round() as Coord, c.y + (r * t.sin()).round() as Coord));
        }
    }
    out
}

/// Rectangle `w`×`h` centered at `c`, rotated `rot` degrees, corner radius `r`.
pub fn round_rect_polygon(c: Point, w: Coord, h: Coord, rot: f64, r: Coord) -> Vec<Point> {
    let r = r.min(w / 2).min(h / 2).max(0);
    let (hw, hh) = (w / 2, h / 2);
    let mut local = Vec::new();
    if r == 0 {
        local.extend([Point::new(hw, hh), Point::new(-hw, hh), Point::new(-hw, -hh), Point::new(hw, -hh)]);
    } else {
        let corners = [(hw - r, hh - r, 0.0), (-hw + r, hh - r, 90.0), (-hw + r, -hh + r, 180.0), (hw - r, -hh + r, 270.0)];
        for (x, y, a0) in corners {
            for i in 0..=6 {
                let a = (a0 + 15.0 * i as f64).to_radians();
                local.push(Point::new(x + (r as f64 * a.cos()).round() as Coord, y + (r as f64 * a.sin()).round() as Coord));
            }
        }
    }
    local.into_iter().map(|p| rotate(p.translated(c.x, c.y), c, rot)).collect()
}

/// Polyline approximation of an arc (≤ 5° per segment).
pub fn arc_points(a: &Arc) -> Vec<Point> {
    let n = ((a.sweep.abs() / 5.0).ceil() as usize).max(2);
    (0..=n).map(|i| a.point_at(a.start_angle + a.sweep * i as f64 / n as f64)).collect()
}

/// Signed area (positive = counter-clockwise), in nm².
pub fn signed_area(pts: &[Point]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| (pts[i].x as f64) * (pts[(i + 1) % n].y as f64) - (pts[(i + 1) % n].x as f64) * (pts[i].y as f64)).sum::<f64>() / 2.0
}

// ---------- board outline ----------

/// Closed loops drawn on the board-outline layer, largest (the board) first.
/// Open chains are closed if their ends are within `tolerance`.
pub fn outline_loops(doc: &Document, tolerance: Coord) -> Vec<Vec<Point>> {
    let mut loops: Vec<Vec<Point>> = Vec::new();
    let mut chains: Vec<Vec<Point>> = Vec::new();
    for (_, item) in doc.items() {
        match item {
            Item::Line(l) if l.layer == layer::BOARD_OUTLINE => chains.push(vec![l.a, l.b]),
            Item::Arc(a) if a.layer == layer::BOARD_OUTLINE => chains.push(arc_points(a)),
            Item::Circle(c) if c.layer == layer::BOARD_OUTLINE => loops.push(circle_polygon(c.center, c.radius, 64)),
            Item::Rect(r) if r.layer == layer::BOARD_OUTLINE => {
                loops.push(vec![r.a, Point::new(r.b.x, r.a.y), r.b, Point::new(r.a.x, r.b.y)]);
            }
            Item::Polyline(p) if p.layer == layer::BOARD_OUTLINE => {
                if p.closed {
                    loops.push(p.points.clone());
                } else {
                    chains.push(p.points.clone());
                }
            }
            _ => {}
        }
    }
    let close = |a: Point, b: Point| (a.x - b.x).abs() <= tolerance && (a.y - b.y).abs() <= tolerance;
    while let Some(mut chain) = chains.pop() {
        loop {
            let (start, end) = (chain[0], *chain.last().unwrap());
            if chain.len() > 2 && close(start, end) {
                chain.pop();
                loops.push(chain);
                break;
            }
            let next = chains.iter().position(|c| close(c[0], end) || close(*c.last().unwrap(), end));
            match next {
                Some(i) => {
                    let mut c = chains.swap_remove(i);
                    if !close(c[0], end) {
                        c.reverse();
                    }
                    chain.extend(c.into_iter().skip(1));
                }
                None => break, // open outline: ignored (DRC reports it)
            }
        }
    }
    loops.sort_by(|a, b| signed_area(b).abs().partial_cmp(&signed_area(a).abs()).unwrap_or(std::cmp::Ordering::Equal));
    loops
}

/// The board settings item of a PCB document, if present.
pub fn board_settings(doc: &Document) -> Option<(ItemId, &BoardSettings)> {
    doc.items().find_map(|(id, i)| if let Item::Board(b) = i { Some((id, b)) } else { None })
}

/// Every pad on the board (footprints whose geometry is missing from the cache are skipped).
pub fn all_pads(doc: &Document, board: &BoardSettings) -> Vec<WorldPad> {
    doc.items()
        .filter_map(|(id, i)| if let Item::Footprint(fp) = i { Some((id, fp)) } else { None })
        .filter_map(|(id, fp)| doc.cached_footprint(&fp.footprint).map(|f| world_pads(id, fp, f, board)))
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Line, Pad};
    use crate::pcb::items::Side;
    use crate::units::mm;

    fn smd(number: &str, x: f64) -> Item {
        Item::Pad(Pad {
            number: number.into(),
            pos: Point::new(mm(x), 0),
            rotation: 0.0,
            shape: PadShape::Rect,
            width: mm(1.0),
            height: mm(0.5),
            corner_ratio: 0.0,
            kind: PadKind::Smd,
            layer: layer::TOP_COPPER,
            drill: None,
        })
    }

    #[test]
    fn bottom_side_footprints_mirror_and_flip_layers() {
        let mut f = Footprint::new("R");
        f.items = vec![smd("1", -1.0), smd("2", 1.0)];
        let fp = PlacedFootprint {
            board_only: false,
            designator: "R1".into(),
            value: "10k".into(),
            footprint: "L/R".into(),
            pos: Point::new(mm(10.0), 0),
            rotation: 90.0,
            side: Side::Bottom,
            locked: false,
            pad_nets: [("1".to_string(), "GND".to_string())].into(),
            component: String::new(),
            parameters: Default::default(),
        };
        let pads = world_pads(ItemId(7), &fp, &f, &BoardSettings::default());
        // Pad 1 at local (-1, 0) → mirrored (1, 0) → rotated 90 → (0, 1) → +offset.
        assert_eq!(pads[0].pos, Point::new(mm(10.0), mm(1.0)));
        assert_eq!(pads[0].copper, vec![layer::BOTTOM_COPPER]);
        assert_eq!(pads[0].net, "GND");
        assert!(pads[0].bottom);
    }

    #[test]
    fn outline_segments_chain_into_a_loop() {
        let mut doc = Document::new(crate::DocKind::Pcb);
        let pts = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
        let items: Vec<Item> = (0..4)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % 4]);
                Item::Line(Line { a: Point::new(mm(a.0), mm(a.1)), b: Point::new(mm(b.0), mm(b.1)), width: mm(0.1), layer: layer::BOARD_OUTLINE })
            })
            .collect();
        doc = Document::from_item_list(doc.kind(), items);
        let loops = outline_loops(&doc, 1000);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4);
        assert!((signed_area(&loops[0]).abs() - (mm(50.0) as f64 * mm(30.0) as f64)).abs() < 1e6);
    }

    #[test]
    fn pad_polygons_grow_with_inflate() {
        let p = WorldPad {
            owner: ItemId(0),
            designator: "U1".into(),
            number: "1".into(),
            net: String::new(),
            pos: Point::default(),
            rotation: 0.0,
            shape: PadShape::Rect,
            width: mm(2.0),
            height: mm(1.0),
            corner_ratio: 0.0,
            kind: PadKind::Smd,
            copper: vec![layer::TOP_COPPER],
            drill: None,
            bottom: false,
        };
        let a0 = signed_area(&p.polygon(0)).abs();
        let a1 = signed_area(&p.polygon(mm(0.1))).abs();
        assert!(a1 > a0);
    }
}
