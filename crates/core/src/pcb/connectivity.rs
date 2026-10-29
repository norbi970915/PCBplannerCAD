//! Copper connectivity and ratsnest.
//!
//! Every piece of copper (tracks, arc tracks, vias, pads, zone fills) becomes
//! a [`CopperObject`]. Objects that physically touch on a shared copper layer
//! are merged into clusters (union-find over an R-tree of bounding boxes).
//! Connectivity is purely geometric: copper of different nets that touches is
//! one cluster (a short, reported by DRC).
//!
//! The ratsnest connects, per net, the clusters holding that net's pads with
//! a minimum spanning tree over anchor points (pad centers, track ends, via
//! centers).

#[path = "conn_geom.rs"]
pub(crate) mod geo;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::document::{Document, ItemId, LayerId};
use crate::geom::rotate;
use crate::item::{Item, PadShape};
use crate::units::{Coord, Point};

use super::board::BoardSettings;
use super::geometry::{WorldPad, board_settings, world_pads};
use super::items::ZonePolygon;
use super::query::{Obj, ObjKind};
use geo::{Bb, BoxTree, Geo, P, PolyGeo, Prim, TOUCH};

/// Copper outline of an object.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// Round-capped segment of total `width`.
    Segment { a: Point, b: Point, width: Coord },
    /// Arc track (`arc.width` is the copper width).
    Arc { arc: crate::item::Arc },
    Circle { center: Point, radius: Coord },
    Polygon(Vec<Point>),
    PolygonWithHoles(ZonePolygon),
}

/// One piece of copper on one or more layers.
#[derive(Clone, Debug, PartialEq)]
pub struct CopperObject {
    pub item: ItemId,
    /// Pad number (footprint pads only).
    pub pad: Option<String>,
    pub net: String,
    pub layers: Vec<LayerId>,
    pub shape: Shape,
    pub kind: ObjKind,
    /// Owning footprint designator (pads).
    pub designator: Option<String>,
    /// Owning footprint cache key (pads).
    pub footprint: Option<String>,
    /// SMD pad (vs plated through-hole).
    pub smd: bool,
    /// Drill (width, height) of pads and vias.
    pub drill: Option<(Coord, Coord)>,
    /// Representative position: pad/via center, track midpoint, first fill vertex.
    pub pos: Point,
}

impl CopperObject {
    /// Rule-query facts of this object on `layer`.
    pub fn obj(&self, layer: Option<LayerId>) -> Obj<'_> {
        Obj { kind: self.kind, net: &self.net, layer, designator: self.designator.as_deref(), footprint: self.footprint.as_deref(), smd: self.smd }
    }

    pub fn on_layer(&self, l: LayerId) -> bool {
        self.layers.contains(&l)
    }

    pub fn shares_layer(&self, o: &CopperObject) -> bool {
        self.layers.iter().any(|l| o.layers.contains(l))
    }

    /// Same physical object (or pads sharing a number in one footprint).
    pub fn same_object(&self, o: &CopperObject) -> bool {
        self.item == o.item && self.pad == o.pad
    }

    /// Ratsnest anchor points.
    pub fn anchors(&self) -> Vec<Point> {
        match (&self.kind, &self.shape) {
            (ObjKind::Zone, _) => vec![],
            (_, Shape::Segment { a, b, .. }) if self.kind != ObjKind::Pad => vec![*a, *b],
            (_, Shape::Arc { arc }) => vec![arc.start(), arc.end()],
            _ => vec![self.pos],
        }
    }

    pub(crate) fn geo(&self) -> Geo {
        shape_geo(&self.shape)
    }
}

pub(crate) fn pt(p: Point) -> P {
    [p.x as f64, p.y as f64]
}

pub(crate) fn to_point(p: P) -> Point {
    Point::new(p[0].round() as Coord, p[1].round() as Coord)
}

pub(crate) fn shape_geo(s: &Shape) -> Geo {
    match s {
        Shape::Segment { a, b, width } => Geo::cap(pt(*a), pt(*b), *width as f64 / 2.0),
        Shape::Arc { arc } => {
            let pts = geo::arc_polyline(pt(arc.center), arc.radius as f64, arc.start_angle, arc.sweep);
            let mut prims = Vec::new();
            geo::polyline_caps(&pts, arc.width as f64 / 2.0, false, &mut prims);
            Geo::new(prims)
        }
        Shape::Circle { center, radius } => Geo::cap(pt(*center), pt(*center), *radius as f64),
        Shape::Polygon(pts) => Geo::new(vec![Prim::Poly(Box::new(PolyGeo::new(vec![pts.iter().map(|p| pt(*p)).collect()])))]),
        Shape::PolygonWithHoles(z) => {
            let rings = std::iter::once(&z.outer).chain(z.holes.iter()).map(|r| r.iter().map(|p| pt(*p)).collect()).collect();
            Geo::new(vec![Prim::Poly(Box::new(PolyGeo::new(rings)))])
        }
    }
}

fn pad_shape(p: &WorldPad) -> Shape {
    match p.shape {
        PadShape::Circle => Shape::Circle { center: p.pos, radius: p.width / 2 },
        PadShape::Oval if p.width != p.height => {
            let (long, short) = (p.width.max(p.height), p.width.min(p.height));
            let along = if p.width >= p.height { 0.0 } else { 90.0 } + p.rotation;
            let half = (long - short) / 2;
            let a = rotate(p.pos.translated(-half, 0), p.pos, along);
            let b = rotate(p.pos.translated(half, 0), p.pos, along);
            Shape::Segment { a, b, width: short }
        }
        PadShape::Oval => Shape::Circle { center: p.pos, radius: p.width / 2 },
        _ => Shape::Polygon(p.polygon(0)),
    }
}

/// Conservative radius around a footprint origin that holds all its items.
pub(crate) fn footprint_extent(f: &crate::library::Footprint) -> f64 {
    let mut r: f64 = 0.0;
    let mut add = |p: Point, extra: Coord| r = r.max((p.x as f64).hypot(p.y as f64) + extra as f64);
    for item in &f.items {
        match item {
            Item::Pad(p) => {add(p.pos,p.width.max(p.height));for shape in p.copper_shapes.values(){add(p.pos,shape.width.max(shape.height));}},
            Item::Line(l) => {
                add(l.a, l.width);
                add(l.b, l.width);
            }
            Item::Arc(a) => add(a.center, a.radius + a.width),
            Item::Circle(c) => add(c.center, c.radius + c.width),
            Item::Rect(x) => {
                add(x.a, x.width);
                add(x.b, x.width);
                add(Point::new(x.a.x, x.b.y), x.width);
                add(Point::new(x.b.x, x.a.y), x.width);
            }
            Item::Polyline(pl) => pl.points.iter().for_each(|p| add(*p, pl.width)),
            Item::Text(t) => add(t.pos, crate::font::text_width(&t.text, t.height).max(t.height) * 2 + t.height),
            _ => {}
        }
    }
    r
}

/// Builds copper objects, optionally only those whose box meets `region`
/// and/or that belong to `only`.
pub(crate) fn build_objects(doc: &Document, board: &BoardSettings, region: Option<Bb>, only: Option<&HashSet<ItemId>>) -> Vec<CopperObject> {
    let keep = |id: ItemId, bb: Bb| only.is_none_or(|s| s.contains(&id)) && region.is_none_or(|r| r.intersects(&bb));
    let mut out = Vec::new();
    let mut extents: HashMap<&str, f64> = HashMap::new();
    for (id, item) in doc.items() {
        match item {
            Item::Track(t) => {
                let bb = Bb::of(pt(t.a), pt(t.b)).grow(t.width as f64 / 2.0);
                if keep(id, bb) {
                    out.push(CopperObject {
                        item: id,
                        pad: None,
                        net: t.net.clone(),
                        layers: vec![t.layer],
                        shape: Shape::Segment { a: t.a, b: t.b, width: t.width },
                        kind: ObjKind::Track,
                        designator: None,
                        footprint: None,
                        smd: false,
                        drill: None,
                        pos: Point::new((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2),
                    });
                }
            }
            Item::ArcTrack(t) => {
                let bb = Bb::of(pt(t.center), pt(t.center)).grow((t.radius + t.width) as f64);
                if keep(id, bb) {
                    let arc = crate::item::Arc { center: t.center, radius: t.radius, start_angle: t.start_angle, sweep: t.sweep, width: t.width, layer: t.layer };
                    let pos = arc.point_at(t.start_angle + t.sweep / 2.0);
                    out.push(CopperObject {
                        item: id,
                        pad: None,
                        net: t.net.clone(),
                        layers: vec![t.layer],
                        shape: Shape::Arc { arc },
                        kind: ObjKind::Arc,
                        designator: None,
                        footprint: None,
                        smd: false,
                        drill: None,
                        pos,
                    });
                }
            }
            Item::Via(v) => {
                let bb = Bb::of(pt(v.pos), pt(v.pos)).grow(v.diameter as f64 / 2.0);
                if keep(id, bb) {
                    out.push(CopperObject {
                        item: id,
                        pad: None,
                        net: v.net.clone(),
                        layers: board.stackup.span(v.from_layer, v.to_layer),
                        shape: Shape::Circle { center: v.pos, radius: v.diameter / 2 },
                        kind: ObjKind::Via,
                        designator: None,
                        footprint: None,
                        smd: false,
                        drill: Some((v.drill, v.drill)),
                        pos: v.pos,
                    });
                }
            }
            Item::Zone(z) if !z.keepout => {
                for f in &z.fill {
                    let mut bb = Bb::EMPTY;
                    f.outer.iter().for_each(|p| bb.add(pt(*p)));
                    if f.outer.len() < 3 || !keep(id, bb) {
                        continue;
                    }
                    out.push(CopperObject {
                        item: id,
                        pad: None,
                        net: z.net.clone(),
                        layers: vec![z.layer],
                        shape: Shape::PolygonWithHoles(f.clone()),
                        kind: ObjKind::Zone,
                        designator: None,
                        footprint: None,
                        smd: false,
                        drill: None,
                        pos: f.outer[0],
                    });
                }
            }
            Item::Footprint(fp) => {
                if only.is_some_and(|s| !s.contains(&id)) {
                    continue;
                }
                let Some(f) = doc.cached_footprint(&fp.footprint) else { continue };
                if let Some(r) = region {
                    let ext = *extents.entry(fp.footprint.as_str()).or_insert_with(|| footprint_extent(f));
                    if !r.intersects(&Bb::of(pt(fp.pos), pt(fp.pos)).grow(ext)) {
                        continue;
                    }
                }
                for p in world_pads(id, fp, f, board) {
                    if p.copper.is_empty() {
                        continue;
                    }
                    let variants=if p.layer_shapes.is_empty(){vec![p]}else{p.copper.iter().map(|layer|{let mut variant=p.for_layer(*layer);variant.copper=vec![*layer];variant}).collect()};
                    for p in variants {
                    let bb = Bb::of(pt(p.pos), pt(p.pos)).grow((p.width as f64).hypot(p.height as f64) / 2.0 + 1.0);
                    if !keep(id, bb) {
                        continue;
                    }
                    let shape = pad_shape(&p);
                    out.push(CopperObject {
                        item: id,
                        net: p.net,
                        layers: p.copper,
                        shape,
                        kind: ObjKind::Pad,
                        designator: Some(p.designator),
                        footprint: Some(fp.footprint.clone()),
                        smd: p.kind == crate::item::PadKind::Smd,
                        drill: p.drill.map(|d| (d.width, d.height)),
                        pos: p.pos,
                        pad: Some(p.number),
                    });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every copper object of the board.
pub fn copper_objects(doc: &Document, board: &BoardSettings) -> Vec<CopperObject> {
    build_objects(doc, board, None, None)
}

/// Copper objects with their geometry and a spatial index.
pub(crate) struct Index {
    pub objs: Vec<CopperObject>,
    pub geos: Vec<Geo>,
    pub tree: BoxTree,
}

impl Index {
    pub fn new(objs: Vec<CopperObject>) -> Index {
        let geos: Vec<Geo> = objs.iter().map(|o| o.geo()).collect();
        let tree = BoxTree::new(geos.iter().enumerate().map(|(i, g)| (g.bb, i)));
        Index { objs, geos, tree }
    }

    pub fn touches(&self, i: usize, j: usize) -> bool {
        self.objs[i].shares_layer(&self.objs[j]) && geo::gap(&self.geos[i], &self.geos[j], TOUCH).is_some_and(|g| g.0 < TOUCH)
    }
}

/// Union-find with path halving.
pub(crate) struct Uf(Vec<usize>);

impl Uf {
    pub fn new(n: usize) -> Uf {
        Uf((0..n).collect())
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }

    /// Dense cluster id per element (numbered by first appearance).
    pub fn labels(&mut self) -> Vec<usize> {
        let mut ids = HashMap::new();
        (0..self.0.len())
            .map(|i| {
                let r = self.find(i);
                let n = ids.len();
                *ids.entry(r).or_insert(n)
            })
            .collect()
    }
}

/// Pads sharing a number within one footprint are connected inside the part.
pub(crate) fn join_same_pads(objs: &[CopperObject], uf: &mut Uf) {
    let mut by_pad: HashMap<(ItemId, &str), usize> = HashMap::new();
    for (i, o) in objs.iter().enumerate() {
        if let Some(p) = &o.pad {
            match by_pad.get(&(o.item, p.as_str())) {
                Some(&j) => uf.union(i, j),
                None => {
                    by_pad.insert((o.item, p.as_str()), i);
                }
            }
        }
    }
}

/// Joins touching objects (and same-numbered pads of one footprint).
pub(crate) fn join_touching(idx: &Index, uf: &mut Uf) {
    join_same_pads(&idx.objs, uf);
    for i in 0..idx.objs.len() {
        for j in idx.tree.query(idx.geos[i].bb.grow(TOUCH)) {
            if j > i && idx.touches(i, j) {
                uf.union(i, j);
            }
        }
    }
}

/// Physically connected copper clusters.
pub struct Connectivity {
    objects: Vec<CopperObject>,
    cluster: Vec<usize>,
    clusters: Vec<Vec<usize>>,
    by_item: HashMap<ItemId, Vec<usize>>,
}

impl Connectivity {
    pub(crate) fn from_labels(objects: Vec<CopperObject>, cluster: Vec<usize>) -> Connectivity {
        let n = cluster.iter().max().map_or(0, |m| m + 1);
        let mut clusters = vec![Vec::new(); n];
        let mut by_item: HashMap<ItemId, Vec<usize>> = HashMap::new();
        for (i, &c) in cluster.iter().enumerate() {
            clusters[c].push(i);
            by_item.entry(objects[i].item).or_default().push(i);
        }
        Connectivity { objects, cluster, clusters, by_item }
    }

    /// All copper objects; cluster lists index into this.
    pub fn objects(&self) -> &[CopperObject] {
        &self.objects
    }

    /// Cluster of an item's copper (`pad` selects a footprint pad; `None`
    /// takes the item's first copper object).
    pub fn cluster_of(&self, item: ItemId, pad: Option<&str>) -> Option<usize> {
        let list = self.by_item.get(&item)?;
        list.iter().find(|&&i| pad.is_none() || self.objects[i].pad.as_deref() == pad).map(|&i| self.cluster[i])
    }

    /// Cluster id of each object, by object index.
    pub fn object_clusters(&self) -> &[usize] {
        &self.cluster
    }

    /// Object indices of every cluster.
    pub fn clusters(&self) -> &[Vec<usize>] {
        &self.clusters
    }

    /// Whether two items' copper is physically connected.
    pub fn connected(&self, a: (ItemId, Option<&str>), b: (ItemId, Option<&str>)) -> bool {
        matches!((self.cluster_of(a.0, a.1), self.cluster_of(b.0, b.1)), (Some(x), Some(y)) if x == y)
    }
}

pub(crate) fn settings_or_default(doc: &Document) -> std::borrow::Cow<'_, BoardSettings> {
    match board_settings(doc) {
        Some((_, b)) => std::borrow::Cow::Borrowed(b),
        None => std::borrow::Cow::Owned(BoardSettings::default()),
    }
}

/// Analyzes the copper connectivity of a PCB document.
pub fn analyze(doc: &Document) -> Connectivity {
    let board = settings_or_default(doc);
    let idx = Index::new(copper_objects(doc, &board));
    let mut uf = Uf::new(idx.objs.len());
    join_touching(&idx, &mut uf);
    let labels = uf.labels();
    Connectivity::from_labels(idx.objs, labels)
}

/// One unrouted connection.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AirWire {
    pub net: String,
    pub a: Point,
    pub b: Point,
}

/// An air wire plus the copper objects at its ends.
pub(crate) struct Wire {
    pub wire: AirWire,
    pub objs: (usize, usize),
}

/// Ratsnest over already clustered objects.
pub(crate) fn airwires(objs: &[CopperObject], cluster: &[usize]) -> Vec<Wire> {
    let mut net_objs: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, o) in objs.iter().enumerate() {
        if !o.net.is_empty() {
            net_objs.entry(o.net.as_str()).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (net, list) in net_objs {
        let pad_clusters: BTreeSet<usize> = list.iter().filter(|&&i| objs[i].kind == ObjKind::Pad).map(|&i| cluster[i]).collect();
        if pad_clusters.len() < 2 {
            continue;
        }
        let local: HashMap<usize, usize> = pad_clusters.iter().enumerate().map(|(k, &c)| (c, k)).collect();
        // Anchor points: (position, local cluster, object).
        let mut pts: Vec<(P, usize, usize)> = Vec::new();
        for &i in &list {
            if let Some(&k) = local.get(&cluster[i]) {
                pts.extend(objs[i].anchors().into_iter().map(|p| (pt(p), k, i)));
            }
        }
        let k = pad_clusters.len();
        let mut members = vec![Vec::new(); k];
        for (pi, p) in pts.iter().enumerate() {
            members[p.1].push(pi);
        }
        let n = pts.len();
        let mut best = vec![(f64::INFINITY, usize::MAX); n];
        let mut done = vec![false; k];
        let add = |c: usize, best: &mut Vec<(f64, usize)>, done: &mut Vec<bool>| {
            done[c] = true;
            for &q in &members[c] {
                for (p, b) in best.iter_mut().enumerate() {
                    if !done[pts[p].1] {
                        let (dx, dy) = (pts[p].0[0] - pts[q].0[0], pts[p].0[1] - pts[q].0[1]);
                        let d = dx * dx + dy * dy;
                        if d < b.0 {
                            *b = (d, q);
                        }
                    }
                }
            }
        };
        add(0, &mut best, &mut done);
        for _ in 1..k {
            let Some(p) = (0..n).filter(|&p| !done[pts[p].1]).min_by(|&a, &b| best[a].0.total_cmp(&best[b].0)) else { break };
            let q = best[p].1;
            out.push(Wire { wire: AirWire { net: net.to_string(), a: to_point(pts[q].0), b: to_point(pts[p].0) }, objs: (pts[q].2, pts[p].2) });
            add(pts[p].1, &mut best, &mut done);
        }
    }
    out
}

/// Unrouted connections of the board.
pub fn ratsnest(doc: &Document) -> Vec<AirWire> {
    let c = analyze(doc);
    airwires(&c.objects, &c.cluster).into_iter().map(|w| w.wire).collect()
}

/// Routing completion figures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingStats {
    /// Nets with at least one pad.
    pub nets: usize,
    /// Pad-to-pad connections required by the netlist (pads − 1 per net).
    pub connections: usize,
    /// Connections still missing (ratsnest lines).
    pub unrouted: usize,
}

pub(crate) fn stats_of(objs: &[CopperObject], unrouted: usize) -> RoutingStats {
    let mut pads: BTreeMap<&str, BTreeSet<(ItemId, &str)>> = BTreeMap::new();
    for o in objs.iter().filter(|o| o.kind == ObjKind::Pad && !o.net.is_empty()) {
        pads.entry(o.net.as_str()).or_default().insert((o.item, o.pad.as_deref().unwrap_or("")));
    }
    RoutingStats { nets: pads.len(), connections: pads.values().map(|s| s.len() - 1).sum(), unrouted }
}

pub fn stats(doc: &Document) -> RoutingStats {
    let c = analyze(doc);
    let unrouted = airwires(&c.objects, &c.cluster).len();
    stats_of(&c.objects, unrouted)
}

/// Tracks, vias, zones and footprints (with a pad on the net) of `net`.
pub fn net_items(doc: &Document, net: &str) -> Vec<ItemId> {
    if net.is_empty() {
        return vec![];
    }
    doc.items()
        .filter(|(_, item)| match item {
            Item::Track(t) => t.net == net,
            Item::ArcTrack(t) => t.net == net,
            Item::Via(v) => v.net == net,
            Item::Zone(z) => z.net == net,
            Item::Footprint(f) => f.pad_nets.values().any(|n| n == net),
            _ => false,
        })
        .map(|(id, _)| id)
        .collect()
}

/// Topmost copper under `p` on `layer`: pads, then vias, then tracks, then
/// zone fills; later items win ties.
pub fn object_at(doc: &Document, p: Point, layer: LayerId) -> Option<(ItemId, String)> {
    let board = settings_or_default(doc);
    let here = Bb::of(pt(p), pt(p)).grow(1.0);
    let rank = |k: ObjKind| match k {
        ObjKind::Pad => 4,
        ObjKind::Via => 3,
        ObjKind::Track | ObjKind::Arc => 2,
        _ => 1,
    };
    build_objects(doc, &board, Some(here), None)
        .into_iter()
        .filter(|o| o.on_layer(layer) && o.geo().contains(pt(p)))
        .max_by_key(|o| (rank(o.kind), o.item))
        .map(|o| (o.item, o.net))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::DocKind;
    use crate::item::{Drill, Pad, PadKind, Rect};
    use crate::layer;
    use crate::library::Footprint;
    use crate::pcb::items::{PlacedFootprint, Side, Track, Via, Zone};
    use crate::units::mm;

    pub fn p(x: f64, y: f64) -> Point {
        Point::new(mm(x), mm(y))
    }

    fn pad(number: &str, x: f64, w: f64, h: f64, kind: PadKind) -> Item {
        Item::Pad(Pad {
            number: number.into(),
            pos: p(x, 0.0),
            rotation: 0.0,
            shape: if kind == PadKind::Smd { PadShape::Rect } else { PadShape::Circle },
            width: mm(w),
            height: mm(h),
            corner_ratio: 0.0,
            kind,
            layer: if kind == PadKind::Smd { layer::TOP_COPPER } else { layer::MULTI_LAYER },
            drill: (kind != PadKind::Smd).then_some(Drill { width: mm(0.8), height: mm(0.8) }),
         copper_shapes: Default::default(),})
    }

    /// "L/R": two 0.8×0.9 mm SMD pads at x = ±0.8 mm, courtyard ±1.5×±0.8 mm.
    /// "L/TH": one 1.6 mm round THT pad (0.8 mm drill), no courtyard.
    pub fn footprints() -> Vec<(&'static str, Footprint)> {
        let mut r = Footprint::new("R");
        r.items = vec![
            pad("1", -0.8, 0.8, 0.9, PadKind::Smd),
            pad("2", 0.8, 0.8, 0.9, PadKind::Smd),
            Item::Rect(Rect { a: p(-1.5, -0.8), b: p(1.5, 0.8), width: mm(0.05), layer: layer::TOP_COURTYARD, fill: Default::default() }),
        ];
        let mut th = Footprint::new("TH");
        th.items = vec![pad("1", 0.0, 1.6, 1.6, PadKind::Tht)];
        vec![("L/R", r), ("L/TH", th)]
    }

    pub fn doc(items: Vec<Item>) -> Document {
        let mut all = vec![Item::Board(BoardSettings::default())];
        all.extend(items);
        let mut d = Document::from_item_list(DocKind::Pcb, all);
        for (k, f) in footprints() {
            d.put_cached_footprint(k, f);
        }
        d
    }

    pub fn fp(des: &str, key: &str, x: f64, y: f64, nets: &[(&str, &str)]) -> Item {
        Item::Footprint(PlacedFootprint {
            board_only: false,
            designator: des.into(),
            value: String::new(),
            footprint: key.into(),
            pos: p(x, y),
            rotation: 0.0,
            side: Side::Top,
            locked: false,
            pad_nets: nets.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            component: String::new(),
            parameters: Default::default(),
        })
    }

    pub fn track(a: (f64, f64), b: (f64, f64), w: f64, l: LayerId, net: &str) -> Item {
        Item::Track(Track { locked: false, a: p(a.0, a.1), b: p(b.0, b.1), width: mm(w), layer: l, net: net.into() })
    }

    pub fn via(x: f64, y: f64, net: &str) -> Item {
        Item::Via(Via { locked: false, pos: p(x, y), diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: net.into() })
    }

    pub fn zone(x0: f64, y0: f64, x1: f64, y1: f64, l: LayerId, net: &str) -> Item {
        let outer = vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
        Item::Zone(Zone { locked: false,
            teardrop: false,
            outline: outer.clone(),
            layer: l,
            net: net.into(),
            name: String::new(),
            priority: 0,
            clearance: 0,
            min_width: 0,
            connect: Default::default(),
            thermal_gap: 0,
            spoke_width: 0,
            keepout: false,
            remove_islands: true,
            fill: vec![ZonePolygon { outer, holes: vec![] }],
        })
    }

    pub fn outline(x0: f64, y0: f64, x1: f64, y1: f64) -> Item {
        Item::Rect(Rect { a: p(x0, y0), b: p(x1, y1), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: Default::default() })
    }

    /// Item id of the n-th item passed to [`doc`] (the board item is id 0).
    pub fn id(n: u64) -> ItemId {
        ItemId(n + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::layer::{BOTTOM_COPPER, TOP_COPPER};

    #[test]
    fn track_touching_pad_connects() {
        let d = doc(vec![
            fp("R1", "L/R", 0.0, 0.0, &[("1", "A"), ("2", "B")]),
            fp("R2", "L/R", 10.0, 0.0, &[("1", "A")]),
            track((-0.8, 0.0), (9.2, 0.0), 0.25, TOP_COPPER, "A"),
        ]);
        // The track passes over R1 pad 2 too (net B): physically connected (a short).
        let c = analyze(&d);
        assert!(c.connected((id(0), Some("1")), (id(1), Some("1"))));
        assert!(c.connected((id(0), Some("2")), (id(2), None)));
        assert_eq!(ratsnest(&d), vec![]);
        let s = stats(&d);
        assert_eq!((s.nets, s.connections, s.unrouted), (2, 1, 0));
    }

    #[test]
    fn separate_layers_do_not_connect_but_vias_join_them() {
        let mut items = vec![
            fp("R1", "L/R", 0.0, 0.0, &[("1", "A")]),
            fp("R2", "L/R", 10.0, 0.0, &[("1", "A")]),
            track((-0.8, 0.0), (5.0, 0.0), 0.25, TOP_COPPER, "A"),
            track((5.0, 0.0), (9.2, 0.0), 0.25, BOTTOM_COPPER, "A"),
        ];
        let d = doc(items.clone());
        assert!(!analyze(&d).connected((id(2), None), (id(3), None)));
        assert_eq!(ratsnest(&d).len(), 1);
        items.push(via(5.0, 0.0, "A"));
        let d = doc(items);
        let c = analyze(&d);
        assert!(c.connected((id(2), None), (id(3), None)));
        // Bottom track does not reach the top-side SMD pad of R2.
        assert!(!c.connected((id(3), None), (id(1), Some("1"))));
        let r = ratsnest(&d);
        assert_eq!(r.len(), 1);
        // Shortest connection: bottom track end (9.2, 0) to R2 pad 1 center.
        assert_eq!((r[0].a, r[0].b), (p(9.2, 0.0), p(9.2, 0.0)));
    }

    #[test]
    fn zone_fill_joins_pads_and_tht_pads_span_layers() {
        let d = doc(vec![
            fp("J1", "L/TH", 0.0, 0.0, &[("1", "GND")]),
            fp("J2", "L/TH", 10.0, 0.0, &[("1", "GND")]),
            fp("J3", "L/TH", 30.0, 0.0, &[("1", "GND")]),
            zone(-2.0, -2.0, 12.0, 2.0, BOTTOM_COPPER, "GND"),
        ]);
        let c = analyze(&d);
        assert!(c.connected((id(0), None), (id(1), None)));
        assert!(!c.connected((id(0), None), (id(2), None)));
        assert_eq!(c.cluster_of(id(3), None), c.cluster_of(id(0), Some("1")));
        let r = ratsnest(&d);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].b, p(30.0, 0.0));
        assert_eq!(r[0].a, p(10.0, 0.0));
    }

    #[test]
    fn unrouted_net_ratsnest_is_a_spanning_tree() {
        let items: Vec<Item> = (0..7).map(|i| fp(&format!("R{i}"), "L/R", i as f64 * 5.0, (i % 3) as f64 * 4.0, &[("1", "N"), ("2", "")])).collect();
        let d = doc(items);
        let r = ratsnest(&d);
        assert_eq!(r.len(), 6);
        assert!(r.iter().all(|w| w.net == "N"));
        assert_eq!(stats(&d), RoutingStats { nets: 1, connections: 6, unrouted: 6 });
    }

    #[test]
    fn object_queries() {
        let d = doc(vec![
            fp("R1", "L/R", 0.0, 0.0, &[("1", "A")]),
            track((-0.8, 0.0), (5.0, 0.0), 0.25, TOP_COPPER, "A"),
            via(5.0, 0.0, "A"),
            zone(-10.0, -10.0, 10.0, 10.0, TOP_COPPER, "GND"),
        ]);
        assert_eq!(object_at(&d, p(-0.8, 0.1), TOP_COPPER), Some((id(0), "A".to_string())));
        assert_eq!(object_at(&d, p(2.0, 0.05), TOP_COPPER), Some((id(1), "A".to_string())));
        assert_eq!(object_at(&d, p(5.0, 0.2), BOTTOM_COPPER), Some((id(2), "A".to_string())));
        assert_eq!(object_at(&d, p(2.0, 5.0), TOP_COPPER), Some((id(3), "GND".to_string())));
        assert_eq!(object_at(&d, p(2.0, 5.0), BOTTOM_COPPER), None);
        assert_eq!(net_items(&d, "A"), vec![id(0), id(1), id(2)]);
        assert_eq!(net_items(&d, ""), vec![]);
    }
}
