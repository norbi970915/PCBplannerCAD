//! Symbol wizard (rectangular IC bodies) and the hand-drawn schematic
//! symbols of the starter library.
//!
//! Grid rules: every pin connection point lies on the 1.27 mm (50 mil)
//! grid; IC pins are 2.54 mm long on a 2.54 mm pitch, two-terminal parts
//! span 7.62 mm or 10.16 mm between connection points.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::item::{
    Arc, Circle, FIELD_DESIGNATOR, FIELD_VALUE, Field, FillStyle, HAlign, Item, Line, Pin, PinShape, PinType, Polyline, Rect, SheetSide, Text, VAlign,
};
use crate::layer::SCHEMATIC;
use crate::library::{Symbol, SymbolUnit};
use crate::units::{Point, mm};

pub(crate) const GRID: f64 = 1.27;
pub(crate) const PIN_LEN: f64 = 2.54;
const PITCH: f64 = 2.54;
pub(crate) const BODY_WIDTH: f64 = 0.254;
pub(crate) const FIELD_HEIGHT: f64 = 1.27;
const PIN_NAME_OFFSET: f64 = 0.508;
/// Estimated advance of one character at 1.27 mm text height.
const CHAR_WIDTH: f64 = 1.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinSpec {
    pub number: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub electrical: PinType,
    #[serde(default)]
    pub side: SheetSide,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSpec {
    pub name: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub description: String,
    pub pins: Vec<PinSpec>,
    /// Body width override (mm); rounded up to a 2.54 mm multiple and never
    /// narrower than the top/bottom pins need.
    #[serde(default)]
    pub body_width: Option<f64>,
}

fn default_prefix() -> String {
    "U".into()
}

// ---------- Item helpers (mm in, schematic layer) ----------

pub(crate) fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

pub(crate) fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Item {
    Item::Line(Line { a: p(x1, y1), b: p(x2, y2), width: mm(BODY_WIDTH), layer: SCHEMATIC })
}

pub(crate) fn thick_line(x1: f64, y1: f64, x2: f64, y2: f64, w: f64) -> Item {
    Item::Line(Line { a: p(x1, y1), b: p(x2, y2), width: mm(w), layer: SCHEMATIC })
}

pub(crate) fn rect(x1: f64, y1: f64, x2: f64, y2: f64, fill: FillStyle) -> Item {
    Item::Rect(Rect { a: p(x1, y1), b: p(x2, y2), width: mm(BODY_WIDTH), layer: SCHEMATIC, fill })
}

pub(crate) fn poly(points: &[(f64, f64)], closed: bool, fill: FillStyle) -> Item {
    Item::Polyline(Polyline { points: points.iter().map(|&(x, y)| p(x, y)).collect(), closed, width: mm(BODY_WIDTH), layer: SCHEMATIC, fill })
}

pub(crate) fn circle(x: f64, y: f64, r: f64, fill: FillStyle) -> Item {
    Item::Circle(Circle { center: p(x, y), radius: mm(r), width: mm(BODY_WIDTH), layer: SCHEMATIC, fill })
}

pub(crate) fn arc(x: f64, y: f64, r: f64, start: f64, sweep: f64) -> Item {
    Item::Arc(Arc { center: p(x, y), radius: mm(r), start_angle: start, sweep, width: mm(BODY_WIDTH), layer: SCHEMATIC })
}

pub(crate) fn text(x: f64, y: f64, s: &str, h: f64) -> Item {
    Item::Text(Text { pos: p(x, y), text: s.into(), height: mm(h), rotation: 0.0, h_align: HAlign::Center, v_align: VAlign::Middle, layer: SCHEMATIC, mirrored: false })
}

/// Pin with its connection point at (`x`, `y`) pointing `rot` degrees toward the body.
pub(crate) fn pin(x: f64, y: f64, rot: f64, len: f64, number: &str, name: &str, electrical: PinType) -> Item {
    Item::Pin(Pin { pos: p(x, y), length: mm(len), rotation: rot, name: name.into(), number: number.into(), electrical, shape: PinShape::Line, hidden: false })
}

pub(crate) fn field(name: &str, value: &str, x: f64, y: f64, h_align: HAlign, v_align: VAlign) -> Field {
    Field { name: name.into(), value: value.into(), offset: p(x, y), rotation: 0.0, height: mm(FIELD_HEIGHT), h_align, v_align, visible: true }
}

/// Symbol with one unit and Designator/Value templates.
pub(crate) fn symbol(name: &str, prefix: &str, description: &str, items: Vec<Item>, des: (f64, f64, HAlign), val: (f64, f64, HAlign)) -> Symbol {
    let mut s = Symbol::new(name, prefix);
    s.description = description.into();
    s.units = vec![SymbolUnit { name: String::new(), items }];
    s.fields = vec![
        field(FIELD_DESIGNATOR, &format!("{prefix}?"), des.0, des.1, des.2, VAlign::Middle),
        field(FIELD_VALUE, name, val.0, val.1, val.2, VAlign::Middle),
    ];
    s
}

fn ceil_to(v: f64, step: f64) -> f64 {
    ((v / step) - 1e-9).ceil() * step
}

fn text_len(s: &str) -> f64 {
    s.chars().count() as f64 * CHAR_WIDTH
}

// ---------- Box symbol wizard ----------

/// Rectangular IC symbol from a pin list. Pins of each side keep their
/// order (left/right: top → bottom, top/bottom: left → right).
pub fn box_symbol(spec: &SymbolSpec) -> Result<Symbol, CoreError> {
    let bad = CoreError::InvalidGeometry;
    if spec.name.trim().is_empty() {
        return Err(bad("symbol name is empty"));
    }
    if spec.prefix.trim().is_empty() {
        return Err(bad("designator prefix is empty"));
    }
    if spec.pins.is_empty() {
        return Err(bad("symbol needs at least one pin"));
    }
    let mut seen = HashSet::new();
    for pin in &spec.pins {
        if pin.number.trim().is_empty() {
            return Err(bad("pin number is empty"));
        }
        if !seen.insert(pin.number.as_str()) {
            return Err(bad("duplicate pin number"));
        }
    }
    if let Some(w) = spec.body_width
        && !(w.is_finite() && w > 0.0)
    {
        return Err(bad("body width must be positive"));
    }

    let side = |s: SheetSide| spec.pins.iter().filter(move |p| p.side == s).collect::<Vec<_>>();
    let (left, right, top, bottom) = (side(SheetSide::Left), side(SheetSide::Right), side(SheetSide::Top), side(SheetSide::Bottom));
    let names_len = |v: &[&PinSpec]| v.iter().filter(|p| !p.hidden).map(|p| text_len(&p.name)).fold(0.0, f64::max);

    // Half sizes are multiples of 1.27 so connection points land on the grid.
    let rows = left.len().max(right.len()) as f64;
    let cols = top.len().max(bottom.len()) as f64;
    let name_room_v = names_len(&top) + names_len(&bottom) + 2.0 * PIN_NAME_OFFSET + GRID;
    let half_h = ceil_to(((rows - 1.0).max(0.0) / 2.0 * PITCH + PITCH).max(name_room_v / 2.0), GRID);
    let min_half_w = (cols - 1.0).max(0.0) / 2.0 * PITCH + PITCH;
    let half_w = match spec.body_width {
        Some(w) => ceil_to(w / 2.0, GRID).max(ceil_to(min_half_w, GRID)),
        None => {
            let name_room_h = names_len(&left) + names_len(&right) + 2.0 * PIN_NAME_OFFSET + PITCH;
            ceil_to(min_half_w.max(name_room_h / 2.0).max(2.0 * PITCH), GRID)
        }
    };

    let mut items = vec![rect(-half_w, half_h, half_w, -half_h, FillStyle::Background)];
    let positions = |n: usize| (0..n).map(move |i| ((n as f64 - 1.0) / 2.0 - i as f64) * PITCH);
    let mut add = |ps: &[&PinSpec], f: &dyn Fn(f64) -> (f64, f64, f64)| {
        for (pinspec, t) in ps.iter().zip(positions(ps.len())) {
            let (x, y, rot) = f(t);
            let mut it = pin(x, y, rot, PIN_LEN, &pinspec.number, &pinspec.name, pinspec.electrical);
            if let Item::Pin(pp) = &mut it {
                pp.hidden = pinspec.hidden;
            }
            items.push(it);
        }
    };
    add(&left, &|t| (-half_w - PIN_LEN, t, 0.0));
    add(&right, &|t| (half_w + PIN_LEN, t, 180.0));
    add(&top, &|t| (-t, half_h + PIN_LEN, 270.0));
    add(&bottom, &|t| (-t, -half_h - PIN_LEN, 90.0));

    let top_extent = half_h + if top.is_empty() { 0.0 } else { PIN_LEN };
    let bottom_extent = half_h + if bottom.is_empty() { 0.0 } else { PIN_LEN };
    let mut s = Symbol::new(spec.name.trim(), spec.prefix.trim());
    s.description = spec.description.clone();
    s.units = vec![SymbolUnit { name: String::new(), items }];
    s.fields = vec![
        field(FIELD_DESIGNATOR, &format!("{}?", s.prefix), -half_w, top_extent + GRID, HAlign::Left, VAlign::Bottom),
        field(FIELD_VALUE, &s.name.clone(), -half_w, -bottom_extent - GRID, HAlign::Left, VAlign::Top),
    ];
    Ok(s)
}

// ---------- Hand-drawn symbols ----------

use PinType::{Input, Output, Passive, PowerIn};

/// Field placement right of a vertical two-terminal body.
const DES_R: (f64, f64, HAlign) = (2.032, 1.016, HAlign::Left);
const VAL_R: (f64, f64, HAlign) = (2.032, -1.016, HAlign::Left);
/// Field placement above a horizontal two-terminal body.
const DES_T: (f64, f64, HAlign) = (0.0, 3.302, HAlign::Center);
const VAL_T: (f64, f64, HAlign) = (0.0, -2.794, HAlign::Center);

fn two_pin(mut s: Symbol) -> Symbol {
    s.show_pin_names = false;
    s.show_pin_numbers = false;
    s
}

/// Vertical two-terminal pins at y = ±`half` (pin 1 on top).
fn vpins(half: f64) -> [Item; 2] {
    [pin(0.0, half, 270.0, PIN_LEN, "1", "", Passive), pin(0.0, -half, 90.0, PIN_LEN, "2", "", Passive)]
}

pub(crate) fn resistor() -> Symbol {
    let [a, b] = vpins(3.81);
    two_pin(symbol("R", "R", "Resistor", vec![rect(-1.016, 1.27, 1.016, -1.27, FillStyle::None), a, b], DES_R, VAL_R))
}

pub(crate) fn fuse() -> Symbol {
    let [a, b] = vpins(3.81);
    two_pin(symbol("Fuse", "F", "Fuse", vec![rect(-0.762, 1.27, 0.762, -1.27, FillStyle::None), line(0.0, 1.27, 0.0, -1.27), a, b], DES_R, VAL_R))
}

pub(crate) fn capacitor() -> Symbol {
    let [a, b] = vpins(3.81);
    let items = vec![
        thick_line(-1.524, 0.508, 1.524, 0.508, 0.3),
        thick_line(-1.524, -0.508, 1.524, -0.508, 0.3),
        line(0.0, 1.27, 0.0, 0.508),
        line(0.0, -1.27, 0.0, -0.508),
        a,
        b,
    ];
    two_pin(symbol("C", "C", "Unpolarized capacitor", items, DES_R, VAL_R))
}

pub(crate) fn capacitor_polarized() -> Symbol {
    let [a, b] = vpins(3.81);
    let items = vec![
        rect(-1.524, 0.762, 1.524, 0.381, FillStyle::None),
        rect(-1.524, -0.381, 1.524, -0.762, FillStyle::Solid),
        line(0.0, 1.27, 0.0, 0.762),
        line(0.0, -1.27, 0.0, -0.762),
        line(-1.524, 1.778, -0.762, 1.778),
        line(-1.143, 2.159, -1.143, 1.397),
        a,
        b,
    ];
    let mut s = two_pin(symbol("C_Polarized", "C", "Polarized capacitor, pin 1 = +", items, DES_R, VAL_R));
    if let Some(Item::Pin(p)) = s.units[0].items.iter_mut().find(|i| matches!(i, Item::Pin(p) if p.number == "1")) {
        p.name = "+".into();
    }
    if let Some(Item::Pin(p)) = s.units[0].items.iter_mut().find(|i| matches!(i, Item::Pin(p) if p.number == "2")) {
        p.name = "-".into();
    }
    s
}

pub(crate) fn inductor() -> Symbol {
    let [a, b] = vpins(5.08);
    let mut items: Vec<Item> = [1.905, 0.635, -0.635, -1.905].iter().map(|&y| arc(0.0, y, 0.635, -90.0, 180.0)).collect();
    items.extend([a, b]);
    two_pin(symbol("L", "L", "Inductor", items, DES_R, VAL_R))
}

pub(crate) fn ferrite_bead() -> Symbol {
    let [a, b] = vpins(3.81);
    // Rectangle 3.2 × 1.5 mm tilted to 60°, crossing the pin axis.
    let items = vec![poly(&[(0.15, 1.761), (1.45, 1.011), (-0.15, -1.761), (-1.45, -1.011)], true, FillStyle::None), a, b];
    two_pin(symbol("FerriteBead", "FB", "Ferrite bead", items, DES_R, VAL_R))
}

/// Horizontal diode body: anode (pin 2) left, cathode (pin 1) right.
fn diode_base(bar: Item, extra: Vec<Item>) -> Vec<Item> {
    let mut v = vec![
        poly(&[(-1.27, 1.27), (-1.27, -1.27), (1.27, 0.0)], true, FillStyle::None),
        line(-1.27, 0.0, 1.27, 0.0),
        bar,
        pin(3.81, 0.0, 180.0, PIN_LEN, "1", "K", Passive),
        pin(-3.81, 0.0, 0.0, PIN_LEN, "2", "A", Passive),
    ];
    v.extend(extra);
    v
}

pub(crate) fn diode() -> Symbol {
    two_pin(symbol("D", "D", "Diode, pin 1 = cathode", diode_base(line(1.27, 1.27, 1.27, -1.27), vec![]), DES_T, VAL_T))
}

pub(crate) fn zener() -> Symbol {
    let bar = poly(&[(0.762, 1.524), (1.27, 1.27), (1.27, -1.27), (1.778, -1.524)], false, FillStyle::None);
    two_pin(symbol("D_Zener", "D", "Zener diode, pin 1 = cathode", diode_base(bar, vec![]), DES_T, VAL_T))
}

pub(crate) fn schottky() -> Symbol {
    let bar = poly(&[(0.635, 0.762), (0.635, 1.27), (1.27, 1.27), (1.27, -1.27), (1.905, -1.27), (1.905, -0.762)], false, FillStyle::None);
    two_pin(symbol("D_Schottky", "D", "Schottky diode, pin 1 = cathode", diode_base(bar, vec![]), DES_T, VAL_T))
}

pub(crate) fn led() -> Symbol {
    let arrow = |x: f64, y: f64| {
        vec![
            line(x, y, x + 1.016, y + 1.016),
            poly(&[(x + 1.016, y + 1.016), (x + 0.508, y + 0.889), (x + 0.889, y + 0.508)], true, FillStyle::Solid),
        ]
    };
    let mut extra = arrow(-0.254, 1.524);
    extra.extend(arrow(0.762, 1.524));
    let s = symbol("LED", "D", "Light-emitting diode, pin 1 = cathode", diode_base(line(1.27, 1.27, 1.27, -1.27), extra), (0.0, 4.064, HAlign::Center), VAL_T);
    two_pin(s)
}

/// Bipolar transistor, SOT-23 numbering: 1 = B, 2 = E, 3 = C.
fn bjt(pnp: bool) -> Symbol {
    let mut items = vec![
        circle(1.27, 0.0, 2.794, FillStyle::None),
        thick_line(0.635, 1.905, 0.635, -1.905, 0.508),
        line(0.0, 0.0, 0.635, 0.0),
        line(0.635, 0.635, 2.54, 2.54),
        line(0.635, -0.635, 2.54, -2.54),
        pin(-2.54, 0.0, 0.0, PIN_LEN, "1", "B", Input),
    ];
    if pnp {
        // Emitter on top, arrow pointing into the base.
        items.push(poly(&[(1.143, 1.143), (1.951, 1.413), (1.413, 1.951)], true, FillStyle::Solid));
        items.push(pin(2.54, 5.08, 270.0, PIN_LEN, "2", "E", Passive));
        items.push(pin(2.54, -5.08, 90.0, PIN_LEN, "3", "C", Passive));
    } else {
        // Emitter at the bottom, arrow pointing out.
        items.push(poly(&[(2.032, -2.032), (1.762, -1.224), (1.224, -1.762)], true, FillStyle::Solid));
        items.push(pin(2.54, -5.08, 90.0, PIN_LEN, "2", "E", Passive));
        items.push(pin(2.54, 5.08, 270.0, PIN_LEN, "3", "C", Passive));
    }
    let (name, desc) = if pnp { ("Q_PNP", "PNP transistor (1 = B, 2 = E, 3 = C)") } else { ("Q_NPN", "NPN transistor (1 = B, 2 = E, 3 = C)") };
    let mut s = symbol(name, "Q", desc, items, (4.826, 1.016, HAlign::Left), (4.826, -1.016, HAlign::Left));
    s.show_pin_names = false;
    s
}

pub(crate) fn npn() -> Symbol {
    bjt(false)
}

pub(crate) fn pnp() -> Symbol {
    bjt(true)
}

/// Enhancement MOSFET, SOT-23 numbering: 1 = G, 2 = S, 3 = D.
fn mosfet(p_channel: bool) -> Symbol {
    // Drain/source on the channel side at x = 2.54; source tied to the body line.
    let (top, bottom) = if p_channel { (("2", "S"), ("3", "D")) } else { (("3", "D"), ("2", "S")) };
    let src_y = if p_channel { 1.397 } else { -1.397 };
    let mut items = vec![
        circle(1.27, 0.0, 2.794, FillStyle::None),
        line(0.0, 0.0, 0.254, 0.0),
        line(0.254, 1.905, 0.254, -1.905),
        thick_line(0.762, 1.016, 0.762, 1.778, 0.254),
        thick_line(0.762, -0.381, 0.762, 0.381, 0.254),
        thick_line(0.762, -1.778, 0.762, -1.016, 0.254),
        line(0.762, 1.397, 2.54, 1.397),
        line(2.54, 1.397, 2.54, 2.54),
        line(0.762, -1.397, 2.54, -1.397),
        line(2.54, -1.397, 2.54, -2.54),
        line(0.762, 0.0, 2.54, 0.0),
        line(2.54, 0.0, 2.54, src_y),
        pin(-2.54, 0.0, 0.0, PIN_LEN, "1", "G", Input),
        pin(2.54, 5.08, 270.0, PIN_LEN, top.0, top.1, Passive),
        pin(2.54, -5.08, 90.0, PIN_LEN, bottom.0, bottom.1, Passive),
    ];
    if p_channel {
        items.push(poly(&[(2.032, 0.0), (1.27, 0.381), (1.27, -0.381)], true, FillStyle::Solid));
    } else {
        items.push(poly(&[(1.016, 0.0), (1.778, 0.381), (1.778, -0.381)], true, FillStyle::Solid));
    }
    let (name, desc) = if p_channel { ("Q_PMOS", "P-channel MOSFET (1 = G, 2 = S, 3 = D)") } else { ("Q_NMOS", "N-channel MOSFET (1 = G, 2 = S, 3 = D)") };
    let mut s = symbol(name, "Q", desc, items, (4.826, 1.016, HAlign::Left), (4.826, -1.016, HAlign::Left));
    s.show_pin_names = false;
    s
}

pub(crate) fn nmos() -> Symbol {
    mosfet(false)
}

pub(crate) fn pmos() -> Symbol {
    mosfet(true)
}

pub(crate) fn crystal() -> Symbol {
    let items = vec![
        rect(-0.508, 1.524, 0.508, -1.524, FillStyle::None),
        thick_line(-1.016, 1.016, -1.016, -1.016, 0.3),
        thick_line(1.016, 1.016, 1.016, -1.016, 0.3),
        line(-1.27, 0.0, -1.016, 0.0),
        line(1.016, 0.0, 1.27, 0.0),
        pin(-3.81, 0.0, 0.0, PIN_LEN, "1", "1", Passive),
        pin(3.81, 0.0, 180.0, PIN_LEN, "2", "2", Passive),
    ];
    two_pin(symbol("Crystal", "Y", "Two-pin crystal", items, DES_T, VAL_T))
}

pub(crate) fn push_button() -> Symbol {
    let items = vec![
        circle(-2.032, 0.0, 0.508, FillStyle::None),
        circle(2.032, 0.0, 0.508, FillStyle::None),
        line(-2.54, 1.27, 2.54, 1.27),
        line(0.0, 1.27, 0.0, 3.048),
        line(-1.016, 3.048, 1.016, 3.048),
        pin(-5.08, 0.0, 0.0, PIN_LEN, "1", "1", Passive),
        pin(5.08, 0.0, 180.0, PIN_LEN, "2", "2", Passive),
    ];
    two_pin(symbol("SW_Push", "SW", "Push button, normally open", items, (0.0, 4.318, HAlign::Center), (0.0, -1.778, HAlign::Center)))
}

pub(crate) fn test_point() -> Symbol {
    let items = vec![circle(0.0, 3.302, 0.762, FillStyle::None), pin(0.0, 0.0, 90.0, PIN_LEN, "1", "1", Passive)];
    two_pin(symbol("TestPoint", "TP", "Test point", items, (1.524, 3.81, HAlign::Left), (1.524, 2.286, HAlign::Left)))
}

/// Op-amp triangle with + (top) and − inputs, output, and optional power pins.
fn opamp_unit(plus: &str, minus: &str, out: &str, power: Option<(&str, &str)>) -> Vec<Item> {
    let mut v = vec![
        poly(&[(-5.08, 5.08), (5.08, 0.0), (-5.08, -5.08)], true, FillStyle::Background),
        text(-3.81, 2.54, "+", 1.27),
        text(-3.81, -2.54, "-", 1.27),
        pin(-7.62, 2.54, 0.0, PIN_LEN, plus, "+", Input),
        pin(-7.62, -2.54, 0.0, PIN_LEN, minus, "-", Input),
        pin(7.62, 0.0, 180.0, PIN_LEN, out, "OUT", Output),
    ];
    if let Some((vp, vm)) = power {
        v.push(pin(-2.54, 6.35, 270.0, PIN_LEN, vp, "V+", PowerIn));
        v.push(pin(-2.54, -6.35, 90.0, PIN_LEN, vm, "V-", PowerIn));
    }
    v
}

/// Single op-amp, standard 8-pin pinout (2 = −IN, 3 = +IN, 4 = V−, 6 = OUT, 7 = V+).
pub(crate) fn opamp() -> Symbol {
    let mut s = symbol(
        "Opamp",
        "U",
        "Single operational amplifier, 8-pin pinout",
        opamp_unit("3", "2", "6", Some(("7", "4"))),
        (1.27, 6.35, HAlign::Left),
        (1.27, 4.826, HAlign::Left),
    );
    s.show_pin_names = false;
    s
}

/// Dual op-amp (LM358 pinout): units A, B and a power unit.
pub(crate) fn opamp_dual() -> Symbol {
    let mut s = symbol(
        "Opamp_Dual",
        "U",
        "Dual operational amplifier, 8-pin pinout",
        opamp_unit("3", "2", "1", None),
        (1.27, 6.35, HAlign::Left),
        (1.27, 4.826, HAlign::Left),
    );
    s.units[0].name = "A".into();
    s.units.push(SymbolUnit { name: "B".into(), items: opamp_unit("5", "6", "7", None) });
    s.units.push(SymbolUnit {
        name: "Power".into(),
        items: vec![
            rect(-2.54, 2.54, 2.54, -2.54, FillStyle::Background),
            text(0.0, 1.27, "V+", 1.0),
            text(0.0, -1.27, "V-", 1.0),
            pin(0.0, 5.08, 270.0, PIN_LEN, "8", "V+", PowerIn),
            pin(0.0, -5.08, 90.0, PIN_LEN, "4", "V-", PowerIn),
        ],
    });
    s.show_pin_names = false;
    s
}

/// Three-terminal regulator, SOT-223 (1117-style) pinout: 1 = GND, 2 = VOUT, 3 = VIN.
pub(crate) fn regulator() -> Symbol {
    let items = vec![
        rect(-5.08, 2.54, 5.08, -2.54, FillStyle::Background),
        pin(-7.62, 0.0, 0.0, PIN_LEN, "3", "VIN", PowerIn),
        pin(7.62, 0.0, 180.0, PIN_LEN, "2", "VOUT", PinType::PowerOut),
        pin(0.0, -5.08, 90.0, PIN_LEN, "1", "GND", PowerIn),
    ];
    symbol("Regulator_3Pin", "U", "Fixed linear regulator (1 = GND, 2 = VOUT, 3 = VIN)", items, (-5.08, 3.81, HAlign::Left), (0.635, -3.81, HAlign::Left))
}

/// Single-row connector Conn_01xNN, pin 1 at the top.
pub(crate) fn connector(n: u32) -> Symbol {
    let half = (n as f64 - 1.0) / 2.0 * PITCH;
    let top = half + GRID;
    let mut items = vec![rect(-2.54, top, 0.0, -top, FillStyle::Background)];
    for i in 0..n {
        let y = half - i as f64 * PITCH;
        let num = (i + 1).to_string();
        items.push(pin(-5.08, y, 0.0, PIN_LEN, &num, &format!("Pin_{num}"), Passive));
        items.push(rect(-1.016, y + 0.127, 0.0, y - 0.127, FillStyle::Solid));
    }
    let name = format!("Conn_01x{n:02}");
    let mut s = symbol(&name, "J", &format!("Generic connector, single row, {n} pins"), items, (-1.27, top + GRID, HAlign::Center), (-1.27, -top - GRID, HAlign::Center));
    s.show_pin_names = false;
    s
}
