//! `.kicad_sch` schematics, following hierarchical sheets.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use pcbplannercad_core::geom::Placement;
use pcbplannercad_core::item::{
    Bus, BusEntry, FIELD_DESIGNATOR, Field, HAlign, Item, Junction, LabelScope, NetLabel, NoConnect, Part, PinType, PortShape, PowerPort,
    PowerStyle, Sheet, SheetEntry, SheetSide, VAlign, Wire,
};
use pcbplannercad_core::library::CachedPart;
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::{DocKind, Document, Library};

use super::common::{Notes, YAxis, angle, effects, unescape};
use super::sexpr::{List, parse};
use super::symbols::{build_all, field_name, graphic, read_text, read_text_box};
use crate::report::{ImportError, ImportReport};
use crate::{ImportedSheet, SchematicImport};

const Y: YAxis = YAxis::Down;

/// Library nickname used when a `lib_id` has no `Lib:` prefix.
const NO_LIB: &str = "local";

/// Upper bound on imported sheet instances (guards against exponential reuse).
const MAX_SHEETS: usize = 2000;

pub fn import_schematic(root: &Path) -> Result<SchematicImport, ImportError> {
    let root_text = std::fs::read_to_string(root)?;
    let mut ctx = Ctx::default();
    let root_name = arsch_name(&root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "root.kicad_sch".into()));
    ctx.used_names.insert(root_name.clone());
    let mut queue = VecDeque::new();
    queue.push_back(Pending {
        path: root.to_path_buf(),
        file_name: root_name.clone(),
        sheet_path: None,
        k6_prefix: String::new(),
        text: Some(root_text),
        ancestors: vec![(canonical(root), root_name)],
    });

    let mut first = true;
    while let Some(mut p) = queue.pop_front() {
        let text = match p.text.take().map(Ok).unwrap_or_else(|| std::fs::read_to_string(&p.path)) {
            Ok(t) => t,
            Err(e) => {
                ctx.report.warn(format!("sheet {}", p.file_name), format!("child sheet {} could not be read: {e}", p.path.display()));
                continue;
            }
        };
        let parsed = match parse(&text) {
            Ok(l) if l.head() == "kicad_sch" => l,
            Ok(l) => {
                let e = ImportError::Invalid(format!("expected a kicad_sch, found '{}'", l.head()));
                if first {
                    return Err(e);
                }
                ctx.report.warn(format!("sheet {}", p.file_name), format!("child sheet skipped: {e}"));
                continue;
            }
            Err(e) => {
                if first {
                    return Err(e);
                }
                ctx.report.warn(format!("sheet {}", p.file_name), format!("child sheet skipped: {e}"));
                continue;
            }
        };
        let children = ctx.import_sheet(&parsed, &p, first);
        first = false;
        queue.extend(children);
    }

    for (file, names) in std::mem::take(&mut ctx.copies) {
        if names.len() > 1 {
            ctx.report.info(
                format!("sheet file {file}"),
                format!("used by {} sheet symbols; imported one copy per instance: {}", names.len(), names.join(", ")),
            );
        }
    }

    Ok(SchematicImport { sheets: ctx.sheets, libraries: ctx.libs.into_values().collect(), report: ctx.report })
}

#[derive(Default)]
struct Ctx {
    report: ImportReport,
    sheets: Vec<ImportedSheet>,
    libs: BTreeMap<String, Library>,
    /// structured format 6 root-level `symbol_instances`: path → (reference, unit).
    k6_instances: HashMap<String, (String, Option<u32>)>,
    used_names: HashSet<String>,
    /// Per child file (in discovery order): the imported copy of each instance.
    copies: Vec<(String, Vec<String>)>,
    copy_index: HashMap<PathBuf, usize>,
}

struct Pending {
    path: PathBuf,
    file_name: String,
    /// structured format 7+ instance path of this sheet ("/root-uuid/sheet-uuid…"); `None` for the root.
    sheet_path: Option<String>,
    /// structured format 6 path prefix (sheet uuids without the root).
    k6_prefix: String,
    text: Option<String>,
    /// Canonical file and imported name of this sheet and every parent, root first.
    ancestors: Vec<(PathBuf, String)>,
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// "sub/amp.kicad_sch" → "sub/amp.arsch".
pub fn arsch_name(file: &str) -> String {
    let file = file.replace('\\', "/");
    let (dir, base) = file.rsplit_once('/').map_or(("", file.as_str()), |(d, b)| (d, b));
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    if dir.is_empty() { format!("{stem}.arsch") } else { format!("{dir}/{stem}.arsch") }
}

impl Ctx {
    fn import_sheet(&mut self, root: &List, p: &Pending, is_root: bool) -> Vec<Pending> {
        let ctx_name = format!("sheet {}", p.file_name);
        let mut notes = Notes::default();
        let uuid = root.child_str("uuid").unwrap_or("");
        let sheet_path = p.sheet_path.clone().unwrap_or_else(|| format!("/{uuid}"));

        if is_root && let Some(si) = root.child("symbol_instances") {
            for path in si.children("path") {
                if let Some(key) = path.atom(1) {
                    let reference = path.child_str("reference").unwrap_or("").to_string();
                    let unit = path.child_num("unit").map(|u| u as u32);
                    self.k6_instances.insert(key.to_string(), (reference, unit));
                }
            }
        }

        // Embedded library symbols → cached parts.
        let mut cache: HashMap<String, CachedPart> = HashMap::new();
        if let Some(ls) = root.child("lib_symbols") {
            for (mut symbol, mut component) in build_all(ls, &mut notes) {
                let full = symbol.name.clone();
                let (lib, comp) = full.split_once(':').unwrap_or((NO_LIB, full.as_str()));
                let (lib, comp) = (lib.to_string(), comp.to_string());
                symbol.name = comp.clone();
                component.name = comp.clone();
                component.symbol = comp;
                let cp = CachedPart { library: lib.clone(), component, symbol };
                let library = self.libs.entry(lib.clone()).or_insert_with(|| Library { name: lib, ..Default::default() });
                library.merge(Library {
                    name: String::new(),
                    symbols: vec![cp.symbol.clone()],
                    footprints: Vec::new(),
                    components: vec![cp.component.clone()],
                });
                cache.insert(full, cp);
            }
        }

        let mut items: Vec<Item> = Vec::new();
        let mut children = Vec::new();
        let base_dir = p.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let k6 = std::mem::take(&mut self.k6_instances);
        let inst = InstanceCtx { sheet_path: &sheet_path, k6_prefix: &p.k6_prefix, k6: &k6 };
        let mut extra_caches: Vec<CachedPart> = Vec::new();

        for l in root.lists() {
            match l.head() {
                "wire" | "bus" => {
                    let pts = Y.pts(l);
                    for w in pts.windows(2) {
                        if w[0] != w[1] {
                            items.push(if l.head() == "wire" {
                                Item::Wire(Wire { a: w[0], b: w[1] })
                            } else {
                                Item::Bus(Bus { a: w[0], b: w[1] })
                            });
                        }
                    }
                }
                "bus_entry" => {
                    if let (Some(pos), Some(size)) = (Y.child_pt(l, "at"), Y.child_pt(l, "size"))
                        && size != Point::default()
                    {
                        items.push(Item::BusEntry(BusEntry { pos, size }));
                    }
                }
                "junction" => items.extend(Y.child_pt(l, "at").map(|pos| Item::Junction(Junction { pos }))),
                "no_connect" => items.extend(Y.child_pt(l, "at").map(|pos| Item::NoConnect(NoConnect { pos }))),
                "label" | "global_label" | "hierarchical_label" => items.extend(read_label(l)),
                "text" => items.extend(read_text(l, 1.0, Y)),
                "text_box" => {
                    notes.info("text boxes imported as plain text", "");
                    items.extend(read_text_box(l, Y));
                }
                "symbol" => {
                    if let Some((item, missing)) = read_instance(l, &cache, &inst, &mut notes) {
                        items.push(item);
                        extra_caches.extend(missing);
                    }
                }
                "sheet" => {
                    if let Some((sheet, file, sheet_uuid)) = read_sheet(l, &mut notes) {
                        let child_path = base_dir.join(&file);
                        let key = canonical(&child_path);
                        let mut sheet = sheet;
                        if let Some((_, name)) = p.ancestors.iter().find(|(a, _)| *a == key) {
                            notes.warn("sheet includes itself (directly or through a child); recursion stopped", &file);
                            sheet.file = name.clone();
                        } else if self.used_names.len() >= MAX_SHEETS {
                            notes.warn("too many sheet instances; remaining child sheets not imported", &file);
                            sheet.file = arsch_name(&file);
                        } else {
                            // Every instance gets its own copy, so per-instance references survive:
                            // "amp.arsch", then "amp_2.arsch", "amp_3.arsch", …
                            let stem = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s).to_string();
                            let mut name = arsch_name(&file);
                            let mut n = 2;
                            while self.used_names.contains(&name) {
                                name = arsch_name(&format!("{stem}_{n}.kicad_sch"));
                                n += 1;
                            }
                            self.used_names.insert(name.clone());
                            let idx = *self.copy_index.entry(key.clone()).or_insert_with(|| {
                                self.copies.push((file.clone(), Vec::new()));
                                self.copies.len() - 1
                            });
                            self.copies[idx].1.push(name.clone());
                            sheet.file = name.clone();
                            let mut ancestors = p.ancestors.clone();
                            ancestors.push((key, name.clone()));
                            children.push(Pending {
                                path: child_path,
                                file_name: name,
                                sheet_path: Some(format!("{sheet_path}/{sheet_uuid}")),
                                k6_prefix: format!("{}/{sheet_uuid}", p.k6_prefix),
                                text: None,
                                ancestors,
                            });
                        }
                        items.push(Item::Sheet(sheet));
                    }
                }
                "polyline" | "rectangle" | "circle" | "arc" | "bezier" => {
                    if let Some(g) = graphic(l, Y, &mut notes, "") {
                        items.extend(g);
                    }
                }
                "image" => notes.info("images skipped", ""),
                "table" => notes.info("tables skipped", ""),
                "rule_area" => notes.info("rule areas skipped", ""),
                "netclass_flag" | "directive_label" => notes.info("net class / directive labels skipped", ""),
                "bus_alias" => notes.warn("bus aliases are not supported; buses using them lose their member names", ""),
                _ => {}
            }
        }

        self.k6_instances = k6;
        let mut doc = Document::from_item_list(DocKind::Schematic, items);
        for cp in cache.into_values().chain(extra_caches) {
            doc.put_cached_part(cp);
        }
        notes.flush(&mut self.report, &ctx_name);
        self.sheets.push(ImportedSheet { file_name: p.file_name.clone(), document: doc });
        children
    }
}

fn port_shape(s: Option<&str>) -> PortShape {
    match s.unwrap_or("") {
        "input" => PortShape::Input,
        "output" => PortShape::Output,
        "bidirectional" => PortShape::Bidirectional,
        "tri_state" => PortShape::TriState,
        _ => PortShape::Passive,
    }
}

fn read_label(l: &List) -> Option<Item> {
    let text = l.atom(1)?;
    if text.is_empty() {
        return None;
    }
    let at = l.child("at")?;
    let scope = match l.head() {
        "global_label" => LabelScope::Global,
        "hierarchical_label" => LabelScope::Hierarchical,
        _ => LabelScope::Local,
    };
    let shape = if scope == LabelScope::Local { PortShape::Passive } else { port_shape(l.child_str("shape")) };
    Some(Item::NetLabel(NetLabel { pos: Y.pt_of(at)?, text: unescape(text), rotation: angle(at.num(3).unwrap_or(0.0)), scope, shape }))
}

struct InstanceCtx<'a> {
    sheet_path: &'a str,
    k6_prefix: &'a str,
    k6: &'a HashMap<String, (String, Option<u32>)>,
}

/// Converts structured format's symbol orientation to ours (mirror = negate X, then
/// rotate counter-clockwise). structured format rotates first and then mirrors in the
/// sheet frame: about the Y axis (`mirror y`, left/right) or the X axis
/// (`mirror x`, up/down). Since `Mx ∘ R(a) = R(-a) ∘ Mx` and `My = R(180) ∘ Mx`,
/// that is (−a, mirror) and (180 − a, mirror) respectively.
pub fn orientation(rot: f64, mirror: Option<&str>) -> (f64, bool) {
    let r = angle(rot);
    match mirror {
        Some("y") => (angle(-r), true),
        Some("x") => (angle(180.0 - r), true),
        _ => (r, false),
    }
}

/// Field angle and alignment in world terms. structured format stores field angles and
/// justification in the symbol's frame and draws text readable (0° or 90°).
pub fn place_text(local_angle: f64, h: HAlign, v: VAlign, pl: &Placement) -> (f64, HAlign, VAlign) {
    let flip_v = |v: VAlign| match v {
        VAlign::Top => VAlign::Bottom,
        VAlign::Bottom => VAlign::Top,
        VAlign::Middle => VAlign::Middle,
    };
    let mut world = pl.apply_angle(local_angle);
    let (mut h, mut v) = (h, v);
    if pl.mirror {
        // Mirroring reverses the text frame's "up" direction.
        v = flip_v(v);
    }
    if world > 90.0 + 1e-6 && world <= 270.0 + 1e-6 {
        world = angle(world - 180.0);
        h = h.flipped();
        v = flip_v(v);
    }
    (world, h, v)
}

/// A placed symbol → `Part` (or `PowerPort` for power symbols). Also returns
/// a stub cache entry when the symbol is missing from `lib_symbols`.
fn read_instance(
    l: &List,
    cache: &HashMap<String, CachedPart>,
    inst: &InstanceCtx,
    notes: &mut Notes,
) -> Option<(Item, Option<CachedPart>)> {
    let lib_id = l.child_str("lib_id")?;
    let key = l.child_str("lib_name").unwrap_or(lib_id);
    let at = l.child("at")?;
    let pos = Y.pt_of(at)?;
    let (rotation, mirror) = orientation(at.num(3).unwrap_or(0.0), l.child_str("mirror"));
    let placement = Placement { offset: pos, rotation, mirror };
    let uuid = l.child_str("uuid").unwrap_or("");

    // Per-instance reference/unit: structured format 7+ `instances`, then structured format 6 `symbol_instances`.
    let mut inst_ref: Option<(String, Option<u32>)> = None;
    if let Some(instances) = l.child("instances") {
        let paths: Vec<&List> = instances.children("project").flat_map(|pr| pr.children("path")).collect();
        let chosen = paths.iter().find(|p| p.atom(1) == Some(inst.sheet_path)).or(paths.first());
        if let Some(p) = chosen {
            inst_ref = Some((p.child_str("reference").unwrap_or("").to_string(), p.child_num("unit").map(|u| u as u32)));
        }
    }
    if inst_ref.is_none() {
        inst_ref = inst.k6.get(&format!("{}/{uuid}", inst.k6_prefix)).cloned();
    }
    let unit = inst_ref.as_ref().and_then(|r| r.1).or_else(|| l.child_num("unit").map(|u| u as u32)).unwrap_or(1).max(1);
    if l.child_num("convert").or_else(|| l.child_num("body_style")).unwrap_or(1.0) > 1.0 {
        notes.info("De Morgan body style shown with the normal body", lib_id);
    }

    let cached = cache.get(key);
    let mut missing = None;
    let lib_ref = match cached {
        Some(cp) => cp.key(),
        None => {
            notes.warn("placed symbol not found in the embedded library; imported without graphics", key);
            let (lib, comp) = key.split_once(':').unwrap_or((NO_LIB, key));
            let mut symbol = pcbplannercad_core::Symbol::new(comp, "U");
            symbol.units = vec![Default::default(); unit as usize];
            let cp = CachedPart {
                library: lib.to_string(),
                component: pcbplannercad_core::Component {
                    name: comp.to_string(),
                    description: String::new(),
                    symbol: comp.to_string(),
                    footprints: Vec::new(),
                    pin_map: Vec::new(),
                    parameters: Vec::new(),
                },
                symbol,
            };
            let k = cp.key();
            missing = Some(cp);
            k
        }
    };

    let mut fields = Vec::new();
    let mut value = String::new();
    for prop in l.children("property") {
        let Some(kname) = prop.atom(1) else { continue };
        let mut fvalue = unescape(prop.atom(2).unwrap_or(""));
        if kname == "Reference"
            && let Some((r, _)) = &inst_ref
            && !r.is_empty()
        {
            fvalue = r.clone();
        }
        if kname == "Value" {
            value = fvalue.clone();
        }
        let e = effects(prop);
        let fat = prop.child("at");
        let fpos = fat.and_then(|a| Y.pt_of(a)).unwrap_or(pos);
        let (frot, h, v) = place_text(fat.and_then(|a| a.num(3)).unwrap_or(0.0), e.h_align, e.v_align, &placement);
        fields.push(Field {
            name: field_name(kname).to_string(),
            value: fvalue,
            offset: Point::new(fpos.x - pos.x, fpos.y - pos.y),
            rotation: frot,
            height: e.height,
            h_align: h,
            v_align: v,
            visible: !e.hidden,
        });
    }
    if !fields.iter().any(|f| f.name == FIELD_DESIGNATOR)
        && let Some((r, _)) = inst_ref.filter(|r| !r.0.is_empty())
    {
        let mut part = Part { pos, rotation, mirror, lib_ref: String::new(), unit, fields: Vec::new() };
        part.set_field(FIELD_DESIGNATOR, r);
        fields.extend(part.fields);
    }

    // Power symbols become power ports when their pin is a power input
    // (PWR_FLAG-style symbols with output pins stay ordinary parts).
    if let Some(cp) = cached.filter(|cp| cp.symbol.is_power) {
        let pin = cp.symbol.unit_pins(unit).next().or_else(|| cp.symbol.all_pins().map(|(_, p)| p).next());
        if let Some(pin) = pin.filter(|p| p.electrical == PinType::PowerIn) {
            let net = if value.is_empty() { pin.name.clone() } else { value.clone() };
            if !net.is_empty() {
                return Some((
                    Item::PowerPort(PowerPort {
                        pos: placement.apply(pin.pos),
                        net,
                        rotation: placement.apply_angle(pin.rotation),
                        style: power_style(&cp.symbol.name),
                    }),
                    None,
                ));
            }
        }
    }

    Some((Item::Part(Part { pos, rotation, mirror, lib_ref, unit, fields }), missing))
}

/// Power port look from the symbol name.
pub fn power_style(name: &str) -> PowerStyle {
    let n = name.to_ascii_uppercase();
    if n.contains("EARTH") || n == "PE" || n.starts_with("PE_") {
        PowerStyle::Earth
    } else if n.starts_with("GNDA") || n.starts_with("AGND") || n.contains("GNDREF") {
        PowerStyle::SignalGround
    } else if n.starts_with("GND") || n.starts_with("VSS") || n.ends_with("GND") {
        PowerStyle::Ground
    } else {
        PowerStyle::Bar
    }
}

/// A sheet symbol → (Sheet with a placeholder `file`, child file as written, sheet uuid).
fn read_sheet(l: &List, notes: &mut Notes) -> Option<(Sheet, String, String)> {
    let (x, y) = l.child("at")?.xy()?;
    let (w, h) = l.child("size")?.xy()?;
    // Normalize to a top-left corner with positive size (structured format Y-down).
    let (x0, y0) = (mm(x.min(x + w)), mm(y.min(y + h)));
    let (x1, y1) = (mm(x.max(x + w)), mm(y.max(y + h)));
    let prop = |names: &[&str]| l.children("property").find(|p| p.atom(1).is_some_and(|n| names.contains(&n))).and_then(|p| p.atom(2));
    let name = prop(&["Sheetname", "Sheet name"]).unwrap_or("").to_string();
    let Some(file) = prop(&["Sheetfile", "Sheet file"]).filter(|f| !f.is_empty()) else {
        notes.warn("sheet symbol without a file skipped", &name);
        return None;
    };
    let mut entries = Vec::new();
    for pin in l.children("pin") {
        let (Some(pname), Some(pat)) = (pin.atom(1), pin.child("at")) else { continue };
        let Some((px, py)) = pat.xy() else { continue };
        let (px, py) = (mm(px), mm(py));
        let dists: [(SheetSide, Coord); 4] = [
            (SheetSide::Left, (px - x0).abs()),
            (SheetSide::Right, (px - x1).abs()),
            (SheetSide::Top, (py - y0).abs()),
            (SheetSide::Bottom, (py - y1).abs()),
        ];
        let side = dists.iter().min_by_key(|(_, d)| *d).map(|(s, _)| *s).unwrap_or_default();
        let offset = match side {
            SheetSide::Left | SheetSide::Right => py - y0,
            SheetSide::Top | SheetSide::Bottom => px - x0,
        };
        entries.push(SheetEntry { name: unescape(pname), side, offset, shape: port_shape(pin.atom(2)) });
    }
    let uuid = l.child_str("uuid").unwrap_or("").to_string();
    let sheet = Sheet { pos: Point::new(x0, -y0), size: Point::new(x1 - x0, y1 - y0), name, file: String::new(), entries };
    Some((sheet, file.to_string(), uuid))
}
