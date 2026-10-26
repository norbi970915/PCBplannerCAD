//! XML `.brd` → one PCBPlannerCAD PCB document.
//!
//! Board settings come from `<designrules>` and `<classes>`, footprints from
//! the embedded `<libraries>`, placed footprints from `<elements>` and copper
//! from `<signals>`. XML mirrors bottom-side elements *after* rotating
//! (`MR90`), so a bottom part gets the core rotation `-90°`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use pcbplannercad_core::item::{Item, Polyline};
use pcbplannercad_core::layer::{self, inner_copper};
use pcbplannercad_core::pcb::{ArcTrack, BoardSettings, NetClass, PlacedFootprint, RuleKind, Side, Stackup, Track, Via, Zone, ZoneConnect};
use pcbplannercad_core::units::{Coord, Point, mm};
use pcbplannercad_core::{DocKind, Document, LayerId, Library};
use roxmltree::Node;

use super::library::unique_name;
use super::package::{PackageNotes, PadRules, convert_package};
use super::util::{
    LayerSkips, Rot, attr, attr_str, board_layer, child, children, coord, elements, grandchildren, graphic, is_yes, layer_num, num,
    num_or, parse_xml, point, polygon_points, text_item, wire_item,
};
use crate::{BoardImport, ImportError, ImportReport};

// ---------- Design rules ----------

/// Parses an XML length (`0.15mm`, `8mil`, `10mic`, `0.1inch`) into mm.
/// Unitless values are taken as mm.
pub(crate) fn parse_len(s: &str) -> Option<f64> {
    let s = s.trim();
    let units = [("mm", 1.0), ("mil", 0.0254), ("mic", 0.001), ("inch", 25.4), ("in", 25.4)];
    let (num, factor) = units.iter().find_map(|(u, f)| s.strip_suffix(u).map(|n| (n, *f))).unwrap_or((s, 1.0));
    num.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v * factor).clamp(-1.0e6, 1.0e6))
}

struct DesignRules(HashMap<String, String>);

impl DesignRules {
    fn from(n: Option<Node<'_, '_>>) -> Self {
        DesignRules(n.map(|d| children(d, "param").map(|p| (attr_str(p, "name"), attr_str(p, "value"))).collect()).unwrap_or_default())
    }

    fn raw(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// First length of a (possibly space-separated) parameter, in mm.
    fn len_mm(&self, name: &str) -> Option<f64> {
        self.raw(name)?.split_whitespace().next().and_then(parse_len)
    }

    fn len(&self, name: &str) -> Option<Coord> {
        self.len_mm(name).map(mm)
    }

    fn ratio(&self, name: &str) -> Option<f64> {
        self.raw(name)?.trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn pad_rules(&self) -> PadRules {
        PadRules {
            rv: self.ratio("rvPadTop").unwrap_or(0.25).clamp(0.0, 10.0),
            min_mm: self.len_mm("rlMinPadTop").unwrap_or(0.254).max(0.0),
            max_mm: self.len_mm("rlMaxPadTop").unwrap_or(0.508).max(0.0),
            board: true,
            elongation: self.ratio("psElongationLong").unwrap_or(100.0).clamp(0.0, 1000.0),
        }
    }

    fn via_rules(&self) -> PadRules {
        PadRules {
            rv: self.ratio("rvViaOuter").unwrap_or(0.25).clamp(0.0, 10.0),
            min_mm: self.len_mm("rlMinViaOuter").unwrap_or(0.2032).max(0.0),
            max_mm: self.len_mm("rlMaxViaOuter").unwrap_or(0.508).max(0.0),
            board: true,
            elongation: 100.0,
        }
    }

    /// Smallest copper-to-copper distance rule.
    fn clearance(&self) -> Option<Coord> {
        ["mdWireWire", "mdWirePad", "mdWireVia", "mdPadPad", "mdPadVia", "mdViaVia", "mdSmdPad", "mdSmdVia", "mdSmdSmd"]
            .iter()
            .filter_map(|n| self.len(n))
            .filter(|v| *v > 0)
            .min()
    }
}

// ---------- Copper layers ----------

/// XML copper layer numbers (1 = top, 2–15 inner, 16 = bottom) → PCBPlannerCAD layers.
pub(crate) struct CopperMap(BTreeMap<u32, LayerId>);

impl CopperMap {
    pub(crate) fn new(used: &BTreeSet<u32>) -> Self {
        let mut map = BTreeMap::from([(1, layer::TOP_COPPER), (16, layer::BOTTOM_COPPER)]);
        for (i, n) in used.iter().filter(|n| (2..=15).contains(*n)).enumerate() {
            map.insert(*n, inner_copper(i as u16 + 1));
        }
        CopperMap(map)
    }

    pub(crate) fn get(&self, n: u32) -> Option<LayerId> {
        self.0.get(&n).copied()
    }

    pub(crate) fn count(&self) -> u16 {
        self.0.len() as u16
    }
}

/// Layer numbers in a `layerSetup` string such as `(1*2*15*16)` or `[1:1+2*15:16]`.
pub(crate) fn setup_layers(s: &str) -> BTreeSet<u32> {
    s.split(|c: char| !c.is_ascii_digit()).filter_map(|t| t.parse::<u32>().ok()).filter(|n| (1..=16).contains(n)).collect()
}

pub(crate) fn via_extent(s: Option<&str>) -> (u32, u32) {
    let mut it = s.unwrap_or("1-16").split('-').filter_map(|t| t.trim().parse::<u32>().ok());
    match (it.next(), it.next()) {
        (Some(a), Some(b)) => (a.min(b), a.max(b)),
        _ => (1, 16),
    }
}

fn used_copper(board: Node<'_, '_>, rules: &DesignRules) -> BTreeSet<u32> {
    let mut used = rules.raw("layerSetup").map(setup_layers).unwrap_or_default();
    for n in board.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "wire" | "polygon" | "rectangle" | "circle" | "text" if n.ancestors().all(|a| !a.has_tag_name("libraries")) => {
                if let Some(l) = layer_num(n).filter(|l| (1..=16).contains(l)) {
                    used.insert(l);
                }
            }
            "via" => {
                let (a, b) = via_extent(attr(n, "extent"));
                used.extend([a, b].into_iter().filter(|l| (1..=16).contains(l)));
            }
            _ => {}
        }
    }
    used
}

// ---------- Items ----------

#[derive(Default)]
struct Stats {
    tracks: usize,
    arcs: usize,
    vias: usize,
    zones: usize,
    footprints: usize,
    airwires: usize,
    zero_width: usize,
    holes: usize,
    dimensions: usize,
    smashed: usize,
    hatched: usize,
    fixed_vias: usize,
    missing_packages: BTreeSet<String>,
}

fn zone(n: Node<'_, '_>, layer: LayerId, net: &str, thermal_gap: Coord, stats: &mut Stats) -> Option<Zone> {
    let outline = polygon_points(n);
    if outline.len() < 3 {
        return None;
    }
    let pour = attr(n, "pour").unwrap_or("solid");
    if pour == "hatch" {
        stats.hatched += 1;
    }
    // XML rank 1 pours first (highest priority) … 6 last.
    let rank = num_or(n, "rank", 1.0).round().clamp(0.0, 7.0) as i32;
    let width = coord(n, "width").max(0);
    stats.zones += 1;
    Some(Zone {
        outline,
        layer,
        net: net.to_string(),
        name: net.to_string(),
        priority: 7 - rank,
        clearance: coord(n, "isolate").max(0),
        min_width: width,
        connect: if attr(n, "thermals") == Some("no") { ZoneConnect::Solid } else { ZoneConnect::Thermal },
        thermal_gap,
        spoke_width: width,
        keepout: pour == "cutout",
        remove_islands: !is_yes(n, "orphans"),
        fill: Vec::new(),
    })
}

/// A copper wire as a straight or curved track.
fn track(n: Node<'_, '_>, layer: LayerId, net: &str, stats: &mut Stats) -> Option<Item> {
    let width = coord(n, "width");
    if width <= 0 {
        stats.zero_width += 1;
        return None;
    }
    let curve = num_or(n, "curve", 0.0);
    let (a, b) = (super::util::fpoint(n, "x1", "y1"), super::util::fpoint(n, "x2", "y2"));
    match wire_item(a, b, curve, width, layer)? {
        Item::Line(l) => {
            stats.tracks += 1;
            Some(Item::Track(Track { a: l.a, b: l.b, width, layer, net: net.to_string() }))
        }
        Item::Arc(arc) => {
            stats.arcs += 1;
            Some(Item::ArcTrack(ArcTrack {
                center: arc.center,
                radius: arc.radius,
                start_angle: arc.start_angle,
                sweep: arc.sweep,
                width,
                layer,
                net: net.to_string(),
            }))
        }
        _ => None,
    }
}

fn rect_outline(n: Node<'_, '_>) -> Vec<Point> {
    let (a, b) = (point(n, "x1", "y1"), point(n, "x2", "y2"));
    let pts = vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
    let rot = Rot::of(n);
    if rot.angle == 0.0 {
        return pts;
    }
    let c = Point::new((a.x + b.x) / 2, (a.y + b.y) / 2);
    pts.into_iter().map(|p| pcbplannercad_core::geom::rotate(p, c, rot.angle)).collect()
}

struct Ctx<'r> {
    copper: CopperMap,
    thermal_gap: Coord,
    via_rules: PadRules,
    skips: LayerSkips,
    stats: Stats,
    report: &'r mut ImportReport,
}

impl Ctx<'_> {
    /// Graphics, copper and keep-outs of `<plain>` or a `<signal>` (`net` set).
    fn drawing_item(&mut self, e: Node<'_, '_>, net: &str, out: &mut Vec<Item>) {
        let tag = e.tag_name().name();
        let ln = layer_num(e);
        match tag {
            "via" => {
                let drill = num(e, "drill").unwrap_or(0.0);
                if drill <= 0.0 {
                    self.stats.zero_width += 1;
                    return;
                }
                let mut diameter = mm(self.via_rules.diameter(drill, num(e, "diameter")));
                let drill = mm(drill);
                if diameter <= drill {
                    diameter = drill + mm(0.1);
                    self.stats.fixed_vias += 1;
                }
                let (a, b) = via_extent(attr(e, "extent"));
                self.stats.vias += 1;
                out.push(Item::Via(Via {
                    pos: point(e, "x", "y"),
                    diameter,
                    drill,
                    from_layer: self.copper.get(a).unwrap_or(layer::TOP_COPPER),
                    to_layer: self.copper.get(b).unwrap_or(layer::BOTTOM_COPPER),
                    net: net.to_string(),
                }));
            }
            "hole" => self.stats.holes += 1,
            "dimension" => self.stats.dimensions += 1,
            "wire" | "polygon" | "rectangle" | "circle" | "text" | "frame" => {
                if ln == Some(19) {
                    self.stats.airwires += 1;
                    return;
                }
                if let Some(cu) = ln.and_then(|l| self.copper.get(l)) {
                    match tag {
                        "wire" => out.extend(track(e, cu, net, &mut self.stats)),
                        "polygon" => out.extend(zone(e, cu, net, self.thermal_gap, &mut self.stats).map(Item::Zone)),
                        "text" => out.extend(text_item(e, e.text().unwrap_or(""), cu, false)),
                        _ => out.extend(graphic(e, cu).unwrap_or_default()),
                    }
                    return;
                }
                // tRestrict / bRestrict areas become copper keep-out zones.
                if let (Some(l @ (41 | 42)), "polygon" | "rectangle") = (ln, tag) {
                    let outline = if tag == "polygon" { polygon_points(e) } else { rect_outline(e) };
                    if outline.len() >= 3 {
                        let cu = if l == 41 { layer::TOP_COPPER } else { layer::BOTTOM_COPPER };
                        out.push(Item::Zone(Zone {
                            outline,
                            layer: cu,
                            net: String::new(),
                            name: format!("Restrict {}", if l == 41 { "Top" } else { "Bottom" }),
                            priority: 0,
                            clearance: 0,
                            min_width: 0,
                            connect: ZoneConnect::None,
                            thermal_gap: 0,
                            spoke_width: 0,
                            keepout: true,
                            remove_islands: false,
                            fill: Vec::new(),
                        }));
                        self.stats.zones += 1;
                    }
                    return;
                }
                let Some(layer) = ln.and_then(board_layer) else {
                    self.skips.add(ln);
                    return;
                };
                match tag {
                    "text" => out.extend(text_item(e, e.text().unwrap_or(""), layer, false)),
                    "polygon" => {
                        let pts = polygon_points(e);
                        if pts.len() >= 2 {
                            let fill = if layer == layer::BOARD_OUTLINE { pcbplannercad_core::item::FillStyle::None } else { pcbplannercad_core::item::FillStyle::Solid };
                            out.push(Item::Polyline(Polyline { points: pts, closed: true, width: coord(e, "width").max(0), layer, fill }));
                        }
                    }
                    _ => out.extend(graphic(e, layer).unwrap_or_default()),
                }
            }
            _ => {}
        }
    }
}

fn element(e: Node<'_, '_>, footprint: String, pad_nets: BTreeMap<String, String>, stats: &mut Stats) -> PlacedFootprint {
    let rot = Rot::of(e);
    let pl = rot.placement(point(e, "x", "y"));
    let mut parameters = BTreeMap::new();
    for a in children(e, "attribute") {
        let name = attr_str(a, "name");
        match attr(a, "value") {
            Some(v) if !name.is_empty() && !name.eq_ignore_ascii_case("NAME") && !name.eq_ignore_ascii_case("VALUE") => {
                parameters.insert(name, v.to_string());
            }
            _ => {}
        }
    }
    if is_yes(e, "smashed") && children(e, "attribute").any(|a| attr(a, "x").is_some()) {
        stats.smashed += 1;
    }
    PlacedFootprint {
        designator: attr_str(e, "name"),
        value: attr_str(e, "value"),
        footprint,
        pos: pl.offset,
        rotation: pl.rotation,
        side: if rot.mirror { Side::Bottom } else { Side::Top },
        locked: is_yes(e, "locked"),
        pad_nets,
        component: String::new(),
        parameters,
    }
}

fn board_settings(rules: &DesignRules, classes: Option<Node<'_, '_>>, signals: &[(String, String)], copper: &CopperMap, report: &mut ImportReport, ctx: &str) -> BoardSettings {
    let mut b = BoardSettings { stackup: Stackup::with_copper(copper.count().max(2)), ..BoardSettings::default() };
    let via_rules = rules.via_rules();
    let min_drill = rules.len("msDrill").filter(|d| *d > 0);
    let min_width = rules.len("msWidth").filter(|d| *d > 0);
    let clearance = rules.clearance();
    let class0 = classes.and_then(|c| children(c, "class").find(|k| attr(*k, "number") == Some("0")));
    let default_width = class0.map(|k| coord(k, "width")).filter(|w| *w > 0);
    let mask = rules.len("mlMinStopFrame");
    if rules.len("mlMinStopFrame") != rules.len("mlMaxStopFrame") {
        report.info(ctx, "solder mask expansion uses the minimum of XML's stop-frame range");
    }
    for r in &mut b.rules {
        match &mut r.kind {
            RuleKind::Clearance { min, .. } => {
                if let Some(v) = clearance {
                    *min = v;
                }
            }
            RuleKind::Width { min, preferred, max, .. } => {
                if let Some(v) = min_width {
                    *min = v;
                }
                *preferred = default_width.unwrap_or(*preferred).max(*min);
                *max = (*max).max(*preferred);
            }
            RuleKind::ViaStyle { diameter, drill, min_annular_ring, .. } => {
                let d = class0.and_then(|k| num(k, "drill")).filter(|d| *d > 0.0).or(min_drill.map(|d| d as f64 / 1e6));
                if let Some(d) = d {
                    *drill = mm(d);
                    *diameter = mm(via_rules.diameter(d, None));
                }
                if let Some(v) = rules.len("rlMinViaOuter") {
                    *min_annular_ring = v;
                }
            }
            RuleKind::HoleSize { min, .. } => {
                if let Some(v) = min_drill {
                    *min = v;
                }
            }
            RuleKind::BoardOutlineClearance { min } => {
                if let Some(v) = rules.len("mdCopperDimension") {
                    *min = v.max(0);
                }
            }
            RuleKind::SolderMaskExpansion { expansion, .. } => {
                if let Some(v) = mask {
                    *expansion = v;
                }
            }
            RuleKind::ZoneConnect { gap, .. } => {
                if let Some(v) = rules.len("slThermalIsolate") {
                    *gap = v.max(0);
                }
            }
            _ => {}
        }
    }

    // Net classes.
    let rule_clearance = clearance.unwrap_or(mm(0.2));
    let mut out: Vec<NetClass> = Vec::new();
    for k in classes.into_iter().flat_map(|c| children(c, "class")) {
        let number = attr_str(k, "number");
        let name = match attr_str(k, "name") {
            n if n.is_empty() || (number == "0" && n.eq_ignore_ascii_case("default")) => {
                if number == "0" { "Default".to_string() } else { format!("Class{number}") }
            }
            n => n,
        };
        let self_clearance = children(k, "clearance")
            .find(|c| attr(*c, "class") == Some(number.as_str()))
            .map(|c| coord(c, "value"))
            .filter(|v| *v > 0);
        let drill = num(k, "drill").filter(|d| *d > 0.0).unwrap_or(min_drill.map_or(0.3, |d| d as f64 / 1e6));
        let width = coord(k, "width");
        let class = NetClass {
            name,
            nets: signals.iter().filter(|(_, c)| *c == number).map(|(n, _)| n.clone()).collect(),
            track_width: if width > 0 { width } else { min_width.unwrap_or(mm(0.25)).max(mm(0.1)) },
            clearance: self_clearance.unwrap_or(rule_clearance),
            via_diameter: mm(via_rules.diameter(drill, None)),
            via_drill: mm(drill),
            diff_pair_width: mm(0.2),
            diff_pair_gap: mm(0.2),
        };
        if number == "0" { out.insert(0, class) } else { out.push(class) }
    }
    if !out.is_empty() {
        b.net_classes = out;
    }
    b
}

pub(crate) fn import_board_text(text: &str, stem: &str) -> Result<BoardImport, ImportError> {
    let doc = parse_xml(text)?;
    let root = doc.root_element();
    if root.tag_name().name() != "eagle" {
        return Err(ImportError::Invalid("not a supported XML design file".into()));
    }
    let board = child(root, "drawing")
        .and_then(|d| child(d, "board"))
        .ok_or_else(|| ImportError::Invalid("not an XML board (no <board> element)".into()))?;
    let ctx_name = format!("{stem}.brd");
    let mut report = ImportReport::default();
    let rules = DesignRules::from(child(board, "designrules"));
    let pad_rules = rules.pad_rules();
    for p in ["psTop", "psBottom", "psFirst"] {
        if rules.ratio(p).is_some_and(|v| v >= 0.0) {
            report.warn(&ctx_name, format!("design rule {p} (pad shape override) ignored; pads keep their library shapes"));
        }
    }
    let copper = CopperMap::new(&used_copper(board, &rules));

    // Libraries → footprints.
    let mut libraries: Vec<Library> = Vec::new();
    let mut lib_keys: HashMap<(String, String), usize> = HashMap::new();
    let mut lib_names = HashSet::new();
    let mut notes = PackageNotes::default();
    let mut skips = LayerSkips::default();
    for lib in grandchildren(board, "libraries", "library") {
        let ename = attr_str(lib, "name");
        let name = unique_name(if ename.is_empty() { "library".into() } else { ename.clone() }, &mut lib_names);
        let mut l = Library { name: name.clone(), ..Library::default() };
        for p in grandchildren(lib, "packages", "package") {
            let fp = convert_package(p, &mut skips, &mut notes, &pad_rules);
            if l.footprint(&fp.name).is_none() {
                l.footprints.push(fp);
            }
        }
        lib_keys.entry((ename, attr_str(lib, "urn"))).or_insert(libraries.len());
        libraries.push(l);
    }
    notes.report(&ctx_name, &mut report);
    if let Some(s) = skips.summary() {
        report.info(&ctx_name, format!("footprints: {s}"));
    }
    let find_lib = |name: &str, urn: &str| -> Option<usize> {
        lib_keys.get(&(name.to_string(), urn.to_string())).copied().or_else(|| {
            let mut hits: Vec<usize> = lib_keys.iter().filter(|((n, _), _)| n == name).map(|(_, i)| *i).collect();
            hits.sort_unstable();
            hits.first().copied()
        })
    };

    // Signals: pad nets and classes.
    let signal_nodes: Vec<Node<'_, '_>> = grandchildren(board, "signals", "signal").collect();
    let mut pad_nets: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    let mut signal_classes: Vec<(String, String)> = Vec::new();
    for s in &signal_nodes {
        let net = attr_str(*s, "name");
        signal_classes.push((net.clone(), attr(*s, "class").unwrap_or("0").to_string()));
        for c in children(*s, "contactref") {
            pad_nets.entry(attr_str(c, "element")).or_default().insert(attr_str(c, "pad"), net.clone());
        }
    }

    let settings = board_settings(&rules, child(board, "classes"), &signal_classes, &copper, &mut report, &ctx_name);
    let thermal_gap = rules.len("slThermalIsolate").unwrap_or(0).max(0);
    let mut c = Ctx { copper, thermal_gap, via_rules: rules.via_rules(), skips: LayerSkips::default(), stats: Stats::default(), report: &mut report };
    let mut items: Vec<Item> = vec![Item::Board(settings)];

    // Plain drawing.
    if let Some(plain) = child(board, "plain") {
        for e in elements(plain) {
            c.drawing_item(e, "", &mut items);
        }
    }

    // Elements.
    let mut cache: BTreeMap<String, pcbplannercad_core::Footprint> = BTreeMap::new();
    for e in grandchildren(board, "elements", "element") {
        let name = attr_str(e, "name");
        let package = attr_str(e, "package");
        let lib = find_lib(attr(e, "library").unwrap_or(""), attr(e, "library_urn").unwrap_or(""));
        let Some(fp) = lib.and_then(|l| libraries[l].footprint(&package).map(|f| (l, f))) else {
            c.stats.missing_packages.insert(format!("{}/{package}", attr_str(e, "library")));
            continue;
        };
        let key = format!("{}/{}", libraries[fp.0].name, package);
        cache.entry(key.clone()).or_insert_with(|| fp.1.clone());
        let nets = pad_nets.remove(&name).unwrap_or_default();
        items.push(Item::Footprint(element(e, key, nets, &mut c.stats)));
        c.stats.footprints += 1;
    }

    // Signals: copper.
    for s in &signal_nodes {
        let net = attr_str(*s, "name");
        for e in elements(*s) {
            if e.tag_name().name() != "contactref" {
                c.drawing_item(e, &net, &mut items);
            }
        }
    }

    // Reports.
    let st = &c.stats;
    let r = &mut *c.report;
    if !st.missing_packages.is_empty() {
        let list: Vec<&str> = st.missing_packages.iter().map(String::as_str).collect();
        r.warn(&ctx_name, format!("elements with missing packages skipped: {}", list.join(", ")));
    }
    if st.airwires > 0 {
        r.info(&ctx_name, format!("{} airwire(s) (unrouted layer 19) skipped; the ratsnest is recomputed from pad nets", st.airwires));
    }
    if st.zero_width > 0 {
        r.warn(&ctx_name, format!("{} zero-width wire(s) or drill-less via(s) skipped", st.zero_width));
    }
    if st.holes > 0 {
        r.warn(&ctx_name, format!("{} free hole(s) skipped (no board-level hole item yet)", st.holes));
    }
    if st.dimensions > 0 {
        r.info(&ctx_name, format!("{} dimension(s) skipped", st.dimensions));
    }
    if st.smashed > 0 {
        r.info(&ctx_name, format!("{} smashed element(s): name/value text positions ignored, footprint texts used", st.smashed));
    }
    if st.hatched > 0 {
        r.info(&ctx_name, format!("{} hatched polygon(s) imported as solid pours", st.hatched));
    }
    if st.fixed_vias > 0 {
        r.warn(&ctx_name, format!("{} via(s) with diameter ≤ drill enlarged by 0.1 mm", st.fixed_vias));
    }
    if st.zones > 0 {
        r.info(&ctx_name, format!("{} polygon(s) imported as zones, unfilled: run Repour", st.zones));
    }
    if let Some(s) = c.skips.summary() {
        r.info(&ctx_name, s);
    }
    r.info(
        &ctx_name,
        format!(
            "imported {} copper layer(s), {} footprint(s), {} track(s), {} arc track(s), {} via(s), {} zone(s)",
            c.copper.count(),
            st.footprints,
            st.tracks,
            st.arcs,
            st.vias,
            st.zones
        ),
    );

    let document = Document::from_item_list(DocKind::Pcb, items).with_footprints(cache);
    Ok(BoardImport { file_name: format!("{stem}.arpcb"), document, libraries, report })
}
