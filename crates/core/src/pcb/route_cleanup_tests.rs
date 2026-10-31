use super::*;
use crate::document::Edit;
use crate::pcb::connectivity::fixtures::{doc, track, via};
use crate::{DocKind, Item, Point, Session, layer, units::mm};
fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}
fn snapshot(d: &Document) -> String {
    crate::io::document_to_string(d).unwrap()
}
fn fixture() -> Document {
    doc(vec![
        via(0., 0., "N"),
        track((0., 0.), (1., 1.), 0.25, layer::TOP_COPPER, "N"),
        track((1., 1.), (2., 1.), 0.25, layer::TOP_COPPER, "N"),
        track((2., 1.), (3., 0.), 0.25, layer::TOP_COPPER, "N"),
    ])
}
fn root() -> Item {
    track((3., 0.), (4., 0.), 0.25, layer::TOP_COPPER, "N")
}
fn request() -> RoutingCleanup {
    RoutingCleanup {
        protect: vec![0],
        preserve_nets: vec![],
        corner: CornerStyle::Any,
    }
}
fn route() -> Command {
    Command::Route {
        delete: vec![],
        replace: vec![],
        add: vec![root()],
    }
}
#[test]
fn committing_a_route_cleans_an_old_bow_without_moving_new_routing_or_fixed_vias() {
    let source = fixture();
    let before = snapshot(&source);
    let fixed = source
        .items()
        .find(|(_, i)| matches!(i, Item::Via(_)))
        .map(|(id, i)| (id, i.clone()))
        .unwrap();
    let planned = prepare(&source, route(), &request()).unwrap();
    let mut session = Session::with_document(source.clone());
    let delta = session.execute_with_via_checks(planned, true).unwrap();
    let tracks: Vec<_> = session
        .document()
        .items()
        .filter_map(|(_, i)| match i {
            Item::Track(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        tracks.len(),
        2,
        "obsolete bends should be cleaned as part of the routing transaction"
    );
    assert!(
        session.document().items().any(|(_, i)| i == &root()),
        "the reviewed new route must remain exact"
    );
    assert_eq!(session.document().get(fixed.0), Some(&fixed.1));
    assert!(tracks.iter().any(|t| t.a == p(0., 0.) && t.b == p(3., 0.)));
    let ids: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
    assert!(
        !crate::pcb::drc::run_near(session.document(), &ids)
            .iter()
            .any(|v| v.severity == crate::pcb::drc::Severity::Error)
    );
    let after = snapshot(session.document());
    session.undo().unwrap();
    assert_eq!(snapshot(session.document()), before);
    assert!(!session.can_undo());
    session.redo().unwrap();
    assert_eq!(snapshot(session.document()), after);
    assert_eq!(
        snapshot(&crate::io::document_from_str(DocKind::Pcb, &after).unwrap()),
        after
    );
    assert_eq!(snapshot(&source), before);
}

fn execute(source: &Document, command: Command, request: &RoutingCleanup) -> Document {
    let before = snapshot(source);
    let prepared = prepare(source, command, request).unwrap();
    let mut session = Session::with_document(source.clone());
    let delta = session.execute_with_via_checks(prepared, true).unwrap();
    let ids: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
    let errors: Vec<_> = crate::pcb::drc::run_near(session.document(), &ids)
        .into_iter()
        .filter(|v| v.severity == crate::pcb::drc::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let after = snapshot(session.document());
    session.undo().unwrap();
    assert_eq!(snapshot(session.document()), before);
    session.redo().unwrap();
    assert_eq!(snapshot(session.document()), after);
    assert_eq!(
        snapshot(&crate::io::document_from_str(DocKind::Pcb, &after).unwrap()),
        after
    );
    session.document().clone()
}
#[test]
fn cleanup_respects_any_angle_and_both_endpoint_orders() {
    for angled in [false, true] {
        for turn in 0..4 {
            for reverse in [false, true] {
                let transform = |mut p: Point| {
                    if angled {
                        p = Point::new(p.x * 3 / 5 - p.y * 4 / 5, p.x * 4 / 5 + p.y * 3 / 5);
                    }
                    for _ in 0..turn {
                        p = Point::new(-p.y, p.x);
                    }
                    p
                };
                let mut items: Vec<_> = fixture()
                    .items()
                    .filter(|(_, i)| !matches!(i, Item::Board(_)))
                    .map(|(_, i)| i.clone())
                    .collect();
                let mut root = root();
                for item in items.iter_mut().chain(std::iter::once(&mut root)) {
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
                        add: vec![root.clone()],
                    },
                    &request(),
                );
                assert_eq!(
                    out.items()
                        .filter(|(_, i)| matches!(i, Item::Track(_)))
                        .count(),
                    2
                );
                assert!(out.items().any(|(_, i)| i == &root));
            }
        }
    }
}
#[test]
fn new_same_net_junctions_and_foreign_obstructions_are_not_created_by_shortcuts() {
    for same_net in [false, true] {
        let mut source = fixture();
        let obstacle = if same_net {
            track((1.4, -0.1), (1.6, -0.1), 0.25, layer::TOP_COPPER, "N")
        } else {
            via(1.5, 0., "BLOCK")
        };
        let id = source.alloc_id();
        source.apply(&[Edit {
            id,
            before: None,
            after: Some(obstacle.clone()),
        }]);
        let after = execute(&source, route(), &request());
        assert_eq!(after.get(id), Some(&obstacle));
        if same_net {
            let fixed = source
                .items()
                .find(|(_, i)| matches!(i,Item::Via(v)if v.net=="N"))
                .unwrap()
                .0;
            assert!(
                !crate::pcb::elastic::net_connectivity(&after).connected((fixed, None), (id, None)),
                "cleanup must not join a previously unrelated net island"
            );
        }
        assert!(
            !after
                .items()
                .any(|(_, i)| matches!(i,Item::Track(t)if t.a==p(0.,0.)&&t.b==p(3.,0.))),
            "a blocked shortcut was accepted"
        );
    }
}
#[test]
fn keepouts_can_reject_cleanup_without_rejecting_the_requested_route() {
    let mut source = fixture();
    let mut keepout =
        crate::pcb::connectivity::fixtures::zone(1.1, -0.5, 1.9, 0.7, layer::TOP_COPPER, "");
    if let Item::Zone(z) = &mut keepout {
        z.keepout = true;
        z.fill.clear();
    }
    let keepout_id = source.alloc_id();
    source.apply(&[Edit {
        id: keepout_id,
        before: None,
        after: Some(keepout),
    }]);
    let after = execute(&source, route(), &request());
    for (id, item) in source.items() {
        assert_eq!(after.get(id), Some(item));
    }
    assert!(after.items().any(|(_, i)| i == &root()));
}
#[test]
fn locks_nearby_unrelated_routes_and_other_layers_keep_their_source_geometry() {
    let mut source = fixture();
    let mut edits = Vec::new();
    for item in [
        track((0., 2.), (1., 3.), 0.25, layer::TOP_COPPER, "N"),
        track((1., 3.), (2., 2.), 0.25, layer::TOP_COPPER, "N"),
        track((0., 0.), (1., 1.), 0.25, layer::BOTTOM_COPPER, "N"),
    ] {
        edits.push(Edit {
            id: source.alloc_id(),
            before: None,
            after: Some(item),
        });
    }
    source.apply(&edits);
    let untouched: Vec<_> = edits.iter().map(|e| e.id).collect();
    let after = execute(&source, route(), &request());
    for id in untouched {
        assert_eq!(after.get(id), source.get(id));
    }
    let mut source = fixture();
    let (id, item) = source
        .items()
        .find(|(_, i)| matches!(i,Item::Track(t)if t.a==p(1.,1.)))
        .map(|(id, i)| (id, i.clone()))
        .unwrap();
    let Item::Track(mut t) = item.clone() else {
        panic!()
    };
    t.locked = true;
    source.apply(&[Edit {
        id,
        before: Some(item),
        after: Some(Item::Track(t)),
    }]);
    let after = execute(&source, route(), &request());
    for (id, item) in source.items() {
        assert_eq!(after.get(id), Some(item));
    }
}
#[test]
fn configured_lengths_and_pairs_and_temporary_pair_intent_keep_their_bends() {
    for kind in 0..3 {
        let mut source = fixture();
        let mut req = request();
        let (bid, item) = source
            .items()
            .find(|(_, i)| matches!(i, Item::Board(_)))
            .map(|(id, i)| (id, i.clone()))
            .unwrap();
        let Item::Board(mut b) = item.clone() else {
            panic!()
        };
        match kind {
            0 => b.rules.push(crate::pcb::Rule {
                name: "Keep length".into(),
                enabled: true,
                kind: crate::pcb::RuleKind::Length {
                    scope: "InNet('N')".into(),
                    min: 0,
                    max: mm(100.),
                },
            }),
            1 => b.diff_pairs.push(crate::pcb::board::DiffPair {
                name: "Pair".into(),
                positive: "N".into(),
                negative: "N2".into(),
            }),
            _ => req.preserve_nets.push("N".into()),
        }
        source.apply(&[Edit {
            id: bid,
            before: Some(item),
            after: Some(Item::Board(b)),
        }]);
        let after = execute(&source, route(), &req);
        for (id, item) in source.items() {
            assert_eq!(after.get(id), Some(item));
        }
    }
}
#[test]
fn repeated_routing_keeps_a_bounded_piece_count_and_one_undo_per_click() {
    let source = doc(vec![
        via(0., 0., "N"),
        track((0., 0.), (1., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let initial = snapshot(&source);
    let mut session = Session::with_document(source);
    for x in 1..=12 {
        let added = track(
            (x as f64, 0.),
            (x as f64 + 1., 0.),
            0.25,
            layer::TOP_COPPER,
            "N",
        );
        let command = Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![added.clone()],
        };
        let prepared = prepare(session.document(), command, &request()).unwrap();
        let delta = session.execute_with_via_checks(prepared, true).unwrap();
        assert!(session.document().items().any(|(_, i)| i == &added));
        assert!(
            session
                .document()
                .items()
                .filter(|(_, i)| matches!(i, Item::Track(_)))
                .count()
                <= 2,
            "old collinear joins must not accumulate"
        );
        let ids: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
        assert!(
            !crate::pcb::drc::run_near(session.document(), &ids)
                .iter()
                .any(|v| v.severity == crate::pcb::drc::Severity::Error)
        );
    }
    let finished = snapshot(session.document());
    for _ in 0..12 {
        session.undo().unwrap();
    }
    assert_eq!(snapshot(session.document()), initial);
    assert!(!session.can_undo());
    for _ in 0..12 {
        session.redo().unwrap();
    }
    assert_eq!(snapshot(session.document()), finished);
}

#[test]
fn a_new_replacement_route_consumes_the_obsolete_bow_instead_of_duplicating_it() {
    let source = fixture();
    let new = track((3., 0.), (0., 0.), 0.25, layer::TOP_COPPER, "N");
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
        1
    );
    assert!(out.items().any(|(_, i)| i == &new));
}
#[test]
fn moved_foreign_bends_are_cleaned_without_leaking_candidate_ids() {
    let mut items = vec![via(0., 0., "B"), via(3., 0., "B")];
    items.extend([
        track((0., 0.), (1., 1.), 0.25, layer::TOP_COPPER, "B"),
        track((1., 1.), (2., 1.), 0.25, layer::TOP_COPPER, "B"),
        track((2., 1.), (3., 0.), 0.25, layer::TOP_COPPER, "B"),
    ]);
    let source = doc(items);
    let delete: Vec<_> = source
        .items()
        .filter_map(|(id, i)| matches!(i, Item::Track(_)).then_some(id))
        .collect();
    let mut add: Vec<_> = delete
        .iter()
        .map(|id| source.get(*id).unwrap().clone())
        .collect();
    let root = track((2., -1.5), (4., -1.5), 0.25, layer::TOP_COPPER, "A");
    add.push(root.clone());
    let command = Command::Route {
        delete,
        replace: vec![],
        add,
    };
    let mut req = request();
    req.protect = vec![3];
    let planned = prepare(&source, command.clone(), &req).unwrap();
    if let Command::Route {
        delete, replace, ..
    } = &planned
    {
        assert!(delete.iter().all(|id| source.get(*id).is_some()));
        assert!(replace.iter().all(|r| source.get(r.id).is_some()));
    } else {
        panic!()
    }
    let out = execute(&source, command, &req);
    assert_eq!(
        out.items()
            .filter(|(_, i)| matches!(i,Item::Track(t)if t.net=="B"))
            .count(),
        1
    );
    assert!(out.items().any(|(_, i)| i == &root));
}
#[test]
fn real_branch_contacts_and_one_nanometre_kinks_are_retained() {
    let mut source = fixture();
    let branch = track((2., 1.), (2., 3.), 0.25, layer::TOP_COPPER, "N");
    let id = source.alloc_id();
    source.apply(&[Edit {
        id,
        before: None,
        after: Some(branch.clone()),
    }]);
    let out = execute(&source, route(), &request());
    for (id, item) in source.items() {
        assert_eq!(out.get(id), Some(item));
    }
    let mut first = track((0., 0.), (1., 0.), 0.25, layer::TOP_COPPER, "N");
    let mut second = track((1., 0.), (3., 0.), 0.25, layer::TOP_COPPER, "N");
    if let Item::Track(t) = &mut first {
        t.b.y = 1;
    }
    if let Item::Track(t) = &mut second {
        t.a.y = 1;
    }
    let source = doc(vec![via(0., 0., "N"), first, second]);
    let out = execute(&source, route(), &request());
    for (id, item) in source.items() {
        assert_eq!(out.get(id), Some(item));
    }
}
#[test]
fn invalid_cleanup_metadata_cannot_allocate_ids_or_discard_pending_redo() {
    let mut session = Session::with_document(fixture());
    session
        .execute(Command::Add {
            items: vec![track((8., 0.), (9., 0.), 0.25, layer::TOP_COPPER, "OTHER")],
        })
        .unwrap();
    session.undo().unwrap();
    let before = snapshot(session.document());
    let mut req = request();
    req.protect = vec![99];
    assert!(prepare(session.document(), route(), &req).is_err());
    assert_eq!(snapshot(session.document()), before);
    assert!(session.can_redo());
    session.redo().unwrap();
    let mut empty = request();
    empty.protect.clear();
    let command = route();
    assert_eq!(
        format!(
            "{:?}",
            prepare(session.document(), command.clone(), &empty).unwrap()
        ),
        format!("{command:?}")
    );
}

#[test]
fn a_shortcut_cannot_cross_far_outside_the_new_route_corridor() {
    let source = doc(vec![
        via(0., 10., "N"),
        track((0., 10.), (1., 9.), 0.25, layer::TOP_COPPER, "N"),
        track((1., 9.), (9., 9.), 0.25, layer::TOP_COPPER, "N"),
        track((9., 9.), (10., 10.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let add = vec![
        track((0., 10.), (0., 0.), 0.25, layer::TOP_COPPER, "N"),
        track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
        track((10., 0.), (10., 10.), 0.25, layer::TOP_COPPER, "N"),
    ];
    let req = RoutingCleanup {
        protect: vec![0, 1, 2],
        preserve_nets: vec![],
        corner: CornerStyle::Any,
    };
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add,
        },
        &req,
    );
    for (id, item) in source.items() {
        assert_eq!(
            out.get(id),
            Some(item),
            "all changed portions must stay inside the affected route corridor"
        );
    }
}

#[test]
fn existing_pour_contacts_are_kept_and_cleanup_does_not_cut_new_foreign_plane_corridors() {
    let mut source = fixture();
    let mut own =
        crate::pcb::connectivity::fixtures::zone(0.9, 0.9, 1.1, 1.1, layer::TOP_COPPER, "N");
    if let Item::Zone(z) = &mut own {
        z.remove_islands = false;
    }
    let zid = source.alloc_id();
    source.apply(&[Edit {
        id: zid,
        before: None,
        after: Some(own),
    }]);
    let out = execute(&source, route(), &request());
    for (id, item) in source
        .items()
        .filter(|(_, i)| matches!(i, Item::Track(_) | Item::Via(_)))
    {
        assert_eq!(out.get(id), Some(item));
    }
    let via = source
        .items()
        .find(|(_, i)| matches!(i, Item::Via(_)))
        .unwrap()
        .0;
    assert!(crate::pcb::connectivity::analyze(&out).connected((via, None), (zid, None)));
    let mut source = fixture();
    let mut foreign =
        crate::pcb::connectivity::fixtures::zone(1.2, -0.2, 1.8, 0.2, layer::TOP_COPPER, "PLANE");
    if let Item::Zone(z) = &mut foreign {
        z.remove_islands = false;
    }
    let zid = source.alloc_id();
    source.apply(&[Edit {
        id: zid,
        before: None,
        after: Some(foreign),
    }]);
    let out = execute(&source, route(), &request());
    assert!(
        !out.items()
            .any(|(_, i)| matches!(i,Item::Track(t)if t.a==p(0.,0.)&&t.b==p(3.,0.)))
    );
    // If the user explicitly routes the new corridor, removing an obsolete bow
    // adds no extra copper; the normal routing transaction refills the pour.
    let replacement = track((3., 0.), (0., 0.), 0.25, layer::TOP_COPPER, "N");
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![replacement.clone()],
        },
        &request(),
    );
    assert_eq!(
        out.items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .count(),
        1
    );
    assert!(out.items().any(|(_, i)| i == &replacement));
}

#[path = "route_cleanup_expansion_tests.rs"]
mod expansion;

#[path = "route_cleanup_obstacle_tests.rs"]
mod obstacles;
