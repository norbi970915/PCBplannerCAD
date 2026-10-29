//! `.kicad_pcb` boards (structured format 6–10).
//!
//! Board coordinates are Y-down and get Y negated. Footprints embedded in the
//! board are converted back to top-side, footprint-local library geometry
//! (see [`Frame::placed`]) and cached in the document; the placed instance
//! carries position, rotation, side and pad nets.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use pcbplannercad_core::item::{DESIGNATOR_TOKEN, Item, VALUE_TOKEN};
use pcbplannercad_core::layer;
use pcbplannercad_core::pcb::board::{DiffPair, NetClass, RuleKind, StackKind, StackLayer, Stackup};
use pcbplannercad_core::pcb::{ArcTrack, BoardSettings, PlacedFootprint, Side, Track, Via, Zone, ZoneConnect, ZonePolygon};
use pcbplannercad_core::pcb::fill;
use pcbplannercad_core::units::{Coord, mm};
use pcbplannercad_core::{DocKind, Document, Footprint, LayerId, Library};

use super::common::{Notes, YAxis, arc_from_3pts, unescape};
use super::footprint::{Frame, map_layer, read_footprint, read_graphic, read_text};
use super::json::{self, Json};
use super::sexpr::{List, Sexpr, parse};
use crate::BoardImport;
use crate::report::{ImportError, ImportReport};

const Y: YAxis = YAxis::Down;

/// Imports a `.kicad_pcb`. Design rules and net classes are read from the
/// `.kicad_pro` next to it when present.
pub fn import_board(path: &Path) -> Result<BoardImport, ImportError> {
    let text = std::fs::read_to_string(path)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok().and_then(|t| json::parse(&t));
    let mut imported=convert(&text, &stem, pro.as_ref())?;
    if path.with_extension("kicad_dru").is_file(){imported.report.warn("board rules", "External custom rule file was not translated; recreate length, skew and scoped constraints before validation.");}
    Ok(imported)
}

/// Parses `.kicad_pcb` text (without project-file rules).
pub fn parse_board(text: &str, file_stem: &str) -> Result<BoardImport, ImportError> {
    convert(text, file_stem, None)
}

fn convert(text: &str, stem: &str, pro: Option<&Json>) -> Result<BoardImport, ImportError> {
    let root = parse(text)?;
    if root.head() != "kicad_pcb" {
        return Err(ImportError::Invalid(format!("expected a kicad_pcb, found '{}'", root.head())));
    }
    let mut notes = Notes::default();

    // structured format 6–8 number nets; structured format 9+ may write names directly.
    let net_table: HashMap<String, String> =
        root.children("net").filter_map(|n| Some((n.atom(1)?.to_string(), board_net(n.atom(2).unwrap_or(""))))).collect();

    let copper_count = root
        .child("layers")
        .map(|ls| ls.lists().filter(|l| l.atom(1).is_some_and(|n| n.ends_with(".Cu"))).count())
        .unwrap_or(2)
        .clamp(1, 32) as u16;

    let mut settings = BoardSettings::default();
    let setup = root.child("setup");
    settings.stackup = setup
        .and_then(|s| s.child("stackup"))
        .and_then(|st| read_stackup(st, &mut settings))
        .unwrap_or_else(|| Stackup::with_copper(copper_count));
    if let Some(o) = setup.and_then(|s| s.child("aux_axis_origin")).and_then(|o| Y.pt_of(o)) {
        settings.origin = o;
    }
    let copper = settings.stackup.copper_layers();

    let source_nets = source_net_names(&root);
    let mut net_names: Vec<String> = source_nets.values().cloned().collect();
    let mut graphics = Vec::new();
    let mut placed = Vec::new();
    let mut copper_items = Vec::new();
    let mut cache = FootprintCache::default();
    for l in root.lists() {
        match l.head() {
            "footprint" | "module" => {
                if let Some(fp) = read_placed(l, &net_table, &mut cache, &mut notes) {
                    net_names.extend(fp.pad_nets.values().cloned());
                    placed.push(Item::Footprint(fp));
                }
            }
            "segment" => {
                let (Some(a), Some(b)) = (Y.child_pt(l, "start"), Y.child_pt(l, "end")) else { continue };
                let Some(layer) = copper_layer(l.child_str("layer").unwrap_or("")) else {
                    notes.info("tracks on non-copper layers skipped", "");
                    continue;
                };
                let width = mm(l.child_num("width").unwrap_or(0.0));
                if a == b || width <= 0 {
                    notes.info("zero-length or zero-width tracks skipped", "");
                    continue;
                }
                copper_items.push(Item::Track(Track { locked: false, a, b, width, layer, net: net_of(l, &net_table) }));
            }
            "arc" => {
                let (Some(s), Some(m), Some(e)) = (Y.child_pt(l, "start"), Y.child_pt(l, "mid"), Y.child_pt(l, "end")) else { continue };
                let Some(layer) = copper_layer(l.child_str("layer").unwrap_or("")) else { continue };
                let width = mm(l.child_num("width").unwrap_or(0.0));
                match arc_from_3pts(s, m, e) {
                    Some(a) if width > 0 => copper_items.push(Item::ArcTrack(ArcTrack { locked: false,
                        center: a.center,
                        radius: a.radius,
                        start_angle: a.start_angle,
                        sweep: a.sweep,
                        width,
                        layer,
                        net: net_of(l, &net_table),
                    })),
                    // Collinear "arcs" are straight tracks.
                    None if width > 0 && s != e => {
                        copper_items.push(Item::Track(Track { locked: false, a: s, b: e, width, layer, net: net_of(l, &net_table) }))
                    }
                    _ => notes.info("degenerate arc tracks skipped", ""),
                }
            }
            "via" => copper_items.extend(read_via(l, &net_table, &copper, &mut notes)),
            "zone" => copper_items.extend(read_zone(l, &net_table, &copper, &mut notes)),
            "gr_line" | "gr_rect" | "gr_circle" | "gr_arc" | "gr_poly" | "gr_curve" => {
                if let Some(layer) = map_layer(l.child_str("layer").unwrap_or(""), &mut notes) {
                    graphics.extend(read_graphic(l, layer, Y, &mut notes));
                }
            }
            "gr_text" => graphics.extend(read_text(l, &Frame::LIBRARY, &mut notes)),
            "dimension" => graphics.extend(read_dimension(l, &mut notes)),
            "gr_text_box" => notes.info("text boxes skipped", ""),
            "image" | "gr_image" => notes.info("images skipped", ""),
            "table" => notes.info("tables skipped", ""),
            "target" => notes.info("alignment targets skipped", ""),
            _ => {}
        }
    }

    net_names.sort();
    net_names.dedup();
    if let Some(pro) = pro {
        apply_project_rules(&mut settings, pro, &source_nets);
    } else {
        apply_board_net_classes(&root, &mut settings);
    }

    settings.diff_pairs = infer_pairs(&net_names);
    if !settings.diff_pairs.is_empty(){notes.info("differential pairs inferred from matching net suffixes; verify polarity and constraints", "");}
    let mut items = vec![Item::Board(settings)];
    items.extend(graphics);
    items.extend(placed);
    items.extend(copper_items);
    let mut doc = Document::from_item_list(DocKind::Pcb, items);
    let mut libs: BTreeMap<String, Library> = BTreeMap::new();
    for (key, fp) in cache.into_entries() {
        let (lib, _) = key.split_once(':').unwrap_or(("board", key.as_str()));
        let library = libs.entry(lib.to_string()).or_insert_with(|| Library { name: lib.to_string(), ..Default::default() });
        library.footprints.push(fp.clone());
        doc.put_cached_footprint(key, fp);
    }

    let file_name = format!("{stem}.arpcb");
    let mut report = ImportReport::default();
    notes.flush(&mut report, &format!("board {stem}.kicad_pcb"));
    Ok(BoardImport { file_name, document: doc, libraries: libs.into_values().collect(), report })
}

/// Retain source paths for rule matching before normalizing names for connectivity.
/// Inline named nets and zone names also occur in files without a root net table.
fn source_net_names(root: &List) -> BTreeMap<String,String> {
    let mut names=BTreeMap::new();
    let mut pending=vec![root];
    while let Some(node)=pending.pop() {
        let name=match node.head() {
            "net_name"=>node.atom(1),
            "net"=>node.atom(2).or_else(||match node.items.get(1) {Some(Sexpr::Atom(atom)) if atom.quoted=>Some(atom.text.as_ref()),_=>None}),
            _=>None,
        };
        if let Some(name)=name {
            let net=board_net(name);
            if !net.is_empty(){names.insert(unescape(name),net);}
        }
        pending.extend(node.lists());
    }
    names
}

/// Matching complete suffix pairs carry the source format's implicit pair convention.
fn infer_pairs(nets:&[String])->Vec<DiffPair> {
    let available:HashSet<_>=nets.iter().map(String::as_str).collect();
    let mut paired=HashSet::new();
    let mut out=vec![];
    for net in nets {
        for (positive,negative) in [("_DP","_DN"),("_P","_N"),("+","-")] {
            if let Some(base)=net.strip_suffix(positive) {
                let other=format!("{base}{negative}");
                if available.contains(other.as_str())&&!paired.contains(net) {
                    paired.insert(net.clone());paired.insert(other.clone());
                    out.push(DiffPair{name:base.to_string(),positive:net.clone(),negative:other});
                }
            }
        }
    }
    out
}

/// Net name of a node: `(net_name "x")` (zones, structured format 6–8), `(net n "x")`,
/// `(net n)` via the board's net table, or `(net "x")` (structured format 9+).
fn net_of(node: &List, table: &HashMap<String, String>) -> String {
    if let Some(n) = node.child_str("net_name") {
        return board_net(n);
    }
    let Some(net) = node.child("net") else { return String::new() };
    if let Some(name) = net.atom(2) {
        return board_net(name);
    }
    match net.items.get(1) {
        Some(Sexpr::Atom(a)) if !a.quoted => table.get(a.text.as_ref()).cloned().unwrap_or_default(),
        Some(Sexpr::Atom(a)) => board_net(&a.text),
        _ => String::new(),
    }
}

/// structured format board net names → schematic net names: single-pin
/// `unconnected-(…)` nets mean no net, and root-sheet local nets drop the
/// leading `/` sheet path so they match the compiled schematic.
fn board_net(name: &str) -> String {
    if name.starts_with("unconnected-(") {
        return String::new();
    }
    // Check the sheet path before unescaping: `{slash}` is part of the name.
    let name = match name.strip_prefix('/') {
        Some(rest) if !rest.is_empty() && !rest.contains('/') => rest,
        _ => name,
    };
    unescape(name)
}

fn copper_layer(name: &str) -> Option<LayerId> {
    match name {
        "F.Cu" => Some(layer::TOP_COPPER),
        "B.Cu" => Some(layer::BOTTOM_COPPER),
        n if n.starts_with("In") && n.ends_with(".Cu") => {
            n[2..n.len() - 3].parse::<u16>().ok().filter(|k| (1..=30).contains(k)).map(layer::inner_copper)
        }
        _ => None,
    }
}

/// Copper layers named by a `layers` list, expanding `*.Cu` / `F&B.Cu`.
fn copper_layers_of(names: &[&str], board_copper: &[LayerId]) -> Vec<LayerId> {
    let mut out = Vec::new();
    for n in names {
        match *n {
            "*.Cu" => out.extend_from_slice(board_copper),
            "F&B.Cu" => out.extend([layer::TOP_COPPER, layer::BOTTOM_COPPER]),
            n => out.extend(copper_layer(n)),
        }
    }
    out.dedup();
    out
}

fn atoms<'a>(l: Option<&'a List>) -> Vec<&'a str> {
    l.map(|l| l.items.iter().skip(1).filter_map(|s| s.as_atom()).collect()).unwrap_or_default()
}

// ---------- footprints ----------

/// Footprint geometry cache: one entry per distinct geometry of a `lib_id`
/// (`Lib:Name`, then `Lib:Name#2`, …).
#[derive(Default)]
struct FootprintCache {
    entries: Vec<(String, Footprint)>,
    by_lib_id: HashMap<String, Vec<usize>>,
}

impl FootprintCache {
    /// Geometry identity ignores reference/value text placement, which
    /// differs per instance.
    fn same_geometry(a: &Footprint, b: &Footprint) -> bool {
        let strip = |f: &Footprint| -> Vec<Item> {
            f.items.iter().filter(|i| !matches!(i, Item::Text(t) if t.text == DESIGNATOR_TOKEN || t.text == VALUE_TOKEN)).cloned().collect()
        };
        a.model == b.model && strip(a) == strip(b)
    }

    fn key_for(&mut self, lib_id: &str, mut fp: Footprint) -> String {
        let ids = self.by_lib_id.entry(lib_id.to_string()).or_default();
        if let Some(&i) = ids.iter().find(|&&i| Self::same_geometry(&self.entries[i].1, &fp)) {
            return self.entries[i].0.clone();
        }
        let key = if ids.is_empty() { lib_id.to_string() } else { format!("{lib_id}#{}", ids.len() + 1) };
        if !ids.is_empty() {
            fp.name = format!("{}#{}", fp.name, ids.len() + 1);
        }
        ids.push(self.entries.len());
        self.entries.push((key.clone(), fp));
        key
    }

    fn into_entries(self) -> Vec<(String, Footprint)> {
        self.entries
    }
}

fn read_placed(l: &List, nets: &HashMap<String, String>, cache: &mut FootprintCache, notes: &mut Notes) -> Option<PlacedFootprint> {
    let lib_id = l.atom(1).unwrap_or("");
    let at = l.child("at")?;
    let pos = Y.pt_of(at)?;
    let rot = at.num(3).unwrap_or(0.0);
    let bottom = l.child_str("layer") == Some("B.Cu");
    let name = lib_id.rsplit_once(':').map_or(lib_id, |(_, n)| n);
    let frame = Frame::placed(rot, bottom);
    let fp = read_footprint(l, name, &frame, notes);

    // Reference/value/properties: structured format 8+ `property`, structured format 6/7 `fp_text`.
    let mut designator = String::new();
    let mut value = String::new();
    let mut parameters = BTreeMap::new();
    for p in l.children("property") {
        let (Some(k), v) = (p.atom(1), p.atom(2).unwrap_or("")) else { continue };
        match k {
            "Reference" => designator = v.to_string(),
            "Value" => value = v.to_string(),
            "Footprint" => {}
            k if k.starts_with("ki_") || v.is_empty() => {}
            k => {
                parameters.insert(k.to_string(), v.to_string());
            }
        }
    }
    for t in l.children("fp_text") {
        match (t.atom(1), t.atom(2)) {
            (Some("reference"), Some(v)) if designator.is_empty() => designator = v.to_string(),
            (Some("value"), Some(v)) if value.is_empty() => value = v.to_string(),
            _ => {}
        }
    }
    let mut pad_nets = BTreeMap::new();
    for pad in l.children("pad") {
        let net = net_of(pad, nets);
        if let Some(num) = pad.atom(1).filter(|n| !n.is_empty())
            && !net.is_empty()
        {
            pad_nets.entry(num.to_string()).or_insert(net);
        }
    }
    if l.lists().any(|c| c.head() == "pad" && c.child("primitives").is_some()) {
        notes.info("custom pad outlines reduced to their anchor shape", &designator);
    }
    let attrs = l.child("attr");
    if attrs.is_some_and(|a| a.has_atom("exclude_from_bom") || a.has_atom("virtual")) {
        parameters.insert(pcbplannercad_core::item::FIELD_EXCLUDE_BOM.into(), "1".into());
    }
    if attrs.is_some_and(|a| a.has_atom("exclude_from_pos_files") || a.has_atom("virtual")) {
        parameters.insert(pcbplannercad_core::item::FIELD_EXCLUDE_POSITION.into(), "1".into());
    }
    if l.flag("dnp") { parameters.insert(pcbplannercad_core::item::FIELD_DNP.into(), "1".into()); }
    let graphic_only = !fp.items.iter().any(|item| matches!(item, Item::Pad(p) if !p.number.is_empty()));
    let board_only = attrs.is_some_and(|a| a.has_atom("board_only")
        || (a.has_atom("exclude_from_bom") && a.has_atom("exclude_from_pos_files") && graphic_only));
    if board_only { notes.info("independent board footprint retained during schematic updates", &designator); }
    let key = cache.key_for(if lib_id.is_empty() { "board:unnamed" } else { lib_id }, fp);
    Some(PlacedFootprint {
        board_only,
        designator,
        value,
        footprint: key,
        pos,
        rotation: Frame::placed_rotation(rot, bottom),
        side: if bottom { Side::Bottom } else { Side::Top },
        locked: l.flag("locked"),
        pad_nets,
        component: String::new(),
        parameters,
    })
}

// ---------- copper ----------

fn read_via(l: &List, nets: &HashMap<String, String>, copper: &[LayerId], notes: &mut Notes) -> Option<Item> {
    let pos = Y.child_pt(l, "at")?;
    let diameter = mm(l.child_num("size").unwrap_or(0.0));
    let drill = mm(l.child_num("drill").unwrap_or(0.0));
    if drill <= 0 || diameter <= drill {
        notes.warn("vias with invalid size/drill skipped", "");
        return None;
    }
    let span = copper_layers_of(&atoms(l.child("layers")), copper);
    let (from_layer, to_layer) = match span.as_slice() {
        [a, b, ..] => (*a, *b),
        _ => (layer::TOP_COPPER, layer::BOTTOM_COPPER),
    };
    if l.has_atom("micro") || l.has_atom("blind") || l.child_str("type").is_some_and(|t| t != "through") {
        notes.info("blind/buried/micro vias imported with their layer span", "");
    }
    Some(Item::Via(Via { locked: false, pos, diameter, drill, from_layer, to_layer, net: net_of(l, nets) }))
}

/// Aligned and orthogonal dimensions; structured format's `height` (Y down) is the negated offset here.
fn read_dimension(l: &List, notes: &mut Notes) -> Option<Item> {
    use pcbplannercad_core::pcb::dimension::{DimUnits, Dimension};
    let layer = map_layer(l.child_str("layer").unwrap_or(""), notes)?;
    let pts = Y.pts(l);
    let (Some(&a), Some(&b)) = (pts.first(), pts.get(1)) else {
        notes.info("dimensions without points skipped", "");
        return None;
    };
    let (orthogonal, angle) = match l.child_str("type").unwrap_or("aligned") {
        "aligned" => (false, 0.0),
        "orthogonal" => (true, if l.child_num("orientation").unwrap_or(0.0) as i64 == 1 { 90.0 } else { 0.0 }),
        other => {
            notes.info(&format!("{other} dimensions skipped"), "");
            return None;
        }
    };
    if a == b {
        return None;
    }
    let format = l.child("format");
    let units = match format.and_then(|f| f.child_num("units")).unwrap_or(2.0) as i64 {
        0 => DimUnits::Inch,
        1 => DimUnits::Mil,
        _ => DimUnits::Mm,
    };
    let size = l.child("gr_text").and_then(|t| t.child("effects")).and_then(|e| e.child("font")).and_then(|f| f.child("size")).and_then(|s| s.num(1));
    Some(Item::Dimension(Dimension {
        a,
        b,
        offset: -mm(l.child_num("height").unwrap_or(0.0)),
        orthogonal,
        angle,
        layer,
        width: mm(l.child("style").and_then(|s| s.child_num("thickness")).unwrap_or(0.15)),
        text_height: mm(size.unwrap_or(1.0)),
        units,
        precision: format.and_then(|f| f.child_num("precision")).unwrap_or(2.0).clamp(0.0, 4.0) as u8,
    }))
}

fn read_zone(l: &List, nets: &HashMap<String, String>, copper: &[LayerId], notes: &mut Notes) -> Vec<Item> {
    let mut names = atoms(l.child("layers"));
    names.extend(l.child_str("layer"));
    let layers = copper_layers_of(&names, copper);
    if layers.is_empty() {
        notes.info("zones on non-copper layers skipped", names.first().copied().unwrap_or(""));
        return Vec::new();
    }
    let Some(outline) = l.child("polygon").map(|p| Y.pts(p)).filter(|p| p.len() >= 3) else {
        notes.info("zones without an outline skipped", "");
        return Vec::new();
    };
    let keepout = match l.child("keepout") {
        Some(k) => {
            let any = ["tracks", "vias", "pads", "copperpour", "footprints"].iter().any(|r| k.child_str(r) == Some("not_allowed"));
            if !any {
                notes.info("rule areas without copper restrictions skipped", "");
                return Vec::new();
            }
            if ["tracks", "vias", "copperpour"].iter().any(|r| k.child_str(r) == Some("allowed")) {
                notes.info("partial keep-outs imported as full copper keep-outs", "");
            }
            true
        }
        None => false,
    };
    let (connect, clearance) = match l.child("connect_pads") {
        Some(c) => {
            let mode = match c.atom(1) {
                Some("yes") => ZoneConnect::Solid,
                Some("no") => ZoneConnect::None,
                Some("thru_hole_only") => {
                    notes.info("zone pad connection 'thru-hole only' imported as thermal relief", "");
                    ZoneConnect::Thermal
                }
                _ => ZoneConnect::Thermal,
            };
            (mode, mm(c.child_num("clearance").unwrap_or(0.0)))
        }
        None => (ZoneConnect::Thermal, 0),
    };
    let fill = l.child("fill");
    let fnum = |k: &str| fill.and_then(|f| f.child_num(k)).map(mm).unwrap_or(0);
    let remove_islands = fill.and_then(|f| f.child_num("island_removal_mode")).unwrap_or(0.0) as i64 != 1;
    let base = Zone { locked: false,
        outline,
        layer: layers[0],
        net: if keepout { String::new() } else { net_of(l, nets) },
        name: l.child_str("name").unwrap_or("").to_string(),
        priority: l.child_num("priority").unwrap_or(0.0) as i32,
        clearance,
        min_width: mm(l.child_num("min_thickness").unwrap_or(0.0)),
        connect,
        thermal_gap: fnum("thermal_gap"),
        spoke_width: fnum("thermal_bridge_width"),
        keepout,
        remove_islands,
        fill: Vec::new(),
        // structured format 7+ stores teardrops as zones tagged (attr (teardrop (type …))).
        teardrop: l.child("attr").and_then(|a| a.child("teardrop")).is_some(),
    };
    layers
        .iter()
        .map(|&layer| {
            let fill: Vec<ZonePolygon> = l
                .children("filled_polygon")
                .filter(|fp| fp.child_str("layer").and_then(copper_layer).unwrap_or(layers[0]) == layer)
                .map(|fp| ZonePolygon { outer: Y.pts(fp), holes: Vec::new() })
                .filter(|p| p.outer.len() >= 3)
                .collect();
            // structured format stores fills fractured (holes joined to the outline by slits).
            let fill = if fill::is_fractured(&fill) { fill::unfracture(&fill) } else { fill };
            Item::Zone(Zone { layer, fill, ..base.clone() })
        })
        .collect()
}

// ---------- settings ----------

fn read_stackup(st: &List, settings: &mut BoardSettings) -> Option<Stackup> {
    let mut layers = Vec::new();
    let mut dielectric = 0;
    for l in st.children("layer") {
        let name = l.atom(1).unwrap_or("");
        let ty = l.child_str("type").unwrap_or("");
        let thickness = mm(l.child_num("thickness").unwrap_or(0.0).max(0.0));
        let material = l.child_str("material").unwrap_or("").to_string();
        let epsilon_r = l.child_num("epsilon_r").unwrap_or(0.0);
        let top = name.starts_with("F.");
        let (kind, id, our_name) = if ty == "copper" {
            let id = copper_layer(name)?;
            let n = match name {
                "F.Cu" => "Top Layer".to_string(),
                "B.Cu" => "Bottom Layer".to_string(),
                _ => format!("Inner {}", id.0 - layer::INNER_COPPER_BASE),
            };
            (StackKind::Copper, Some(id), n)
        } else if ty == "core" || ty == "prepreg" || name.starts_with("dielectric") {
            dielectric += 1;
            (StackKind::Dielectric, None, format!("Dielectric {dielectric}"))
        } else if ty.contains("Solder Mask") {
            if let Some(c) = l.child_str("color").and_then(color_hex)
                && top
            {
                settings.mask_color = c.into();
            }
            (
                StackKind::SolderMask,
                Some(if top { layer::TOP_MASK } else { layer::BOTTOM_MASK }),
                if top { "Top Solder" } else { "Bottom Solder" }.into(),
            )
        } else if ty.contains("Silk Screen") {
            if let Some(c) = l.child_str("color").and_then(color_hex)
                && top
            {
                settings.silk_color = c.into();
            }
            (
                StackKind::Silkscreen,
                Some(if top { layer::TOP_SILK } else { layer::BOTTOM_SILK }),
                if top { "Top Overlay" } else { "Bottom Overlay" }.into(),
            )
        } else if ty.contains("Solder Paste") {
            (
                StackKind::Paste,
                Some(if top { layer::TOP_PASTE } else { layer::BOTTOM_PASTE }),
                if top { "Top Paste" } else { "Bottom Paste" }.into(),
            )
        } else {
            continue;
        };
        let material = if kind == StackKind::Copper && material.is_empty() { "Copper".to_string() } else { material };
        layers.push(StackLayer { name: our_name, kind, layer: id, thickness, material, epsilon_r });
    }
    let s = Stackup { layers };
    (!s.copper_layers().is_empty()).then_some(s)
}

fn color_hex(name: &str) -> Option<&'static str> {
    Some(match name {
        "Green" => "#1f5f2a",
        "Red" => "#a01818",
        "Blue" => "#1b3d8f",
        "Purple" => "#5a2d82",
        "Black" => "#141414",
        "White" => "#f2f2f2",
        "Yellow" => "#c8b400",
        _ => return None,
    })
}

/// Simple `*` / `?` wildcard match (structured format net class patterns).
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Design rules and net classes from the `.kicad_pro` (structured format 6–10 layouts).
fn apply_project_rules(settings: &mut BoardSettings, pro: &Json, source_nets: &BTreeMap<String,String>) {
    let normalize=|name:&str|source_nets.get(name).cloned().unwrap_or_else(||board_net(name));
    let rules = pro.path(&["board", "design_settings", "rules"]);
    let r = |k: &str| rules.and_then(|j| j.get(k)).and_then(Json::num).filter(|v| *v >= 0.0).map(mm);
    let ns = pro.get("net_settings");
    let classes_json = ns.and_then(|n| n.get("classes")).map(Json::arr).unwrap_or(&[]);

    let default = settings.net_classes.first().cloned();
    let mut classes: Vec<NetClass> = Vec::new();
    for c in classes_json {
        let Some(name) = c.get("name").and_then(Json::str) else { continue };
        let d = default.clone().unwrap_or_else(|| BoardSettings::default().net_classes[0].clone());
        let v = |k: &str, fallback: Coord| c.get(k).and_then(Json::num).map(mm).unwrap_or(fallback);
        let mut class = NetClass {
            name: name.to_string(),
            nets: Vec::new(),
            track_width: v("track_width", d.track_width),
            clearance: v("clearance", d.clearance),
            via_diameter: v("via_diameter", d.via_diameter),
            via_drill: v("via_drill", d.via_drill),
            diff_pair_width: v("diff_pair_width", d.diff_pair_width),
            diff_pair_gap: v("diff_pair_gap", d.diff_pair_gap),
        };
        // structured format 6/7: explicit member lists.
        class.nets.extend(c.get("nets").map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::str).map(normalize));
        classes.push(class);
    }
    if let Some(i) = classes.iter().position(|c| c.name == "Default") {
        let d = classes.remove(i);
        classes.insert(0, d);
    }
    // structured format 8+: explicit assignments and wildcard patterns.
    let assign = |net: &str, class: &str, classes: &mut Vec<NetClass>| {
        if let Some(c) = classes.iter_mut().find(|c| c.name == class && c.name != "Default")
            && !c.nets.iter().any(|n| n == net)
        {
            c.nets.push(net.to_string());
        }
    };
    for (net, class) in ns.and_then(|n| n.get("netclass_assignments")).map(Json::obj).unwrap_or(&[]) {
        match class {
            Json::Str(c) => assign(&normalize(net), c, &mut classes),
            Json::Arr(cs) => {
                if let Some(c) = cs.first().and_then(Json::str) {
                    assign(&normalize(net), c, &mut classes);
                }
            }
            _ => {}
        }
    }
    for p in ns.and_then(|n| n.get("netclass_patterns")).map(Json::arr).unwrap_or(&[]) {
        let (Some(class), Some(pattern)) = (p.get("netclass").and_then(Json::str), p.get("pattern").and_then(Json::str)) else { continue };
        for (source,net) in source_nets {
            // Match the original sheet-qualified name; store the connectivity name.
            // Preserve compatibility with existing patterns using normalized/short names.
            let short = net.rsplit('/').next().unwrap_or(net);
            if glob(pattern, source) || glob(pattern, net) || glob(pattern, short) {
                assign(net, class, &mut classes);
            }
        }
    }
    if !classes.is_empty() {
        settings.net_classes = classes;
    }
    let dc = settings.net_classes.first().cloned();

    for rule in &mut settings.rules {
        match &mut rule.kind {
            RuleKind::Clearance { min, .. } => {
                if let Some(v) = r("min_clearance").filter(|v| *v > 0).or(dc.as_ref().map(|c| c.clearance)) {
                    *min = v;
                }
            }
            RuleKind::Width { min, preferred, .. } => {
                if let Some(v) = r("min_track_width") {
                    *min = v;
                }
                if let Some(c) = &dc {
                    *preferred = c.track_width.max(*min);
                }
            }
            RuleKind::ViaStyle { diameter, drill, min_annular_ring, .. } => {
                if let Some(c) = &dc {
                    *diameter = c.via_diameter;
                    *drill = c.via_drill;
                } else if let Some(v) = r("min_via_diameter").filter(|v| *v > 0) {
                    *diameter = v;
                }
                if let Some(v) = r("min_via_annular_width").filter(|v| *v > 0) {
                    *min_annular_ring = v;
                }
            }
            RuleKind::HoleSize { min, .. } => {
                if let Some(v) = r("min_through_hole_diameter").filter(|v| *v > 0) {
                    *min = v;
                }
            }
            RuleKind::BoardOutlineClearance { min } => {
                if let Some(v) = r("min_copper_edge_clearance") {
                    *min = v;
                }
            }
            RuleKind::SilkClearance { min } => {
                if let Some(v) = r("min_silk_clearance") {
                    *min = v;
                }
            }
            RuleKind::DiffPair { width, gap, .. } => {
                if let Some(c) = &dc {
                    *width = c.diff_pair_width;
                    *gap = c.diff_pair_gap;
                }
            }
            _ => {}
        }
    }
}

/// structured format 6 boards may carry `(net_class "Name" "desc" (clearance ..) (add_net ..))`.
fn apply_board_net_classes(root: &List, settings: &mut BoardSettings) {
    let d = settings.net_classes[0].clone();
    let mut classes = Vec::new();
    for c in root.children("net_class") {
        let Some(name) = c.atom(1) else { continue };
        let v = |k: &str, fallback: Coord| c.child_num(k).map(mm).unwrap_or(fallback);
        classes.push(NetClass {
            name: name.to_string(),
            nets: if name == "Default" { Vec::new() } else { c.children("add_net").filter_map(|n| n.atom(1)).map(board_net).collect() },
            track_width: v("trace_width", d.track_width),
            clearance: v("clearance", d.clearance),
            via_diameter: v("via_dia", d.via_diameter),
            via_drill: v("via_drill", d.via_drill),
            diff_pair_width: v("diff_pair_width", d.diff_pair_width),
            diff_pair_gap: v("diff_pair_gap", d.diff_pair_gap),
        });
    }
    if let Some(i) = classes.iter().position(|c| c.name == "Default") {
        let d = classes.remove(i);
        classes.insert(0, d);
    }
    if !classes.is_empty() {
        settings.net_classes = classes;
    }
}

#[cfg(test)]
mod tests {
    use super::glob;

    #[test]
    fn structured_netclass_globs() {
        assert!(glob("+*V", "+12V"));
        assert!(glob("GND", "GND"));
        assert!(!glob("GND", "AGND"));
        assert!(glob("USB_D?", "USB_DP"));
        assert!(glob("*", ""));
        assert!(!glob("A*B", "AxxC"));
    }
}

#[cfg(test)]
mod high_speed_import_tests {
    use super::*;
    #[test]
    fn pairs_require_both_members_and_keep_polarity() {
        let nets:Vec<String>=["A_P","A_N","ORPHAN_P","USB_DP","USB_DN","D+","D-","POWER"].iter().map(|n|n.to_string()).collect();
        let pairs=infer_pairs(&nets);assert_eq!(pairs.len(),3);
        assert_eq!((&pairs[0].positive,&pairs[0].negative),(&"A_P".to_string(),&"A_N".to_string()));
        assert!(!pairs.iter().any(|p|p.positive.contains("ORPHAN")));
    }
    #[test]
    fn explicit_zero_width_minimum_is_not_replaced_by_default() {
        let pro=json::parse(r#"{"board":{"design_settings":{"rules":{"min_track_width":0}}}}"#).unwrap();
        let mut b=BoardSettings::default();apply_project_rules(&mut b,&pro,&BTreeMap::new());
        let min=b.rules.iter().find_map(|r|if let RuleKind::Width{min,..}=r.kind{Some(min)}else{None}).unwrap();
        assert_eq!(min,0);
    }
}

#[cfg(test)]
mod source_class_path_tests {
    use super::*;
    const BOARD:&str=r#"(kicad_pcb (version 20240108)
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
        (net 1 "/LINK_P") (net 2 "/LINK_N") (net 3 "LINK_GLOBAL")
        (segment (start 0 0) (end 8 0) (width 0.14) (layer "F.Cu") (net 1))
        (segment (start 0.12178 0.294) (end 7.87822 0.294) (width 0.14) (layer "F.Cu") (net 2)))"#;
    const CLASSES:&str=r#"{"net_settings":{"classes":[
        {"name":"Default","clearance":0.13,"diff_pair_gap":0.25},
        {"name":"Fast","track_width":0.19,"diff_pair_width":0.14,"diff_pair_gap":0.154}],
        "netclass_patterns":[{"netclass":"Fast","pattern":"/LINK*"}]}}"#;

    #[test]
    fn root_path_patterns_keep_the_source_class_and_route_spacing() {
        let imported=convert(BOARD,"pair",Some(&json::parse(CLASSES).unwrap())).unwrap();
        let board=pcbplannercad_core::pcb::geometry::board_settings(&imported.document).unwrap().1;
        let class=board.net_classes.iter().find(|c|c.name=="Fast").unwrap();
        assert_eq!(class.nets,vec!["LINK_N","LINK_P"]);assert!(!class.nets.iter().any(|n|n=="LINK_GLOBAL"));
        assert_eq!(class.diff_pair_gap,mm(0.154));assert_eq!(class.diff_pair_width,mm(0.14));
        let p=board.diff_pairs.iter().find(|p|p.positive=="LINK_P").unwrap();
        let req=pcbplannercad_core::pcb::router::RouteRequest{net:p.positive.clone(),layer:layer::TOP_COPPER,start:pcbplannercad_core::units::Point::new(0,0),end:pcbplannercad_core::units::Point::new(mm(8.),0),width:class.diff_pair_width,mode:pcbplannercad_core::pcb::router::RouteMode::Walkaround,corner:pcbplannercad_core::pcb::router::CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
        let (positive,negative)=pcbplannercad_core::pcb::router::route_diff_pair(&imported.document,&req,&p.negative,class.diff_pair_gap);
        assert!(positive.reached&&negative.reached,"{}",positive.message);
        assert_eq!(negative.tracks.first().unwrap().a.x,mm(0.12178));
        let text=pcbplannercad_core::io::document_to_string(&imported.document).unwrap();
        let reopened=pcbplannercad_core::io::document_from_str(DocKind::Pcb,&text).unwrap();
        assert_eq!(pcbplannercad_core::pcb::geometry::board_settings(&reopened).unwrap().1.net_classes,board.net_classes);
    }

    #[test]
    fn explicit_assignments_and_legacy_members_store_normalized_names() {
        for assignment in [r#""netclass_assignments":{"/LINK_P":"Fast","/LINK_N":["Fast"]}"#,r#""netclass_patterns":[{"pattern":"/LINK*","netclass":"Fast"}]"#] {
            let pro=json::parse(&format!(r#"{{"net_settings":{{"classes":[{{"name":"Default"}},{{"name":"Fast","nets":["/LINK_P"]}}],{assignment}}}}}"#)).unwrap();
            let imported=convert(BOARD,"explicit",Some(&pro)).unwrap();
            let board=pcbplannercad_core::pcb::geometry::board_settings(&imported.document).unwrap().1;
            let nets=&board.net_classes[1].nets;
            assert!(nets.iter().any(|n|n=="LINK_P")&&nets.iter().any(|n|n=="LINK_N"));assert!(nets.iter().all(|n|!n.starts_with('/')));
        }
    }

    #[test]
    fn inline_named_nets_keep_paths_and_escaped_name_characters_separate() {
        let root=parse(r#"(kicad_pcb
            (segment (net "/BUS{slash}P")) (segment (net "/Child/DATA"))
            (zone (net_name "GLOBAL")) (pad "1" (net 3 "/LOCAL"))
            (segment (net "unconnected-(U1-Pad1)")))"#).unwrap();
        let names=source_net_names(&root);
        assert_eq!(names.get("/BUS/P").map(String::as_str),Some("BUS/P"));
        assert_eq!(names.get("/Child/DATA").map(String::as_str),Some("/Child/DATA"));
        assert_eq!(names.get("/LOCAL").map(String::as_str),Some("LOCAL"));assert_eq!(names.len(),4);
        let pro=json::parse(r#"{"net_settings":{"classes":[{"name":"Default"},{"name":"Fast"}],"netclass_assignments":{"/BUS/P":"Fast"}}}"#).unwrap();
        let mut board=BoardSettings::default();apply_project_rules(&mut board,&pro,&names);
        assert_eq!(board.net_classes[1].nets,vec!["BUS/P"]);
    }
}
