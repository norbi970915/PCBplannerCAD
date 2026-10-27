//! Cleanup Tracks & Vias: removes zero-length, duplicate and dangling track
//! segments and vias that connect at most one layer, and merges collinear
//! segments joined end to end. Planned against the document and committed by
//! the frontend as one `Route` command (copper plus pour refill in one undo).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::document::{Document, IdItem, ItemId, LayerId};
use crate::item::Item;
use crate::units::Point;

use super::connectivity::geo::TOUCH;
use super::connectivity::{copper_objects, pt, settings_or_default, Index};
use super::items::Track;
use super::query::ObjKind;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOptions {
    pub zero_length: bool,
    pub duplicates: bool,
    pub dangling: bool,
    pub unconnected_vias: bool,
    pub merge_collinear: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub delete: Vec<ItemId>,
    pub replace: Vec<IdItem>,
    pub zero_length: usize,
    pub duplicates: usize,
    pub dangling: usize,
    pub vias: usize,
    pub merged: usize,
}

/// Distance in nm below which a point counts as lying on a line.
const ON_LINE: f64 = 2.0;

fn dist_to_line(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return ((p.x - a.x) as f64).hypot((p.y - a.y) as f64);
    }
    (((p.x - a.x) as f64) * dy - ((p.y - a.y) as f64) * dx).abs() / len
}

/// `inner` lies on `outer` (collinear and between its ends).
fn covered_by(inner: &Track, outer: &Track) -> bool {
    let on = |p: Point| {
        let t = ((p.x - outer.a.x) as f64 * (outer.b.x - outer.a.x) as f64 + (p.y - outer.a.y) as f64 * (outer.b.y - outer.a.y) as f64)
            / (((outer.b.x - outer.a.x) as f64).powi(2) + ((outer.b.y - outer.a.y) as f64).powi(2)).max(1.0);
        dist_to_line(p, outer.a, outer.b) <= ON_LINE && (-1e-9..=1.0 + 1e-9).contains(&t)
    };
    on(inner.a) && on(inner.b)
}

pub fn cleanup(doc: &Document, opts: &CleanupOptions) -> CleanupResult {
    let mut out = CleanupResult::default();
    let mut removed: HashSet<ItemId> = HashSet::new();
    let tracks: Vec<(ItemId, &Track)> = doc.items().filter_map(|(id, i)| if let Item::Track(t) = i { if t.locked { None } else { Some((id, t)) } } else { None }).collect();

    if opts.zero_length {
        for &(id, t) in &tracks {
            if t.a == t.b && removed.insert(id) {
                out.zero_length += 1;
            }
        }
    }

    if opts.duplicates {
        // A segment lying on a same-net segment at least as wide adds no copper; of two equal ones the later goes.
        let mut by_line: HashMap<(LayerId, &str), Vec<(ItemId, &Track)>> = HashMap::new();
        for &(id, t) in &tracks {
            if !removed.contains(&id) {
                by_line.entry((t.layer, t.net.as_str())).or_default().push((id, t));
            }
        }
        for group in by_line.values() {
            for &(id, t) in group {
                let redundant = group.iter().any(|&(oid, o)| {
                    oid != id && !removed.contains(&oid) && o.width >= t.width && covered_by(t, o) && (!covered_by(o, t) || o.width > t.width || oid < id)
                });
                if redundant && removed.insert(id) {
                    out.duplicates += 1;
                }
            }
        }
    }

    if opts.dangling || opts.unconnected_vias {
        let board = settings_or_default(doc);
        let idx = Index::new(copper_objects(doc, &board));
        // Removing a stub can strand a via and the other way round: repeat until stable.
        loop {
            let mut changed = false;
            if opts.dangling {
                for (i, o) in idx.objs.iter().enumerate() {
                    if o.kind != ObjKind::Track || removed.contains(&o.item) || doc.get(o.item).is_some_and(crate::command::is_locked) {
                        continue;
                    }
                    let Some(Item::Track(t)) = doc.get(o.item) else { continue };
                    let touched = |p: Point| {
                        let q = pt(p);
                        idx.tree.query(super::connectivity::geo::Bb::of(q, q).grow(TOUCH)).any(|j| {
                            j != i && idx.objs[j].item != o.item && !removed.contains(&idx.objs[j].item) && idx.objs[j].on_layer(t.layer) && idx.geos[j].contains(q)
                        })
                    };
                    if !touched(t.a) || !touched(t.b) {
                        removed.insert(o.item);
                        out.dangling += 1;
                        changed = true;
                    }
                }
            }
            if opts.unconnected_vias {
                for (i, o) in idx.objs.iter().enumerate() {
                    if o.kind != ObjKind::Via || removed.contains(&o.item) || doc.get(o.item).is_some_and(crate::command::is_locked) {
                        continue;
                    }
                    let mut layers: HashSet<LayerId> = HashSet::new();
                    for j in idx.tree.query(idx.geos[i].bb.grow(TOUCH)) {
                        if j != i && !removed.contains(&idx.objs[j].item) && idx.touches(i, j) {
                            layers.extend(o.layers.iter().filter(|l| idx.objs[j].on_layer(**l)));
                        }
                    }
                    if layers.len() <= 1 {
                        removed.insert(o.item);
                        out.vias += 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    if opts.merge_collinear {
        out.merged = merge_collinear(doc, &mut removed, &mut out.replace);
    }

    out.delete = removed.into_iter().collect();
    out.delete.sort();
    out
}

/// Joins pairs of equal tracks meeting end to end in a straight line where nothing else connects.
fn merge_collinear(doc: &Document, removed: &mut HashSet<ItemId>, replace: &mut Vec<IdItem>) -> usize {
    let mut live: HashMap<ItemId, Track> =
        doc.items().filter_map(|(id, i)| if let Item::Track(t) = i { (!removed.contains(&id) && t.a != t.b).then(|| (id, t.clone())) } else { None }).collect();
    // Points where vias or pads sit: a joint there is a real connection.
    let mut fixed: Vec<(Point, crate::units::Coord, Vec<LayerId>)> = Vec::new();
    let board = settings_or_default(doc);
    for o in copper_objects(doc, &board) {
        if matches!(o.kind, ObjKind::Via | ObjKind::Pad) && !removed.contains(&o.item) {
            let g = o.geo();
            fixed.push((o.pos, ((g.bb.max[0] - g.bb.min[0]).max(g.bb.max[1] - g.bb.min[1]) / 2.0) as i64, o.layers.clone()));
        }
    }
    let on_fixed = |p: Point, l: LayerId| fixed.iter().any(|(c, r, ls)| ls.contains(&l) && ((c.x - p.x) as f64).hypot((c.y - p.y) as f64) <= *r as f64);
    let mut merged = 0;
    let mut touched: HashSet<ItemId> = HashSet::new();
    loop {
        let mut ends: HashMap<(LayerId, Point), Vec<ItemId>> = HashMap::new();
        let mut ids: Vec<&ItemId> = live.keys().collect();
        ids.sort();
        for &id in &ids {
            let t = &live[id];
            ends.entry((t.layer, t.a)).or_default().push(*id);
            ends.entry((t.layer, t.b)).or_default().push(*id);
        }
        let mut joints: Vec<(&(LayerId, Point), &Vec<ItemId>)> = ends.iter().filter(|(_, v)| v.len() == 2 && v[0] != v[1]).collect();
        joints.sort_by_key(|(k, _)| (k.0, k.1.x, k.1.y));
        let mut step = None;
        for (&(layer, p), v) in joints {
            let (t1, t2) = (&live[&v[0]], &live[&v[1]]);
            if t1.net != t2.net || t1.width != t2.width || on_fixed(p, layer) {
                continue;
            }
            let far1 = if t1.a == p { t1.b } else { t1.a };
            let far2 = if t2.a == p { t2.b } else { t2.a };
            let dot = (far1.x - p.x) as f64 * (far2.x - p.x) as f64 + (far1.y - p.y) as f64 * (far2.y - p.y) as f64;
            let opposite = dot < 0.0;
            if opposite && dist_to_line(p, far1, far2) <= ON_LINE {
                step = Some((v[0], v[1], Track { a: far1, b: far2, ..t1.clone() }));
                break;
            }
        }
        let Some((keep, drop, joined)) = step else { break };
        live.remove(&drop);
        live.insert(keep, joined);
        touched.remove(&drop);
        touched.insert(keep);
        removed.insert(drop);
        merged += 1;
    }
    let mut kept: Vec<ItemId> = touched.into_iter().collect();
    kept.sort();
    replace.extend(kept.into_iter().map(|id| IdItem { id, item: Item::Track(live.remove(&id).unwrap()) }));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer;
    use crate::pcb::connectivity::fixtures::{doc, fp, p, track, via};
    use crate::{Command, Session};

    const ALL: CleanupOptions = CleanupOptions { zero_length: true, duplicates: true, dangling: true, unconnected_vias: true, merge_collinear: true };

    fn apply(d: &Document, r: &CleanupResult) -> Document {
        let mut s = Session::with_document(d.clone());
        s.execute(Command::Route { delete: r.delete.clone(), add: vec![], replace: r.replace.clone() }).unwrap();
        s.document().clone()
    }

    fn tracks(d: &Document) -> Vec<Track> {
        d.items().filter_map(|(_, i)| if let Item::Track(t) = i { Some(t.clone()) } else { None }).collect()
    }

    /// R1 pad 2 at (0,0) → R2 pad 1 at (20,0) on net A, routed in three collinear pieces.
    fn routed(extra: Vec<Item>) -> Document {
        let mut items = vec![
            fp("R1", "L/R", -0.8, 0.0, &[("2", "A")]),
            fp("R2", "L/R", 20.8, 0.0, &[("1", "A")]),
            track((0.0, 0.0), (5.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((5.0, 0.0), (12.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((12.0, 0.0), (20.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
        ];
        items.extend(extra);
        doc(items)
    }

    #[test]
    fn collinear_pieces_merge_into_one_segment_and_stay_connected() {
        let d = routed(vec![]);
        let r = cleanup(&d, &ALL);
        assert_eq!((r.merged, r.dangling, r.duplicates), (2, 0, 0));
        let after = apply(&d, &r);
        let t = tracks(&after);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].a, t[0].b), (p(0.0, 0.0), p(20.0, 0.0)));
        assert_eq!(crate::pcb::connectivity::stats(&after).unrouted, 0);
    }

    #[test]
    fn a_bend_or_a_via_keeps_the_joint() {
        let d = doc(vec![
            fp("R1", "L/R", -0.8, 0.0, &[("2", "A")]),
            fp("R2", "L/R", 10.8, 5.0, &[("1", "A")]),
            track((0.0, 0.0), (5.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((5.0, 0.0), (10.0, 5.0), 0.25, layer::TOP_COPPER, "A"),
        ]);
        assert_eq!(cleanup(&d, &ALL).merged, 0);
        let mut items = vec![via(12.0, 0.0, "A"), track((12.0, 0.0), (12.0, 4.0), 0.25, layer::BOTTOM_COPPER, "A")];
        items.extend([track((12.0, 4.0), (20.0, 4.0), 0.25, layer::BOTTOM_COPPER, "A")]);
        let d = routed(items);
        let r = cleanup(&d, &CleanupOptions { merge_collinear: true, ..NONE });
        // (5,0) merges; (12,0) holds a via.
        assert_eq!(r.merged, 1);
    }

    const NONE: CleanupOptions = CleanupOptions { zero_length: false, duplicates: false, dangling: false, unconnected_vias: false, merge_collinear: false };

    #[test]
    fn dangling_stubs_go_including_chains_and_their_lonely_via() {
        // A stub off the joint at (5,0) running to a via with nothing on the bottom, then a chain to nowhere.
        let d = routed(vec![
            track((5.0, 0.0), (5.0, 5.0), 0.25, layer::TOP_COPPER, "A"),
            via(5.0, 5.0, "A"),
            track((30.0, 0.0), (35.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((35.0, 0.0), (35.0, 5.0), 0.25, layer::TOP_COPPER, "A"),
        ]);
        let r = cleanup(&d, &CleanupOptions { dangling: true, unconnected_vias: true, ..NONE });
        assert_eq!((r.dangling, r.vias), (3, 1));
        let after = apply(&d, &r);
        assert_eq!(tracks(&after).len(), 3);
        assert_eq!(crate::pcb::connectivity::stats(&after).unrouted, 0);
    }

    #[test]
    fn duplicates_and_zero_length_segments_are_removed() {
        let d = routed(vec![
            track((5.0, 0.0), (12.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((6.0, 0.0), (8.0, 0.0), 0.2, layer::TOP_COPPER, "A"),
            track((8.0, 0.0), (8.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
        ]);
        let r = cleanup(&d, &CleanupOptions { zero_length: true, duplicates: true, ..NONE });
        assert_eq!((r.zero_length, r.duplicates), (1, 2));
        assert_eq!(tracks(&apply(&d, &r)).len(), 3);
    }

    #[test]
    fn a_via_joining_two_layers_is_kept() {
        let d = doc(vec![
            fp("R1", "L/R", -0.8, 0.0, &[("2", "A")]),
            fp("J1", "L/TH", 10.0, 0.0, &[("1", "A")]),
            track((0.0, 0.0), (5.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            via(5.0, 0.0, "A"),
            track((5.0, 0.0), (10.0, 0.0), 0.25, layer::BOTTOM_COPPER, "A"),
        ]);
        let r = cleanup(&d, &ALL);
        assert_eq!((r.vias, r.dangling, r.merged), (0, 0, 0), "{r:?}");
    }
}
