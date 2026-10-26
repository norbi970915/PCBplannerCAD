//! `.kicad_sym` symbol libraries (also used for a schematic's embedded `lib_symbols`).

use std::collections::{BTreeMap, HashMap};

use pcbplannercad_core::item::{
    Circle, FIELD_DESIGNATOR, FIELD_VALUE, Field, FillStyle, HAlign, Item, Pin, PinShape, PinType, Rect, Text, VAlign,
};
use pcbplannercad_core::layer::SCHEMATIC;
use pcbplannercad_core::library::{Component, Parameter, SymbolUnit};
use pcbplannercad_core::units::{Point, mm};
use pcbplannercad_core::{Library, Symbol};

use super::common::{Notes, YAxis, angle, arc_from_3pts, arc_from_center, bezier_points, effects, fill, poly_item, stroke_width};
use super::sexpr::{List, parse};
use crate::report::{ImportError, ImportReport};

const Y: YAxis = YAxis::Up;

pub fn parse_symbol_library(text: &str, lib_name: &str) -> Result<(Library, ImportReport), ImportError> {
    let root = parse(text)?;
    if root.head() != "kicad_symbol_lib" {
        return Err(ImportError::Invalid(format!("expected a kicad_symbol_lib, found '{}'", root.head())));
    }
    let mut report = ImportReport::default();
    let mut notes = Notes::default();
    let mut lib = Library { name: lib_name.to_string(), ..Default::default() };
    for (symbol, component) in build_all(&root, &mut notes) {
        lib.symbols.push(symbol);
        lib.components.push(component);
    }
    notes.flush(&mut report, lib_name);
    Ok((lib, report))
}

/// Builds every top-level `symbol` of `container` (a `kicad_symbol_lib` or a
/// schematic's `lib_symbols`). Returned symbols keep the full original name
/// (`Lib:Name` inside schematics).
pub fn build_all(container: &List, notes: &mut Notes) -> Vec<(Symbol, Component)> {
    let nodes: Vec<(&str, &List)> = container.children("symbol").filter_map(|s| Some((s.atom(1)?, s))).collect();
    let by_name: HashMap<&str, &List> = nodes.iter().copied().collect();
    nodes.iter().map(|(name, node)| build_symbol(name, node, &by_name, notes)).collect()
}

/// A property read from a symbol definition.
#[derive(Clone, Debug)]
struct Prop {
    name: String,
    value: String,
    field: Field,
}

fn read_prop(p: &List) -> Option<Prop> {
    let name = p.atom(1)?.to_string();
    let value = p.atom(2).unwrap_or("").to_string();
    let at = p.child("at");
    let pos = at.and_then(|a| Y.pt_of(a)).unwrap_or_default();
    let rot = at.and_then(|a| a.num(3)).map(angle).unwrap_or(0.0);
    let e = effects(p);
    let field = Field {
        name: field_name(&name).to_string(),
        value: value.clone(),
        offset: pos,
        rotation: rot,
        height: e.height,
        h_align: e.h_align,
        v_align: e.v_align,
        visible: !e.hidden,
    };
    Some(Prop { name, value, field })
}

/// structured format property name → PCBPlannerCAD field name.
pub fn field_name(structured: &str) -> &str {
    match structured {
        "Reference" => FIELD_DESIGNATOR,
        "Value" => FIELD_VALUE,
        other => other,
    }
}

/// The extends chain from the root ancestor down to `node`.
fn chain<'n, 'a>(node: &'n List<'a>, by_name: &HashMap<&str, &'n List<'a>>, name: &str, notes: &mut Notes) -> Vec<&'n List<'a>> {
    let mut out = vec![node];
    let mut cur = node;
    while let Some(parent) = cur.child_str("extends") {
        // `extends` names the parent without the library prefix inside schematics.
        let found = by_name.get(parent).copied().or_else(|| {
            let prefix = name.split_once(':').map(|(l, _)| l);
            prefix.and_then(|l| by_name.get(format!("{l}:{parent}").as_str()).copied())
        });
        match found {
            Some(p) if out.len() < 16 && !out.iter().any(|n| std::ptr::eq(*n, p)) => {
                out.push(p);
                cur = p;
            }
            Some(_) => {
                notes.warn("symbol inheritance loop or too deep; stopped resolving 'extends'", name);
                break;
            }
            None => {
                notes.warn("derived symbol's parent not found; imported without parent graphics", name);
                break;
            }
        }
    }
    out.reverse();
    out
}

fn build_symbol(full_name: &str, node: &List, by_name: &HashMap<&str, &List>, notes: &mut Notes) -> (Symbol, Component) {
    let chain = chain(node, by_name, full_name, notes);
    let short = full_name.split_once(':').map_or(full_name, |(_, n)| n);

    // Properties: ancestors first, overridden by descendants.
    let mut props: Vec<Prop> = Vec::new();
    for n in &chain {
        for p in n.children("property").filter_map(read_prop) {
            match props.iter_mut().find(|q| q.name == p.name) {
                Some(q) => *q = p,
                None => props.push(p),
            }
        }
    }
    let prop = |name: &str| props.iter().find(|p| p.name == name).map(|p| p.value.as_str());

    let mut sym = Symbol::new(full_name, "U");
    let mut is_power = false;
    for n in &chain {
        if let Some(pn) = n.child("pin_names") {
            if let Some(off) = pn.child_num("offset") {
                sym.pin_name_offset = mm(off);
            }
            sym.show_pin_names = !pn.flag("hide");
        }
        if let Some(pn) = n.child("pin_numbers") {
            sym.show_pin_numbers = !pn.flag("hide");
        }
        if n.child("power").is_some() || n.has_atom("power") {
            is_power = true;
        }
    }
    sym.is_power = is_power;
    if chain.iter().any(|n| n.child("power").and_then(|p| p.atom(1)) == Some("local")) {
        notes.info("local power symbols are imported as global power symbols", short);
    }

    let reference = prop("Reference").unwrap_or("U");
    sym.prefix = designator_prefix(reference);
    sym.description = prop("Description").or_else(|| prop("ki_description")).unwrap_or("").to_string();
    sym.fields = props.iter().filter(|p| p.name == "Reference" || p.name == "Value").map(|p| p.field.clone()).collect();

    // Graphics come from the nearest ancestor (or self) that defines sub-symbols.
    let gfx_node = chain.iter().rev().find(|n| n.child("symbol").is_some()).copied().unwrap_or(node);
    sym.units = build_units(gfx_node, short, notes);

    let footprint = prop("Footprint").unwrap_or("").trim();
    let mut parameters = Vec::new();
    for p in &props {
        let keep = match p.name.as_str() {
            "Reference" | "Footprint" | "ki_fp_filters" | "ki_locked" => false,
            "Value" => true,
            _ => !p.value.is_empty(),
        };
        if keep {
            parameters.push(Parameter { name: p.name.clone(), value: p.value.clone() });
        }
    }
    let component = Component {
        name: full_name.to_string(),
        description: sym.description.clone(),
        symbol: full_name.to_string(),
        footprints: if footprint.is_empty() { Vec::new() } else { vec![footprint.to_string()] },
        pin_map: Vec::new(),
        parameters,
    };
    (sym, component)
}

/// "R?" → "R", "U12" → "U", "#PWR" → "#PWR".
pub fn designator_prefix(reference: &str) -> String {
    let p = reference.trim().trim_end_matches(|c: char| c == '?' || c.is_ascii_digit());
    if p.is_empty() { "U".to_string() } else { p.to_string() }
}

/// Splits a sub-symbol name `NAME_U_S` into (unit, style).
fn unit_style(name: &str) -> Option<(u32, u32)> {
    let mut it = name.rsplitn(3, '_');
    let style = it.next()?.parse().ok()?;
    let unit = it.next()?.parse().ok()?;
    it.next()?;
    Some((unit, style))
}

fn build_units(node: &List, name: &str, notes: &mut Notes) -> Vec<SymbolUnit> {
    let mut common: Vec<Item> = Vec::new();
    let mut units: BTreeMap<u32, SymbolUnit> = BTreeMap::new();
    let mut demorgan = false;
    for sub in node.children("symbol") {
        let (unit, style) = match sub.atom(1).and_then(unit_style) {
            Some(us) => us,
            None => {
                notes.warn("sub-symbol name not in NAME_UNIT_STYLE form; treated as common to all units", name);
                (0, 1)
            }
        };
        if style > 1 {
            demorgan = true;
            continue;
        }
        let items = sub_items(sub, name, notes);
        if unit == 0 {
            common.extend(items);
        } else {
            let u = units.entry(unit).or_default();
            if let Some(n) = sub.child_str("unit_name") {
                u.name = n.to_string();
            }
            u.items.extend(items);
        }
    }
    if demorgan {
        notes.info("De Morgan alternate body style skipped (style 1 kept)", name);
    }
    // Graphics can also sit directly in the symbol (rare, old files).
    common.extend(sub_items(node, name, notes).into_iter().filter(|i| !matches!(i, Item::Pin(_))));

    let max = units.keys().next_back().copied().unwrap_or(1).clamp(1, 1024);
    if units.len() as u32 != max && !units.is_empty() {
        notes.warn("symbol units are not numbered contiguously; missing units left empty", name);
    }
    (1..=max)
        .map(|u| {
            let own = units.remove(&u).unwrap_or_default();
            let mut items = common.clone();
            items.extend(own.items);
            SymbolUnit { name: own.name, items }
        })
        .collect()
}

/// Graphics and pins of one sub-symbol.
fn sub_items(sub: &List, name: &str, notes: &mut Notes) -> Vec<Item> {
    let mut out = Vec::new();
    for l in sub.lists() {
        match l.head() {
            "pin" => out.extend(read_pin(l, name, notes)),
            "text" => out.extend(read_text(l, 10.0, Y)),
            "text_box" => {
                notes.info("text boxes imported as plain text", name);
                out.extend(read_text_box(l, Y));
            }
            "symbol"
            | "property"
            | "pin_names"
            | "pin_numbers"
            | "power"
            | "extends"
            | "unit_name"
            | "in_bom"
            | "on_board"
            | "exclude_from_sim"
            | "in_pos_files"
            | "duplicate_pin_numbers_are_jumpers"
            | "embedded_fonts"
            | "body_styles"
            | "jumper_pin_groups"
            | "embedded_files" => {}
            _ => match graphic(l, Y, notes, name) {
                Some(items) => out.extend(items),
                None => notes.info(&format!("unsupported symbol item '{}' skipped", l.head()), name),
            },
        }
    }
    out
}

/// Schematic-style graphics shared by symbols and sheets: polyline,
/// rectangle, circle, arc, bezier. `None` for anything else.
pub fn graphic(l: &List, y: YAxis, notes: &mut Notes, ctx: &str) -> Option<Vec<Item>> {
    let width = stroke_width(l);
    let fill = fill(l).unwrap_or(FillStyle::None);
    let layer = SCHEMATIC;
    let item = match l.head() {
        "polyline" => poly_item(y.pts(l), width, layer, fill),
        "bezier" => poly_item(bezier_points(&y.pts(l)), width, layer, fill),
        "rectangle" => {
            let (a, b) = (y.child_pt(l, "start")?, y.child_pt(l, "end")?);
            (a.x != b.x && a.y != b.y).then_some(Item::Rect(Rect { a, b, width, layer, fill }))
        }
        "circle" => {
            let center = y.child_pt(l, "center")?;
            let radius = mm(l.child_num("radius")?);
            (radius > 0).then_some(Item::Circle(Circle { center, radius, width, layer, fill }))
        }
        "arc" => {
            let item = match (y.child_pt(l, "start"), y.child_pt(l, "mid"), y.child_pt(l, "end")) {
                (Some(s), Some(m), Some(e)) => arc_from_3pts(s, m, e).map(|a| a.item(width, layer)),
                (Some(s), None, Some(e)) => legacy_arc(l, s, e, y).map(|a| a.item(width, layer)),
                _ => None,
            };
            if item.is_none() {
                notes.info("degenerate arc skipped", ctx);
            }
            item
        }
        _ => return None,
    };
    Some(item.into_iter().collect())
}

/// structured format 6 pre-release arcs: `(arc (start) (end) (radius (at cx cy) (length r) (angles a1 a2)))`.
fn legacy_arc(l: &List, s: Point, e: Point, y: YAxis) -> Option<super::common::ArcGeom> {
    let r = l.child("radius")?;
    let center = y.child_pt(r, "at")?;
    let radius = mm(r.child_num("length")?);
    let ang = |p: Point| ((p.y - center.y) as f64).atan2((p.x - center.x) as f64).to_degrees();
    let (a_s, a_e) = (ang(s), ang(e));
    // These arcs were always the minor arc.
    let mut sweep = (a_e - a_s).rem_euclid(360.0);
    if sweep > 180.0 {
        sweep -= 360.0;
    }
    arc_from_center(center, radius, a_s, sweep)
}

fn read_pin(l: &List, name: &str, notes: &mut Notes) -> Option<Item> {
    let at = l.child("at")?;
    let pos = Y.pt_of(at)?;
    let electrical = match l.atom(1).unwrap_or("") {
        "input" => PinType::Input,
        "output" => PinType::Output,
        "bidirectional" => PinType::Bidirectional,
        "tri_state" => PinType::TriState,
        "passive" => PinType::Passive,
        "power_in" => PinType::PowerIn,
        "power_out" => PinType::PowerOut,
        "open_collector" => PinType::OpenCollector,
        "open_emitter" => PinType::OpenEmitter,
        "no_connect" | "unconnected" => PinType::NoConnect,
        "unspecified" | "free" => PinType::Unspecified,
        other => {
            notes.warn(&format!("unknown pin type '{other}' imported as unspecified"), name);
            PinType::Unspecified
        }
    };
    let shape = match l.atom(2).unwrap_or("line") {
        "line" => PinShape::Line,
        "inverted" => PinShape::Inverted,
        "clock" => PinShape::Clock,
        "inverted_clock" => PinShape::InvertedClock,
        other => {
            notes.info(&format!("pin shape '{other}' imported as a plain line"), name);
            PinShape::Line
        }
    };
    let text = |k: &str| {
        let s = l.child_str(k).unwrap_or("");
        if s == "~" { String::new() } else { s.to_string() }
    };
    Some(Item::Pin(Pin {
        pos,
        length: mm(l.child_num("length").unwrap_or(2.54).max(0.0)),
        rotation: angle(at.num(3).unwrap_or(0.0)),
        name: text("name"),
        number: text("number"),
        electrical,
        shape,
        hidden: l.flag("hide"),
    }))
}

/// A `(text "s" (at x y angle) (effects ...))`; `angle_div` is 10 for
/// symbol libraries (tenths of a degree) and 1 for schematics.
pub fn read_text(l: &List, angle_div: f64, y: YAxis) -> Option<Item> {
    let s = l.atom(1)?;
    let e = effects(l);
    if s.is_empty() || e.hidden {
        return None;
    }
    let at = l.child("at")?;
    Some(Item::Text(Text {
        pos: y.pt_of(at)?,
        text: s.to_string(),
        height: e.height,
        rotation: angle(at.num(3).unwrap_or(0.0) / angle_div),
        h_align: e.h_align,
        v_align: e.v_align,
        layer: SCHEMATIC,
        mirrored: false,
    }))
}

/// A text box as top-left aligned text (the box itself is not kept).
pub fn read_text_box(l: &List, y: YAxis) -> Option<Item> {
    let s = l.atom(1)?;
    let e = effects(l);
    if s.is_empty() || e.hidden {
        return None;
    }
    let (pos, rotation) = match (l.child("at"), l.child("start")) {
        (Some(at), _) => (y.pt_of(at)?, at.num(3).unwrap_or(0.0)),
        (None, Some(st)) => (y.pt_of(st)?, 0.0),
        _ => return None,
    };
    Some(Item::Text(Text {
        pos,
        text: s.to_string(),
        height: e.height,
        rotation: angle(rotation),
        h_align: HAlign::Left,
        v_align: VAlign::Top,
        layer: SCHEMATIC,
        mirrored: false,
    }))
}
