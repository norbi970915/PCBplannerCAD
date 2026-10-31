use super::*;
use crate::layer::{BOTTOM_COPPER, TOP_COPPER};
use crate::pcb::connectivity::fixtures::{doc, fp, track, via};
use crate::{Command, Session, units::mm};
fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}
fn request() -> RouteRequest {
    RouteRequest {
        net: "A".into(),
        layer: TOP_COPPER,
        start: p(0., 0.),
        end: p(10., 0.),
        width: mm(0.25),
        mode: super::super::RouteMode::Shove,
        corner: super::super::CornerStyle::Diagonal45,
        ignore: vec![],
        posture_flip: false,
    }
}
fn apply(source: &Document, result: RouteResult) -> Document {
    assert!(result.reached, "{}", result.message);
    assert!(
        result.moved.iter().all(|r| source.get(r.id).is_some()),
        "temporary candidate IDs escaped into the commit plan"
    );
    let before = crate::io::document_to_string(source).unwrap();
    let delete = result
        .moved
        .iter()
        .map(|r| r.id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let add = result
        .moved
        .into_iter()
        .map(|r| r.item)
        .chain(result.tracks.into_iter().map(Item::Track))
        .collect();
    let mut session = Session::with_document(source.clone());
    let delta = session
        .execute_with_via_checks(
            Command::Route {
                delete,
                add,
                replace: vec![],
            },
            true,
        )
        .unwrap();
    let touched: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
    assert!(
        !crate::pcb::drc::run_near(session.document(), &touched)
            .iter()
            .any(|v| v.severity == crate::pcb::drc::Severity::Error)
    );
    let after = crate::io::document_to_string(session.document()).unwrap();
    session.undo().unwrap();
    assert_eq!(
        crate::io::document_to_string(session.document()).unwrap(),
        before
    );
    session.redo().unwrap();
    assert_eq!(
        crate::io::document_to_string(session.document()).unwrap(),
        after
    );
    let reopen = crate::io::document_from_str(crate::DocKind::Pcb, &after).unwrap();
    assert_eq!(crate::io::document_to_string(&reopen).unwrap(), after);
    reopen
}
#[test]
fn off_center_endpoint_and_interior_contacts_follow_the_via_on_every_heading() {
    for contact in [0, 1] {
        for turns in 0..4 {
            for reverse in [false, true] {
                let rotate = |mut p: Point| {
                    for _ in 0..turns {
                        p = Point::new(-p.y, p.x);
                    }
                    p
                };
                let mut items = vec![
                    via(5., 0., "B"),
                    if contact == 0 {
                        track((0., 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B")
                    } else {
                        track((5.3, 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B")
                    },
                ];
                for item in &mut items {
                    match item {
                        Item::Via(v) => v.pos = rotate(v.pos),
                        Item::Track(t) => {
                            t.a = rotate(t.a);
                            t.b = rotate(t.b);
                            if reverse {
                                std::mem::swap(&mut t.a, &mut t.b);
                            }
                        }
                        _ => {}
                    }
                }
                let source = doc(items);
                let mut req = request();
                req.start = rotate(req.start);
                req.end = rotate(req.end);
                let result = super::super::route(&source, &req);
                assert!(result.moved.iter().any(|r| matches!(r.item, Item::Via(_))));
                let after = apply(&source, result);
                let via_id = after
                    .items()
                    .find(|(_, i)| matches!(i,Item::Via(v)if v.net=="B"))
                    .unwrap()
                    .0;
                let fixed = source
                    .items()
                    .find(|(_, i)| matches!(i, Item::Track(_)))
                    .unwrap()
                    .0;
                assert_eq!(after.get(fixed), source.get(fixed));
                assert!(
                    crate::pcb::connectivity::analyze(&after)
                        .connected((via_id, None), (fixed, None))
                );
            }
        }
    }
}
#[test]
fn pad_edge_and_arc_interior_contacts_anchor_the_via() {
    let arc = Item::ArcTrack(crate::pcb::ArcTrack {
        center: p(5., -3.),
        radius: mm(3.),
        start_angle: 0.,
        sweep: 180.,
        width: mm(0.2),
        layer: BOTTOM_COPPER,
        net: "B".into(),
        locked: false,
    });
    for source in [
        doc(vec![via(5., 0., "B"), arc]),
        doc(vec![
            via(5., 0., "B"),
            fp("J1", "L/TH", 4.05, 0., &[("1", "B")]),
        ]),
        doc(vec![via(5., 0., "B"), via(5., 0., "B")]),
    ] {
        let before = crate::io::document_to_string(&source).unwrap();
        let result = super::super::route(&source, &request());
        assert!(!result.moved.iter().any(|r| matches!(r.item, Item::Via(_))));
        assert_eq!(crate::io::document_to_string(&source).unwrap(), before);
    }
}
#[test]
fn a_blocked_inner_layer_prevents_the_complete_through_via_displacement() {
    let mut source = doc(vec![
        via(5., 0., "B"),
        track((0., 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B"),
    ]);
    let (bid, b) = source
        .items()
        .find(|(_, i)| matches!(i, Item::Board(_)))
        .map(|(id, i)| (id, i.clone()))
        .unwrap();
    let Item::Board(mut board) = b else {
        unreachable!()
    };
    board.stackup = crate::pcb::board::Stackup::with_copper(6);
    source.apply(&[Edit {
        id: bid,
        before: source.get(bid).cloned(),
        after: Some(Item::Board(board)),
    }]);
    let step = mm(0.635);
    let mut edits = Vec::new();
    for (dx, dy) in [
        (0, step),
        (0, -step),
        (step, 0),
        (-step, 0),
        (step, step),
        (-step, -step),
        (step, -step),
        (-step, step),
    ] {
        let at = Point::new(mm(5.) + dx, dy);
        let item = Item::Track(crate::pcb::Track {
            a: Point::new(at.x - mm(0.01), at.y),
            b: Point::new(at.x + mm(0.01), at.y),
            width: mm(0.2),
            layer: crate::layer::inner_copper(2),
            net: "BLOCK".into(),
            locked: true,
        });
        edits.push(Edit {
            id: source.alloc_id(),
            before: None,
            after: Some(item),
        });
    }
    source.apply(&edits);
    let errors: Vec<_> = crate::pcb::drc::run(&source)
        .violations
        .into_iter()
        .filter(|v| v.severity == crate::pcb::drc::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let result = super::super::route(&source, &request());
    assert!(!result.moved.iter().any(|r| matches!(r.item, Item::Via(_))));
    apply(&source, result);
}
#[test]
fn midspan_via_tethers_and_a_second_shoved_net_commit_without_temporary_ids() {
    let source = doc(vec![
        via(5., 0., "B"),
        track((0., 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B"),
        track((8., -3.), (8., 3.), 0.2, TOP_COPPER, "C"),
    ]);
    let result = super::super::route(&source, &request());
    assert!(
        result
            .moved
            .iter()
            .any(|r| matches!(&r.item,Item::Track(t)if t.net=="C")),
        "the second foreign net must actually be shoved"
    );
    assert!(result.moved.iter().any(|r| matches!(r.item, Item::Via(_))));
    let after = apply(&source, result);
    let via_id = after
        .items()
        .find(|(_, i)| matches!(i,Item::Via(v)if v.net=="B"))
        .unwrap()
        .0;
    let fixed = source
        .items()
        .find(|(_, i)| matches!(i,Item::Track(t)if t.net=="B"))
        .unwrap()
        .0;
    assert!(crate::pcb::connectivity::analyze(&after).connected((via_id, None), (fixed, None)));
}
#[test]
fn repeated_via_shoves_keep_one_tether_and_exact_complete_history() {
    let source = doc(vec![
        via(5., 0., "B"),
        track((0., 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B"),
    ]);
    let original = crate::io::document_to_string(&source).unwrap();
    let fixed = source
        .items()
        .find(|(_, i)| matches!(i, Item::Track(_)))
        .map(|(id, i)| (id, i.clone()))
        .unwrap();
    let mut session = Session::with_document(source);
    for _ in 0..8 {
        let old = session
            .document()
            .items()
            .find_map(|(_, i)| match i {
                Item::Via(v) if v.net == "B" => Some(v.pos),
                _ => None,
            })
            .unwrap();
        let mut req = request();
        req.start.y = old.y;
        req.end.y = old.y;
        let result = super::super::route(session.document(), &req);
        assert!(result.reached, "{}", result.message);
        assert!(
            result
                .moved
                .iter()
                .all(|r| session.document().get(r.id).is_some())
        );
        let new = result
            .moved
            .iter()
            .find_map(|r| match &r.item {
                Item::Via(v) => Some(v.pos),
                _ => None,
            })
            .unwrap();
        assert_ne!(new, old);
        let delete = result
            .moved
            .iter()
            .map(|r| r.id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let add = result
            .moved
            .into_iter()
            .map(|r| r.item)
            .chain(result.tracks.into_iter().map(Item::Track))
            .collect();
        let delta = session
            .execute_with_via_checks(
                Command::Route {
                    delete,
                    add,
                    replace: vec![],
                },
                true,
            )
            .unwrap();
        let touched: Vec<_> = delta.upserted.iter().map(|r| r.id).collect();
        assert!(
            !crate::pcb::drc::run_near(session.document(), &touched)
                .iter()
                .any(|v| v.severity == crate::pcb::drc::Severity::Error)
        );
        let new_id = session
            .document()
            .items()
            .find(|(_, i)| matches!(i,Item::Via(v)if v.net=="B"))
            .unwrap()
            .0;
        assert!(
            crate::pcb::connectivity::analyze(session.document())
                .connected((new_id, None), (fixed.0, None))
        );
        assert_eq!(session.document().get(fixed.0), Some(&fixed.1));
        assert_eq!(
            session
                .document()
                .items()
                .filter(|(_, i)| matches!(i,Item::Track(t)if t.net=="B"))
                .count(),
            2,
            "old tethers must be stretched rather than accumulated"
        );
    }
    let final_state = crate::io::document_to_string(session.document()).unwrap();
    for _ in 0..8 {
        session.undo().unwrap();
    }
    assert_eq!(
        crate::io::document_to_string(session.document()).unwrap(),
        original
    );
    for _ in 0..8 {
        session.redo().unwrap();
    }
    assert_eq!(
        crate::io::document_to_string(session.document()).unwrap(),
        final_state
    );
}

#[test]
fn scoped_via_clearance_drives_collision_selection_and_displacement() {
    let mut source = doc(vec![via(5., 0.8, "B")]);
    let (bid, b) = source
        .items()
        .find(|(_, i)| matches!(i, Item::Board(_)))
        .map(|(id, i)| (id, i.clone()))
        .unwrap();
    let Item::Board(mut b) = b else {
        unreachable!()
    };
    b.rules.insert(
        0,
        crate::pcb::board::Rule {
            name: "Via clearance".into(),
            enabled: true,
            kind: crate::pcb::board::RuleKind::Clearance {
                first: "IsVia".into(),
                second: "InNet('A')".into(),
                min: mm(0.8),
            },
        },
    );
    source.apply(&[Edit {
        id: bid,
        before: source.get(bid).cloned(),
        after: Some(Item::Board(b)),
    }]);
    let result = super::super::route(&source, &request());
    assert!(
        result.moved.iter().any(|r| matches!(r.item, Item::Via(_))),
        "a collision beyond the net-class radius must still be handled"
    );
    apply(&source, result);
}

#[test]
fn blind_and_buried_vias_reconnect_only_their_defined_copper_span() {
    for buried in [false, true] {
        let from = if buried {
            crate::layer::inner_copper(1)
        } else {
            TOP_COPPER
        };
        let to = crate::layer::inner_copper(3);
        let mut v = via(5., 0., "B");
        if let Item::Via(v) = &mut v {
            v.from_layer = from;
            v.to_layer = to;
        }
        let source_items = vec![
            v,
            track((0., 0.), (10., 0.), 0.2, from, "B"),
            track((0., 0.), (10., 0.), 0.2, to, "B"),
            track((0., 0.), (10., 0.), 0.2, BOTTOM_COPPER, "B"),
        ];
        let mut source = doc(source_items);
        let (bid, b) = source
            .items()
            .find(|(_, i)| matches!(i, Item::Board(_)))
            .map(|(id, i)| (id, i.clone()))
            .unwrap();
        let Item::Board(mut b) = b else {
            unreachable!()
        };
        b.stackup = crate::pcb::board::Stackup::with_copper(6);
        b.via_types.push(crate::pcb::board::ViaType {
            name: "Defined span".into(),
            from_layer: from,
            to_layer: to,
            diameter: mm(0.6),
            drill: mm(0.3),
        });
        source.apply(&[Edit {
            id: bid,
            before: source.get(bid).cloned(),
            after: Some(Item::Board(b)),
        }]);
        let mut req = request();
        req.layer = crate::layer::inner_copper(2);
        let result = super::super::route(&source, &req);
        assert!(
            result
                .moved
                .iter()
                .any(|r| matches!(&r.item,Item::Via(v)if v.from_layer==from&&v.to_layer==to))
        );
        assert!(
            !result
                .moved
                .iter()
                .any(|r| matches!(&r.item,Item::Track(t)if t.layer==BOTTOM_COPPER))
        );
        let after = apply(&source, result);
        let vid = after
            .items()
            .find(|(_, i)| matches!(i, Item::Via(_)))
            .unwrap()
            .0;
        let connectivity = crate::pcb::connectivity::analyze(&after);
        for (id, item) in source.items() {
            if let Item::Track(t) = item {
                assert_eq!(after.get(id), Some(item));
                assert_eq!(
                    connectivity.connected((vid, None), (id, None)),
                    t.layer != BOTTOM_COPPER
                );
            }
        }
    }
}
