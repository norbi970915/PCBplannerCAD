//! Moving placed footprints while their routing stays connected.
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
    pub ids: Vec<ItemId>,
    pub delta: Point,
    #[serde(default)]
    pub mode: MoveMode,
    pub route_mode: RouteMode,
    pub corner: CornerStyle,
}

/// The drag as a `Route` command: same-ID replacements, removals and new copper.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragResult {
    pub replace: Vec<IdItem>,
    pub delete: Vec<ItemId>,
    pub add: Vec<Item>,
    /// Connections re-routed with the router.
    pub rerouted: usize,
    /// Connections the router could not complete; joined straight instead (DRC will flag them).
    pub blocked: usize,
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
            Item::Via(v) if v.pos == far => return None,
            Item::Track(n) if nid != id && !moving.contains(&nid) && n.layer == t.layer && n.net == t.net && (n.a == far || n.b == far) => neighbours.push((nid, n)),
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

/// Plans a drag of `req.ids` by `req.delta`; nothing is changed in `doc`.
pub fn drag_footprints(doc: &Document, req: &DragRequest) -> DragResult {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let moving: std::collections::HashSet<ItemId> = req.ids.iter().copied().collect();
    let mut work = doc.clone();
    let mut edits: Vec<Edit> = Vec::new();

    // Pads of the moved footprints before the move.
    let mut pads = Vec::new();
    for &id in &req.ids {
        let Some(item) = doc.get(id) else { continue };
        if let Item::Footprint(fp) = item {
            if let Some(geom) = doc.cached_footprint(&fp.footprint) {
                pads.extend(world_pads(id, fp, geom, board));
            }
        }
        if !matches!(item, Item::Board(_)) {
            edits.push(Edit { id, before: Some(item.clone()), after: Some(item.translated(req.delta.x, req.delta.y)) });
        }
    }
    let on_moved_pad = |p: Point, layer: LayerId, net: &str| {
        pads.iter().any(|pad| {
            pad.net == net && pad.copper.contains(&layer) && ((pad.pos.x - p.x) as f64).hypot((pad.pos.y - p.y) as f64) <= pad.width.max(pad.height) as f64 / 2.0
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
            let (on_a, on_b) = (on_moved_pad(t.a, t.layer, &t.net), on_moved_pad(t.b, t.layer, &t.net));
            match (on_a, on_b) {
                (false, false) => {}
                // Between two moved pads: the whole segment moves with them.
                (true, true) => edits.push(Edit { id, before: Some(item.clone()), after: Some(item.translated(req.delta.x, req.delta.y)) }),
                _ => {
                    let (far, near) = if on_a { (t.b, t.a) } else { (t.a, t.b) };
                    let to = shifted(near, req.delta);
                    if req.mode == MoveMode::Free {
                        let moved = if on_a { Track { a: to, ..t.clone() } } else { Track { b: to, ..t.clone() } };
                        edits.push(Edit { id, before: Some(item.clone()), after: Some(Item::Track(moved)) });
                    } else if let Some(s) = slide(doc, &moving, id, t, on_a, req.delta) {
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
        let request = RouteRequest { net: r.net.clone(), layer: r.layer, start: r.from, end: r.to, width: r.width, mode: req.route_mode, corner: req.corner, ignore: vec![], posture_flip: false };
        let mut result = route(&work, &request);
        if !result.reached || result.tracks.is_empty() {
            // Keep the connection: a direct 45° path, reported as blocked.
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
        let step = drag_footprints(&work, &DragRequest { ids: vec![m.id], delta: m.delta, mode: req.mode, route_mode: req.route_mode, corner: req.corner });
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
        Item::Track(Track { a, b, width: mm(0.25), layer: layer::TOP_COPPER, net: net.into() })
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
        DragRequest { ids, delta, mode, route_mode: RouteMode::Shove, corner: CornerStyle::Diagonal45 }
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
    fn a_pad_dropped_onto_another_net_keeps_its_connection_and_reports_the_conflict() {
        // The moved pad lands on a long net-B track: no clean route exists to it.
        let d = doc(vec![track(p(-30.0, -1.0), p(60.0, -1.0), "B")]);
        let r = drag_footprints(&d, &req(vec![id_of(&d, "U2")], p(0.0, -0.6), MoveMode::PreserveAngles));
        assert_eq!((r.rerouted, r.blocked), (0, 1));
        let mut s = Session::with_document(d);
        s.execute(Command::Route { delete: r.delete, add: r.add, replace: r.replace }).unwrap();
        assert!(connected(s.document()), "joined directly instead of left open");
        assert!(copper_violations(s.document()) > 0, "and DRC reports the overlap");
    }
}
