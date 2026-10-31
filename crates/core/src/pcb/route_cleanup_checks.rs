//! Exact physical gates for local bend cleanup. Existing pours are regenerated.
use super::super::{
    Track,
    connectivity::{
        self, Index,
        geo::{Geo, TOUCH},
    },
    query::{Obj, ObjKind, RuleSet},
};
use crate::{Document, Item, ItemId, Point, document::Edit};
use std::collections::{HashMap, HashSet};
fn cap(a: Point, b: Point, width: i64) -> Geo {
    Geo::cap(connectivity::pt(a), connectivity::pt(b), width as f64 / 2.)
}
fn touching(geos: &[Geo], fixed: &Geo) -> bool {
    geos.iter()
        .any(|g| connectivity::geo::gap(g, fixed, TOUCH).is_some())
}
pub(super) fn physical(
    index: &Index,
    rules: &RuleSet,
    removed: &[ItemId],
    t: &Track,
    old: &[Point],
    next: &[Point],
    no_new_copper: bool,
) -> bool {
    let unchanged = super::super::elastic::simplify(old).as_slice() == next;
    let old: Vec<_> = old.windows(2).map(|p| cap(p[0], p[1], t.width)).collect();
    let next: Vec<_> = next.windows(2).map(|p| cap(p[0], p[1], t.width)).collect();
    let mut nearby = HashSet::new();
    for geo in old.iter().chain(next.iter()) {
        nearby.extend(
            index
                .tree
                .query(geo.bb.grow(rules.max_clearance() as f64 + TOUCH)),
        );
    }
    let own = Obj::new(ObjKind::Track, &t.net, Some(t.layer));
    for i in nearby {
        let fixed = &index.objs[i];
        if removed.contains(&fixed.item) || !fixed.on_layer(t.layer) {
            continue;
        }
        if fixed.kind == ObjKind::Zone
            && fixed.net == t.net
            && !unchanged
            && touching(&old, &index.geos[i])
        {
            return false;
        }
        // Foreign fills can be regenerated for copper already requested by the
        // user; cleanup itself must not cut a new corridor through a plane.
        if fixed.kind == ObjKind::Zone && fixed.net != t.net && no_new_copper {
            continue;
        }
        if fixed.net == t.net {
            if touching(&old, &index.geos[i]) != touching(&next, &index.geos[i]) {
                return false;
            }
        } else {
            let clearance = rules.clearance(&own, &fixed.obj(Some(t.layer))) as f64;
            if next.iter().any(|g| {
                connectivity::geo::gap(g, &index.geos[i], clearance + TOUCH)
                    .is_some_and(|(gap, _)| gap < clearance - TOUCH)
            }) {
                return false;
            }
        }
    }
    true
}
pub(super) fn clear(doc: &Document, changed: &[ItemId]) -> bool {
    let mut check = doc.clone();
    let edits: Vec<_> = check
        .items()
        .filter_map(|(id, item)| {
            matches!(item,Item::Zone(z)if !z.keepout).then(|| Edit {
                id,
                before: Some(item.clone()),
                after: None,
            })
        })
        .collect();
    check.apply(&edits);
    !super::super::drc::run_near(&check, changed)
        .iter()
        .any(|v| v.severity == super::super::drc::Severity::Error)
}
pub(super) fn connections(before: &Document, after: &Document) -> bool {
    let old = super::super::elastic::net_connectivity(before);
    let next = super::super::elastic::net_connectivity(after);
    let mut forward = HashMap::new();
    let mut reverse = HashMap::new();
    for o in old.objects().iter().filter(|o| after.get(o.item).is_some()) {
        let Some(a) = old.cluster_of(o.item, o.pad.as_deref()) else {
            continue;
        };
        let Some(b) = next.cluster_of(o.item, o.pad.as_deref()) else {
            return false;
        };
        if forward
            .insert((a, o.net.as_str()), b)
            .is_some_and(|prior| prior != b)
            || reverse
                .insert((b, o.net.as_str()), a)
                .is_some_and(|prior| prior != a)
        {
            return false;
        }
    }
    true
}
/// Existing same-net copper may already cover the proposed shortcut. In that
/// case remove the obsolete old span instead of creating duplicate copper.
pub(super) fn covered(
    index: &Index,
    removed: &[ItemId],
    t: &Track,
    next: &[Point],
) -> Option<Vec<ItemId>> {
    let mut keepers = HashSet::new();
    for pair in next.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let dx = b.x as i128 - a.x as i128;
        let dy = b.y as i128 - a.y as i128;
        let value = |p: Point| {
            if dx.abs() >= dy.abs() {
                p.x as i128
            } else {
                p.y as i128
            }
        };
        let cross = |p: Point| dx * (p.y as i128 - a.y as i128) - dy * (p.x as i128 - a.x as i128);
        let lo = value(a).min(value(b));
        let hi = value(a).max(value(b));
        let geometry = cap(a, b, t.width);
        let mut intervals = Vec::new();
        for i in index.tree.query(geometry.bb) {
            let object = &index.objs[i];
            if removed.contains(&object.item) || object.net != t.net || !object.on_layer(t.layer) {
                continue;
            }
            if let connectivity::Shape::Segment { a, b, width } = &object.shape {
                if object.kind != ObjKind::Track
                    || *width < t.width
                    || cross(*a) != 0
                    || cross(*b) != 0
                {
                    continue;
                }
                let l = value(*a).min(value(*b)).max(lo);
                let h = value(*a).max(value(*b)).min(hi);
                if l < h {
                    intervals.push((l, h, object.item));
                }
            }
        }
        intervals.sort();
        let mut reached = lo;
        for (l, h, id) in intervals {
            if l > reached {
                break;
            }
            if h > reached {
                reached = h;
                keepers.insert(id);
            }
            if reached >= hi {
                break;
            }
        }
        if reached < hi {
            return None;
        }
    }
    let mut ids: Vec<_> = keepers.into_iter().collect();
    ids.sort();
    Some(ids)
}
