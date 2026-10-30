//! Rebuild routed sections between fixed electrical anchors during connected edits.
use super::{
    board::BoardSettings,
    drag::DragResult,
    geometry::{board_settings, world_pads},
    items::Track,
    router::{self, CornerStyle, RouteMode},
};
use crate::{Document, IdItem, Item, ItemId, LayerId, Point};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Chain {
    pub edges: Vec<ItemId>,
    pub end: Point,
}
pub struct Topology<'a> {
    doc: &'a Document,
    adj: HashMap<Point, Vec<ItemId>>,
    anchors: Vec<(ItemId, String, Point, f64, Vec<LayerId>)>,
}
impl<'a> Topology<'a> {
    pub fn new(doc: &'a Document) -> Self {
        let fallback = BoardSettings::default();
        let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
        let mut adj: HashMap<Point, Vec<ItemId>> = HashMap::new();
        let mut anchors = Vec::new();
        for (id, item) in doc.items() {
            match item {
                Item::Track(t) => {
                    adj.entry(t.a).or_default().push(id);
                    adj.entry(t.b).or_default().push(id);
                }
                Item::Via(v) => anchors.push((
                    id,
                    v.net.clone(),
                    v.pos,
                    v.diameter as f64 / 2.,
                    board.stackup.span(v.from_layer, v.to_layer),
                )),
                Item::Footprint(fp) => {
                    if let Some(g) = doc.cached_footprint(&fp.footprint) {
                        for p in world_pads(id, fp, g, board) {
                            anchors.push((
                                id,
                                p.net,
                                p.pos,
                                p.width.max(p.height) as f64 / 2.,
                                p.copper,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        Self { doc, adj, anchors }
    }
    pub fn anchor(&self, p: Point, t: &Track, ignore: &HashSet<ItemId>) -> bool {
        self.anchors.iter().any(|(owner, net, at, reach, layers)| {
            !ignore.contains(owner)
                && net == &t.net
                && layers.contains(&t.layer)
                && distance(p, *at) <= *reach + t.width as f64 / 2.
        }) || self.doc.items().any(|(id, item)| {
            !ignore.contains(&id)
                && match item {
                    Item::ArcTrack(a) if a.net == t.net && a.layer == t.layer => {
                        distance(p, super::drag::nearest_on_arc(a, p))
                            <= (a.width + t.width) as f64 / 2.
                    }
                    _ => false,
                }
        })
    }
    fn incident(&self, p: Point, t: &Track) -> Vec<ItemId> {
        self.adj.get(&p).into_iter().flatten().copied().filter(|id|matches!(self.doc.get(*id),Some(Item::Track(n))if n.net==t.net&&n.layer==t.layer)).collect()
    }
    /// Follow an unbranched section, stopping at a pad, via, curve, lock or width change.
    pub fn walk(&self, start: Point, t: &Track, excluded: &HashSet<ItemId>) -> Chain {
        let mut used = excluded.clone();
        let mut at = start;
        let mut edges = Vec::new();
        loop {
            if self.anchor(at, t, &HashSet::new()) {
                break;
            }
            let next: Vec<_> = self
                .incident(at, t)
                .into_iter()
                .filter(|id| !used.contains(id))
                .collect();
            if next.len() != 1 {
                break;
            }
            let id = next[0];
            let Some(Item::Track(n)) = self.doc.get(id) else {
                break;
            };
            if n.locked || n.width != t.width {
                break;
            }
            used.insert(id);
            edges.push(id);
            at = if n.a == at { n.b } else { n.a };
            if at == start {
                break;
            }
        }
        Chain { edges, end: at }
    }
    pub fn continuation(&self, p: Point, t: &Track, except: ItemId) -> Option<ItemId> {
        if self.anchor(p, t, &HashSet::new()) {
            return None;
        }
        let rest: Vec<_> = self
            .incident(p, t)
            .into_iter()
            .filter(|id| *id != except)
            .collect();
        if rest.len() == 1
            && matches!(self.doc.get(rest[0]),Some(Item::Track(n))if !n.locked&&n.width==t.width)
        {
            Some(rest[0])
        } else {
            None
        }
    }
}

fn distance(a: Point, b: Point) -> f64 {
    ((a.x - b.x) as f64).hypot((a.y - b.y) as f64)
}
fn cross(a: Point, b: Point, c: Point) -> i128 {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128
}
fn between(p: Point, a: Point, b: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}
fn intersects(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (x, y, z, w) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    (x.signum() != y.signum() && z.signum() != w.signum())
        || (x == 0 && between(c, a, b))
        || (y == 0 && between(d, a, b))
        || (z == 0 && between(a, c, d))
        || (w == 0 && between(b, c, d))
}
pub fn simple_path(path: &[Point]) -> bool {
    for p in path.windows(3) {
        let dx = p[1].x - p[0].x;
        let dy = p[1].y - p[0].y;
        let ex = p[2].x - p[1].x;
        let ey = p[2].y - p[1].y;
        if dx as i128 * ex as i128 + dy as i128 * (ey as i128) < 0 {
            return false;
        }
    }
    for i in 0..path.len().saturating_sub(1) {
        if path[i] == path[i + 1] {
            return false;
        }
        for j in i + 2..path.len().saturating_sub(1) {
            if intersects(path[i], path[i + 1], path[j], path[j + 1]) {
                return false;
            }
        }
    }
    true
}
pub fn simplify(path: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for &p in path {
        if out.last() == Some(&p) {
            continue;
        }
        while out.len() > 1 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let dot = (b.x - a.x) as i128 * (p.x - b.x) as i128
                + (b.y - a.y) as i128 * (p.y - b.y) as i128;
            if cross(a, b, p) == 0 && dot >= 0 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}
fn bevel(path: &[Point], radius: i64) -> Vec<Point> {
    let mut out = vec![path[0]];
    for i in 1..path.len() - 1 {
        let (a, b, c) = (path[i - 1], path[i], path[i + 1]);
        let incoming = (b.x - a.x, b.y - a.y);
        let outgoing = (c.x - b.x, c.y - b.y);
        if incoming.0 as i128 * outgoing.0 as i128 + incoming.1 as i128 * outgoing.1 as i128 == 0 {
            let r = radius
                .min(incoming.0.abs().max(incoming.1.abs()) / 3)
                .min(outgoing.0.abs().max(outgoing.1.abs()) / 3);
            if r > 0 {
                out.push(Point::new(
                    b.x - incoming.0.signum() * r,
                    b.y - incoming.1.signum() * r,
                ));
                out.push(Point::new(
                    b.x + outgoing.0.signum() * r,
                    b.y + outgoing.1.signum() * r,
                ));
                continue;
            }
        }
        out.push(b);
    }
    out.push(*path.last().unwrap());
    simplify(&out)
}
fn octilinear(path: &[Point]) -> bool {
    path.windows(2).all(|p| {
        let dx = (p[1].x - p[0].x).abs();
        let dy = (p[1].y - p[0].y).abs();
        dx == 0 || dy == 0 || dx == dy
    })
}
fn length(path: &[Point]) -> f64 {
    path.windows(2).map(|p| distance(p[0], p[1])).sum()
}
fn on_segment(p: Point, a: Point, b: Point) -> bool {
    cross(a, b, p).abs() <= 4 * (distance(a, b) as i128 + 1) && between(p, a, b)
}
const DIRS: [(i64, i64); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
fn joins(a: Point, b: Point) -> Vec<Vec<Point>> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let n = dx.abs().min(dy.abs());
    vec![
        simplify(&[
            a,
            Point::new(a.x + dx.signum() * n, a.y + dy.signum() * n),
            b,
        ]),
        simplify(&[
            a,
            Point::new(b.x - dx.signum() * n, b.y - dy.signum() * n),
            b,
        ]),
    ]
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRequest {
    pub ids: Vec<ItemId>,
    pub delta: Point,
    pub point: Point,
    pub mode: RouteMode,
    pub corner: CornerStyle,
    #[serde(default)]
    pub tolerance: i64,
}

fn window_chain(doc: &Document, start: Point, full: Chain, limit: usize) -> (Chain, Vec<Point>) {
    let mut points = vec![start];
    let mut at = start;
    for id in &full.edges {
        if let Some(Item::Track(t)) = doc.get(*id) {
            at = if t.a == at { t.b } else { t.a };
            points.push(at);
        }
    }
    let count = limit.min(full.edges.len());
    (
        Chain {
            edges: full.edges[..count].to_vec(),
            end: points[count],
        },
        points[count..].to_vec(),
    )
}

fn candidate_plan(
    doc: &Document,
    old: &[ItemId],
    picked: ItemId,
    prototype: &Track,
    path: &[Point],
    target: Point,
    mode: RouteMode,
) -> Result<DragResult, String> {
    if path.len() < 2 || !simple_path(path) || !octilinear(path) {
        return Err("That move cannot form a simple connected route".into());
    }
    let mut tracks: Vec<_> = path
        .windows(2)
        .map(|p| Track {
            a: p[0],
            b: p[1],
            ..prototype.clone()
        })
        .collect();
    let keep = tracks
        .iter()
        .position(|t| on_segment(target, t.a, t.b))
        .unwrap_or(0);
    let chosen = tracks.remove(keep);
    let replace = vec![IdItem {
        id: picked,
        item: Item::Track(chosen),
    }];
    let delete = old.iter().copied().filter(|id| *id != picked).collect();
    router::plan_track_edit_removed(
        doc,
        replace,
        tracks.into_iter().map(Item::Track).collect(),
        delete,
        mode,
    )
}

/// Rebuild a single dragged segment or a two-segment corner between actual anchors.
pub fn drag_tracks(doc: &Document, req: &TrackRequest) -> Result<DragResult, String> {
    let mut first = None;
    let mut last = "No clean connected route reaches this position".to_string();
    for (left, right) in [
        (1, 1),
        (2, 1),
        (1, 2),
        (2, 2),
        (3, 1),
        (1, 3),
        (3, 2),
        (2, 3),
        (3, 3),
        (5, 5),
        (8, 8),
        (usize::MAX, usize::MAX),
    ] {
        match drag_window(doc, req, left, right) {
            Ok(result) => return Ok(result),
            Err(error) => {if first.is_none(){first=Some(error.clone());}last=error;},
        }
    }
    Err(first.unwrap_or(last))
}
fn drag_window(
    doc: &Document,
    req: &TrackRequest,
    left_limit: usize,
    right_limit: usize,
) -> Result<DragResult, String> {
    if req.ids.is_empty() || req.ids.len() > 2 {
        return Err("Drag one track or a connected two-track corner".into());
    }
    let picked = req.ids[0];
    let Some(Item::Track(t)) = doc.get(picked) else {
        return Err("Select a straight track to drag".into());
    };
    if t.locked {
        return Err("Unlock this track before dragging it".into());
    }
    let topo = Topology::new(doc);
    let mut selected: HashSet<_> = req.ids.iter().copied().collect();
    let close = [t.a, t.b]
        .into_iter()
        .min_by(|a, b| distance(*a, req.point).total_cmp(&distance(*b, req.point)))
        .unwrap();
    let vertex = if req.ids.len() == 2 {
        let Some(Item::Track(other)) = doc.get(req.ids[1]) else {
            return Err("Select a connected track corner".into());
        };
        if other.locked || other.net != t.net || other.layer != t.layer || other.width != t.width {
            return Err("The selected corner must share net, layer and width".into());
        }
        if [other.a, other.b].contains(&t.a) {
            Some(t.a)
        } else if [other.a, other.b].contains(&t.b) {
            Some(t.b)
        } else {
            return Err("Select two tracks sharing a corner".into());
        }
    } else if distance(close, req.point) <= req.tolerance.max(t.width / 2) as f64 {
        topo.continuation(close, t, picked).map(|id| {
            selected.insert(id);
            close
        })
    } else {
        None
    };
    let (a, b, target, raw, left_tail, right_tail) = if let Some(vertex) = vertex {
        let other_id = *selected
            .iter()
            .find(|id| **id != picked)
            .ok_or("A corner requires two tracks")?;
        let Some(Item::Track(other)) = doc.get(other_id) else {
            unreachable!()
        };
        let left = if t.a == vertex { t.b } else { t.a };
        let right = if other.a == vertex { other.b } else { other.a };
        let l = topo.walk(left, t, &selected);
        let mut used = selected.clone();
        used.extend(l.edges.iter().copied());
        let r = topo.walk(right, t, &used);
        let (l, left_tail) = window_chain(doc, left, l, left_limit);
        let (r, right_tail) = window_chain(doc, right, r, right_limit);
        let target = Point::new(vertex.x + req.delta.x, vertex.y + req.delta.y);
        let mut raw = Vec::new();
        for x in joins(l.end, target) {
            for y in joins(target, r.end) {
                let mut p = x.clone();
                p.extend(y.into_iter().skip(1));
                raw.push(p);
            }
        }
        (l, r, target, raw, left_tail, right_tail)
    } else {
        let a = topo.walk(t.a, t, &selected);
        let mut used = selected.clone();
        used.extend(a.edges.iter().copied());
        let b = topo.walk(t.b, t, &used);
        let (a, left_tail) = window_chain(doc, t.a, a, left_limit);
        let (b, right_tail) = window_chain(doc, t.b, b, right_limit);
        let dx = (t.b.x - t.a.x) as f64;
        let dy = (t.b.y - t.a.y) as f64;
        let len = dx.hypot(dy);
        if len == 0. {
            return Err("Remove this zero-length track before dragging".into());
        }
        let n = (-dy / len, dx / len);
        let shift = req.delta.x as f64 * n.0 + req.delta.y as f64 * n.1;
        let along = (((req.point.x - t.a.x) as f64 * dx + (req.point.y - t.a.y) as f64 * dy)
            / (len * len))
            .clamp(0., 1.);
        let target = Point::new(
            (t.a.x as f64 + along * dx + n.0 * shift).round() as i64,
            (t.a.y as f64 + along * dy + n.1 * shift).round() as i64,
        );
        let hits = |p: Point, plane: f64| {
            let mut out = Vec::new();
            for (x, y) in DIRS {
                let den = n.0 * x as f64 + n.1 * y as f64;
                if den.abs() < 1e-9 {
                    continue;
                }
                let s = (plane - n.0 * p.x as f64 - n.1 * p.y as f64) / den;
                if s >= -1. {
                    out.push(Point::new(
                        (p.x as f64 + x as f64 * s).round() as i64,
                        (p.y as f64 + y as f64 * s).round() as i64,
                    ));
                }
            }
            if (plane - n.0 * p.x as f64 - n.1 * p.y as f64).abs() < 2. {
                out.push(p);
            }
            out
        };
        let mut raw = Vec::new();
        // A diagonal rail can intersect the integer coordinate grid at half a unit.
        // Try adjacent integer rails, within one nanometre of the pointer projection.
        for (ox, oy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let plane = n.0 * (target.x + ox) as f64 + n.1 * (target.y + oy) as f64;
            for p in hits(a.end, plane) {
                for q in hits(b.end, plane) {
                    if p == q || !on_segment(target, p, q) {
                        continue;
                    }
                    if (q.x - p.x) as i128 * (t.b.x - t.a.x) as i128
                        + (q.y - p.y) as i128 * (t.b.y - t.a.y) as i128
                        <= 0
                    {
                        continue;
                    }
                    raw.push(vec![a.end, p, q, b.end]);
                }
            }
        }
        (a, b, target, raw, left_tail, right_tail)
    };
    let mut old: Vec<_> = selected.into_iter().chain(a.edges).chain(b.edges).collect();
    old.sort();
    old.dedup();
    if old
        .iter()
        .any(|id| matches!(doc.get(*id),Some(Item::Track(t))if t.locked))
    {
        return Err("A fixed track prevents this drag".into());
    }
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw {
        let clean = simplify(&raw);
        if clean.len() < 2 {
            continue;
        }
        let smooth = if req.corner == CornerStyle::Orthogonal90 {
            clean.clone()
        } else {
            bevel(&clean, (t.width * 2).max(100_000))
        };
        for path in [smooth, clean] {
            let mut whole: Vec<_> = left_tail.iter().rev().copied().collect();
            whole.extend(path.iter().skip(1).copied());
            whole.extend(right_tail.iter().skip(1).copied());
            if !simple_path(&simplify(&whole)) {
                continue;
            }
            if !simple_path(&path)
                || !octilinear(&path)
                || !path.windows(2).any(|p| on_segment(target, p[0], p[1]))
                || !seen.insert(path.clone())
            {
                continue;
            }
            let error = path
                .windows(2)
                .map(|p| {
                    super::connectivity::geo::point_seg(
                        super::connectivity::pt(target),
                        super::connectivity::pt(p[0]),
                        super::connectivity::pt(p[1]),
                    )
                    .0
                })
                .fold(f64::INFINITY, f64::min);
            let score = length(&path) + path.len() as f64 * 100_000. + error * 10_000.;
            candidates.push((score, path));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut last = "No clean connected route reaches this position".to_string();
    for (_, path) in candidates.into_iter().take(24) {
        match candidate_plan(doc, &old, picked, t, &path, target, req.mode) {
            Ok(plan) => return Ok(plan),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(test)]
#[path = "elastic_tests.rs"]
mod tests;

/// Electrical paths used by a move must not rely on an existing foreign-net short.
pub(crate) fn net_connectivity(doc: &Document) -> super::connectivity::Connectivity {
    use super::connectivity::{self, Index, Uf};
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let index = Index::new(
        connectivity::build_objects(doc, board, None, None)
            .into_iter()
            .filter(|o| o.kind != super::query::ObjKind::Zone)
            .collect(),
    );
    let mut uf = Uf::new(index.objs.len());
    connectivity::join_same_pads(&index.objs, &mut uf);
    for i in 0..index.objs.len() {
        for j in index
            .tree
            .query(index.geos[i].bb.grow(connectivity::geo::TOUCH))
        {
            if j > i && index.objs[i].net == index.objs[j].net && index.touches(i, j) {
                uf.union(i, j);
            }
        }
    }
    connectivity::Connectivity::from_labels(index.objs, uf.labels())
}
