use crate::{io, layer, Command, Document, Item, Session};
use crate::units::mm;
use super::{board::{BoardSettings, Stackup, ViaType}, connectivity::fixtures::{doc, fp, outline, p, track, zone}, router, Side};

fn board(items: Vec<Item>) -> Document {
    let settings = BoardSettings {
        stackup: Stackup::with_copper(6),
        via_types: vec![
            ViaType { name: "Micro".into(), from_layer: layer::TOP_COPPER, to_layer: layer::inner_copper(1), diameter: mm(0.3), drill: mm(0.1) },
            ViaType { name: "Blind".into(), from_layer: layer::TOP_COPPER, to_layer: layer::inner_copper(2), diameter: mm(0.6), drill: mm(0.3) },
            ViaType { name: "Buried".into(), from_layer: layer::inner_copper(2), to_layer: layer::inner_copper(4), diameter: mm(0.6), drill: mm(0.3) },
        ],
        ..BoardSettings::default()
    };
    let document = doc(items);
    let id = document.items().find(|(_,i)|matches!(i,Item::Board(_))).unwrap().0;
    let mut session = Session::with_document(document);
    session.execute(Command::Replace { id, item: Item::Board(settings) }).unwrap();
    session.execute(Command::Add { items: vec![outline(0.,0.,30.,30.)] }).unwrap();
    session.document().clone()
}
fn snapshot(doc: &Document) -> String { io::document_to_string(doc).unwrap() }
fn place(via: super::Via) -> Command { Command::Route { delete: vec![], replace: vec![], add: vec![Item::Via(via)] } }

#[test]
fn via_guard_checks_every_crossed_layer_and_allows_layers_outside_the_actual_span() {
    let (top,bottom) = (layer::TOP_COPPER,layer::BOTTOM_COPPER);
    let (i1,i2,i4) = (layer::inner_copper(1),layer::inner_copper(2),layer::inner_copper(4));
    // Physical order includes Bottom (ID 1) after all the inner layers. Also
    // test reversed spans and a fallback spanning more than requested.
    for (from,to) in [(top,bottom),(top,i1),(top,i2),(i2,i4),(i4,i2),(i2,bottom)] {
        for blocked_layer in Stackup::with_copper(6).copper_layers() {
            let document = board(vec![track((9.,10.),(11.,10.),0.2,blocked_layer,"B")]);
            let via = router::via_between(&document,"A",p(10.,10.),from,to);
            let crosses = Stackup::with_copper(6).span(via.from_layer,via.to_layer).contains(&blocked_layer);
            let mut session = Session::with_document(document);
            let before = snapshot(session.document());
            let result = session.execute_with_via_checks(place(via),true);
            assert_eq!(result.is_err(),crosses,"request {from:?}->{to:?}, obstacle {blocked_layer:?}");
            if crosses {
                assert!(result.unwrap_err().to_string().contains("Via blocked"));
                assert_eq!(snapshot(session.document()),before);
                assert!(!session.is_dirty()); assert!(!session.can_undo());
            } else {
                let after = snapshot(session.document());
                session.undo().unwrap(); assert_eq!(snapshot(session.document()),before);
                session.redo().unwrap(); assert_eq!(snapshot(session.document()),after);
            }
        }
    }
}

#[test]
fn via_guard_rejects_top_and_bottom_pads_keepouts_and_near_clearance() {
    let mut bottom_pad = fp("R2","L/R",9.2,10.,&[("1","B"),("2","B")]);
    if let Item::Footprint(fp) = &mut bottom_pad { fp.side = Side::Bottom; }
    let mut obstacles = vec![
        fp("R1","L/R",10.8,10.,&[("1","B"),("2","B")]), bottom_pad,
        // Gap = 0.1 mm, below the required 0.2 mm, without a physical short.
        track((9.,10.5),(11.,10.5),0.2,layer::inner_copper(3),"B"),
    ];
    for copper in Stackup::with_copper(6).copper_layers() {
        let mut keepout = zone(9.,9.,11.,11.,copper,"");
        if let Item::Zone(z) = &mut keepout { z.keepout = true; }
        obstacles.push(keepout);
    }
    for obstacle in obstacles {
        let document = board(vec![obstacle]);
        let via = router::via_for(&document,"A",p(10.,10.));
        let mut session = Session::with_document(document);
        let before = snapshot(session.document());
        assert!(session.execute_with_via_checks(place(via),true).is_err());
        assert_eq!(snapshot(session.document()),before);
    }
}

#[test]
fn via_guard_allows_same_net_and_refillable_foreign_pours() {
    for mut obstacle in [track((9.,10.),(11.,10.),0.2,layer::inner_copper(3),"A"),zone(2.,2.,28.,28.,layer::TOP_COPPER,"B")] {
        let is_pour = matches!(obstacle,Item::Zone(_));
        if let Item::Zone(z) = &mut obstacle { z.remove_islands = false; }
        let mut session = Session::with_document(board(vec![obstacle]));
        session.execute(Command::EditPours { items: vec![], refill: None }).unwrap();
        if is_pour { assert!(session.document().items().any(|(_,i)|matches!(i,Item::Zone(z) if !z.fill.is_empty()))); }
        let before = snapshot(session.document());
        let via = router::via_for(session.document(),"A",p(10.,10.));
        session.execute_with_via_checks(place(via),true).unwrap();
        let after = snapshot(session.document());
        session.undo().unwrap(); assert_eq!(snapshot(session.document()),before);
        session.redo().unwrap(); assert_eq!(snapshot(session.document()),after);
    }
}

#[test]
fn via_guard_is_atomic_keeps_redo_and_does_not_allocate_ids_on_rejection() {
    let document = board(vec![track((9.,10.),(11.,10.),0.2,layer::TOP_COPPER,"B")]);
    let setup = |document: Document| {
        let mut session = Session::with_document(document);
        session.execute(Command::Add { items: vec![track((2.,2.),(3.,2.),0.2,layer::TOP_COPPER,"C")] }).unwrap();
        session.undo().unwrap(); session.mark_saved(); session
    };
    let mut control = setup(document.clone());
    let mut session = setup(document);
    let before = snapshot(session.document());
    let bad = router::via_for(session.document(),"A",p(10.,10.));
    let command = Command::Route { delete: vec![], replace: vec![], add: vec![track((15.,10.),(10.,10.),0.2,layer::inner_copper(2),"A"),Item::Via(bad)] };
    assert!(session.execute_with_via_checks(command,true).is_err());
    assert_eq!(snapshot(session.document()),before);
    assert!(!session.is_dirty()); assert!(!session.can_undo()); assert!(session.can_redo());
    let good = router::via_for(session.document(),"A",p(20.,20.));
    let actual = session.execute_with_via_checks(place(good.clone()),true).unwrap();
    let expected = control.execute_with_via_checks(place(good),true).unwrap();
    assert_eq!(actual,expected);
    assert_eq!(snapshot(session.document()),snapshot(control.document()));
}

#[test]
fn via_guard_checks_final_shoved_geometry_and_honors_explicit_rules_off() {
    let document = board(vec![track((9.,10.),(11.,10.),0.2,layer::TOP_COPPER,"B")]);
    let id = document.items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
    let via = router::via_for(&document,"A",p(10.,10.));
    let mut session = Session::with_document(document.clone());
    session.execute_with_via_checks(Command::Route { delete: vec![id], replace: vec![], add: vec![track((9.,12.),(11.,12.),0.2,layer::TOP_COPPER,"B"),Item::Via(via.clone())] },true).unwrap();
    let mut unchecked = Session::with_document(document);
    unchecked.execute_with_via_checks(place(via),false).unwrap();
    assert!(super::drc::run(unchecked.document()).violations.iter().any(|v|v.code=="short"));
    assert!(unchecked.can_undo());
}

#[test]
fn via_guard_rejects_the_inner_layer_to_bottom_r1_pad2_regression() {
    let document = board(vec![fp("R1","L/R",10.,10.,&[("1","SIGNAL"),("2","GND")])]);
    let via = router::via_between(&document,"SIGNAL",p(10.8,10.),layer::inner_copper(2),layer::BOTTOM_COPPER);
    assert_eq!((via.from_layer,via.to_layer),(layer::TOP_COPPER,layer::BOTTOM_COPPER));
    let mut session = Session::with_document(document);
    let before = snapshot(session.document());
    let error = session.execute_with_via_checks(Command::Route {
        delete: vec![], replace: vec![],
        add: vec![track((15.,16.),(10.8,16.),0.25,layer::inner_copper(2),"SIGNAL"),
            track((10.8,16.),(10.8,10.),0.25,layer::inner_copper(2),"SIGNAL"),Item::Via(via)],
    },true).unwrap_err().to_string();
    assert!(error.contains("Pad R1.2 (GND)"),"{error}");
    assert!(error.contains("Top Layer"),"{error}");
    assert_eq!(snapshot(session.document()),before);
    assert!(!session.can_undo());
}

#[test]
fn via_guard_allows_lock_only_edits_so_existing_bad_copper_can_be_repaired() {
    let document = board(vec![track((9.,10.),(11.,10.),0.2,layer::TOP_COPPER,"B")]);
    let via = router::via_for(&document,"A",p(10.,10.));
    let mut session = Session::with_document(document);
    session.execute(place(via.clone())).unwrap(); // Simulates an already invalid board.
    let id = session.document().items().find(|(_,i)|matches!(i,Item::Via(_))).unwrap().0;
    for locked in [true,false] {
        let mut edited = via.clone(); edited.locked = locked;
        session.execute_with_via_checks(Command::Route {delete:vec![],add:vec![],replace:vec![crate::IdItem{id,item:Item::Via(edited)}]},true).unwrap();
    }
    let before = snapshot(session.document());
    let mut moved = via; moved.pos.x += mm(0.1);
    assert!(session.execute_with_via_checks(Command::Route {delete:vec![],add:vec![],replace:vec![crate::IdItem{id,item:Item::Via(moved)}]},true).is_err());
    assert_eq!(snapshot(session.document()),before);
}
