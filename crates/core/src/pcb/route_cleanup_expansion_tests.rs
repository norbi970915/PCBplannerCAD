use super::*;

fn tracks(d: &Document) -> Vec<Track> {
    d.items()
        .filter_map(|(_, i)| match i {
            Item::Track(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn layer_scoped_length_constraints_keep_old_bends_on_the_whole_net() {
    for scope in [
        "InNet('N') and OnLayer('Top Layer')",
        "IsTrack and OnLayer('Top Layer')",
        "InNet('N') and OnLayer('Bottom Layer')",
    ] {
        let mut source = fixture();
        let extra = track((8., 0.), (9., 0.), 0.25, layer::BOTTOM_COPPER, "N");
        let id = source.alloc_id();
        source.apply(&[Edit {
            id,
            before: None,
            after: Some(extra),
        }]);
        let (id, item) = source
            .items()
            .find(|(_, i)| matches!(i, Item::Board(_)))
            .map(|(id, i)| (id, i.clone()))
            .unwrap();
        let Item::Board(mut b) = item.clone() else {
            unreachable!()
        };
        b.rules.push(crate::pcb::Rule {
            name: "Scoped length".into(),
            enabled: true,
            kind: crate::pcb::RuleKind::Length {
                scope: scope.into(),
                min: 0,
                max: mm(100.),
            },
        });
        source.apply(&[Edit {
            id,
            before: Some(item),
            after: Some(Item::Board(b)),
        }]);
        let out = execute(&source, route(), &request());
        for (id, item) in source.items() {
            assert_eq!(out.get(id), Some(item), "scope: {scope}");
        }
    }
}

#[test]
fn long_equal_length_staircases_collapse_to_the_selected_corner_style() {
    for style in [
        CornerStyle::Orthogonal90,
        CornerStyle::Diagonal45,
        CornerStyle::Arc45,
    ] {
        let mut items = vec![via(0., 0., "N")];
        let mut at = p(0., 0.);
        for step in 0..6 {
            let pitch = mm(if step == 0 { 0.5 } else { 0.12 });
            let horizontal = at.translated(pitch, 0);
            let next = match style {
                CornerStyle::Orthogonal90 => horizontal.translated(0, pitch),
                _ => horizontal.translated(pitch, pitch),
            };
            for (a, b) in [(at, horizontal), (horizontal, next)] {
                items.push(Item::Track(Track {
                    a,
                    b,
                    width: mm(0.2),
                    layer: layer::TOP_COPPER,
                    net: "N".into(),
                    locked: false,
                }));
            }
            at = next;
        }
        let new = Item::Track(Track {
            a: at,
            b: at.translated(mm(0.5), 0),
            width: mm(0.2),
            layer: layer::TOP_COPPER,
            net: "N".into(),
            locked: false,
        });
        let source = doc(items);
        let mut req = request();
        req.corner = style;
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &req,
        );
        assert_eq!(
            tracks(&out).len(),
            3,
            "{style:?}: one retained corner plus the protected new route"
        );
        assert!(out.items().any(|(_, i)| i == &new));
        let old_len: f64 = tracks(&source).iter().map(|t| length(&[t.a, t.b])).sum();
        let clean_len: f64 = tracks(&out)
            .iter()
            .filter(|t| Item::Track((*t).clone()) != new)
            .map(|t| length(&[t.a, t.b]))
            .sum();
        assert!(
            (old_len - clean_len).abs() < 0.01,
            "{style:?}: preserve exact routing metric"
        );
    }
}

#[test]
fn rerouting_to_the_middle_of_old_copper_consumes_only_the_obsolete_bow() {
    let source = doc(vec![
        via(0., 0., "N"),
        via(5., 0., "N"),
        track((0., 0.), (1., 1.), 0.25, layer::TOP_COPPER, "N"),
        track((1., 1.), (3., 1.), 0.25, layer::TOP_COPPER, "N"),
        track((3., 1.), (5., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let new = track((0., 0.), (2., 1.), 0.25, layer::TOP_COPPER, "N");
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        },
        &request(),
    );
    let ts = tracks(&out);
    assert!(out.items().any(|(_, i)| i == &new));
    assert!(
        !ts.iter().any(|t| t.a == p(1., 1.) || t.b == p(1., 1.)),
        "obsolete old bow must be consumed"
    );
    assert!(
        ts.iter().any(|t| t.a == p(2., 1.) && t.b == p(3., 1.)),
        "retain the original-heading continuation after the landing"
    );
    let fixed: Vec<_> = source
        .items()
        .filter(|(_, i)| matches!(i, Item::Via(_)))
        .collect();
    for (id, item) in &fixed {
        assert_eq!(out.get(*id), Some(*item));
    }
    assert!(
        crate::pcb::connectivity::analyze(&out).connected((fixed[0].0, None), (fixed[1].0, None))
    );
}

#[test]
fn cleanup_does_not_leave_unused_landing_splits_on_another_conductor() {
    let mut source = fixture();
    let original = track((5., 3.), (7., 3.), 0.25, layer::TOP_COPPER, "M");
    let id = source.alloc_id();
    source.apply(&[Edit {
        id,
        before: None,
        after: Some(original.clone()),
    }]);
    let other_root = track((6., 3.), (6., 4.), 0.25, layer::TOP_COPPER, "M");
    let mut req = request();
    req.protect.push(1);
    let out = execute(
        &source,
        Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![root(), other_root.clone()],
        },
        &req,
    );
    assert_eq!(
        out.get(id),
        Some(&original),
        "unmodified straight copper must not accumulate fragments"
    );
    assert_eq!(tracks(&out).len(), 4);
    assert!(out.items().any(|(_, i)| i == &other_root));
}

#[test]
fn shorter_two_segment_bends_can_be_rebuilt_without_adding_bends() {
    for turn in 0..4 {
        for reverse in [false, true] {
            let transform = |mut point: Point| {
                point = Point::new(point.x * 4 / 5, point.y * 4 / 5);
                for _ in 0..turn {
                    point = Point::new(-point.y, point.x);
                }
                point
            };
            let mut items = vec![
                via(0., 0., "N"),
                track((0., 0.), (2., 2.), 0.25, layer::TOP_COPPER, "N"),
                track((2., 2.), (3., 1.), 0.25, layer::TOP_COPPER, "N"),
            ];
            let mut new = track((3., 1.), (4., 1.), 0.25, layer::TOP_COPPER, "N");
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
            let mut req = request();
            req.corner = CornerStyle::Diagonal45;
            let out = execute(
                &source,
                Command::Route {
                    delete: vec![],
                    replace: vec![],
                    add: vec![new.clone()],
                },
                &req,
            );
            let ts = tracks(&out);
            assert_eq!(ts.len(), 3);
            assert!(out.items().any(|(_, i)| i == &new));
            let new_len: f64 = ts
                .iter()
                .filter(|t| Item::Track((*t).clone()) != new)
                .map(|t| length(&[t.a, t.b]))
                .sum();
            assert!((new_len - (2. + 2f64.sqrt()) * mm(0.8) as f64).abs() < 0.01);
            assert!(ts.iter().all(|t| {
                let dx = (t.b.x - t.a.x).abs();
                let dy = (t.b.y - t.a.y).abs();
                dx == 0 || dy == 0 || dx == dy
            }));
        }
    }
}

#[test]
fn root_landings_are_exact_on_rotated_reversed_and_free_angle_tracks() {
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
                let mut items = vec![
                    via(0., 0., "N"),
                    via(5., 0., "N"),
                    track((0., 0.), (1., 1.), 0.25, layer::TOP_COPPER, "N"),
                    track((1., 1.), (3., 1.), 0.25, layer::TOP_COPPER, "N"),
                    track((3., 1.), (5., 0.), 0.25, layer::TOP_COPPER, "N"),
                ];
                let mut new = track((0., 0.), (2., 1.), 0.25, layer::TOP_COPPER, "N");
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
                assert!(out.items().any(|(_, i)| i == &new));
                let ts = tracks(&out);
                assert!(
                    !ts.iter()
                        .any(|t| t.a == transform(p(1., 1.)) || t.b == transform(p(1., 1.)))
                );
                assert!(ts.iter().any(|t| (t.a == transform(p(2., 1.))
                    && t.b == transform(p(3., 1.)))
                    || (t.b == transform(p(2., 1.)) && t.a == transform(p(3., 1.)))));
                for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
                    assert_eq!(out.get(id), Some(item));
                }
            }
        }
    }
}

#[test]
fn new_routes_consume_single_segment_duplicates_and_partial_collinear_overlap() {
    for (a, b) in [(0., 3.), (0.5, 2.5), (-1., 2.)] {
        let old = track((a, 0.), (b, 0.), 0.25, layer::TOP_COPPER, "N");
        let source = doc(vec![via(a, 0., "N"), via(3., 0., "N"), old]);
        let new = track((0., 0.), (3., 0.), 0.25, layer::TOP_COPPER, "N");
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &request(),
        );
        let ts = tracks(&out);
        assert!(out.items().any(|(_, i)| i == &new));
        assert_eq!(
            ts.len(),
            if a < 0. { 2 } else { 1 },
            "{a}..{b}: redundant copper should be consumed"
        );
        if a < 0. {
            assert!(
                ts.iter().any(|t| t.a == p(-1., 0.) && t.b == p(0., 0.)),
                "retain the uncovered end exactly"
            );
        }
        for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
            assert_eq!(out.get(id), Some(item));
        }
    }
}

#[test]
fn locked_wider_and_paired_copper_is_not_erased_by_new_overlap() {
    for kind in 0..4 {
        let mut old = track((0., 0.), (3., 0.), 0.25, layer::TOP_COPPER, "N");
        if let Item::Track(t) = &mut old {
            if kind == 0 {
                t.locked = true;
            }
            if kind == 1 {
                t.width = mm(0.5);
            }
        }
        let source = doc(vec![old.clone()]);
        let id = source.items().find(|(_, i)| *i == &old).unwrap().0;
        let mut req = request();
        if kind == 2 {
            req.preserve_nets.push("N".into());
        }
        let new = if kind == 3 {
            track((0., 0.), (3., 0.), 0.25, layer::BOTTOM_COPPER, "N")
        } else {
            track((0., 0.), (3., 0.), 0.25, layer::TOP_COPPER, "N")
        };
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &req,
        );
        assert_eq!(out.get(id), Some(&old), "protected source kind {kind}");
        assert!(out.items().any(|(_, i)| i == &new));
        assert_eq!(tracks(&out).len(), 2);
    }
}

#[test]
fn failed_or_off_center_landings_leave_no_speculative_source_fragments() {
    for offset in [0, 1] {
        let old = track((0., 0.), (4., 0.), 0.25, layer::TOP_COPPER, "N");
        let source = doc(vec![old.clone()]);
        let new = Item::Track(Track {
            a: Point::new(mm(2.), offset),
            b: p(2., 1.),
            width: mm(0.25),
            layer: layer::TOP_COPPER,
            net: "N".into(),
            locked: false,
        });
        let command = Command::Route {
            delete: vec![],
            replace: vec![],
            add: vec![new.clone()],
        };
        let prepared = prepare(&source, command.clone(), &request()).unwrap();
        assert_eq!(format!("{prepared:?}"), format!("{command:?}"));
        let out = execute(&source, command, &request());
        assert_eq!(tracks(&out).len(), 2);
        for (id, item) in source.items() {
            assert_eq!(out.get(id), Some(item));
        }
    }
}

#[test]
fn arc_scopes_and_net_level_length_constraints_both_protect_approach_tracks() {
    for scope in [
        "IsArc and OnLayer('Bottom Layer')",
        "InNet('N') and not OnLayer('Top Layer')",
    ] {
        let mut source = fixture();
        let arc = Item::ArcTrack(crate::pcb::ArcTrack {
            center: p(10., 0.),
            radius: mm(1.),
            start_angle: 0.,
            sweep: 90.,
            width: mm(0.25),
            layer: layer::BOTTOM_COPPER,
            net: "N".into(),
            locked: false,
        });
        if scope.starts_with("IsArc") {
            let aid = source.alloc_id();
            source.apply(&[Edit {
                id: aid,
                before: None,
                after: Some(arc),
            }]);
        }
        let (id, item) = source
            .items()
            .find(|(_, i)| matches!(i, Item::Board(_)))
            .map(|(id, i)| (id, i.clone()))
            .unwrap();
        let Item::Board(mut b) = item.clone() else {
            unreachable!()
        };
        b.rules.push(crate::pcb::Rule {
            name: "Preserve approach".into(),
            enabled: true,
            kind: crate::pcb::RuleKind::Length {
                scope: scope.into(),
                min: 0,
                max: mm(100.),
            },
        });
        source.apply(&[Edit {
            id,
            before: Some(item),
            after: Some(Item::Board(b)),
        }]);
        let out = execute(&source, route(), &request());
        for (id, item) in source.items() {
            assert_eq!(out.get(id), Some(item), "{scope}");
        }
    }
}

#[test]
fn multiple_landings_on_one_track_keep_every_protected_endpoint_and_uncovered_end() {
    for reverse in [false, true] {
        let mut old = track((-1., 0.), (5., 0.), 0.25, layer::TOP_COPPER, "N");
        if let Item::Track(t) = &mut old {
            if reverse {
                std::mem::swap(&mut t.a, &mut t.b);
            }
        }
        let source = doc(vec![via(-1., 0., "N"), via(5., 0., "N"), old]);
        let new = vec![
            track((0., 0.), (2., 0.), 0.25, layer::TOP_COPPER, "N"),
            track((2., 0.), (4., 0.), 0.25, layer::TOP_COPPER, "N"),
        ];
        let mut req = request();
        req.protect = vec![0, 1, 1];
        let out = execute(
            &source,
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: new.clone(),
            },
            &req,
        );
        let ts = tracks(&out);
        assert_eq!(ts.len(), 4);
        for new in &new {
            assert!(out.items().any(|(_, i)| i == new));
        }
        assert!(
            ts.iter().any(|t| (t.a == p(-1., 0.) && t.b == p(0., 0.))
                || (t.b == p(-1., 0.) && t.a == p(0., 0.)))
        );
        assert!(
            ts.iter().any(|t| (t.a == p(4., 0.) && t.b == p(5., 0.))
                || (t.b == p(4., 0.) && t.a == p(5., 0.)))
        );
        let vias: Vec<_> = source
            .items()
            .filter(|(_, i)| matches!(i, Item::Via(_)))
            .collect();
        assert!(
            crate::pcb::connectivity::analyze(&out).connected((vias[0].0, None), (vias[1].0, None))
        );
    }
}

#[test]
fn repeated_overlapping_routes_do_not_accumulate_tracks_around_a_fixed_interior_via() {
    let source = doc(vec![
        via(0., 0., "N"),
        via(3., 0., "N"),
        track((0., 0.), (3., 0.), 0.25, layer::TOP_COPPER, "N"),
    ]);
    let before = snapshot(&source);
    let mut session = Session::with_document(source.clone());
    let mut states = vec![before];
    for i in 0..12 {
        let new = track(
            (1. + i as f64 * 0.05, 0.),
            (4. + i as f64 * 0.05, 0.),
            0.25,
            layer::TOP_COPPER,
            "N",
        );
        let prepared = prepare(
            session.document(),
            Command::Route {
                delete: vec![],
                replace: vec![],
                add: vec![new.clone()],
            },
            &request(),
        )
        .unwrap();
        let delta = session.execute_with_via_checks(prepared, true).unwrap();
        let ids: Vec<_> = delta.upserted.iter().map(|i| i.id).collect();
        assert!(
            crate::pcb::drc::run_near(session.document(), &ids)
                .iter()
                .all(|v| v.severity != crate::pcb::drc::Severity::Error)
        );
        assert!(session.document().items().any(|(_, item)| item == &new));
        assert_eq!(
            tracks(session.document()).len(),
            2,
            "click {i}: covered copper must not accumulate around the via"
        );
        for (id, item) in source.items().filter(|(_, i)| matches!(i, Item::Via(_))) {
            assert_eq!(session.document().get(id), Some(item));
        }
        states.push(snapshot(session.document()));
    }
    for state in states[..states.len() - 1].iter().rev() {
        session.undo().unwrap();
        assert_eq!(&snapshot(session.document()), state);
    }
    assert!(!session.can_undo());
    for state in states.iter().skip(1) {
        session.redo().unwrap();
        assert_eq!(&snapshot(session.document()), state);
    }
    let after = snapshot(session.document());
    assert_eq!(
        snapshot(&crate::io::document_from_str(DocKind::Pcb, &after).unwrap()),
        after
    );
}

#[test]
fn local_cleanup_leaves_a_large_unrelated_board_unchanged_and_is_deterministic() {
    let mut items: Vec<_> = fixture()
        .items()
        .filter(|(_, i)| !matches!(i, Item::Board(_)))
        .map(|(_, i)| i.clone())
        .collect();
    for i in 0..2048 {
        let x = 100. + (i % 64) as f64;
        let y = 100. + (i / 64) as f64;
        items.push(track(
            (x, y),
            (x + 0.4, y),
            0.25,
            layer::TOP_COPPER,
            "UNRELATED",
        ));
    }
    let source = doc(items);
    let start = std::time::Instant::now();
    let prepared = prepare(&source, route(), &request()).unwrap();
    let elapsed = start.elapsed();
    let repeat = prepare(&source, route(), &request()).unwrap();
    assert_eq!(
        format!("{prepared:?}"),
        format!("{repeat:?}"),
        "stable source ownership and candidate order"
    );
    let mut session = Session::with_document(source.clone());
    session.execute_with_via_checks(prepared, true).unwrap();
    assert_eq!(tracks(session.document()).len(), 2050);
    for (id, item) in source
        .items()
        .filter(|(_, i)| !matches!(i, Item::Track(t) if t.net == "N"))
    {
        assert_eq!(session.document().get(id), Some(item));
    }
    let simple = doc(vec![track(
        (0., 0.),
        (3., 0.),
        0.25,
        layer::TOP_COPPER,
        "N",
    )]);
    let no_op = prepare(&simple, route(), &request()).unwrap();
    assert_eq!(format!("{no_op:?}"), format!("{:?}", route()));
    println!(
        "synthetic board: 2048 unrelated tracks, cleanup prepare {elapsed:?} (debug; not desktop latency)"
    );
}
