use super::*;
use crate::document::DocKind;
use crate::item::{FIELD_DESIGNATOR, FillStyle, Part, PadKind, PadShape, Pin, PinShape, PinType, Rect, Wire};
use crate::library::{CachedPart, Component, PinMapping, Symbol, SymbolUnit};
use crate::pcb::board::{BoardSettings, NetClass};
use crate::pcb::items::{Track, Via, Zone, ZoneConnect};
use crate::schematic::{self, Net, NetNode, SheetRef};
use crate::session::Session;

// ---------- Builders ----------

fn geom(name: &str, pads: &[&str]) -> Footprint {
    let mut f = Footprint::new(name);
    for (i, n) in pads.iter().enumerate() {
        f.items.push(Item::Pad(Pad {
            number: (*n).into(),
            pos: Point::new(mm(2.0) * i as Coord, 0),
            rotation: 0.0,
            shape: PadShape::Rect,
            width: mm(1.0),
            height: mm(1.0),
            corner_ratio: 0.0,
            kind: PadKind::Smd,
            layer: layer::TOP_COPPER,
            drill: None,
         copper_shapes: Default::default(),}));
    }
    let w = mm(2.0) * pads.len().saturating_sub(1) as Coord;
    f.items.push(Item::Rect(Rect {
        a: Point::new(-mm(1.0), -mm(1.0)),
        b: Point::new(w + mm(1.0), mm(1.0)),
        width: mm(0.05),
        layer: layer::TOP_COURTYARD,
        fill: FillStyle::None,
    }));
    f
}

fn library(name: &str) -> Option<Footprint> {
    match name {
        "R_0603" | "R_0805" => Some(geom(name, &["1", "2"])),
        "SOT23" => Some(geom(name, &["1", "2", "3"])),
        "LED_0603" => Some(geom(name, &["A", "K"])),
        "SOIC8" => Some(geom(name, &["1", "2", "3", "4", "5", "6", "7", "8"])),
        _ => None,
    }
}

fn resolver(p: &NetlistPart) -> Option<(String, Footprint)> {
    library(&p.footprint).map(|f| (format!("Lib/{}", p.footprint), f))
}

fn part(des: &str, value: &str, footprint: &str) -> NetlistPart {
    NetlistPart {
        designator: des.into(),
        value: value.into(),
        footprint: footprint.into(),
        lib_ref: "Lib/R".into(),
        component: "R".into(),
        library: "Lib".into(),
        description: String::new(),
        units: vec![("top.arsch".into(), ItemId(0), 1)],
        parameters: vec![],
    }
}

fn net(name: &str, pads: &[(&str, &str)]) -> Net {
    Net {
        name: name.into(),
        nodes: pads
            .iter()
            .map(|(d, p)| NetNode {
                designator: (*d).into(),
                pin_number: (*p).into(),
                pin_name: String::new(),
                pin_type: PinType::Passive,
                pads: vec![(*p).into()],
                sheet: "top.arsch".into(),
                part: ItemId(0),
            })
            .collect(),
        items: vec![],
    }
}

fn compiled(parts: Vec<NetlistPart>, nets: Vec<Net>) -> Compiled {
    Compiled { nets, parts, violations: vec![], instances: vec![] }
}

fn placed(des: &str, value: &str, key: &str, x: f64, nets: &[(&str, &str)]) -> Item {
    Item::Footprint(PlacedFootprint {
        board_only: false,
        designator: des.into(),
        value: value.into(),
        footprint: key.into(),
        pos: Point::new(mm(x), mm(10.0)),
        rotation: 0.0,
        side: Side::Top,
        locked: false,
        pad_nets: nets.iter().map(|(p, n)| ((*p).into(), (*n).into())).collect(),
        component: "Lib/R".into(),
        parameters: BTreeMap::new(),
    })
}

fn outline() -> Item {
    Item::Rect(Rect { a: Point::new(0, 0), b: Point::new(mm(50.0), mm(30.0)), width: mm(0.1), layer: layer::BOARD_OUTLINE, fill: FillStyle::None })
}

fn board(items: Vec<Item>) -> Document {
    let mut d = Document::from_item_list(DocKind::Pcb, items);
    for name in ["R_0603", "R_0805", "SOT23", "LED_0603", "SOIC8"] {
        if d.items().any(|(_, i)| matches!(i, Item::Footprint(f) if f.footprint == format!("Lib/{name}"))) {
            d.put_cached_footprint(format!("Lib/{name}"), library(name).unwrap());
        }
    }
    d
}

fn kinds(plan: &EcoPlan) -> Vec<&'static str> {
    plan.changes
        .iter()
        .map(|c| match c {
            EcoChange::AddFootprint { .. } => "add",
            EcoChange::RemoveFootprint { .. } => "remove",
            EcoChange::ChangeFootprint { .. } => "change",
            EcoChange::UpdateValue { .. } => "value",
            EcoChange::UpdateNets { .. } => "nets",
            EcoChange::UpdateParameters { .. } => "params",
            EcoChange::RenameNet { .. } => "rename",
            EcoChange::Warning { .. } => "warning",
        })
        .collect()
}

fn warnings(plan: &EcoPlan) -> Vec<&str> {
    plan.changes.iter().filter_map(|c| if let EcoChange::Warning { message } = c { Some(message.as_str()) } else { None }).collect()
}

fn all(plan: &EcoPlan) -> Vec<usize> {
    (0..plan.changes.len()).collect()
}

fn run(plan: &EcoPlan, doc: &Document, selected: &[usize]) -> Session {
    let mut s = Session::with_document(doc.clone());
    for (k, f) in &plan.footprints {
        s.put_cached_footprint(k.clone(), f.clone());
    }
    s.execute(apply(plan, doc, selected)).unwrap();
    s
}

fn footprint<'a>(doc: &'a Document, des: &str) -> &'a PlacedFootprint {
    doc.items().find_map(|(_, i)| if let Item::Footprint(f) = i { (f.designator == des).then_some(f) } else { None }).unwrap()
}

fn items_of(doc: &Document) -> Vec<(ItemId, Item)> {
    doc.items().map(|(id, i)| (id, i.clone())).collect()
}

// ---------- Plan ----------

#[test]
fn adds_new_footprints_with_pad_nets() {
    let c = compiled(
        vec![part("R1", "10k", "R_0603"), part("R2", "1k", "R_0603"), part("#PWR1", "", "R_0603")],
        vec![net("A", &[("R1", "1"), ("R2", "1")]), net("LONE", &[("R1", "2")])],
    );
    let plan = plan(&c, &board(vec![outline()]), &resolver);
    assert_eq!(kinds(&plan), ["add", "add"]);
    let EcoChange::AddFootprint { designator, footprint_key, pad_nets, value, .. } = &plan.changes[1] else { panic!() };
    assert_eq!((designator.as_str(), footprint_key.as_str(), value.as_str()), ("R2", "Lib/R_0603", "1k"));
    assert_eq!(pad_nets.get("1").map(String::as_str), Some("A"));
    assert_eq!(pad_nets.get("2").map(String::as_str), Some(""), "unconnected pads get an empty net");
    let EcoChange::AddFootprint { pad_nets, .. } = &plan.changes[0] else { panic!() };
    assert_eq!(pad_nets["2"], "LONE", "single-pad nets keep their name");
    assert_eq!(plan.footprints.len(), 1, "each footprint is cached once");
    assert_eq!(plan.footprints[0].0, "Lib/R_0603");

    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(json["changes"][0]["kind"], "addFootprint");
    assert_eq!(json["changes"][0]["footprintKey"], "Lib/R_0603");
    assert!(json.get("footprints").is_none());
}

#[test]
fn detects_remove_change_value_and_parameter_updates() {
    let mut r3 = part("R3", "4k7", "R_0603");
    r3.parameters = vec![("MPN".into(), "RC0603-4K7".into())];
    let c = compiled(vec![part("R1", "10k", "R_0603"), part("R2", "10k", "R_0603"), r3, part("R4", "1k", "R_0603")], vec![]);
    let doc = board(vec![
        outline(),
        placed("R1", "10k", "Lib/R_0805", 5.0, &[("1", ""), ("2", "")]),
        placed("R2", "1k", "Lib/R_0603", 10.0, &[("1", ""), ("2", "")]),
        placed("R3", "4k7", "Lib/R_0603", 15.0, &[("1", ""), ("2", "")]),
        placed("R4", "1k", "Lib/R_0603", 20.0, &[("1", ""), ("2", "")]),
        placed("R9", "1k", "Lib/R_0603", 25.0, &[]),
    ]);
    let plan = plan(&c, &doc, &resolver);
    assert_eq!(kinds(&plan), ["change", "value", "params", "remove"], "{:?}", plan.changes);
    assert!(matches!(&plan.changes[0], EcoChange::ChangeFootprint { item: ItemId(1), from, to, .. } if from == "Lib/R_0805" && to == "Lib/R_0603"));
    assert!(matches!(&plan.changes[1], EcoChange::UpdateValue { from, to, .. } if from == "1k" && to == "10k"));
    assert!(matches!(&plan.changes[3], EcoChange::RemoveFootprint { item: ItemId(5), designator } if designator == "R9"));
    assert!(plan.footprints.is_empty(), "Lib/R_0603 is already cached");

    let s = run(&plan, &doc, &all(&plan));
    let d = s.document();
    let r1 = footprint(d, "R1");
    assert_eq!((r1.footprint.as_str(), r1.pos), ("Lib/R_0603", Point::new(mm(5.0), mm(10.0))));
    assert_eq!(footprint(d, "R2").value, "10k");
    assert_eq!(footprint(d, "R3").parameters["MPN"], "RC0603-4K7");
    assert!(!d.items().any(|(_, i)| matches!(i, Item::Footprint(f) if f.designator == "R9")));
}

#[test]
fn net_updates_per_pad() {
    let c = compiled(
        vec![part("R1", "10k", "R_0603"), part("R2", "10k", "R_0603")],
        vec![net("A", &[("R1", "1")]), net("B", &[("R2", "1"), ("R1", "2")])],
    );
    let doc = board(vec![
        placed("R1", "10k", "Lib/R_0603", 0.0, &[("1", "A"), ("2", "A")]),
        placed("R2", "10k", "Lib/R_0603", 5.0, &[("1", "A"), ("2", "OLD")]),
    ]);
    let plan = plan(&c, &doc, &resolver);
    assert_eq!(kinds(&plan), ["nets", "nets", "warning"], "{:?}", plan.changes);
    let EcoChange::UpdateNets { designator, changes, .. } = &plan.changes[1] else { panic!() };
    assert_eq!(designator, "R2");
    assert_eq!(changes, &[("1".into(), "A".into(), "B".into()), ("2".into(), "OLD".into(), String::new())]);

    // Only the R2 change selected.
    let s = run(&plan, &doc, &[1]);
    assert_eq!(footprint(s.document(), "R2").net_of("1"), "B");
    assert_eq!(footprint(s.document(), "R1").net_of("2"), "A");
}

#[test]
fn renames_nets_with_unchanged_pads_including_copper() {
    let c = compiled(
        vec![part("R1", "10k", "R_0603"), part("R2", "10k", "R_0603")],
        vec![net("SIG", &[("R1", "2"), ("R2", "1")]), net("GND", &[("R1", "1"), ("R2", "2")])],
    );
    let mut settings = BoardSettings::default();
    settings.net_classes.push(NetClass {
        name: "Fast".into(),
        nets: vec!["Net-(R1-Pad2)".into()],
        track_width: mm(0.2),
        clearance: mm(0.2),
        via_diameter: mm(0.6),
        via_drill: mm(0.3),
        diff_pair_width: 0,
        diff_pair_gap: 0,
    });
    let old = "Net-(R1-Pad2)";
    let doc = board(vec![
        placed("R1", "10k", "Lib/R_0603", 0.0, &[("1", "GND"), ("2", old)]),
        placed("R2", "10k", "Lib/R_0603", 5.0, &[("1", old), ("2", "GND")]),
        Item::Track(Track { locked: false, a: Point::new(mm(2.0), mm(10.0)), b: Point::new(mm(5.0), mm(10.0)), width: mm(0.2), layer: layer::TOP_COPPER, net: old.into() }),
        Item::Via(Via { locked: false, pos: Point::new(mm(3.0), mm(10.0)), diameter: mm(0.6), drill: mm(0.3), from_layer: layer::TOP_COPPER, to_layer: layer::BOTTOM_COPPER, net: old.into() }),
        Item::Zone(Zone { locked: false,
            teardrop: false,
            outline: vec![Point::new(0, 0), Point::new(mm(5.0), 0), Point::new(mm(5.0), mm(5.0))],
            layer: layer::BOTTOM_COPPER,
            net: old.into(),
            name: String::new(),
            priority: 0,
            clearance: 0,
            min_width: 0,
            connect: ZoneConnect::Thermal,
            thermal_gap: 0,
            spoke_width: 0,
            keepout: false,
            remove_islands: true,
            fill: vec![],
        }),
        Item::Track(Track { locked: false, a: Point::new(0, 0), b: Point::new(mm(1.0), 0), width: mm(0.2), layer: layer::TOP_COPPER, net: "GND".into() }),
        Item::Board(settings),
    ]);
    let plan = plan(&c, &doc, &resolver);
    assert_eq!(plan.changes, [EcoChange::RenameNet { from: old.into(), to: "SIG".into() }]);

    let mut s = run(&plan, &doc, &[0]);
    let d = s.document();
    assert_eq!(footprint(d, "R1").net_of("2"), "SIG");
    assert_eq!(footprint(d, "R2").net_of("1"), "SIG");
    let nets: Vec<&str> = d
        .items()
        .filter_map(|(_, i)| match i {
            Item::Track(t) => Some(t.net.as_str()),
            Item::Via(v) => Some(v.net.as_str()),
            Item::Zone(z) => Some(z.net.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(nets, ["SIG", "SIG", "SIG", "GND"]);
    let Some((_, b)) = crate::pcb::geometry::board_settings(d) else { panic!() };
    assert_eq!(b.net_classes.last().unwrap().nets, ["SIG"]);

    // Pure modifications keep item ids and undo in one step.
    assert_eq!(d.items().map(|(id, _)| id).collect::<Vec<_>>(), doc.items().map(|(id, _)| id).collect::<Vec<_>>());
    s.undo().unwrap();
    assert_eq!(items_of(s.document()), items_of(&doc));
}

#[test]
fn rename_onto_an_existing_board_net_is_not_offered() {
    // Board net X has exactly the pads of schematic net Y, but Y also exists on
    // the board with other pads: renaming would merge copper, so update pads.
    let c = compiled(
        vec![part("R1", "10k", "R_0603"), part("R2", "10k", "R_0603")],
        vec![net("Y", &[("R1", "1"), ("R2", "1")])],
    );
    let doc = board(vec![
        placed("R1", "10k", "Lib/R_0603", 0.0, &[("1", "X"), ("2", "Y")]),
        placed("R2", "10k", "Lib/R_0603", 5.0, &[("1", "X"), ("2", "")]),
    ]);
    let plan = plan(&c, &doc, &resolver);
    assert!(!kinds(&plan).contains(&"rename"));
    assert_eq!(kinds(&plan), ["nets", "nets"]);
}

#[test]
fn warnings_for_missing_footprints_and_pads() {
    let mut odd = part("U1", "X", "SOT23");
    odd.footprint = "SOT23".into();
    let c = compiled(
        vec![part("R1", "10k", ""), part("R2", "10k", "NOPE"), odd, part("R?", "1k", "R_0603")],
        vec![net("A", &[("U1", "1"), ("U1", "9")])],
    );
    let plan = plan(&c, &board(vec![]), &resolver);
    assert_eq!(kinds(&plan), ["add", "warning", "warning", "warning", "warning"], "{:?}", plan.changes);
    let w = warnings(&plan).join("\n");
    assert!(w.contains("R? (Lib/R) is not annotated"), "{w}");
    assert!(w.contains("Footprint 'NOPE' of R2 was not found"), "{w}");
    assert!(w.contains("R1 has no footprint assigned"), "{w}");
    assert!(w.contains("U1: pin for pad '9'"), "{w}");
}

#[test]
fn multi_unit_parts_and_pin_maps_via_schematic_compile() {
    const G: Coord = 2_540_000;
    let pin = |n: &str, x: Coord, y: Coord, t: PinType| {
        Item::Pin(Pin { pos: Point::new(x * G, y * G), length: G, rotation: 0.0, name: n.into(), number: n.into(), electrical: t, shape: PinShape::Line, hidden: false })
    };
    let cached = |name: &str, prefix: &str, units: Vec<Vec<Item>>, fp: &str, pin_map: Vec<PinMapping>| {
        let mut symbol = Symbol::new(name, prefix);
        symbol.units = units.into_iter().map(|items| SymbolUnit { name: String::new(), items }).collect();
        CachedPart {
            library: "Lib".into(),
            component: Component {
                name: name.into(),
                description: String::new(),
                symbol: name.into(),
                footprints: vec![fp.into()],
                pin_map,
                parameters: vec![],
            },
            symbol,
        }
    };
    let opamp = cached(
        "LM358",
        "U",
        vec![
            vec![pin("1", 3, 0, PinType::Output), pin("2", -3, 1, PinType::Input), pin("3", -3, -1, PinType::Input)],
            vec![pin("7", 3, 0, PinType::Output), pin("6", -3, 1, PinType::Input), pin("5", -3, -1, PinType::Input)],
        ],
        "SOIC8",
        vec![],
    );
    let led = cached(
        "LED",
        "D",
        vec![vec![pin("1", -1, 0, PinType::Passive), pin("2", 1, 0, PinType::Passive)]],
        "LED_0603",
        vec![
            PinMapping { unit: 1, pin: "1".into(), pads: vec!["A".into()] },
            PinMapping { unit: 1, pin: "2".into(), pads: vec!["K".into()] },
        ],
    );
    let mut items = Vec::new();
    for (c, unit, x) in [(&opamp, 1, 0), (&opamp, 2, 20), (&led, 1, 40)] {
        let mut p = Part { pos: Point::new(x * G, 0), rotation: 0.0, mirror: false, lib_ref: c.key(), unit, fields: vec![] };
        p.set_field(FIELD_DESIGNATOR, if c.symbol.prefix == "U" { "U1" } else { "D1" });
        items.push(Item::Part(p));
    }
    // U1A output → U1B non-inverting input; U1B output → LED anode.
    items.push(Item::Wire(Wire { a: Point::new(3 * G, 0), b: Point::new(17 * G, -G) }));
    items.push(Item::Wire(Wire { a: Point::new(23 * G, 0), b: Point::new(39 * G, 0) }));
    let mut sch = Document::from_item_list(DocKind::Schematic, items);
    sch.put_cached_part(opamp.clone());
    sch.put_cached_part(led.clone());
    let comp = schematic::compile(&[SheetRef::new("top.arsch", &sch)]);
    let resolve = |p: &NetlistPart| library(&p.footprint).map(|f| (format!("Lib/{}", p.footprint), f));

    let plan = plan(&comp, &board(vec![]), &resolve);
    assert_eq!(kinds(&plan), ["add", "add"], "{:?}", plan.changes);
    let EcoChange::AddFootprint { designator, pad_nets, .. } = &plan.changes[1] else { panic!() };
    assert_eq!(designator, "U1", "both units share one footprint");
    assert_eq!(pad_nets.len(), 8);
    assert_eq!(pad_nets["1"], pad_nets["5"]);
    assert!(!pad_nets["1"].is_empty());
    assert_eq!(pad_nets["4"], "", "power unit not placed: pads stay unconnected");
    let EcoChange::AddFootprint { designator, pad_nets: led_pads, .. } = &plan.changes[0] else { panic!() };
    assert_eq!(designator, "D1");
    assert_eq!(led_pads["A"], pad_nets["7"], "pin 1 maps to pad A");
    assert_eq!(led_pads["K"], "");
}

// ---------- Apply ----------

fn bboxes(doc: &Document) -> Vec<(String, BBox)> {
    doc.items()
        .filter_map(|(_, i)| if let Item::Footprint(f) = i { Some((f.designator.clone(), world_bbox(f, doc.cached_footprint(&f.footprint)))) } else { None })
        .collect()
}

fn overlap(a: &BBox, b: &BBox) -> bool {
    a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
}

#[test]
fn new_footprints_are_placed_in_a_grid_beside_the_board() {
    let mut parts: Vec<NetlistPart> = (1..=12).map(|i| part(&format!("U{i}"), "X", "SOIC8")).collect();
    parts.push(part("R10", "1k", "R_0603"));
    parts.push(part("R2", "1k", "R_0603"));
    parts.push(part("R1", "1k", "R_0603"));
    parts.push(part("Q1", "1k", "SOT23"));
    let c = compiled(parts, vec![]);
    // An existing footprint sits outside the outline, to its right.
    let doc = board(vec![outline(), placed("Q1", "1k", "Lib/SOT23", 56.0, &[("1", ""), ("2", ""), ("3", "")])]);
    let plan = plan(&c, &doc, &resolver);
    assert_eq!(kinds(&plan).iter().filter(|k| **k == "add").count(), 15);

    let s = run(&plan, &doc, &all(&plan));
    let boxes = bboxes(s.document());
    let q1 = boxes.iter().find(|(d, _)| d == "Q1").unwrap().1;
    for (i, (d, b)) in boxes.iter().enumerate() {
        if d == "Q1" {
            continue;
        }
        assert!(b.min.x > mm(50.0), "{d} must be right of the board");
        assert!(!overlap(b, &q1), "{d} overlaps the existing Q1");
        for (e, o) in &boxes[i + 1..] {
            assert!(!overlap(b, o), "{d} overlaps {e}");
        }
    }
    let x_of = |d: &str| footprint(s.document(), d).pos.x;
    assert!(x_of("R1") < x_of("R2") && x_of("R2") < x_of("R10"), "natural designator order");
    let rows: BTreeSet<Coord> = boxes.iter().filter(|(d, _)| d != "Q1").map(|(_, b)| b.max.y).collect();
    assert!(rows.len() > 1, "wraps into several rows");
}

#[test]
fn apply_is_one_undoable_command() {
    let c = compiled(
        vec![part("R1", "22k", "R_0603"), part("R3", "1k", "R_0603")],
        vec![net("A", &[("R1", "1"), ("R3", "1")])],
    );
    let doc = board(vec![outline(), placed("R1", "10k", "Lib/R_0603", 5.0, &[("1", ""), ("2", "")]), placed("R2", "1k", "Lib/R_0603", 10.0, &[])]);
    let plan = plan(&c, &doc, &resolver);
    assert_eq!(kinds(&plan), ["value", "nets", "add", "remove"], "{:?}", plan.changes);
    let cmd = apply(&plan, &doc, &all(&plan));
    assert!(matches!(cmd, Command::Change { .. }));

    let mut s = run(&plan, &doc, &all(&plan));
    let after = items_of(s.document());
    let d = s.document();
    assert_eq!(footprint(d, "R1").value, "22k");
    assert_eq!(footprint(d, "R1").net_of("1"), "A");
    assert_eq!(footprint(d, "R3").net_of("1"), "A");
    assert_eq!(bboxes(d).len(), 2);

    assert!(s.undo().is_some());
    assert_eq!(items_of(s.document()), items_of(&doc));
    assert!(!s.can_undo(), "a single undo step");
    s.redo().unwrap();
    assert_eq!(items_of(s.document()), after);

    // Nothing selected (or only warnings): an empty command.
    let mut s = Session::with_document(doc.clone());
    s.execute(apply(&plan, &doc, &[99])).unwrap();
    assert!(!s.can_undo());
}


#[test]
fn independent_board_items_survive_schematic_updates_and_round_trip() {
    let mut graphic=placed("LOGO1","Art","Lib/R_0603",0.,&[]);
    if let Item::Footprint(f)=&mut graphic { f.board_only=true; }
    let doc=board(vec![graphic,placed("R9","Old","Lib/R_0603",5.,&[])]);
    let c=compiled(vec![],vec![]);
    let plan=plan(&c,&doc,&resolver);
    assert_eq!(plan.changes,vec![EcoChange::RemoveFootprint{item:ItemId(1),designator:"R9".into()}]);
    let mut session=run(&plan,&doc,&all(&plan));
    assert_eq!(session.document().len(),1);assert_eq!(session.document().get(ItemId(0)),doc.get(ItemId(0)));
    let text=crate::io::document_to_string(session.document()).unwrap();
    assert!(text.contains("\"boardOnly\": true"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["version"],2);
    let reopened=crate::io::document_from_str(DocKind::Pcb,&text).unwrap();
    assert!(footprint(&reopened,"LOGO1").board_only);
    session.undo().unwrap();assert_eq!(items_of(session.document()),items_of(&doc));
    session.redo().unwrap();assert_eq!(items_of(session.document()),items_of(&reopened));
    let old=crate::io::document_to_string(&doc).unwrap().replace("\"boardOnly\": true,", "");
    let legacy=crate::io::document_from_str(DocKind::Pcb,&old).unwrap();
    assert!(!footprint(&legacy,"LOGO1").board_only,"old files default to ordinary linked footprints");
}

#[test]
fn schematic_update_review_expires_after_board_cache_or_netlist_changes() {
    let c=compiled(vec![part("R1","10k","R_0603")],vec![]);
    let doc=board(vec![placed("R1","1k","Lib/R_0603",0.,&[])]);
    let reviewed=plan(&c,&doc,&resolver);
    assert!(reviewed.is_current(&c,&doc));
    let mut changed=c.clone();changed.parts[0].value="22k".into();
    assert!(!reviewed.is_current(&changed,&doc));
    let changed_doc=board(vec![placed("R1","4k7","Lib/R_0603",0.,&[])]);
    assert!(!reviewed.is_current(&c,&changed_doc));
    let mut changed_geometry=doc.clone();changed_geometry.put_cached_footprint("Lib/R_0603",geom("R_0603",&["1"]));
    assert!(!reviewed.is_current(&c,&changed_geometry));
    assert_eq!(items_of(&doc),items_of(&board(vec![placed("R1","1k","Lib/R_0603",0.,&[])])));
    let json=serde_json::to_value(reviewed).unwrap();
    assert!(json.get("sourceBoard").is_none()&&json.get("sourceNetlist").is_none(),"private snapshots never cross IPC");
}

#[test]
fn split_board_net_has_explicit_hierarchy_diagnostic_without_a_global_rename() {
    let c=compiled(vec![part("R1","10k","R_0603"),part("R2","10k","R_0603")],vec![net("/Left/D",&[("R1","1")]),net("/Right/D",&[("R2","1")])]);
    let doc=board(vec![placed("R1","10k","Lib/R_0603",0.,&[("1","OLD")]),placed("R2","10k","Lib/R_0603",5.,&[("1","OLD")])]);
    let reviewed=plan(&c,&doc,&resolver);
    assert!(!reviewed.changes.iter().any(|c|matches!(c,EcoChange::RenameNet{..})));
    assert!(reviewed.changes.iter().any(|c|matches!(c,EcoChange::Warning{message}if message.contains("Board net 'OLD'")&&message.contains("bus/hierarchy"))));
    let session=run(&reviewed,&doc,&[]);assert_eq!(items_of(session.document()),items_of(&doc));assert!(!session.can_undo());
}

#[test]
fn added_pads_reuse_existing_generated_net_name_including_new_footprints() {
    let (old,new)=("Net-(R1-A)","Net-(R1-Pad1)");
    let c=compiled(vec![part("R1","10k","R_0603"),part("R2","10k","R_0603"),part("R3","10k","R_0603")],vec![net(new,&[("R1","1"),("R2","1"),("R3","1")])]);
    let doc=board(vec![placed("R1","10k","Lib/R_0603",0.,&[("1",old)]),placed("R2","10k","Lib/R_0603",5.,&[("1",old)]),Item::Track(Track{locked:false,a:Point::new(0,0),b:Point::new(mm(5.),0),width:mm(0.2),layer:layer::TOP_COPPER,net:old.into()})]);
    let reviewed=plan(&c,&doc,&resolver);
    assert_eq!(kinds(&reviewed),vec!["add"]);
    let session=run(&reviewed,&doc,&all(&reviewed));
    for des in ["R1","R2","R3"] {assert_eq!(footprint(session.document(),des).net_of("1"),old);}
    assert!(session.document().items().any(|(_,i)|matches!(i,Item::Track(t)if t.net==old)));
}

#[test]
fn rename_does_not_merge_into_a_net_represented_only_by_loose_copper() {
    let c=compiled(vec![part("R1","10k","R_0603")],vec![net("TARGET",&[("R1","1")])]);
    let doc=board(vec![placed("R1","10k","Lib/R_0603",0.,&[("1","OLD")]),Item::Track(Track{locked:false,a:Point::new(0,0),b:Point::new(mm(2.),0),width:mm(0.2),layer:layer::TOP_COPPER,net:"TARGET".into()})]);
    let reviewed=plan(&c,&doc,&resolver);
    assert!(!reviewed.changes.iter().any(|change|matches!(change,EcoChange::RenameNet{..})));
    assert!(reviewed.changes.iter().any(|change|matches!(change,EcoChange::UpdateNets{..})));
}
