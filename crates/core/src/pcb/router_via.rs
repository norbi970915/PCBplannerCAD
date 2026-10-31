//! Bounded via displacement with physical contact preservation on every layer.
use super::geom::{V, corner_path, seg_seg_dist};
use super::{RouteRequest, RouteResult};
use crate::document::{Document, Edit, IdItem};
use crate::item::Item;
use crate::pcb::board::BoardSettings;
use crate::pcb::geometry::board_settings;
use crate::units::Point;
use std::collections::{HashMap, HashSet};

/// Indexed rejection precedes expensive speculative board copies and full DRC.
fn copper_clear(
    index: &crate::pcb::connectivity::Index,
    board: &BoardSettings,
    rules: &crate::pcb::query::RuleSet<'_>,
    replacements: &[IdItem],
) -> bool {
    let moving: HashSet<_> = replacements.iter().map(|entry| entry.id).collect();
    let sample = Document::from_item_list(
        crate::DocKind::Pcb,
        replacements.iter().map(|entry| entry.item.clone()),
    );
    for object in crate::pcb::connectivity::build_objects(&sample, board, None, None) {
        let geometry = object.geo();
        for other in index
            .tree
            .query(geometry.bb.grow(rules.max_clearance() as f64 + 5.))
        {
            let fixed = &index.objs[other];
            if moving.contains(&fixed.item)
                || (!object.net.is_empty() && object.net == fixed.net)
                || !object.shares_layer(fixed)
            {
                continue;
            }
            for layer in object.layers.iter().filter(|layer| fixed.on_layer(**layer)) {
                let clearance =
                    rules.clearance(&object.obj(Some(*layer)), &fixed.obj(Some(*layer)));
                if crate::pcb::connectivity::geo::gap(
                    &geometry,
                    &index.geos[other],
                    clearance as f64 + 5.,
                )
                .is_some_and(|(gap, _)| gap < clearance as f64 - 5.)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn connected(before: &crate::pcb::connectivity::Connectivity, work: &Document) -> bool {
    let after = crate::pcb::elastic::net_connectivity(work);
    let mut groups = HashMap::new();
    for object in before.objects().iter().filter(|o| !o.net.is_empty()) {
        let Some(old) = before.cluster_of(object.item, object.pad.as_deref()) else {
            continue;
        };
        let Some(now) = after.cluster_of(object.item, object.pad.as_deref()) else {
            return false;
        };
        if groups
            .insert((old, object.net.as_str()), now)
            .is_some_and(|previous| previous != now)
        {
            return false;
        }
    }
    true
}

fn closest(t: &crate::pcb::Track, point: Point) -> Point {
    let a = V::of(t.a);
    let edge = V::of(t.b) - a;
    let len2 = edge.dot(edge);
    if len2 == 0. {
        return t.a;
    }
    (a + edge * ((V::of(point) - a).dot(edge) / len2).clamp(0., 1.)).round()
}

pub(super) fn route(doc: &Document, req: &RouteRequest) -> Option<RouteResult> {
    // A rejected via candidate must leave allowance for ordinary routing.
    let _work = super::engine::WorkSlice::reserving(256);
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_, b)| b);
    let path = corner_path(req.start, req.end, req.corner, req.posture_flip);
    let rules = crate::pcb::query::RuleSet::new(board);
    let mut attempted = 0;
    let mut index = None;
    let mut connectivity = None;
    for (id, item) in doc.items() {
        let Item::Via(via) = item else {
            continue;
        };
        let span = board.stackup.span(via.from_layer, via.to_layer);
        if via.locked
            || via.net.is_empty()
            || via.net == req.net
            || req.ignore.contains(&id)
            || !span.contains(&req.layer)
        {
            continue;
        }
        let clearance = rules.clearance(
            &crate::pcb::query::Obj::new(
                crate::pcb::query::ObjKind::Via,
                &via.net,
                Some(req.layer),
            ),
            &crate::pcb::query::Obj::new(
                crate::pcb::query::ObjKind::Track,
                &req.net,
                Some(req.layer),
            ),
        );
        let radius = (via.diameter + req.width) as f64 / 2. + clearance as f64;
        if !path.windows(2).any(|w| {
            seg_seg_dist(V::of(w[0]), V::of(w[1]), V::of(via.pos), V::of(via.pos)) < radius
        }) {
            continue;
        }
        attempted += 1;
        if attempted > 8 || super::engine::work_exhausted() {
            break;
        }
        let index = index.get_or_insert_with(|| super::engine::cached_copper(doc));
        let Some(vi) = index.objs.iter().position(|o| o.item == id) else {
            continue;
        };
        let contacts: Vec<_> = index
            .tree
            .query(index.geos[vi].bb.grow(crate::pcb::connectivity::geo::TOUCH))
            .into_iter()
            .filter(|other| *other != vi && index.touches(vi, *other))
            .collect();
        // A physically touching pad, curve, via stack or locked track anchors the
        // via even when the contact is away from either object's centerline ends.
        if contacts.iter().any(|other| {
            matches!(
                doc.get(index.objs[*other].item),
                Some(Item::Footprint(_) | Item::ArcTrack(_) | Item::Via(_))
            ) || matches!(doc.get(index.objs[*other].item),Some(Item::Track(t))if t.locked)
        }) {
            continue;
        }
        let attached: Vec<_> = contacts
            .iter()
            .filter_map(|other| {
                let fid = index.objs[*other].item;
                match doc.get(fid) {
                    Some(Item::Track(t)) if t.net == via.net => Some((fid, t.clone())),
                    _ => None,
                }
            })
            .collect();
        let step = radius.ceil() as i64 + 10_000;
        for (dx, dy) in [
            (0, step),
            (0, -step),
            (step, 0),
            (-step, 0),
            (step, step),
            (-step, -step),
            (step, -step),
            (-step, step),
        ] {
            let pos = Point::new(via.pos.x + dx, via.pos.y + dy);
            for keep_tracks in [false, true] {
                if !super::engine::consume_work(index.objs.len().div_ceil(32).max(1)) {
                    return None;
                }
                let mut moved = via.clone();
                moved.pos = pos;
                let mut replacements = vec![IdItem {
                    id,
                    item: Item::Via(moved),
                }];
                let mut tethers = Vec::new();
                for (tid, t) in &attached {
                    if !keep_tracks && (t.a == via.pos || t.b == via.pos) {
                        let mut shifted = t.clone();
                        if shifted.a == via.pos {
                            shifted.a = pos;
                        }
                        if shifted.b == via.pos {
                            shifted.b = pos;
                        }
                        replacements.push(IdItem {
                            id: *tid,
                            item: Item::Track(shifted),
                        });
                    } else {
                        let from = closest(t, via.pos);
                        for pair in corner_path(from, pos, req.corner, req.posture_flip).windows(2)
                        {
                            if pair[0] != pair[1] {
                                tethers.push(Item::Track(crate::pcb::Track {
                                    a: pair[0],
                                    b: pair[1],
                                    ..t.clone()
                                }));
                            }
                        }
                    }
                }
                let mut proposed = replacements.clone();
                proposed.extend(tethers.iter().cloned().map(|item| IdItem { id, item }));
                if !copper_clear(index, board, &rules, &proposed) {
                    continue;
                }
                let mut work = doc.clone();
                let edits: Vec<_> = replacements
                    .iter()
                    .map(|r| Edit {
                        id: r.id,
                        before: work.get(r.id).cloned(),
                        after: Some(r.item.clone()),
                    })
                    .collect();
                work.apply(&edits);
                let mut touched: Vec<_> = replacements.iter().map(|r| r.id).collect();
                // Temporary IDs are mapped back to the source via in the result
                // protocol; they must never escape as source deletion requests.
                let mut new = Vec::new();
                for item in tethers {
                    let tid = work.alloc_id();
                    work.apply(&[Edit {
                        id: tid,
                        before: None,
                        after: Some(item.clone()),
                    }]);
                    touched.push(tid);
                    new.push(IdItem { id: tid, item });
                }
                if crate::pcb::drc::run_near(&work, &touched)
                    .iter()
                    .any(|v| v.severity == crate::pcb::drc::Severity::Error)
                {
                    continue;
                }
                let before =
                    connectivity.get_or_insert_with(|| crate::pcb::elastic::net_connectivity(doc));
                if !connected(before, &work) {
                    continue;
                }
                let mut routed = super::route_base(&work, req);
                if !routed.reached {
                    continue;
                }
                let shoved: HashSet<_> = routed.moved.iter().map(|r| r.id).collect();
                // Keep one stable identity per displaced source for the final
                // connectivity check, then allocate the remaining pieces locally.
                let mut used = HashSet::new();
                for r in &routed.moved {
                    let tid = if used.insert(r.id) {
                        r.id
                    } else {
                        work.alloc_id()
                    };
                    let before = work.get(tid).cloned();
                    work.apply(&[Edit {
                        id: tid,
                        before,
                        after: Some(r.item.clone()),
                    }]);
                    touched.push(tid);
                }
                for track in &routed.tracks {
                    let tid = work.alloc_id();
                    work.apply(&[Edit {
                        id: tid,
                        before: None,
                        after: Some(Item::Track(track.clone())),
                    }]);
                    touched.push(tid);
                }
                if !connected(before, &work)
                    || crate::pcb::drc::run_near(&work, &touched)
                        .iter()
                        .any(|v| v.severity == crate::pcb::drc::Severity::Error)
                {
                    continue;
                }
                replacements.retain(|r| !shoved.contains(&r.id));
                let temporary: HashSet<_> = new.iter().map(|r| r.id).collect();
                for entry in &mut routed.moved {
                    if temporary.contains(&entry.id) {
                        entry.id = id;
                    }
                }
                routed.moved.extend(replacements);
                routed.moved.extend(
                    new.into_iter()
                        .filter(|r| !shoved.contains(&r.id))
                        .map(|r| IdItem { id, item: r.item }),
                );
                return Some(routed);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "router_via_tests.rs"]
mod tests;
