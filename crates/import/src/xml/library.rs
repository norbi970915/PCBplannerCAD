//! XML `<library>` → PCBPlannerCAD [`Library`]: packages become footprints,
//! each deviceset becomes one symbol (one unit per gate) and every
//! device/technology combination becomes a component.

use std::collections::{BTreeSet, HashMap, HashSet};

use pcbplannercad_core::item::{FIELD_DESIGNATOR, FIELD_VALUE, Field, HAlign, Item, Pin, PinShape, PinType, PowerStyle, VAlign};
use pcbplannercad_core::library::{Parameter, PinMapping, SymbolUnit};
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::{Component, Library, Symbol};
use roxmltree::Node;

use super::package::{PackageNotes, PadRules, convert_package};
use super::util::{
    LayerSkips, Rot, attr, attr_str, children, coord, description, elements, graphic, grandchildren, layer_num, parse_align,
    placeholder, point, resolve_text, schematic_layer, strip_at, text_item,
};
use crate::ImportReport;

/// An attribute placeholder text (`>NAME`, `>VALUE`, `>MPN`…) in a gate symbol,
/// kept in XML terms so it can be composed with an instance placement.
#[derive(Clone, Debug)]
pub(crate) struct FieldTemplate {
    /// PCBPlannerCAD field name (`Designator`, `Value`, or the XML attribute name).
    pub name: String,
    pub pos: Point,
    pub rot: Rot,
    pub height: Coord,
    pub h: HAlign,
    pub v: VAlign,
}

impl FieldTemplate {
    /// The template as a symbol-local [`Field`].
    pub(crate) fn to_field(&self, value: String) -> Field {
        let (rotation, h_align, v_align) = resolve_text(self.rot, self.h, self.v, true);
        Field { name: self.name.clone(), value, offset: self.pos, rotation, height: self.height, h_align, v_align, visible: true }
    }
}

pub(crate) fn field_name(placeholder: &str) -> String {
    match placeholder {
        "NAME" => FIELD_DESIGNATOR.to_string(),
        "VALUE" => FIELD_VALUE.to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GateInfo {
    pub name: String,
    pub xml_symbol: String,
    pub fields: Vec<FieldTemplate>,
    /// XML pin name → PCBPlannerCAD pin number.
    pub pin_numbers: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DevicesetInfo {
    pub symbol: String,
    pub gates: Vec<GateInfo>,
    pub is_power: bool,
    pub power_style: PowerStyle,
}

impl DevicesetInfo {
    pub(crate) fn gate_index(&self, gate: &str) -> Option<usize> {
        self.gates.iter().position(|g| g.name == gate)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ConvertedLib {
    pub library: Library,
    pub devicesets: HashMap<String, DevicesetInfo>,
    /// `(deviceset, device, technology)` → component name.
    pub components: HashMap<(String, String, String), String>,
}

impl ConvertedLib {
    /// Component for a placed part, falling back to any technology of the
    /// device, then any component of the deviceset.
    pub(crate) fn component_for(&self, deviceset: &str, device: &str, technology: &str) -> Option<&str> {
        let key = (deviceset.to_string(), device.to_string(), technology.to_string());
        if let Some(c) = self.components.get(&key) {
            return Some(c);
        }
        let mut fallback: Vec<(&(String, String, String), &String)> =
            self.components.iter().filter(|((ds, dev, _), _)| ds == deviceset && dev == device).collect();
        if fallback.is_empty() {
            fallback = self.components.iter().filter(|((ds, _, _), _)| ds == deviceset).collect();
        }
        fallback.sort();
        fallback.first().map(|(_, c)| c.as_str())
    }
}

// ---------- Pins ----------

pub(crate) fn pin_length(s: Option<&str>) -> Coord {
    match s.unwrap_or("long") {
        "point" => 0,
        "short" => mm(2.54),
        "middle" => mm(5.08),
        _ => mm(7.62),
    }
}

pub(crate) fn pin_type(s: Option<&str>) -> PinType {
    match s.unwrap_or("io") {
        "nc" => PinType::NoConnect,
        "in" => PinType::Input,
        "out" => PinType::Output,
        "oc" => PinType::OpenCollector,
        "hiz" => PinType::TriState,
        "pas" => PinType::Passive,
        "pwr" => PinType::PowerIn,
        "sup" => PinType::PowerOut,
        _ => PinType::Bidirectional,
    }
}

pub(crate) fn pin_shape(s: Option<&str>) -> PinShape {
    match s.unwrap_or("none") {
        "dot" => PinShape::Inverted,
        "clk" => PinShape::Clock,
        "dotclk" => PinShape::InvertedClock,
        _ => PinShape::Line,
    }
}

/// `(name visible, number visible)` for an XML `visible` value.
pub(crate) fn pin_visibility(s: Option<&str>) -> (bool, bool) {
    match s.unwrap_or("both") {
        "off" => (false, false),
        "pad" => (false, true),
        "pin" => (true, false),
        _ => (true, true),
    }
}

pub(crate) fn convert_pin(n: Node<'_, '_>, number: String) -> Pin {
    Pin {
        pos: point(n, "x", "y"),
        length: pin_length(attr(n, "length")),
        rotation: Rot::of(n).direction(),
        name: strip_at(attr(n, "name").unwrap_or("")).to_string(),
        number,
        electrical: pin_type(attr(n, "direction")),
        shape: pin_shape(attr(n, "function")),
        hidden: false,
    }
}

// ---------- Symbols ----------

struct UnitData {
    items: Vec<Item>,
    fields: Vec<FieldTemplate>,
    names_visible: bool,
    numbers_visible: bool,
}

fn convert_unit(sym: Node<'_, '_>, numbers: &HashMap<String, String>, skips: &mut LayerSkips) -> UnitData {
    let mut u = UnitData { items: Vec::new(), fields: Vec::new(), names_visible: false, numbers_visible: false };
    for e in elements(sym) {
        let tag = e.tag_name().name();
        if tag == "pin" {
            let name = attr_str(e, "name");
            let number = numbers.get(&name).cloned().unwrap_or_else(|| name.clone());
            let (nv, numv) = pin_visibility(attr(e, "visible"));
            u.names_visible |= nv;
            u.numbers_visible |= numv;
            u.items.push(Item::Pin(convert_pin(e, number)));
            continue;
        }
        if !matches!(tag, "text" | "wire" | "circle" | "rectangle" | "polygon" | "polygonshape" | "frame") {
            continue;
        }
        let ln = layer_num(e);
        let Some(layer) = ln.and_then(schematic_layer) else {
            skips.add(ln);
            continue;
        };
        if tag == "text" {
            let raw = e.text().unwrap_or("");
            if let Some(ph) = placeholder(raw) {
                let name = field_name(&ph);
                if !u.fields.iter().any(|f| f.name == name) {
                    let (h, v) = parse_align(attr(e, "align"));
                    let height = coord(e, "size");
                    u.fields.push(FieldTemplate {
                        name,
                        pos: point(e, "x", "y"),
                        rot: Rot::of(e),
                        height: if height > 0 { height } else { mm(1.778) },
                        h,
                        v,
                    });
                }
            } else {
                u.items.extend(text_item(e, raw, layer, true));
            }
        } else {
            u.items.extend(graphic(e, layer).unwrap_or_default());
        }
    }
    u
}

// ---------- Devicesets ----------

struct Device {
    name: String,
    package: Option<String>,
    /// `(gate, pin, pads)`.
    connects: Vec<(String, String, Vec<String>)>,
    /// `(technology name, attributes)`.
    techs: Vec<(String, Vec<(String, String)>)>,
    attrs: Vec<(String, String)>,
}

fn att_values(n: Node<'_, '_>) -> Vec<(String, String)> {
    grandchildren(n, "att_values", "att_value").map(|a| (attr_str(a, "name"), attr_str(a, "value"))).collect()
}

fn parse_device(n: Node<'_, '_>) -> Device {
    let connects = grandchildren(n, "connects", "connect")
        .map(|c| (attr_str(c, "gate"), attr_str(c, "pin"), attr_str(c, "pad").split_whitespace().map(str::to_string).collect()))
        .collect();
    let mut techs: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut add_tech = |name: String, attrs: Vec<(String, String)>| match techs.iter_mut().find(|(t, _)| *t == name) {
        Some((_, a)) => a.extend(attrs),
        None => techs.push((name, attrs)),
    };
    for t in grandchildren(n, "technologies", "technology") {
        let attrs = children(t, "attribute").map(|a| (attr_str(a, "name"), attr_str(a, "value"))).collect();
        add_tech(attr_str(t, "name"), attrs);
    }
    // XML design (XML 9.7) attribute sets play the role of technologies.
    for s in grandchildren(n, "att_sets", "att_set") {
        add_tech(attr_str(s, "name"), att_values(s));
    }
    if techs.is_empty() {
        techs.push((String::new(), Vec::new()));
    }
    Device {
        name: attr_str(n, "name"),
        package: attr(n, "package").filter(|p| !p.is_empty()).map(str::to_string),
        connects,
        techs,
        attrs: att_values(n),
    }
}

/// XML device name: `*` → technology, `?` → device (package variant);
/// without wildcards the device and then the technology are appended.
pub(crate) fn full_device_name(deviceset: &str, device: &str, technology: &str) -> String {
    let mut s = if deviceset.contains('?') { deviceset.replace('?', device) } else { format!("{deviceset}{device}") };
    if s.contains('*') {
        s = s.replace('*', technology);
    } else {
        s.push_str(technology);
    }
    if s.is_empty() { "Component".to_string() } else { s }
}

pub(crate) fn unique_name(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    (2..).map(|i| format!("{base}_{i}")).find(|n| used.insert(n.clone())).unwrap_or(base)
}

pub(crate) fn power_style(names: &[&str]) -> PowerStyle {
    for n in names {
        let u = n.to_ascii_uppercase();
        if u.contains("AGND") || u.contains("GNDA") {
            return PowerStyle::SignalGround;
        }
        if u.contains("EARTH") || u == "PE" {
            return PowerStyle::Earth;
        }
        if u.contains("GND") {
            return PowerStyle::Ground;
        }
    }
    PowerStyle::Bar
}

/// Whether symbol pins can carry the pad names directly: exactly one
/// device with a package, and each gate pin connects to exactly one pad.
pub(crate) fn pad_numbering(
    gate_pins: &[(String, Vec<String>)],
    packaged_connects: &[&[(String, String, Vec<String>)]],
) -> Option<HashMap<(String, String), String>> {
    let [connects] = packaged_connects else { return None };
    let mut map = HashMap::new();
    let mut pads_seen = HashSet::new();
    for (gate, pins) in gate_pins {
        for pin in pins {
            let mut hits = connects.iter().filter(|(g, p, _)| g == gate && p == pin);
            let (Some((_, _, pads)), None) = (hits.next(), hits.next()) else { return None };
            let [pad] = pads.as_slice() else { return None };
            if !pads_seen.insert(pad.clone()) {
                return None;
            }
            map.insert((gate.clone(), pin.clone()), pad.clone());
        }
    }
    Some(map)
}

/// Unique pin numbers from pin names: names repeated across gates get a
/// `GATE.` prefix.
pub(crate) fn name_numbering(gate_pins: &[(String, Vec<String>)]) -> HashMap<(String, String), String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for (_, pins) in gate_pins {
        for p in pins {
            *count.entry(p.as_str()).or_default() += 1;
        }
    }
    let mut used = HashSet::new();
    let mut map = HashMap::new();
    for (gate, pins) in gate_pins {
        for p in pins {
            let base = if count[p.as_str()] > 1 { format!("{gate}.{p}") } else { p.clone() };
            map.insert((gate.clone(), p.clone()), unique_name(base, &mut used));
        }
    }
    map
}

struct LibCtx<'a, 'i> {
    lib: String,
    symbols: HashMap<String, Node<'a, 'i>>,
    footprints: HashSet<String>,
    skips: LayerSkips,
    missing_packages: BTreeSet<String>,
    used_components: HashSet<String>,
}

fn convert_deviceset(ds: Node<'_, '_>, ctx: &mut LibCtx<'_, '_>, out: &mut ConvertedLib, report: &mut ImportReport) {
    let ds_name = attr_str(ds, "name");
    let context = format!("{}:{}", ctx.lib, ds_name);
    let mut gates: Vec<(String, String, Node<'_, '_>)> = Vec::new();
    for g in grandchildren(ds, "gates", "gate") {
        let (gname, sname) = (attr_str(g, "name"), attr_str(g, "symbol"));
        match ctx.symbols.get(&sname) {
            Some(s) => gates.push((gname, sname, *s)),
            None => report.warn(&context, format!("gate {gname}: symbol {sname} not found; gate skipped")),
        }
    }
    if gates.is_empty() {
        report.warn(&context, "deviceset has no usable gates; skipped");
        return;
    }
    let devices: Vec<Device> = grandchildren(ds, "devices", "device").map(parse_device).collect();
    let gate_pins: Vec<(String, Vec<String>)> =
        gates.iter().map(|(g, _, s)| (g.clone(), children(*s, "pin").map(|p| attr_str(p, "name")).collect())).collect();

    let packaged: Vec<&[(String, String, Vec<String>)]> =
        devices.iter().filter(|d| d.package.is_some()).map(|d| d.connects.as_slice()).collect();
    let pad_map = pad_numbering(&gate_pins, &packaged);
    let pad_mode = pad_map.is_some();
    let numbers = pad_map.unwrap_or_else(|| name_numbering(&gate_pins));

    // Supply symbol: no packaged device, one gate, one `sup` pin.
    let is_power = packaged.is_empty() && gates.len() == 1 && {
        let pins: Vec<Node<'_, '_>> = children(gates[0].2, "pin").collect();
        pins.len() == 1 && attr(pins[0], "direction") == Some("sup")
    };

    let mut symbol = Symbol::new(ds_name.clone(), attr(ds, "prefix").filter(|p| !p.is_empty()).unwrap_or("U"));
    symbol.description = description(ds);
    symbol.is_power = is_power;
    symbol.units.clear();
    let (mut names_visible, mut numbers_visible) = (false, false);
    let mut infos = Vec::new();
    for (gname, sname, snode) in &gates {
        let pin_numbers: HashMap<String, String> =
            numbers.iter().filter(|((g, _), _)| g == gname).map(|((_, p), n)| (p.clone(), n.clone())).collect();
        let unit = convert_unit(*snode, &pin_numbers, &mut ctx.skips);
        names_visible |= unit.names_visible;
        numbers_visible |= unit.numbers_visible;
        symbol.units.push(SymbolUnit { name: gname.clone(), items: unit.items });
        infos.push(GateInfo { name: gname.clone(), xml_symbol: sname.clone(), fields: unit.fields, pin_numbers });
    }
    symbol.show_pin_names = names_visible;
    symbol.show_pin_numbers = pad_mode && numbers_visible;
    symbol.fields = infos[0]
        .fields
        .iter()
        .map(|t| {
            let value = if t.name == FIELD_DESIGNATOR { symbol.prefix.clone() } else { String::new() };
            t.to_field(value)
        })
        .collect();
    let power = power_style(&[infos[0].xml_symbol.as_str(), ds_name.as_str()]);

    // Components.
    let ds_attrs = att_values(ds);
    let ds_desc = symbol.description.clone();
    for dev in &devices {
        if let Some(p) = &dev.package
            && !ctx.footprints.contains(p)
        {
            ctx.missing_packages.insert(p.clone());
        }
        let pin_map: Vec<PinMapping> = if pad_mode || dev.package.is_none() {
            Vec::new()
        } else {
            dev.connects
                .iter()
                .filter_map(|(g, p, pads)| {
                    let unit = infos.iter().position(|i| &i.name == g)? as u32 + 1;
                    let pin = numbers.get(&(g.clone(), p.clone()))?.clone();
                    Some(PinMapping { unit, pin, pads: pads.clone() })
                })
                .collect()
        };
        for (tech, tech_attrs) in &dev.techs {
            let name = unique_name(full_device_name(&ds_name, &dev.name, tech), &mut ctx.used_components);
            let mut params = vec![Parameter { name: FIELD_VALUE.to_string(), value: name.clone() }];
            for (k, v) in ds_attrs.iter().chain(&dev.attrs).chain(tech_attrs) {
                if v.is_empty() || k.is_empty() {
                    continue;
                }
                let k = if k.eq_ignore_ascii_case("value") { FIELD_VALUE } else { k.as_str() };
                match params.iter_mut().find(|p| p.name == k) {
                    Some(p) => p.value = v.clone(),
                    None => params.push(Parameter { name: k.to_string(), value: v.clone() }),
                }
            }
            out.components.insert((ds_name.clone(), dev.name.clone(), tech.clone()), name.clone());
            out.library.components.push(Component {
                name,
                description: ds_desc.clone(),
                symbol: symbol.name.clone(),
                footprints: dev.package.iter().cloned().collect(),
                pin_map: pin_map.clone(),
                parameters: params,
            });
        }
    }
    out.devicesets.insert(ds_name, DevicesetInfo { symbol: symbol.name.clone(), gates: infos, is_power, power_style: power });
    out.library.symbols.push(symbol);
}

/// Converts one `<library>` element (from a `.lbr` or embedded in a `.sch`).
pub(crate) fn convert_library(lib: Node<'_, '_>, name: &str, report: &mut ImportReport) -> ConvertedLib {
    let mut out = ConvertedLib { library: Library { name: name.to_string(), ..Library::default() }, ..ConvertedLib::default() };
    let mut skips = LayerSkips::default();
    let mut notes = PackageNotes::default();
    let mut fp_names = HashSet::new();
    for p in grandchildren(lib, "packages", "package") {
        let fp = convert_package(p, &mut skips, &mut notes, &PadRules::default());
        if fp_names.insert(fp.name.clone()) {
            out.library.footprints.push(fp);
        } else {
            report.warn(name, format!("duplicate package {} ignored", fp.name));
        }
    }
    let mut ctx = LibCtx {
        lib: name.to_string(),
        symbols: grandchildren(lib, "symbols", "symbol").map(|s| (attr_str(s, "name"), s)).collect(),
        footprints: fp_names,
        skips,
        missing_packages: BTreeSet::new(),
        used_components: HashSet::new(),
    };
    for ds in grandchildren(lib, "devicesets", "deviceset") {
        convert_deviceset(ds, &mut ctx, &mut out, report);
    }

    notes.report(name, report);
    if !ctx.missing_packages.is_empty() {
        let list: Vec<&str> = ctx.missing_packages.iter().map(String::as_str).collect();
        report.warn(name, format!("devices reference missing packages: {}", list.join(", ")));
    }
    if let Some(s) = ctx.skips.summary() {
        report.info(name, s);
    }
    out
}
