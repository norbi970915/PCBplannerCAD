//! Every object that can live in a document: graphics shared by all editors,
//! symbol pins, footprint pads, and schematic connectivity objects.
//!
//! Conventions: world Y points up, angles are degrees counter-clockwise,
//! mirroring is across a vertical line (X is negated).

use serde::{Deserialize, Serialize};

use crate::document::LayerId;
use crate::error::CoreError;
use crate::geom::{Placement, mirror_x, normalize_deg, rotate};
use crate::pcb::{ArcTrack, BoardSettings, PlacedFootprint, Side, Track, Via, Zone};
use crate::units::{Coord, Point};

// ---------- Shared enums ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillStyle {
    #[default]
    None,
    /// Filled with the stroke color.
    Solid,
    /// Filled with the symbol body color (schematic only).
    Background,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl HAlign {
    pub fn flipped(self) -> Self {
        match self {
            HAlign::Left => HAlign::Right,
            HAlign::Center => HAlign::Center,
            HAlign::Right => HAlign::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    Bottom,
    #[default]
    Middle,
    Top,
}

/// Whether an angle reads left-to-right / right-to-left (as opposed to vertical).
fn is_horizontal(deg: f64) -> bool {
    let d = normalize_deg(deg);
    !(45.0..135.0).contains(&d) && !(225.0..315.0).contains(&d)
}

// ---------- Graphics ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub a: Point,
    pub b: Point,
    /// Stroke width; 0 means "default thin line" in schematic contexts.
    pub width: Coord,
    pub layer: LayerId,
}

/// Circular arc from `start_angle` sweeping `sweep` degrees (positive = CCW).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arc {
    pub center: Point,
    pub radius: Coord,
    pub start_angle: f64,
    pub sweep: f64,
    pub width: Coord,
    pub layer: LayerId,
}

impl Arc {
    pub fn point_at(&self, deg: f64) -> Point {
        let (s, c) = deg.to_radians().sin_cos();
        Point::new(self.center.x + (self.radius as f64 * c).round() as Coord, self.center.y + (self.radius as f64 * s).round() as Coord)
    }

    pub fn start(&self) -> Point {
        self.point_at(self.start_angle)
    }

    pub fn end(&self) -> Point {
        self.point_at(self.start_angle + self.sweep)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Circle {
    pub center: Point,
    pub radius: Coord,
    pub width: Coord,
    pub layer: LayerId,
    #[serde(default)]
    pub fill: FillStyle,
}

/// Axis-aligned rectangle between two opposite corners.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub a: Point,
    pub b: Point,
    pub width: Coord,
    pub layer: LayerId,
    #[serde(default)]
    pub fill: FillStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Polyline {
    pub points: Vec<Point>,
    #[serde(default)]
    pub closed: bool,
    pub width: Coord,
    pub layer: LayerId,
    #[serde(default)]
    pub fill: FillStyle,
}

/// Free text. In footprints, `.Designator` and `.Value` are replaced by the
/// owning part's values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub pos: Point,
    pub text: String,
    pub height: Coord,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub h_align: HAlign,
    #[serde(default)]
    pub v_align: VAlign,
    pub layer: LayerId,
    /// Drawn mirrored (bottom-side PCB text).
    #[serde(default)]
    pub mirrored: bool,
}

pub const DESIGNATOR_TOKEN: &str = ".Designator";
pub const VALUE_TOKEN: &str = ".Value";

// ---------- Symbol pins ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    #[default]
    Passive,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinShape {
    #[default]
    Line,
    Inverted,
    Clock,
    InvertedClock,
}

/// A symbol pin. `pos` is the electrical connection point (the outer end);
/// the pin extends `length` in direction `rotation` toward the symbol body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub pos: Point,
    pub length: Coord,
    pub rotation: f64,
    pub name: String,
    pub number: String,
    #[serde(default)]
    pub electrical: PinType,
    #[serde(default)]
    pub shape: PinShape,
    #[serde(default)]
    pub hidden: bool,
}

impl Pin {
    /// The inner end, where the pin meets the body.
    pub fn body_end(&self) -> Point {
        rotate(self.pos.translated(self.length, 0), self.pos, self.rotation)
    }
}

// ---------- Footprint pads ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PadShape {
    /// Uses `width` as the diameter.
    Circle,
    #[default]
    Rect,
    RoundRect,
    Oval,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PadKind {
    #[default]
    Smd,
    /// Plated through-hole.
    Tht,
    /// Non-plated hole.
    Npth,
}

/// Round when `width == height`, otherwise a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drill {
    pub width: Coord,
    pub height: Coord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pad {
    pub number: String,
    pub pos: Point,
    #[serde(default)]
    pub rotation: f64,
    pub shape: PadShape,
    pub width: Coord,
    pub height: Coord,
    /// Corner radius as a fraction of the shorter side (RoundRect only, 0–0.5).
    #[serde(default)]
    pub corner_ratio: f64,
    /// Serialized as `padKind`: `kind` is the item tag.
    #[serde(rename = "padKind")]
    pub kind: PadKind,
    /// TOP_COPPER / BOTTOM_COPPER for SMD pads, MULTI_LAYER for holes.
    pub layer: LayerId,
    #[serde(default)]
    pub drill: Option<Drill>,
}

// ---------- Schematic connectivity ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wire {
    pub a: Point,
    pub b: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bus {
    pub a: Point,
    pub b: Point,
}

/// Diagonal stub from `pos` to `pos + size` joining a wire to a bus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusEntry {
    pub pos: Point,
    pub size: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Junction {
    pub pos: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoConnect {
    pub pos: Point,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelScope {
    /// Connects to same-named labels on the same sheet.
    #[default]
    Local,
    /// Connects to same-named global labels on every sheet.
    Global,
    /// Connects to the matching entry on the parent sheet symbol.
    Hierarchical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortShape {
    Input,
    Output,
    Bidirectional,
    TriState,
    #[default]
    Passive,
}

/// `pos` is the connection point; text runs in direction `rotation`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetLabel {
    pub pos: Point,
    pub text: String,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub scope: LabelScope,
    #[serde(default)]
    pub shape: PortShape,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerStyle {
    #[default]
    Bar,
    Arrow,
    Circle,
    Wave,
    Ground,
    SignalGround,
    Earth,
}

/// A global net connection drawn as a power symbol. `rotation` is the
/// direction the symbol points away from its connection point (90 = up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPort {
    pub pos: Point,
    pub net: String,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub style: PowerStyle,
}

/// A named, positionable text attached to a part (Designator, Value, …).
/// `offset` is relative to the part origin in world orientation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub name: String,
    pub value: String,
    pub offset: Point,
    #[serde(default)]
    pub rotation: f64,
    pub height: Coord,
    #[serde(default)]
    pub h_align: HAlign,
    #[serde(default)]
    pub v_align: VAlign,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

pub const FIELD_DESIGNATOR: &str = "Designator";
pub const FIELD_VALUE: &str = "Value";
pub const FIELD_FOOTPRINT: &str = "Footprint";

/// A placed component unit. `lib_ref` keys the document's part cache.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub pos: Point,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub mirror: bool,
    pub lib_ref: String,
    /// 1-based unit index into the symbol's units.
    #[serde(default = "default_unit")]
    pub unit: u32,
    #[serde(default)]
    pub fields: Vec<Field>,
}

fn default_unit() -> u32 {
    1
}

impl Part {
    pub fn placement(&self) -> Placement {
        Placement { offset: self.pos, rotation: self.rotation, mirror: self.mirror }
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn field_value(&self, name: &str) -> &str {
        self.field(name).map_or("", |f| f.value.as_str())
    }

    pub fn designator(&self) -> &str {
        self.field_value(FIELD_DESIGNATOR)
    }

    pub fn set_field(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.fields.iter_mut().find(|f| f.name == name) {
            Some(f) => f.value = value,
            None => self.fields.push(Field {
                name: name.into(),
                value,
                offset: Point::default(),
                rotation: 0.0,
                height: crate::units::mm(1.27),
                h_align: HAlign::Left,
                v_align: VAlign::Middle,
                visible: false,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SheetSide {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

/// A connection on a sheet symbol; `offset` runs from the top (left/right
/// sides) or from the left (top/bottom sides) of the sheet rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetEntry {
    pub name: String,
    pub side: SheetSide,
    pub offset: Coord,
    #[serde(default)]
    pub shape: PortShape,
}

/// A hierarchical sheet symbol. `pos` is the top-left corner, `size` is
/// positive width/height (the rectangle extends right and down).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub pos: Point,
    pub size: Point,
    pub name: String,
    /// Child schematic path, relative to the project directory.
    pub file: String,
    #[serde(default)]
    pub entries: Vec<SheetEntry>,
}

impl Sheet {
    pub fn entry_point(&self, e: &SheetEntry) -> Point {
        match e.side {
            SheetSide::Left => Point::new(self.pos.x, self.pos.y - e.offset),
            SheetSide::Right => Point::new(self.pos.x + self.size.x, self.pos.y - e.offset),
            SheetSide::Top => Point::new(self.pos.x + e.offset, self.pos.y),
            SheetSide::Bottom => Point::new(self.pos.x + e.offset, self.pos.y - self.size.y),
        }
    }

    fn center(&self) -> Point {
        Point::new(self.pos.x + self.size.x / 2, self.pos.y - self.size.y / 2)
    }

    fn with_center(&self, c: Point) -> Sheet {
        Sheet { pos: Point::new(c.x - self.size.x / 2, c.y + self.size.y / 2), ..self.clone() }
    }
}

// ---------- The item enum ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Line(Line),
    Arc(Arc),
    Circle(Circle),
    Rect(Rect),
    Polyline(Polyline),
    Text(Text),
    Pin(Pin),
    Pad(Pad),
    Wire(Wire),
    Bus(Bus),
    BusEntry(BusEntry),
    Junction(Junction),
    NoConnect(NoConnect),
    NetLabel(NetLabel),
    PowerPort(PowerPort),
    Part(Part),
    Sheet(Sheet),
    // Board objects (see crate::pcb).
    Footprint(PlacedFootprint),
    Track(Track),
    ArcTrack(ArcTrack),
    Via(Via),
    Zone(Zone),
    Dimension(crate::pcb::dimension::Dimension),
    /// Singleton board settings (stack, rules, net classes) of a PCB document.
    Board(BoardSettings),
}

fn check(ok: bool, msg: &'static str) -> Result<(), CoreError> {
    if ok { Ok(()) } else { Err(CoreError::InvalidGeometry(msg)) }
}

fn rot_text_align(h: HAlign, deg: f64) -> HAlign {
    if is_horizontal(deg) { h.flipped() } else { h }
}

impl Item {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Item::Line(_) => "line",
            Item::Arc(_) => "arc",
            Item::Circle(_) => "circle",
            Item::Rect(_) => "rect",
            Item::Polyline(_) => "polyline",
            Item::Text(_) => "text",
            Item::Pin(_) => "pin",
            Item::Pad(_) => "pad",
            Item::Wire(_) => "wire",
            Item::Bus(_) => "bus",
            Item::BusEntry(_) => "busEntry",
            Item::Junction(_) => "junction",
            Item::NoConnect(_) => "noConnect",
            Item::NetLabel(_) => "netLabel",
            Item::PowerPort(_) => "powerPort",
            Item::Part(_) => "part",
            Item::Sheet(_) => "sheet",
            Item::Footprint(_) => "footprint",
            Item::Track(_) => "track",
            Item::ArcTrack(_) => "arcTrack",
            Item::Via(_) => "via",
            Item::Zone(_) => "zone",
            Item::Dimension(_) => "dimension",
            Item::Board(_) => "board",
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        match self {
            Item::Line(l) => {
                check(l.width >= 0, "line width must not be negative")?;
                check(l.a != l.b, "line has zero length")
            }
            Item::Arc(a) => {
                check(a.radius > 0, "arc radius must be positive")?;
                check(a.sweep != 0.0 && a.sweep.is_finite(), "arc sweep must be non-zero")?;
                check(a.width >= 0, "arc width must not be negative")
            }
            Item::Circle(c) => check(c.radius > 0, "circle radius must be positive"),
            Item::Rect(r) => check(r.a.x != r.b.x && r.a.y != r.b.y, "rectangle has zero area"),
            Item::Polyline(p) => check(p.points.len() >= 2, "polyline needs at least two points"),
            Item::Text(t) => {
                check(!t.text.is_empty(), "text is empty")?;
                check(t.height > 0, "text height must be positive")
            }
            Item::Pin(p) => check(p.length >= 0, "pin length must not be negative"),
            Item::Pad(p) => {
                check(p.width > 0 && p.height > 0, "pad size must be positive")?;
                check(p.kind == PadKind::Smd || p.drill.is_some_and(|d| d.width > 0 && d.height > 0), "hole pads need a drill")?;
                check((0.0..=0.5).contains(&p.corner_ratio), "corner ratio must be between 0 and 0.5")
            }
            Item::Wire(w) => check(w.a != w.b, "wire has zero length"),
            Item::Bus(b) => check(b.a != b.b, "bus has zero length"),
            Item::BusEntry(b) => check(b.size != Point::default(), "bus entry has zero size"),
            Item::NetLabel(l) => check(!l.text.is_empty(), "net label is empty"),
            Item::PowerPort(p) => check(!p.net.is_empty(), "power port has no net name"),
            Item::Part(p) => {
                check(!p.lib_ref.is_empty(), "part has no library reference")?;
                check(p.unit >= 1, "part unit is 1-based")
            }
            Item::Sheet(s) => {
                check(s.size.x > 0 && s.size.y > 0, "sheet size must be positive")?;
                check(!s.file.is_empty(), "sheet has no file")
            }
            Item::Footprint(f) => check(!f.footprint.is_empty(), "footprint has no library reference"),
            Item::Track(t) => {
                check(t.width > 0, "track width must be positive")?;
                check(t.a != t.b, "track has zero length")
            }
            Item::ArcTrack(a) => {
                check(a.width > 0 && a.radius > 0, "arc track needs positive width and radius")?;
                check(a.sweep != 0.0 && a.sweep.is_finite(), "arc track sweep must be non-zero")
            }
            Item::Via(v) => {
                check(v.drill > 0, "via drill must be positive")?;
                check(v.diameter > v.drill, "via diameter must exceed its drill")
            }
            Item::Zone(z) => check(z.outline.len() >= 3, "zone outline needs at least three points"),
            Item::Dimension(d) => {
                check(d.a != d.b, "dimension has zero length")?;
                check(d.text_height > 0 && d.width >= 0, "dimension text height must be positive")
            }
            Item::Junction(_) | Item::NoConnect(_) | Item::Board(_) => Ok(()),
        }
    }

    pub fn translated(&self, dx: Coord, dy: Coord) -> Item {
        let t = |p: Point| p.translated(dx, dy);
        match self.clone() {
            Item::Line(l) => Item::Line(Line { a: t(l.a), b: t(l.b), ..l }),
            Item::Arc(a) => Item::Arc(Arc { center: t(a.center), ..a }),
            Item::Circle(c) => Item::Circle(Circle { center: t(c.center), ..c }),
            Item::Rect(r) => Item::Rect(Rect { a: t(r.a), b: t(r.b), ..r }),
            Item::Polyline(p) => Item::Polyline(Polyline { points: p.points.iter().copied().map(t).collect(), ..p }),
            Item::Text(x) => Item::Text(Text { pos: t(x.pos), ..x }),
            Item::Pin(p) => Item::Pin(Pin { pos: t(p.pos), ..p }),
            Item::Pad(p) => Item::Pad(Pad { pos: t(p.pos), ..p }),
            Item::Wire(w) => Item::Wire(Wire { a: t(w.a), b: t(w.b) }),
            Item::Bus(b) => Item::Bus(Bus { a: t(b.a), b: t(b.b) }),
            Item::BusEntry(b) => Item::BusEntry(BusEntry { pos: t(b.pos), ..b }),
            Item::Junction(j) => Item::Junction(Junction { pos: t(j.pos) }),
            Item::NoConnect(n) => Item::NoConnect(NoConnect { pos: t(n.pos) }),
            Item::NetLabel(l) => Item::NetLabel(NetLabel { pos: t(l.pos), ..l }),
            Item::PowerPort(p) => Item::PowerPort(PowerPort { pos: t(p.pos), ..p }),
            Item::Part(p) => Item::Part(Part { pos: t(p.pos), ..p }),
            Item::Sheet(s) => Item::Sheet(Sheet { pos: t(s.pos), ..s }),
            Item::Footprint(f) => Item::Footprint(PlacedFootprint { pos: t(f.pos), ..f }),
            Item::Track(x) => Item::Track(Track { a: t(x.a), b: t(x.b), ..x }),
            Item::ArcTrack(x) => Item::ArcTrack(ArcTrack { center: t(x.center), ..x }),
            Item::Via(v) => Item::Via(Via { pos: t(v.pos), ..v }),
            Item::Zone(z) => Item::Zone(map_zone(z, t)),
            Item::Dimension(d) => Item::Dimension(crate::pcb::dimension::Dimension { a: t(d.a), b: t(d.b), ..d }),
            Item::Board(b) => Item::Board(b),
        }
    }

    /// Rotates counter-clockwise by `deg` around `c`.
    pub fn rotated(&self, c: Point, deg: f64) -> Item {
        let r = |p: Point| rotate(p, c, deg);
        let ang = |a: f64| normalize_deg(a + deg);
        let quarter = normalize_deg(deg) % 90.0 == 0.0;
        match self.clone() {
            Item::Line(l) => Item::Line(Line { a: r(l.a), b: r(l.b), ..l }),
            Item::Arc(a) => Item::Arc(Arc { center: r(a.center), start_angle: ang(a.start_angle), ..a }),
            Item::Circle(x) => Item::Circle(Circle { center: r(x.center), ..x }),
            Item::Rect(x) if quarter => Item::Rect(Rect { a: r(x.a), b: r(x.b), ..x }),
            Item::Rect(x) => Item::Polyline(Polyline {
                points: [x.a, Point::new(x.b.x, x.a.y), x.b, Point::new(x.a.x, x.b.y)].into_iter().map(r).collect(),
                closed: true,
                width: x.width,
                layer: x.layer,
                fill: x.fill,
            }),
            Item::Polyline(p) => Item::Polyline(Polyline { points: p.points.iter().copied().map(r).collect(), ..p }),
            Item::Text(t) => Item::Text(Text { pos: r(t.pos), rotation: ang(t.rotation), ..t }),
            Item::Pin(p) => Item::Pin(Pin { pos: r(p.pos), rotation: ang(p.rotation), ..p }),
            Item::Pad(p) => Item::Pad(Pad { pos: r(p.pos), rotation: ang(p.rotation), ..p }),
            Item::Wire(w) => Item::Wire(Wire { a: r(w.a), b: r(w.b) }),
            Item::Bus(b) => Item::Bus(Bus { a: r(b.a), b: r(b.b) }),
            Item::BusEntry(b) => Item::BusEntry(BusEntry { pos: r(b.pos), size: rotate(b.size, Point::default(), deg) }),
            Item::Junction(j) => Item::Junction(Junction { pos: r(j.pos) }),
            Item::NoConnect(n) => Item::NoConnect(NoConnect { pos: r(n.pos) }),
            Item::NetLabel(l) => Item::NetLabel(NetLabel { pos: r(l.pos), rotation: ang(l.rotation), ..l }),
            Item::PowerPort(p) => Item::PowerPort(PowerPort { pos: r(p.pos), rotation: ang(p.rotation), ..p }),
            Item::Part(p) => Item::Part(Part {
                pos: r(p.pos),
                rotation: ang(p.rotation),
                fields: p
                    .fields
                    .iter()
                    .map(|f| Field { offset: rotate(f.offset, Point::default(), deg), rotation: ang(f.rotation), ..f.clone() })
                    .collect(),
                ..p
            }),
            // Sheets stay axis-aligned: only their center moves.
            Item::Sheet(s) => Item::Sheet(s.with_center(r(s.center()))),
            Item::Footprint(f) => Item::Footprint(PlacedFootprint { pos: r(f.pos), rotation: ang(f.rotation), ..f }),
            Item::Track(x) => Item::Track(Track { a: r(x.a), b: r(x.b), ..x }),
            Item::ArcTrack(x) => Item::ArcTrack(ArcTrack { center: r(x.center), start_angle: ang(x.start_angle), ..x }),
            Item::Via(v) => Item::Via(Via { pos: r(v.pos), ..v }),
            Item::Zone(z) => Item::Zone(map_zone(z, r)),
            Item::Dimension(d) => Item::Dimension(crate::pcb::dimension::Dimension { a: r(d.a), b: r(d.b), angle: ang(d.angle), ..d }),
            Item::Board(b) => Item::Board(b),
        }
    }

    /// Mirrors across the vertical line through `c`.
    pub fn mirrored(&self, c: Point) -> Item {
        let m = |p: Point| mirror_x(p, c);
        let flip_angle = |a: f64| normalize_deg(180.0 - a);
        match self.clone() {
            Item::Line(l) => Item::Line(Line { a: m(l.a), b: m(l.b), ..l }),
            Item::Arc(a) => Item::Arc(Arc { center: m(a.center), start_angle: normalize_deg(180.0 - a.start_angle - a.sweep), ..a }),
            Item::Circle(x) => Item::Circle(Circle { center: m(x.center), ..x }),
            Item::Rect(x) => Item::Rect(Rect { a: m(x.a), b: m(x.b), ..x }),
            Item::Polyline(p) => Item::Polyline(Polyline { points: p.points.iter().copied().map(m).collect(), ..p }),
            Item::Text(t) => Item::Text(Text { pos: m(t.pos), h_align: rot_text_align(t.h_align, t.rotation), ..t }),
            Item::Pin(p) => Item::Pin(Pin { pos: m(p.pos), rotation: flip_angle(p.rotation), ..p }),
            Item::Pad(p) => Item::Pad(Pad { pos: m(p.pos), rotation: normalize_deg(-p.rotation), ..p }),
            Item::Wire(w) => Item::Wire(Wire { a: m(w.a), b: m(w.b) }),
            Item::Bus(b) => Item::Bus(Bus { a: m(b.a), b: m(b.b) }),
            Item::BusEntry(b) => Item::BusEntry(BusEntry { pos: m(b.pos), size: Point::new(-b.size.x, b.size.y) }),
            Item::Junction(j) => Item::Junction(Junction { pos: m(j.pos) }),
            Item::NoConnect(n) => Item::NoConnect(NoConnect { pos: m(n.pos) }),
            Item::NetLabel(l) => Item::NetLabel(NetLabel { pos: m(l.pos), rotation: flip_angle(l.rotation), ..l }),
            Item::PowerPort(p) => Item::PowerPort(PowerPort { pos: m(p.pos), rotation: flip_angle(p.rotation), ..p }),
            Item::Part(p) => Item::Part(Part {
                pos: m(p.pos),
                rotation: normalize_deg(-p.rotation),
                mirror: !p.mirror,
                fields: p
                    .fields
                    .iter()
                    .map(|f| Field {
                        offset: Point::new(-f.offset.x, f.offset.y),
                        h_align: rot_text_align(f.h_align, f.rotation),
                        ..f.clone()
                    })
                    .collect(),
                ..p
            }),
            Item::Sheet(s) => {
                let mut out = s.with_center(m(s.center()));
                for e in &mut out.entries {
                    e.side = match e.side {
                        SheetSide::Left => SheetSide::Right,
                        SheetSide::Right => SheetSide::Left,
                        side => side,
                    };
                    if matches!(e.side, SheetSide::Top | SheetSide::Bottom) {
                        e.offset = s.size.x - e.offset;
                    }
                }
                Item::Sheet(out)
            }
            // Mirroring a footprint flips it to the other board side:
            // geometry mirrors and its layers swap sides.
            Item::Footprint(f) => Item::Footprint(PlacedFootprint { pos: m(f.pos), rotation: normalize_deg(-f.rotation), side: f.side.flipped(), ..f }),
            Item::Track(x) => Item::Track(Track { a: m(x.a), b: m(x.b), ..x }),
            Item::ArcTrack(x) => Item::ArcTrack(ArcTrack { center: m(x.center), start_angle: normalize_deg(180.0 - x.start_angle - x.sweep), ..x }),
            Item::Via(v) => Item::Via(Via { pos: m(v.pos), ..v }),
            Item::Zone(z) => Item::Zone(map_zone(z, m)),
            // The left normal flips with the mirror, so the offset changes sign.
            Item::Dimension(d) => Item::Dimension(crate::pcb::dimension::Dimension { a: m(d.a), b: m(d.b), angle: flip_angle(d.angle), offset: -d.offset, ..d }),
            Item::Board(b) => Item::Board(b),
        }
    }
}

/// Applies a point transform to a zone's outline and its computed fill.
fn map_zone(z: Zone, f: impl Fn(Point) -> Point) -> Zone {
    let fill = z
        .fill
        .iter()
        .map(|p| crate::pcb::ZonePolygon {
            outer: p.outer.iter().copied().map(&f).collect(),
            holes: p.holes.iter().map(|h| h.iter().copied().map(&f).collect()).collect(),
        })
        .collect();
    Zone { outline: z.outline.iter().copied().map(&f).collect(), fill, ..z }
}

impl PlacedFootprint {
    pub fn is_bottom(&self) -> bool {
        self.side == Side::Bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::SCHEMATIC;
    use crate::units::mm;

    fn part() -> Part {
        let mut p = Part { pos: Point::new(mm(10.0), 0), rotation: 0.0, mirror: false, lib_ref: "Lib/R".into(), unit: 1, fields: vec![] };
        p.set_field(FIELD_DESIGNATOR, "R1");
        p.fields[0].offset = Point::new(mm(2.0), mm(1.0));
        p
    }

    #[test]
    fn part_rotation_and_mirror_compose_with_placement() {
        let local_pin = Point::new(mm(-2.54), 0);
        let rotated = match Item::Part(part()).rotated(Point::new(mm(10.0), 0), 90.0) {
            Item::Part(p) => p,
            _ => unreachable!(),
        };
        assert_eq!(rotated.placement().apply(local_pin), Point::new(mm(10.0), mm(-2.54)));
        assert_eq!(rotated.fields[0].offset, Point::new(mm(-1.0), mm(2.0)));

        // Mirroring the placed part must equal mirroring its world geometry.
        let c = Point::new(mm(3.0), 0);
        let mirrored = match Item::Part(rotated.clone()).mirrored(c) {
            Item::Part(p) => p,
            _ => unreachable!(),
        };
        assert_eq!(mirrored.placement().apply(local_pin), mirror_x(rotated.placement().apply(local_pin), c));
    }

    #[test]
    fn arc_mirror_keeps_endpoints_on_mirrored_curve() {
        let a = Arc { center: Point::new(0, 0), radius: mm(1.0), start_angle: 0.0, sweep: 90.0, width: 0, layer: SCHEMATIC };
        let Item::Arc(m) = Item::Arc(a.clone()).mirrored(Point::default()) else { unreachable!() };
        let ends = [m.start(), m.end()];
        assert!(ends.contains(&mirror_x(a.start(), Point::default())));
        assert!(ends.contains(&mirror_x(a.end(), Point::default())));
    }

    #[test]
    fn pin_body_end_follows_rotation() {
        let p = Pin {
            pos: Point::new(0, 0),
            length: mm(2.54),
            rotation: 90.0,
            name: "A".into(),
            number: "1".into(),
            electrical: PinType::Input,
            shape: PinShape::Line,
            hidden: false,
        };
        assert_eq!(p.body_end(), Point::new(0, mm(2.54)));
    }

    #[test]
    fn sheet_entries_follow_sides() {
        let s = Sheet {
            pos: Point::new(0, mm(10.0)),
            size: Point::new(mm(20.0), mm(10.0)),
            name: "Power".into(),
            file: "power.arsch".into(),
            entries: vec![SheetEntry { name: "VIN".into(), side: SheetSide::Left, offset: mm(2.0), shape: PortShape::Input }],
        };
        assert_eq!(s.entry_point(&s.entries[0]), Point::new(0, mm(8.0)));
        let Item::Sheet(m) = Item::Sheet(s).mirrored(Point::new(mm(10.0), 0)) else { unreachable!() };
        assert_eq!(m.entries[0].side, SheetSide::Right);
        assert_eq!(m.entry_point(&m.entries[0]), Point::new(mm(20.0), mm(8.0)));
    }
}
