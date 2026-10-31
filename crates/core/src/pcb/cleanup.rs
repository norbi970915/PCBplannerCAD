//! Cleanup Tracks & Vias: removes zero-length, duplicate and dangling track
//! segments and vias that connect at most one layer, and merges collinear
//! segments joined end to end. Free tails are trimmed without changing heading.
//! Reviewed Apply replans before one Route commit and dependent pour refill.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::document::{Document, IdItem, ItemId, LayerId};
use crate::item::Item;
use crate::units::Point;

use super::connectivity::geo::TOUCH;
use super::connectivity::{Index, copper_objects, settings_or_default};
use super::items::Track;
use super::query::ObjKind;
#[path = "cleanup_contacts.rs"]
mod contacts;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOptions {
    pub zero_length: bool,
    pub duplicates: bool,
    pub dangling: bool,
    pub unconnected_vias: bool,
    pub merge_collinear: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub delete: Vec<ItemId>,
    pub replace: Vec<IdItem>,
    pub zero_length: usize,
    pub duplicates: usize,
    pub dangling: usize,
    pub vias: usize,
    pub merged: usize,
    /// Useful tracks whose unattached tails were shortened.
    pub trimmed: usize,
    /// Reviewed source copper prevents same-ID edits from silently becoming stale.
    #[serde(default)]
    pub original: Vec<IdItem>,
}

/// Exact integer containment preserves arbitrary source headings and tiny kinks.
fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let cross = (b.x as i128 - a.x as i128) * (p.y as i128 - a.y as i128)
        - (b.y as i128 - a.y as i128) * (p.x as i128 - a.x as i128);
    cross == 0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}
fn covered_by(inner: &Track, outer: &Track) -> bool {
    on_segment(inner.a, outer.a, outer.b) && on_segment(inner.b, outer.a, outer.b)
}

pub fn cleanup(doc: &Document, opts: &CleanupOptions) -> CleanupResult {
    let mut out = CleanupResult::default();
    let mut removed: HashSet<ItemId> = HashSet::new();
    let mut trimmed: HashMap<ItemId, Track> = HashMap::new();
    let tracks: Vec<(ItemId, &Track)> = doc
        .items()
        .filter_map(|(id, i)| {
            if let Item::Track(t) = i {
                if t.locked { None } else { Some((id, t)) }
            } else {
                None
            }
        })
        .collect();

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
        for (id, item) in doc.items() {
            if let Item::Track(t) = item {
                if !removed.contains(&id) {
                    by_line
                        .entry((t.layer, t.net.as_str()))
                        .or_default()
                        .push((id, t));
                }
            }
        }
        for group in by_line.values() {
            for &(id, t) in group {
                if t.locked {
                    continue;
                }
                let redundant = group.iter().any(|&(oid, o)| {
                    oid != id
                        && !removed.contains(&oid)
                        && o.width >= t.width
                        && covered_by(t, o)
                        && (o.locked || !covered_by(o, t) || o.width > t.width || oid < id)
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
                    if o.kind != ObjKind::Track
                        || removed.contains(&o.item)
                        || doc.get(o.item).is_some_and(crate::command::is_locked)
                    {
                        continue;
                    }
                    let Some(Item::Track(original)) = doc.get(o.item) else {
                        continue;
                    };
                    let t = trimmed.get(&o.item).unwrap_or(original);
                    let contacts = contacts::contacts(&idx, i, &removed);
                    let a = contacts::touches_end(&idx, &contacts, t, t.a);
                    let b = contacts::touches_end(&idx, &contacts, t, t.b);
                    if !a || !b {
                        if contacts::one_support(&idx, &contacts) {
                            removed.insert(o.item);
                            trimmed.remove(&o.item);
                            out.dangling += 1;
                            changed = true;
                        } else if let Some(next) = contacts::trim(&idx, &contacts, t, a, b) {
                            trimmed.insert(o.item, next);
                            changed = true;
                        }
                    }
                }
            }
            if opts.unconnected_vias {
                for (i, o) in idx.objs.iter().enumerate() {
                    if o.kind != ObjKind::Via
                        || removed.contains(&o.item)
                        || doc.get(o.item).is_some_and(crate::command::is_locked)
                    {
                        continue;
                    }
                    let mut layers: HashSet<LayerId> = HashSet::new();
                    for j in idx.tree.query(idx.geos[i].bb.grow(TOUCH)) {
                        if j != i
                            && idx.objs[j].net == o.net
                            && !removed.contains(&idx.objs[j].item)
                            && idx.touches(i, j)
                        {
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

    out.trimmed = trimmed.len();
    out.replace.extend(
        trimmed
            .into_iter()
            .filter(|(id, _)| !removed.contains(id))
            .map(|(id, t)| IdItem {
                id,
                item: Item::Track(t),
            }),
    );
    if opts.merge_collinear {
        out.merged = merge_collinear(doc, &mut removed, &mut out.replace);
    }

    out.delete = removed.into_iter().collect();
    out.delete.sort();
    out.replace.sort_by_key(|r| r.id);
    // A final graph check catches interactions between independently cleaned
    // spans. Conservative fallback retains useful routing if it cannot be proven.
    if !preserves_connections(doc, &out) {
        if opts.dangling || opts.unconnected_vias {
            return cleanup(
                doc,
                &CleanupOptions {
                    dangling: false,
                    unconnected_vias: false,
                    ..*opts
                },
            );
        }
        return CleanupResult::default();
    }
    out.original = out
        .delete
        .iter()
        .copied()
        .chain(out.replace.iter().map(|r| r.id))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| doc.get(id).cloned().map(|item| IdItem { id, item }))
        .collect();
    out
}

/// Replan under the caller's document lock and accept only the reviewed result.
pub fn cleanup_command(
    doc: &Document,
    opts: &CleanupOptions,
    expected: &CleanupResult,
) -> Result<crate::Command, String> {
    let current = cleanup(doc, opts);
    if &current != expected {
        return Err(
            "Board or footprint library changed after review; refresh the cleanup preview".into(),
        );
    }
    if current.delete.is_empty() && current.replace.is_empty() {
        return Err("No cleanup changes to apply".into());
    }
    Ok(crate::Command::Route {
        delete: current.delete,
        replace: current.replace,
        add: vec![],
    })
}

fn preserves_connections(doc: &Document, plan: &CleanupResult) -> bool {
    if plan.delete.is_empty() && plan.replace.is_empty() {
        return true;
    }
    let before = super::elastic::net_connectivity(doc);
    let mut work = doc.clone();
    let edits: Vec<_> = plan
        .delete
        .iter()
        .map(|id| crate::document::Edit {
            id: *id,
            before: doc.get(*id).cloned(),
            after: None,
        })
        .chain(plan.replace.iter().map(|r| crate::document::Edit {
            id: r.id,
            before: doc.get(r.id).cloned(),
            after: Some(r.item.clone()),
        }))
        .collect();
    work.apply(&edits);
    let after = super::elastic::net_connectivity(&work);
    let mut groups = HashMap::new();
    for object in before
        .objects()
        .iter()
        .filter(|o| work.get(o.item).is_some())
    {
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

/// Joins pairs of equal tracks meeting end to end in a straight line where nothing else connects.
fn merge_collinear(
    doc: &Document,
    removed: &mut HashSet<ItemId>,
    replace: &mut Vec<IdItem>,
) -> usize {
    let planned: HashMap<_, _> = replace
        .iter()
        .filter_map(|r| match &r.item {
            Item::Track(t) => Some((r.id, t.clone())),
            _ => None,
        })
        .collect();
    let mut live: HashMap<ItemId, Track> = doc
        .items()
        .filter_map(|(id, i)| {
            if let Item::Track(t) = i {
                (!removed.contains(&id) && t.a != t.b)
                    .then(|| (id, planned.get(&id).unwrap_or(t).clone()))
            } else {
                None
            }
        })
        .collect();
    // Preserve explicit joints at actual fixed copper contacts, including
    // offset lands and circular interiors. Bounding circles are not pad shapes.
    let board = settings_or_default(doc);
    let fixed = Index::new(
        copper_objects(doc, &board)
            .into_iter()
            .filter(|o| {
                !removed.contains(&o.item)
                    && (matches!(o.kind, ObjKind::Via | ObjKind::Pad | ObjKind::Arc)
                        || doc.get(o.item).is_some_and(crate::command::is_locked))
            })
            .collect(),
    );
    let on_fixed = |p: Point, layer: crate::LayerId, net: &str, width: i64| {
        let q = super::connectivity::pt(p);
        let cap = super::connectivity::geo::Geo::cap(q, q, width as f64 / 2.);
        fixed.tree.query(cap.bb.grow(TOUCH)).any(|j| {
            fixed.objs[j].net == net
                && fixed.objs[j].on_layer(layer)
                && super::connectivity::geo::gap(&cap, &fixed.geos[j], TOUCH).is_some()
        })
    };
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
        let mut joints: Vec<(&(LayerId, Point), &Vec<ItemId>)> = ends
            .iter()
            .filter(|(_, v)| v.len() == 2 && v[0] != v[1])
            .collect();
        joints.sort_by_key(|(k, _)| (k.0, k.1.x, k.1.y));
        let mut step = None;
        for (&(layer, p), v) in joints {
            let (t1, t2) = (&live[&v[0]], &live[&v[1]]);
            if t1.locked
                || t2.locked
                || t1.net != t2.net
                || t1.width != t2.width
                || on_fixed(p, layer, &t1.net, t1.width)
            {
                continue;
            }
            let far1 = if t1.a == p { t1.b } else { t1.a };
            let far2 = if t2.a == p { t2.b } else { t2.a };
            if far1 != far2 && on_segment(p, far1, far2) {
                step = Some((
                    v[0],
                    v[1],
                    Track {
                        a: far1,
                        b: far2,
                        ..t1.clone()
                    },
                ));
                break;
            }
        }
        let Some((keep, drop, joined)) = step else {
            break;
        };
        live.remove(&drop);
        live.insert(keep, joined);
        touched.remove(&drop);
        touched.insert(keep);
        removed.insert(drop);
        merged += 1;
    }
    let mut kept: Vec<ItemId> = touched.into_iter().collect();
    kept.sort();
    replace.retain(|r| !removed.contains(&r.id) && !kept.contains(&r.id));
    replace.extend(kept.into_iter().map(|id| IdItem {
        id,
        item: Item::Track(live.remove(&id).unwrap()),
    }));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer;
    use crate::pcb::connectivity::fixtures::{doc, fp, p, track, via};
    use crate::{Command, Session};

    const ALL: CleanupOptions = CleanupOptions {
        zero_length: true,
        duplicates: true,
        dangling: true,
        unconnected_vias: true,
        merge_collinear: true,
    };

    fn apply(d: &Document, r: &CleanupResult) -> Document {
        let mut s = Session::with_document(d.clone());
        s.execute(Command::Route {
            delete: r.delete.clone(),
            add: vec![],
            replace: r.replace.clone(),
        })
        .unwrap();
        s.document().clone()
    }

    fn tracks(d: &Document) -> Vec<Track> {
        d.items()
            .filter_map(|(_, i)| {
                if let Item::Track(t) = i {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .collect()
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
        let mut items = vec![
            via(12.0, 0.0, "A"),
            track((12.0, 0.0), (12.0, 4.0), 0.25, layer::BOTTOM_COPPER, "A"),
        ];
        items.extend([track(
            (12.0, 4.0),
            (20.0, 4.0),
            0.25,
            layer::BOTTOM_COPPER,
            "A",
        )]);
        let d = routed(items);
        let r = cleanup(
            &d,
            &CleanupOptions {
                merge_collinear: true,
                ..NONE
            },
        );
        // (5,0) merges; (12,0) holds a via.
        assert_eq!(r.merged, 1);
    }

    const NONE: CleanupOptions = CleanupOptions {
        zero_length: false,
        duplicates: false,
        dangling: false,
        unconnected_vias: false,
        merge_collinear: false,
    };

    #[test]
    fn dangling_stubs_go_including_chains_and_their_lonely_via() {
        // A stub off the joint at (5,0) running to a via with nothing on the bottom, then a chain to nowhere.
        let d = routed(vec![
            track((5.0, 0.0), (5.0, 5.0), 0.25, layer::TOP_COPPER, "A"),
            via(5.0, 5.0, "A"),
            track((30.0, 0.0), (35.0, 0.0), 0.25, layer::TOP_COPPER, "A"),
            track((35.0, 0.0), (35.0, 5.0), 0.25, layer::TOP_COPPER, "A"),
        ]);
        let r = cleanup(
            &d,
            &CleanupOptions {
                dangling: true,
                unconnected_vias: true,
                ..NONE
            },
        );
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
        let r = cleanup(
            &d,
            &CleanupOptions {
                zero_length: true,
                duplicates: true,
                ..NONE
            },
        );
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

#[cfg(test)]
#[path = "cleanup_regression_tests.rs"]
mod regressions;
