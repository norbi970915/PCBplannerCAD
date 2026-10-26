use super::*;
use crate::document::{DocKind, Document, ItemId};
use crate::item::{
    Bus, BusEntry, FIELD_DESIGNATOR, FIELD_VALUE, Field, HAlign, Item, Junction, LabelScope, NetLabel, NoConnect, Part, Pin, PinShape,
    PinType, PortShape, PowerPort, PowerStyle, Sheet, SheetEntry, SheetSide, VAlign, Wire,
};
use crate::library::{CachedPart, Component, Parameter, PinMapping, Symbol, SymbolUnit};

const G: i64 = 2_540_000;

fn p(x: i64, y: i64) -> Point {
    Point::new(x * G, y * G)
}

fn pin(number: &str, name: &str, x: i64, y: i64, ty: PinType) -> Item {
    Item::Pin(Pin {
        pos: p(x, y),
        length: G,
        rotation: 0.0,
        name: name.into(),
        number: number.into(),
        electrical: ty,
        shape: PinShape::Line,
        hidden: false,
    })
}

fn hidden(item: Item) -> Item {
    match item {
        Item::Pin(mut p) => {
            p.hidden = true;
            Item::Pin(p)
        }
        other => other,
    }
}

fn cached(name: &str, prefix: &str, units: Vec<Vec<Item>>, params: &[(&str, &str)], footprint: &str) -> CachedPart {
    let mut symbol = Symbol::new(name, prefix);
    symbol.units = units.into_iter().map(|items| SymbolUnit { name: String::new(), items }).collect();
    CachedPart {
        library: "Lib".into(),
        component: Component {
            name: name.into(),
            description: format!("{name} part"),
            symbol: name.into(),
            footprints: if footprint.is_empty() { vec![] } else { vec![footprint.into()] },
            pin_map: vec![],
            parameters: params.iter().map(|(n, v)| Parameter { name: (*n).into(), value: (*v).into() }).collect(),
        },
        symbol,
    }
}

fn resistor() -> CachedPart {
    cached("R", "R", vec![vec![pin("1", "~", -1, 0, PinType::Passive), pin("2", "~", 1, 0, PinType::Passive)]], &[("Value", "10k")], "R_0603")
}

fn led() -> CachedPart {
    let mut c = cached("LED", "D", vec![vec![pin("1", "A", -1, 0, PinType::Passive), pin("2", "K", 1, 0, PinType::Passive)]], &[], "LED_0603");
    c.component.pin_map = vec![
        PinMapping { unit: 1, pin: "1".into(), pads: vec!["A".into()] },
        PinMapping { unit: 1, pin: "2".into(), pads: vec!["K".into()] },
    ];
    c
}

fn opamp() -> CachedPart {
    let mut c = cached(
        "LM358",
        "U",
        vec![
            vec![pin("1", "OUT", 3, 0, PinType::Output), pin("2", "IN-", -3, 1, PinType::Input), pin("3", "IN+", -3, -1, PinType::Input)],
            vec![pin("7", "OUT", 3, 0, PinType::Output), pin("6", "IN-", -3, 1, PinType::Input), pin("5", "IN+", -3, -1, PinType::Input)],
            vec![pin("8", "V+", 0, 2, PinType::PowerIn), pin("4", "V-", 0, -2, PinType::PowerIn)],
        ],
        &[("Value", "LM358")],
        "SOIC-8",
    );
    c.symbol.fields = vec![Field {
        name: FIELD_DESIGNATOR.into(),
        value: "U?".into(),
        offset: p(2, 3),
        rotation: 0.0,
        height: G / 2,
        h_align: HAlign::Left,
        v_align: VAlign::Middle,
        visible: true,
    }];
    c
}

fn inverter() -> CachedPart {
    cached(
        "74HC04",
        "U",
        vec![vec![
            pin("1", "A", -2, 0, PinType::Input),
            pin("2", "Y", 2, 0, PinType::Output),
            hidden(pin("14", "VCC", 0, 1, PinType::PowerIn)),
            hidden(pin("7", "GND", 0, -1, PinType::PowerIn)),
        ]],
        &[("Value", "74HC04")],
        "SOIC-14",
    )
}

fn regulator() -> CachedPart {
    cached(
        "LDO",
        "U",
        vec![vec![pin("1", "VIN", -2, 0, PinType::PowerIn), pin("2", "GND", 0, -2, PinType::PowerIn), pin("3", "VOUT", 2, 0, PinType::PowerOut)]],
        &[],
        "SOT-223",
    )
}

fn open_collector() -> CachedPart {
    cached("OC", "U", vec![vec![pin("1", "OC", 1, 0, PinType::OpenCollector)]], &[], "")
}

fn gnd_symbol() -> CachedPart {
    let mut c = cached("GND", "#PWR", vec![vec![hidden(pin("1", "GND", 0, 0, PinType::PowerIn))]], &[], "");
    c.symbol.is_power = true;
    c
}

#[derive(Default)]
struct Sch {
    items: Vec<Item>,
    cache: Vec<CachedPart>,
}

impl Sch {
    fn add(&mut self, item: Item) -> ItemId {
        self.items.push(item);
        ItemId(self.items.len() as u64 - 1)
    }

    fn wire(&mut self, a: Point, b: Point) -> ItemId {
        self.add(Item::Wire(Wire { a, b }))
    }

    fn part_unit(&mut self, cp: &CachedPart, des: &str, pos: Point, unit: u32) -> ItemId {
        if !self.cache.iter().any(|c| c.key() == cp.key()) {
            self.cache.push(cp.clone());
        }
        let mut part = Part { pos, rotation: 0.0, mirror: false, lib_ref: cp.key(), unit, fields: vec![] };
        part.set_field(FIELD_DESIGNATOR, des);
        self.add(Item::Part(part))
    }

    fn part(&mut self, cp: &CachedPart, des: &str, pos: Point) -> ItemId {
        self.part_unit(cp, des, pos, 1)
    }

    fn label(&mut self, pos: Point, text: &str, scope: LabelScope) -> ItemId {
        self.add(Item::NetLabel(NetLabel { pos, text: text.into(), rotation: 0.0, scope, shape: PortShape::Passive }))
    }

    fn power(&mut self, pos: Point, net: &str) -> ItemId {
        self.add(Item::PowerPort(PowerPort { pos, net: net.into(), rotation: 90.0, style: PowerStyle::Bar }))
    }

    fn junction(&mut self, pos: Point) -> ItemId {
        self.add(Item::Junction(Junction { pos }))
    }

    fn nc(&mut self, pos: Point) -> ItemId {
        self.add(Item::NoConnect(NoConnect { pos }))
    }

    fn sheet(&mut self, pos: Point, name: &str, file: &str, entries: &[(&str, SheetSide, i64)]) -> ItemId {
        self.add(Item::Sheet(Sheet {
            pos,
            size: p(10, 10),
            name: name.into(),
            file: file.into(),
            entries: entries
                .iter()
                .map(|(n, side, off)| SheetEntry { name: (*n).into(), side: *side, offset: off * G, shape: PortShape::Passive })
                .collect(),
        }))
    }

    fn doc(&self) -> Document {
        let mut d = Document::from_item_list(DocKind::Schematic, self.items.clone());
        for c in &self.cache {
            d.put_cached_part(c.clone());
        }
        d
    }
}

fn compile1(doc: &Document) -> Compiled {
    compile(&[SheetRef::new("top.arsch", doc)])
}

fn count(c: &Compiled, code: &str) -> usize {
    c.violations.iter().filter(|v| v.code == code).count()
}

fn find<'a>(c: &'a Compiled, code: &str) -> Vec<&'a Violation> {
    c.violations.iter().filter(|v| v.code == code).collect()
}

fn members(net: &Net) -> Vec<String> {
    net.nodes.iter().map(|n| format!("{}.{}", n.designator, n.pin_number)).collect()
}

fn same_net(c: &Compiled, a: (&str, &str), b: (&str, &str)) -> bool {
    match (c.net_of(a.0, a.1), c.net_of(b.0, b.1)) {
        (Some(x), Some(y)) => x.name == y.name,
        _ => false,
    }
}

// ---------- Connectivity ----------

/// R1.2 —— R2.1 with a T branch to R3.1 and a crossing wire R4.1 | R5.1.
fn t_and_cross(with_junction: bool) -> Document {
    let mut s = Sch::default();
    let r = resistor();
    s.part(&r, "R1", p(0, 0));
    s.part(&r, "R2", p(10, 0));
    s.part(&r, "R3", p(6, -5));
    s.part(&r, "R4", p(3, 3)); // pin 1 at (2, 3)
    s.part(&r, "R5", p(3, -3)); // pin 1 at (2, -3)
    s.wire(p(1, 0), p(9, 0));
    s.wire(p(5, 0), p(5, -5)); // T onto the interior of the first wire
    s.wire(p(2, 3), p(2, -3)); // crosses the first wire at (2, 0)
    if with_junction {
        s.junction(p(2, 0));
    }
    s.doc()
}

#[test]
fn t_junction_connects_but_crossing_does_not() {
    let c = compile1(&t_and_cross(false));
    let net = c.net_of("R1", "2").unwrap();
    assert_eq!(members(net), ["R1.2", "R2.1", "R3.1"]);
    assert_eq!(members(c.net_of("R4", "1").unwrap()), ["R4.1", "R5.1"]);
    assert_eq!(net.items.len(), 2, "two wires form the T net");
    assert_eq!(count(&c, "dangling-wire"), 0);
}

#[test]
fn junction_at_crossing_connects() {
    let c = compile1(&t_and_cross(true));
    assert_eq!(members(c.net_of("R1", "2").unwrap()), ["R1.2", "R2.1", "R3.1", "R4.1", "R5.1"]);
}

#[test]
fn wire_endpoint_on_wire_endpoint_and_pin_interior() {
    // A pin lying in the middle of a wire connects to it; chained wires connect.
    let mut s = Sch::default();
    let r = resistor();
    s.part(&r, "R1", p(0, 0));
    s.part(&r, "R2", p(10, 0));
    s.part(&r, "R3", p(5, 1)); // pin 1 at (4, 1) — not on the wire
    s.wire(p(1, 0), p(4, 0));
    s.wire(p(4, 0), p(9, 0));
    s.part(&r, "R4", p(6, 0)); // pin 1 at (5,0), pin 2 at (7,0): both on the wire
    let c = compile1(&s.doc());
    assert_eq!(members(c.net_of("R1", "2").unwrap()), ["R1.2", "R2.1", "R4.1", "R4.2"]);
    assert!(c.net_of("R3", "1").is_none(), "isolated pins form no net");
}

#[test]
fn local_labels_are_per_sheet_and_global_labels_span_sheets() {
    let r = resistor();
    let mut a = Sch::default();
    a.part(&r, "R1", p(0, 0));
    a.label(p(1, 0), "SIG", LabelScope::Local);
    a.label(p(-1, 0), "G", LabelScope::Global);
    a.part(&r, "R3", p(0, 5));
    a.label(p(1, 5), "SIG", LabelScope::Local);
    let mut b = Sch::default();
    b.part(&r, "R2", p(0, 0));
    b.label(p(1, 0), "SIG", LabelScope::Local);
    b.label(p(-1, 0), "G", LabelScope::Global);
    let (da, db) = (a.doc(), b.doc());
    let c = compile(&[SheetRef::new("a.arsch", &da), SheetRef::new("b.arsch", &db)]);

    assert_eq!(c.instances.len(), 2, "two top-level sheets");
    assert!(same_net(&c, ("R1", "2"), ("R3", "2")));
    assert!(!same_net(&c, ("R1", "2"), ("R2", "2")));
    assert!(same_net(&c, ("R1", "1"), ("R2", "1")));
    assert_eq!(c.net_of("R1", "1").unwrap().name, "G");
    let mut names = vec![c.net_of("R1", "2").unwrap().name.clone(), c.net_of("R2", "2").unwrap().name.clone()];
    names.sort();
    assert_eq!(names, ["SIG", "SIG_2"], "distinct nets get distinct names");
    assert_eq!(count(&c, "unconnected-label"), 0);
}

#[test]
fn power_ports_join_by_name() {
    let r = resistor();
    let mut s = Sch::default();
    s.part(&r, "R1", p(0, 0));
    s.power(p(-1, 0), "VCC");
    s.part(&r, "R2", p(0, 5));
    s.wire(p(-1, 5), p(-3, 5));
    s.power(p(-3, 5), "VCC");
    s.power(p(1, 0), "GND");
    s.power(p(1, 5), "GND");
    let c = compile1(&s.doc());
    let vcc = c.net("VCC").unwrap();
    assert_eq!(members(vcc), ["R1.1", "R2.1"]);
    assert_eq!(members(c.net("GND").unwrap()), ["R1.2", "R2.2"]);
    assert!(vcc.items.len() == 3, "two ports and a wire: {:?}", vcc.items);
    assert!(c.violations.is_empty(), "{:?}", c.violations);
}

#[test]
fn hidden_power_pins_join_global_nets() {
    let mut s = Sch::default();
    s.part(&inverter(), "U1", p(0, 0));
    s.part(&resistor(), "R1", p(10, 0));
    s.power(p(9, 0), "VCC");
    s.power(p(11, 0), "GND");
    s.wire(p(-2, 0), p(-5, 0));
    s.power(p(-5, 0), "GND");
    s.wire(p(2, 0), p(5, 0));
    s.label(p(5, 0), "OUT", LabelScope::Local);
    let c = compile1(&s.doc());
    assert_eq!(members(c.net("VCC").unwrap()), ["R1.1", "U1.14"]);
    assert_eq!(members(c.net("GND").unwrap()), ["R1.2", "U1.1", "U1.7"]);
    let node = c.net("VCC").unwrap().nodes.iter().find(|n| n.designator == "U1").unwrap();
    assert_eq!(node.pin_type, PinType::PowerIn);
    assert_eq!(node.pads, ["14"]);
    assert_eq!(count(&c, "unconnected-pin"), 0);
    // The VCC/GND nets have power ports, so they count as driven.
    assert_eq!(count(&c, "power-not-driven"), 0);
}

#[test]
fn hidden_power_pins_without_port_are_not_driven() {
    let mut s = Sch::default();
    s.part(&inverter(), "U1", p(0, 0));
    s.nc(p(-2, 0));
    s.nc(p(2, 0));
    let c = compile1(&s.doc());
    assert_eq!(members(c.net("VCC").unwrap()), ["U1.14"]);
    assert_eq!(count(&c, "power-not-driven"), 2);
    assert_eq!(count(&c, "unconnected-pin"), 0, "hidden pins and no-connect pins are exempt");
    assert_eq!(count(&c, "single-pin-net"), 0);
}

#[test]
fn power_symbol_parts_act_as_ports() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part(&gnd_symbol(), "#PWR01", p(1, 0));
    s.part(&resistor(), "R2", p(0, 5));
    s.power(p(1, 5), "GND");
    s.nc(p(-1, 0));
    s.nc(p(-1, 5));
    let c = compile1(&s.doc());
    assert_eq!(members(c.net("GND").unwrap()), ["R1.2", "R2.2"]);
    assert!(c.part("#PWR01").is_none(), "power symbols are not netlist parts");
    assert!(c.violations.is_empty(), "{:?}", c.violations);
}

#[test]
fn buses_do_not_carry_nets_but_labels_do() {
    let build = |labels: bool| {
        let r = resistor();
        let mut s = Sch::default();
        s.part(&r, "R1", p(0, 0));
        s.part(&r, "R2", p(0, -4));
        s.add(Item::Bus(Bus { a: p(5, 5), b: p(5, -10) }));
        s.wire(p(1, 0), p(4, 0));
        s.add(Item::BusEntry(BusEntry { pos: p(4, 0), size: p(1, 1) }));
        s.wire(p(1, -4), p(4, -4));
        s.add(Item::BusEntry(BusEntry { pos: p(4, -4), size: p(1, -1) }));
        if labels {
            s.label(p(2, 0), "D0", LabelScope::Local);
            s.label(p(2, -4), "D0", LabelScope::Local);
        }
        s.label(p(5, 2), "D[0..7]", LabelScope::Local); // bus label
        compile1(&s.doc())
    };
    let c = build(false);
    assert!(!same_net(&c, ("R1", "2"), ("R2", "2")));
    assert_eq!(count(&c, "dangling-wire"), 0, "{:?}", c.violations);
    assert_eq!(count(&c, "unconnected-label"), 0, "bus labels are fine");
    let c = build(true);
    assert!(same_net(&c, ("R1", "2"), ("R2", "2")));
    assert_eq!(c.net_of("R1", "2").unwrap().name, "D0");
}

// ---------- Hierarchy ----------

/// Top: R1.2 wired to entry VIN of sheet "Power"; child: hier label VIN on R2.1.
fn hierarchy(extra_entry: bool, extra_label: bool) -> (Document, Document) {
    let r = resistor();
    let mut top = Sch::default();
    top.part(&r, "R1", p(0, 8));
    top.power(p(-1, 8), "VCC");
    let mut entries = vec![("VIN", SheetSide::Left, 2)];
    if extra_entry {
        entries.push(("EN", SheetSide::Left, 4));
    }
    top.sheet(p(10, 10), "Power", "power.arsch", &entries);
    top.wire(p(1, 8), p(10, 8));
    if extra_entry {
        top.wire(p(10, 6), p(8, 6));
        top.label(p(8, 6), "EN", LabelScope::Local);
    }
    let mut child = Sch::default();
    child.part(&r, "R2", p(0, 0));
    child.label(p(-1, 0), "VIN", LabelScope::Hierarchical);
    child.label(p(1, 0), "X", LabelScope::Local);
    child.part(&r, "R3", p(0, 5));
    child.label(p(1, 5), "X", LabelScope::Local);
    child.power(p(-1, 5), "GND");
    if extra_label {
        child.part(&r, "R4", p(0, 10));
        child.label(p(-1, 10), "FOO", LabelScope::Hierarchical);
        child.nc(p(1, 10));
    }
    (top.doc(), child.doc())
}

#[test]
fn hierarchical_label_connects_to_sheet_entry() {
    let (top, child) = hierarchy(false, false);
    // Child listed first: roots are found by reference, not by order.
    let c = compile(&[SheetRef::new("power.arsch", &child), SheetRef::new("top.arsch", &top)]);
    assert_eq!(c.instances.len(), 2);
    assert_eq!(c.instances[0].path, "/");
    assert_eq!(c.instances[0].sheet, "top.arsch");
    assert_eq!(c.instances[1].path, "/Power/");
    assert_eq!(c.instances[1].parent.as_deref(), Some("/"));
    assert_eq!(c.instances[1].sheet_item, Some(ItemId(2)));

    let vin = c.net_of("R1", "2").unwrap();
    assert_eq!(members(vin), ["R1.2", "R2.1"]);
    assert_eq!(vin.name, "/Power/VIN");
    assert!(vin.items.contains(&("top.arsch".to_string(), ItemId(3))));
    assert_eq!(c.net_of("R2", "2").unwrap().name, "/Power/X");
    assert_eq!(members(c.net_of("R2", "2").unwrap()), ["R2.2", "R3.2"]);
    assert_eq!(count(&c, "missing-sheet-entry") + count(&c, "unused-sheet-entry"), 0, "{:?}", c.violations);
    assert_eq!(c.net_of("R2", "2").unwrap().nodes[0].sheet, "power.arsch");
}

#[test]
fn sheet_entry_mismatches_are_reported() {
    let (top, child) = hierarchy(true, true);
    let c = compile(&[SheetRef::new("top.arsch", &top), SheetRef::new("power.arsch", &child)]);
    let missing = find(&c, "missing-sheet-entry");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].severity, Severity::Error);
    assert_eq!(missing[0].sheet, "power.arsch");
    assert!(missing[0].message.contains("FOO"));
    let unused = find(&c, "unused-sheet-entry");
    assert_eq!(unused.len(), 1);
    assert_eq!(unused[0].severity, Severity::Warning);
    assert_eq!(unused[0].sheet, "top.arsch");
    assert_eq!(unused[0].pos, Some(p(10, 6)));
}

#[test]
fn child_instantiated_twice_compiles_each_instance() {
    let r = resistor();
    let mut top = Sch::default();
    top.sheet(p(0, 0), "A", "ch.arsch", &[]);
    top.sheet(p(20, 0), "B", "./ch.arsch", &[]);
    let mut ch = Sch::default();
    ch.part(&r, "R1", p(0, 0));
    ch.label(p(1, 0), "L", LabelScope::Local);
    ch.power(p(-1, 0), "GND");
    let (dt, dc) = (top.doc(), ch.doc());
    let c = compile(&[SheetRef::new("top.arsch", &dt), SheetRef::new("ch.arsch", &dc)]);
    let paths: Vec<&str> = c.instances.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["/", "/A/", "/B/"]);
    assert!(c.net("/A/L").is_some() && c.net("/B/L").is_some());
    assert_eq!(members(c.net("GND").unwrap()), ["R1.1", "R1.1"]);
    let dup = find(&c, "duplicate-designator");
    assert_eq!(dup.len(), 1, "{:?}", c.violations);
    assert_eq!(dup[0].sheet, "ch.arsch");
}

#[test]
fn missing_child_and_recursion_are_errors() {
    let mut top = Sch::default();
    let sid = top.sheet(p(0, 0), "Gone", "gone.arsch", &[]);
    let dt = top.doc();
    let c = compile1(&dt);
    let v = find(&c, "missing-sheet");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].items, [sid]);
    assert_eq!(v[0].severity, Severity::Error);

    let mut a = Sch::default();
    a.sheet(p(0, 0), "B", "b.arsch", &[]);
    let mut b = Sch::default();
    b.sheet(p(0, 0), "A", "a.arsch", &[]);
    let (da, db) = (a.doc(), b.doc());
    let c = compile(&[SheetRef::new("a.arsch", &da), SheetRef::new("b.arsch", &db)]);
    assert_eq!(count(&c, "sheet-recursion"), 1);
    assert_eq!(c.instances.len(), 2);
}

// ---------- Naming ----------

#[test]
fn auto_names_use_first_node_and_pad() {
    let mut s = Sch::default();
    s.part(&resistor(), "R10", p(0, 0));
    s.part(&resistor(), "R2", p(10, 0));
    s.wire(p(1, 0), p(9, 0));
    s.part(&led(), "D1", p(0, 5));
    s.part(&resistor(), "R3", p(4, 5));
    s.wire(p(1, 5), p(3, 5));
    let c = compile1(&s.doc());
    assert_eq!(c.net_of("R10", "2").unwrap().name, "Net-(R2-Pad1)");
    let net = c.net_of("D1", "2").unwrap();
    assert_eq!(net.name, "Net-(D1-PadK)");
    assert_eq!(net.nodes[0].pads, ["K"]);
    assert_eq!(net.nodes[0].pin_name, "K");
}

#[test]
fn label_names_take_priority_and_global_conflicts_warn() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part(&resistor(), "R2", p(10, 0));
    s.wire(p(1, 0), p(9, 0));
    s.label(p(3, 0), "LOCAL", LabelScope::Local);
    s.power(p(5, 0), "VCC");
    s.label(p(7, 0), "+5V", LabelScope::Global);
    let c = compile1(&s.doc());
    assert_eq!(c.net_of("R1", "2").unwrap().name, "+5V");
    let v = find(&c, "multiple-net-names");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].severity, Severity::Warning);

    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.wire(p(1, 0), p(3, 0));
    s.label(p(3, 0), "LOCAL2", LabelScope::Local);
    s.label(p(1, 0), "LOCAL", LabelScope::Local);
    let c = compile1(&s.doc());
    assert_eq!(c.net_of("R1", "2").unwrap().name, "LOCAL");
    assert_eq!(count(&c, "multiple-net-names"), 0);
}

// ---------- ERC ----------

#[test]
fn erc_unannotated() {
    let mut s = Sch::default();
    s.part(&resistor(), "R?", p(0, 0));
    s.part(&resistor(), "", p(0, 5));
    s.part(&resistor(), "R1", p(0, 10));
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "unannotated"), 2);
    assert!(find(&c, "unannotated").iter().all(|v| v.severity == Severity::Error));
    // Unannotated parts are kept apart in the part list.
    assert_eq!(c.parts.len(), 3);
}

#[test]
fn erc_duplicate_designator() {
    let op = opamp();
    let mut s = Sch::default();
    s.part_unit(&op, "U1", p(0, 0), 1);
    s.part_unit(&op, "U1", p(10, 0), 2);
    s.part_unit(&op, "U1", p(20, 0), 3);
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "duplicate-designator"), 0);
    assert_eq!(c.parts.len(), 1);
    assert_eq!(c.parts[0].units.iter().map(|u| u.2).collect::<Vec<_>>(), [1, 2, 3]);

    s.part_unit(&op, "U1", p(30, 0), 1);
    s.part(&resistor(), "R1", p(0, 10));
    s.part(&led(), "R1", p(0, 20));
    let c = compile1(&s.doc());
    let v = find(&c, "duplicate-designator");
    assert_eq!(v.len(), 2, "{v:?}");
    assert!(v.iter().any(|v| v.message.contains("Unit A of U1")));
    assert!(v.iter().any(|v| v.message.contains("different components")));
}

#[test]
fn erc_missing_symbol() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part_unit(&resistor(), "R2", p(0, 5), 4);
    s.add(Item::Part(Part { pos: p(0, 10), rotation: 0.0, mirror: false, lib_ref: "Lib/Nope".into(), unit: 1, fields: vec![] }));
    let c = compile1(&s.doc());
    let v = find(&c, "missing-symbol");
    assert_eq!(v.len(), 2);
    assert!(v.iter().all(|v| v.severity == Severity::Error));
    assert_eq!(v[0].items, [ItemId(1)]);
}

#[test]
fn erc_unconnected_pins() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part_unit(&opamp(), "U1", p(20, 0), 1);
    s.nc(p(23, 0)); // output marked no-connect
    let c = compile1(&s.doc());
    let v = find(&c, "unconnected-pin");
    assert_eq!(v.len(), 4, "{v:?}");
    let errors: Vec<_> = v.iter().filter(|v| v.severity == Severity::Error).collect();
    assert_eq!(errors.len(), 2, "op-amp inputs are errors");
    assert!(errors.iter().all(|v| v.items == [ItemId(1)]));
    assert_eq!(v.iter().filter(|v| v.severity == Severity::Warning).count(), 2, "passive pins warn");
    assert!(v.iter().any(|v| v.pos == Some(p(-1, 0))));

    // Wired up, nothing is reported.
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part(&resistor(), "R2", p(2, 0));
    s.power(p(-1, 0), "A");
    s.power(p(3, 0), "B");
    let c = compile1(&s.doc());
    assert!(c.violations.is_empty(), "{:?}", c.violations);
}

#[test]
fn erc_no_connect_connected() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.part(&resistor(), "R2", p(10, 0));
    s.wire(p(1, 0), p(9, 0));
    let nc = s.nc(p(1, 0));
    s.nc(p(-1, 0)); // fine: R1.1 is otherwise unconnected
    s.power(p(11, 0), "GND");
    let c = compile1(&s.doc());
    let v = find(&c, "no-connect-connected");
    assert_eq!(v.len(), 1, "{:?}", c.violations);
    assert_eq!(v[0].pos, Some(p(1, 0)));
    assert_eq!(count(&c, "unconnected-pin"), 0);
    assert_eq!(count(&c, "dangling-wire"), 0);
    assert!(connected_items(&s.doc(), nc).contains(&ItemId(1)));
}

#[test]
fn erc_output_conflict() {
    let inv = inverter();
    let mut s = Sch::default();
    s.part(&inv, "U1", p(0, 0));
    s.part(&inv, "U2", p(0, 5));
    s.wire(p(2, 0), p(2, 5));
    s.power(p(-2, 0), "VCC");
    s.power(p(-2, 5), "VCC");
    s.power(p(0, 10), "GND"); // keeps GND net driven
    s.power(p(0, 9), "VCC");
    let c = compile1(&s.doc());
    let v = find(&c, "output-conflict");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].severity, Severity::Error);
    assert_eq!(v[0].items, [ItemId(0), ItemId(1)]);

    let oc = open_collector();
    let mut s = Sch::default();
    s.part(&oc, "U1", p(0, 0));
    s.part(&oc, "U2", p(0, 5));
    s.part(&inv, "U3", p(10, 0));
    s.wire(p(1, 0), p(1, 5));
    s.wire(p(1, 0), p(8, 0)); // into U3 input
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "output-conflict"), 0);
}

#[test]
fn erc_power_not_driven() {
    let op = opamp();
    let mut s = Sch::default();
    s.part_unit(&op, "U1", p(0, 0), 3);
    s.part(&resistor(), "R1", p(3, 2)); // R1.1 at (2,2)
    s.wire(p(0, 2), p(2, 2));
    s.power(p(0, -2), "GND");
    s.power(p(4, 2), "GND");
    let c = compile1(&s.doc());
    let v = find(&c, "power-not-driven");
    assert_eq!(v.len(), 1, "{:?}", c.violations);
    assert_eq!(v[0].pos, Some(p(0, 2)));

    // Driven by a regulator output.
    let mut s = Sch::default();
    s.part_unit(&op, "U1", p(0, 0), 3);
    s.part(&regulator(), "U2", p(-4, 2)); // VOUT at (-2, 2)
    s.wire(p(-2, 2), p(0, 2));
    s.power(p(0, -2), "GND");
    s.power(p(-4, 0), "GND");
    s.power(p(-6, 2), "VIN");
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "power-not-driven"), 0, "{:?}", c.violations);
}

#[test]
fn erc_single_pin_net_and_dangling_wire() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    let w = s.wire(p(1, 0), p(5, 0));
    s.power(p(-1, 0), "GND");
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "single-pin-net"), 1);
    assert_eq!(count(&c, "unconnected-pin"), 0);
    let d = find(&c, "dangling-wire");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].items, [w]);
    assert_eq!(d[0].pos, Some(p(5, 0)));

    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    s.wire(p(1, 0), p(5, 0));
    s.label(p(5, 0), "TP", LabelScope::Global);
    s.power(p(-1, 0), "GND");
    let c = compile1(&s.doc());
    assert_eq!(count(&c, "single-pin-net"), 0);
    assert_eq!(count(&c, "dangling-wire"), 0);
    assert_eq!(count(&c, "unconnected-label"), 0);
}

#[test]
fn erc_unconnected_label() {
    let mut s = Sch::default();
    let l = s.label(p(5, 5), "FLOAT", LabelScope::Local);
    let pp = s.power(p(8, 8), "VCC");
    s.wire(p(0, 0), p(2, 0));
    s.label(p(1, 0), "ONWIRE", LabelScope::Local);
    let c = compile1(&s.doc());
    let v = find(&c, "unconnected-label");
    assert_eq!(v.iter().map(|v| v.items[0]).collect::<Vec<_>>(), [l, pp]);
    assert!(v.iter().all(|v| v.severity == Severity::Warning));
}

// ---------- Annotation ----------

fn designators(sheets: &[SheetRef], mode: AnnotateMode) -> Vec<(ItemId, String)> {
    annotate(sheets, mode)
        .into_iter()
        .flat_map(|(_, items)| items)
        .map(|it| match it.item {
            Item::Part(p) => (it.id, p.designator().to_string()),
            _ => unreachable!(),
        })
        .collect()
}

#[test]
fn annotate_incremental_fills_gaps() {
    let r = resistor();
    let mut s = Sch::default();
    s.part(&r, "R1", p(0, 10));
    let a = s.part(&r, "R?", p(0, 5));
    let b = s.part(&r, "R?", p(5, 5));
    s.part(&r, "R3", p(0, 0));
    let d = s.part(&led(), "", p(0, -5));
    let doc = s.doc();
    let sheets = [SheetRef::new("top.arsch", &doc)];
    assert_eq!(designators(&sheets, AnnotateMode::Incremental), [(a, "R2".into()), (b, "R4".into()), (d, "D1".into())]);
    let out = annotate(&sheets, AnnotateMode::Incremental);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "top.arsch");
}

#[test]
fn annotate_all_renumbers_by_position() {
    let r = resistor();
    let mut s = Sch::default();
    let low = s.part(&r, "R1", p(0, 0));
    let high = s.part(&r, "R7", p(0, 10));
    let mid_right = s.part(&r, "R2", p(5, 5));
    let mid_left = s.part(&r, "R?", p(0, 5));
    let doc = s.doc();
    let sheets = [SheetRef::new("top.arsch", &doc)];
    let mut got = designators(&sheets, AnnotateMode::All);
    got.sort();
    let mut want = vec![(high, "R1".to_string()), (mid_left, "R2".into()), (mid_right, "R3".into()), (low, "R4".into())];
    want.sort();
    assert_eq!(got, want);

    // Already in order: nothing changes.
    let mut s = Sch::default();
    s.part(&r, "R1", p(0, 10));
    s.part(&r, "R2", p(0, 0));
    let doc = s.doc();
    assert!(annotate(&[SheetRef::new("top.arsch", &doc)], AnnotateMode::All).is_empty());
}

#[test]
fn annotate_packs_units_of_multi_unit_parts() {
    let op = opamp();
    let mut s = Sch::default();
    let a1 = s.add(Item::Part(Part { pos: p(0, 10), rotation: 90.0, mirror: false, lib_ref: op.key(), unit: 1, fields: vec![] }));
    s.cache.push(op.clone());
    let b1 = s.part_unit(&op, "U?", p(10, 10), 2);
    let a2 = s.part_unit(&op, "U?", p(0, 0), 1);
    let r = s.part(&resistor(), "R?", p(5, 5));
    let doc = s.doc();
    let sheets = [SheetRef::new("top.arsch", &doc)];
    let got = designators(&sheets, AnnotateMode::All);
    assert_eq!(got, [(a1, "U1".into()), (b1, "U1".into()), (r, "R1".into()), (a2, "U2".into())]);

    // The created field is visible and placed from the rotated template.
    let out = annotate(&sheets, AnnotateMode::All);
    let Item::Part(part) = &out[0].1[0].item else { unreachable!() };
    let f = part.field(FIELD_DESIGNATOR).unwrap();
    assert!(f.visible);
    assert_eq!(f.offset, p(-3, 2));
    assert_eq!(f.rotation, 90.0);

    // Incremental: an unannotated unit B joins the existing U1 (unit A).
    let mut s = Sch::default();
    s.part_unit(&op, "U1", p(0, 10), 1);
    let b = s.part_unit(&op, "U?", p(0, 0), 2);
    let c = s.part_unit(&op, "U?", p(5, 0), 2);
    let doc = s.doc();
    let got = designators(&[SheetRef::new("top.arsch", &doc)], AnnotateMode::Incremental);
    assert_eq!(got, [(b, "U1".into()), (c, "U2".into())]);
}

#[test]
fn annotate_across_sheets_in_order() {
    let r = resistor();
    let mut a = Sch::default();
    let a1 = a.part(&r, "R?", p(0, 0));
    let mut b = Sch::default();
    let b1 = b.part(&r, "R?", p(0, 100));
    let (da, db) = (a.doc(), b.doc());
    let out = annotate(&[SheetRef::new("a.arsch", &da), SheetRef::new("b.arsch", &db)], AnnotateMode::Incremental);
    assert_eq!(out.len(), 2);
    let des = |i: usize| match &out[i].1[0].item {
        Item::Part(p) => (out[i].0.clone(), out[i].1[0].id, p.designator().to_string()),
        _ => unreachable!(),
    };
    assert_eq!(des(0), ("a.arsch".into(), a1, "R1".into()));
    assert_eq!(des(1), ("b.arsch".into(), b1, "R2".into()));
}

// ---------- BOM ----------

#[test]
fn bom_groups_and_sorts_naturally() {
    let r = resistor();
    let mut s = Sch::default();
    for (i, d) in ["R10", "R2", "R1"].iter().enumerate() {
        s.part(&r, d, p(0, i as i64 * 5));
    }
    let k = s.part(&r, "R3", p(10, 0));
    if let Item::Part(part) = &mut s.items[k.0 as usize] {
        part.set_field(FIELD_VALUE, "1k");
        part.set_field("MPN", "RC0603, \"1k\"");
    }
    s.part(&led(), "D1", p(20, 0));
    let doc = s.doc();
    let c = compile1(&doc);
    let csv = bom_csv(&c);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Designator,Quantity,Value,Footprint,Component,Library,Description,MPN");
    assert_eq!(lines[1], "D1,1,,LED_0603,LED,Lib,LED part,");
    assert_eq!(lines[2], "\"R1, R2, R10\",3,10k,R_0603,R,Lib,R part,");
    assert_eq!(lines[3], "R3,1,1k,R_0603,R,Lib,R part,\"RC0603, \"\"1k\"\"\"");
    assert_eq!(lines.len(), 4);
    assert!(csv.ends_with("\r\n"));
}

#[test]
fn natural_ordering() {
    let mut v = vec!["R10", "R2", "C1", "R1", "r3", "U1A", "U1"];
    v.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(v, ["C1", "R1", "R2", "r3", "R10", "U1", "U1A"]);
}

// ---------- Highlighting & serialization ----------

#[test]
fn connected_items_follows_wires_and_local_labels() {
    let r = resistor();
    let mut s = Sch::default();
    let r1 = s.part(&r, "R1", p(0, 0));
    let r2 = s.part(&r, "R2", p(10, 0));
    let w1 = s.wire(p(1, 0), p(9, 0));
    let l1 = s.label(p(5, 0), "X", LabelScope::Local);
    let r3 = s.part(&r, "R3", p(0, 10));
    let w2 = s.wire(p(1, 10), p(5, 10));
    let l2 = s.label(p(5, 10), "X", LabelScope::Local);
    let w3 = s.wire(p(-1, 10), p(-5, 10));
    let doc = s.doc();
    let mut want = vec![r1, r2, w1, l1, r3, w2, l2];
    want.sort();
    assert_eq!(connected_items(&doc, w1), want);
    assert_eq!(connected_items(&doc, l2), want);
    let other = connected_items(&doc, w3);
    assert!(other.contains(&w3) && other.contains(&r3) && !other.contains(&w1));
    assert!(connected_items(&doc, ItemId(999)).is_empty());
}

#[test]
fn compiled_serializes_camel_case() {
    let mut s = Sch::default();
    s.part(&resistor(), "R1", p(0, 0));
    let c = compile1(&s.doc());
    let v = serde_json::to_value(&c).unwrap();
    assert!(v["violations"][0]["severity"] == "warning");
    assert!(v["violations"][0]["code"] == "unconnected-pin");
    assert!(v["parts"][0]["libRef"] == "Lib/R");
    assert!(v["instances"][0]["sheetItem"].is_null());
}
