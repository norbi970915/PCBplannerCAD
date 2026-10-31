//! Bounded cleanup of old straight-track bends beside a committed route.
use super::{
    Track,
    connectivity::{self, Index},
    elastic,
    router::CornerStyle,
};
use crate::{Command, Document, IdItem, Item, ItemId, Point, document::Edit, error::CoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
#[path = "route_cleanup_checks.rs"]
mod checks;
#[path = "route_cleanup_geom.rs"]
mod geom;
#[path = "route_cleanup_paths.rs"]
mod paths;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingCleanup {
    /// Indices in the incoming Route add list whose exact geometry is protected.
    pub protect: Vec<usize>,
    #[serde(default)]
    pub preserve_nets: Vec<String>,
    pub corner: CornerStyle,
}
const MAX_ATTEMPTS: usize = 64;
const MAX_CHECKS: usize = 8;
const MAX_ACCEPTED: usize = 4;
const MAX_SEEDS: usize = 32;
const MAX_CHAIN: usize = 24;
const MAX_WINDOW: usize = MAX_CHAIN;
const MAX_WAYPOINT_WINDOWS: usize = 16;

fn length(path: &[Point]) -> f64 {
    path.windows(2)
        .map(|w| ((w[1].x - w[0].x) as f64).hypot((w[1].y - w[0].y) as f64))
        .sum()
}
fn near(point: Point, roots: &[Track], margin: f64) -> bool {
    roots.iter().any(|t| {
        let dx = (t.b.x - t.a.x) as f64;
        let dy = (t.b.y - t.a.y) as f64;
        let len = dx * dx + dy * dy;
        let u = if len == 0. {
            0.
        } else {
            (((point.x - t.a.x) as f64 * dx + (point.y - t.a.y) as f64 * dy) / len).clamp(0., 1.)
        };
        ((point.x - t.a.x) as f64 - u * dx).hypot((point.y - t.a.y) as f64 - u * dy) <= margin
    })
}
// Each capsule is convex: both ends inside the same local capsule imply
// the complete segment is local, even beside a long U-shaped route.
fn corridor(path: &[Point], roots: &[Track], margin: f64) -> bool {
    path.windows(2).all(|w| {
        roots.iter().any(|root| {
            near(w[0], std::slice::from_ref(root), margin)
                && near(w[1], std::slice::from_ref(root), margin)
        })
    })
}
fn track(doc: &Document, id: ItemId) -> Option<&Track> {
    match doc.get(id) {
        Some(Item::Track(t)) => Some(t),
        _ => None,
    }
}
fn chain(
    doc: &Document,
    topo: &elastic::Topology,
    seed: ItemId,
    protected: &HashSet<ItemId>,
    anchors: &HashSet<Point>,
) -> Option<(Vec<ItemId>, Vec<Point>)> {
    let t = track(doc, seed)?;
    if t.locked || !topo.interior_joints(seed).is_empty() {
        return None;
    }
    let mut used = HashSet::from([seed]);
    let mut grow = |start: Point| {
        let mut ids = Vec::new();
        let mut points = vec![start];
        let (mut at, mut id) = (start, seed);
        while used.len() < MAX_CHAIN && !anchors.contains(&at) {
            let current = track(doc, id).unwrap();
            let Some(next) = topo.continuation(at, current, id) else {
                break;
            };
            if protected.contains(&next)
                || used.contains(&next)
                || !topo.interior_joints(next).is_empty()
            {
                break;
            }
            let next_track = track(doc, next).unwrap();
            at = if next_track.a == at {
                next_track.b
            } else {
                next_track.a
            };
            used.insert(next);
            ids.push(next);
            points.push(at);
            id = next;
        }
        (ids, points)
    };
    let (left, left_points) = grow(t.a);
    let (right, right_points) = grow(t.b);
    let ids = left
        .into_iter()
        .rev()
        .chain(std::iter::once(seed))
        .chain(right)
        .collect();
    let mut path: Vec<_> = left_points.into_iter().rev().collect();
    path.push(t.b);
    path.extend(right_points.into_iter().skip(1));
    Some((ids, path))
}
fn candidates(a: Point, b: Point, style: CornerStyle) -> Vec<Vec<Point>> {
    if a == b {
        return Vec::new();
    }
    let dx = b.x as i128 - a.x as i128;
    let dy = b.y as i128 - a.y as i128;
    let direct = match style {
        CornerStyle::Any => true,
        CornerStyle::Orthogonal90 => dx == 0 || dy == 0,
        _ => dx == 0 || dy == 0 || dx.abs() == dy.abs(),
    };
    let mut out = Vec::new();
    if direct {
        out.push(vec![a, b]);
    }
    for flip in [false, true] {
        out.push(elastic::simplify(&super::router::cleanup_corner_path(
            a, b, style, flip,
        )));
    }
    out.sort_by(|a, b| {
        length(a).total_cmp(&length(b)).then_with(|| {
            a.iter()
                .map(|p| (p.x, p.y))
                .cmp(b.iter().map(|p| (p.x, p.y)))
        })
    });
    out.dedup();
    out
}
fn replace_window(
    doc: &Document,
    ids: &[ItemId],
    points: &[Point],
    prototype: &Track,
) -> (Document, Vec<ItemId>) {
    let mut work = doc.clone();
    let mut edits: Vec<_> = ids
        .iter()
        .skip(1)
        .map(|id| Edit {
            id: *id,
            before: doc.get(*id).cloned(),
            after: None,
        })
        .collect();
    let mut changed = Vec::new();
    for (i, pair) in points.windows(2).enumerate() {
        let id = if i == 0 { ids[0] } else { work.alloc_id() };
        changed.push(id);
        edits.push(Edit {
            id,
            before: doc.get(id).cloned(),
            after: Some(Item::Track(Track {
                a: pair[0],
                b: pair[1],
                ..prototype.clone()
            })),
        });
    }
    work.apply(&edits);
    (work, changed)
}

/// Prepare one atomic routing command; failed cleanup leaves the requested route
/// intact. Temporary planning IDs never escape into source replacements.
pub fn prepare(
    doc: &Document,
    command: Command,
    request: &RoutingCleanup,
) -> Result<Command, CoreError> {
    let Command::Route {
        delete,
        replace,
        add,
    } = &command
    else {
        return Err(CoreError::InvalidGeometry(
            "automatic cleanup requires a PCB routing command",
        ));
    };
    if doc.kind() != crate::DocKind::Pcb {
        return Err(CoreError::InvalidGeometry(
            "automatic cleanup requires a PCB routing command",
        ));
    }
    if request
        .protect
        .iter()
        .any(|i| !matches!(add.get(*i), Some(Item::Track(_))))
    {
        return Err(CoreError::InvalidGeometry(
            "automatic cleanup requires valid protected routing tracks",
        ));
    }
    if add
        .iter()
        .any(|i| !matches!(i, Item::Track(_) | Item::Via(_) | Item::ArcTrack(_)))
    {
        return Ok(command);
    }
    if request.protect.is_empty() {
        return Ok(command);
    }
    let mut work = doc.clone();
    let mut edits = Command::Change {
        delete: delete.clone(),
        add: add.clone(),
    }
    .resolve(&mut work)?;
    let added: Vec<_> = edits
        .iter()
        .filter(|e| e.before.is_none() && e.after.is_some())
        .map(|e| e.id)
        .collect();
    edits.extend(
        Command::ReplaceMany {
            items: replace.clone(),
        }
        .resolve(&mut work)?,
    );
    work.apply(&edits);
    let staged = work.clone();
    let mut protected: HashSet<_> = request.protect.iter().map(|i| added[*i]).collect();
    protected.extend(replace.iter().map(|r| r.id));
    let roots: Vec<_> = request
        .protect
        .iter()
        .filter_map(|i| track(&work, added[*i]).cloned())
        .collect();
    let anchors: HashSet<_> = roots.iter().flat_map(|t| [t.a, t.b]).collect();
    let board = super::connectivity::settings_or_default(&work);
    let rules = super::query::RuleSet::new(&board);
    if !rules.errors().is_empty() {
        return Ok(command);
    }
    let margin = 3_000_000f64
        .max(rules.max_clearance() as f64 * 2.)
        .max(roots.iter().map(|t| t.width as f64 * 4.).fold(0., f64::max));
    let mut excluded: HashSet<_> = request.preserve_nets.iter().cloned().collect();
    for pair in &board.diff_pairs {
        excluded.insert(pair.positive.clone());
        excluded.insert(pair.negative.clone());
    }
    // Evaluate real object kinds and layers. A constraint on one layer or an
    // arc protects the whole net, including its unconstrained approach tracks.
    let objects: HashSet<_> = work
        .items()
        .filter_map(|(_, item)| match item {
            Item::Track(t) => Some([
                (super::query::ObjKind::Track, t.net.as_str(), None),
                (super::query::ObjKind::Track, t.net.as_str(), Some(t.layer)),
            ]),
            Item::ArcTrack(t) => Some([
                (super::query::ObjKind::Track, t.net.as_str(), None),
                (super::query::ObjKind::Arc, t.net.as_str(), Some(t.layer)),
            ]),
            _ => None,
        })
        .flatten()
        .collect();
    for (kind, net, layer) in objects {
        if net.is_empty()
            || rules
                .length_rule(&super::query::Obj::new(kind, net, layer))
                .is_some()
        {
            excluded.insert(net.to_string());
        }
    }
    drop(board);
    let mut active: BTreeSet<_> = added
        .iter()
        .copied()
        .filter(|id| !protected.contains(id) && track(&work, *id).is_some())
        .collect();
    // Expose exact centerline landings as fixed endpoints for local traversal.
    // These geometry-preserving splits are discarded if no cleanup is accepted.
    let landings = geom::split_landings(&mut work, &roots, &protected, &excluded);
    active.extend(landings.active.iter().copied());
    let (mut attempts, mut full_checks, mut accepted) = (0, 0, 0);
    let mut waypoint_windows = 0;
    while attempts < MAX_ATTEMPTS && full_checks < MAX_CHECKS && accepted < MAX_ACCEPTED {
        let mut seeds = active.clone();
        let mut ends: HashMap<Point, Vec<ItemId>> = HashMap::new();
        for (id, item) in work.items() {
            if let Item::Track(t) = item {
                ends.entry(t.a).or_default().push(id);
                ends.entry(t.b).or_default().push(id);
            }
        }
        for root in &roots {
            for point in [root.a, root.b] {
                for id in ends.get(&point).into_iter().flatten() {
                    if track(&work, *id).is_some_and(|t| t.net == root.net && t.layer == root.layer)
                    {
                        seeds.insert(*id);
                    }
                }
            }
        }
        seeds.retain(|id| {
            !protected.contains(id)
                && track(&work, *id).is_some_and(|t| !t.locked && !excluded.contains(&t.net))
        });
        if seeds.is_empty() {
            break;
        }
        let topology = elastic::Topology::new(&work);
        let board = connectivity::settings_or_default(&work);
        let rules = super::query::RuleSet::new(&board);
        let mut index = None;
        let mut seen = HashSet::new();
        let mut result = None;
        'search: for seed in seeds.into_iter().take(MAX_SEEDS) {
            if protected.contains(&seed) || seen.contains(&seed) {
                continue;
            }
            let Some(prototype) = track(&work, seed) else {
                continue;
            };
            if excluded.contains(&prototype.net) {
                continue;
            }
            // Consuming a fully covered source changes no copper shape and can
            // safely retain an interior branch/via through the protected root.
            // Such contacts still stop every shape-changing chain traversal.
            let section = if geom::root_covers(&roots, prototype, prototype.a, prototype.b) {
                Some((vec![seed], vec![prototype.a, prototype.b]))
            } else {
                chain(&work, &topology, seed, &protected, &anchors)
            };
            let Some((ids, path)) = section else { continue };
            seen.extend(ids.iter().copied());
            for i in 0..path.len().saturating_sub(1) {
                for j in ((i + 1)..=path.len().saturating_sub(1).min(i + MAX_WINDOW)).rev() {
                    let old = &path[i..=j];
                    if !old[1..old.len() - 1]
                        .iter()
                        .all(|p| near(*p, &roots, margin))
                        || old[1..old.len() - 1].iter().any(|p| anchors.contains(p))
                    {
                        continue;
                    }
                    // A one-edge window is only useful for consuming copper
                    // already covered by the protected route. Avoid constructing
                    // a board-wide spatial index for ordinary straight routing.
                    if old.len() == 2 && !geom::root_covers(&roots, prototype, old[0], old[1]) {
                        continue;
                    }
                    let paths = if old.len() == 2 {
                        vec![old.to_vec()]
                    } else {
                        candidates(path[i], path[j], request.corner)
                    };
                    // Keep the cheap endpoint candidates first. Only construct
                    // waypoint alternatives when they cannot produce a safe edit.
                    let paths = paths.into_iter().chain(
                        std::iter::once_with(|| {
                            if old.len() < 4
                                || waypoint_windows >= MAX_WAYPOINT_WINDOWS
                                || !corridor(old, &roots, margin)
                            {
                                return Vec::new();
                            }
                            waypoint_windows += 1;
                            paths::retained(old, request.corner)
                        })
                        .flatten(),
                    );
                    for points in paths {
                        if points.len() > old.len() || points.len() < 2 {
                            continue;
                        }
                        let exact = elastic::simplify(old) == points;
                        if !exact
                            && (!corridor(old, &roots, margin)
                                || !corridor(&points, &roots, margin)
                                || !geom::improves(old, &points))
                        {
                            continue;
                        }
                        let mut whole = path[..i].to_vec();
                        whole.extend(points.iter().copied());
                        whole.extend_from_slice(&path[j + 1..]);
                        if !elastic::simple_path(&elastic::simplify(&whole)) {
                            continue;
                        }
                        attempts += 1;
                        let removed = &ids[i..j];
                        let index = index.get_or_insert_with(|| {
                            Index::new(connectivity::copper_objects(&work, &board))
                        });
                        let covered = checks::covered(index, removed, prototype, &points);
                        // Equal geometry without removed pieces is useful only
                        // when other copper already covers the whole span.
                        if exact && points.len() == old.len() && covered.is_none() {
                            if attempts >= MAX_ATTEMPTS {
                                break 'search;
                            }
                            continue;
                        }
                        // Covered shortcuts add no copper, but deleting a bowed
                        // span can still lose its own-net pour contact.
                        if checks::physical(
                            index,
                            &rules,
                            removed,
                            prototype,
                            old,
                            &points,
                            exact || covered.is_some(),
                        ) {
                            let (candidate, changed) = if let Some(keepers) = covered {
                                let mut candidate = work.clone();
                                let edits: Vec<_> = removed
                                    .iter()
                                    .map(|id| Edit {
                                        id: *id,
                                        before: work.get(*id).cloned(),
                                        after: None,
                                    })
                                    .collect();
                                candidate.apply(&edits);
                                (candidate, keepers)
                            } else {
                                replace_window(&work, removed, &points, prototype)
                            };
                            full_checks += 1;
                            if checks::clear(&candidate, &changed) {
                                result = Some((candidate, changed));
                                break 'search;
                            }
                        }
                        if attempts >= MAX_ATTEMPTS || full_checks >= MAX_CHECKS {
                            break 'search;
                        }
                    }
                }
            }
        }
        let Some((candidate, changed)) = result else {
            break;
        };
        work = candidate;
        active.extend(changed);
        accepted += 1;
    }
    landings.restore_unused(&mut work);
    if accepted == 0
        || protected.iter().any(|id| work.get(*id) != staged.get(*id))
        || !checks::connections(&staged, &work)
    {
        return Ok(command);
    }
    let mut delete = Vec::new();
    let mut replace = Vec::new();
    let mut add = Vec::new();
    for (id, item) in doc.items() {
        match work.get(id) {
            None => delete.push(id),
            Some(next) if next != item => replace.push(IdItem {
                id,
                item: next.clone(),
            }),
            _ => {}
        }
    }
    for (id, item) in work.items() {
        if doc.get(id).is_none() {
            add.push(item.clone());
        }
    }
    Ok(Command::Route {
        delete,
        replace,
        add,
    })
}
#[cfg(test)]
#[path = "route_cleanup_tests.rs"]
mod tests;
