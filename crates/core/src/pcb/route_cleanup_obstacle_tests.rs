use super::*;

fn obstacle_bow() -> (Document, Item) {
    let source = doc(vec![
        via(0., 0., "N"),
        via(4., 0., "N"),
        via(2., 0., "STOP"),
        track((0., 0.), (-0.5, 1.), 0.25, layer::TOP_COPPER, "N"),
        track((-0.5, 1.), (2., 1.5), 0.25, layer::TOP_COPPER, "N"),
        track((2., 1.5), (4.5, 1.), 0.25, layer::TOP_COPPER, "N"),
        track((4.5, 1.), (4., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let new = track((4., 0.), (0., -1.5), 0.25, layer::TOP_COPPER, "N");
    (source, new)
}

#[test]
fn cleanup_can_shorten_both_sides_of_an_obstacle_in_one_safe_step() {
    let (source, new) = obstacle_bow();
    let prepared = prepare(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    )
    .unwrap();
    let repeat = prepare(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    )
    .unwrap();
    assert_eq!(format!("{prepared:?}"), format!("{repeat:?}"));
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    let tracks: Vec<_> = out
        .items()
        .filter_map(|(_, i)| match i {
            Item::Track(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(
        tracks.len(),
        3,
        "keep the safe obstacle waypoint while removing both obsolete shoulders"
    );
    assert!(out.items().any(|(_, i)| i == &new));
    assert!(tracks.iter().any(|t| t.a == p(0., 0.) && t.b == p(2., 1.5)));
    assert!(tracks.iter().any(|t| t.a == p(2., 1.5) && t.b == p(4., 0.)));
    for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
        assert_eq!(out.get(id), Some(item));
    }
}

#[test]
fn obstacle_cleanup_is_invariant_under_rotation_and_endpoint_reversal() {
    for angled in [false, true] {
        for turn in 0..4 {
            for reverse in [false, true] {
                let transform = |mut point: Point| {
                    if angled {
                        point = Point::new(
                            point.x * 3 / 5 - point.y * 4 / 5,
                            point.x * 4 / 5 + point.y * 3 / 5,
                        );
                    }
                    for _ in 0..turn {
                        point = Point::new(-point.y, point.x);
                    }
                    point
                };
                let (source, mut new) = obstacle_bow();
                let mut items: Vec<_> = source
                    .items()
                    .filter(|(_, i)| !matches!(i, Item::Board(_)))
                    .map(|(_, i)| i.clone())
                    .collect();
                for item in items.iter_mut().chain(std::iter::once(&mut new)) {
                    match item {
                        Item::Track(t) => {
                            t.a = transform(t.a);
                            t.b = transform(t.b);
                            if reverse {
                                std::mem::swap(&mut t.a, &mut t.b);
                            }
                        }
                        Item::Via(v) => v.pos = transform(v.pos),
                        _ => {}
                    }
                }
                let source = doc(items);
                let out = execute(
                    &source,
                    Command::Route {
                        delete: vec![],
                        replace: vec![],
                        add: vec![new.clone()],
                    },
                    &request(),
                );
                assert_eq!(
                    out.items()
                        .filter(|(_, i)| matches!(i, Item::Track(_)))
                        .count(),
                    3
                );
                assert!(out.items().any(|(_, i)| i == &new));
                for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
                    assert_eq!(out.get(id), Some(item));
                }
            }
        }
    }
}

#[test]
fn retained_waypoints_respect_the_selected_straight_corner_modes() {
    for corner in [
        CornerStyle::Orthogonal90,
        CornerStyle::Diagonal45,
        CornerStyle::Arc45,
    ] {
        let (source, new) = obstacle_bow();
        let mut req = request();
        req.corner = corner;
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &req,
        );
        let mut old_length = 0.;
        for (_, item) in source.items() {
            if let Item::Track(t) = item {
                old_length += length(&[t.a, t.b]);
            }
        }
        let mut next_length = 0.;
        let mut count = 0;
        for (_, item) in out.items().filter(|(_, i)| *i != &new) {
            let Item::Track(t) = item else { continue };
            count += 1;
            next_length += length(&[t.a, t.b]);
            let dx = (t.b.x - t.a.x).abs();
            let dy = (t.b.y - t.a.y).abs();
            assert!(
                dx == 0 || dy == 0 || (!matches!(corner, CornerStyle::Orthogonal90) && dx == dy)
            );
        }
        assert!(count <= 4, "{corner:?}: cleanup cannot add bends");
        assert!(
            next_length + 1. < old_length,
            "{corner:?}: obstacle shoulders should shorten"
        );
        assert!(out.items().any(|(_, i)| i == &new));
    }
}

#[test]
fn alternate_cleanup_paths_cannot_join_islands_or_cross_other_copper() {
    for net in ["N", "BLOCK"] {
        let (mut source, new) = obstacle_bow();
        let obstacle = via(1., 0.5, net);
        let id = source.alloc_id();
        source.apply(&[Edit {
            id,
            before: None,
            after: Some(obstacle.clone()),
        }]);
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &request(),
        );
        for (id, item) in source.items().filter(|(_, i)| !matches!(i, Item::Track(_))) {
            assert_eq!(out.get(id), Some(item));
        }
        assert!(out.items().any(|(_, i)| i == &new));
        if net == "N" {
            let end = source
                .items()
                .find(|(_, item)| matches!(item, Item::Via(v) if v.pos == p(0., 0.)))
                .unwrap()
                .0;
            assert!(
                !crate::pcb::elastic::net_connectivity(&out).connected((end, None), (id, None))
            );
        }
    }
}

#[test]
fn keepouts_remain_effective_when_a_different_safe_waypoint_is_available() {
    let (mut source, new) = obstacle_bow();
    let mut zone =
        crate::pcb::connectivity::fixtures::zone(0.8, 0.45, 1.2, 0.85, layer::TOP_COPPER, "");
    if let Item::Zone(z) = &mut zone {
        z.keepout = true;
        z.fill.clear();
    }
    let id = source.alloc_id();
    source.apply(&[Edit {
        id,
        before: None,
        after: Some(zone),
    }]);
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    for (id, item) in source.items().filter(|(_, i)| !matches!(i, Item::Track(_))) {
        assert_eq!(out.get(id), Some(item));
    }
    assert!(out.items().any(|(_, i)| i == &new));
    assert_eq!(
        out.items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .count(),
        3
    );
}

#[test]
fn retained_waypoints_do_not_bypass_the_pour_contact_guard() {
    let (mut source, new) = obstacle_bow();
    let mut zone =
        crate::pcb::connectivity::fixtures::zone(1.9, 1.4, 2.1, 1.6, layer::TOP_COPPER, "N");
    if let Item::Zone(z) = &mut zone {
        z.remove_islands = false;
    }
    let id = source.alloc_id();
    source.apply(&[Edit {
        id,
        before: None,
        after: Some(zone),
    }]);
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    for (id, item) in source
        .items()
        .filter(|(_, i)| matches!(i, Item::Track(_) | Item::Via(_)))
    {
        assert_eq!(out.get(id), Some(item));
    }
    let via = source
        .items()
        .find(|(_, item)| matches!(item, Item::Via(v) if v.pos == p(0., 0.)))
        .unwrap()
        .0;
    assert!(crate::pcb::connectivity::analyze(&out).connected((via, None), (id, None)));
    assert!(out.items().any(|(_, i)| i == &new));
}

#[test]
fn moved_foreign_bends_keep_their_scoped_clearance_during_waypoint_cleanup() {
    for min in [0.2, 1.0] {
        let (mut source, _) = obstacle_bow();
        let (bid, board) = source
            .items()
            .find(|(_, i)| matches!(i, Item::Board(_)))
            .map(|(id, item)| (id, item.clone()))
            .unwrap();
        let Item::Board(mut b) = board.clone() else {
            unreachable!()
        };
        b.rules.insert(
            0,
            crate::pcb::Rule {
                name: "Obstacle spacing".into(),
                enabled: true,
                kind: crate::pcb::RuleKind::Clearance {
                    first: "InNet('N')".into(),
                    second: "InNet('STOP')".into(),
                    min: mm(min),
                },
            },
        );
        source.apply(&[Edit {
            id: bid,
            before: Some(board),
            after: Some(Item::Board(b)),
        }]);
        let old: Vec<_> = source
            .items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .map(|(id, i)| (id, i.clone()))
            .collect();
        let new = track((0., -1.5), (4., -1.5), 0.25, layer::TOP_COPPER, "ROUTING");
        let mut req = request();
        req.protect = vec![old.len()];
        let mut add: Vec<_> = old.iter().map(|(_, i)| i.clone()).collect();
        add.push(new.clone());
        let out = execute(
            &source,
            Command::Route {
                delete: old.iter().map(|(id, _)| *id).collect(),
                replace: vec![],
                add,
            },
            &req,
        );
        assert!(out.items().any(|(_, i)| i == &new));
        if min > 0.2 {
            for (_, item) in &old {
                assert!(out.items().any(|(_, i)| i == item));
            }
        } else {
            assert_eq!(
                out.items()
                    .filter(|(_, i)| matches!(i, Item::Track(_)))
                    .count(),
                3
            );
        }
        for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
            assert_eq!(out.get(id), Some(item));
        }
    }
}

#[test]
fn a_three_sided_detour_can_use_an_exact_point_on_its_old_outer_edge() {
    let source = doc(vec![
        via(0., 0., "N"),
        via(4., 0., "N"),
        via(2., 0., "STOP"),
        track((0., 0.), (0., 1.5), 0.25, layer::TOP_COPPER, "N"),
        track((0., 1.5), (4., 1.5), 0.25, layer::TOP_COPPER, "N"),
        track((4., 1.5), (4., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let new = track((4., 0.), (0., -1.5), 0.25, layer::TOP_COPPER, "N");
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    assert_eq!(
        out.items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .count(),
        3
    );
    assert!(
        out.items()
            .any(|(_, i)| matches!(i, Item::Track(t) if t.a == p(0., 0.) && t.b == p(2., 1.5)))
    );
    assert!(
        out.items()
            .any(|(_, i)| matches!(i, Item::Track(t) if t.a == p(2., 1.5) && t.b == p(4., 0.)))
    );
    assert!(out.items().any(|(_, i)| i == &new));
}

#[test]
fn a_free_angle_edge_without_an_interior_lattice_point_is_never_rounded() {
    let a = p(0., 0.);
    let left = p(0., 1.5);
    let right = Point::new(mm(4.) + 1, mm(1.5) + 1);
    let b = Point::new(mm(4.) + 1, 0);
    let make = |a, b| {
        Item::Track(Track {
            a,
            b,
            width: mm(0.25),
            layer: layer::TOP_COPPER,
            net: "N".into(),
            locked: false,
        })
    };
    let source = doc(vec![
        via(0., 0., "N"),
        via(2., 0., "STOP"),
        make(a, left),
        make(left, right),
        make(right, b),
    ]);
    let new = make(b, p(0., -1.5));
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    for (id, item) in source.items() {
        assert_eq!(out.get(id), Some(item));
    }
    assert!(out.items().any(|(_, i)| i == &new));
}
