//! Moving placed footprints and vias while their routing stays connected.
//!
//! Tracks ending on a moved pad follow it. In `PreserveAngles` mode the segment
//! touching the pad keeps its angle and slides with the pad, while its neighbour
//! shortens or lengthens along its own direction. Where that is impossible (a via
//! or junction at the far end, a parallel neighbour) or the slid copper crowds
//! another net, the segment is re-routed from its far end with the interactive
//! router, pushing or walking around other nets' tracks according to the routing
//! mode. Other components never move: pads are fixed obstacles for the router.

use serde::{Deserialize, Serialize};

use crate::document::{Document, Edit, IdItem, ItemId, LayerId};
use crate::item::Item;
use crate::units::Point;

use super::board::BoardSettings;
use super::geometry::{board_settings, world_pads};
use super::items::Track;
use super::router::{route, CornerStyle, RouteMode, RouteRequest};

/// How tracks attached to a moved footprint behave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveMode {
    /// Tracks stay where they are; the connections become unrouted.
    Disconnect,
    /// The attached end stretches straight to the new pad position.
    Free,
    /// The attached segment slides keeping its angle; re-routed (pushing or avoiding other copper) when it can't.
    #[default]
    PreserveAngles,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DragRequest {
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub center: Point,
    pub ids: Vec<ItemId>,
    pub delta: Point,
    #[serde(default)]
    pub mode: MoveMode,
    pub route_mode: RouteMode,
    pub corner: CornerStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DragFailure { Locked, Unroutable }

/// The drag as a `Route` command: same-ID replacements, removals and new copper.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragResult {
    pub replace: Vec<IdItem>,
    pub delete: Vec<ItemId>,
    pub add: Vec<Item>,
    /// Connections re-routed with the router.
    pub rerouted: usize,
    /// Blocked connections, or direct fallbacks when Ignore mode was explicitly requested.
    pub blocked: usize,
    /// A rejected plan contains no edits and must not be committed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<DragFailure>,
}

fn rejected(failure: DragFailure) -> DragResult {
    DragResult { blocked: 1, failure: Some(failure), ..DragResult::default() }
}

struct Reroute {
    net: String,
    layer: LayerId,
    width: i64,
    from: Point,
    to: Point,
}

fn shifted(p: Point, d: Point) -> Point {
    Point::new(p.x + d.x, p.y + d.y)
}

/// Intersection of the line through `p` along `d` with the line through `q` along `e`.
fn intersect(p: (f64, f64), d: (f64, f64), q: (f64, f64), e: (f64, f64)) -> Option<(f64, f64)> {
    let den = d.0 * e.1 - d.1 * e.0;
    if den.abs() < 1e-9 * d.0.hypot(d.1) * e.0.hypot(e.1) {
        return None;
    }
    let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / den;
    Some((p.0 + d.0 * t, p.1 + d.1 * t))
}

/// Closest point on the arc centreline, including its round-capped endpoints.
fn nearest_on_arc(arc: &super::items::ArcTrack, p: Point) -> Point {
    let at = |angle: f64| { let r=angle.to_radians(); Point::new(arc.center.x+(arc.radius as f64*r.cos()).round() as i64,arc.center.y+(arc.radius as f64*r.sin()).round() as i64) };
    let distance = |q: Point| ((q.x-p.x) as f64).hypot((q.y-p.y) as f64);
    let a=at(arc.start_angle);let b=at(arc.start_angle+arc.sweep);
    let mut nearest=if distance(a)<=distance(b){a}else{b};
    let angle=((p.y-arc.center.y) as f64).atan2((p.x-arc.center.x) as f64).to_degrees();
    let offset=if arc.sweep>=0. {(angle-arc.start_angle).rem_euclid(360.)} else {(arc.start_angle-angle).rem_euclid(360.)};
    if arc.sweep.abs()>=360. || offset<=arc.sweep.abs()+1e-9 {
        let radial=at(angle);if distance(radial)<distance(nearest){nearest=radial;}
    }
    nearest
}

/// The pad segment `t` moved by `delta` keeping its angle, with its far end slid
/// along the single neighbour it joins (which keeps its angle as well).
/// None when that is impossible: a via or junction at the far end, a parallel
/// neighbour, or a neighbour/segment that would fold back over itself.
fn slide(doc: &Document, moving: &std::collections::HashSet<ItemId>, id: ItemId, t: &Track, near_is_a: bool, delta: Point) -> Option<(Track, Option<(ItemId, Track)>)> {
    let (near, far) = if near_is_a { (t.a, t.b) } else { (t.b, t.a) };
    let near2 = shifted(near, delta);
    let mut neighbours = Vec::new();
    for (nid, item) in doc.items() {
        match item {
            Item::Via(v) if !moving.contains(&nid) && v.net==t.net => {
                let fallback=BoardSettings::default();let board=board_settings(doc).map_or(&fallback,|(_,b)|b);
                if board.stackup.span(v.from_layer,v.to_layer).contains(&t.layer) && ((v.pos.x-far.x) as f64).hypot((v.pos.y-far.y) as f64)<=(v.diameter+t.width) as f64/2. {return None;}
            }
            Item::ArcTrack(a) if !moving.contains(&nid) && a.net==t.net && a.layer==t.layer => {
                let q=nearest_on_arc(a,far);
                if ((q.x-far.x) as f64).hypot((q.y-far.y) as f64)<=(a.width+t.width) as f64/2. {return None;}
            }
            Item::Track(n) if nid != id && !moving.contains(&nid) && n.layer == t.layer && n.net == t.net => {
                if n.a==far || n.b==far {neighbours.push((nid,n));}
                else {
                    let (gap,_)=super::connectivity::geo::point_seg(super::connectivity::pt(far),super::connectivity::pt(n.a),super::connectivity::pt(n.b));
                    if gap<=(n.width+t.width) as f64/2. {return None;}
                }
            }
            _ => {}
        }
    }
    let rebuild = |a_end: Point, other: Point| if near_is_a { Track { a: a_end, b: other, ..t.clone() } } else { Track { a: other, b: a_end, ..t.clone() } };
    match neighbours.as_slice() {
        // Ending on a pad that stays: re-route from that pad instead.
        [] if on_fixed_pad(doc, moving, far, t) => None,
        // A dangling end: the whole segment translates.
        [] => Some((rebuild(near2, shifted(far, delta)), None)),
        [(nid, n)] => {
            if n.locked { return None; }
            let other = if n.a == far { n.b } else { n.a };
            let f = |p: Point| (p.x as f64, p.y as f64);
            let dir = (f(far).0 - f(near).0, f(far).1 - f(near).1);
            let ndir = (f(far).0 - f(other).0, f(far).1 - f(other).1);
            let x = intersect(f(near2), dir, f(other), ndir)?;
            // Both segments keep a positive length in their original direction.
            if (x.0 - f(other).0) * ndir.0 + (x.1 - f(other).1) * ndir.1 <= 0.0 || (x.0 - f(near2).0) * dir.0 + (x.1 - f(near2).1) * dir.1 <= 0.0 {
                return None;
            }
            let xp = Point::new(x.0.round() as i64, x.1.round() as i64);
            let n2 = if n.a == far { Track { a: xp, ..(*n).clone() } } else { Track { b: xp, ..(*n).clone() } };
            Some((rebuild(near2, xp), Some((*nid, n2))))
        }
        _ => None,
    }
}

/// `p` lies on a pad of `t`'s net and layer that belongs to a footprint not being moved.
fn on_fixed_pad(doc: &Document, moving: &std::collections::HashSet<ItemId>, p: Point, t: &Track) -> bool {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    doc.items().any(|(id, item)| {
        let Item::Footprint(fp) = item else { return false };
        if moving.contains(&id) {
            return false;
        }
        doc.cached_footprint(&fp.footprint).is_some_and(|g| {
            world_pads(id, fp, g, board).iter().any(|pad| {
                pad.net == t.net && pad.copper.contains(&t.layer) && ((pad.pos.x - p.x) as f64).hypot((pad.pos.y - p.y) as f64) <= pad.width.max(pad.height) as f64 / 2.0
            })
        })
    })
}

struct DragAnchor { owner: ItemId, number: String, net: String, pos: Point, copper: Vec<LayerId>, reach: f64, via: bool }

fn drag_anchors(doc: &Document, id: ItemId, item: &Item, board: &BoardSettings) -> Vec<DragAnchor> {
    match item {
        Item::Footprint(fp) => doc.cached_footprint(&fp.footprint).map(|g| world_pads(id, fp, g, board)
            .into_iter().map(|p| DragAnchor { owner: id, number: p.number, net: p.net, pos: p.pos,
                copper: p.copper, reach: p.width.max(p.height) as f64 / 2.0, via: false }).collect()).unwrap_or_default(),
        Item::Via(v) => vec![DragAnchor { owner: id, number: String::new(), net: v.net.clone(), pos: v.pos,
            copper: board.stackup.span(v.from_layer, v.to_layer), reach: v.diameter as f64 / 2.0, via: true }],
        _ => Vec::new(),
    }
}

/// Split tracks passing through a selected via so their two sides remain attached.
/// The preparation lives only in the candidate; cancellation/rejection cannot split the board.
pub fn drag_footprints(doc: &Document, req: &DragRequest) -> DragResult {
    if req.mode == MoveMode::Disconnect { return drag_connected(doc,req); }
    let fallback=BoardSettings::default();let board=board_settings(doc).map_or(&fallback,|(_,b)|b);
    let vias:Vec<_>=req.ids.iter().filter_map(|id|match doc.get(*id){Some(Item::Via(v))=>Some(v),_=>None}).collect();
    if vias.is_empty(){return drag_connected(doc,req);}
    let mut edits=Vec::new();let mut work=doc.clone();
    for (id,item) in doc.items() {
        let Item::Track(t)=item else {continue};
        if req.ids.contains(&id)||t.net.is_empty(){continue;}
        let dx=(t.b.x-t.a.x)as f64;let dy=(t.b.y-t.a.y)as f64;let len2=dx*dx+dy*dy;
        if len2==0. {continue;}
        let mut joints:Vec<_>=vias.iter().filter_map(|v|{
            if v.net!=t.net||!board.stackup.span(v.from_layer,v.to_layer).contains(&t.layer){return None;}
            let radius=(v.diameter+t.width)as f64/2.;
            if ((v.pos.x-t.a.x)as f64).hypot((v.pos.y-t.a.y)as f64)<=radius||((v.pos.x-t.b.x)as f64).hypot((v.pos.y-t.b.y)as f64)<=radius{return None;}
            let u=((v.pos.x-t.a.x)as f64*dx+(v.pos.y-t.a.y)as f64*dy)/len2;
            if u<=0.||u>=1. {return None;}
            let gap=((v.pos.x-t.a.x)as f64-u*dx).hypot((v.pos.y-t.a.y)as f64-u*dy);
            (gap<=radius).then_some((u,v.pos))
        }).collect();
        if joints.is_empty(){continue;}
        if t.locked{return rejected(DragFailure::Locked);}
        joints.sort_by(|a,b|a.0.total_cmp(&b.0));joints.dedup_by_key(|j|j.1);
        let mut points=vec![t.a];points.extend(joints.into_iter().map(|(_,p)|p));points.push(t.b);
        for (index,pair) in points.windows(2).enumerate(){
            edits.push(Edit{id:if index==0{id}else{work.alloc_id()},before:if index==0{Some(item.clone())}else{None},after:Some(Item::Track(Track{a:pair[0],b:pair[1],..t.clone()}))});
        }
    }
    if edits.is_empty(){return drag_connected(doc,req);}
    work.apply(&edits);let plan=drag_connected(&work,req);
    if plan.blocked>0&&plan.replace.is_empty()&&plan.add.is_empty()&&plan.delete.is_empty(){return plan;}
    let mut out=DragResult{rerouted:plan.rerouted,blocked:plan.blocked,..DragResult::default()};
    let mut changes:Vec<_>=plan.replace.into_iter().map(|r|Edit{id:r.id,before:work.get(r.id).cloned(),after:Some(r.item)}).collect();
    changes.extend(plan.delete.into_iter().map(|id|Edit{id,before:work.get(id).cloned(),after:None}));work.apply(&changes);
    let added:Vec<_>=plan.add.into_iter().map(|item|Edit{id:work.alloc_id(),before:None,after:Some(item)}).collect();work.apply(&added);
    diff_into(doc,&work,&mut out);out
}

/// Plans a connected drag; nothing is changed in the source document.
fn drag_connected(doc: &Document, req: &DragRequest) -> DragResult {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let moving: std::collections::HashSet<ItemId> = req.ids.iter().copied().collect();
    let mut work = doc.clone();
    let transform = |item: &Item| {
        let rotated = item.rotated(req.center, req.angle);
        let flipped = if req.flipped { rotated.mirrored(req.center) } else { rotated };
        flipped.translated(req.delta.x, req.delta.y)
    };
    let mut edits: Vec<Edit> = Vec::new();

    // Pads of the moved footprints before the move.
    let mut pads = Vec::new();
    for &id in &req.ids {
        let Some(item) = doc.get(id) else { continue };
        if crate::command::is_locked(item) { return rejected(DragFailure::Locked); }
        pads.extend(drag_anchors(doc, id, item, board));
        if !matches!(item, Item::Board(_)) {
            edits.push(Edit { id, before: Some(item.clone()), after: Some(transform(item)) });
        }
    }
    let new_pads: Vec<_> = edits.iter().flat_map(|edit| match &edit.after {
        Some(item) => drag_anchors(doc, edit.id, item, board),
        _ => vec![],
    }).collect();
    let on_moved_pad = |p: Point, layer: LayerId, net: &str, width: i64| {
        pads.iter().any(|pad| {
            pad.net == net && pad.copper.contains(&layer) && ((pad.pos.x - p.x) as f64).hypot((pad.pos.y - p.y) as f64) <= pad.reach + if pad.via { width as f64 / 2.0 } else { 0.0 }
        })
    };

    let mut reroutes = Vec::new();
    // Slid pad segments: (id, original, far end, new pad point), checked for clearance after the move.
    let mut slid: Vec<(ItemId, Track, Point, Point)> = Vec::new();
    if req.mode != MoveMode::Disconnect {
        for (id, item) in doc.items() {
            let Item::Track(t) = item else { continue };
            if moving.contains(&id) || t.net.is_empty() {
                continue;
            }
            let (on_a, on_b) = (on_moved_pad(t.a, t.layer, &t.net, t.width), on_moved_pad(t.b, t.layer, &t.net, t.width));
            if (on_a || on_b) && t.locked { return rejected(DragFailure::Locked); }
            match (on_a, on_b) {
                (false, false) => {}
                // Between two moved pads: the whole segment moves with them.
                (true, true) => edits.push(Edit { id, before: Some(item.clone()), after: Some(transform(item)) }),
                _ => {
                    let (far, near) = if on_a { (t.b, t.a) } else { (t.a, t.b) };
                    let pad = pads.iter().find(|pad| pad.net == t.net && pad.copper.contains(&t.layer) && ((pad.pos.x-near.x) as f64).hypot((pad.pos.y-near.y) as f64) <= pad.reach + if pad.via { t.width as f64 / 2.0 } else { 0.0 });
                    let target = pad.and_then(|pad| new_pads.iter().find(|p| p.owner == pad.owner && p.number == pad.number));
                    let to = target.map_or_else(|| shifted(near, req.delta), |p| p.pos);
                    // A flipped SMD connection needs a transition back to its existing routing layer.
                    if let Some(target) = target.filter(|p| !p.copper.contains(&t.layer)) {
                        if let Some(&layer) = target.copper.first() {
                            if !edits.iter().any(|e| matches!(&e.after, Some(Item::Via(v)) if v.pos == to && v.net == t.net)) {
                                let mut via = super::router::via_for(doc, &t.net, to);
                                via.from_layer = t.layer; via.to_layer = layer;
                                if let Some(style) = board.via_type_between(t.layer, layer) { via.diameter = style.diameter; via.drill = style.drill; }
                                edits.push(Edit { id: work.alloc_id(), before: None, after: Some(Item::Via(via)) });
                            }
                        }
                    }
                    if on_fixed_pad(doc, &moving, near, t) {
                        reroutes.push(Reroute { net:t.net.clone(),layer:t.layer,width:t.width,from:near,to });
                        continue;
                    }
                    if req.mode == MoveMode::Free {
                        let moved = if on_a { Track { a: to, ..t.clone() } } else { Track { b: to, ..t.clone() } };
                        edits.push(Edit { id, before: Some(item.clone()), after: Some(Item::Track(moved)) });
                    } else if let Some(s) = if req.angle == 0.0 && !req.flipped { slide(doc, &moving, id, t, on_a, req.delta) } else { None } {
                        // The pad segment keeps its angle and slides; its neighbour keeps its own angle too.
                        slid.push((id, t.clone(), far, to));
                        edits.push(Edit { id, before: Some(item.clone()), after: Some(Item::Track(s.0)) });
                        if let Some((nid, n)) = s.1 {
                            edits.push(Edit { id: nid, before: doc.get(nid).cloned(), after: Some(Item::Track(n)) });
                        }
                    } else {
                        edits.push(Edit { id, before: Some(item.clone()), after: None });
                        reroutes.push(Reroute { net: t.net.clone(), layer: t.layer, width: t.width, from: far, to });
                    }
                }
            }
        }
    }
    // Keep the curve fixed and join every moved contact, including via contacts
    // along the interior. A curve between two moved endpoints transforms whole.
    if req.mode != MoveMode::Disconnect {
        for (id, item) in doc.items() {
            let Item::ArcTrack(arc) = item else { continue };
            if moving.contains(&id) || arc.net.is_empty() { continue; }
            let at = |degrees: f64| { let r=degrees.to_radians(); Point::new(arc.center.x+(arc.radius as f64*r.cos()).round() as i64, arc.center.y+(arc.radius as f64*r.sin()).round() as i64) };
            let (a,b)=(at(arc.start_angle),at(arc.start_angle+arc.sweep));
            if on_moved_pad(a,arc.layer,&arc.net,arc.width) && on_moved_pad(b,arc.layer,&arc.net,arc.width) {
                if arc.locked { return rejected(DragFailure::Locked); }
                edits.push(Edit { id, before: Some(item.clone()), after: Some(transform(item)) });
                continue;
            }
            for anchor in pads.iter().filter(|p| p.net==arc.net && p.copper.contains(&arc.layer)) {
                let contacts=if anchor.via { vec![nearest_on_arc(arc,anchor.pos)] } else { vec![a,b] };
                for from in contacts {
                    let reach=anchor.reach+if anchor.via {arc.width as f64/2.}else{0.};
                    if ((anchor.pos.x-from.x)as f64).hypot((anchor.pos.y-from.y)as f64)>reach {continue;}
                    if let Some(target)=new_pads.iter().find(|p|p.owner==anchor.owner&&p.number==anchor.number) {
                        if from!=target.pos {reroutes.push(Reroute {net:arc.net.clone(),layer:arc.layer,width:arc.width,from,to:target.pos});}
                    }
                }
            }
        }
    }
    work.apply(&edits);

    // A slide that now crowds another net is undone and re-routed instead, so violators get pushed or avoided.
    if req.route_mode != RouteMode::Ignore && !slid.is_empty() {
        let ids: Vec<ItemId> = slid.iter().map(|s| s.0).collect();
        let crowded: std::collections::HashSet<ItemId> = super::drc::run_near(&work, &ids)
            .into_iter()
            .filter(|v| v.code == "clearance" || v.code == "short")
            .flat_map(|v| v.items)
            .collect();
        let mut undo = Vec::new();
        for (id, original, far, to) in slid {
            if !crowded.contains(&id) {
                continue;
            }
            undo.push(Edit { id, before: work.get(id).cloned(), after: None });
            // The neighbour went back to its original shape, still ending at `far`.
            for (nid, n) in doc.items() {
                if let (Item::Track(nt), Some(Item::Track(now))) = (n, work.get(nid)) {
                    if nid != id && nt.net == original.net && (nt.a == far || nt.b == far) && now != nt {
                        undo.push(Edit { id: nid, before: Some(Item::Track(now.clone())), after: Some(n.clone()) });
                    }
                }
            }
            reroutes.push(Reroute { net: original.net.clone(), layer: original.layer, width: original.width, from: far, to });
        }
        work.apply(&undo);
    }

    let mut out = DragResult::default();
    // Shorter connections first: they have the least room to detour.
    reroutes.sort_by_key(|r| (r.to.x - r.from.x).abs() + (r.to.y - r.from.y).abs());
    for r in reroutes {
        if r.from == r.to { continue; }
        let request = RouteRequest { net: r.net.clone(), layer: r.layer, start: r.from, end: r.to, width: r.width, mode: req.route_mode, corner: req.corner, ignore: vec![], posture_flip: false };
        let mut result = route(&work, &request);
        if !result.reached || result.tracks.is_empty() {
            // Avoid/push modes must not silently fall back to crossing obstacles.
            if req.route_mode != RouteMode::Ignore { return rejected(DragFailure::Unroutable); }
            // Explicit ignore mode permits the direct connection and reports it.
            out.blocked += 1;
            result = route(&work, &RouteRequest { mode: RouteMode::Ignore, ..request });
        } else {
            out.rerouted += 1;
        }
        let mut step: Vec<Edit> = Vec::new();
        let mut gone = std::collections::HashSet::new();
        for m in &result.moved {
            if gone.insert(m.id) {
                step.push(Edit { id: m.id, before: work.get(m.id).cloned(), after: None });
            }
        }
        work.apply(&step);
        let mut adds: Vec<Edit> = Vec::new();
        for item in result.moved.into_iter().map(|m| m.item).chain(result.tracks.into_iter().filter(|t| t.a != t.b).map(Item::Track)) {
            adds.push(Edit { id: work.alloc_id(), before: None, after: Some(item) });
        }
        work.apply(&adds);
    }

    diff_into(doc, &work, &mut out);
    out
}

/// Expresses a planned document as a Route command against the original.
fn diff_into(doc: &Document, work: &Document, out: &mut DragResult) {
    for (id, before) in doc.items() {
        match work.get(id) {
            Some(after) if after == before => {}
            Some(after) => out.replace.push(IdItem { id, item: after.clone() }),
            None => out.delete.push(id),
        }
    }
    for (id, item) in work.items() {
        if doc.get(id).is_none() {
            out.add.push(item.clone());
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FootprintMove {
    pub id: ItemId,
    pub delta: Point,
}

/// Several footprints, each by its own offset (Pack & Move), with tracks kept attached.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiDragRequest {
    pub moves: Vec<FootprintMove>,
    #[serde(default)]
    pub mode: MoveMode,
    pub route_mode: RouteMode,
    pub corner: CornerStyle,
}

/// Moves the footprints one after another as [`drag_footprints`] does; the result is one Route plan.
pub fn drag_each(doc: &Document, req: &MultiDragRequest) -> DragResult {
    let mut work = doc.clone();
    let mut out = DragResult::default();
    for m in &req.moves {
        if m.delta == Point::default() {
            continue;
        }
        let step = drag_footprints(&work, &DragRequest { angle: 0.0, flipped: false, center: Point::default(), ids: vec![m.id], delta: m.delta, mode: req.mode, route_mode: req.route_mode, corner: req.corner });
        if step.failure.is_some() { return step; }
        out.rerouted += step.rerouted;
        out.blocked += step.blocked;
        let mut edits: Vec<Edit> = step.replace.into_iter().map(|r| Edit { id: r.id, before: work.get(r.id).cloned(), after: Some(r.item) }).collect();
        edits.extend(step.delete.into_iter().map(|id| Edit { id, before: work.get(id).cloned(), after: None }));
        work.apply(&edits);
        let adds: Vec<Edit> = step.add.into_iter().map(|item| Edit { id: work.alloc_id(), before: None, after: Some(item) }).collect();
        work.apply(&adds);
    }
    diff_into(doc, &work, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Pad, PadKind, PadShape};
    use crate::layer;
    use crate::library::Footprint;
    use crate::pcb::board::Stackup;
    use crate::pcb::items::{PlacedFootprint, Side};
    use crate::units::mm;
    use crate::{Command, DocKind, Session};

    fn p(x: f64, y: f64) -> Point {
        Point::new(mm(x), mm(y))
    }

    fn part(designator: &str, x: f64, y: f64, net: &str) -> Item {
        Item::Footprint(PlacedFootprint {
            board_only: false,
            designator: designator.into(),
            value: String::new(),
            footprint: "L/P".into(),
            pos: p(x, y),
            rotation: 0.0,
            side: Side::Top,
            locked: false,
            pad_nets: [("1".to_string(), net.to_string())].into_iter().collect(),
            component: String::new(),
            parameters: Default::default(),
        })
    }

    fn track(a: Point, b: Point, net: &str) -> Item {
        Item::Track(Track { locked: false, a, b, width: mm(0.25), layer: layer::TOP_COPPER, net: net.into() })
    }

    /// U1 at (0,0) routed to U2 at (20,0) on net A via a corner at (10,5); U3 on net B nearby.
    fn doc(extra: Vec<Item>) -> Document {
        let mut items = vec![
            Item::Board(BoardSettings { stackup: Stackup::with_copper(2), ..BoardSettings::default() }),
            part("U1", 0.0, 0.0, "A"),
            part("U2", 20.0, 0.0, "A"),
            part("U3", 10.0, -8.0, "B"),
            track(p(0.0, 0.0), p(5.0, 5.0), "A"),
            track(p(5.0, 5.0), p(15.0, 5.0), "A"),
            track(p(15.0, 5.0), p(20.0, 0.0), "A"),
        ];
        items.extend(extra);
        let mut d = Document::from_item_list(DocKind::Pcb, items);
        let mut f = Footprint::new("P");
        f.items = vec![Item::Pad(Pad {
            number: "1".into(),
            pos: Point::new(0, 0),
            rotation: 0.0,
            shape: PadShape::Rect,
            width: mm(1.0),
            height: mm(1.0),
            corner_ratio: 0.0,
            kind: PadKind::Smd,
            drill: None,
            layer: layer::TOP_COPPER,
        })];
        d.put_cached_footprint("L/P", f);
        d
    }

    fn id_of(d: &Document, designator: &str) -> ItemId {
        d.items().find(|(_, i)| matches!(i, Item::Footprint(f) if f.designator == designator)).unwrap().0
    }

    fn req(ids: Vec<ItemId>, delta: Point, mode: MoveMode) -> DragRequest {
        DragRequest { angle: 0.0, flipped: false, center: Point::default(), ids, delta, mode, route_mode: RouteMode::Shove, corner: CornerStyle::Diagonal45 }
    }

    fn connected(d: &Document) -> bool {
        crate::pcb::connectivity::stats(d).unrouted == 0
    }

    #[test]
    fn preserve_angles_keeps_the_part_connected_and_its_id() {
        let d = doc(vec![]);
        assert!(connected(&d));
        let u2 = id_of(&d, "U2");
        let r = drag_footprints(&d, &req(vec![u2], p(0.0, -3.0), MoveMode::PreserveAngles));
        assert_eq!(r.rerouted, 0, "a free slide needs no re-routing");
        assert_eq!(r.blocked, 0);
        assert!(r.replace.iter().any(|x| x.id == u2), "the footprint keeps its ID");
        let mut s = Session::with_document(d);
        s.execute(Command::Route { delete: r.delete, add: r.add, replace: r.replace }).unwrap();
        assert!(connected(s.document()), "no connection is broken");
        for (_, i) in s.document().items() {
            if let Item::Track(t) = i {
                let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs());
                assert!(dx == 0 || dy == 0 || dx == dy, "every segment stays at 0/45/90°: {t:?}");
            }
        }
    }

    #[test]
    fn the_pad_segment_slides_along_its_neighbour_keeping_both_angles() {
        // U2's 45° segment (15,5)→(20,0) meets the horizontal (5,5)→(15,5). Move U2 down by 3 mm.
        let d = doc(vec![]);
        let r = drag_footprints(&d, &req(vec![id_of(&d, "U2")], p(0.0, -3.0), MoveMode::PreserveAngles));
        assert!(r.add.is_empty() && r.delete.is_empty(), "no segments are added or removed");
        let tracks: Vec<&Track> = r.replace.iter().filter_map(|x| if let Item::Track(t) = &x.item { Some(t) } else { None }).collect();
        assert!(tracks.iter().any(|t| (t.a == p(12.0, 5.0) && t.b == p(20.0, -3.0)) || (t.b == p(12.0, 5.0) && t.a == p(20.0, -3.0))), "still 45°, now from (12,5): {tracks:?}");
        assert!(tracks.iter().any(|t| t.a == p(5.0, 5.0) && t.b == p(12.0, 5.0)), "the horizontal neighbour just got shorter");
    }

    #[test]
    fn several_parts_each_by_their_own_offset_stay_connected() {
        let d = doc(vec![]);
        let (u1, u2) = (id_of(&d, "U1"), id_of(&d, "U2"));
        let req = MultiDragRequest {
            moves: vec![FootprintMove { id: u1, delta: p(0.0, -2.0) }, FootprintMove { id: u2, delta: p(0.0, -2.0) }],
            mode: MoveMode::PreserveAngles,
            route_mode: RouteMode::Shove,
            corner: CornerStyle::Diagonal45,
        };
        let r = drag_each(&d, &req);
        let mut s = Session::with_document(d);
        s.execute(Command::Route { delete: r.delete, add: r.add, replace: r.replace }).unwrap();
        assert!(connected(s.document()));
        let fp = |n: &str| s.document().items().find_map(|(_, i)| if let Item::Footprint(f) = i { (f.designator == n).then(|| f.pos) } else { None }).unwrap();
        assert_eq!((fp("U1"), fp("U2"), fp("U3")), (p(0.0, -2.0), p(20.0, -2.0), p(10.0, -8.0)));
    }

    #[test]
    fn a_single_segment_between_two_parts_stays_on_the_part_that_does_not_move() {
        // U3 (net B) at (10,-8) joined straight to U4 at (16,-8): moving U4 must not drag the track off U3.
        let mut d = doc(vec![part("U4", 16.0, -8.0, "B"), track(p(10.0, -8.0), p(16.0, -8.0), "B")]);
        let u4 = id_of(&d, "U4");
        let r = drag_footprints(&d, &req(vec![u4], p(4.0, 3.0), MoveMode::PreserveAngles));
        let mut s = Session::with_document(std::mem::replace(&mut d, Document::new(DocKind::Pcb)));
        s.execute(Command::Route { delete: r.delete, add: r.add, replace: r.replace }).unwrap();
        assert!(connected(s.document()), "U3 and U4 stay connected");
        assert!(s.document().items().any(|(_, i)| matches!(i, Item::Track(t) if t.net == "B" && (t.a == p(10.0, -8.0) || t.b == p(10.0, -8.0)))));
    }

    #[test]
    fn other_components_never_move() {
        let d = doc(vec![]);
        let (u1, u3) = (id_of(&d, "U1"), id_of(&d, "U3"));
        let r = drag_footprints(&d, &req(vec![id_of(&d, "U2")], p(-4.0, -6.0), MoveMode::PreserveAngles));
        assert!(!r.replace.iter().any(|x| x.id == u1 || x.id == u3));
        assert!(!r.delete.contains(&u1) && !r.delete.contains(&u3));
    }

    #[test]
    fn free_stretches_and_disconnect_leaves_the_tracks() {
        let d = doc(vec![]);
        let u2 = id_of(&d, "U2");
        let free = drag_footprints(&d, &req(vec![u2], p(0.0, -3.0), MoveMode::Free));
        assert!(free.add.is_empty());
        assert!(free.replace.iter().any(|x| matches!(&x.item, Item::Track(t) if t.b == p(20.0, -3.0))));
        let off = drag_footprints(&d, &req(vec![u2], p(0.0, -3.0), MoveMode::Disconnect));
        assert_eq!(off.replace.len(), 1, "only the footprint changes");
        assert!(off.add.is_empty() && off.delete.is_empty());
    }

    fn copper_violations(d: &Document) -> usize {
        crate::pcb::drc::run(d).violations.iter().filter(|v| v.code == "clearance" || v.code == "short").count()
    }

    #[test]
    fn rerouting_avoids_or_pushes_other_nets_instead_of_crossing_them() {
        // A net-B track lies across the path the re-routed segment needs; walking around its end is allowed.
        let d = doc(vec![track(p(17.0, 7.0), p(30.0, 7.0), "B"), track(p(22.0, -6.0), p(22.0, 7.0), "B")]);
        let r = drag_footprints(&d, &req(vec![id_of(&d, "U2")], p(4.0, 0.0), MoveMode::PreserveAngles));
        assert_eq!(r.blocked, 0);
        let mut s = Session::with_document(d);
        s.execute(Command::Route { delete: r.delete, add: r.add, replace: r.replace }).unwrap();
        assert!(connected(s.document()));
        assert_eq!(copper_violations(s.document()), 0, "the new copper neither crosses nor crowds net B");
    }

    #[test]
    fn blocked_clean_reroute_is_rejected_and_explicit_ignore_remains_available() {
        let d = doc(vec![track(p(-30.0, -1.0), p(60.0, -1.0), "B")]);
        let before=crate::io::document_to_string(&d).unwrap();
        for mode in [RouteMode::Walkaround,RouteMode::Shove] {
            let mut request=req(vec![id_of(&d,"U2")],p(0.,-0.6),MoveMode::PreserveAngles);request.route_mode=mode;
            let plan=drag_footprints(&d,&request);
            assert_eq!(plan.failure,Some(DragFailure::Unroutable));
            assert!(plan.replace.is_empty()&&plan.add.is_empty()&&plan.delete.is_empty());
            assert_eq!(crate::io::document_to_string(&d).unwrap(),before);
        }
        let mut request=req(vec![id_of(&d,"U2")],p(0.,-0.6),MoveMode::PreserveAngles);request.route_mode=RouteMode::Ignore;
        let plan=drag_footprints(&d,&request);assert_eq!(plan.failure,None);
        let mut session=Session::with_document(d);
        session.execute(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace}).unwrap();
        assert!(connected(session.document()));assert!(copper_violations(session.document())>0);
    }

    #[test]
    fn rotation_and_flip_preserve_pad_connections_and_undo() {
        for flipped in [false,true] {
            let d=doc(vec![]);let id=id_of(&d,"U1");
            let mut request=req(vec![id],p(2.,1.),MoveMode::PreserveAngles);
            request.angle=90.;request.flipped=flipped;request.center=p(2.,0.);
            let plan=drag_footprints(&d,&request);
            let mut session=crate::Session::with_document(d.clone());
            session.execute(crate::Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace}).unwrap();
            assert!(connected(session.document()),"flip={flipped}");
            if flipped {assert!(session.document().items().any(|(_,i)|matches!(i,Item::Via(_))));}
            session.undo();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),crate::io::document_to_string(&d).unwrap());
        }
    }
    fn via_fixture(from: LayerId, to: LayerId) -> (Document, ItemId) {
        use crate::pcb::connectivity::fixtures::{doc, outline};
        let via=|pos| Item::Via(crate::pcb::Via { pos,diameter:mm(0.6),drill:mm(0.3),from_layer:from,to_layer:to,net:"A".into(),locked:false });
        let mut d=doc(vec![outline(0.,0.,30.,30.),via(p(15.,15.)),via(p(5.,10.)),via(p(25.,20.)),via(p(15.,5.))]);
        let board=d.items().find(|(_,i)|matches!(i,Item::Board(_))).unwrap().0;
        let mut session=Session::with_document(d);
        let settings=BoardSettings { stackup:Stackup::with_copper(6),via_types:vec![super::super::board::ViaType{name:"Test span".into(),from_layer:from,to_layer:to,diameter:mm(0.6),drill:mm(0.3)}],..BoardSettings::default() };
        session.execute(Command::Replace{id:board,item:Item::Board(settings)}).unwrap();
        d=session.document().clone();
        let id=d.items().find(|(_,i)|matches!(i,Item::Via(v) if v.pos==p(15.,15.))).unwrap().0;
        (d,id)
    }

    #[test]
    fn dragging_a_via_keeps_straight_tracks_connected_on_every_spanned_layer() {
        use crate::pcb::{connectivity, connectivity::fixtures::track as segment};
        for mode in [MoveMode::Free,MoveMode::PreserveAngles] {
            let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
            let mut session=Session::with_document(d);
            session.execute(Command::Add{items:vec![
                segment((5.,10.),(15.,15.),0.2,layer::TOP_COPPER,"A"),
                segment((15.,15.),(25.,20.),0.2,layer::inner_copper(2),"A"),
                segment((15.,15.),(15.,5.),0.2,layer::BOTTOM_COPPER,"A"),
            ]}).unwrap();
            let before=crate::io::document_to_string(session.document()).unwrap();
            let plan=drag_footprints(session.document(),&req(vec![id],p(1.,1.),mode));assert_eq!(plan.blocked,0);
            session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
            assert!(matches!(session.document().get(id),Some(Item::Via(v)) if v.pos==p(16.,16.)));
            let conn=connectivity::analyze(session.document());
            for object in conn.objects().iter().filter(|o|o.net=="A") {assert!(conn.connected((id,None),(object.item,object.pad.as_deref())),"disconnected object {:?} in {mode:?}",object.item);}
            let after=crate::io::document_to_string(session.document()).unwrap();
            session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
            session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
        }
    }

    #[test]
    fn via_drag_ignores_other_nets_and_layers_outside_its_span() {
        use crate::pcb::connectivity::fixtures::track as segment;
        let (d,id)=via_fixture(layer::inner_copper(1),layer::inner_copper(2));
        let mut session=Session::with_document(d);
        let ids=session.execute(Command::Add{items:vec![segment((5.,10.),(15.,15.),0.2,layer::inner_copper(1),"A"),segment((15.,15.),(25.,20.),0.2,layer::BOTTOM_COPPER,"A"),segment((15.,15.),(15.,5.),0.2,layer::inner_copper(2),"B")]}).unwrap().upserted.into_iter().map(|e|e.id).collect::<Vec<_>>();
        let plan=drag_footprints(session.document(),&req(vec![id],p(1.,0.),MoveMode::Free));
        assert!(plan.replace.iter().any(|e|e.id==ids[0]));
        for untouched in &ids[1..] { assert!(!plan.replace.iter().any(|e|e.id==*untouched));assert!(!plan.delete.contains(untouched)); }
    }

    #[test]
    fn locked_attached_track_blocks_a_connected_via_drag() {
        let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
        let mut t=track(p(5.,10.),p(15.,15.),"A");if let Item::Track(t)=&mut t {t.locked=true;}
        let mut session=Session::with_document(d);session.execute(Command::Add{items:vec![t]}).unwrap();
        let plan=drag_footprints(session.document(),&req(vec![id],p(1.,0.),MoveMode::PreserveAngles));
        assert_eq!(plan.blocked,1);assert!(plan.replace.is_empty()&&plan.add.is_empty()&&plan.delete.is_empty());
    }

    #[test]
    fn arc_connected_to_a_dragged_via_keeps_its_curve_and_gets_a_joining_route() {
        let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
        let mut session=Session::with_document(d);
        let arc=Item::ArcTrack(crate::pcb::ArcTrack{center:p(15.,10.),radius:mm(5.),start_angle:90.,sweep:90.,width:mm(0.2),layer:layer::TOP_COPPER,net:"A".into(),locked:false});
        let arc_id=session.execute(Command::Add{items:vec![arc.clone()]}).unwrap().upserted[0].id;
        let plan=drag_footprints(session.document(),&req(vec![id],p(1.,1.),MoveMode::PreserveAngles));
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        assert_eq!(session.document().get(arc_id),Some(&arc));
        assert!(crate::pcb::connectivity::analyze(session.document()).connected((id,None),(arc_id,None)));
    }

    #[test]
    fn two_selected_vias_keep_the_track_between_them_connected_after_rotation() {
        let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
        let other=d.items().find(|(_,i)|matches!(i,Item::Via(v) if v.pos==p(5.,10.))).unwrap().0;
        let mut session=Session::with_document(d);
        let tid=session.execute(Command::Add{items:vec![track(p(5.,10.),p(15.,15.),"A")]}).unwrap().upserted[0].id;
        let mut request=req(vec![id,other],p(0.,0.),MoveMode::PreserveAngles);request.angle=90.;request.center=p(15.,15.);
        let plan=drag_footprints(session.document(),&request);
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        let conn=crate::pcb::connectivity::analyze(session.document());assert!(conn.connected((id,None),(tid,None)));assert!(conn.connected((other,None),(tid,None)));
    }

    #[test]
    fn via_in_the_middle_of_a_trace_drags_both_sides_in_one_transaction() {
        let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
        let left=d.items().find(|(_,i)|matches!(i,Item::Via(v) if v.pos==p(5.,10.))).unwrap().0;
        let right=d.items().find(|(_,i)|matches!(i,Item::Via(v) if v.pos==p(25.,20.))).unwrap().0;
        let mut session=Session::with_document(d);
        session.execute(Command::Add{items:vec![track(p(5.,10.),p(25.,20.),"A")]}).unwrap();
        let before=session.document().clone();
        let plan=drag_footprints(session.document(),&req(vec![id],p(0.,2.),MoveMode::PreserveAngles));
        assert_eq!(session.document(),&before,"planning must not split the source");
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        let conn=crate::pcb::connectivity::analyze(session.document());
        assert!(conn.connected((id,None),(left,None)));assert!(conn.connected((id,None),(right,None)));
        session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),crate::io::document_to_string(&before).unwrap());
    }

    #[test]
    fn moving_a_via_off_a_fixed_pad_keeps_the_pad_and_its_trace_joined() {
        use crate::pcb::connectivity::fixtures::{doc,fp,outline};
        let d=doc(vec![outline(0.,0.,30.,30.),fp("R1","L/R",10.8,10.,&[("1","A"),("2","B")]),Item::Via(crate::pcb::Via{pos:p(10.,10.),diameter:mm(0.6),drill:mm(0.3),from_layer:layer::TOP_COPPER,to_layer:layer::BOTTOM_COPPER,net:"A".into(),locked:false}),track(p(10.,10.),p(20.,10.),"A")]);
        let via=d.items().find(|(_,i)|matches!(i,Item::Via(_))).unwrap().0;
        let fp=d.items().find(|(_,i)|matches!(i,Item::Footprint(_))).unwrap().0;
        let tid=d.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
        let mut session=Session::with_document(d);
        let plan=drag_footprints(session.document(),&req(vec![via],p(0.,3.),MoveMode::PreserveAngles));
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        let conn=crate::pcb::connectivity::analyze(session.document());
        assert!(conn.connected((via,None),(fp,Some("1"))));assert!(conn.connected((via,None),(tid,None)));
    }

    #[test]
    fn via_drag_keeps_a_trace_whose_copper_touches_the_via_edge_connected() {
        use crate::pcb::connectivity::fixtures::{doc,outline};
        let v=Item::Via(crate::pcb::Via{pos:p(15.,15.),diameter:mm(0.6),drill:mm(0.3),from_layer:layer::TOP_COPPER,to_layer:layer::BOTTOM_COPPER,net:"A".into(),locked:false});
        let d=doc(vec![outline(0.,0.,30.,30.),v,track(p(20.,15.),p(15.35,15.),"A")]);
        let via=d.items().find(|(_,i)|matches!(i,Item::Via(_))).unwrap().0;
        let tid=d.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
        assert!(crate::pcb::connectivity::analyze(&d).connected((via,None),(tid,None)));
        let mut session=Session::with_document(d);let plan=drag_footprints(session.document(),&req(vec![via],p(0.,2.),MoveMode::Free));
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        assert!(crate::pcb::connectivity::analyze(session.document()).connected((via,None),(tid,None)));
    }


    #[test]
    fn via_drag_keeps_arc_interior_contacts_and_undo_redo_without_mutating_the_curve() {
        for (start,sweep) in [(0.,180.),(330.,180.),(150.,-180.)] {
            for mode in [MoveMode::Free,MoveMode::PreserveAngles] {
                let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
                let arc=Item::ArcTrack(crate::pcb::ArcTrack{center:p(15.,10.),radius:mm(5.),start_angle:start,sweep,width:mm(0.2),layer:layer::TOP_COPPER,net:"A".into(),locked:true});
                let mut session=Session::with_document(d);
                let arc_id=session.execute(Command::Add{items:vec![arc.clone()]}).unwrap().upserted[0].id;
                assert!(crate::pcb::connectivity::analyze(session.document()).connected((id,None),(arc_id,None)));
                let before=crate::io::document_to_string(session.document()).unwrap();
                let plan=drag_footprints(session.document(),&req(vec![id],p(2.,2.),mode));
                assert_eq!(plan.blocked,0);assert!(!plan.add.is_empty());
                assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before,"planning changes nothing");
                session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
                assert_eq!(session.document().get(arc_id),Some(&arc),"locked curve remains exact");
                assert!(crate::pcb::connectivity::analyze(session.document()).connected((id,None),(arc_id,None)),"start={start} sweep={sweep} mode={mode:?}");
                let after=crate::io::document_to_string(session.document()).unwrap();
                session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
                session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
                let reopened=crate::io::document_from_str(DocKind::Pcb,&after).unwrap();
                assert!(crate::pcb::connectivity::analyze(&reopened).connected((id,None),(arc_id,None)));
            }
        }
    }

    #[test]
    fn arc_contact_projection_respects_sweep_and_round_endcaps() {
        let arc=crate::pcb::ArcTrack{center:p(10.,10.),radius:mm(5.),start_angle:330.,sweep:60.,width:mm(0.2),layer:layer::TOP_COPPER,net:"A".into(),locked:false};
        assert_eq!(nearest_on_arc(&arc,p(16.,10.)),p(15.,10.));
        let opposite=nearest_on_arc(&arc,p(5.,10.));
        assert!(opposite.x>mm(14.),"missing half of circle must not be treated as copper");
        let end=nearest_on_arc(&arc,p(14.,14.));
        assert_eq!(end,p(10.+5.*30_f64.to_radians().cos(),12.5));
    }

    #[test]
    fn via_drag_reroutes_instead_of_sliding_a_locked_neighbor() {
        let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
        let mut locked=track(p(5.,10.),p(10.,10.),"A");if let Item::Track(t)=&mut locked{t.locked=true;}
        let mut session=Session::with_document(d);
        let locked_id=session.execute(Command::Add{items:vec![locked.clone(),track(p(10.,10.),p(15.,15.),"A")]}).unwrap().upserted[0].id;
        let plan=drag_footprints(session.document(),&req(vec![id],p(0.,2.),MoveMode::PreserveAngles));
        assert!(!plan.replace.iter().any(|r|r.id==locked_id));assert!(!plan.delete.contains(&locked_id));
        session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
        assert_eq!(session.document().get(locked_id),Some(&locked));
        assert!(crate::pcb::connectivity::analyze(session.document()).connected((id,None),(locked_id,None)));
    }

    #[test]
    fn via_drag_preserves_a_far_connection_to_a_curve_or_segment_interior() {
        for fixed in [
            Item::ArcTrack(crate::pcb::ArcTrack{center:p(20.,5.),radius:mm(5.),start_angle:90.,sweep:90.,width:mm(0.2),layer:layer::TOP_COPPER,net:"A".into(),locked:false}),
            track(p(18.,10.),p(23.,10.),"A"),
        ] {
            let (d,id)=via_fixture(layer::TOP_COPPER,layer::BOTTOM_COPPER);
            let mut session=Session::with_document(d);
            let fixed_id=session.execute(Command::Add{items:vec![fixed.clone(),track(p(15.,15.),p(20.,10.),"A")]}).unwrap().upserted[0].id;
            let plan=drag_footprints(session.document(),&req(vec![id],p(0.,2.),MoveMode::PreserveAngles));
            session.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true).unwrap();
            assert_eq!(session.document().get(fixed_id),Some(&fixed));
            assert!(crate::pcb::connectivity::analyze(session.document()).connected((id,None),(fixed_id,None)));
        }
    }


    #[test]
    fn multi_move_rejection_discards_earlier_steps_and_preserves_ids_history_and_dirty_state() {
        for locked in [false,true] {
            let mut extra=vec![track(p(-30.,-1.),p(60.,-1.),"B")];
            if locked { let mut attached=track(p(20.,0.),p(25.,0.),"A");if let Item::Track(t)=&mut attached{t.locked=true;}extra.push(attached); }
            let d=doc(extra);let (first,second)=(id_of(&d,"U3"),id_of(&d,"U2"));
            let mut session=Session::with_document(d);
            session.execute(Command::Add{items:vec![track(p(80.,80.),p(81.,80.),"X")]}).unwrap();session.undo().unwrap();session.mark_saved();
            let before=session.document().clone();
            let plan=drag_each(session.document(),&MultiDragRequest{moves:vec![FootprintMove{id:first,delta:p(2.,0.)},FootprintMove{id:second,delta:p(0.,-0.6)}],mode:MoveMode::PreserveAngles,route_mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45});
            assert_eq!(plan.failure,Some(if locked{DragFailure::Locked}else{DragFailure::Unroutable}));
            assert!(plan.replace.is_empty()&&plan.add.is_empty()&&plan.delete.is_empty(),"no partial move plan escapes");
            assert_eq!(session.document(),&before);assert!(session.can_redo());assert!(!session.can_undo());assert!(!session.is_dirty());
            let encoded=serde_json::to_value(&plan).unwrap();assert_eq!(encoded["failure"],if locked {"locked"} else {"unroutable"});
        }
    }

    #[test]
    fn distinct_alignment_offsets_preserve_connections_and_one_step_recovery() {
        let d=doc(vec![]);let (u1,u2)=(id_of(&d,"U1"),id_of(&d,"U2"));
        let before=crate::io::document_to_string(&d).unwrap();
        let plan=drag_each(&d,&MultiDragRequest{moves:vec![FootprintMove{id:u1,delta:p(0.,-2.)},FootprintMove{id:u2,delta:p(3.,-1.)}],mode:MoveMode::PreserveAngles,route_mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45});
        assert_eq!(plan.failure,None);assert!(serde_json::to_value(&plan).unwrap().get("failure").is_none());
        let mut session=Session::with_document(d);
        session.execute(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace}).unwrap();
        assert!(connected(session.document()));assert_eq!(copper_violations(session.document()),0);
        assert!(matches!(session.document().get(u1),Some(Item::Footprint(fp))if fp.pos==p(0.,-2.)));
        assert!(matches!(session.document().get(u2),Some(Item::Footprint(fp))if fp.pos==p(23.,-1.)));
        let after=crate::io::document_to_string(session.document()).unwrap();
        session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);assert!(!session.can_undo());
        session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
        assert!(connected(&crate::io::document_from_str(DocKind::Pcb,&after).unwrap()));
    }
}
