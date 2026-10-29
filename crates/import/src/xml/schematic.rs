//! XML `.sch` → one pCAD schematic document per `<sheet>`.

use std::collections::{BTreeMap, HashMap, HashSet};

use pcbplannercad_core::geom::{on_segment, Placement};
use pcbplannercad_core::item::{
    FIELD_DESIGNATOR, FIELD_VALUE, Field, HAlign, Item, Junction, LabelScope, NetLabel, Part, PortShape, PowerPort, VAlign, Wire,
};
use pcbplannercad_core::library::CachedPart;
use pcbplannercad_core::units::{Point, mm};
use pcbplannercad_core::{DocKind, Document};
use roxmltree::Node;

use super::library::{ConvertedLib, GateInfo, convert_library, field_name, unique_name};
use super::util::{
    LayerSkips, Rot, attr, attr_str, child, children, coord, elements, grandchildren, graphic, is_yes, layer_num, parse_align,
    parse_xml, point, resolve_text, schematic_layer, strip_at, text_item,
};
use crate::{ImportError, ImportReport, ImportedSheet, SchematicImport};

struct PartInfo {
    lib: Option<usize>,
    deviceset: String,
    component: Option<String>,
    value: Option<String>,
    attrs: Vec<(String, String)>,
}

/// One XML net segment, kept for cross-sheet label synthesis.
struct Segment {
    sheet: usize,
    net: String,
    wires: Vec<(Point, Point)>,
    has_label: bool,
    has_port: bool,
    pins: Vec<Point>,
    junctions: Vec<Point>,
}

#[derive(Default)]
struct Stats {
    parts: usize,
    power_ports: usize,
    wires: usize,
    labels: usize,
    snapped_labels: usize,
    moved_labels: usize,
    curved_wires: usize,
    frames: usize,
    dimensions: usize,
    module_instances: usize,
    unresolved: usize,
}

type PinKey = (String, String, String);

fn pin_key(n: Node<'_, '_>) -> PinKey {
    (attr_str(n, "part"), attr_str(n, "gate"), attr_str(n, "pin"))
}

/// Moves `p` onto the nearest point of `wires` (labels in XML may sit
/// beside their segment; pCAD labels connect at their position).
fn snap_to_wires(p: Point, wires: &[(Point, Point)]) -> (Point, bool) {
    if wires.is_empty() || wires.iter().any(|(a, b)| on_segment(p, *a, *b)) {
        return (p, false);
    }
    let mut best: Option<(f64, Point)> = None;
    for &(a, b) in wires {
        let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let mut q = Point::new(a.x + (dx * t).round() as i64, a.y + (dy * t).round() as i64);
        if !on_segment(q, a, b) {
            q = if t < 0.5 { a } else { b };
        }
        let d = ((q.x - p.x) as f64).hypot((q.y - p.y) as f64);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    }
    best.map_or((p, false), |(_, q)| (q, true))
}

/// Wire ends of a segment that touch nothing (no other wire, pin, junction).
fn dangling_ends(wires: &[(Point, Point)], pins: &[Point], junctions: &[Point]) -> Vec<Point> {
    let mut out = Vec::new();
    for (i, &(a, b)) in wires.iter().enumerate() {
        for e in [a, b] {
            let touches = wires.iter().enumerate().any(|(j, &(c, d))| j != i && on_segment(e, c, d))
                || pins.contains(&e)
                || junctions.contains(&e);
            if !touches {
                out.push(e);
            }
        }
    }
    out
}

/// A net/bus label. XML labels may sit beside or in the middle of their
/// wire; the label is snapped onto the segment and, when it lies on a wire
/// with a free (dangling) end, moved to that end so the stub is terminated.
fn label_item(n: Node<'_, '_>, text: &str, wires: &[(Point, Point)], dangling: &mut Vec<Point>, stats: &mut Stats) -> NetLabel {
    let (mut pos, snapped) = snap_to_wires(point(n, "x", "y"), wires);
    if snapped {
        stats.snapped_labels += 1;
    }
    if !dangling.contains(&pos) {
        let end = wires.iter().filter(|(a, b)| on_segment(pos, *a, *b)).flat_map(|&(a, b)| [a, b]).find(|e| dangling.contains(e));
        if let Some(e) = end {
            pos = e;
            stats.moved_labels += 1;
        }
    }
    dangling.retain(|d| *d != pos);
    stats.labels += 1;
    NetLabel {
        pos,
        text: text.to_string(),
        rotation: Rot::of(n).text_direction(),
        scope: LabelScope::Global,
        shape: if is_yes(n, "xref") { PortShape::Bidirectional } else { PortShape::Passive },
    }
}

fn default_field(name: &str, value: String) -> Field {
    Field {
        name: name.to_string(),
        value,
        offset: Point::default(),
        rotation: 0.0,
        height: mm(1.778),
        h_align: HAlign::Left,
        v_align: VAlign::Bottom,
        visible: false,
    }
}

/// Fields of a placed instance: smashed attributes keep their own
/// position; otherwise the gate's placeholder texts are transformed by the
/// instance placement.
fn part_fields(inst: Node<'_, '_>, pos: Point, rot: Rot, gate: &GateInfo, value_for: &dyn Fn(&str) -> String) -> Vec<Field> {
    let mut fields: Vec<Field> = Vec::new();
    let smashed = is_yes(inst, "smashed");
    for a in children(inst, "attribute") {
        let name = field_name(&attr_str(a, "name").to_ascii_uppercase());
        if name.is_empty() || fields.iter().any(|f| f.name == name) {
            continue;
        }
        let (Some(_), Some(_)) = (attr(a, "x"), attr(a, "y")) else { continue };
        let p = point(a, "x", "y");
        let (h, v) = parse_align(attr(a, "align"));
        let (rotation, h_align, v_align) = resolve_text(Rot::of(a), h, v, true);
        let height = coord(a, "size");
        fields.push(Field {
            value: value_for(&name),
            name,
            offset: Point::new(p.x - pos.x, p.y - pos.y),
            rotation,
            height: if height > 0 { height } else { mm(1.778) },
            h_align,
            v_align,
            visible: attr(a, "display") != Some("off"),
        });
    }
    let placement = rot.placement(pos);
    for t in &gate.fields {
        if fields.iter().any(|f| f.name == t.name) {
            continue;
        }
        let world = placement.apply(t.pos);
        let (rotation, h_align, v_align) = resolve_text(t.rot.within(rot), t.h, t.v, true);
        fields.push(Field {
            name: t.name.clone(),
            value: value_for(&t.name),
            offset: Point::new(world.x - pos.x, world.y - pos.y),
            rotation,
            height: t.height,
            h_align,
            v_align,
            visible: !smashed,
        });
    }
    for name in [FIELD_DESIGNATOR, FIELD_VALUE] {
        if !fields.iter().any(|f| f.name == name) {
            fields.push(default_field(name, value_for(name)));
        }
    }
    fields
}

pub(crate) fn import_schematic_text(text: &str, stem: &str) -> Result<SchematicImport, ImportError> {
    let doc = parse_xml(text)?;
    let root = doc.root_element();
    if root.tag_name().name() != "eagle" {
        return Err(ImportError::Invalid("not a supported XML design file".into()));
    }
    let sch = child(root, "drawing")
        .and_then(|d| child(d, "schematic"))
        .ok_or_else(|| ImportError::Invalid("not an XML schematic (no <schematic> element)".into()))?;
    let mut report = ImportReport::default();
    let ctx = format!("{stem}.sch");
    let mut stats = Stats::default();

    // Embedded libraries.
    let mut libs: Vec<ConvertedLib> = Vec::new();
    let mut lib_keys: HashMap<(String, String), usize> = HashMap::new();
    let mut lib_names = HashSet::new();
    for lib in grandchildren(sch, "libraries", "library") {
        let ename = attr_str(lib, "name");
        let urn = attr_str(lib, "urn");
        let name = unique_name(if ename.is_empty() { "library".into() } else { ename.clone() }, &mut lib_names);
        lib_keys.entry((ename, urn)).or_insert(libs.len());
        libs.push(convert_library(lib, &name, &mut report));
    }
    let find_lib = |name: &str, urn: &str| -> Option<usize> {
        lib_keys.get(&(name.to_string(), urn.to_string())).copied().or_else(|| {
            let mut hits: Vec<usize> = lib_keys.iter().filter(|((n, _), _)| n == name).map(|(_, i)| *i).collect();
            hits.sort_unstable();
            hits.first().copied()
        })
    };

    // Parts.
    let mut parts: HashMap<String, PartInfo> = HashMap::new();
    for p in grandchildren(sch, "parts", "part") {
        let name = attr_str(p, "name");
        let lib = find_lib(attr(p, "library").unwrap_or(""), attr(p, "library_urn").unwrap_or(""));
        let deviceset = attr_str(p, "deviceset");
        let component = lib.and_then(|l| {
            libs[l].component_for(&deviceset, attr(p, "device").unwrap_or(""), attr(p, "technology").unwrap_or("")).map(str::to_string)
        });
        let attrs = children(p, "attribute").map(|a| (attr_str(a, "name"), attr_str(a, "value"))).collect();
        parts.insert(name, PartInfo { lib, deviceset, component, value: attr(p, "value").map(str::to_string), attrs });
    }

    for (tag, what) in [("modules", "module"), ("variantdefs", "variantdef")] {
        let n = grandchildren(sch, tag, what).count();
        if n > 0 {
            report.warn(&ctx, format!("{n} {what}(s) not supported; ignored"));
        }
    }
    let global_attrs = grandchildren(sch, "attributes", "attribute").count();
    if global_attrs > 0 {
        report.info(&ctx, format!("{global_attrs} global schematic attribute(s) ignored"));
    }

    let sheets: Vec<Node<'_, '_>> = grandchildren(sch, "sheets", "sheet").collect();
    if sheets.is_empty() {
        return Err(ImportError::Invalid("schematic has no sheets".into()));
    }

    // Net of every pin reference (for supply symbols).
    let mut net_of_pin: HashMap<PinKey, String> = HashMap::new();
    for sheet in &sheets {
        for net in grandchildren(*sheet, "nets", "net") {
            for seg in children(net, "segment") {
                for pr in children(seg, "pinref") {
                    net_of_pin.entry(pin_key(pr)).or_insert_with(|| attr_str(net, "name"));
                }
            }
        }
    }

    let mut skips = LayerSkips::default();
    let mut sheet_items: Vec<Vec<Item>> = Vec::new();
    let mut sheet_cache: Vec<BTreeMap<String, CachedPart>> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut power_parts: HashSet<String> = HashSet::new();

    for (si, sheet) in sheets.iter().enumerate() {
        let mut items: Vec<Item> = Vec::new();
        let mut cache: BTreeMap<String, CachedPart> = BTreeMap::new();
        let mut pin_pos: HashMap<PinKey, Point> = HashMap::new();

        // Plain graphics.
        if let Some(plain) = child(*sheet, "plain") {
            for e in elements(plain) {
                let tag = e.tag_name().name();
                if tag == "dimension" {
                    stats.dimensions += 1;
                    continue;
                }
                if !matches!(tag, "text" | "wire" | "circle" | "rectangle" | "polygon" | "frame") {
                    continue;
                }
                let ln = layer_num(e);
                let Some(layer) = ln.and_then(schematic_layer) else {
                    skips.add(ln);
                    continue;
                };
                if tag == "text" {
                    items.extend(text_item(e, e.text().unwrap_or(""), layer, true));
                } else {
                    if tag == "frame" {
                        stats.frames += 1;
                    }
                    items.extend(graphic(e, layer).unwrap_or_default());
                }
            }
        }
        stats.module_instances += grandchildren(*sheet, "moduleinsts", "moduleinst").count();

        // Instances.
        for inst in grandchildren(*sheet, "instances", "instance") {
            let pname = attr_str(inst, "part");
            let gname = attr_str(inst, "gate");
            let Some(info) = parts.get(&pname) else {
                report.warn(&ctx, format!("instance of unknown part {pname} skipped"));
                stats.unresolved += 1;
                continue;
            };
            let Some(lib) = info.lib.map(|l| &libs[l]) else {
                report.warn(&ctx, format!("part {pname}: library not found; skipped"));
                stats.unresolved += 1;
                continue;
            };
            let Some(ds) = lib.devicesets.get(&info.deviceset) else {
                report.warn(&ctx, format!("part {pname}: deviceset {} not found; skipped", info.deviceset));
                stats.unresolved += 1;
                continue;
            };
            let Some(gi) = ds.gate_index(&gname) else {
                report.warn(&ctx, format!("part {pname}: gate {gname} not found; skipped"));
                stats.unresolved += 1;
                continue;
            };
            let gate = &ds.gates[gi];
            let Some(symbol) = lib.library.symbol(&ds.symbol) else { continue };
            let pos = point(inst, "x", "y");
            let rot = Rot::of(inst);
            let placement: Placement = rot.placement(pos);
            let unit = gi as u32 + 1;

            if ds.is_power {
                let Some(pin) = symbol.unit_pins(unit).next() else { continue };
                let xml_pin = gate.pin_numbers.keys().next().cloned().unwrap_or_default();
                let key = (pname.clone(), gname.clone(), xml_pin.clone());
                let net = net_of_pin.get(&key).cloned().unwrap_or_else(|| strip_at(&xml_pin).to_string());
                let world = placement.apply(pin.pos);
                pin_pos.insert(key, world);
                power_parts.insert(pname.clone());
                items.push(Item::PowerPort(PowerPort {
                    pos: world,
                    net: if net.is_empty() { pin.name.clone() } else { net },
                    rotation: placement.apply_angle(pin.rotation),
                    style: ds.power_style,
                }));
                stats.power_ports += 1;
                continue;
            }

            let Some(comp_name) = info.component.as_deref() else {
                report.warn(&ctx, format!("part {pname}: no matching device in {}; skipped", info.deviceset));
                stats.unresolved += 1;
                continue;
            };
            let Some(cached) = lib.library.cached_part(comp_name) else { continue };
            for (xml_pin, number) in &gate.pin_numbers {
                if let Some(pin) = symbol.unit_pins(unit).find(|p| &p.number == number) {
                    pin_pos.insert((pname.clone(), gname.clone(), xml_pin.clone()), placement.apply(pin.pos));
                }
            }
            let component = &cached.component;
            let value_for = |name: &str| -> String {
                match name {
                    FIELD_DESIGNATOR | "PART" => pname.clone(),
                    FIELD_VALUE => info.value.clone().unwrap_or_else(|| component.parameter(FIELD_VALUE).unwrap_or("").to_string()),
                    "GATE" => gname.clone(),
                    other => info
                        .attrs
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(other))
                        .map(|(_, v)| v.clone())
                        .or_else(|| component.parameter(other).map(str::to_string))
                        .unwrap_or_default(),
                }
            };
            let mut fields = part_fields(inst, pos, rot, gate, &value_for);
            for (k, v) in &info.attrs {
                if !k.is_empty() && !fields.iter().any(|f| f.name.eq_ignore_ascii_case(k)) {
                    fields.push(default_field(k, v.clone()));
                }
            }
            let key = cached.key();
            cache.entry(key.clone()).or_insert(cached);
            items.push(Item::Part(Part { pos, rotation: placement.rotation, mirror: placement.mirror, lib_ref: key, unit, fields }));
            stats.parts += 1;
        }

        // Busses.
        for bus in grandchildren(*sheet, "busses", "bus") {
            let name = attr_str(bus, "name");
            for seg in children(bus, "segment") {
                let wires: Vec<(Point, Point)> =
                    children(seg, "wire").map(|w| (point(w, "x1", "y1"), point(w, "x2", "y2"))).filter(|(a, b)| a != b).collect();
                items.extend(wires.iter().map(|&(a, b)| Item::Bus(pcbplannercad_core::item::Bus { a, b })));
                if !name.is_empty() {
                    for l in children(seg, "label") {
                        items.push(Item::NetLabel(label_item(l, &name, &wires, &mut Vec::new(), &mut stats)));
                    }
                }
            }
        }

        // Nets.
        for net in grandchildren(*sheet, "nets", "net") {
            let name = attr_str(net, "name");
            for seg in children(net, "segment") {
                let mut s = Segment {
                    sheet: si,
                    net: name.clone(),
                    wires: Vec::new(),
                    has_label: false,
                    has_port: false,
                    pins: Vec::new(),
                    junctions: Vec::new(),
                };
                for w in children(seg, "wire") {
                    if attr(w, "curve").and_then(|c| c.parse::<f64>().ok()).is_some_and(|c| c != 0.0) {
                        stats.curved_wires += 1;
                    }
                    let (a, b) = (point(w, "x1", "y1"), point(w, "x2", "y2"));
                    if a != b {
                        s.wires.push((a, b));
                        items.push(Item::Wire(Wire { a, b }));
                        stats.wires += 1;
                    }
                }
                for j in children(seg, "junction") {
                    let p = point(j, "x", "y");
                    s.junctions.push(p);
                    items.push(Item::Junction(Junction { pos: p }));
                }
                for pr in children(seg, "pinref") {
                    let key = pin_key(pr);
                    if power_parts.contains(&key.0) {
                        s.has_port = true;
                    }
                    if let Some(p) = pin_pos.get(&key) {
                        s.pins.push(*p);
                    }
                }
                if !name.is_empty() {
                    let mut dangling = dangling_ends(&s.wires, &s.pins, &s.junctions);
                    for l in children(seg, "label") {
                        items.push(Item::NetLabel(label_item(l, &name, &s.wires, &mut dangling, &mut stats)));
                        s.has_label = true;
                    }
                }
                segments.push(s);
            }
        }
        sheet_items.push(items);
        sheet_cache.push(cache);
    }

    // Preserve source names even for a single unlabelled segment: the board
    // uses those identities too, so generated compiler names would break parity.
    let mut synthesized = 0usize;
    let mut unlabeled = 0usize;
    for s in &segments {
        if s.net.is_empty() || s.has_label || s.has_port {
            continue;
        }
        match synth_label_pos(s) {
            Some((pos, rotation)) => {
                sheet_items[s.sheet].push(Item::NetLabel(NetLabel {
                    pos,
                    text: s.net.clone(),
                    rotation,
                    scope: LabelScope::Global,
                    shape: PortShape::Passive,
                }));
                synthesized += 1;
            }
            None => unlabeled += 1,
        }
    }

    // Reports.
    if synthesized > 0 {
        report.info(&ctx, format!("{synthesized} global net label(s) synthesized to keep XML name-based connectivity between net segments"));
    }
    if unlabeled > 0 {
        report.warn(&ctx, format!("{unlabeled} net segment(s) without wires or pins could not be labeled; connectivity may be lost"));
    }
    if stats.snapped_labels > 0 {
        report.info(&ctx, format!("{} label(s) moved onto their net segment (XML labels may sit beside the wire)", stats.snapped_labels));
    }
    if stats.moved_labels > 0 {
        report.info(&ctx, format!("{} label(s) moved along their wire to its free end so the stub is terminated", stats.moved_labels));
    }
    if stats.curved_wires > 0 {
        report.warn(&ctx, format!("{} curved net wire(s) imported as straight wires", stats.curved_wires));
    }
    if stats.frames > 0 {
        report.info(&ctx, format!("{} drawing frame(s) imported as rectangles", stats.frames));
    }
    if stats.dimensions > 0 {
        report.info(&ctx, format!("{} dimension(s) not supported; skipped", stats.dimensions));
    }
    if stats.module_instances > 0 {
        report.warn(&ctx, format!("{} module instance(s) (design blocks) not supported; skipped", stats.module_instances));
    }
    if let Some(s) = skips.summary() {
        report.info(&ctx, s);
    }
    report.info(
        &ctx,
        format!(
            "imported {} sheet(s): {} part unit(s), {} power port(s), {} wire(s), {} label(s), {} librar{}",
            sheets.len(),
            stats.parts,
            stats.power_ports,
            stats.wires,
            stats.labels + synthesized,
            libs.len(),
            if libs.len() == 1 { "y" } else { "ies" }
        ),
    );

    let single = sheets.len() == 1;
    let out_sheets = sheet_items
        .into_iter()
        .zip(sheet_cache)
        .enumerate()
        .map(|(i, (items, cache))| {
            let mut document = Document::from_item_list(DocKind::Schematic, items);
            for (_, part) in cache {
                document.put_cached_part(part);
            }
            let file_name = if single { format!("{stem}.arsch") } else { format!("{stem}_sheet{}.arsch", i + 1) };
            ImportedSheet { file_name, document }
        })
        .collect();

    Ok(SchematicImport { sheets: out_sheets, libraries: libs.into_iter().map(|l| l.library).collect(), report })
}

/// Where to put a synthesized label: a dangling wire end if any, else any
/// wire end, else a pin. Returns position and text direction.
fn synth_label_pos(s: &Segment) -> Option<(Point, f64)> {
    let dir = |from: Point, to: Point| -> f64 {
        pcbplannercad_core::geom::normalize_deg(((to.y - from.y) as f64).atan2((to.x - from.x) as f64).to_degrees())
    };
    let mut first = None;
    for (i, &(a, b)) in s.wires.iter().enumerate() {
        for (end, other) in [(a, b), (b, a)] {
            let candidate = (end, dir(other, end));
            first.get_or_insert(candidate);
            let touches = s.wires.iter().enumerate().any(|(j, &(c, d))| j != i && on_segment(end, c, d))
                || s.pins.contains(&end)
                || s.junctions.contains(&end);
            if !touches {
                return Some(candidate);
            }
        }
    }
    first.or_else(|| s.pins.first().map(|p| (*p, 0.0)))
}
