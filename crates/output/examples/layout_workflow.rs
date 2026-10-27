//! Local acceptance fixture and native routing/pour transactions.
//! Run with the ignored workflow fixture directory as the first argument.
use std::{collections::HashSet, fs, path::PathBuf};
use pcbplannercad_core::{io, layer, pcb::{self, board::{Stackup, ViaType}, router::{self, RouteRequest, RouteMode, CornerStyle}}, units::mm, Command, DocKind, Document, IdItem, Item, ItemId, Point, Session};
use serde_json::json;

fn snapshot(doc: &Document) -> String { io::document_to_string(doc).unwrap() }
fn p(x: f64, y: f64) -> Point { Point::new(mm(x), mm(y)) }
fn assert_clear(doc: &Document) {
    let errors: Vec<_> = pcb::drc::run(doc).violations.into_iter()
        .filter(|v| matches!(v.code, "short" | "clearance")).collect();
    assert!(errors.is_empty(), "{errors:?}");
}
fn add_route(session: &mut Session, layer: pcbplannercad_core::LayerId, start: Point, end: Point) {
    let route = router::route(session.document(), &RouteRequest {
        net: "SIGNAL".into(), layer, start, end, width: mm(0.25),
        mode: RouteMode::Walkaround, corner: CornerStyle::Diagonal45,
        ignore: vec![], posture_flip: false,
    });
    assert!(route.reached, "{}", route.message);
    assert!(!route.tracks.is_empty());
    session.execute_with_via_checks(Command::Route { delete: vec![], replace: vec![], add: route.tracks.into_iter().map(Item::Track).collect() },true).unwrap();
}
fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("workflow fixture directory"));
    let out = root.join("acceptance-output");
    fs::create_dir_all(&out).unwrap();
    // Optional native inspection of a board saved by a live negative test.
    if let Some(path) = std::env::args().nth(2) {
        let inspected = io::document_from_str(DocKind::Pcb, &fs::read_to_string(path).unwrap()).unwrap();
        let report = pcb::drc::run(&inspected);
        fs::write(out.join("inspection-drc.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
        for violation in report.violations.iter().filter(|v| matches!(v.code,"short"|"clearance")) {
            println!("{}",serde_json::to_string(violation).unwrap());
        }
        return;
    }
    let clean = io::document_from_str(DocKind::Pcb, &fs::read_to_string(root.join("clean-routing.arpcb")).unwrap()).unwrap();
    let mut setup = Session::with_document(clean);
    let routing = setup.document().items().filter_map(|(id, item)| matches!(item, Item::Track(_) | Item::Via(_) | Item::Zone(_)).then_some(id)).collect();
    setup.execute(Command::Route { delete: routing, replace: vec![], add: vec![] }).unwrap();
    let mut replacements = vec![];
    let (top, bottom, in1, in2) = (layer::TOP_COPPER, layer::BOTTOM_COPPER, layer::inner_copper(1), layer::inner_copper(2));
    for (id, item) in setup.document().items() {
        let mut item = item.clone();
        match &mut item {
            Item::Board(board) => {
                board.stackup = Stackup::with_copper(4);
                board.via_types = vec![
                    ViaType { name: "Top to Inner 1".into(), from_layer: top, to_layer: in1, diameter: mm(0.3), drill: mm(0.1) },
                    ViaType { name: "Inner 1 to Inner 2".into(), from_layer: in1, to_layer: in2, diameter: mm(0.5), drill: mm(0.2) },
                ];
            }
            Item::Footprint(fp) if fp.designator == "R2" => fp.side = pcb::items::Side::Bottom,
            _ => continue,
        }
        replacements.push(IdItem { id, item });
    }
    setup.execute(Command::ReplaceMany { items: replacements }).unwrap();
    let zone = |name: &str, net: &str, priority: i32, a: Point, b: Point| -> Item {
        serde_json::from_value(json!({"kind":"zone", "name":name, "net":net, "layer":0,
            "priority":priority,"outline":[a,{"x":b.x,"y":a.y},b,{"x":a.x,"y":b.y}],
            "connect":"thermal","thermalGap":mm(0.3),"spokeWidth":mm(0.3),"removeIslands":false})).unwrap()
    };
    setup.execute(Command::Add { items: vec![zone("Ground", "GND", 0, p(2.,2.), p(38.,28.)), zone("Power", "PWR", 20, p(28.,18.), p(36.,26.))] }).unwrap();
    setup.execute(Command::EditPours { items: vec![], refill: None }).unwrap();
    let base = setup.document().clone();
    let baseline = snapshot(&base);
    // Do not overwrite a disposable board that is open in the live editor.
    if !root.join("layout-start.arpcb").exists() {
        fs::write(root.join("layout-start.arpcb"), &baseline).unwrap();
    }
    assert_clear(&base);
    let mut results = vec![];
    let mut route = Session::with_document(base.clone());
    add_route(&mut route, top, p(9.2,10.), p(9.2,16.));
    for (from, to, at, drill) in [(top,in1,p(9.2,16.),mm(0.1)), (in1,in2,p(15.,16.),mm(0.2)), (in2,bottom,p(15.,20.),mm(0.3))] {
        let via = router::via_between(route.document(), "SIGNAL", at, from, to);
        assert_eq!(via.drill, drill);
        route.execute_with_via_checks(Command::Route { delete: vec![], replace: vec![], add: vec![Item::Via(via)] },true).unwrap();
        if to == in1 { add_route(&mut route, in1, at, p(15.,16.)); }
        else if to == in2 { add_route(&mut route, in2, at, p(15.,20.)); }
        else { add_route(&mut route, bottom, at, p(24.,20.)); }
    }
    assert_clear(route.document());
    let connectivity = pcb::connectivity::analyze(route.document());
    for (id, item) in route.document().items() {
        if matches!(item, Item::Track(t) if t.net == "SIGNAL") || matches!(item, Item::Via(v) if v.net == "SIGNAL") {
            assert!(connectivity.connected((ItemId(1),Some("1")),(id,None)));
        }
    }
    let routed = snapshot(route.document());
    for _ in 0..7 { route.undo().unwrap(); }
    assert_eq!(snapshot(route.document()), baseline);
    for _ in 0..7 { route.redo().unwrap(); }
    assert_eq!(snapshot(route.document()), routed);
    fs::write(out.join("four-layer-routed.arpcb"), routed).unwrap();
    results.push(json!({"check":"four-layer-pad-route-three-via-spans-pour-clearance-undo-redo","status":"passed"}));
    let mut bottom_route = Session::with_document(base.clone());
    add_route(&mut bottom_route, bottom, p(25.8,10.), p(25.8,16.));
    assert_clear(bottom_route.document());
    results.push(json!({"check":"bottom-side-pad-center-escape","status":"passed"}));
    let mut pours = Session::with_document(base.clone());
    let (id, mut power) = pours.document().items().find_map(|(id,i)| match i {Item::Zone(z) if z.name == "Power" => Some((id,z.clone())), _=>None}).unwrap();
    power.priority = -10;
    power.clearance = mm(0.4);
    pours.execute(Command::EditPours { items: vec![IdItem { id, item: Item::Zone(power) }], refill: Some(vec![id]) }).unwrap();
    assert!(matches!(pours.document().get(id), Some(Item::Zone(z)) if z.fill.is_empty()));
    let changed = snapshot(pours.document());
    pours.undo().unwrap(); assert_eq!(snapshot(pours.document()), baseline);
    pours.redo().unwrap(); assert_eq!(snapshot(pours.document()), changed);
    results.push(json!({"check":"overlapping-pour-priority-clearance-selected-refill-undo-redo","status":"passed"}));
    let plans = root.join("array-plans.json");
    if plans.exists() {
        let plans: serde_json::Value = serde_json::from_str(&fs::read_to_string(plans).unwrap()).unwrap();
        for plan in plans.as_array().unwrap() {
            let copies: Vec<Item> = serde_json::from_value(plan["items"].clone()).unwrap();
            let mut session = Session::with_document(base.clone());
            session.execute(Command::Add { items: copies.clone() }).unwrap();
            let references: Vec<_> = session.document().items().filter_map(|(_,i)|if let Item::Footprint(fp)=i{Some(fp.designator.to_uppercase())}else{None}).collect();
            assert_eq!(references.len(), 2 + copies.len());
            assert_eq!(references.iter().collect::<HashSet<_>>().len(), references.len());
            let added = snapshot(session.document());
            session.execute(Command::EditPours { items: vec![], refill: None }).unwrap();
            assert_clear(session.document());
            let filled = snapshot(session.document());
            session.undo().unwrap(); assert_eq!(snapshot(session.document()), added);
            session.undo().unwrap(); assert_eq!(snapshot(session.document()), baseline);
            session.redo().unwrap(); assert_eq!(snapshot(session.document()), added);
            session.redo().unwrap(); assert_eq!(snapshot(session.document()), filled);
            let reopened = io::document_from_str(DocKind::Pcb, &filled).unwrap();
            assert_eq!(snapshot(&reopened), filled);
            fs::write(out.join(format!("array-{}.arpcb", plan["name"].as_str().unwrap())), filled).unwrap();
            results.push(json!({"check":plan["name"],"status":"passed","copies":copies.len(),"refilledClearancesChecked":true}));
        }
    }
    let live_path = out.join("live-routed.arpcb");
    if live_path.exists() {
        let live = io::document_from_str(DocKind::Pcb, &fs::read_to_string(live_path).unwrap()).unwrap();
        assert_clear(&live);
        let connectivity = pcb::connectivity::analyze(&live);
        let copper: Vec<_> = live.items().filter(|(_,item)| matches!(item,Item::Track(t) if t.net=="SIGNAL") || matches!(item,Item::Via(v) if v.net=="SIGNAL")).map(|(id,_)|id).collect();
        assert_eq!(copper.len(),4);
        for id in copper { assert!(connectivity.connected((ItemId(1),Some("1")),(id,None))); }
        results.push(json!({"check":"live-route-save-native-drc-connectivity","status":"passed"}));
    }
    let bad_path = out.join("live-via-crossnet.arpcb");
    let good_path = out.join("live-routed.arpcb");
    if bad_path.exists() && good_path.exists() {
        let good = io::document_from_str(DocKind::Pcb,&fs::read_to_string(good_path).unwrap()).unwrap();
        let bad = io::document_from_str(DocKind::Pcb,&fs::read_to_string(bad_path).unwrap()).unwrap();
        let add = bad.items().filter(|(id,_)|good.get(*id).is_none()).map(|(_,item)|item.clone()).collect();
        let before = snapshot(&good);
        let mut checked = Session::with_document(good);
        let error = checked.execute_with_via_checks(Command::Route {delete:vec![],replace:vec![],add},true).unwrap_err().to_string();
        assert!(error.contains("Pad R1.2 (GND)"),"{error}");
        assert_eq!(snapshot(checked.document()),before);
        assert!(!checked.can_undo());
        results.push(json!({"check":"original-live-negative-replay-rejected-atomically","status":"passed","message":error}));
    }
    fs::write(out.join("layout-results.json"), serde_json::to_string_pretty(&results).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
