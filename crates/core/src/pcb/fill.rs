//! Copper pour (zone) filling.
//!
//! Per zone: start from the zone outline clipped to the board (shrunk by the
//! board-outline clearance, cutouts grown by it), subtract every foreign
//! copper object grown by its clearance, knock out thermal reliefs / clearances
//! around same-net pads, remove slivers narrower than the zone's minimum width,
//! and drop islands that touch no same-net copper. Geometry runs through
//! `i_overlay` (i32 engine) in a frame centered on the zone.

use std::cmp::Reverse;
use std::collections::HashMap;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;

use crate::document::{Document, ItemId, LayerId};
use crate::item::{Item, PadKind, PadShape};
use crate::units::{Coord, Point};

use super::board::{BoardSettings, RuleKind};
use super::geometry::{WorldPad, all_pads, board_settings, outline_loops};
use super::items::{Zone, ZoneConnect, ZonePolygon};
use super::query::{Obj, ObjKind, RuleSet};

#[path = "fill_geom.rs"]
mod geom;

use geom::{Contour, FBox, Frame, Shape, Shapes};

/// Board-outline segments closer than this are chained into loops.
const OUTLINE_TOLERANCE: Coord = 10_000;

/// Computes fills for every non-keepout zone; result is (zone item id, new fill).
///
/// Zones are poured from the highest priority down, so each zone sees the
/// fresh fills of the zones that outrank it. Results are in item-id order.
pub fn fill_all(doc: &Document) -> Vec<(ItemId, Vec<ZonePolygon>)> {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let ctx = Ctx::new(doc, board);
    let mut zones: Vec<(ItemId, &Zone)> = doc
        .items()
        .filter_map(|(id, i)| match i {
            Item::Zone(z) if !z.keepout => Some((id, z)),
            _ => None,
        })
        .collect();
    zones.sort_by_key(|(id, z)| (Reverse(z.priority), *id));
    let mut fresh: HashMap<ItemId, Vec<ZonePolygon>> = HashMap::new();
    for (id, z) in zones {
        let f = ctx.fill(id, z, &fresh);
        fresh.insert(id, f);
    }
    let mut out: Vec<_> = fresh.into_iter().collect();
    out.sort_by_key(|(id, _)| *id);
    out
}

/// Fills one zone in the context of the whole board (other zones' current fills count as obstacles per priority rules).
pub fn fill_zone(doc: &Document, zone: ItemId) -> Vec<ZonePolygon> {
    let Some(Item::Zone(z)) = doc.get(zone) else { return Vec::new() };
    if z.keepout {
        return Vec::new();
    }
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    Ctx::new(doc, board).fill(zone, z, &HashMap::new())
}

/// Same-net copper an island may touch to be kept: capsule `a`–`b`, radius `r` (local frame).
struct Anchor {
    a: (f64, f64),
    b: (f64, f64),
    r: f64,
}

impl Anchor {
    fn bbox(&self) -> FBox {
        FBox::of_points([self.a, self.b]).grown(self.r)
    }
}

/// Board-wide data shared by every zone.
struct Ctx<'a> {
    doc: &'a Document,
    board: &'a BoardSettings,
    rules: RuleSet<'a>,
    pads: Vec<WorldPad>,
    outline: Vec<Vec<Point>>,
    outline_clearance: Coord,
    has_connect_rule: bool,
}

impl<'a> Ctx<'a> {
    fn new(doc: &'a Document, board: &'a BoardSettings) -> Self {
        let rules = RuleSet::new(board);
        let outline_clearance = rules.outline_clearance().max(0);
        Ctx {
            doc,
            board,
            rules,
            pads: all_pads(doc, board),
            outline: outline_loops(doc, OUTLINE_TOLERANCE).into_iter().filter(|l| l.len() >= 3).collect(),
            outline_clearance,
            has_connect_rule: board.rules.iter().any(|r| r.enabled && matches!(r.kind, RuleKind::ZoneConnect { .. })),
        }
    }

    fn footprint_key(&self, owner: ItemId) -> Option<&'a str> {
        match self.doc.get(owner) {
            Some(Item::Footprint(f)) => Some(f.footprint.as_str()),
            _ => None,
        }
    }

    fn pad_obj<'p>(&self, pad: &'p WorldPad, layer: LayerId) -> Obj<'p>
    where
        'a: 'p,
    {
        Obj {
            kind: ObjKind::Pad,
            net: &pad.net,
            layer: Some(layer),
            designator: Some(&pad.designator),
            footprint: self.footprint_key(pad.owner),
            smd: pad.kind == PadKind::Smd,
        }
    }

    /// (style, gap, spoke width) for a same-net pad.
    fn connect(&self, zone: &Zone, obj: &Obj) -> (ZoneConnect, Coord, Coord) {
        let (style, gap, spoke) = self.rules.zone_connect(obj);
        let (style, gap, spoke) = if self.has_connect_rule { (style, gap, spoke) } else { (zone.connect, zone.thermal_gap, zone.spoke_width) };
        let gap = if gap > 0 { gap } else { zone.thermal_gap.max(0) };
        let spoke = if spoke > 0 { spoke } else { zone.spoke_width.max(0) };
        (style, gap, spoke)
    }

    /// Board area (outline minus cutouts) shrunk by the outline clearance.
    fn board_shape(&self, fr: &Frame) -> Option<Shapes> {
        if self.outline.is_empty() {
            return None;
        }
        let loops: Vec<Contour> = self.outline.iter().map(|l| l.iter().map(|p| fr.p(*p)).collect()).collect();
        let shape = geom::normalize(&loops, FillRule::EvenOdd);
        Some(geom::offset(&shape, -self.outline_clearance))
    }

    fn fill(&self, zone_id: ItemId, zone: &Zone, fresh: &HashMap<ItemId, Vec<ZonePolygon>>) -> Vec<ZonePolygon> {
        if zone.outline.len() < 3 {
            return Vec::new();
        }
        let layer = zone.layer;
        let fr = Frame::around(&zone.outline);
        let outline: Contour = zone.outline.iter().map(|p| fr.p(*p)).collect();

        // 1. Zone outline ∩ board.
        let base = match self.board_shape(&fr) {
            Some(board) => geom::boolean(&[vec![outline]], &board, &[], OverlayRule::Intersect),
            None => geom::normalize(&[outline], FillRule::NonZero),
        };
        if base.is_empty() {
            return Vec::new();
        }
        let area = FBox::of_shapes(&base);

        // 2./3. Obstacles and thermal reliefs.
        let zobj = Obj::new(ObjKind::Zone, &zone.net, Some(layer));
        let same = |net: &str| !zone.net.is_empty() && net == zone.net;
        let clr = |o: &Obj| self.rules.clearance(&zobj, o).max(zone.clearance).max(0) as f64;
        let mut clip: Vec<Contour> = Vec::new();
        let mut clip_shapes: Shapes = Vec::new();
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut stubs: Vec<FBox> = Vec::new();

        let stroke = |pts: &[(f64, f64)], radius: f64, clip: &mut Vec<Contour>| {
            for w in pts.windows(2) {
                if FBox::of_points([w[0], w[1]]).grown(radius).overlaps(&area) {
                    clip.push(geom::capsule(&fr, w[0], w[1], radius));
                }
            }
        };

        for (id, item) in self.doc.items() {
            match item {
                Item::Track(t) if t.layer == layer => {
                    let (a, b) = (fr.f(t.a), fr.f(t.b));
                    if same(&t.net) {
                        anchors.push(Anchor { a, b, r: t.width as f64 / 2.0 });
                    } else {
                        let c = clr(&Obj::new(ObjKind::Track, &t.net, Some(layer)));
                        stroke(&[a, b], t.width as f64 / 2.0 + c, &mut clip);
                    }
                }
                Item::ArcTrack(t) if t.layer == layer => {
                    let (pts, sag) = geom::arc_chain(t.center, t.radius, t.start_angle, t.sweep);
                    let pts: Vec<(f64, f64)> = pts.iter().map(|&(x, y)| fr.f(Point::new(x.round() as Coord, y.round() as Coord))).collect();
                    if same(&t.net) {
                        anchors.extend(pts.windows(2).map(|w| Anchor { a: w[0], b: w[1], r: t.width as f64 / 2.0 }));
                    } else {
                        let c = clr(&Obj::new(ObjKind::Arc, &t.net, Some(layer)));
                        stroke(&pts, t.width as f64 / 2.0 + c + sag + 1.0, &mut clip);
                    }
                }
                Item::Via(v) if self.board.stackup.span(v.from_layer, v.to_layer).contains(&layer) => {
                    let p = fr.f(v.pos);
                    if same(&v.net) {
                        anchors.push(Anchor { a: p, b: p, r: v.diameter as f64 / 2.0 });
                    } else {
                        let c = clr(&Obj::new(ObjKind::Via, &v.net, Some(layer)));
                        stroke(&[p, p], v.diameter as f64 / 2.0 + c, &mut clip);
                    }
                }
                Item::Zone(z) if id != zone_id && z.layer == layer => {
                    if z.keepout {
                        if z.outline.len() >= 3 {
                            clip.push(geom::make_ccw(z.outline.iter().map(|p| fr.p(*p)).collect()));
                        }
                    } else if z.priority > zone.priority && !same(&z.net) {
                        let polys = fresh.get(&id).unwrap_or(&z.fill);
                        let shapes: Shapes = polys
                            .iter()
                            .filter(|p| p.outer.len() >= 3)
                            .map(|p| {
                                let mut s = vec![geom::make_ccw(p.outer.iter().map(|q| fr.p(*q)).collect())];
                                s.extend(p.holes.iter().filter(|h| h.len() >= 3).map(|h| {
                                    let mut h = geom::make_ccw(h.iter().map(|q| fr.p(*q)).collect());
                                    h.reverse();
                                    h
                                }));
                                s
                            })
                            .collect();
                        if shapes.is_empty() {
                            continue;
                        }
                        let c = clr(&Obj::new(ObjKind::Zone, &z.net, Some(layer)));
                        if !FBox::of_shapes(&shapes).grown(c).overlaps(&area) {
                            continue;
                        }
                        // Round joins are inscribed; pad by their worst-case sagitta.
                        let pad = (c * 0.001).ceil() + 2.0;
                        clip_shapes.extend(geom::offset(&shapes, (c + pad) as Coord));
                    }
                }
                Item::Line(l) if l.layer == layer => {
                    let c = clr(&Obj::new(ObjKind::Graphic, "", Some(layer)));
                    stroke(&[fr.f(l.a), fr.f(l.b)], l.width as f64 / 2.0 + c, &mut clip);
                }
                Item::Arc(a) if a.layer == layer => {
                    let (pts, sag) = geom::arc_chain(a.center, a.radius, a.start_angle, a.sweep);
                    let pts: Vec<(f64, f64)> = pts.iter().map(|&(x, y)| fr.f(Point::new(x.round() as Coord, y.round() as Coord))).collect();
                    let c = clr(&Obj::new(ObjKind::Graphic, "", Some(layer)));
                    stroke(&pts, a.width as f64 / 2.0 + c + sag + 1.0, &mut clip);
                }
                Item::Polyline(p) if p.layer == layer && p.points.len() >= 2 => {
                    let mut pts: Vec<(f64, f64)> = p.points.iter().map(|q| fr.f(*q)).collect();
                    if p.closed {
                        pts.push(pts[0]);
                    }
                    let c = clr(&Obj::new(ObjKind::Graphic, "", Some(layer)));
                    stroke(&pts, p.width as f64 / 2.0 + c, &mut clip);
                }
                _ => {}
            }
        }

        for pad in &self.pads {
            let (px, py) = fr.f(pad.pos);
            let reach = pad.width.max(pad.height) as f64;
            if pad.kind == PadKind::Npth || pad.copper.is_empty() {
                // Non-plated hole: cleared on every copper layer.
                if let Some(d) = pad.drill {
                    let c = clr(&Obj { net: "", ..self.pad_obj(pad, layer) });
                    let (w, h) = (d.width as f64 + 2.0 * c, d.height as f64 + 2.0 * c);
                    if FBox::of_points([(px, py)]).grown(w.max(h)).overlaps(&area) {
                        clip.push(geom::rounded_rect(&fr, px, py, w, h, pad.rotation, w.min(h) / 2.0, true));
                    }
                }
                continue;
            }
            if !pad.on_layer(layer) {
                continue;
            }
            let obj = self.pad_obj(pad, layer);
            if !same(&pad.net) {
                let c = clr(&obj);
                if FBox::of_points([(px, py)]).grown(reach + c).overlaps(&area) {
                    clip.push(pad_contour(&fr, pad, c, true));
                }
                continue;
            }
            let (style, gap, spoke) = self.connect(zone, &obj);
            let gap = gap as f64;
            if !FBox::of_points([(px, py)]).grown(reach + gap).overlaps(&area) {
                continue;
            }
            match style {
                ZoneConnect::Solid => anchors.push(pad_anchor(&fr, pad)),
                ZoneConnect::None => clip.push(pad_contour(&fr, pad, gap, true)),
                ZoneConnect::Thermal => {
                    let ring = pad_contour(&fr, pad, gap, true);
                    stubs.push(FBox::of_points(ring.iter().map(|p| (p.x as f64, p.y as f64))));
                    let mut keep = vec![pad_contour(&fr, pad, 0.0, false)];
                    if spoke > 0 {
                        let len = 2.0 * (reach + gap + spoke as f64);
                        let sw = spoke as f64;
                        keep.push(geom::rounded_rect(&fr, px, py, len, sw, pad.rotation, 0.0, false));
                        keep.push(geom::rounded_rect(&fr, px, py, sw, len, pad.rotation, 0.0, false));
                    }
                    clip_shapes.extend(geom::boolean(&[vec![ring]], &[], &keep, OverlayRule::Difference));
                    anchors.push(pad_anchor(&fr, pad));
                }
            }
        }

        let mut fill = if clip.is_empty() && clip_shapes.is_empty() { base } else { geom::boolean(&base, &clip_shapes, &clip, OverlayRule::Difference) };

        // 4. Minimum width: morphological opening (features ≥ ~0.98 × min width survive).
        if zone.min_width > 0 && !fill.is_empty() {
            let h = zone.min_width / 2 - (zone.min_width / 100).max(10);
            if h > 0 {
                fill = geom::opening(&fill, h);
            }
        }

        // 5. Islands.
        if zone.remove_islands && !zone.net.is_empty() {
            fill.retain(|s| connected(s, &anchors, &stubs));
        }

        fill.iter().filter(|s| !s.is_empty()).map(|s| to_polygon(&fr, s)).collect()
    }
}

/// Pad copper grown by `inflate` (circumscribed when `outer`).
fn pad_contour(fr: &Frame, pad: &WorldPad, inflate: f64, outer: bool) -> Contour {
    let (x, y) = fr.f(pad.pos);
    let (w, h) = (pad.width as f64, pad.height as f64);
    let (gw, gh) = ((w + 2.0 * inflate).max(1.0), (h + 2.0 * inflate).max(1.0));
    let r = match pad.shape {
        PadShape::Circle => return geom::rounded_rect(fr, x, y, gw, gw, 0.0, gw / 2.0, outer),
        PadShape::Oval => w.min(h) / 2.0 + inflate,
        PadShape::RoundRect => w.min(h) * pad.corner_ratio.clamp(0.0, 0.5) + inflate,
        PadShape::Rect => inflate,
    };
    geom::rounded_rect(fr, x, y, gw, gh, pad.rotation, r.max(0.0), outer)
}

/// Capsule inscribed in the pad, used for island connectivity.
fn pad_anchor(fr: &Frame, pad: &WorldPad) -> Anchor {
    let c = fr.f(pad.pos);
    if pad.shape == PadShape::Circle {
        return Anchor { a: c, b: c, r: pad.width as f64 / 2.0 };
    }
    let (w, h) = (pad.width as f64, pad.height as f64);
    let (long, short, along) = if w >= h { (w, h, pad.rotation) } else { (h, w, pad.rotation + 90.0) };
    let half = (long - short) / 2.0;
    let (s, k) = along.to_radians().sin_cos();
    Anchor { a: (c.0 - half * k, c.1 - half * s), b: (c.0 + half * k, c.1 + half * s), r: short / 2.0 }
}

/// Whether an island touches same-net copper (and is more than the leftover
/// of a thermal relief around a single pad).
fn connected(s: &Shape, anchors: &[Anchor], stubs: &[FBox]) -> bool {
    let bb = FBox::of_shape(s);
    if stubs.iter().any(|st| st.contains(&bb)) {
        return false;
    }
    anchors.iter().any(|a| a.bbox().overlaps(&bb) && geom::capsule_touches_shape(s, a.a, a.b, a.r))
}

fn to_polygon(fr: &Frame, s: &Shape) -> ZonePolygon {
    let conv = |c: &Contour| c.iter().map(|p| fr.world(*p)).collect::<Vec<Point>>();
    let mut outer = conv(&s[0]);
    if super::geometry::signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let holes = s[1..]
        .iter()
        .map(|h| {
            let mut h = conv(h);
            if super::geometry::signed_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    ZonePolygon { outer, holes }
}

#[cfg(test)]
#[path = "fill_tests.rs"]
mod tests;
