//! Design rule check.
//!
//! [`run`] checks the whole board; [`run_near`] is the online variant that
//! only builds geometry around the given items and only reports violations
//! involving them. Copper checks share one pass over an R-tree of copper
//! objects: each candidate pair on a shared layer gets its clearance from
//! precomputed rule profiles (O(1) per pair) and one exact gap computation,
//! which also feeds the union-find used for shorts and the ratsnest.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Serialize;

use crate::document::{Document, ItemId, LayerId};
use crate::font;
use crate::item::{FillStyle, Item};
use crate::layer;
use crate::units::{Coord, Point, to_mm};

use super::board::{BoardSettings, ViaKind};
use super::connectivity::geo::{self, Bb, BoxTree, Geo, P, PolyGeo, Prim, TOUCH};
use super::connectivity::{self as conn, CopperObject, Index, RoutingStats, Shape, Uf, pt, to_point};
use super::geometry::{board_settings, outline_loops, world_graphics, world_pads};
use super::items::Side;
use super::query::{ClearanceProfile, Obj, ObjKind, RuleSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrcViolation {
    pub severity: Severity,
    /// `clearance`, `short`, `track-width`, `via-style`, `hole-size`,
    /// `board-outline`, `no-outline`, `courtyard-overlap`, `silk-over-pad`,
    /// `keepout`, `unrouted`, `missing-footprint`, `rule-error`.
    pub code: &'static str,
    /// Rule (or net class) that applied.
    pub rule: String,
    pub message: String,
    pub items: Vec<ItemId>,
    /// Where to zoom to.
    pub pos: Point,
    pub layer: Option<LayerId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrcReport {
    pub violations: Vec<DrcViolation>,
    pub stats: RoutingStats,
    pub rule_errors: Vec<String>,
}

/// Clearance shortfalls below this (nm) are rounding noise, not violations.
const TOLERANCE: f64 = 5.0;
/// Outline / courtyard chains closer than this are joined into loops.
const CHAIN_TOL: Coord = 10_000;

/// Full design rule check.
pub fn run(doc: &Document) -> DrcReport {
    let board = conn::settings_or_default(doc);
    let mut d = Drc::new(doc, &board, None);
    d.rule_errors();
    d.missing_footprints();
    let idx = Index::new(conn::copper_objects(doc, &board));
    let mut uf = Uf::new(idx.objs.len());
    conn::join_same_pads(&idx.objs, &mut uf);
    let shorted = d.copper_pairs(&idx, Some(&mut uf));
    let labels = uf.labels();
    d.bridged_shorts(&idx, &labels, &shorted);
    d.object_rules(&idx);
    d.pad_holes();
    d.outline(&idx);
    d.keepouts(&idx);
    let gfx = d.footprint_graphics(None);
    d.courtyards(&gfx);
    d.silk(&idx, &gfx, None);
    let wires = conn::airwires(&idx.objs, &labels);
    d.unrouted(&idx, &wires);
    DrcReport { stats: conn::stats_of(&idx.objs, wires.len()), rule_errors: d.rules.errors().to_vec(), violations: d.out }
}

/// Online DRC: only checks involving `items` (geometry is built only around them).
pub fn run_near(doc: &Document, items: &[ItemId]) -> Vec<DrcViolation> {
    let board = conn::settings_or_default(doc);
    let focus: HashSet<ItemId> = items.iter().copied().filter(|id| doc.get(*id).is_some()).collect();
    if focus.is_empty() {
        return vec![];
    }
    let mut d = Drc::new(doc, &board, Some(focus));
    d.missing_footprints();
    // Region: the focus items' extent grown by the largest distance any check uses.
    let focus = d.focus.as_ref().unwrap();
    let mut region = conn::build_objects(doc, &board, None, Some(focus)).iter().fold(Bb::EMPTY, |b, o| b.union(o.geo().bb));
    for &id in focus {
        match doc.get(id) {
            Some(Item::Footprint(fp)) => {
                if let Some(f) = doc.cached_footprint(&fp.footprint) {
                    region = region.union(Bb::of(pt(fp.pos), pt(fp.pos)).grow(conn::footprint_extent(f)));
                }
            }
            Some(Item::Zone(z)) => z.outline.iter().for_each(|p| region.add(pt(*p))),
            Some(item) => {
                if let Some(g) = stroke_geo(item) {
                    region = region.union(g.bb);
                }
            }
            None => {}
        }
    }
    if region.is_empty() {
        return d.out;
    }
    let margin = [d.rules.max_clearance(), d.rules.silk_clearance(), d.rules.component_clearance(), d.rules.outline_clearance()].into_iter().max().unwrap_or(0);
    let region = region.grow(margin as f64 + 1000.0);
    let idx = Index::new(conn::build_objects(doc, &board, Some(region), None));
    d.copper_pairs(&idx, None);
    d.object_rules(&idx);
    d.pad_holes();
    d.outline(&idx);
    d.keepouts(&idx);
    let gfx = d.footprint_graphics(Some(region));
    d.courtyards(&gfx);
    d.silk(&idx, &gfx, Some(region));
    d.out
}

fn mm_str(nm: f64) -> String {
    format!("{:.3} mm", nm / 1e6)
}

fn mmc(c: Coord) -> String {
    format!("{:.3} mm", to_mm(c))
}

fn net_label(n: &str) -> &str {
    if n.is_empty() { "No Net" } else { n }
}

fn describe(o: &CopperObject) -> String {
    let what = match o.kind {
        ObjKind::Pad => format!("Pad {}.{}", o.designator.as_deref().unwrap_or("?"), o.pad.as_deref().unwrap_or("?")),
        ObjKind::Track => "Track".into(),
        ObjKind::Arc => "Arc".into(),
        ObjKind::Via => "Via".into(),
        ObjKind::Zone => "Zone".into(),
        k => format!("{k:?}"),
    };
    format!("{what} ({})", net_label(&o.net))
}

/// Footprint geometry used by the courtyard and silk checks.
struct FpGfx {
    id: ItemId,
    designator: String,
    courtyards: Vec<(Side, Geo)>,
    silk: Vec<(Side, Geo)>,
}

struct Drc<'a> {
    doc: &'a Document,
    board: &'a BoardSettings,
    rules: RuleSet<'a>,
    focus: Option<HashSet<ItemId>>,
    out: Vec<DrcViolation>,
}

impl<'a> Drc<'a> {
    fn new(doc: &'a Document, board: &'a BoardSettings, focus: Option<HashSet<ItemId>>) -> Self {
        Drc { doc, board, rules: RuleSet::new(board), focus, out: Vec::new() }
    }

    fn focused(&self, id: ItemId) -> bool {
        self.focus.as_ref().is_none_or(|f| f.contains(&id))
    }

    fn layer_name(&self, l: LayerId) -> String {
        self.board.stackup.layer_name(l).map_or_else(|| format!("layer {}", l.0), str::to_string)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, severity: Severity, code: &'static str, rule: String, message: String, items: Vec<ItemId>, pos: Point, layer: Option<LayerId>) {
        let mut items = items;
        let mut seen = HashSet::new();
        items.retain(|i| seen.insert(*i));
        self.out.push(DrcViolation { severity, code, rule, message, items, pos, layer });
    }

    fn rule_errors(&mut self) {
        let board_id = board_settings(self.doc).map(|(id, _)| id);
        let origin = self.board.origin;
        for e in self.rules.errors().to_vec() {
            let rule = e.split(':').next().unwrap_or("").to_string();
            self.push(Severity::Warning, "rule-error", rule, format!("Rule scope does not parse: {e}"), board_id.into_iter().collect(), origin, None);
        }
    }

    fn missing_footprints(&mut self) {
        let doc = self.doc;
        for (id, item) in doc.items() {
            if let Item::Footprint(fp) = item
                && self.focused(id)
                && doc.cached_footprint(&fp.footprint).is_none()
            {
                self.push(
                    Severity::Error,
                    "missing-footprint",
                    String::new(),
                    format!("{}: footprint '{}' is missing from the board's footprint cache", fp.designator, fp.footprint),
                    vec![id],
                    fp.pos,
                    None,
                );
            }
        }
    }

    /// Clearance and short checks; joins touching copper in `uf`. Returns
    /// objects involved in direct shorts.
    fn copper_pairs(&mut self, idx: &Index, mut uf: Option<&mut Uf>) -> Vec<usize> {
        let profiles: Vec<Vec<ClearanceProfile>> =
            idx.objs.iter().map(|o| o.layers.iter().map(|&l| self.rules.clearance_profile(&o.obj(Some(l)))).collect()).collect();
        let max_cl = self.rules.max_clearance() as f64;
        let mut shorted = Vec::new();
        let full = self.focus.is_none();
        for i in 0..idx.objs.len() {
            let fi = self.focused(idx.objs[i].item);
            if !full && !fi {
                continue;
            }
            for j in idx.tree.query(idx.geos[i].bb.grow(max_cl.max(TOUCH))) {
                if j == i || (full && j < i) || (!full && j < i && self.focused(idx.objs[j].item)) {
                    continue;
                }
                let (a, b) = (&idx.objs[i], &idx.objs[j]);
                if a.same_object(b) {
                    continue;
                }
                // Required clearance: the largest over shared layers.
                let mut req: Option<(Coord, LayerId, String)> = None;
                for (li, l) in a.layers.iter().enumerate() {
                    if let Some(lj) = b.layers.iter().position(|x| x == l) {
                        let (c, src) = self.rules.clearance_between(&profiles[i][li], &profiles[j][lj]);
                        if req.as_ref().is_none_or(|r| c > r.0) {
                            req = Some((c, *l, src.to_string()));
                        }
                    }
                }
                let Some((cl, layer, rule)) = req else { continue };
                let same_net = !a.net.is_empty() && a.net == b.net;
                let limit = if same_net { TOUCH } else { (cl as f64).max(TOUCH) };
                let Some((g, gp)) = geo::gap(&idx.geos[i], &idx.geos[j], limit) else { continue };
                let touching = g < TOUCH;
                if touching && let Some(uf) = uf.as_deref_mut() {
                    uf.union(i, j);
                }
                if same_net {
                    continue;
                }
                let pos = to_point(gp);
                if touching {
                    // Unassigned copper touching a net simply joins it.
                    if !a.net.is_empty() && !b.net.is_empty() {
                        shorted.push(i);
                        let msg = format!("Short circuit between {} and {}: {} touches {} on {}", a.net, b.net, describe(a), describe(b), self.layer_name(layer));
                        self.push(Severity::Error, "short", rule, msg, vec![a.item, b.item], pos, Some(layer));
                    }
                } else if g < cl as f64 - TOLERANCE {
                    let msg = format!("Clearance {} < {}: {} to {} on {}", mm_str(g), mmc(cl), describe(a), describe(b), self.layer_name(layer));
                    self.push(Severity::Error, "clearance", rule, msg, vec![a.item, b.item], pos, Some(layer));
                }
            }
        }
        shorted
    }

    /// Different nets joined through unassigned copper (no direct contact).
    fn bridged_shorts(&mut self, idx: &Index, labels: &[usize], shorted: &[usize]) {
        let reported: HashSet<usize> = shorted.iter().map(|&i| labels[i]).collect();
        let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, &c) in labels.iter().enumerate() {
            clusters.entry(c).or_default().push(i);
        }
        for (c, members) in clusters {
            if reported.contains(&c) {
                continue;
            }
            let nets: BTreeSet<&str> = members.iter().map(|&i| idx.objs[i].net.as_str()).filter(|n| !n.is_empty()).collect();
            if nets.len() < 2 {
                continue;
            }
            let bridge = members.iter().copied().find(|&i| idx.objs[i].net.is_empty()).unwrap_or(members[0]);
            let mut items: Vec<ItemId> = members.iter().filter(|&&i| idx.objs[i].net.is_empty()).map(|&i| idx.objs[i].item).collect();
            for n in &nets {
                if let Some(&i) = members.iter().find(|&&i| idx.objs[i].net == *n) {
                    items.push(idx.objs[i].item);
                }
            }
            let names: Vec<&str> = nets.into_iter().collect();
            let o = &idx.objs[bridge];
            let msg = format!("Short circuit: nets {} are connected through unassigned copper", names.join(", "));
            self.push(Severity::Error, "short", String::new(), msg, items, o.pos, o.layers.first().copied());
        }
    }

    /// Track width and via style.
    fn object_rules(&mut self, idx: &Index) {
        for o in &idx.objs {
            if !self.focused(o.item) {
                continue;
            }
            match (&o.kind, &o.shape) {
                (ObjKind::Track, Shape::Segment { width, .. }) | (ObjKind::Arc, Shape::Arc { arc: crate::item::Arc { width, .. } }) => {
                    let l = o.layers[0];
                    let ((min, _, max), src) = self.rules.width_rule(&o.obj(Some(l)));
                    let problem = if *width < min {
                        Some(format!("below the minimum {}", mmc(min)))
                    } else if *width > max {
                        Some(format!("above the maximum {}", mmc(max)))
                    } else {
                        None
                    };
                    if let Some(p) = problem {
                        let msg = format!("{} width {} is {p}", describe(o), mmc(*width));
                        self.push(Severity::Error, "track-width", src.to_string(), msg, vec![o.item], o.pos, Some(l));
                    }
                }
                (ObjKind::Via, Shape::Circle { radius, .. }) => {
                    let dia = radius * 2;
                    let drill = o.drill.map_or(0, |d| d.0.min(d.1));
                    // A via matching a defined via type is an approved style; laser microvias
                    // are also exempt from the mechanical minimum hole size.
                    let via_type = match self.doc.get(o.item) {
                        Some(Item::Via(v)) => self.board.via_type_of(v.from_layer, v.to_layer, v.diameter, v.drill),
                        _ => None,
                    };
                    if let Some(t) = via_type {
                        if self.board.stackup.via_kind(t.from_layer, t.to_layer, t.drill) != ViaKind::Micro {
                            self.hole(o.item, &o.obj(None), (drill, drill), o.pos, describe(o));
                        }
                        continue;
                    }
                    let ((rd, rdrill, ring), src) = self.rules.via_style_rule(&o.obj(None));
                    let mut problems = Vec::new();
                    if dia < rd {
                        problems.push(format!("diameter {} < {}", mmc(dia), mmc(rd)));
                    }
                    if drill < rdrill {
                        problems.push(format!("drill {} < {}", mmc(drill), mmc(rdrill)));
                    }
                    if (dia - drill) / 2 < ring {
                        problems.push(format!("annular ring {} < {}", mmc((dia - drill) / 2), mmc(ring)));
                    }
                    if !problems.is_empty() {
                        let msg = format!("{}: {}", describe(o), problems.join(", "));
                        self.push(Severity::Error, "via-style", src.to_string(), msg, vec![o.item], o.pos, None);
                    }
                    self.hole(o.item, &o.obj(None), (drill, drill), o.pos, describe(o));
                }
                _ => {}
            }
        }
    }

    fn hole(&mut self, item: ItemId, obj: &Obj, drill: (Coord, Coord), pos: Point, what: String) {
        let size = drill.0.min(drill.1);
        let ((min, max), src) = self.rules.hole_size_rule(obj);
        let problem = if size < min {
            format!("below the minimum {}", mmc(min))
        } else if size > max {
            format!("above the maximum {}", mmc(max))
        } else {
            return;
        };
        let msg = format!("{what}: hole size {} is {problem}", mmc(size));
        self.push(Severity::Error, "hole-size", src.to_string(), msg, vec![item], pos, None);
    }

    /// Hole sizes of footprint pads (including non-plated holes).
    fn pad_holes(&mut self) {
        let doc = self.doc;
        for (id, item) in doc.items() {
            let Item::Footprint(fp) = item else { continue };
            if !self.focused(id) {
                continue;
            }
            let Some(f) = doc.cached_footprint(&fp.footprint) else { continue };
            for p in world_pads(id, fp, f, self.board) {
                let Some(d) = p.drill else { continue };
                let mut o = Obj::new(ObjKind::Pad, &p.net, None);
                o.designator = Some(&p.designator);
                o.footprint = Some(&fp.footprint);
                let what = format!("Pad {}.{}", p.designator, p.number);
                self.hole(id, &o, (d.width, d.height), p.pos, what);
            }
        }
    }

    fn outline(&mut self, idx: &Index) {
        let loops = outline_loops(self.doc, CHAIN_TOL);
        if loops.is_empty() {
            if self.focus.is_none() && !idx.objs.is_empty() {
                let o = &idx.objs[0];
                self.push(Severity::Warning, "no-outline", String::new(), "The board has copper but no closed board outline".into(), vec![], o.pos, None);
            }
            return;
        }
        let (ocl, src) = self.rules.outline_clearance_rule();
        let rings: Vec<PolyGeo> = loops.iter().map(|l| PolyGeo::new(vec![l.iter().map(|p| pt(*p)).collect()])).collect();
        let edges: Vec<Geo> = loops.iter().flat_map(|l| (0..l.len()).map(move |i| Geo::cap(pt(l[i]), pt(l[(i + 1) % l.len()]), 0.0))).collect();
        let tree = BoxTree::new(edges.iter().enumerate().map(|(i, g)| (g.bb, i)));
        for (i, o) in idx.objs.iter().enumerate() {
            if !self.focused(o.item) {
                continue;
            }
            let g = &idx.geos[i];
            let mut best: Option<geo::Gap> = None;
            for e in tree.query(g.bb.grow(ocl as f64)) {
                let lim = best.map_or(ocl as f64, |b| b.0);
                if let Some(x) = geo::gap(g, &edges[e], lim)
                    && best.is_none_or(|b| x.0 < b.0)
                {
                    best = Some(x);
                }
            }
            let layer = o.layers.first().copied();
            match best {
                Some((d, p)) if d < ocl as f64 - TOLERANCE => {
                    let msg = if d <= 0.0 {
                        format!("{} crosses the board outline", describe(o))
                    } else {
                        format!("{} is {} from the board outline (minimum {})", describe(o), mm_str(d), mmc(ocl))
                    };
                    self.push(Severity::Error, "board-outline", src.to_string(), msg, vec![o.item], to_point(p), layer);
                }
                Some(_) => {}
                None => {
                    let inside = g.anchor().is_some_and(|a| rings.iter().filter(|r| r.contains(a)).count() % 2 == 1);
                    if !inside {
                        let msg = format!("{} is outside the board outline", describe(o));
                        self.push(Severity::Error, "board-outline", src.to_string(), msg, vec![o.item], o.pos, layer);
                    }
                }
            }
        }
    }

    fn keepouts(&mut self, idx: &Index) {
        let doc = self.doc;
        for (zid, item) in doc.items() {
            let Item::Zone(z) = item else { continue };
            if !z.keepout || z.outline.len() < 3 {
                continue;
            }
            let area = Geo::new(vec![Prim::Poly(Box::new(PolyGeo::new(vec![z.outline.iter().map(|p| pt(*p)).collect()])))]);
            for j in idx.tree.query(area.bb) {
                let o = &idx.objs[j];
                if o.item == zid || o.kind == ObjKind::Pad || !o.on_layer(z.layer) || !(self.focused(o.item) || self.focused(zid)) {
                    continue;
                }
                if let Some((g, p)) = geo::gap(&idx.geos[j], &area, 0.0)
                    && g < 0.0
                {
                    let name = if z.name.is_empty() { String::new() } else { format!(" '{}'", z.name) };
                    let msg = format!("{} is inside keep-out area{name} on {}", describe(o), self.layer_name(z.layer));
                    self.push(Severity::Error, "keepout", z.name.clone(), msg, vec![o.item, zid], to_point(p), Some(z.layer));
                }
            }
        }
    }

    fn footprint_graphics(&self, region: Option<Bb>) -> Vec<FpGfx> {
        let mut out = Vec::new();
        for (id, item) in self.doc.items() {
            let Item::Footprint(fp) = item else { continue };
            let Some(f) = self.doc.cached_footprint(&fp.footprint) else { continue };
            if let Some(r) = region
                && !r.intersects(&Bb::of(pt(fp.pos), pt(fp.pos)).grow(conn::footprint_extent(f)))
            {
                continue;
            }
            let gfx = world_graphics(fp, f);
            let mut courtyards = Vec::new();
            for (side, l) in [(Side::Top, layer::TOP_COURTYARD), (Side::Bottom, layer::BOTTOM_COURTYARD)] {
                let items: Vec<&Item> = gfx.iter().filter(|i| item_layer(i) == Some(l)).collect();
                if let Some(g) = region_geo(&items) {
                    courtyards.push((side, g));
                }
            }
            if courtyards.is_empty() {
                // Fallback: bounding box of the pads.
                let mut bb = Bb::EMPTY;
                for p in world_pads(id, fp, f, self.board) {
                    p.polygon(0).iter().for_each(|q| bb.add(pt(*q)));
                }
                if !bb.is_empty() {
                    let ring = vec![bb.min, [bb.max[0], bb.min[1]], bb.max, [bb.min[0], bb.max[1]]];
                    courtyards.push((fp.side, Geo::new(vec![Prim::Poly(Box::new(PolyGeo::new(vec![ring])))])));
                }
            }
            let silk = gfx
                .iter()
                .filter_map(|i| {
                    let side = match item_layer(i)? {
                        layer::TOP_SILK => Side::Top,
                        layer::BOTTOM_SILK => Side::Bottom,
                        _ => return None,
                    };
                    Some((side, stroke_geo(i)?))
                })
                .collect();
            out.push(FpGfx { id, designator: fp.designator.clone(), courtyards, silk });
        }
        out
    }

    fn courtyards(&mut self, gfx: &[FpGfx]) {
        let (cc, src) = self.rules.component_clearance_rule();
        let list: Vec<(usize, Side, &Geo)> = gfx.iter().enumerate().flat_map(|(k, f)| f.courtyards.iter().map(move |(s, g)| (k, *s, g))).collect();
        let tree = BoxTree::new(list.iter().enumerate().map(|(i, c)| (c.2.bb, i)));
        let lim = cc.max(0) as f64;
        let mut seen = HashSet::new();
        for (i, &(fa, sa, ga)) in list.iter().enumerate() {
            for j in tree.query(ga.bb.grow(lim)) {
                let (fb, sb, gb) = list[j];
                if fb == fa || sb != sa || !(self.focused(gfx[fa].id) || self.focused(gfx[fb].id)) {
                    continue;
                }
                let key = (fa.min(fb), fa.max(fb), sa);
                if j < i || seen.contains(&key) {
                    continue;
                }
                // With a zero minimum only real overlap counts (abutting is fine).
                if let Some((g, p)) = geo::gap(ga, gb, lim)
                    && g < lim - TOLERANCE.min(lim)
                {
                    seen.insert(key);
                    let (a, b) = (&gfx[fa], &gfx[fb]);
                    let msg = if g < 0.0 {
                        format!("Courtyards of {} and {} overlap", a.designator, b.designator)
                    } else {
                        format!("Courtyards of {} and {} are {} apart (minimum {})", a.designator, b.designator, mm_str(g), mmc(cc))
                    };
                    let l = if sa == Side::Top { layer::TOP_COURTYARD } else { layer::BOTTOM_COURTYARD };
                    self.push(Severity::Error, "courtyard-overlap", src.to_string(), msg, vec![a.id, b.id], to_point(p), Some(l));
                }
            }
        }
    }

    fn silk(&mut self, idx: &Index, gfx: &[FpGfx], region: Option<Bb>) {
        let (sc, src) = self.rules.silk_clearance_rule();
        let mut silk: Vec<(ItemId, Side, &str, Geo)> = Vec::new();
        for (id, item) in self.doc.items() {
            let side = match item_layer(item) {
                Some(layer::TOP_SILK) => Side::Top,
                Some(layer::BOTTOM_SILK) => Side::Bottom,
                _ => continue,
            };
            if let Some(g) = stroke_geo(item)
                && region.is_none_or(|r| r.intersects(&g.bb))
            {
                silk.push((id, side, "", g));
            }
        }
        let mut owned: Vec<(ItemId, Side, &str, &Geo)> = silk.iter().map(|(a, b, c, d)| (*a, *b, *c, d)).collect();
        for f in gfx {
            owned.extend(f.silk.iter().map(|(s, g)| (f.id, *s, f.designator.as_str(), g)));
        }
        let mut seen = HashSet::new();
        for (sid, side, owner, g) in owned {
            let copper = if side == Side::Top { layer::TOP_COPPER } else { layer::BOTTOM_COPPER };
            for j in idx.tree.query(g.bb.grow(sc as f64)) {
                let o = &idx.objs[j];
                if o.kind != ObjKind::Pad || !o.on_layer(copper) || !(self.focused(sid) || self.focused(o.item)) {
                    continue;
                }
                let Some((d, p)) = geo::gap(g, &idx.geos[j], sc as f64) else { continue };
                if d < sc as f64 - TOLERANCE && seen.insert((sid, j)) {
                    let whose = if owner.is_empty() { String::new() } else { format!(" of {owner}") };
                    let msg = if d < 0.0 {
                        format!("Silkscreen{whose} overlaps {}", describe(o))
                    } else {
                        format!("Silkscreen{whose} is {} from {} (minimum {})", mm_str(d), describe(o), mmc(sc))
                    };
                    let l = if side == Side::Top { layer::TOP_SILK } else { layer::BOTTOM_SILK };
                    self.push(Severity::Warning, "silk-over-pad", src.to_string(), msg, vec![sid, o.item], to_point(p), Some(l));
                }
            }
        }
    }

    fn unrouted(&mut self, idx: &Index, wires: &[conn::Wire]) {
        let mut by_net: BTreeMap<&str, Vec<&conn::Wire>> = BTreeMap::new();
        for w in wires {
            by_net.entry(w.wire.net.as_str()).or_default().push(w);
        }
        for (net, list) in by_net {
            let first = &list[0].wire;
            let pos = Point::new((first.a.x + first.b.x) / 2, (first.a.y + first.b.y) / 2);
            let items = list.iter().flat_map(|w| [idx.objs[w.objs.0].item, idx.objs[w.objs.1].item]).collect();
            let n = list.len();
            let msg = format!("Net {net} has {n} unrouted connection{}", if n == 1 { "" } else { "s" });
            self.push(Severity::Warning, "unrouted", String::new(), msg, items, pos, None);
        }
    }
}

fn item_layer(i: &Item) -> Option<LayerId> {
    Some(match i {
        Item::Line(x) => x.layer,
        Item::Arc(x) => x.layer,
        Item::Circle(x) => x.layer,
        Item::Rect(x) => x.layer,
        Item::Polyline(x) => x.layer,
        Item::Text(x) => x.layer,
        _ => return None,
    })
}

fn rect_ring(a: Point, b: Point) -> Vec<P> {
    vec![pt(a), [b.x as f64, a.y as f64], pt(b), [a.x as f64, b.y as f64]]
}

/// Stroked outline of a graphic (silkscreen): lines, arcs, circles, rects,
/// polylines (filled shapes included) and text strokes.
fn stroke_geo(item: &Item) -> Option<Geo> {
    let mut prims = Vec::new();
    let r = |w: Coord| w.max(1) as f64 / 2.0;
    match item {
        Item::Line(l) => prims.push(Prim::Cap { a: pt(l.a), b: pt(l.b), r: r(l.width) }),
        Item::Arc(a) => geo::polyline_caps(&geo::arc_polyline(pt(a.center), a.radius as f64, a.start_angle, a.sweep), r(a.width), false, &mut prims),
        Item::Circle(c) => {
            if c.fill == FillStyle::None {
                geo::polyline_caps(&geo::arc_polyline(pt(c.center), c.radius as f64, 0.0, 360.0), r(c.width), false, &mut prims);
            } else {
                prims.push(Prim::Cap { a: pt(c.center), b: pt(c.center), r: c.radius as f64 + r(c.width) });
            }
        }
        Item::Rect(x) => {
            let ring = rect_ring(x.a, x.b);
            geo::polyline_caps(&ring, r(x.width), true, &mut prims);
            if x.fill != FillStyle::None {
                prims.push(Prim::Poly(Box::new(PolyGeo::new(vec![ring]))));
            }
        }
        Item::Polyline(pl) => {
            let pts: Vec<P> = pl.points.iter().map(|p| pt(*p)).collect();
            geo::polyline_caps(&pts, r(pl.width), pl.closed, &mut prims);
            if pl.closed && pl.fill != FillStyle::None {
                prims.push(Prim::Poly(Box::new(PolyGeo::new(vec![pts]))));
            }
        }
        Item::Text(t) => {
            let w = r(font::stroke_width(t.height));
            for (a, b) in font::text_segments(&t.text, t.pos, font::TextStyle::of(t)) {
                prims.push(Prim::Cap { a: pt(a), b: pt(b), r: w });
            }
        }
        _ => return None,
    }
    let g = Geo::new(prims);
    (!g.prims.is_empty()).then_some(g)
}

/// Area enclosed by courtyard graphics: closed shapes are filled, open
/// line/arc chains are joined into loops; leftovers count as thin lines.
fn region_geo(items: &[&Item]) -> Option<Geo> {
    let mut prims = Vec::new();
    let mut chains: Vec<Vec<P>> = Vec::new();
    for item in items {
        match item {
            Item::Rect(x) => prims.push(Prim::Poly(Box::new(PolyGeo::new(vec![rect_ring(x.a, x.b)])))),
            Item::Circle(c) => prims.push(Prim::Cap { a: pt(c.center), b: pt(c.center), r: c.radius as f64 }),
            Item::Polyline(pl) if pl.closed => prims.push(Prim::Poly(Box::new(PolyGeo::new(vec![pl.points.iter().map(|p| pt(*p)).collect()])))),
            Item::Polyline(pl) => chains.push(pl.points.iter().map(|p| pt(*p)).collect()),
            Item::Line(l) => chains.push(vec![pt(l.a), pt(l.b)]),
            Item::Arc(a) => chains.push(geo::arc_polyline(pt(a.center), a.radius as f64, a.start_angle, a.sweep)),
            _ => {}
        }
    }
    let tol = CHAIN_TOL as f64;
    let close = |a: P, b: P| (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol;
    chains.retain(|c| !c.is_empty());
    while let Some(mut chain) = chains.pop() {
        loop {
            let (start, end) = (chain[0], *chain.last().unwrap());
            if chain.len() > 2 && close(start, end) {
                chain.pop();
                prims.push(Prim::Poly(Box::new(PolyGeo::new(vec![chain]))));
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
                None => {
                    geo::polyline_caps(&chain, 0.0, false, &mut prims);
                    break;
                }
            }
        }
    }
    let g = Geo::new(prims);
    (!g.prims.is_empty()).then_some(g)
}

#[cfg(test)]
#[path = "drc_tests.rs"]
mod tests;
