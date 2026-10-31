use super::super::{connectivity, items::Via};
use super::*;
use crate::{Command, DocKind, Session};
fn p(x: f64, y: f64) -> Point {
    Point::new((x * 1e6).round() as i64, (y * 1e6).round() as i64)
}
fn tr(a: Point, b: Point) -> Item {
    Item::Track(Track {
        a,
        b,
        width: 250_000,
        layer: LayerId(0),
        net: "N".into(),
        locked: false,
    })
}
fn fixture() -> (Session, ItemId, Vec<ItemId>) {
    let mut s = Session::new(DocKind::Pcb);
    let delta = s
        .execute(Command::Add {
            items: vec![
                Item::Board(Default::default()),
                Item::Rect(crate::item::Rect {
                    a: p(-50., -50.),
                    b: p(70., 70.),
                    width: 100_000,
                    layer: crate::layer::BOARD_OUTLINE,
                    fill: crate::item::FillStyle::None,
                }),
                Item::Via(Via {
                    pos: p(0., 0.),
                    diameter: 600_000,
                    drill: 300_000,
                    from_layer: LayerId(0),
                    to_layer: LayerId(1),
                    net: "N".into(),
                    locked: false,
                }),
                Item::Via(Via {
                    pos: p(8., 0.),
                    diameter: 600_000,
                    drill: 300_000,
                    from_layer: LayerId(0),
                    to_layer: LayerId(1),
                    net: "N".into(),
                    locked: false,
                }),
                tr(p(0., 0.), p(2., 2.)),
                tr(p(2., 2.), p(6., 2.)),
                tr(p(6., 2.), p(8., 0.)),
            ],
        })
        .unwrap();
    let ids: Vec<_> = delta.upserted.iter().map(|e| e.id).collect();
    (s, ids[5], ids)
}
fn apply(s: &mut Session, r: DragResult) {
    s.execute(Command::Route {
        replace: r.replace,
        delete: r.delete,
        add: r.add,
    })
    .unwrap();
}
fn request(id: ItemId, y: f64, from: f64) -> TrackRequest {
    TrackRequest {
        ids: vec![id],
        point: p(4., from),
        delta: p(0., y - from),
        mode: RouteMode::Walkaround,
        corner: CornerStyle::Diagonal45,
        tolerance: 10_000,
    }
}

#[test]
fn zero_displacement_has_no_copper_edits_or_history() {
    let (session,id,_)=fixture();let before=crate::io::document_to_string(session.document()).unwrap();
    let result=drag_tracks(session.document(),&request(id,2.,2.)).unwrap();
    assert!(result.replace.is_empty()&&result.delete.is_empty()&&result.add.is_empty());
    assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
}
fn chain_path(doc: &Document) -> Vec<Point> {
    let mut points = vec![p(0., 0.)];
    let mut used = HashSet::new();
    loop {
        let at = *points.last().unwrap();
        let next = doc.items().find_map(|(id, item)| {
            if let Item::Track(t) = item {
                (!used.contains(&id) && (t.a == at || t.b == at))
                    .then_some((id, if t.a == at { t.b } else { t.a }))
            } else {
                None
            }
        });
        let Some((id, p)) = next else { break };
        used.insert(id);
        points.push(p);
        assert!(points.len() < 32, "unexpected route growth");
    }
    assert_eq!(
        used.len(),
        doc.items()
            .filter(|(_, i)| matches!(i, Item::Track(_)))
            .count(),
        "no detached loops or duplicate spurs"
    );
    points
}
#[test]
fn sliding_a_diagonal_consumes_bends_without_raising_the_via_exit() {
    for octant in 0..8 {
    let orient = |mut at: Point| {
        if octant>=4 {at.x=-at.x;}
        for _ in 0..octant%4 {at=Point::new(-at.y,at.x);}
        at
    };
    let unorient = |mut at: Point| {
        for _ in 0..octant%4 {at=Point::new(at.y,-at.x);}
        if octant>=4 {at.x=-at.x;}
        at
    };
    for shift in [0.1, 0.3, 0.59, 0.6, 0.61, 0.7, 1.0] {
        let mut s = Session::new(DocKind::Pcb);
        let points = [p(0., 0.), p(0.4, 0.), p(0.8, -0.4), p(1.4, -0.4), p(1.8, -0.8), p(3., -0.8)].map(orient);
        let via = |pos| Item::Via(Via { pos, diameter: 200_000, drill: 100_000,
            from_layer: LayerId(0), to_layer: LayerId(1), net: "N".into(), locked: false });
        let mut items = vec![Item::Board(Default::default()), via(points[0]), via(points[5])];
        items.extend(points.windows(2).map(|v| tr(v[0], v[1])));
        let delta = s.execute(Command::Add { items }).unwrap();
        let id = delta.upserted[4].id;
        let before = s.document().clone();
        let r = drag_tracks(s.document(), &TrackRequest { ids: vec![id], point: orient(p(0.6, -0.2)),
            delta: orient(p(shift, 0.)), mode: RouteMode::Walkaround, corner: CornerStyle::Diagonal45,
            tolerance: 10_000 }).unwrap();
        apply(&mut s, r);
        let path = chain_path(s.document());
        assert!(simple_path(&path)&&octilinear(&path));
        let corridor: Vec<_> = path.iter().copied().map(unorient).collect();
        assert!(corridor.iter().all(|v| v.y <= 0 && v.y >= -800_000), "invented hump at {shift}: {path:?}");
        assert!(corridor.windows(2).all(|v| v[1].x >= v[0].x && v[1].y <= v[0].y), "reversed corridor: {path:?}");
        assert!(path.len() <= points.len(), "accumulated bends: {path:?}");
        assert_eq!(*path.last().unwrap(), points[5]);
        assert_eq!(connectivity::ratsnest(s.document()).len(), 0);
        for entry in &delta.upserted[1..3] { assert_eq!(before.get(entry.id), s.document().get(entry.id)); }
        s.undo().unwrap();
        assert_eq!(s.document().items().collect::<Vec<_>>(), before.items().collect::<Vec<_>>());
        s.redo().unwrap();
        assert_eq!(chain_path(s.document()), path);
    }
    }
}
#[test]
fn slide_through_both_sides_of_old_bends_is_simple_and_keeps_fixed_vias() {
    for y in [-12., -1., 2.5, 3., 4., 7., 12.] {
        let (mut s, id, ids) = fixture();
        let before = s.document().clone();
        let plan = drag_tracks(s.document(), &request(id, y, 2.)).unwrap();
        apply(&mut s, plan);
        let path = chain_path(s.document());
        assert_eq!(path[0], p(0., 0.));
        assert_eq!(*path.last().unwrap(), p(8., 0.));
        assert!(simple_path(&path));
        assert!(octilinear(&path));
        assert!(path.len() <= 8, "unnecessary bends: {path:?}");
        for via in &ids[2..4] {
            assert_eq!(before.get(*via), s.document().get(*via));
        }
        assert_eq!(connectivity::ratsnest(s.document()).len(), 0);
    }
}
#[test]
fn repeated_back_and_forth_dragging_does_not_accumulate_bends_or_copper() {
    let (mut s, id, _) = fixture();
    let mut from = 2.;
    for _ in 0..12 {
        for y in [3., 7., -3., 2.] {
            let plan = drag_tracks(s.document(), &request(id, y, from)).unwrap();
            apply(&mut s, plan);
            let path = chain_path(s.document());
            assert!(simple_path(&path));
            assert!(path.len() <= 8);
            from = y;
        }
    }
    let path = chain_path(s.document());
    assert!((length(&path) - (2. * 8_f64.sqrt() + 4.) * 1e6).abs() < 5., "{path:?}");
}
#[test]
fn fixed_branch_and_locked_tracks_are_preserved() {
    let (mut s, id, ids) = fixture();
    let branch = s
        .execute(Command::Add {
            items: vec![tr(p(6., 2.), p(6., 6.))],
        })
        .unwrap()
        .upserted[0]
        .id;
    let fixed = s.document().get(branch).unwrap().clone();
    let far = s.document().get(ids[6]).unwrap().clone();
    let plan = drag_tracks(s.document(), &request(id, 4., 2.)).unwrap();
    apply(&mut s, plan);
    assert_eq!(s.document().get(branch), Some(&fixed));
    assert_eq!(s.document().get(ids[6]), Some(&far));
    let (mut s, id, ids) = fixture();
    let mut lock = s.document().get(ids[4]).unwrap().clone();
    if let Item::Track(t) = &mut lock {
        t.locked = true;
    }
    s.execute(Command::Replace {
        id: ids[4],
        item: lock.clone(),
    })
    .unwrap();
    let plan = drag_tracks(s.document(), &request(id, 3., 2.)).unwrap();
    apply(&mut s, plan);
    assert_eq!(s.document().get(ids[4]), Some(&lock));
}
#[test]
fn dragging_a_corner_rebuilds_a_simple_path_through_the_cursor() {
    let (mut s, id, _) = fixture();
    let req = TrackRequest {
        ids: vec![id],
        point: p(2., 2.),
        delta: p(1., 3.),
        mode: RouteMode::Walkaround,
        corner: CornerStyle::Diagonal45,
        tolerance: 100_000,
    };
    let r = drag_tracks(s.document(), &req).unwrap();
    apply(&mut s, r);
    let path = chain_path(s.document());
    assert!(path.windows(2).any(|p| on_segment(
        crate::Point::new(3_000_000, 5_000_000),
        p[0],
        p[1]
    )));
    assert!(simple_path(&path));
}
#[test]
fn blocked_chain_drag_has_no_partial_edits_and_undo_restores_the_original() {
    let (mut s, id, _) = fixture();
    let before = s.document().clone();
    let result = drag_tracks(s.document(), &request(id, 3., 2.)).unwrap();
    apply(&mut s, result);
    s.undo().unwrap();
    assert_eq!(
        s.document().items().collect::<Vec<_>>(),
        before.items().collect::<Vec<_>>()
    );
    let mut req = request(id, 80., 2.);
    req.mode = RouteMode::Walkaround;
    assert!(drag_tracks(s.document(), &req).is_err());
    assert_eq!(
        s.document().items().collect::<Vec<_>>(),
        before.items().collect::<Vec<_>>()
    );
}

#[test]
fn repeated_via_moves_replace_the_chain_without_preserving_old_detours() {
    let (mut s, _, ids) = fixture();
    let mut previous = p(0., 0.);
    for _ in 0..8 {
        for target in [p(-3., 5.), p(2., -4.), p(0., 0.)] {
            let req = super::super::drag::DragRequest {
                ids: vec![ids[2]],
                delta: Point::new(target.x - previous.x, target.y - previous.y),
                angle: 0.,
                flipped: false,
                center: Point::default(),
                mode: super::super::drag::MoveMode::PreserveAngles,
                route_mode: RouteMode::Walkaround,
                corner: CornerStyle::Diagonal45,
            };
            let result = super::super::drag::drag_footprints(s.document(), &req);
            assert_eq!(result.failure, None);
            apply(&mut s, result);
            let mut at = target;
            let mut points = vec![at];
            let mut seen = HashSet::new();
            while let Some((id, t)) = s.document().items().find_map(|(id, i)| {
                if let Item::Track(t) = i {
                    (!seen.contains(&id) && (t.a == at || t.b == at)).then_some((id, t))
                } else {
                    None
                }
            }) {
                seen.insert(id);
                at = if t.a == at { t.b } else { t.a };
                points.push(at);
                assert!(points.len() < 10);
            }
            assert_eq!(at, p(8., 0.));
            assert!(simple_path(&points));
            assert!(octilinear(&points));
            assert!(points.len() <= 4);
            assert_eq!(
                seen.len(),
                s.document()
                    .items()
                    .filter(|(_, i)| matches!(i, Item::Track(_)))
                    .count()
            );
            assert_eq!(connectivity::ratsnest(s.document()).len(), 0);
            previous = target;
        }
    }
    let total: f64 = s
        .document()
        .items()
        .filter_map(|(_, i)| {
            if let Item::Track(t) = i {
                Some(distance(t.a, t.b))
            } else {
                None
            }
        })
        .sum();
    assert_eq!(total, 8e6);
}

#[test]
fn slides_work_in_every_octant_and_project_an_off_center_grab_onto_the_track() {
    for angle in [0., 45., 90., 135., 180., 225., 270., 315.] {
        let (s, id, _) = fixture();
        let doc = Document::from_item_list(
            DocKind::Pcb,
            s.document()
                .items()
                .map(|(_, item)| item.rotated(Point::default(), angle)),
        );
        let Some(Item::Track(t)) = doc.get(id) else {
            unreachable!()
        };
        let point = Point::new((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2);
        let dx = (t.b.x - t.a.x).signum();
        let dy = (t.b.y - t.a.y).signum();
        let delta = Point::new(-dy * 3_000_000, dx * 3_000_000);
        let mut session = Session::with_document(doc);
        let req = TrackRequest {
            ids: vec![id],
            point,
            delta,
            mode: RouteMode::Walkaround,
            corner: CornerStyle::Diagonal45,
            tolerance: 1,
        };
        let result =
            drag_tracks(session.document(), &req).unwrap_or_else(|e| panic!("angle {angle}: {e}"));
        apply(&mut session, result);
        let path = chain_path(session.document());
        assert!(simple_path(&path));
        assert!(octilinear(&path));
        assert!(path.len() <= 8);
    }
    let (s, id, _) = fixture();
    let mut req = request(id, 3., 2.);
    req.point = p(4., 2.05);
    let result = drag_tracks(s.document(), &req).unwrap();
    let picked = result.replace.iter().find(|r| r.id == id).unwrap();
    assert!(matches!(&picked.item,Item::Track(t)if t.a.y==3_000_000&&t.b.y==3_000_000), "{result:?}");
}
#[test]
fn track_motion_along_its_axis_does_not_shift_the_supporting_line() {
    let (s, id, _) = fixture();
    let mut req = request(id, 2., 2.);
    req.delta = p(3., 0.);
    let result = drag_tracks(s.document(), &req).unwrap();
    assert!(matches!(&result.replace[0].item,Item::Track(t)if t.a.y==2_000_000&&t.b.y==2_000_000), "{result:?}");
}
#[test]
fn acute_spikes_reversed_segments_and_nonadjacent_touches_are_rejected() {
    assert!(!simple_path(&[p(0., 0.), p(3., 0.), p(2., 0.)]));
    assert!(!simple_path(&[p(0., 0.), p(3., 0.), p(2., 1.)]));
    assert!(!simple_path(&[p(0., 0.), p(3., 3.), p(0., 3.), p(3., 0.)]));
}

#[test]
fn a_small_slide_changes_nearby_bends_and_preserves_the_distant_route_corridor() {
    let points = [
        p(0., 0.),
        p(0., 2.),
        p(2., 4.),
        p(4., 4.),
        p(6., 6.),
        p(12., 6.),
        p(14., 4.),
        p(16., 4.),
        p(18., 2.),
        p(18., 0.),
    ];
    let (base, _, _) = fixture();
    let mut items: Vec<_> = base
        .document()
        .items()
        .filter(|(_, i)| !matches!(i, Item::Track(_)))
        .map(|(_, i)| i.clone())
        .collect();
    if let Item::Via(v) = &mut items[3] {
        v.pos = p(18., 0.);
    }
    items.extend(points.windows(2).map(|p| tr(p[0], p[1])));
    let mut s = Session::with_document(Document::from_item_list(DocKind::Pcb, items));
    let original = s.document().clone();
    let selected = ItemId(8);
    let req = TrackRequest {
        ids: vec![selected],
        point: p(9., 6.),
        delta: p(0., 0.2),
        mode: RouteMode::Walkaround,
        corner: CornerStyle::Diagonal45,
        tolerance: 1,
    };
    let result = drag_tracks(s.document(), &req).unwrap();
    assert_eq!(
        result.delete.len(),
        2,
        "only the two neighbouring bends are replaced"
    );
    apply(&mut s, result);
    for id in [
        ItemId(4),
        ItemId(5),
        ItemId(6),
        ItemId(10),
        ItemId(11),
        ItemId(12),
    ] {
        assert_eq!(
            s.document().get(id),
            original.get(id),
            "distant geometry must stay in its original corridor"
        );
    }
    let mut previous = 6.2;
    for _ in 0..8 {
        for y in [6.5, 7., 5., 6.] {
            let req = TrackRequest {
                delta: p(0., y - previous),
                point: p(9., previous),
                ..req.clone()
            };
            let result = drag_tracks(s.document(), &req).unwrap();
            apply(&mut s, result);
            let path = chain_path(s.document());
            assert!(simple_path(&path));
            assert!(path.len() <= 14);
            previous = y;
        }
    }
}

#[test]
fn a_neck_inside_a_via_is_walked_to_the_real_fixed_center() {
    let mut s=Session::new(DocKind::Pcb);
    let delta=s.execute(Command::Add{items:vec![
        Item::Board(Default::default()),
        Item::Via(Via{pos:p(0.,0.),diameter:600_000,drill:300_000,from_layer:LayerId(0),to_layer:LayerId(1),net:"N".into(),locked:false}),
        tr(p(0.,0.),p(0.,0.12)),tr(p(0.,0.12),p(0.9,1.02)),tr(p(0.9,1.02),p(0.9,3.)),
    ]}).unwrap();
    let id=delta.upserted[3].id;let Some(Item::Track(t))=s.document().get(id)else{unreachable!()};
    let chain=Topology::new(s.document()).walk(t.a,t,&HashSet::from([id]));
    assert_eq!(chain.end,p(0.,0.));assert_eq!(chain.edges,vec![delta.upserted[2].id]);
}

#[test]
fn corner_motion_near_a_via_never_folds_across_its_retained_neck() {
    let mut accepted=0;
    for dx in [-1.2,-0.9,-0.6,-0.3,0.3,0.6] {for dy in [-1.,-0.6,-0.3,0.,0.3] {
        let mut s=Session::new(DocKind::Pcb);
        let points=[p(0.,0.),p(0.,0.12),p(0.9,1.02),p(0.9,3.),p(2.,4.1),p(2.,7.)];
        let via=|pos|Item::Via(Via{pos,diameter:600_000,drill:300_000,from_layer:LayerId(0),to_layer:LayerId(1),net:"N".into(),locked:false});
        let mut items=vec![Item::Board(Default::default()),via(points[0]),via(points[5])];items.extend(points.windows(2).map(|w|tr(w[0],w[1])));
        let delta=s.execute(Command::Add{items}).unwrap();let id=delta.upserted[4].id;
        let req=TrackRequest{ids:vec![id],point:points[2],delta:p(dx,dy),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:10_000};
        if let Ok(plan)=drag_tracks(s.document(),&req){
            apply(&mut s,plan);let path=chain_path(s.document());
            assert!(simple_path(&simplify(&path)),"fold at {dx}/{dy}: {path:?}");
            assert_eq!(*path.last().unwrap(),points[5]);assert_eq!(connectivity::ratsnest(s.document()).len(),0);accepted+=1;
        }
    }}
    assert!(accepted>=20,"too many valid via departures were rejected: {accepted}");
}
#[test]
fn dragging_one_arm_of_an_unsplit_tee_keeps_the_branch_and_history(){
    for shift in [-1.,1.] {
        let mut s=Session::new(DocKind::Pcb);
        let via=|pos|Item::Via(Via{pos,diameter:600_000,drill:300_000,from_layer:LayerId(0),to_layer:LayerId(1),net:"N".into(),locked:false});
        let delta=s.execute(Command::Add{items:vec![Item::Board(Default::default()),via(p(0.,0.)),via(p(10.,0.)),via(p(5.,4.)),tr(p(0.,0.),p(10.,0.)),tr(p(5.,0.),p(5.,4.))]}).unwrap();
        let id=delta.upserted[4].id;let branch=delta.upserted[5].id;let original=s.document().clone();
        let plan=drag_tracks(s.document(),&TrackRequest{ids:vec![id],point:p(7.5,0.),delta:p(0.,shift),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:10_000}).unwrap();
        apply(&mut s,plan);
        assert_eq!(s.document().get(branch),original.get(branch));
        for entry in &delta.upserted[1..4]{assert_eq!(s.document().get(entry.id),original.get(entry.id));}
        assert_eq!(connectivity::ratsnest(s.document()).len(),0);
        assert!(s.document().items().any(|(_,item)|matches!(item,Item::Track(t)if t.a==p(0.,0.)&&t.b==p(5.,0.))));
        let after=crate::io::document_to_string(s.document()).unwrap();s.undo().unwrap();
        assert_eq!(s.document().items().collect::<Vec<_>>(),original.items().collect::<Vec<_>>());s.redo().unwrap();assert_eq!(crate::io::document_to_string(s.document()).unwrap(),after);
    }
}
#[test]
fn dragging_beside_an_interior_via_keeps_its_layer_transition_fixed(){
    let mut s=Session::new(DocKind::Pcb);
    let via=|pos|Item::Via(Via{pos,diameter:600_000,drill:300_000,from_layer:LayerId(0),to_layer:LayerId(1),net:"N".into(),locked:false});
    let bottom=Item::Track(Track{a:p(5.,0.),b:p(5.,4.),width:250_000,layer:LayerId(1),net:"N".into(),locked:false});
    let delta=s.execute(Command::Add{items:vec![Item::Board(Default::default()),via(p(0.,0.)),via(p(10.,0.)),via(p(5.,0.)),via(p(5.,4.)),tr(p(0.,0.),p(10.,0.)),bottom]}).unwrap();
    let id=delta.upserted[5].id;let original=s.document().clone();
    let plan=drag_tracks(s.document(),&TrackRequest{ids:vec![id],point:p(7.5,0.),delta:p(0.,1.),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:10_000}).unwrap();apply(&mut s,plan);
    for entry in &delta.upserted[1..5]{assert_eq!(s.document().get(entry.id),original.get(entry.id));}
    assert_eq!(s.document().get(delta.upserted[6].id),original.get(delta.upserted[6].id));assert_eq!(connectivity::ratsnest(s.document()).len(),0);
}
#[test]
fn invalid_rule_expressions_prevent_checked_routing_and_dragging(){
 use crate::units::mm;
 let (mut s,id,_)=fixture();let (bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.rules.insert(0,super::super::board::Rule{name:"Broken".into(),enabled:true,kind:super::super::board::RuleKind::Clearance{first:"InNet(".into(),second:"All".into(),min:mm(0.2)}});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=s.document().clone();
 let result=router::route(s.document(),&router::RouteRequest{net:"N".into(),layer:LayerId(0),start:p(10.,10.),end:p(12.,10.),width:mm(0.25),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false});assert!(!result.reached&&result.tracks.is_empty());assert!(result.message.contains("invalid design rules"));
 assert!(drag_tracks(s.document(),&request(id,2.2,2.)).unwrap_err().contains("invalid design rules"));assert_eq!(s.document(),&before);
}