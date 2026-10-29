//! Schematic → board engineering change orders (ECO).
//!
//! [`plan`] compares a compiled schematic with a board and lists the changes
//! needed to bring the board up to date; [`apply`] turns a selection of those
//! changes into one undoable [`Command`]. Parts are matched to placed
//! footprints by designator (all units of a multi-unit part share one
//! footprint).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::command::Command;
use crate::document::{Document, IdItem, ItemId};
use crate::geom::BBox;
use crate::item::{Item, Pad};
use crate::layer;
use crate::library::Footprint;
use crate::schematic::{Compiled, NetlistPart, natural_cmp};
use crate::units::{Coord, Point, mm};

use super::geometry::outline_loops;
use super::items::{PlacedFootprint, Side};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EcoChange {
    #[serde(rename_all = "camelCase")]
    AddFootprint {
        designator: String,
        value: String,
        footprint_key: String,
        component: String,
        pad_nets: BTreeMap<String, String>,
        parameters: BTreeMap<String, String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveFootprint { item: ItemId, designator: String },
    /// Swaps the footprint geometry; position, rotation and side are kept.
    #[serde(rename_all = "camelCase")]
    ChangeFootprint { item: ItemId, designator: String, from: String, to: String },
    #[serde(rename_all = "camelCase")]
    UpdateValue { item: ItemId, designator: String, from: String, to: String },
    /// `(pad, old net, new net)`.
    #[serde(rename_all = "camelCase")]
    UpdateNets { item: ItemId, designator: String, changes: Vec<(String, String, String)> },
    #[serde(rename_all = "camelCase")]
    UpdateParameters { item: ItemId, designator: String, component: String, parameters: BTreeMap<String, String> },
    /// Same pads, new name: pads, tracks, vias, zones and net classes follow.
    #[serde(rename_all = "camelCase")]
    RenameNet { from: String, to: String },
    #[serde(rename_all = "camelCase")]
    Warning { message: String },
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct EcoPlan {
    #[serde(skip)]
    source_board: Option<Document>,
    #[serde(skip)]
    source_netlist: serde_json::Value,
    pub changes: Vec<EcoChange>,
    /// Footprints the caller must cache (`put_cached_footprint`) before
    /// executing the command from [`apply`]: key → geometry.
    #[serde(skip)]
    pub footprints: Vec<(String, Footprint)>,
}

impl EcoPlan {
    /// Applying a reviewed list to a changed board or schematic requires a fresh review.
    pub fn is_current(&self, compiled: &Compiled, board: &Document) -> bool {
        self.source_board.as_ref() == Some(board)
            && serde_json::to_value(compiled).is_ok_and(|value| value == self.source_netlist)
    }
}

fn is_placeable(des: &str) -> bool {
    let d = des.trim();
    !d.is_empty() && !d.ends_with('?') && !d.starts_with('#')
}

fn pad_numbers(fp: &Footprint) -> BTreeSet<String> {
    fp.items.iter().filter_map(|i| if let Item::Pad(p) = i { Some(p.number.clone()) } else { None }).filter(|n| !n.is_empty()).collect()
}

/// Compares the schematic with the board. `resolve` maps a part to its
/// footprint cache key and geometry (`None` = not found in any library).
/// Library key equality that ignores a board-local variant suffix (`#2`, `#3`…).
fn same_footprint(a: &str, b: &str) -> bool {
    let base = |k: &str| match k.rsplit_once('#') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => head.to_string(),
        _ => k.to_string(),
    };
    let norm = |k: &str| base(k).replace('/', ":");
    a == b || norm(a) == norm(b)
}

/// Footprint name without library prefix or board-variant suffix.
fn footprint_name(key: &str) -> &str {
    let name = key.rsplit([':', '/']).next().unwrap_or(key);
    match name.rsplit_once('#') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    }
}

/// Generated names for unnamed nets (`Net-(…)` or `N$12`).
fn is_auto_net(name: &str) -> bool {
    name.starts_with("Net-(") || name.starts_with("N$") || name.starts_with("unconnected-(")
}

/// True when a schematic parameter with a real value is missing or different
/// on the board. Board-only parameters don't count.
fn parameters_differ(board: &BTreeMap<String, String>, schematic: &BTreeMap<String, String>) -> bool {
    schematic.iter().any(|(k, v)| {
        let v = v.trim();
        !v.is_empty() && v != "~" && !matches!(k.as_str(), "Value" | "Footprint" | "Reference" | "Designator") && board.get(k).map(|b| b.trim()) != Some(v)
    })
}

pub fn plan(compiled: &Compiled, board: &Document, resolve: &dyn Fn(&NetlistPart) -> Option<(String, Footprint)>) -> EcoPlan {
    let mut changes: Vec<EcoChange> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut footprints: Vec<(String, Footprint)> = Vec::new();

    // ----- Schematic side -----
    let mut parts: Vec<&NetlistPart> = Vec::new();
    for p in &compiled.parts {
        let des = p.designator.trim();
        if des.starts_with('#') || p.excluded_from_board() {
            continue;
        }
        if !is_placeable(des) {
            warnings.push(format!("Part {} ({}) is not annotated and was skipped", if des.is_empty() { "?" } else { des }, p.lib_ref));
            continue;
        }
        parts.push(p);
    }
    parts.sort_by(|a, b| natural_cmp(&a.designator, &b.designator));
    let known: HashSet<&str> = parts.iter().map(|p| p.designator.trim()).collect();

    // designator → pad → net
    let mut want: HashMap<&str, BTreeMap<String, String>> = HashMap::new();
    for net in &compiled.nets {
        for node in &net.nodes {
            let des = node.designator.trim();
            if !known.contains(des) {
                continue;
            }
            if node.pads.is_empty() {
                warnings.push(format!("{des} pin {} ({}) has no pad in the pin map", node.pin_number, net.name));
                continue;
            }
            let map = want.entry(des).or_default();
            for pad in &node.pads {
                match map.get(pad) {
                    Some(other) if other != &net.name => {
                        warnings.push(format!("{des} pad {pad} is connected to both '{other}' and '{}'", net.name));
                    }
                    _ => {
                        map.insert(pad.clone(), net.name.clone());
                    }
                }
            }
        }
    }

    // ----- Board side -----
    let mut on_board: HashMap<&str, (ItemId, &PlacedFootprint)> = HashMap::new();
    let mut board_fps: Vec<(ItemId, &PlacedFootprint)> = Vec::new();
    for (id, item) in board.items() {
        if let Item::Footprint(f) = item {
            board_fps.push((id, f));
            if on_board.contains_key(f.designator.trim()) {
                warnings.push(format!("Designator {} is used by several footprints on the board", f.designator));
            } else {
                on_board.insert(f.designator.trim(), (id, f));
            }
        }
    }

    // ----- Renames: identical pad sets under a different name -----
    let mut pcb_sets: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
    for (_, f) in &board_fps {
        for (pad, net) in &f.pad_nets {
            if !net.is_empty() {
                pcb_sets.entry(net.as_str()).or_default().insert((f.designator.trim(), pad.as_str()));
            }
        }
    }
    let mut sch_sets: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
    for (des, map) in &want {
        if !on_board.contains_key(des) { continue; } // New, not-yet-placed parts must not invalidate existing copper names.
        for (pad, net) in map {
            sch_sets.entry(net.as_str()).or_default().insert((des, pad.as_str()));
        }
    }
    let sch_by_set: HashMap<&BTreeSet<(&str, &str)>, &str> = sch_sets.iter().map(|(n, s)| (s, *n)).collect();
    let candidates: BTreeMap<&str, &str> =
        pcb_sets.iter().filter_map(|(n, s)| sch_by_set.get(s).filter(|m| **m != *n).map(|m| (*n, *m))).collect();
    let occupied_nets: HashSet<&str> = board.items().flat_map(|(_,item)| match item {
        Item::Footprint(f) => f.pad_nets.values().map(String::as_str).collect::<Vec<_>>(),
        Item::Track(t) => vec![t.net.as_str()], Item::ArcTrack(a) => vec![a.net.as_str()],
        Item::Via(v) => vec![v.net.as_str()], Item::Zone(z) => vec![z.net.as_str()], _ => vec![],
    }).filter(|net| !net.is_empty()).collect();
    // Never rename onto a board net that keeps its name (that would merge copper).
    let renames: BTreeMap<&str, &str> =
        candidates.iter().filter(|(_, to)| !occupied_nets.contains(**to) || candidates.contains_key(**to)).map(|(f, t)| (*f, *t)).collect();

    // Equivalent generated labels are cosmetic. Reuse the board name for new pads too,
    // so adding a part cannot create a disconnected net beside existing copper.
    let canonical_names: HashMap<&str, &str> = renames.iter().filter(|(from,to)| is_auto_net(from) && is_auto_net(to)).map(|(from,to)|(*to,*from)).collect();

    // A split can be intentional, but incomplete imported hierarchy can also cause it.
    // Never disguise this as a cosmetic rename: retain the individual changes for review.
    for (old, pads) in &pcb_sets {
        let targets: BTreeSet<&str> = pads.iter().filter_map(|(des, pad)| want.get(des).and_then(|m| m.get(*pad)).map(String::as_str)).collect();
        if targets.len() > 1 {
            warnings.push(format!("Board net '{old}' maps to several schematic nets ({}); review bus/hierarchy connectivity and routing before applying pad changes", targets.into_iter().map(|n| if n.is_empty() { "<unconnected>" } else { n }).collect::<Vec<_>>().join(", ")));
        }
    }

    // ----- Per part -----
    let mut queued: HashSet<String> = HashSet::new();
    let mut need_geometry = |key: &str, fp: &Footprint, footprints: &mut Vec<(String, Footprint)>| {
        if board.cached_footprint(key).is_none() && queued.insert(key.to_string()) {
            footprints.push((key.to_string(), fp.clone()));
        }
    };
    for part in &parts {
        let des = part.designator.trim();
        // The board already carries this footprint (same name, any library
        // prefix): use its geometry instead of searching the libraries.
        let on_board_geom = on_board.get(des).and_then(|(_, f)| {
            let geom = board.cached_footprint(&f.footprint)?;
            (!part.footprint.trim().is_empty() && footprint_name(&f.footprint) == footprint_name(&part.footprint)).then(|| (f.footprint.clone(), geom.clone()))
        });
        let resolved = if part.footprint.trim().is_empty() {
            None
        } else if on_board_geom.is_some() {
            on_board_geom
        } else {
            let r = resolve(part);
            if r.is_none() {
                warnings.push(format!("Footprint '{}' of {des} was not found in any library", part.footprint));
            }
            r
        };
        let wanted = want.get(des).map(|pads| pads.iter().map(|(pad,net)| (pad.clone(),canonical_names.get(net.as_str()).copied().unwrap_or(net).to_string())).collect()).unwrap_or_default();
        let parameters: BTreeMap<String, String> = part.parameters.iter().cloned().collect();

        match on_board.get(des) {
            None => {
                if part.footprint.trim().is_empty() {
                    warnings.push(format!("{des} has no footprint assigned and was not placed"));
                    continue;
                }
                let Some((key, geom)) = resolved else { continue };
                need_geometry(&key, &geom, &mut footprints);
                let pad_nets = target_pads(des, &key, Some(&geom), &wanted, &mut warnings);
                changes.push(EcoChange::AddFootprint {
                    designator: des.to_string(),
                    value: part.value.clone(),
                    footprint_key: key,
                    component: part.lib_ref.clone(),
                    pad_nets,
                    parameters,
                });
            }
            Some(&(item, fp)) => {
                if part.footprint.trim().is_empty() {
                    warnings.push(format!("{des} has no footprint assigned in the schematic; the board footprint is kept"));
                }
                let key = match &resolved {
                    Some((key, geom)) => {
                        if !same_footprint(key, &fp.footprint) {
                            need_geometry(key, geom, &mut footprints);
                            changes.push(EcoChange::ChangeFootprint {
                                item,
                                designator: des.to_string(),
                                from: fp.footprint.clone(),
                                to: key.clone(),
                            });
                        }
                        key.clone()
                    }
                    None => fp.footprint.clone(),
                };
                // Keep board-local variants ("Lib:Name#2") the importer created.
                let key = if same_footprint(&key, &fp.footprint) { fp.footprint.clone() } else { key };
                if fp.value != part.value {
                    changes.push(EcoChange::UpdateValue { item, designator: des.to_string(), from: fp.value.clone(), to: part.value.clone() });
                }
                if parameters_differ(&fp.parameters, &parameters) || (!fp.component.is_empty() && fp.component != part.lib_ref) {
                    changes.push(EcoChange::UpdateParameters { item, designator: des.to_string(), component: part.lib_ref.clone(), parameters });
                }
                let geom = board.cached_footprint(&key).or(resolved.as_ref().map(|(_, g)| g));
                let mut target = target_pads(des, &key, geom, &wanted, &mut warnings);
                if geom.is_none() {
                    // Pads unknown: at least disconnect pads the schematic no longer uses.
                    for pad in fp.pad_nets.keys() {
                        target.entry(pad.clone()).or_default();
                    }
                }
                let net_changes: Vec<(String, String, String)> = target
                    .iter()
                    .filter_map(|(pad, new)| {
                        let old = fp.net_of(pad);
                        let renamed = renames.get(old).is_some_and(|to| *to == new);
                        (old != new && !renamed).then(|| (pad.clone(), old.to_string(), new.clone()))
                    })
                    .collect();
                if !net_changes.is_empty() {
                    changes.push(EcoChange::UpdateNets { item, designator: des.to_string(), changes: net_changes });
                }
            }
        }
    }

    let mut removed: Vec<(ItemId, &PlacedFootprint)> = board_fps.iter().filter(|(_, f)| !f.board_only && !known.contains(f.designator.trim())).copied().collect();
    removed.sort_by(|a, b| natural_cmp(&a.1.designator, &b.1.designator));
    for (item, f) in removed {
        changes.push(EcoChange::RemoveFootprint { item, designator: f.designator.clone() });
    }
    for (from, to) in renames {
        // Auto-generated names on both sides ("Net-(R1-Pad2)" vs structured format's
        // "Net-(R1-A)") describe the same net; renaming would only be noise.
        if is_auto_net(from) && is_auto_net(to) {
            continue;
        }
        changes.push(EcoChange::RenameNet { from: from.to_string(), to: to.to_string() });
    }
    changes.extend(warnings.into_iter().map(|message| EcoChange::Warning { message }));
    EcoPlan { changes, footprints, source_board: Some(board.clone()), source_netlist: serde_json::to_value(compiled).expect("netlist serializes") }
}

/// Pad → net for every pad of the footprint (unconnected pads get "").
/// Without geometry, only the schematic's pads are known.
fn target_pads(des: &str, key: &str, geom: Option<&Footprint>, wanted: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    let Some(geom) = geom else { return wanted.clone() };
    let pads = pad_numbers(geom);
    for (pad, net) in wanted {
        if !pads.contains(pad) {
            warnings.push(format!("{des}: pin for pad '{pad}' (net '{net}') has no matching pad in footprint '{key}'"));
        }
    }
    pads.into_iter()
        .map(|p| {
            let net = wanted.get(&p).cloned().unwrap_or_default();
            (p, net)
        })
        .collect()
}

// ---------- Applying ----------

/// Gap between new footprints, and between them and the board.
const PART_GAP: Coord = 1_000_000;
const BOARD_GAP: Coord = 5_000_000;
const ROW_WIDTH: Coord = 100_000_000;

/// Builds one undoable command for the selected change indices (warnings and
/// out-of-range indices are ignored). Net renames apply to the board first,
/// then per-footprint changes in plan order; new footprints are laid out in
/// rows to the right of the board outline and existing footprints.
pub fn apply(plan: &EcoPlan, board: &Document, selected: &[usize]) -> Command {
    let chosen: Vec<&EcoChange> = selected.iter().copied().collect::<BTreeSet<_>>().into_iter().filter_map(|i| plan.changes.get(i)).collect();
    let mut modified: BTreeMap<ItemId, Item> = BTreeMap::new();

    let renames: HashMap<&str, &str> = chosen
        .iter()
        .filter_map(|c| if let EcoChange::RenameNet { from, to } = c { Some((from.as_str(), to.as_str())) } else { None })
        .collect();
    if !renames.is_empty() {
        for (id, item) in board.items() {
            if let Some(new) = renamed_item(item, &renames) {
                modified.insert(id, new);
            }
        }
    }

    let mut deleted: Vec<ItemId> = Vec::new();
    let mut adds: Vec<PlacedFootprint> = Vec::new();
    for c in chosen {
        match c {
            EcoChange::RemoveFootprint { item, .. } => deleted.push(*item),
            EcoChange::ChangeFootprint { item, to, .. } => edit_fp(&mut modified, board, *item, |f| f.footprint = to.clone()),
            EcoChange::UpdateValue { item, to, .. } => edit_fp(&mut modified, board, *item, |f| f.value = to.clone()),
            EcoChange::UpdateNets { item, changes, .. } => edit_fp(&mut modified, board, *item, |f| {
                for (pad, _, new) in changes {
                    f.pad_nets.insert(pad.clone(), new.clone());
                }
            }),
            EcoChange::UpdateParameters { item, component, parameters, .. } => edit_fp(&mut modified, board, *item, |f| {
                f.component = component.clone();
                // Merge: board-only parameters (DNP, imported extras) survive.
                f.parameters.extend(parameters.iter().map(|(k, v)| (k.clone(), v.clone())));
            }),
            EcoChange::AddFootprint { designator, value, footprint_key, component, pad_nets, parameters } => adds.push(PlacedFootprint {
                board_only: false,
                designator: designator.clone(),
                value: value.clone(),
                footprint: footprint_key.clone(),
                pos: Point::default(),
                rotation: 0.0,
                side: Side::Top,
                locked: false,
                pad_nets: pad_nets.clone(),
                component: component.clone(),
                parameters: parameters.clone(),
            }),
            EcoChange::RenameNet { .. } | EcoChange::Warning { .. } => {}
        }
    }
    for id in &deleted {
        modified.remove(id);
    }
    adds.sort_by(|a, b| natural_cmp(&a.designator, &b.designator));
    place_new(&mut adds, board, plan, &deleted);

    if deleted.is_empty() && adds.is_empty() {
        return Command::ReplaceMany { items: modified.into_iter().map(|(id, item)| IdItem { id, item }).collect() };
    }
    // `Change` has no in-place replacement, so modified items are re-added
    // (they get new ids) together with the new footprints.
    let mut delete = deleted;
    delete.extend(modified.keys().copied());
    let mut add: Vec<Item> = modified.into_values().collect();
    add.extend(adds.into_iter().map(Item::Footprint));
    Command::Change { delete, add }
}

fn edit_fp(modified: &mut BTreeMap<ItemId, Item>, board: &Document, id: ItemId, f: impl FnOnce(&mut PlacedFootprint)) {
    let current = modified.get(&id).or_else(|| board.get(id)).cloned();
    if let Some(Item::Footprint(mut fp)) = current {
        f(&mut fp);
        modified.insert(id, Item::Footprint(fp));
    }
}

/// The item with renamed nets, or `None` if it has none of them.
fn renamed_item(item: &Item, renames: &HashMap<&str, &str>) -> Option<Item> {
    let ren = |n: &mut String| -> bool {
        match renames.get(n.as_str()) {
            Some(to) => {
                *n = (*to).to_string();
                true
            }
            None => false,
        }
    };
    let mut out = item.clone();
    let changed = match &mut out {
        Item::Footprint(f) => f.pad_nets.values_mut().fold(false, |acc, n| ren(n) | acc),
        Item::Track(t) => ren(&mut t.net),
        Item::ArcTrack(t) => ren(&mut t.net),
        Item::Via(v) => ren(&mut v.net),
        Item::Zone(z) => ren(&mut z.net),
        Item::Board(b) => {
            let mut any = false;
            for c in &mut b.net_classes {
                for n in &mut c.nets {
                    any |= ren(n);
                }
            }
            for d in &mut b.diff_pairs {
                any |= ren(&mut d.positive);
                any |= ren(&mut d.negative);
            }
            any
        }
        _ => false,
    };
    changed.then_some(out)
}

// ---------- Placement ----------

fn pad_extent(p: &Pad) -> (Coord, Coord) {
    let r = crate::geom::normalize_deg(p.rotation);
    if r == 0.0 || r == 180.0 {
        (p.width / 2, p.height / 2)
    } else if r == 90.0 || r == 270.0 {
        (p.height / 2, p.width / 2)
    } else {
        let d = ((p.width as f64).hypot(p.height as f64) / 2.0).ceil() as Coord;
        (d, d)
    }
}

fn item_points(item: &Item) -> Vec<Point> {
    match item {
        Item::Line(l) => vec![l.a, l.b],
        Item::Rect(r) => vec![r.a, r.b],
        Item::Polyline(p) => p.points.clone(),
        Item::Circle(c) => vec![c.center.translated(-c.radius, -c.radius), c.center.translated(c.radius, c.radius)],
        Item::Arc(a) => vec![a.center.translated(-a.radius, -a.radius), a.center.translated(a.radius, a.radius)],
        Item::Pad(p) => {
            let (hx, hy) = pad_extent(p);
            vec![p.pos.translated(-hx, -hy), p.pos.translated(hx, hy)]
        }
        Item::Text(t) => vec![t.pos],
        _ => vec![],
    }
}

/// Local bounding box: the courtyard if drawn, else pads and graphics.
pub(crate) fn local_bbox(fp: &Footprint) -> Option<BBox> {
    let courtyard = |i: &&Item| match i {
        Item::Line(l) => l.layer == layer::TOP_COURTYARD,
        Item::Rect(r) => r.layer == layer::TOP_COURTYARD,
        Item::Polyline(p) => p.layer == layer::TOP_COURTYARD,
        Item::Circle(c) => c.layer == layer::TOP_COURTYARD,
        Item::Arc(a) => a.layer == layer::TOP_COURTYARD,
        _ => false,
    };
    BBox::from_points(fp.items.iter().filter(courtyard).flat_map(item_points))
        .or_else(|| BBox::from_points(fp.items.iter().filter(|i| !matches!(i, Item::Text(_))).flat_map(item_points)))
}

fn world_bbox(f: &PlacedFootprint, geom: Option<&Footprint>) -> BBox {
    match geom.and_then(local_bbox) {
        Some(b) => {
            let pl = f.placement();
            let corners = [b.min, Point::new(b.max.x, b.min.y), b.max, Point::new(b.min.x, b.max.y)];
            BBox::from_points(corners.map(|c| pl.apply(c))).unwrap_or(BBox { min: f.pos, max: f.pos })
        }
        None => BBox { min: f.pos, max: f.pos },
    }
}

fn place_new(adds: &mut [PlacedFootprint], board: &Document, plan: &EcoPlan, deleted: &[ItemId]) {
    if adds.is_empty() {
        return;
    }
    let geometry = |key: &str| plan.footprints.iter().find(|(k, _)| k == key).map(|(_, f)| f).or_else(|| board.cached_footprint(key));

    let outline = BBox::from_points(outline_loops(board, mm(0.01)).into_iter().flatten());
    let existing = BBox::from_points(
        board
            .items()
            .filter(|(id, _)| !deleted.contains(id))
            .filter_map(|(_, i)| if let Item::Footprint(f) = i { Some(world_bbox(f, board.cached_footprint(&f.footprint))) } else { None })
            .flat_map(|b| [b.min, b.max]),
    );
    let (x0, top) = match (outline, existing) {
        (Some(o), Some(e)) => (o.max.x.max(e.max.x) + BOARD_GAP, o.max.y),
        (Some(o), None) => (o.max.x + BOARD_GAP, o.max.y),
        (None, Some(e)) => (e.max.x + BOARD_GAP, e.max.y),
        (None, None) => (0, 0),
    };

    let fallback = BBox { min: Point::new(-mm(2.5), -mm(2.5)), max: Point::new(mm(2.5), mm(2.5)) };
    let (mut x, mut y, mut row_h) = (x0, top, 0);
    for f in adds.iter_mut() {
        let b = geometry(&f.footprint).and_then(local_bbox).unwrap_or(fallback);
        let (w, h) = (b.max.x - b.min.x, b.max.y - b.min.y);
        if x > x0 && x + w > x0 + ROW_WIDTH {
            y -= row_h + PART_GAP;
            x = x0;
            row_h = 0;
        }
        f.pos = Point::new(x - b.min.x, y - b.max.y);
        x += w + PART_GAP;
        row_h = row_h.max(h);
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
