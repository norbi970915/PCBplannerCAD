use super::*;
use crate::pcb::connectivity::fixtures::{doc, fp, track};
use crate::{Command, DocKind, Session, layer, units::mm};
const NONE: CleanupOptions = CleanupOptions {
    zero_length: false,
    duplicates: false,
    dangling: false,
    unconnected_vias: false,
    merge_collinear: false,
};
fn snapshot(d: &Document) -> String {
    crate::io::document_to_string(d).unwrap()
}
fn commit(d: &Document, r: &CleanupResult) -> Document {
    let before = snapshot(d);
    let mut session = Session::with_document(d.clone());
    if r.delete.is_empty() && r.replace.is_empty() {
        return d.clone();
    }
    let delta = session
        .execute_with_via_checks(
            Command::Route {
                delete: r.delete.clone(),
                replace: r.replace.clone(),
                add: vec![],
            },
            true,
        )
        .unwrap();
    let touched: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
    let errors: Vec<_> = crate::pcb::drc::run_near(session.document(), &touched)
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
fn collinear_cleanup_never_replaces_or_deletes_locked_source_tracks() {
    let mut locked = track((0., 0.), (5., 0.), 0.25, layer::TOP_COPPER, "N");
    if let Item::Track(t) = &mut locked {
        t.locked = true;
    }
    let d = doc(vec![
        locked,
        track((5., 0.), (7., 0.), 0.25, layer::TOP_COPPER, "N"),
        track((7., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let locked_id = d
        .items()
        .find(|(_, i)| matches!(i,Item::Track(t)if t.locked))
        .unwrap()
        .0;
    let plan = cleanup(
        &d,
        &CleanupOptions {
            merge_collinear: true,
            ..NONE
        },
    );
    assert!(!plan.delete.contains(&locked_id));
    assert!(
        plan.replace.iter().all(|r| r.id != locked_id),
        "cleanup must not propose an edit that the lock guard will reject"
    );
    assert_eq!(plan.merged, 1);
    let after = commit(&d, &plan);
    assert_eq!(after.get(locked_id), d.get(locked_id));
}
#[test]
fn cleanup_preserves_nanometre_kinks_and_parallel_offsets_exactly() {
    let mut left = track((0., 0.), (5., 0.), 0.25, layer::TOP_COPPER, "N");
    let mut right = track((5., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N");
    if let Item::Track(t) = &mut left {
        t.b.y = 1;
    }
    if let Item::Track(t) = &mut right {
        t.a.y = 1;
    }
    let d = doc(vec![left, right]);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            merge_collinear: true,
            ..NONE
        },
    );
    assert_eq!(plan.merged, 0, "a one-nanometre kink is source geometry");
    assert_eq!(snapshot(&commit(&d, &plan)), snapshot(&d));
    let straight = track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N");
    let mut offset = straight.clone();
    if let Item::Track(t) = &mut offset {
        t.a.y = 1;
        t.b.y = 1;
    }
    let d = doc(vec![straight, offset]);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            duplicates: true,
            ..NONE
        },
    );
    assert!(
        plan.delete.is_empty(),
        "parallel copper one nanometre away is not an identical or contained centerline"
    );
}
#[test]
fn rounded_end_contacts_count_even_when_the_endpoint_center_is_outside_the_pad() {
    let d = doc(vec![
        fp("J1", "L/TH", 0., 0., &[("1", "N")]),
        fp("J2", "L/TH", 10., 0., &[("1", "N")]),
        track((0.85, 0.), (9.15, 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let pads: Vec<_> = d
        .items()
        .filter_map(|(id, item)| matches!(item, Item::Footprint(_)).then_some(id))
        .collect();
    assert!(
        crate::pcb::elastic::net_connectivity(&d)
            .connected((pads[0], Some("1")), (pads[1], Some("1")))
    );
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            ..NONE
        },
    );
    assert!(
        plan.delete.is_empty(),
        "physically connected end caps must not be removed as dangling"
    );
    commit(&d, &plan);
}
#[test]
fn a_dangling_tail_must_not_delete_the_useful_span_between_interior_pad_contacts() {
    let d = doc(vec![
        fp("J1", "L/TH", 3., 0., &[("1", "N")]),
        fp("J2", "L/TH", 7., 0., &[("1", "N")]),
        track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let pads: Vec<_> = d
        .items()
        .filter_map(|(id, item)| matches!(item, Item::Footprint(_)).then_some(id))
        .collect();
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            ..NONE
        },
    );
    let after = commit(&d, &plan);
    assert!(
        crate::pcb::elastic::net_connectivity(&after)
            .connected((pads[0], Some("1")), (pads[1], Some("1"))),
        "dangling cleanup disconnected a useful interior conductor"
    );
}
#[test]
fn foreign_net_shorts_do_not_count_as_valid_cleanup_anchors() {
    let d = doc(vec![
        fp("J1", "L/TH", 0., 0., &[("1", "OTHER")]),
        fp("J2", "L/TH", 10., 0., &[("1", "OTHER")]),
        track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let id = d
        .items()
        .find(|(_, i)| matches!(i, Item::Track(_)))
        .unwrap()
        .0;
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            ..NONE
        },
    );
    assert!(
        plan.delete.contains(&id),
        "contacts to foreign copper cannot make a dangling net valid"
    );
    commit(&d, &plan);
}

#[test]
fn dangling_cleanup_trims_free_tails_without_changing_the_integer_heading() {
    for heading in 0..3 {
        for reverse in [false, true] {
            let transform = |p: Point| match heading {
                0 => p,
                1 => Point::new(p.x - p.y, p.x + p.y),
                _ => Point::new(2 * p.x - p.y, p.x + 2 * p.y),
            };
            let p = |x: f64| transform(Point::new(mm(x), 0));
            let mut items = vec![
                fp("J1", "L/TH", 3., 0., &[("1", "N")]),
                fp("J2", "L/TH", 7., 0., &[("1", "N")]),
                track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
            ];
            for item in &mut items {
                match item {
                    Item::Footprint(f) => f.pos = transform(f.pos),
                    Item::Track(t) => {
                        t.a = transform(t.a);
                        t.b = transform(t.b);
                        if reverse {
                            std::mem::swap(&mut t.a, &mut t.b);
                        }
                    }
                    _ => {}
                }
            }
            let d = doc(items);
            let plan = cleanup(
                &d,
                &CleanupOptions {
                    dangling: true,
                    ..NONE
                },
            );
            assert_eq!(
                plan.replace.len(),
                1,
                "keep the useful middle and trim its free tails"
            );
            assert!(plan.delete.is_empty());
            let Item::Track(t) = &plan.replace[0].item else {
                panic!()
            };
            assert_eq!(
                (t.a, t.b),
                if reverse {
                    (p(7.), p(3.))
                } else {
                    (p(3.), p(7.))
                }
            );
            let after = commit(&d, &plan);
            let pads: Vec<_> = after
                .items()
                .filter_map(|(id, i)| matches!(i, Item::Footprint(_)).then_some(id))
                .collect();
            assert!(
                crate::pcb::elastic::net_connectivity(&after)
                    .connected((pads[0], Some("1")), (pads[1], Some("1")))
            );
            let second = cleanup(
                &after,
                &CleanupOptions {
                    dangling: true,
                    ..NONE
                },
            );
            assert!(
                second.replace.is_empty() && second.delete.is_empty(),
                "cleanup must settle after one application"
            );
        }
    }
}
#[test]
fn trimming_and_merging_share_one_consistent_replacement_per_source() {
    let d = doc(vec![
        fp("J1", "L/TH", 3., 0., &[("1", "N")]),
        fp("J2", "L/TH", 7., 0., &[("1", "N")]),
        track((0., 0.), (5., 0.), 0.25, layer::TOP_COPPER, "N"),
        track((5., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            merge_collinear: true,
            ..NONE
        },
    );
    assert_eq!(plan.replace.len(), 1);
    let Item::Track(t) = &plan.replace[0].item else {
        panic!()
    };
    assert_eq!((t.a, t.b), (Point::new(mm(3.), 0), Point::new(mm(7.), 0)));
    let after = commit(&d, &plan);
    assert_eq!(
        after
            .items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .count(),
        1
    );
}
#[test]
fn an_already_connected_end_and_a_coprime_heading_are_never_shifted_by_trimming() {
    let d = doc(vec![
        fp("J1", "L/TH", 0., 0., &[("1", "N")]),
        fp("J2", "L/TH", 7., 0., &[("1", "N")]),
        track((0.85, 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            ..NONE
        },
    );
    let Item::Track(t) = &plan.replace[0].item else {
        panic!()
    };
    assert_eq!(t.a, Point::new(mm(0.85), 0));
    assert_eq!(t.b, Point::new(mm(7.), 0));
    commit(&d, &plan);
    let mut t = track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N");
    if let Item::Track(t) = &mut t {
        t.b.y = 1;
    }
    let d = doc(vec![
        fp("J1", "L/TH", 3., 0., &[("1", "N")]),
        fp("J2", "L/TH", 7., 0., &[("1", "N")]),
        t,
    ]);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            dangling: true,
            ..NONE
        },
    );
    assert!(
        plan.replace.is_empty() && plan.delete.is_empty(),
        "no exact interior lattice point exists on this heading"
    );
}

#[test]
fn merge_keeps_real_edge_and_arc_contacts_without_using_a_circular_pad_guess() {
    for arc in [false, true] {
        let contact = if arc {
            Item::ArcTrack(crate::pcb::ArcTrack {
                center: Point::new(mm(5.), -mm(3.)),
                radius: mm(3.),
                start_angle: 0.,
                sweep: 180.,
                width: mm(0.2),
                layer: layer::TOP_COPPER,
                net: "N".into(),
                locked: false,
            })
        } else {
            crate::pcb::connectivity::fixtures::via(5., 0.35, "N")
        };
        let d = doc(vec![
            track((0., 0.), (5., 0.), 0.25, layer::TOP_COPPER, "N"),
            track((5., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
            contact,
        ]);
        assert_eq!(
            cleanup(
                &d,
                &CleanupOptions {
                    merge_collinear: true,
                    ..NONE
                }
            )
            .merged,
            0,
            "retain an explicit joint at a physical fixed contact"
        );
    }
    let mut d = doc(vec![
        fp("J1", "L/TH", 5., 0., &[("1", "N")]),
        track((0., 1.), (5., 1.), 0.25, layer::TOP_COPPER, "N"),
        track((5., 1.), (10., 1.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let mut f = d.cached_footprint("L/TH").unwrap().clone();
    for item in &mut f.items {
        if let Item::Pad(p) = item {
            p.width = mm(4.);
            p.height = mm(0.2);
            p.shape = crate::item::PadShape::Rect;
            p.kind = crate::item::PadKind::Smd;
            p.drill = None;
        }
    }
    d.put_cached_footprint("L/TH", f);
    let plan = cleanup(
        &d,
        &CleanupOptions {
            merge_collinear: true,
            ..NONE
        },
    );
    assert_eq!(
        plan.merged, 1,
        "an unrelated wide pad must not block cleanup through its bounding circle"
    );
    commit(&d, &plan);
}

#[test]
fn reviewed_cleanup_rejects_same_id_geometry_changes_and_preserves_redo() {
    let d = doc(vec![track(
        (0., 0.),
        (10., 0.),
        0.25,
        layer::TOP_COPPER,
        "N",
    )]);
    let opts = CleanupOptions {
        dangling: true,
        ..NONE
    };
    let expected = cleanup(&d, &opts);
    assert_eq!(expected.delete.len(), 1);
    let id = expected.delete[0];
    let mut changed = d.get(id).unwrap().clone();
    if let Item::Track(t) = &mut changed {
        t.b.x = mm(20.);
    }
    let mut next = d.clone();
    next.apply(&[crate::document::Edit {
        id,
        before: d.get(id).cloned(),
        after: Some(changed),
    }]);
    let newer = cleanup(&next, &opts);
    assert_eq!(
        newer.delete, expected.delete,
        "the deletion IDs alone cannot detect this change"
    );
    assert_ne!(newer.original, expected.original);
    let mut session = Session::with_document(next);
    session
        .execute(Command::Add {
            items: vec![track((30., 0.), (31., 0.), 0.25, layer::TOP_COPPER, "N")],
        })
        .unwrap();
    session.undo().unwrap();
    let before = snapshot(session.document());
    assert!(
        cleanup_command(session.document(), &opts, &expected)
            .unwrap_err()
            .contains("changed")
    );
    assert_eq!(snapshot(session.document()), before);
    session.redo().unwrap();
    let command = cleanup_command(&d, &opts, &expected).unwrap();
    let mut accepted = Session::with_document(d.clone());
    accepted.execute(command).unwrap();
    assert!(
        !accepted
            .document()
            .items()
            .any(|(_, i)| matches!(i, Item::Track(_)))
    );
    accepted.undo().unwrap();
    assert_eq!(snapshot(accepted.document()), snapshot(&d));
}
#[test]
fn reviewed_cleanup_rechecks_locks_and_footprint_geometry() {
    let d = doc(vec![
        fp("J1", "L/TH", 3., 0., &[("1", "N")]),
        fp("J2", "L/TH", 7., 0., &[("1", "N")]),
        track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let opts = CleanupOptions {
        dangling: true,
        ..NONE
    };
    let expected = cleanup(&d, &opts);
    assert_eq!(expected.trimmed, 1);
    assert_eq!(expected.original.len(), 1);
    let mut changed = d.clone();
    let mut f = d.cached_footprint("L/TH").unwrap().clone();
    for item in &mut f.items {
        if let Item::Pad(p) = item {
            p.width = mm(20.);
            p.height = mm(20.);
        }
    }
    changed.put_cached_footprint("L/TH", f);
    assert!(cleanup_command(&changed, &opts, &expected).is_err());
    let mut changed = d.clone();
    let id = expected.original[0].id;
    let mut locked = d.get(id).unwrap().clone();
    if let Item::Track(t) = &mut locked {
        t.locked = true;
    }
    changed.apply(&[crate::document::Edit {
        id,
        before: d.get(id).cloned(),
        after: Some(locked),
    }]);
    assert!(cleanup_command(&changed, &opts, &expected).is_err());
}
#[test]
fn zero_length_copper_is_retained_if_deleting_it_would_split_useful_connections() {
    let d = doc(vec![
        fp("J1", "L/TH", 3.9, 0., &[("1", "N")]),
        fp("J2", "L/TH", 6.1, 0., &[("1", "N")]),
        track((5., 0.), (5., 0.), 0.8, layer::TOP_COPPER, "N"),
    ]);
    let pads: Vec<_> = d
        .items()
        .filter_map(|(id, i)| matches!(i, Item::Footprint(_)).then_some(id))
        .collect();
    assert!(
        crate::pcb::elastic::net_connectivity(&d)
            .connected((pads[0], Some("1")), (pads[1], Some("1")))
    );
    let plan = cleanup(
        &d,
        &CleanupOptions {
            zero_length: true,
            ..NONE
        },
    );
    assert!(
        plan.delete.is_empty(),
        "cleanup must retain a small copper bridge when connectivity cannot be preserved without it"
    );
}
#[test]
fn foreign_copper_on_two_layers_does_not_keep_an_unconnected_via() {
    let d = doc(vec![
        crate::pcb::connectivity::fixtures::via(5., 0., "UNCONNECTED"),
        track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "OTHER"),
        track((0., 0.), (10., 0.), 0.25, layer::BOTTOM_COPPER, "OTHER"),
    ]);
    let id = d
        .items()
        .find(|(_, i)| matches!(i, Item::Via(_)))
        .unwrap()
        .0;
    let plan = cleanup(
        &d,
        &CleanupOptions {
            unconnected_vias: true,
            ..NONE
        },
    );
    assert_eq!(plan.delete, vec![id]);
    assert_eq!(plan.vias, 1);
    commit(&d, &plan);
}

#[test]
fn an_unlocked_duplicate_can_be_removed_without_touching_a_locked_keeper() {
    for first_locked in [false, true] {
        let mut locked = track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N");
        if let Item::Track(t) = &mut locked {
            t.locked = true;
        }
        let open = track((0., 0.), (10., 0.), 0.25, layer::TOP_COPPER, "N");
        let d = doc(if first_locked {
            vec![locked, open]
        } else {
            vec![open, locked]
        });
        let locked_id = d
            .items()
            .find(|(_, i)| matches!(i,Item::Track(t)if t.locked))
            .unwrap()
            .0;
        let open_id = d
            .items()
            .find(|(_, i)| matches!(i,Item::Track(t)if !t.locked))
            .unwrap()
            .0;
        let plan = cleanup(
            &d,
            &CleanupOptions {
                duplicates: true,
                ..NONE
            },
        );
        assert_eq!(plan.delete, vec![open_id]);
        let after = commit(&d, &plan);
        assert_eq!(after.get(locked_id), d.get(locked_id));
    }
}

#[test]
fn connected_shove_then_cleanup_removes_protocol_splits_in_one_exact_history_step() {
    let via = crate::pcb::connectivity::fixtures::via;
    let mut items = vec![
        via(0., 0.4, "B"),
        via(20., 0.4, "B"),
        track((5., -2.), (15., -2.), 0.25, layer::TOP_COPPER, "A"),
    ];
    for x in 0..10 {
        items.push(track(
            (2. * x as f64, 0.4),
            (2. * x as f64 + 2., 0.4),
            0.25,
            layer::TOP_COPPER,
            "B",
        ));
    }
    let d = doc(items);
    let original = snapshot(&d);
    let vias: Vec<_> = d
        .items()
        .filter_map(|(id, i)| matches!(i, Item::Via(_)).then_some(id))
        .collect();
    let (root, item) = d
        .items()
        .find(|(_, i)| matches!(i,Item::Track(t)if t.net=="A"))
        .unwrap();
    let Item::Track(mut next) = item.clone() else {
        panic!()
    };
    next.a.y = 0;
    next.b.y = 0;
    let moved = crate::pcb::router::plan_track_edit(
        &d,
        vec![IdItem {
            id: root,
            item: Item::Track(next),
        }],
        vec![],
        crate::pcb::router::RouteMode::Shove,
    )
    .unwrap();
    let mut session = Session::with_document(d.clone());
    session
        .execute_with_via_checks(
            Command::Route {
                replace: moved.replace,
                delete: moved.delete,
                add: moved.add,
            },
            true,
        )
        .unwrap();
    let shoved = snapshot(session.document());
    let count = |d: &Document| {
        d.items()
            .filter(|(_, i)| matches!(i,Item::Track(t)if t.net=="B"))
            .count()
    };
    let before_count = count(session.document());
    let opts = CleanupOptions {
        merge_collinear: true,
        duplicates: true,
        ..NONE
    };
    let plan = cleanup(session.document(), &opts);
    assert!(
        plan.merged > 0,
        "remove the collinear identity splits left by the grouped replacement protocol"
    );
    assert_eq!(
        plan,
        cleanup(session.document(), &opts),
        "repeated cleanup reviews must be deterministic"
    );
    let command = cleanup_command(session.document(), &opts, &plan).unwrap();
    let delta = session.execute_with_via_checks(command, true).unwrap();
    assert!(count(session.document()) < before_count);
    let touched: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
    assert!(
        !crate::pcb::drc::run_near(session.document(), &touched)
            .iter()
            .any(|v| v.severity == crate::pcb::drc::Severity::Error)
    );
    assert!(
        crate::pcb::elastic::net_connectivity(session.document())
            .connected((vias[0], None), (vias[1], None))
    );
    for id in vias {
        assert_eq!(session.document().get(id), d.get(id));
    }
    let finished = snapshot(session.document());
    let again = cleanup(session.document(), &opts);
    assert!(again.delete.is_empty() && again.replace.is_empty());
    session.undo().unwrap();
    assert_eq!(snapshot(session.document()), shoved);
    session.undo().unwrap();
    assert_eq!(snapshot(session.document()), original);
    session.redo().unwrap();
    session.redo().unwrap();
    assert_eq!(snapshot(session.document()), finished);
}
