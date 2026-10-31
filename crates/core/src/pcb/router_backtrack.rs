//! Bounded reconsideration of shove choices after a downstream dependency fails.
//! Each candidate owns its overlay snapshot. Failed descendants cannot leave
//! removed source copper, added segments or queue state in a sibling branch.
use super::{
    chain, engine,
    geom::{FxSet, V},
    shove::{Detour, MAX_DEPTH, Pusher, alternatives, anchored_detour},
    world::{Core, Hit, ObKind, Obstacle, Probe, World},
};
use crate::{
    ItemId, Point,
    pcb::{Track, elastic::Topology},
};
use std::{collections::VecDeque, sync::Arc};

type Moves = Vec<(ItemId, Vec<Track>)>;
const MAX_CHOICES: usize = 96;
const MAX_DECISIONS: usize = 48;

#[derive(Clone)]
struct State {
    pushers: Vec<Pusher>,
    pending: VecDeque<(u32, usize)>,
    queued: FxSet<u32>,
    moved: Moves,
}
struct Search {
    topology: Arc<Topology>,
    remaining: usize,
    limited: bool,
    blocked: Vec<u32>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Blocked,
    Limit,
}

impl Search {
    fn movable(&self, world: &World, index: u32) -> bool {
        world.obstacles[index as usize].track.is_some()
    }

    fn append(&self, world: &World, state: &mut State, pusher: Pusher, depth: usize) -> bool {
        let excluded = [pusher.net];
        let probe = Probe {
            hw: pusher.hw,
            net: pusher.net,
            excl: &excluded,
        };
        for hit in world.seg_collisions(pusher.a, pusher.b, &probe) {
            match hit {
                Hit::Ob(index) if self.movable(world, index) && !world.removed.contains(&index) => {
                    if state.queued.insert(index) {
                        if depth > 1 {
                            state.pending.push_front((index, depth));
                        } else {
                            state.pending.push_back((index, depth));
                        }
                    } else if depth > 1 {
                        // Resolve the newly found dependency before unrelated root
                        // collisions, without increasing an existing root depth.
                        if let Some(at) = state
                            .pending
                            .iter()
                            .position(|(pending, _)| *pending == index)
                        {
                            let (_, previous) = state.pending.remove(at).unwrap();
                            state.pending.push_front((index, previous.min(depth)));
                        }
                    }
                }
                _ => return false,
            }
        }
        true
    }

    fn apply(
        &self,
        world: &mut World,
        state: &State,
        index: u32,
        depth: usize,
        indices: &[u32],
        points: Vec<Point>,
    ) -> Option<State> {
        if indices.is_empty() || indices.iter().any(|index| world.removed.contains(index)) {
            return None;
        }
        let points = chain::assign(points, indices.len())?;
        let obstacle = world.obstacles[index as usize].clone();
        let track = obstacle.track.as_ref()?;
        let pieces: Vec<_> = points
            .windows(2)
            .map(|w| Track {
                a: w[0],
                b: w[1],
                ..track.clone()
            })
            .collect();
        if pieces.is_empty() {
            return None;
        }
        let mut next = state.clone();
        for index in indices {
            world.removed.insert(*index);
        }
        for (ordinal, piece) in pieces.iter().enumerate() {
            let id = world.obstacles[indices[ordinal % indices.len()] as usize].id;
            world.added.push(Obstacle {
                core: Core::Seg(V::of(piece.a), V::of(piece.b)),
                r: obstacle.r,
                net: obstacle.net,
                key: obstacle.key,
                kind: ObKind::Track,
                id,
                track: None,
            });
        }
        for piece in &pieces {
            let pusher = Pusher {
                a: V::of(piece.a),
                b: V::of(piece.b),
                hw: obstacle.r,
                net: obstacle.net,
            };
            if !self.append(world, &mut next, pusher, depth + 1) {
                return None;
            }
            next.pushers.push(pusher);
        }
        for (ordinal, index) in indices.iter().enumerate() {
            let id = world.obstacles[*index as usize].id?;
            next.moved.push((
                id,
                pieces
                    .iter()
                    .enumerate()
                    .filter(|(piece, _)| piece % indices.len() == ordinal)
                    .map(|(_, piece)| piece.clone())
                    .collect(),
            ));
        }
        Some(next)
    }

    fn candidate(
        &mut self,
        world: &mut World,
        state: &State,
        index: u32,
        depth: usize,
        indices: Vec<u32>,
        points: Vec<Point>,
        level: usize,
        seen: &mut FxSet<(Vec<u32>, Vec<Point>)>,
    ) -> Option<Moves> {
        if !seen.insert((indices.clone(), points.clone())) {
            return None;
        }
        if self.remaining == 0 || !engine::consume_work(points.len().max(indices.len() + 1)) {
            self.limited = true;
            return None;
        }
        self.remaining -= 1;
        let removed = world.removed.clone();
        let added = world.added.clone();
        let result = self
            .apply(world, state, index, depth, &indices, points)
            .and_then(|next| self.solve(world, next, level + 1));
        if result.is_none() {
            world.removed = removed;
            world.added = added;
        }
        result
    }

    // A failed descendant can be left in its original position instead. Feed
    // its source geometry back to the earlier choice, never into accepted edits.
    fn pin_failures(
        &mut self,
        world: &World,
        index: u32,
        start: usize,
        seen: &FxSet<(Vec<u32>, Vec<Point>)>,
        pinned: &mut Vec<u32>,
        pushers: &mut Vec<Pusher>,
    ) -> bool {
        let obstacle = &world.obstacles[index as usize];
        let mut changed = false;
        for source in self.blocked[start..].iter().copied() {
            if source == index || pinned.contains(&source) || world.removed.contains(&source) {
                continue;
            }
            let other = &world.obstacles[source as usize];
            if other.net == obstacle.net && other.net != super::world::NO_NET {
                continue;
            }
            let Some(track) = &other.track else {
                continue;
            };
            if !engine::consume_work(1) {
                self.limited = true;
                break;
            }
            let clearance = other.r + obstacle.r + world.clr(other.key, obstacle.net);
            let touches = seen.iter().any(|(_, path)| {
                path.windows(2).any(|w| {
                    super::geom::seg_seg_dist(
                        V::of(w[0]),
                        V::of(w[1]),
                        V::of(track.a),
                        V::of(track.b),
                    ) < clearance
                })
            });
            if !touches {
                continue;
            }
            if pinned.len() >= 16 {
                self.limited = true;
                break;
            }
            pinned.push(source);
            pushers.push(Pusher {
                a: V::of(track.a),
                b: V::of(track.b),
                hw: other.r,
                net: other.net,
            });
            changed = true;
        }
        changed
    }

    fn solve(&mut self, world: &mut World, mut state: State, level: usize) -> Option<Moves> {
        let mut current = None;
        while let Some((index, depth)) = state.pending.pop_front() {
            if world.removed.contains(&index) {
                continue;
            }
            if depth > MAX_DEPTH {
                self.limited = true;
                self.blocked.push(index);
                return None;
            }
            let obstacle = &world.obstacles[index as usize];
            let track = obstacle.track.as_ref()?;
            let reqs: Vec<_> = state
                .pushers
                .iter()
                .filter(|p| p.net != obstacle.net || p.net == super::world::NO_NET)
                .map(|p| (p.a, p.b, obstacle.r + p.hw + world.clr(obstacle.key, p.net)))
                .collect();
            let joints = self.topology.interior_joints(obstacle.id?).to_vec();
            let nearest = anchored_detour(track.a, track.b, &joints, &reqs, None);
            if matches!(nearest, Some(Detour::NotNeeded)) {
                continue;
            }
            current = Some((index, depth, track.a, track.b, joints, reqs, nearest));
            break;
        }
        let Some((index, depth, a, b, joints, mut reqs, mut nearest)) = current else {
            for pusher in &state.pushers {
                let excluded = [pusher.net];
                let probe = Probe {
                    hw: pusher.hw,
                    net: pusher.net,
                    excl: &excluded,
                };
                if !world.seg_clear(pusher.a, pusher.b, &probe) {
                    return None;
                }
            }
            return Some(state.moved);
        };
        if level >= MAX_DECISIONS || self.remaining == 0 || engine::work_exhausted() {
            self.limited = true;
            self.blocked.push(index);
            return None;
        }
        let start = self.blocked.len();
        let mut seen = FxSet::default();
        let mut pinned = Vec::new();
        let mut guides = state.pushers.clone();
        let obstacle = world.obstacles[index as usize].clone();
        for pass in 0..=16 {
            if pass > 0 {
                reqs = guides
                    .iter()
                    .filter(|p| p.net != obstacle.net || p.net == super::world::NO_NET)
                    .map(|p| (p.a, p.b, obstacle.r + p.hw + world.clr(obstacle.key, p.net)))
                    .collect();
                nearest = anchored_detour(a, b, &joints, &reqs, None);
            }
            for points in alternatives(a, b, &joints, &reqs, nearest.take(), false) {
                if let Some(moves) = self.candidate(
                    world,
                    &state,
                    index,
                    depth,
                    vec![index],
                    points,
                    level,
                    &mut seen,
                ) {
                    return Some(moves);
                }
                if self.remaining == 0 || engine::work_exhausted() {
                    break;
                }
            }
            if self.remaining == 0 || engine::work_exhausted() {
                self.limited = true;
                break;
            }
            // Reconsider a failed dependency before spending work on unrelated
            // connected-window searches that would repeat the same collision.
            if self.pin_failures(world, index, start, &seen, &mut pinned, &mut guides) {
                continue;
            }
            let mut result = None;
            chain::visit_displacements(world, index, &guides, false, |world, indices, points| {
                result = self.candidate(
                    world, &state, index, depth, indices, points, level, &mut seen,
                );
                result.is_some() || self.remaining == 0 || engine::work_exhausted()
            });
            if result.is_some() {
                return result;
            }
            if self.remaining == 0 || engine::work_exhausted() {
                self.limited = true;
                break;
            }
            if !self.pin_failures(world, index, start, &seen, &mut pinned, &mut guides) {
                break;
            }
        }
        self.blocked.push(index);
        None
    }
}

/// No caller overlay is modified unless a complete alternative is accepted.
pub(super) fn solve(world: &mut World, roots: &[Pusher]) -> Result<Moves, Failure> {
    let topology =
        engine::cached_topology(world.doc).unwrap_or_else(|| Arc::new(Topology::new(world.doc)));
    let mut search = Search {
        topology,
        remaining: MAX_CHOICES,
        limited: false,
        blocked: Vec::new(),
    };
    let mut state = State {
        pushers: roots.to_vec(),
        pending: VecDeque::new(),
        queued: FxSet::default(),
        moved: Vec::new(),
    };
    for root in roots {
        if !search.append(world, &mut state, *root, 1) {
            return Err(Failure::Blocked);
        }
    }
    let removed = world.removed.clone();
    let added = world.added.clone();
    if let Some(moves) = search.solve(world, state, 0) {
        return Ok(moves);
    }
    world.removed = removed;
    world.added = added;
    Err(
        if search.limited || search.remaining == 0 || engine::work_exhausted() {
            Failure::Limit
        } else {
            Failure::Blocked
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::{RouteMode, shove};
    use super::*;
    use crate::{Command, DocKind, Document, IdItem, Item, LayerId, Session, units::mm};
    fn p(x: f64, y: f64) -> Point {
        Point::new(mm(x), mm(y))
    }
    fn track(net: &str, a: Point, b: Point) -> Item {
        Item::Track(Track {
            a,
            b,
            width: mm(0.25),
            layer: LayerId(0),
            net: net.into(),
            locked: false,
        })
    }
    fn fixture(blocked: bool) -> Document {
        let mut items = vec![
            Item::Board(Default::default()),
            track("LEFT", p(-10., 0.), p(-2., 0.)),
            track("RIGHT", p(2., 0.), p(10., 0.)),
            track("LOWER_STOP", p(-6.2, -0.6), p(-5.8, -0.6)),
            track("UPPER_STOP", p(5.8, 1.), p(6.2, 1.)),
        ];
        for (net, at) in [
            ("LEFT", p(-10., 0.)),
            ("LEFT", p(-2., 0.)),
            ("RIGHT", p(2., 0.)),
            ("RIGHT", p(10., 0.)),
        ] {
            items.push(Item::Via(crate::pcb::Via {
                pos: at,
                diameter: mm(0.6),
                drill: mm(0.3),
                from_layer: LayerId(0),
                to_layer: LayerId(1),
                net: net.into(),
                locked: false,
            }));
        }
        if blocked {
            items.push(track("OTHER_LEFT_STOP", p(-6.2, 1.), p(-5.8, 1.)));
            // This fixture must remain physically blocked even when a larger
            // detour around an otherwise immovable neighbor becomes available.
            items.push(Item::Via(crate::pcb::Via {
                pos: p(-6., 0.2),
                diameter: mm(0.6),
                drill: mm(0.3),
                from_layer: LayerId(0),
                to_layer: LayerId(1),
                net: "FIXED_STOP".into(),
                locked: true,
            }));
        }
        Document::from_item_list(DocKind::Pcb, items)
    }
    fn roots(world: &mut World) -> Vec<Pusher> {
        let net = world.net_id("ROOT");
        [(-7., -5.), (5., 7.)]
            .into_iter()
            .map(|(a, b)| Pusher {
                a: V::of(p(a, 0.2)),
                b: V::of(p(b, 0.2)),
                hw: mm(0.25) as f64 / 2.,
                net,
            })
            .collect()
    }
    fn add_marker(world: &mut World) {
        let net = world.net_id("MARKER");
        let key = world.track_key(net);
        world.added.push(Obstacle {
            core: Core::Seg(V::of(p(90., 90.)), V::of(p(91., 90.))),
            r: 0.,
            net,
            key,
            kind: ObKind::Track,
            id: None,
            track: None,
        });
        // A pre-existing removed source record is part of the caller's branch.
        let stop_net = world.net_id("UPPER_STOP");
        let stop = world
            .obstacles
            .iter()
            .position(|o| o.net == stop_net)
            .unwrap();
        world.removed.insert(stop as u32);
    }
    fn checked_history(source: &Document, plan: crate::pcb::drag::DragResult) {
        let original = crate::io::document_to_string(source).unwrap();
        let mut session = Session::with_document(source.clone());
        let delta = session
            .execute_with_via_checks(
                Command::Route {
                    replace: plan.replace,
                    delete: plan.delete,
                    add: plan.add,
                },
                true,
            )
            .unwrap();
        let ids: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
        let errors: Vec<_> = crate::pcb::drc::run_near(session.document(), &ids)
            .into_iter()
            .filter(|v| v.severity == crate::pcb::drc::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        let after = crate::io::document_to_string(session.document()).unwrap();
        assert_ne!(after, original);
        for (id, item) in source.items() {
            if matches!(item, Item::Via(_) | Item::Footprint(_))
                || matches!(item,Item::Track(t)if t.net.contains("STOP")||t.layer==LayerId(1))
            {
                assert_eq!(
                    session.document().get(id),
                    Some(item),
                    "unrelated or fixed track changed"
                );
            }
        }
        for net in ["LEFT", "RIGHT"] {
            let original = source
                .items()
                .find_map(|(_, item)| match item {
                    Item::Track(t) if t.net == net => Some(t),
                    _ => None,
                })
                .unwrap();
            for endpoint in [original.a, original.b] {
                assert!(session.document().items().any(|(_,item)|matches!(item,Item::Track(t)if t.net==net&&(t.a==endpoint||t.b==endpoint))),"fixed source endpoint detached");
            }
        }
        session.undo().unwrap();
        assert_eq!(
            crate::io::document_to_string(session.document()).unwrap(),
            original
        );
        session.redo().unwrap();
        assert_eq!(
            crate::io::document_to_string(session.document()).unwrap(),
            after
        );
        let reopened = crate::io::document_from_str(DocKind::Pcb, &after).unwrap();
        assert_eq!(crate::io::document_to_string(&reopened).unwrap(), after);
    }
    #[test]
    fn checked_group_edit_reconsiders_independent_corridors_at_all_quarter_turns() {
        for turns in 0..4 {
            for reverse in [false, true] {
                let rotate = |mut point: Point| {
                    for _ in 0..turns {
                        point = Point::new(-point.y, point.x);
                    }
                    point
                };
                let mut items: Vec<_> = fixture(false)
                    .items()
                    .map(|(_, item)| item.clone())
                    .collect();
                items.extend([
                    track("ROOT", p(-7., 2.), p(-5., 2.)),
                    track("ROOT", p(5., 2.), p(7., 2.)),
                ]);
                let mut other = track("OTHER_LAYER", p(-7., 0.2), p(-5., 0.2));
                if let Item::Track(t) = &mut other {
                    t.layer = LayerId(1);
                }
                items.push(other);
                for item in &mut items {
                    match item {
                        Item::Track(t) => {
                            t.a = rotate(t.a);
                            t.b = rotate(t.b);
                            if reverse {
                                std::mem::swap(&mut t.a, &mut t.b);
                            }
                        }
                        Item::Via(v) => v.pos = rotate(v.pos),
                        _ => {}
                    }
                }
                let source = Document::from_item_list(DocKind::Pcb, items);
                let displacement = rotate(p(0., -1.8));
                let replacements: Vec<_> = source
                    .items()
                    .filter_map(|(id, item)| match item {
                        Item::Track(t) if t.net == "ROOT" => {
                            let mut after = t.clone();
                            after.a =
                                Point::new(after.a.x + displacement.x, after.a.y + displacement.y);
                            after.b =
                                Point::new(after.b.x + displacement.x, after.b.y + displacement.y);
                            Some(IdItem {
                                id,
                                item: Item::Track(after),
                            })
                        }
                        _ => None,
                    })
                    .collect();
                let original = crate::io::document_to_string(&source).unwrap();
                let plan = super::super::plan_track_edit(
                    &source,
                    replacements.clone(),
                    vec![],
                    RouteMode::Shove,
                )
                .unwrap();
                let repeat = super::super::plan_track_edit(
                    &source,
                    replacements.clone(),
                    vec![],
                    RouteMode::Shove,
                )
                .unwrap();
                assert_eq!(
                    format!("{plan:?}"),
                    format!("{repeat:?}"),
                    "repeated request must be deterministic"
                );
                assert!(plan.rerouted >= 2);
                checked_history(&source, plan);
                assert!(
                    super::super::plan_track_edit(
                        &source,
                        replacements,
                        vec![],
                        RouteMode::Walkaround
                    )
                    .is_err(),
                    "Avoid must not silently shove"
                );
                assert_eq!(crate::io::document_to_string(&source).unwrap(), original);
            }
        }
    }
    #[test]
    fn failed_search_restores_caller_overlay_and_the_next_preview() {
        let source = fixture(true);
        let original = crate::io::document_to_string(&source).unwrap();
        let mut world = World::build(&source, LayerId(0), &[]);
        let root = roots(&mut world);
        add_marker(&mut world);
        let removed = world.removed.clone();
        let added = format!("{:?}", world.added);
        assert!(shove::shove_many(&mut world, &root).is_err());
        assert_eq!(world.removed, removed);
        assert_eq!(format!("{:?}", world.added), added);
        let away: Vec<_> = root
            .iter()
            .map(|r| Pusher {
                a: r.a + V::of(p(0., 5.)),
                b: r.b + V::of(p(0., 5.)),
                ..*r
            })
            .collect();
        assert!(shove::shove_many(&mut world, &away).unwrap().is_empty());
        assert_eq!(world.removed, removed);
        assert_eq!(format!("{:?}", world.added), added);
        assert_eq!(crate::io::document_to_string(&source).unwrap(), original);
    }
    #[test]
    fn exhausted_allowance_does_not_reset_or_leak_a_speculative_branch() {
        let _work = engine::WorkScope::enter();
        assert!(engine::consume_work(5999));
        let source = fixture(false);
        let mut world = World::build(&source, LayerId(0), &[]);
        let root = roots(&mut world);
        let removed = world.removed.clone();
        let added = format!("{:?}", world.added);
        assert!(
            shove::shove_many(&mut world, &root)
                .unwrap_err()
                .contains("search limit")
        );
        assert!(engine::work_exhausted());
        assert!(!engine::consume_work(1));
        assert_eq!(world.removed, removed);
        assert_eq!(format!("{:?}", world.added), added);
    }
    #[test]
    fn locks_and_interior_junctions_are_not_removed_to_find_an_alternative() {
        for junction in [false, true] {
            let mut items: Vec<_> = fixture(false)
                .items()
                .map(|(_, item)| item.clone())
                .collect();
            if junction {
                items.push(track("LEFT", p(-6., 0.), p(-6., -3.)));
            } else {
                for item in &mut items {
                    if let Item::Track(t) = item {
                        if t.net == "LEFT" {
                            t.locked = true;
                        }
                    }
                }
            }
            let source = Document::from_item_list(DocKind::Pcb, items);
            let original = crate::io::document_to_string(&source).unwrap();
            let mut world = World::build(&source, LayerId(0), &[]);
            let root = roots(&mut world);
            assert!(shove::shove_many(&mut world, &root).is_err());
            assert!(world.removed.is_empty());
            assert!(world.added.is_empty());
            assert_eq!(crate::io::document_to_string(&source).unwrap(), original);
        }
    }
    #[test]
    fn alternative_and_decision_caps_are_reported_as_limits() {
        for (remaining, level) in [(0, 0), (1, 0), (MAX_CHOICES, MAX_DECISIONS)] {
            let source = fixture(false);
            let mut world = World::build(&source, LayerId(0), &[]);
            let root = roots(&mut world);
            let topology = Arc::new(Topology::new(&source));
            let mut search = Search {
                topology,
                remaining,
                limited: false,
                blocked: Vec::new(),
            };
            let mut state = State {
                pushers: root.clone(),
                pending: VecDeque::new(),
                queued: FxSet::default(),
                moved: vec![],
            };
            for r in &root {
                assert!(search.append(&world, &mut state, *r, 1));
            }
            assert!(search.solve(&mut world, state, level).is_none());
            assert!(search.limited);
            assert!(world.removed.is_empty());
            assert!(world.added.is_empty());
        }
    }
    #[test]
    fn descendants_are_resolved_before_unrelated_choices_and_exact_cap_can_succeed() {
        let source = fixture(false);
        let mut world = World::build(&source, LayerId(0), &[]);
        let root = roots(&mut world);
        let mut search = Search {
            topology: Arc::new(Topology::new(&source)),
            remaining: 3,
            limited: false,
            blocked: Vec::new(),
        };
        let mut state = State {
            pushers: root.clone(),
            pending: VecDeque::new(),
            queued: FxSet::default(),
            moved: vec![],
        };
        for r in &root {
            assert!(search.append(&world, &mut state, *r, 1));
        }
        let moves = search
            .solve(&mut world, state, 0)
            .expect("two left choices and one independent right choice suffice");
        assert_eq!(search.remaining, 0);
        assert!(!search.limited);
        assert_eq!(moves.len(), 2);
    }
    #[test]
    fn a_fixed_interior_contact_survives_reconsidering_an_earlier_corridor() {
        let mut items: Vec<_> = fixture(false)
            .items()
            .map(|(_, item)| item.clone())
            .collect();
        for (net, at) in [("LEFT", p(-9., 0.)), ("RIGHT", p(9., 0.))] {
            items.push(Item::Via(crate::pcb::Via {
                pos: at,
                diameter: mm(0.6),
                drill: mm(0.3),
                from_layer: LayerId(0),
                to_layer: LayerId(1),
                net: net.into(),
                locked: false,
            }));
        }
        items.extend([
            track("ROOT", p(-7., 2.), p(-5., 2.)),
            track("ROOT", p(5., 2.), p(7., 2.)),
        ]);
        let source = Document::from_item_list(DocKind::Pcb, items);
        let replace: Vec<_> = source
            .items()
            .filter_map(|(id, item)| match item {
                Item::Track(t) if t.net == "ROOT" => {
                    let mut t = t.clone();
                    t.a.y = mm(0.2);
                    t.b.y = mm(0.2);
                    Some(IdItem {
                        id,
                        item: Item::Track(t),
                    })
                }
                _ => None,
            })
            .collect();
        let plan =
            super::super::plan_track_edit(&source, replace, vec![], RouteMode::Shove).unwrap();
        assert_eq!(plan.rerouted, 2);
        for (net, at) in [("LEFT", p(-9., 0.)), ("RIGHT", p(9., 0.))] {
            assert!(
                plan.add
                    .iter()
                    .any(|item| matches!(item,Item::Track(t)if t.net==net&&(t.a==at||t.b==at))),
                "fixed interior contact was lost"
            );
        }
        checked_history(&source, plan);
    }
    #[test]
    fn a_failed_neighbor_can_make_an_earlier_track_take_a_larger_detour() {
        for turns in 0..4 {
            for reverse in [false, true] {
                let rotate = |mut p: Point| {
                    for _ in 0..turns {
                        p = Point::new(-p.y, p.x);
                    }
                    p
                };
                let mut items: Vec<_> = fixture(false)
                    .items()
                    .map(|(_, item)| item.clone())
                    .collect();
                items.push(track("OTHER_LEFT_STOP", p(-6.2, 1.), p(-5.8, 1.)));
                items.extend([
                    track("ROOT", p(-7., 2.), p(-5., 2.)),
                    track("ROOT", p(5., 2.), p(7., 2.)),
                ]);
                for item in &mut items {
                    match item {
                        Item::Track(t) => {
                            t.a = rotate(t.a);
                            t.b = rotate(t.b);
                            if reverse {
                                std::mem::swap(&mut t.a, &mut t.b);
                            }
                        }
                        Item::Via(v) => v.pos = rotate(v.pos),
                        _ => {}
                    }
                }
                let source = Document::from_item_list(DocKind::Pcb, items);
                let shift = rotate(p(0., -1.8));
                let replace: Vec<_> = source
                    .items()
                    .filter_map(|(id, item)| match item {
                        Item::Track(t) if t.net == "ROOT" => {
                            let mut t = t.clone();
                            t.a = Point::new(t.a.x + shift.x, t.a.y + shift.y);
                            t.b = Point::new(t.b.x + shift.x, t.b.y + shift.y);
                            Some(IdItem {
                                id,
                                item: Item::Track(t),
                            })
                        }
                        _ => None,
                    })
                    .collect();
                let plan = super::super::plan_track_edit(
                    &source,
                    replace.clone(),
                    vec![],
                    RouteMode::Shove,
                )
                .expect(
                    "the first conductor can take a larger detour around an immovable neighbor",
                );
                let repeat =
                    super::super::plan_track_edit(&source, replace, vec![], RouteMode::Shove)
                        .unwrap();
                assert_eq!(format!("{plan:?}"), format!("{repeat:?}"));
                assert_eq!(
                    plan.rerouted, 2,
                    "the failed neighboring conductors must remain untouched"
                );
                checked_history(&source, plan);
            }
        }
    }

    #[test]
    fn exhausted_dependency_reconsideration_restores_caller_copper_before_the_next_preview() {
        let mut items: Vec<_> = fixture(false)
            .items()
            .map(|(_, item)| item.clone())
            .collect();
        items.push(track("OTHER_LEFT_STOP", p(-6.2, 1.), p(-5.8, 1.)));
        let source = Document::from_item_list(DocKind::Pcb, items);
        let mut world = World::build(&source, LayerId(0), &[]);
        let roots = roots(&mut world);
        add_marker(&mut world);
        let removed = world.removed.clone();
        let added = format!("{:?}", world.added);
        {
            let _scope = engine::WorkScope::enter();
            let _slice = engine::WorkSlice::enter(100);
            let error = shove::shove_many(&mut world, &roots).unwrap_err();
            assert!(error.contains("search limit"), "{error}");
            assert_eq!(world.removed, removed);
            assert_eq!(format!("{:?}", world.added), added);
        }
        let moves = shove::shove_many(&mut world, &roots).unwrap();
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn fast_attempts_leave_work_for_dependency_reconsideration() {
        let mut items: Vec<_> = fixture(false)
            .items()
            .map(|(_, item)| item.clone())
            .collect();
        items.push(track("OTHER_LEFT_STOP", p(-6.2, 1.), p(-5.8, 1.)));
        let source = Document::from_item_list(DocKind::Pcb, items);
        let before = crate::io::document_to_string(&source).unwrap();
        let mut world = World::build(&source, LayerId(0), &[]);
        let roots = roots(&mut world);
        let _work = engine::WorkScope::enter();
        let _slice = engine::WorkSlice::enter(128);
        let moves = shove::shove_many(&mut world, &roots)
            .expect("bounded fast attempts must leave room for the larger legal detour");
        assert_eq!(moves.len(), 2);
        for root in &roots {
            let excluded = [root.net];
            assert!(world.seg_clear(
                root.a,
                root.b,
                &Probe {
                    hw: root.hw,
                    net: root.net,
                    excl: &excluded
                }
            ));
        }
        assert_eq!(crate::io::document_to_string(&source).unwrap(), before);
    }
}
