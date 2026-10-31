//! Native interactive routing with immutable board snapshots and speculative edits.
//!
//! The desktop [RoutingEngine] retains topology, layer indexes and a bounded
//! history of reviewed targets. Each solve starts with a fresh overlay; shared
//! spatial data cannot retain failed edits or cross a changed source board.
//!
//! Walkaround first tests direct connections, then clearance-contour vertices,
//! with a bounded grid fallback. Searches share one deterministic work allowance
//! across pad launches, connected-chain windows and fallback attempts. A limited
//! preview stops at checked copper and reports that it needs a closer target.
//!
//! Shove preserves local corridors where possible and expands to connected
//! conductor windows when neighboring bends must move. Fixed copper remains
//! anchored and failed branches restore their overlay. Pair corridors, joint
//! via transitions and length tuning use the same geometry and rule primitives.

use serde::{Deserialize, Serialize};

use crate::document::{Document, IdItem, ItemId, LayerId};
use crate::item::Item;
use crate::layer;
use crate::units::{Coord, Point};

use super::items::{Track, Via};
use super::query::{Obj, ObjKind, RuleSet};

#[path = "router_engine.rs"]
mod engine;
pub use engine::{RoutingEngine,EngineStats};
pub(crate) use engine::{WorkScope,WorkSlice,with_snapshot};

#[path = "router_geom.rs"]
mod geom;
#[path = "router_search.rs"]
mod search;
#[path = "router_chain.rs"]
mod chain;
#[path = "router_contour.rs"]
mod contour;
#[path = "router_backtrack.rs"]
mod backtrack;
#[path = "router_shove.rs"]
mod shove;
#[path = "router_via.rs"]
mod via_shove;
#[path = "router_tune.rs"]
mod tune;
#[path = "router_world.rs"]
mod world;
#[path = "router_escape.rs"]
mod escape;
#[path = "router_transition.rs"]
mod transition;
pub use transition::{pair_via_plan,PairViaPlan,PairViaRequest};

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;

use geom::{corner_path, offset_polyline, simplify, V};
use world::{Probe, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteMode {
    /// No obstacle checks.
    Ignore,
    Walkaround,
    Shove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CornerStyle {
    Diagonal45,
    /// Routed like `Diagonal45` (results are straight segments).
    Arc45,
    Orthogonal90,
    Any,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    pub net: String,
    pub layer: LayerId,
    pub start: Point,
    pub end: Point,
    pub width: Coord,
    pub mode: RouteMode,
    pub corner: CornerStyle,
    /// Items not treated as obstacles (e.g. the segments being re-routed).
    #[serde(default)]
    pub ignore: Vec<ItemId>,
    /// Put the 45° (or vertical) leg first.
    #[serde(default)]
    pub posture_flip: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteResult {
    /// New segments, start → end (or as far as possible).
    pub tracks: Vec<Track>,
    /// The end was reached without violations.
    pub reached: bool,
    /// Shoved other-net copper. Each entry's `id` is an original source item;
    /// one source may own several pieces, including a moved via's new tethers. Apply
    /// as `Change { delete: unique ids, add: items }`.
    pub moved: Vec<IdItem>,
    pub blocked_at: Option<Point>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TuneStyle {
    /// Square meanders.
    Trombone,
    /// Meanders with 45° chamfered corners.
    #[default]
    Accordion,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneRequest {
    /// A chain of same-net segments.
    pub tracks: Vec<ItemId>,
    pub target_length: Coord,
    /// Maximum meander height (centerline to centerline).
    pub amplitude: Coord,
    /// Distance between meander legs (centerline to centerline).
    pub spacing: Coord,
    #[serde(default)]
    pub style: TuneStyle,
}

struct Outcome {
    path: Vec<Point>,
    reached: bool,
    moved: Vec<(ItemId, Vec<Track>)>,
    blocked_at: Option<Point>,
    message: String,
}

fn route_in(world: &mut World, probe: &Probe, start: Point, end: Point, mode: RouteMode, corner: CornerStyle, flip: bool) -> Outcome {
    let direct = corner_path(start, end, corner, flip);
    match mode {
        RouteMode::Ignore => Outcome { path: direct, reached: true, moved: vec![], blocked_at: None, message: String::new() },
        RouteMode::Walkaround => walk(world, probe, start, end, corner, flip, String::new()),
        RouteMode::Shove => {
            if world.path_clear(&direct, probe) {
                return Outcome { path: direct, reached: true, moved: vec![], blocked_at: None, message: String::new() };
            }
            let mut err = String::new();
            for f in [flip, !flip] {
                let path = corner_path(start, end, corner, f);
                if path.len() < 2 {
                    break;
                }
                world.removed.clear();
                world.added.clear();
                match shove::shove(world, probe, &path) {
                    Ok(moved) => return Outcome { path, reached: true, moved, blocked_at: None, message: String::new() },
                    Err(e) => err = e,
                }
                if corner == CornerStyle::Any {
                    break;
                }
            }
            world.removed.clear();
            world.added.clear();
            walk(world, probe, start, end, corner, flip, format!("shove failed ({err}); walked around"))
        }
    }
}

fn walk(world: &World, probe: &Probe, start: Point, end: Point, corner: CornerStyle, flip: bool, note: String) -> Outcome {
    let w = search::walkaround(world, probe, start, end, corner, flip);
    let message = match (note.is_empty(), w.message.is_empty()) {
        (true, _) => w.message,
        (false, true) => note,
        (false, false) => format!("{note}; {}", w.message),
    };
    Outcome { path: w.path, reached: w.reached, moved: vec![], blocked_at: w.blocked_at, message }
}

pub(crate) fn cleanup_corner_path(a:Point,b:Point,style:CornerStyle,flip:bool)->Vec<Point>{corner_path(a,b,style,flip)}

fn tracks_of(pts: &[Point], width: Coord, layer: LayerId, net: &str) -> Vec<Track> {
    pts.windows(2).filter(|w| w[0] != w[1]).map(|w| Track { locked: false, a: w[0], b: w[1], width, layer, net: net.to_string() }).collect()
}

fn moved_items(moved: Vec<(ItemId, Vec<Track>)>) -> Vec<IdItem> {
    moved.into_iter().flat_map(|(id, ts)| ts.into_iter().map(move |t| IdItem { id, item: Item::Track(t) })).collect()
}

// Zone fills are regenerated around committed tracks; they are not fixed obstacles.
// Keep-outs, pads, tracks, vias and the board edge still constrain the route.
fn routing_ignored(doc: &Document, ignored: &[ItemId]) -> Vec<ItemId> {
    ignored.iter().copied().chain(doc.items().filter_map(|(id, item)| {
        matches!(item, Item::Zone(zone) if !zone.keepout).then_some(id)
    })).collect()
}

/// Routes one track from `req.start` toward `req.end`.
pub fn route(doc: &Document, req: &RouteRequest) -> RouteResult {
    let _work=WorkScope::enter();
    route_indexed(doc,req,true)
}

fn route_base(doc: &Document, req: &RouteRequest) -> RouteResult {
    route_indexed(doc,req,false)
}

fn route_indexed(doc: &Document, req: &RouteRequest, allow_via_shove:bool) -> RouteResult {
    if req.mode == RouteMode::Ignore {
        let path = corner_path(req.start, req.end, req.corner, req.posture_flip);
        return RouteResult { tracks: tracks_of(&path, req.width, req.layer, &req.net), reached: true, moved: vec![], blocked_at: None, message: String::new() };
    }
    let ignored = routing_ignored(doc, &req.ignore);
    let mut world = World::build(doc, req.layer, &ignored);
    if let Some(error)=world.rules.errors().first(){return RouteResult{tracks:vec![],reached:false,moved:vec![],blocked_at:Some(req.start),message:format!("Fix invalid design rules before checked routing: {error}")};}
    let net = world.net_id(&req.net);
    let excl = [net];
    let probe = Probe { hw: req.width as f64 / 2.0, net, excl: &excl };
    // Expensive multi-layer via displacement is needed only for a real via
    // collision, rather than every nearby via inside a conservative search radius.
    if allow_via_shove && req.mode==RouteMode::Shove {
        let path=corner_path(req.start,req.end,req.corner,req.posture_flip);
        let via_hit=path.windows(2).any(|w|world.seg_collisions(V::of(w[0]),V::of(w[1]),&probe).into_iter().any(|hit|matches!(hit,world::Hit::Ob(i) if world.obstacles[i as usize].kind==world::ObKind::Via)));
        if via_hit {if let Some(result)=via_shove::route(doc,req){return result;}}
    }
    let o = escape::route(&mut world, &probe, doc, req);
    RouteResult { tracks: tracks_of(&o.path, req.width, req.layer, &req.net), reached: o.reached, moved: moved_items(o.moved), blocked_at: o.blocked_at, message: o.message }
}

fn unit_dir(a: Point, b: Point) -> V {
    (V::of(b) - V::of(a)).norm()
}

/// Right-hand normal of travel direction `d`, scaled.
fn right(d: V, k: f64) -> V {
    V::new(d.y, -d.x) * k
}

/// Routes a differential pair. `pos` describes the positive track (its
/// start/end and width); the negative track runs on the right-hand side of
/// travel at center distance `width + gap`. Both nets are routed as one
/// corridor, so the tracks stay coupled through every bend (the inner track
/// is shorter). Shoved tracks are reported in the positive result.
pub fn route_diff_pair(doc: &Document, pos: &RouteRequest, neg_net: &str, gap: Coord) -> (RouteResult, RouteResult) {
    route_diff_pair_from(doc,pos,neg_net,gap,None)
}

/// Continue a pair from both accepted endpoints. The partner's old endpoint
/// must not be recomputed from a different outgoing direction.
pub fn route_diff_pair_from(doc: &Document, pos: &RouteRequest, neg_net: &str, gap: Coord, neg_start: Option<Point>) -> (RouteResult, RouteResult) {
    let _work=WorkScope::enter();
    let pads:Vec<_> = super::geometry::all_pads(doc, world::board_of(doc)).into_iter().map(|pad|pad.for_layer(pos.layer)).collect();
    let source = pads.iter().find(|pad| pad.net == pos.net && pad.on_layer(pos.layer) && pad.pos == pos.start);
    let anchor_at = |at:Point| {
        let source=pads.iter().find(|pad|pad.net==pos.net && pad.on_layer(pos.layer) && pad.pos==at);
        source.and_then(|source| {
        pads.iter().filter(|pad| pad.owner == source.owner && pad.net == neg_net && pad.on_layer(pos.layer))
            .min_by_key(|pad| {let x=(pad.pos.x-at.x) as i128;let y=(pad.pos.y-at.y) as i128;x*x+y*y}).map(|pad|pad.pos)
    }).or_else(|| {
        // Resuming on copper must use the existing partner track, rather than
        // invent an offset that can pass through its copper or a ground pad.
        let on_track=doc.items().any(|(_,item)|matches!(item,Item::Track(t) if t.net==pos.net && t.layer==pos.layer && geom::point_seg_dist(V::of(at),V::of(t.a),V::of(t.b))<=4.0));
        if !on_track {return None;}
        doc.items().filter_map(|(_,item)|{
            let Item::Track(t)=item else{return None};
            if t.net!=neg_net || t.layer!=pos.layer {return None;}
            let a=V::of(t.a);let d=V::of(t.b)-a;
            let projected=if d.dot(d)<1.0 {a}else{a+d*((V::of(at)-a).dot(d)/d.dot(d)).clamp(0.0,1.0)};
            let distance=(projected-V::of(at)).len();
            (distance <= 4.0*(pos.width+gap).max(1) as f64).then_some((projected.round(),distance))
        }).min_by(|a,b|a.1.total_cmp(&b.1)).map(|(at,_)|at)
    })
    };
    let partner=neg_start.or_else(||anchor_at(pos.start));
    let target=anchor_at(pos.end);
    if source.is_some() && partner.is_none() {
        let blocked=RouteResult{tracks:vec![],reached:false,moved:vec![],blocked_at:Some(pos.start),message:"No partner pad on this component and copper layer; choose a paired pad".into()};
        return (blocked.clone(),blocked);
    }
    if let Some(anchor)=partner {
        let pitch=(V::of(anchor)-V::of(pos.start)).len();
        // Pad escapes may start uncoupled, then widen to the requested pair gap.
        // Existing track anchors still need the selected coupled spacing.
        let clearance=RuleSet::new(world::board_of(doc)).clearance(&Obj::new(ObjKind::Track,&pos.net,Some(pos.layer)),&Obj::new(ObjKind::Track,neg_net,Some(pos.layer)));
        let required=(pos.width+if source.is_some(){clearance}else{gap}) as f64;
        if pitch+4.0<required {
            let blocked=RouteResult{tracks:vec![],reached:false,moved:vec![],blocked_at:Some(pos.start),message:format!("Pair anchors are {:.4} mm apart; {:.4} mm required by the selected width and gap. Review the pair settings or choose another start",pitch/1e6,required/1e6)};
            return (blocked.clone(),blocked);
        }
    }
    let result = route_pair_corridor(doc,pos,neg_net,gap,partner,target);
    if result.0.reached || pos.width <= 0 || gap <= 0 || pos.net == neg_net {return result;}
    let Some(anchor) = partner else {return result};
    let separation = V::of(anchor)-V::of(pos.start);
    if source.is_none() && separation.len() < (pos.width+gap) as f64-4.0 {return result;}
    // Escape narrow pad rows with two independently checked necks before coupling.
    // Bounded walkaround candidates never shove already accepted copper.
    let mut direction = V::new(-separation.y,separation.x).norm();
    if direction.dot(V::of(pos.end)-V::of(pos.start)) < 0.0 {direction=direction*-1.0;}
    let spacing = (pos.width+gap) as f64;
    let side = separation.norm();
    let extent = source.map_or(spacing,|pad|pad.width.max(pad.height) as f64);
    let directions=[direction,V::new(direction.x-direction.y,direction.x+direction.y).norm(),V::new(direction.x+direction.y,direction.y-direction.x).norm()];
    for (index,direction) in directions.into_iter().enumerate() {
    if direction.dot(V::of(pos.end)-V::of(pos.start))<=0.0 || (pos.corner==CornerStyle::Orthogonal90&&index>0){continue;}
    let mut launch_side=V::new(direction.y,-direction.x);
    if launch_side.dot(side)<0.0{launch_side=launch_side*-1.0;}
    for factor in [1.0,2.0,4.0] {
        let distance=extent.max(separation.len()).max(spacing)*factor;
        if distance >= (V::of(pos.end)-V::of(pos.start)).len() {continue;}
        let midpoint=(V::of(pos.start)+V::of(anchor))*0.5+direction*distance;
        let launch=if index==0{(V::of(pos.start)+direction*distance).round()}else{(midpoint-launch_side*(spacing/2.0)).round()};
        let other=if index==0{(V::of(launch)+side*spacing).round()}else{(midpoint+launch_side*(spacing/2.0)).round()};
        let mut request=pos.clone();request.mode=RouteMode::Walkaround;request.start=launch;
        let (mut p,mut n)=route_pair_corridor(doc,&request,neg_net,gap,Some(other),target);
        if !p.reached || !n.reached {continue;}
        request.start=pos.start;request.end=launch;
        let neck=route(doc,&request);
        request.net=neg_net.into();request.start=anchor;request.end=other;
        let other_neck=route(doc,&request);
        if !neck.reached || !other_neck.reached || !neck.moved.is_empty() || !other_neck.moved.is_empty() {continue;}
        p.tracks.splice(0..0,neck.tracks);n.tracks.splice(0..0,other_neck.tracks);
        // The coupled section already satisfies gap. Escape necks must satisfy
        // electrical clearance while transitioning from the actual pad pitch.
        let required=RuleSet::new(world::board_of(doc)).clearance(&Obj::new(ObjKind::Track,&pos.net,Some(pos.layer)),&Obj::new(ObjKind::Track,neg_net,Some(pos.layer)));
        if p.tracks.iter().all(|p|n.tracks.iter().all(|n|geom::seg_seg_dist(V::of(p.a),V::of(p.b),V::of(n.a),V::of(n.b))+4.0 >= (pos.width+required) as f64)) {return (p,n);}
    }
    }
    result
}

fn route_pair_corridor(doc: &Document, pos: &RouteRequest, neg_net: &str, gap: Coord, neg_start: Option<Point>, neg_end: Option<Point>) -> (RouteResult, RouteResult) {
    let fail = |msg: &str| {
        let r = RouteResult { tracks: vec![], reached: false, moved: vec![], blocked_at: Some(pos.start), message: msg.to_string() };
        (r.clone(), r)
    };
    if pos.width<=0 || gap<=0 || pos.net.is_empty() || neg_net.is_empty() || pos.net==neg_net { return fail("Pair routing needs two different nets, positive width and a positive gap"); }
    let w = pos.width;
    let c = w as f64 + gap as f64;
    let miter = match pos.corner {
        CornerStyle::Diagonal45 | CornerStyle::Arc45 => c * (1.0 / (22.5f64).to_radians().cos() - 1.0),
        CornerStyle::Orthogonal90 | CornerStyle::Any => c * (std::f64::consts::SQRT_2 - 1.0),
    };
    let corridor = 2.0 * w as f64 + gap as f64 + miter + 4.0;
    let ignored = routing_ignored(doc, &pos.ignore);
    let mut world = World::build(doc, pos.layer, &ignored);
    if let Some(error)=world.rules.errors().first(){return fail(&format!("Fix invalid design rules before checked routing: {error}"));}
    let pnet = world.net_id(&pos.net);
    let nnet = world.net_id(neg_net);
    let excl = [pnet, nnet];
    let probe = Probe { hw: corridor / 2.0, net: pnet, excl: &excl };

    let direct = corner_path(pos.start, pos.end, pos.corner, pos.posture_flip);
    if direct.len() < 2 {
        return fail("start and end coincide");
    }
    let (mut d0, mut d1) = (unit_dir(direct[0], direct[1]), unit_dir(direct[direct.len() - 2], direct[direct.len() - 1]));
    let side = if neg_start.is_some_and(|at| (V::of(at)-V::of(pos.start)).dot(right(d0,1.0))<0.0) {-1.0} else {1.0};
    let mut out = None;
    let mut corridor_start=pos.start;
    for _ in 0..3 {
        let sc = (V::of(pos.start) + right(d0, side*c / 2.0)).round();corridor_start=sc;
        let ec = (V::of(pos.end) + right(d1, side*c / 2.0)).round();
        world.removed.clear();
        world.added.clear();
        let o = route_in(&mut world, &probe, sc, ec, pos.mode, pos.corner, pos.posture_flip);
        let p = &o.path;
        if p.len() < 2 {
            out = Some(o);
            break;
        }
        let (n0, n1) = (unit_dir(p[0], p[1]), unit_dir(p[p.len() - 2], p[p.len() - 1]));
        let same = (n0 - d0).len() < 1e-6 && (n1 - d1).len() < 1e-6;
        d0 = n0;
        d1 = n1;
        out = Some(o);
        if same {
            break;
        }
    }
    let o = out.expect("at least one attempt");
    if o.path.len() < 2 {
        let mut message=if o.message.is_empty(){"no room for the pair".to_string()}else{o.message.clone()};
        if let Some(hit)=world.seg_collisions(V::of(corridor_start),V::of(corridor_start),&probe).first().copied() {
            let detail=match hit {
                world::Hit::Ob(index)=>{let ob=&world.obstacles[index as usize];format!("{}{} on net '{}'",ob.kind.name(),ob.id.map(|id|format!(" #{}",id.0)).unwrap_or_default(),world.net_name(ob.net))},
                _=>world.hit_kind(hit).name().to_string(),
            };
            message=format!("Pair corridor start blocked by {detail}; choose another start or direction");
        }
        return fail(&message);
    }
    let center = simplify(&o.path);
    let mut ppath = offset_polyline(&center, side*c / 2.0);
    let mut npath = offset_polyline(&center, -side*c / 2.0);
    // Snap / connect the positive track to the requested endpoints.
    let fix = |path: &mut Vec<Point>, at_start: bool, want: Point| {
        let i = if at_start { 0 } else { path.len() - 1 };
        let p = path[i];
        if (p.x - want.x).abs() <= 4 && (p.y - want.y).abs() <= 4 {
            path[i] = want;
        } else if at_start {
            path.insert(0, want);
        } else {
            path.push(want);
        }
    };
    fix(&mut ppath, true, pos.start);
    if o.reached {
        fix(&mut ppath, false, pos.end);
    }
    if let Some(anchor)=neg_start {
        // At an existing bend the outer mate starts beyond the positive corner.
        // Trim the virtual offset prefix instead of connecting backwards to it.
        if (V::of(anchor)-V::of(npath[0])).len()<=8.0*c {
            if let Some(index)=npath.windows(2).position(|segment|geom::point_seg_dist(V::of(anchor),V::of(segment[0]),V::of(segment[1]))<=4.0) {
                npath.drain(..index);npath[0]=anchor;
            }
        }
        if anchor!=npath[0] {
        let previous=V::new(-(anchor.y-pos.start.y) as f64,(anchor.x-pos.start.x) as f64).norm()*side;
        let next=unit_dir(npath[0],npath[1]);let cross=previous.cross(next);
        if cross.abs()>1e-6 {
            let distance=(V::of(npath[0])-V::of(anchor)).cross(next)/cross;
            let joint=V::of(anchor)+previous*distance;
            if distance < -4.0 || (V::of(npath[0])-joint).dot(next) < -4.0 || (joint-V::of(anchor)).len()>8.0*c {
                return fail("Pair turn needs backtracking; change direction or undo the previous step");
            }
            npath.insert(0,joint.round());
        }
        npath.insert(0,anchor);
    }}
    if o.reached {if let Some(anchor)=neg_end {
        let last=*npath.last().unwrap();
        if anchor!=last {
            if (V::of(anchor)-V::of(last)).len()>8.0*c {return fail("Partner target needs a longer fanout; route closer to the paired pads first");}
            if let Some(index)=npath.windows(2).rposition(|segment|geom::point_seg_dist(V::of(anchor),V::of(segment[0]),V::of(segment[1]))<=4.0) {
                npath.truncate(index+2);*npath.last_mut().unwrap()=anchor;
            }else{
                npath.extend(corner_path(last,anchor,pos.corner,pos.posture_flip).into_iter().skip(1));
            }
        }
    }}
    let ppath = simplify(&ppath);
    npath = simplify(&npath);
    if ppath.len()<2 || npath.len()<2 {return fail("Move beyond the pair corner to continue both tracks");}
    // Verify both tracks against the (possibly shoved) world.
    let tprobe = Probe { hw: w as f64 / 2.0, net: pnet, excl: &[pnet] };
    let nprobe = Probe { hw: w as f64 / 2.0, net: nnet, excl: &[nnet] };
    let clean = world.path_clear(&ppath, &tprobe) && world.path_clear(&npath, &nprobe);
    let required_gap=if pos.mode==RouteMode::Ignore {gap}else{gap.max(world.rules.clearance(&Obj::new(ObjKind::Track,&pos.net,Some(pos.layer)),&Obj::new(ObjKind::Track,neg_net,Some(pos.layer))))};
    let closest=ppath.windows(2).flat_map(|p|npath.windows(2).map(move|n|geom::seg_seg_dist(V::of(p[0]),V::of(p[1]),V::of(n[0]),V::of(n[1])))).fold(f64::INFINITY,f64::min)-w as f64;
    if closest+4.0 < required_gap as f64 { return fail(&format!("Pair gap is {:.4} mm; {:.4} mm required. Change direction or review the pair gap/rules",closest/1e6,required_gap as f64/1e6)); }
    if !clean && pos.mode!=RouteMode::Ignore { return fail("Pair tracks violate clearance; no partial pair was applied"); }
    let reached = o.reached && (clean || pos.mode == RouteMode::Ignore);
    let message = o.message.clone();
    let blocked_at = if reached { None } else { o.blocked_at.or(ppath.last().copied()) };
    let p = RouteResult { tracks: tracks_of(&ppath, w, pos.layer, &pos.net), reached, moved: moved_items(o.moved), blocked_at, message: message.clone() };
    let n = RouteResult { tracks: tracks_of(&npath, w, pos.layer, neg_net), reached, moved: vec![], blocked_at, message };
    (p, n)
}

/// Replaces a chain of segments with a meandered chain whose length is
/// within 0.5 % of `target_length`.
pub fn tune_length(doc: &Document, req: &TuneRequest) -> Result<Vec<Track>, String> {
    tune::tune(doc, req)
}

/// Recompute under the caller's document lock and reject a changed reviewed preview.
pub fn tune_length_checked(doc: &Document, req: &TuneRequest, expected: Option<&[Track]>) -> Result<Vec<Track>,String> {
    let plan=tune_length(doc,req)?;
    if expected.is_some_and(|shown|shown!=plan) {return Err("Tuning result changed; review the new preview before applying".into());}
    Ok(plan)
}

/// Build one transaction; an already-in-range chain retains its IDs and history.
pub fn tune_length_command(doc: &Document, req: &TuneRequest, expected: Option<&[Track]>) -> Result<crate::Command,String> {
    let plan=tune_length_checked(doc,req,expected)?;
    let key=|track:&Track| {let a=(track.a.x,track.a.y);let b=(track.b.x,track.b.y);(track.layer.0,track.net.clone(),track.width,track.locked,a.min(b),a.max(b))};
    let original:std::collections::BTreeSet<_>=req.tracks.iter().filter_map(|id|match doc.get(*id){Some(Item::Track(track))=>Some(key(track)),_=>None}).collect();
    let unchanged=plan.len()==req.tracks.len() && plan.iter().map(key).collect::<std::collections::BTreeSet<_>>()==original;
    Ok(if unchanged {crate::Command::Change{delete:vec![],add:vec![]}} else {crate::Command::Route{delete:req.tracks.clone(),add:plan.into_iter().map(Item::Track).collect(),replace:vec![]}})
}

/// Routed copper length of a net: tracks plus arc tracks (vias count 0).
pub fn net_length(doc: &Document, net: &str) -> Coord {
    let mut total = 0.0f64;
    for (_, item) in doc.items() {
        match item {
            Item::Track(t) if t.net == net => total += (V::of(t.b) - V::of(t.a)).len(),
            Item::ArcTrack(a) if a.net == net => total += a.radius as f64 * a.sweep.abs().to_radians(),
            _ => {}
        }
    }
    total.round() as Coord
}

/// Clearance for paired annuli and their source-layer escape necks.
/// Only the actual shared via span and the two pair nets affect fanout size.
pub fn pair_via_clearance(doc: &Document, positive: &Via, negative: &Via, source: LayerId) -> Result<Coord,String> {
    let board=world::board_of(doc);let copper=board.stackup.copper_layers();
    if positive.net.is_empty() || negative.net.is_empty() || positive.net==negative.net {return Err("Pair vias need two different assigned nets".into());}
    for via in [positive,negative] {
        if via.from_layer==via.to_layer || !copper.contains(&via.from_layer) || !copper.contains(&via.to_layer) || !board.stackup.span(via.from_layer,via.to_layer).contains(&source) {
            return Err("Pair vias must cross the active routing layer with valid copper spans".into());
        }
    }
    let rules=RuleSet::new(board);let pspan=board.stackup.span(positive.from_layer,positive.to_layer);let nspan=board.stackup.span(negative.from_layer,negative.to_layer);let mut required=0;
    for layer in pspan.into_iter().filter(|layer|nspan.contains(layer)) {
        required=required.max(rules.clearance(&Obj::new(ObjKind::Via,&positive.net,Some(layer)),&Obj::new(ObjKind::Via,&negative.net,Some(layer))));
    }
    for (p,n) in [(ObjKind::Track,ObjKind::Track),(ObjKind::Track,ObjKind::Via),(ObjKind::Via,ObjKind::Track)] {
        required=required.max(rules.clearance(&Obj::new(p,&positive.net,Some(source)),&Obj::new(n,&negative.net,Some(source))));
    }
    Ok(required)
}

/// A new through via for `net` at `pos`, sized by the via-style rule.
pub fn via_for(doc: &Document, net: &str, pos: Point) -> Via {
    let board = world::board_of(doc);
    let (diameter, drill, _) = RuleSet::new(board).via_style(&Obj::new(ObjKind::Via, net, None));
    let cu = board.stackup.copper_layers();
    Via { locked: false,
        pos,
        diameter,
        drill,
        from_layer: cu.first().copied().unwrap_or(layer::TOP_COPPER),
        to_layer: cu.last().copied().unwrap_or(layer::BOTTOM_COPPER),
        net: net.to_string(),
    }
}

/// A via for a layer change from `a` to `b`: the smallest defined via type that
/// connects both layers (blind, buried or micro), else a through via.
pub fn via_between(doc: &Document, net: &str, pos: Point, a: LayerId, b: LayerId) -> Via {
    let board = world::board_of(doc);
    match board.via_type_between(a, b) {
        Some(t) => Via { locked: false, pos, diameter: t.diameter, drill: t.drill, from_layer: t.from_layer, to_layer: t.to_layer, net: net.to_string() },
        None => via_for(doc, net, pos),
    }
}

/// Read-only connected-track drag plan, including pushed foreign tracks.
pub fn plan_track_edit(doc:&Document, replace:Vec<IdItem>, add:Vec<Item>, mode:RouteMode)->Result<super::drag::DragResult,String> {
    plan_track_edit_removed(doc,replace,add,vec![],mode)
}
pub(crate) fn plan_track_edit_removed(doc:&Document, replace:Vec<IdItem>, add:Vec<Item>, delete:Vec<ItemId>, mode:RouteMode)->Result<super::drag::DragResult,String> {
    let _work=WorkScope::enter();
    let mut ids=std::collections::BTreeSet::new();let mut tracks=Vec::new();
    for entry in &replace {
        if !ids.insert(entry.id){return Err("Duplicate track in drag plan".into());}
        let valid=match (doc.get(entry.id),&entry.item) {
            (Some(Item::Track(before)),Item::Track(after))=>{let mut allowed=before.clone();allowed.a=after.a;allowed.b=after.b;!before.locked&&allowed==*after},
            (Some(Item::ArcTrack(before)),Item::ArcTrack(after))=>{let mut allowed=before.clone();allowed.center=after.center;allowed.radius=after.radius;allowed.start_angle=after.start_angle;allowed.sweep=after.sweep;!before.locked&&allowed==*after},
            (Some(Item::ArcTrack(before)),Item::Track(after))=>!before.locked&&!after.locked&&before.width==after.width&&before.layer==after.layer&&before.net==after.net,
            _=>false,
        };
        if !valid{return Err("Locked copper or conductor properties changed during drag".into());}
        tracks.extend(drag_capsules(&entry.item)?);
    }
    if replace.is_empty(){return Err("Select tracks to drag".into());}
    for id in &delete {if !ids.insert(*id)||!matches!(doc.get(*id),Some(Item::Track(t))if !t.locked)&&!matches!(doc.get(*id),Some(Item::ArcTrack(t))if !t.locked){return Err("Only unlocked tracks in the affected section may be removed".into());}}
    for item in &add {tracks.extend(drag_capsules(item)?); }
    // Splitting a conductor can re-express its fixed outer copper with new
    // identities. Exact same-net, same-width coverage is unchanged geometry,
    // not a new pusher against an unrelated pre-existing clearance finding.
    let unchanged=|t:&Track|ids.iter().any(|id|match doc.get(*id){Some(Item::Track(old))=>same_track_coverage(t,old),_=>false});
    let mut out=super::drag::DragResult{replace,add,delete,..Default::default()};
    if mode!=RouteMode::Ignore {
        let ignored=routing_ignored(doc,&ids.iter().copied().collect::<Vec<_>>());
        let layers:std::collections::BTreeSet<_>=tracks.iter().map(|t|t.layer).collect();
        for layer in layers {
            let mut world=World::build(doc,layer,&ignored);if let Some(error)=world.rules.errors().first(){return Err(format!("Fix invalid design rules before checked dragging: {error}"));}let mut roots=Vec::new();
            for t in tracks.iter().filter(|t|t.layer==layer){
                let net=world.net_id(&t.net);
                if unchanged(t){
                    let key=world.track_key(net);world.added.push(world::Obstacle{core:world::Core::Seg(V::of(t.a),V::of(t.b)),r:t.width as f64/2.,net,key,kind:world::ObKind::Track,id:None,track:None});
                }else{roots.push(shove::Pusher{a:V::of(t.a),b:V::of(t.b),hw:t.width as f64/2.,net});}
            }
            if mode==RouteMode::Shove {
                for (id,pieces) in shove::shove_many(&mut world,&roots)? {out.delete.push(id);out.add.extend(pieces.into_iter().map(Item::Track));out.rerouted+=1;}
            }else{for p in roots {let excl=[p.net];if !world.seg_clear(p.a,p.b,&Probe{hw:p.hw,net:p.net,excl:&excl}){return Err("Track drag is blocked; choose another position or Push mode".into());}}}
        }
    }
        // Pours are regenerated on commit; check all fixed copper and mechanical obstacles now.
        let mut delete:Vec<_>=out.replace.iter().map(|r|r.id).chain(out.delete.iter().copied()).collect();
        delete.extend(doc.items().filter_map(|(id,item)|matches!(item,Item::Zone(z)if !z.keepout).then_some(id)));
        let added=out.replace.iter().map(|r|r.item.clone()).chain(out.add.iter().cloned()).collect();
        let mut check=crate::Session::with_document(doc.clone());let delta=check.execute(crate::Command::Change{delete,add:added}).map_err(|e|e.to_string())?;
        let touched:Vec<_>=delta.upserted.iter().map(|r|r.id).collect();
        // A pushed span must not lift away from a fixed pad, via, arc or branch.
        let removed:std::collections::HashSet<_>=out.replace.iter().map(|r|r.id).chain(out.delete.iter().copied()).collect();
        let before=engine::cached_copper(doc);
        let after=super::connectivity::Index::new(super::connectivity::build_objects(check.document(),world::board_of(check.document()),None,None));
        for (i,original) in before.objs.iter().enumerate().filter(|(_,o)|removed.contains(&o.item)) {
            for j in before.tree.query(before.geos[i].bb.grow(super::connectivity::geo::TOUCH)) {
                let fixed=&before.objs[j];if removed.contains(&fixed.item)||fixed.net!=original.net||!fixed.shares_layer(original)||matches!(doc.get(fixed.item),Some(Item::Zone(_))){continue;}
                let touch=|a:&super::connectivity::geo::Geo,b:&super::connectivity::geo::Geo|super::connectivity::geo::gap(a,b,super::connectivity::geo::TOUCH).is_some_and(|(gap,_)|gap<=super::connectivity::geo::TOUCH);
                if !touch(&before.geos[i],&before.geos[j]){continue;}
                if !after.objs.iter().enumerate().any(|(k,o)|touched.contains(&o.item)&&o.net==fixed.net&&o.shares_layer(fixed)&&touch(&after.geos[k],&before.geos[j])){return Err("Track drag would detach fixed copper; choose another position".into());}
            }
        }

        if mode!=RouteMode::Ignore {let changed:Vec<_>=delta.upserted.iter().filter(|r|!matches!(&r.item,Item::Track(t)if unchanged(t))).map(|r|r.id).collect();if let Some(v)=super::drc::run_near(check.document(),&changed).into_iter().find(|v|v.severity==super::drc::Severity::Error){return Err(format!("Track drag blocked: {}",v.message));}}
    Ok(out)
}

/// Conservative collision capsules; committed curves remain exact circular arcs.
fn drag_capsules(item:&Item)->Result<Vec<Track>,String>{
    match item {
        Item::Track(t) if t.a!=t.b&&t.width>0=>Ok(vec![t.clone()]),
        Item::ArcTrack(a) if a.radius>0&&a.width>0&&a.start_angle.is_finite()&&a.sweep.is_finite()&&a.sweep.abs()>1e-8&&a.sweep.abs()<=360.=>{
            let count=((a.sweep.abs()/3.).ceil()as usize).clamp(1,120);let step=a.sweep/count as f64;
            let sag=(a.radius as f64*(1.-(step/2.).to_radians().cos())).ceil()as i64+2;
            let point=|i:usize|{let angle=(a.start_angle+step*i as f64).to_radians();Point::new(a.center.x+(a.radius as f64*angle.cos()).round()as i64,a.center.y+(a.radius as f64*angle.sin()).round()as i64)};
            Ok((0..count).map(|i|Track{a:point(i),b:point(i+1),width:a.width+2*sag,layer:a.layer,net:a.net.clone(),locked:false}).collect())
        },
        _=>Err("Choose nonzero tracks or finite circular arcs to drag".into()),
    }
}

/// Exact containment of a straight copper capsule in existing source copper.
fn same_track_coverage(part:&Track,source:&Track)->bool{
    if part.net!=source.net||part.width!=source.width||part.layer!=source.layer||source.a==source.b{return false;}
    let on=|p:Point|{
        let cross=(source.b.x as i128-source.a.x as i128)*(p.y as i128-source.a.y as i128)-(source.b.y as i128-source.a.y as i128)*(p.x as i128-source.a.x as i128);
        cross==0&&p.x>=source.a.x.min(source.b.x)&&p.x<=source.a.x.max(source.b.x)&&p.y>=source.a.y.min(source.b.y)&&p.y<=source.a.y.max(source.b.y)
    };on(part.a)&&on(part.b)
}
