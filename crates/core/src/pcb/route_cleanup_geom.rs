//! Exact geometry preparation for bounded automatic bend cleanup.
use super::*;
use crate::LayerId;

/// An octilinear path has length axial + diagonal * sqrt(2). Equal integer
/// coefficients prove equal length without flattening nanometre-scale kinks.
fn metric(path: &[Point]) -> Option<(i128, i128)> {
    let (mut axial, mut diagonal) = (0, 0);
    for pair in path.windows(2) {
        let dx = (pair[1].x as i128 - pair[0].x as i128).abs();
        let dy = (pair[1].y as i128 - pair[0].y as i128).abs();
        if dx == 0 || dy == 0 {
            axial += dx + dy;
        } else if dx == dy {
            diagonal += dx;
        } else {
            return None;
        }
    }
    Some((axial, diagonal))
}

pub(super) fn improves(old: &[Point], next: &[Point]) -> bool {
    length(next) + 1. < length(old)
        || (next.len() < old.len() && metric(old).is_some_and(|cost| metric(next) == Some(cost)))
}

pub(super) struct Landings {
    pub active: Vec<ItemId>,
    splits: Vec<(IdItem, Vec<IdItem>)>,
}
impl Landings {
    /// A landing considered for cleanup must not fragment unrelated old copper
    /// when only a different conductor can actually be improved.
    pub fn restore_unused(&self, doc: &mut Document) {
        let mut edits = Vec::new();
        for (original, parts) in &self.splits {
            if !parts
                .iter()
                .all(|part| doc.get(part.id) == Some(&part.item))
            {
                continue;
            }
            for part in parts {
                edits.push(Edit {
                    id: part.id,
                    before: Some(part.item.clone()),
                    after: (part.id == original.id).then(|| original.item.clone()),
                });
            }
        }
        doc.apply(&edits);
    }
}

fn on_segment(t: &Track, p: Point) -> bool {
    let dx = t.b.x as i128 - t.a.x as i128;
    let dy = t.b.y as i128 - t.a.y as i128;
    t.a != t.b
        && p.x >= t.a.x.min(t.b.x)
        && p.x <= t.a.x.max(t.b.x)
        && p.y >= t.a.y.min(t.b.y)
        && p.y <= t.a.y.max(t.b.y)
        && dx * (p.y as i128 - t.a.y as i128) == dy * (p.x as i128 - t.a.x as i128)
}

pub(super) fn root_covers(roots: &[Track], t: &Track, a: Point, b: Point) -> bool {
    roots.iter().any(|root| {
        root.net == t.net
            && root.layer == t.layer
            && root.width >= t.width
            && on_segment(root, a)
            && on_segment(root, b)
    })
}

/// Split only unlocked, unprotected old tracks at exact same-net/layer
/// centerline landings of the new route. Root endpoints remain fixed.
pub(super) fn split_landings(
    doc: &mut Document,
    roots: &[Track],
    protected: &HashSet<ItemId>,
    excluded: &HashSet<String>,
) -> Landings {
    let mut points: HashMap<(&str, LayerId), Vec<Point>> = HashMap::new();
    for root in roots {
        points
            .entry((&root.net, root.layer))
            .or_default()
            .extend([root.a, root.b]);
    }
    for landings in points.values_mut() {
        landings.sort_by_key(|p| (p.x, p.y));
        landings.dedup();
    }
    let mut plan = Landings {
        active: Vec::new(),
        splits: Vec::new(),
    };
    let mut budget = super::MAX_SEEDS;
    let mut splits = Vec::new();
    for (id, item) in doc.items() {
        let Item::Track(t) = item else { continue };
        if protected.contains(&id) || t.locked || excluded.contains(&t.net) || t.a == t.b {
            continue;
        }
        let Some(landings) = points.get(&(t.net.as_str(), t.layer)) else {
            continue;
        };
        // A new route can cover an old track without sharing its endpoints.
        // Seed exact covered spans as well as endpoint-attached sections.
        if plan.active.len() < super::MAX_SEEDS && root_covers(roots, t, t.a, t.b) {
            plan.active.push(id);
        }
        let dx = t.b.x as i128 - t.a.x as i128;
        let dy = t.b.y as i128 - t.a.y as i128;
        let mut path = vec![t.a];
        for &at in landings {
            if at != t.a
                && at != t.b
                && at.x >= t.a.x.min(t.b.x)
                && at.x <= t.a.x.max(t.b.x)
                && at.y >= t.a.y.min(t.b.y)
                && at.y <= t.a.y.max(t.b.y)
                && dx * (at.y as i128 - t.a.y as i128) == dy * (at.x as i128 - t.a.x as i128)
            {
                path.push(at);
                budget -= 1;
                if budget == 0 {
                    break;
                }
            }
        }
        if path.len() > 1 {
            path.push(t.b);
            path.sort_by_key(|p| {
                if dx != 0 {
                    p.x as i128 * dx.signum()
                } else {
                    p.y as i128 * dy.signum()
                }
            });
            splits.push((id, t.clone(), path));
        }
        if budget == 0 {
            break;
        }
    }

    let mut edits = Vec::new();
    for (source, t, path) in splits {
        let mut parts = Vec::new();
        for (i, edge) in path.windows(2).enumerate() {
            let id = if i == 0 { source } else { doc.alloc_id() };
            plan.active.push(id);
            let item = Item::Track(Track {
                a: edge[0],
                b: edge[1],
                ..t.clone()
            });
            parts.push(IdItem {
                id,
                item: item.clone(),
            });
            edits.push(Edit {
                id,
                before: doc.get(id).cloned(),
                after: Some(item),
            });
        }
        plan.splits.push((
            IdItem {
                id: source,
                item: Item::Track(t),
            },
            parts,
        ));
    }
    doc.apply(&edits);
    plan
}
