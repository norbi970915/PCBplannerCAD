//! Interactive router: walkaround, push-and-shove, differential pairs and
//! length tuning.
//!
//! Every call builds an obstacle [`world::World`] for the routed layer (an
//! R-tree of every other copper object, grown by its half width; clearances
//! come from [`RuleSet`](super::query::RuleSet) and are memoized per call).
//! There is no cross-call cache: building the index is linear in the board
//! size and takes ~1–3 ms for a few thousand objects in release builds, well
//! inside an interactive mouse-move budget, and it can never go stale.
//!
//! - **Walkaround** ([`search`]): direct corner-style path if clean, otherwise
//!   A* on an implicit grid aligned to the start point (coarse pitch =
//!   half width + clearance, then a 3× finer retry), with direction-aware
//!   states that penalize bends and only allow 45° (or 90°) turns. Edges are
//!   proven clear from cached per-node clearance slack and only tested exactly
//!   near obstacles. The result is shortcut with corner-style connections.
//! - **Shove** ([`shove`]): pushes other-net tracks sideways into 45° detours,
//!   recursively (depth 5); falls back to walkaround.
//! - **Diff pairs**: one corridor (`2w + gap`, plus miter allowance) is routed
//!   along the pair centerline and offset to both sides.
//! - **Tuning** ([`tune`]): one-sided meanders, trombone or accordion.

use serde::{Deserialize, Serialize};

use crate::document::{Document, IdItem, ItemId, LayerId};
use crate::item::Item;
use crate::layer;
use crate::units::{Coord, Point};

use super::items::{Track, Via};
use super::query::{Obj, ObjKind, RuleSet};

#[path = "router_geom.rs"]
mod geom;
#[path = "router_search.rs"]
mod search;
#[path = "router_shove.rs"]
mod shove;
#[path = "router_via.rs"]
mod via_shove;
#[path = "router_tune.rs"]
mod tune;
#[path = "router_world.rs"]
mod world;

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
    /// Shoved other-net tracks. Each entry's `id` is the *original* item it
    /// replaces; one original may map to several entries (a detour), so apply
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
    if req.mode == RouteMode::Shove {
        if let Some(result) = via_shove::route(doc, req) { return result; }
    }
    route_base(doc, req)
}

fn route_base(doc: &Document, req: &RouteRequest) -> RouteResult {
    if req.mode == RouteMode::Ignore {
        let path = corner_path(req.start, req.end, req.corner, req.posture_flip);
        return RouteResult { tracks: tracks_of(&path, req.width, req.layer, &req.net), reached: true, moved: vec![], blocked_at: None, message: String::new() };
    }
    let ignored = routing_ignored(doc, &req.ignore);
    let mut world = World::build(doc, req.layer, &ignored);
    let net = world.net_id(&req.net);
    let excl = [net];
    let probe = Probe { hw: req.width as f64 / 2.0, net, excl: &excl };
    let o = route_in(&mut world, &probe, req.start, req.end, req.mode, req.corner, req.posture_flip);
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
    let w = pos.width;
    let c = (w + gap) as f64;
    let miter = match pos.corner {
        CornerStyle::Diagonal45 | CornerStyle::Arc45 => c * (1.0 / (22.5f64).to_radians().cos() - 1.0),
        CornerStyle::Orthogonal90 | CornerStyle::Any => c * (std::f64::consts::SQRT_2 - 1.0),
    };
    let corridor = (2 * w + gap) as f64 + miter + 4.0;
    let ignored = routing_ignored(doc, &pos.ignore);
    let mut world = World::build(doc, pos.layer, &ignored);
    let pnet = world.net_id(&pos.net);
    let nnet = world.net_id(neg_net);
    let excl = [pnet, nnet];
    let probe = Probe { hw: corridor / 2.0, net: pnet, excl: &excl };

    let direct = corner_path(pos.start, pos.end, pos.corner, pos.posture_flip);
    let fail = |msg: &str| {
        let r = RouteResult { tracks: vec![], reached: false, moved: vec![], blocked_at: Some(pos.start), message: msg.to_string() };
        (r.clone(), r)
    };
    if direct.len() < 2 {
        return fail("start and end coincide");
    }
    let (mut d0, mut d1) = (unit_dir(direct[0], direct[1]), unit_dir(direct[direct.len() - 2], direct[direct.len() - 1]));
    let mut out = None;
    for _ in 0..3 {
        let sc = (V::of(pos.start) + right(d0, c / 2.0)).round();
        let ec = (V::of(pos.end) + right(d1, c / 2.0)).round();
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
        return fail(if o.message.is_empty() { "no room for the pair" } else { &o.message });
    }
    let center = simplify(&o.path);
    let mut ppath = offset_polyline(&center, c / 2.0);
    let mut npath = offset_polyline(&center, -c / 2.0);
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
    let ppath = simplify(&ppath);
    npath = simplify(&npath);
    // Verify both tracks against the (possibly shoved) world.
    let tprobe = Probe { hw: w as f64 / 2.0, net: pnet, excl: &excl };
    let nprobe = Probe { hw: w as f64 / 2.0, net: nnet, excl: &excl };
    let clean = world.path_clear(&ppath, &tprobe) && world.path_clear(&npath, &nprobe);
    let reached = o.reached && (clean || pos.mode == RouteMode::Ignore);
    let mut message = o.message.clone();
    if o.reached && !reached {
        message = "pair tracks violate clearance at a bend".into();
    }
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
