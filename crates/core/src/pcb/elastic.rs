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
struct TopologySpan {id:ItemId,bounds:rstar::AABB<[i64;2]>}
impl rstar::RTreeObject for TopologySpan {
    type Envelope=rstar::AABB<[i64;2]>;
    fn envelope(&self)->Self::Envelope{self.bounds}
}
pub struct Topology {
    tracks: HashMap<ItemId, Track>,
    joints: HashMap<ItemId,Vec<Point>>,
    arcs: Vec<(ItemId, super::items::ArcTrack)>,
    adj: HashMap<Point, Vec<ItemId>>,
    anchors: Vec<(ItemId, String, Point, f64, Vec<LayerId>)>,
    pads: Vec<(ItemId,String,LayerId,Point,super::connectivity::geo::Geo)>,
}
impl Topology {
    pub fn new(doc: &Document) -> Self {
        let fallback = BoardSettings::default();
        let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
        let mut adj: HashMap<Point, Vec<ItemId>> = HashMap::new();
        let mut tracks=HashMap::new();
        let mut arcs=Vec::new();
        let mut anchors = Vec::new();
        let mut pads=Vec::new();
        for (id, item) in doc.items() {
            match item {
                Item::Track(t) => {
                    tracks.insert(id,t.clone());
                    adj.entry(t.a).or_default().push(id);
                    adj.entry(t.b).or_default().push(id);
                }
                Item::ArcTrack(a) => arcs.push((id,a.clone())),
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
                            for layer in &p.copper {
                                pads.push((id,p.net.clone(),*layer,p.pos,super::connectivity::shape_geo(&super::connectivity::pad_shape(&p.for_layer(*layer)))));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let tree=rstar::RTree::bulk_load(tracks.iter().map(|(id,t)|TopologySpan{id:*id,bounds:rstar::AABB::from_corners([t.a.x.min(t.b.x),t.a.y.min(t.b.y)],[t.a.x.max(t.b.x),t.a.y.max(t.b.y)])}).collect());
        let mut contacts=Vec::new();
        for (at,ids) in &adj {for id in ids {let t=&tracks[id];contacts.push((*at,t.net.clone(),t.layer));}}
        for (_,net,at,_,layers) in &anchors {for layer in layers {contacts.push((*at,net.clone(),*layer));}}
        for (_,net,layer,at,_) in &pads {contacts.push((*at,net.clone(),*layer));}
        let mut joints:HashMap<ItemId,Vec<Point>>=HashMap::new();
        for (at,net,layer) in contacts {
            for span in tree.locate_in_envelope_intersecting(rstar::AABB::from_point([at.x,at.y])) {
                let t=&tracks[&span.id];
                if t.net!=net||t.layer!=layer||at==t.a||at==t.b||cross(t.a,t.b,at)!=0||!between(at,t.a,t.b){continue;}
                let list=joints.entry(span.id).or_default();if !list.contains(&at){list.push(at);}
                let incident=adj.entry(at).or_default();if !incident.contains(&span.id){incident.push(span.id);}
            }
        }
        Self { tracks, joints, arcs, adj, anchors, pads }
    }
    pub fn interior_joints(&self,id:ItemId)->&[Point]{self.joints.get(&id).map_or(&[],Vec::as_slice)}
    pub fn anchor(&self, p: Point, t: &Track, ignore: &HashSet<ItemId>) -> bool {
        self.anchors.iter().any(|(owner, net, at, reach, layers)| {
            !ignore.contains(owner)
                && net == &t.net
                && layers.contains(&t.layer)
                && distance(p, *at) <= *reach + t.width as f64 / 2.
        }) || self.pads.iter().any(|(owner,net,layer,_,geometry)|{
            if ignore.contains(owner)||net!=&t.net||*layer!=t.layer{return false;}
            let at=super::connectivity::pt(p);
            super::connectivity::geo::gap(geometry,&super::connectivity::geo::Geo::cap(at,at,t.width as f64/2.0),super::connectivity::geo::TOUCH).is_some()
        }) || self.arcs.iter().any(|(id,a)| {
            !ignore.contains(id) && a.net==t.net && a.layer==t.layer
                && distance(p,super::drag::nearest_on_arc(a,p)) <= (a.width+t.width) as f64/2.
        })
    }
    pub fn anchor_center(&self,p:Point,t:&Track)->Option<Point>{
        let vias=self.anchors.iter().filter(|(_,net,at,reach,layers)|net==&t.net&&layers.contains(&t.layer)&&distance(p,*at)<=*reach+t.width as f64/2.).map(|(_,_,at,_,_)|*at);
        let pads=self.pads.iter().filter(|(_,net,layer,_,geo)|net==&t.net&&*layer==t.layer&&super::connectivity::geo::gap(geo,&super::connectivity::geo::Geo::cap(super::connectivity::pt(p),super::connectivity::pt(p),t.width as f64/2.),super::connectivity::geo::TOUCH).is_some()).map(|(_,_,_,center,_)|*center);
        vias.chain(pads).min_by(|a,b|distance(p,*a).total_cmp(&distance(p,*b)))
    }
    fn incident(&self, p: Point, t: &Track) -> Vec<ItemId> {
        self.adj.get(&p).into_iter().flatten().copied().filter(|id|self.tracks.get(id).is_some_and(|n|n.net==t.net&&n.layer==t.layer)).collect()
    }
    /// Follow an unbranched section, stopping at a pad, via, curve, lock or width change.
    pub fn walk(&self, start: Point, t: &Track, excluded: &HashSet<ItemId>) -> Chain {
        let mut used = excluded.clone();
        let mut at = start;
        let mut edges = Vec::new();
        loop {
            if self.anchor(at, t, excluded) {
                // A short neck inside a land is still part of this conductor.
                // Stopping at its outer vertex hides the real departure tangent
                // and can fold a dragged section back across the fixed neck.
                let center=self.anchor_center(at,t);
                let inward:Vec<_>=self.incident(at,t).into_iter().filter(|id|!used.contains(id)).collect();
                let can_reach_center=center.is_some_and(|center|center!=at&&inward.len()==1&&self.tracks.get(&inward[0]).is_some_and(|next|{
                    let far=if next.a==at{next.b}else{next.a};
                    !next.locked&&next.width==t.width&&distance(far,center)<distance(at,center)&&self.anchor_center(far,t)==Some(center)
                }));
                if !can_reach_center{break;}
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
            let Some(n) = self.tracks.get(&id) else {
                break;
            };
            if n.locked || n.width != t.width || (n.a!=at&&n.b!=at) {
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
            && self.tracks.get(&rest[0]).is_some_and(|n|!n.locked&&n.width==t.width&&(n.a==p||n.b==p))
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

#[derive(Clone, Debug, serde::Serialize, Deserialize)]
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
    rail: Option<((f64,f64),f64)>,
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
    // Keep the picked identity on the moved rail, even when the cursor lies
    // beyond a newly shortened segment. A corner can instead keep either arm.
    let rank = |t: &Track| {
        let parallel = cross(Point::default(), Point::new(prototype.b.x-prototype.a.x, prototype.b.y-prototype.a.y), Point::new(t.b.x-t.a.x, t.b.y-t.a.y)) == 0;
        let gap = super::connectivity::geo::point_seg(super::connectivity::pt(target), super::connectivity::pt(t.a), super::connectivity::pt(t.b)).0;
        let follows = parallel && rail.is_some_and(|(n,plane)| [t.a,t.b].iter().all(|p|(n.0*p.x as f64+n.1*p.y as f64-plane).abs()<=2.));
        (if follows {0} else if on_segment(target,t.a,t.b) {1} else {2}, gap)
    };
    let keep = tracks.iter().enumerate().min_by(|(_,a),(_,b)| {
        let a=rank(a); let b=rank(b); a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
    }).map_or(0, |(i,_)|i);
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

/// Split a virtual junction only inside the speculative board. The source
/// identities and all unrelated branches remain intact until one atomic commit.
fn drag_split_track(doc:&Document,req:&TrackRequest,joints:&[Point])->Result<DragResult,String>{
    let id=req.ids[0];let Some(Item::Track(t))=doc.get(id)else{return Err("Select a track arm to drag".into())};
    if t.locked{return Err("Unlock this track before dragging it".into());}
    if joints.len()>64{return Err("Choose a shorter section beside the fixed junctions".into());}
    let mut points=vec![t.a,t.b];points.extend_from_slice(joints);
    let projection=|p:Point|(p.x-t.a.x)as i128*(t.b.x-t.a.x)as i128+(p.y-t.a.y)as i128*(t.b.y-t.a.y)as i128;
    points.sort_by_key(|p|projection(*p));points.dedup();
    let picked=points.windows(2).enumerate().min_by(|(_,a),(_,b)|{
        let gap=|w:&[Point]|super::connectivity::geo::point_seg(super::connectivity::pt(req.point),super::connectivity::pt(w[0]),super::connectivity::pt(w[1])).0;
        gap(a).total_cmp(&gap(b))
    }).map_or(0,|(i,_)|i);
    let mut work=doc.clone();let mut edits=Vec::new();
    for (i,w) in points.windows(2).enumerate(){
        let key=if i==picked{id}else{work.alloc_id()};
        edits.push(crate::document::Edit{id:key,before:doc.get(key).cloned(),after:Some(Item::Track(Track{a:w[0],b:w[1],..t.clone()}))});
    }
    work.apply(&edits);
    let plan=drag_tracks_indexed(&work,req,&Topology::new(&work))?;let rerouted=plan.rerouted;
    let mut edits:Vec<_>=plan.replace.into_iter().map(|entry|crate::document::Edit{id:entry.id,before:work.get(entry.id).cloned(),after:Some(entry.item)}).collect();
    edits.extend(plan.delete.into_iter().map(|id|crate::document::Edit{id,before:work.get(id).cloned(),after:None}));work.apply(&edits);
    let added:Vec<_>=plan.add.into_iter().map(|item|crate::document::Edit{id:work.alloc_id(),before:None,after:Some(item)}).collect();work.apply(&added);
    let mut replace=Vec::new();let mut delete=Vec::new();let mut add=Vec::new();
    for (id,item) in doc.items(){match work.get(id){Some(after)if after!=item=>replace.push(IdItem{id,item:after.clone()}),None=>delete.push(id),_=>{}}}
    for (id,item) in work.items(){if doc.get(id).is_none(){add.push(item.clone());}}
    let mut result=router::plan_track_edit_removed(doc,replace,add,delete,req.mode)?;result.rerouted+=rerouted;Ok(result)
}

#[path = "elastic_group.rs"]
mod group;

/// Rebuild connected conductors between actual anchors.
pub fn drag_tracks(doc: &Document, req: &TrackRequest) -> Result<DragResult, String> {
    drag_tracks_indexed(doc,req,&Topology::new(doc))
}

pub(crate) fn drag_tracks_indexed(doc: &Document, req: &TrackRequest, topo: &Topology) -> Result<DragResult, String> {
    let _work=router::WorkScope::enter();
    if let Some(result)=group::dispatch(doc,req,topo){return result;}
    if req.ids.len()==1&&!topo.interior_joints(req.ids[0]).is_empty()&&req.delta!=Point::default(){
        return drag_split_track(doc,req,topo.interior_joints(req.ids[0]));
    }
    if req.ids.len()==2&&req.ids.iter().any(|id|!topo.interior_joints(*id).is_empty()){
        return Err("Drag one arm beside the fixed junction".into());
    }
    if req.delta==Point::default() {
        if req.ids.is_empty()||req.ids.len()>2{return Err("Drag one track or a connected two-track corner".into());}
        let mut tracks=Vec::new();
        for id in &req.ids {match doc.get(*id){Some(Item::Track(t))if !t.locked=>tracks.push(t),_=>return Err("Select unlocked straight tracks to drag".into())}}
        if tracks.len()==2 && (req.ids[0]==req.ids[1]||tracks[0].net!=tracks[1].net||tracks[0].layer!=tracks[1].layer||tracks[0].width!=tracks[1].width||![tracks[0].a,tracks[0].b].iter().any(|p|[tracks[1].a,tracks[1].b].contains(p))) {return Err("Select two tracks sharing a corner, net, layer and width".into());}
        return Ok(DragResult::default());
    }
    let mut first = None;
    let mut last = "No clean connected route reaches this position".to_string();
    // Exhaust direction-preserving windows before allowing new leads at fixed
    // anchors. Otherwise a small local window can invent a hump instead of
    // consuming the old bend just outside it.
    for preserve in [true, false] {
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
        match drag_window(doc, topo, req, left, right, preserve) {
            Ok(result) => return Ok(result),
            Err(error) => {if first.is_none(){first=Some(error.clone());}last=error;},
        }
    }
    }
    Err(first.unwrap_or(last))
}
fn drag_window(
    doc: &Document,
    topo: &Topology,
    req: &TrackRequest,
    left_limit: usize,
    right_limit: usize,
    preserve: bool,
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
    let mut rail = None;
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
        let along = ((req.point.x + req.delta.x - t.a.x) as f64 * dx
            + (req.point.y + req.delta.y - t.a.y) as f64 * dy) / (len * len);
        let target = Point::new(
            (t.a.x as f64 + along * dx + n.0 * shift).round() as i64,
            (t.a.y as f64 + along * dy + n.1 * shift).round() as i64,
        );
        rail = Some((n, n.0 * target.x as f64 + n.1 * target.y as f64));
        let guide = |chain: &Chain| chain.edges.last().and_then(|id| match doc.get(*id) {
            Some(Item::Track(edge)) => {
                let inner = if edge.a == chain.end {edge.b} else {edge.a};
                Some(((inner.x-chain.end.x).signum(), (inner.y-chain.end.y).signum()))
            }, _ => None,
        });
        let hits = |p: Point, plane: f64, chain: &Chain, tail: &[Point], forward: (i64,i64)| {
            let direction = guide(chain);
            let mut out = Vec::new();
            for (x, y) in DIRS {
                // Unchanged outer copper fixes the boundary tangent. At a true
                // anchor, prefer leads within 45 degrees of the picked heading.
                if let Some(d) = direction {
                    if (preserve || tail.len()>1) && d != (x,y) {continue;}
                } else if preserve {
                    let dot=x*forward.0+y*forward.1;
                    if dot<=0 || (dot*dot) as f64 + 1e-9 < 0.5*((x*x+y*y)*(forward.0*forward.0+forward.1*forward.1)) as f64 {continue;}
                }
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
            for p in hits(a.end, plane, &a, &left_tail, ((t.b.x-t.a.x).signum(),(t.b.y-t.a.y).signum())) {
                for q in hits(b.end, plane, &b, &right_tail, ((t.a.x-t.b.x).signum(),(t.a.y-t.b.y).signum())) {
                    if (q.x - p.x) as i128 * (t.b.x - t.a.x) as i128
                        + (q.y - p.y) as i128 * ((t.b.y - t.a.y) as i128)
                        < 0
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
                || !match rail {
                    Some((n,plane)) => path.iter().any(|p| (n.0*p.x as f64+n.1*p.y as f64-plane).abs()<=2.),
                    None => path.windows(2).any(|p| on_segment(target, p[0], p[1])),
                }
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
            let score = length(&path) + path.len() as f64 * 100_000. + error * if rail.is_some() {0.01} else {10_000.};
            let rail_error = rail.map_or(0., |(n,plane)| path.iter().map(|p|(n.0*p.x as f64+n.1*p.y as f64-plane).abs()).fold(f64::INFINITY,f64::min));
            candidates.push((if rail_error<1e-6 {0.} else {rail_error}, score, path));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut last = "No clean connected route reaches this position".to_string();
    for (_, _, path) in candidates.into_iter().take(24) {
        match candidate_plan(doc, &old, picked, t, &path, target, rail, req.mode) {
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
