//! Teardrops: flared copper where a track meets a pad or via, so the joint
//! survives drill wander and thermal stress. They are zones flagged
//! `teardrop` (solid connection, top priority), so they fill around other nets,
//! connect, and reach the Gerbers like any pour. Regenerating replaces all of
//! them; the frontend commits the plan as one `Route` command (with refill).

use serde::{Deserialize, Serialize};

use crate::document::{Document, ItemId};
use crate::item::{Item, PadKind, PadShape};
use crate::units::{Coord, Point};

use super::board::BoardSettings;
use super::geometry::{board_settings, world_pads};
use super::items::{Zone, ZoneConnect};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardropOptions {
    pub vias: bool,
    pub pth_pads: bool,
    pub smd_pads: bool,
    /// Length beyond the pad edge, as a fraction of the pad size.
    pub length_ratio: f64,
    /// Width at the pad, as a fraction of the pad size.
    pub width_ratio: f64,
    pub max_length: Coord,
    pub max_width: Coord,
    /// Concave flanks for curved joints instead of straight ones.
    pub curved: bool,
    /// No teardrop when the track is at least this fraction of the pad size.
    pub max_track_ratio: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardropPlan {
    /// Existing teardrops, replaced by `add`.
    pub delete: Vec<ItemId>,
    pub add: Vec<Item>,
    pub vias: usize,
    pub pads: usize,
}

/// Priority above any ordinary pour, so other nets' pours clear around teardrops.
pub const TEARDROP_PRIORITY: i32 = 1_000_000;

/// Something a track can end on: centre, size across, layers and net.
struct Anchor {
    center: Point,
    size: f64,
    net: String,
    on: Box<dyn Fn(crate::document::LayerId) -> bool>,
    reach: f64,
    via: bool,
}

fn inside(p: Point, ring: &[Point]) -> bool {
    let (x, y) = (p.x as f64, p.y as f64);
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (ax, ay, bx, by) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64);
        if (ay > y) != (by > y) && x < ax + (y - ay) * (bx - ax) / (by - ay) {
            c = !c;
        }
    }
    c
}

/// Teardrops whose joint is gone: no same-net track on their layer ends inside them, or no
/// same-net pad or via sits there any more (unrouted, deleted or moved copper). Only teardrops
/// meeting `near` (min, max) are checked; `None` checks all.
pub fn stale(doc: &Document, near: Option<(Point, Point)>) -> Vec<ItemId> {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let mut out = Vec::new();
    let bbox = |pts: &[Point]| {
        let (mut lo, mut hi) = (Point::new(Coord::MAX, Coord::MAX), Point::new(Coord::MIN, Coord::MIN));
        for p in pts {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (lo, hi)
    };
    let within = |p: Point, (lo, hi): (Point, Point)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    for (id, item) in doc.items() {
        let Item::Zone(z) = item else { continue };
        if !z.teardrop || z.locked || z.outline.len() < 3 {
            continue;
        }
        let bb = bbox(&z.outline);
        if let Some((lo, hi)) = near {
            if bb.1.x < lo.x || bb.0.x > hi.x || bb.1.y < lo.y || bb.0.y > hi.y {
                continue;
            }
        }
        let hit = |p: Point| within(p, bb) && inside(p, &z.outline);
        let tracked = doc.items().any(|(_, i)| matches!(i, Item::Track(t) if t.net == z.net && t.layer == z.layer && (hit(t.a) || hit(t.b))));
        let anchored = tracked
            && doc.items().any(|(fid, i)| match i {
                Item::Via(v) => v.net == z.net && board.stackup.span(v.from_layer, v.to_layer).contains(&z.layer) && hit(v.pos),
                Item::Footprint(fp) => doc.cached_footprint(&fp.footprint).is_some_and(|g| {
                    world_pads(fid, fp, g, board).iter().any(|p| p.net == z.net && p.copper.contains(&z.layer) && hit(p.pos))
                }),
                _ => false,
            });
        if !anchored {
            out.push(id);
        }
    }
    out
}

/// Extent of the copper an edit list touches (before and after), for [`stale`].
pub fn touched_extent(doc_before: &Document, edits: &[crate::document::Edit]) -> Option<(Point, Point)> {
    let mut pts: Vec<Point> = Vec::new();
    let fallback = BoardSettings::default();
    let board = board_settings(doc_before).map_or(&fallback, |(_, b)| b);
    for e in edits {
        for it in [&e.before, &e.after].into_iter().flatten() {
            match it {
                Item::Track(t) => pts.extend([t.a, t.b]),
                Item::Via(v) => pts.push(v.pos),
                Item::Footprint(fp) => match doc_before.cached_footprint(&fp.footprint) {
                    Some(g) => pts.extend(world_pads(e.id, fp, g, board).iter().map(|p| p.pos)),
                    None => pts.push(fp.pos),
                },
                _ => {}
            }
        }
    }
    if pts.is_empty() {
        return None;
    }
    // Teardrops reach at most a few millimetres past their joint.
    let m: Coord = 5_000_000;
    let lo = Point::new(pts.iter().map(|p| p.x).min()? - m, pts.iter().map(|p| p.y).min()? - m);
    let hi = Point::new(pts.iter().map(|p| p.x).max()? + m, pts.iter().map(|p| p.y).max()? + m);
    Some((lo, hi))
}

/// Deletes the teardrops an applied edit list left stale; returns the deletions (already applied).
pub fn prune(doc: &mut Document, edits: &[crate::document::Edit]) -> Vec<crate::document::Edit> {
    if !doc.items().any(|(_, i)| matches!(i, Item::Zone(z) if z.teardrop)) {
        return vec![];
    }
    let Some(near) = touched_extent(doc, edits) else { return vec![] };
    let del: Vec<crate::document::Edit> =
        stale(doc, Some(near)).into_iter().map(|id| crate::document::Edit { id, before: doc.get(id).cloned(), after: None }).collect();
    doc.apply(&del);
    del
}

/// Existing teardrop zones.
pub fn teardrops(doc: &Document) -> Vec<ItemId> {
    doc.items().filter(|(_, i)| matches!(i, Item::Zone(z) if z.teardrop)).map(|(id, _)| id).collect()
}

fn quad(a: (f64, f64), c: (f64, f64), b: (f64, f64), steps: usize) -> Vec<(f64, f64)> {
    (1..steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            (u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1)
        })
        .collect()
}

/// The teardrop outline for a track of `width` leaving `center` along `dir` (unit), `room` long.
fn outline(center: Point, dir: (f64, f64), width: f64, size: f64, room: f64, o: &TeardropOptions) -> Option<Vec<Point>> {
    let w_pad = (size * o.width_ratio).min(o.max_width as f64);
    if width >= w_pad || width >= size * o.max_track_ratio {
        return None;
    }
    let length = (size * o.length_ratio).min(o.max_length as f64);
    let reach = (size / 2.0 + length).min(room - width / 2.0);
    if reach <= size / 2.0 + width / 4.0 {
        return None;
    }
    let c = (center.x as f64, center.y as f64);
    let n = (-dir.1, dir.0);
    let at = |along: f64, side: f64| (c.0 + dir.0 * along + n.0 * side, c.1 + dir.1 * along + n.1 * side);
    let mut ring = Vec::new();
    for s in [1.0, -1.0] {
        let a = at(0.0, s * w_pad / 2.0);
        let b = at(reach, s * width / 2.0);
        let mut side = vec![a];
        if o.curved {
            // Control point near the pad edge at the track's width: a concave flare.
            side.extend(quad(a, at(size / 2.0, s * width / 2.0), b, 8));
        }
        side.push(b);
        if s < 0.0 {
            side.reverse();
        }
        ring.push(side);
    }
    let (plus, minus) = (ring.remove(0), ring.remove(0));
    // Pad side, flank, track end, other flank back to the pad (behind the centre, inside the pad).
    let mut pts: Vec<Point> = std::iter::once(at(-size / 4.0, 0.0)).chain(plus).chain(minus).map(|p| Point::new(p.0.round() as Coord, p.1.round() as Coord)).collect();
    pts.dedup();
    Some(pts)
}

pub fn plan(doc: &Document, o: &TeardropOptions) -> TeardropPlan {
    plan_near(doc, o, None)
}

fn plan_near(doc: &Document, o: &TeardropOptions, near: Option<(Point, Point)>) -> TeardropPlan {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let mut anchors: Vec<Anchor> = Vec::new();
    for (id, item) in doc.items() {
        match item {
            Item::Via(v) if o.vias && !v.net.is_empty() => {
                let span = board.stackup.span(v.from_layer, v.to_layer);
                anchors.push(Anchor { center: v.pos, size: v.diameter as f64, net: v.net.clone(), on: Box::new(move |l| span.contains(&l)), reach: v.diameter as f64 / 2.0, via: true });
            }
            Item::Footprint(fp) => {
                let Some(geom) = doc.cached_footprint(&fp.footprint) else { continue };
                for physical in world_pads(id, fp, geom, board) {
                    let variants=if physical.layer_shapes.is_empty(){vec![physical]}else{physical.copper.iter().map(|l|{let mut p=physical.for_layer(*l);p.copper=vec![*l];p}).collect()};
                    for pad in variants {
                    let smd = pad.kind == PadKind::Smd;
                    if pad.net.is_empty() || pad.kind == PadKind::Npth || (smd && !o.smd_pads) || (!smd && !o.pth_pads) {
                        continue;
                    }
                    // Teardrops suit round-ish pads; on long pads they would only cover the narrow side.
                    let (w, h) = (pad.width as f64, pad.height as f64);
                    if pad.shape != PadShape::Circle && w.max(h) > 1.5 * w.min(h) {
                        continue;
                    }
                    let copper = pad.copper.clone();
                    anchors.push(Anchor { center: pad.pos, size: w.min(h), net: pad.net.clone(), on: Box::new(move |l| copper.contains(&l)), reach: w.min(h) / 2.0, via: false });
                    }
                }
            }
            _ => {}
        }
    }

    let within = |p: Point| near.is_none_or(|(lo, hi)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y);
    anchors.retain(|a| within(a.center));
    let mut out = TeardropPlan { delete: teardrops(doc).into_iter().filter(|id| matches!(doc.get(*id), Some(Item::Zone(z)) if !z.locked && z.outline.iter().any(|p| within(*p)))).collect(), ..TeardropPlan::default() };
    for (_, item) in doc.items() {
        let Item::Track(t) = item else { continue };
        for (end, other) in [(t.a, t.b), (t.b, t.a)] {
            let Some(anchor) = anchors.iter().find(|a| a.net == t.net && (a.on)(t.layer) && ((end.x - a.center.x) as f64).hypot((end.y - a.center.y) as f64) <= a.reach) else { continue };
            if doc.items().any(|(_, i)| matches!(i, Item::Zone(z) if z.teardrop && z.locked && z.net == t.net && z.layer == t.layer && inside(anchor.center, &z.outline))) { continue; }
            let (dx, dy) = ((other.x - anchor.center.x) as f64, (other.y - anchor.center.y) as f64);
            let room = dx.hypot(dy);
            if room <= 0.0 {
                continue;
            }
            let Some(outline) = outline(anchor.center, (dx / room, dy / room), t.width as f64, anchor.size, room, o) else { continue };
            if anchor.via {
                out.vias += 1;
            } else {
                out.pads += 1;
            }
            out.add.push(Item::Zone(Zone { locked: false,
                outline,
                layer: t.layer,
                net: t.net.clone(),
                name: "teardrop".into(),
                priority: TEARDROP_PRIORITY,
                clearance: 0,
                min_width: 0,
                connect: ZoneConnect::Solid,
                thermal_gap: 0,
                spoke_width: 0,
                keepout: false,
                remove_islands: true,
                fill: vec![],
                teardrop: true,
            }));
        }
    }
    out
}

/// Regenerate only joints near edited copper, preserving unchanged zone IDs.
pub fn maintain(doc: &mut Document, changes: &[crate::document::Edit]) -> Vec<crate::document::Edit> {
    use crate::document::Edit;
    let Some(options) = board_settings(doc).and_then(|(_, b)| b.auto_teardrops) else { return vec![] };
    let settings_changed = changes.iter().any(|e| match (&e.before, &e.after) {
        (Some(Item::Board(a)), Some(Item::Board(b))) => a.auto_teardrops != b.auto_teardrops,
        (_, Some(Item::Board(b))) => b.auto_teardrops.is_some(),
        _ => false,
    });
    let near = if settings_changed { None } else {
        let Some(bounds) = touched_extent(doc, changes) else { return vec![] };
        Some(bounds)
    };
    let plan = plan_near(doc, &options, near);
    let mut remaining = plan.delete;
    let mut edits = Vec::new();
    for item in plan.add {
        if let Some(index) = remaining.iter().position(|id| match (doc.get(*id), &item) {
            (Some(Item::Zone(a)), Item::Zone(b)) => a.outline == b.outline && a.net == b.net && a.layer == b.layer,
            _ => false,
        }) { remaining.remove(index); continue; }
        edits.push(Edit { id: doc.alloc_id(), before: None, after: Some(item) });
    }
    edits.extend(remaining.into_iter().map(|id| Edit { id, before: doc.get(id).cloned(), after: None }));
    if edits.is_empty() { return edits; }
    doc.apply(&edits);
    for (id, fill) in super::fill::fill_all(doc) {
        if let Some(Item::Zone(zone)) = doc.get(id) {
            if zone.fill == fill { continue; }
            let mut updated = zone.clone(); updated.fill = fill;
            edits.push(Edit { id, before: Some(Item::Zone(zone.clone())), after: Some(Item::Zone(updated)) });
        }
    }
    doc.apply(&edits);
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{BOTTOM_COPPER, TOP_COPPER};
    use crate::pcb::connectivity::fixtures::{doc, fp, track, via};
    use crate::units::mm;
    use crate::{Command, Session};

    const OPTS: TeardropOptions = TeardropOptions {
        vias: true,
        pth_pads: true,
        smd_pads: true,
        length_ratio: 0.5,
        width_ratio: 1.0,
        max_length: 1_000_000,
        max_width: 2_000_000,
        curved: true,
        max_track_ratio: 0.9,
    };

    fn commit(d: &Document, p: &TeardropPlan) -> Document {
        let mut s = Session::with_document(d.clone());
        s.execute(Command::Route { delete: p.delete.clone(), add: p.add.clone(), replace: vec![] }).unwrap();
        s.document().clone()
    }

    #[test]
    fn a_track_leaving_a_via_and_a_round_pad_gets_one_filled_teardrop_each() {
        // J1 (1.6 mm THT) at (0,0) → track → via (0.6 mm) at (10,0).
        let d = doc(vec![
            fp("J1", "L/TH", 0.0, 0.0, &[("1", "A")]),
            track((0.0, 0.0), (10.0, 0.0), 0.2, TOP_COPPER, "A"),
            via(10.0, 0.0, "A"),
        ]);
        let p = plan(&d, &OPTS);
        assert_eq!((p.pads, p.vias, p.add.len()), (1, 1, 2));
        let Item::Zone(z) = &p.add[0] else { panic!() };
        assert!(z.teardrop && z.connect == ZoneConnect::Solid && z.priority == TEARDROP_PRIORITY);
        // Pad teardrop reaches 0.8 mm (edge) + 0.8 mm (50 % of 1.6 mm) along +x, 1.6 mm wide at the pad.
        let max_x = z.outline.iter().map(|q| q.x).max().unwrap();
        assert!((max_x - mm(1.6)).abs() < 2, "{max_x}");
        let half = z.outline.iter().map(|q| q.y.abs()).max().unwrap();
        assert!((half - mm(0.8)).abs() < 2, "{half}");

        let after = commit(&d, &p);
        let fills: Vec<_> = after.items().filter_map(|(_, i)| if let Item::Zone(z) = i { Some(z) } else { None }).collect();
        assert_eq!(fills.len(), 2);
        assert!(fills.iter().all(|z| !z.fill.is_empty()), "teardrops are filled on commit");
        assert_eq!(crate::pcb::connectivity::stats(&after).unrouted, 0);
        // Regenerating replaces them rather than stacking.
        let again = plan(&after, &OPTS);
        assert_eq!((again.delete.len(), again.add.len()), (2, 2));
    }

    #[test]
    fn teardrops_go_with_their_track_in_the_same_undo_step() {
        let d = doc(vec![
            fp("J1", "L/TH", 0.0, 0.0, &[("1", "A")]),
            track((0.0, 0.0), (10.0, 0.0), 0.2, TOP_COPPER, "A"),
            via(10.0, 0.0, "A"),
            fp("J2", "L/TH", 0.0, 10.0, &[("1", "B")]),
            track((0.0, 10.0), (10.0, 10.0), 0.2, TOP_COPPER, "B"),
        ]);
        let mut s = Session::with_document(d.clone());
        s.execute(Command::Route { delete: vec![], add: plan(&d, &OPTS).add, replace: vec![] }).unwrap();
        let count = |s: &Session| s.document().items().filter(|(_, i)| matches!(i, Item::Zone(z) if z.teardrop)).count();
        assert_eq!(count(&s), 3, "J1 pad, the via, J2 pad");
        let track_a = s.document().items().find(|(_, i)| matches!(i, Item::Track(t) if t.net == "A")).unwrap().0;
        // Unroute (a Route command) and a plain Delete both take the joint's teardrops along.
        s.execute(Command::Route { delete: vec![track_a], add: vec![], replace: vec![] }).unwrap();
        assert_eq!(count(&s), 1, "only net B's teardrop is left");
        s.undo();
        assert_eq!(count(&s), 3);
        s.execute(Command::Delete { ids: vec![track_a] }).unwrap();
        assert_eq!(count(&s), 1);
        // Moving the pad away from its track end strands that teardrop too.
        let j2 = s.document().items().find(|(_, i)| matches!(i, Item::Footprint(f) if f.designator == "J2")).unwrap().0;
        s.execute(Command::Move { ids: vec![j2], dx: mm(3.0), dy: 0 }).unwrap();
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn options_wide_tracks_other_layers_and_long_pads_are_respected() {
        let d = doc(vec![
            fp("R1", "L/R", 0.0, 0.0, &[("1", "A")]),
            track((-0.8, 0.0), (-5.0, 0.0), 0.2, TOP_COPPER, "A"),
            via(10.0, 0.0, "B"),
            track((10.0, 0.0), (15.0, 0.0), 0.55, BOTTOM_COPPER, "B"),
            track((10.0, 0.0), (10.0, 5.0), 0.2, BOTTOM_COPPER, "B"),
        ]);
        // R1's 0.8 × 0.9 pad is near square: it gets one; the 0.55 mm track is over 90 % of the 0.6 mm via.
        let p = plan(&d, &OPTS);
        assert_eq!((p.pads, p.vias), (1, 1));
        let p = plan(&d, &TeardropOptions { smd_pads: false, vias: false, ..OPTS });
        assert!(p.add.is_empty());
        let Item::Zone(z) = &plan(&d, &OPTS).add[1] else { panic!() };
        assert_eq!(z.layer, BOTTOM_COPPER);
    }
}
