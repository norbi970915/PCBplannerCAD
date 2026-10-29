//! Hierarchy expansion, cross-sheet net merging, net naming and the part list.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::document::ItemId;
use crate::item::{FIELD_DESIGNATOR, FIELD_FOOTPRINT, FIELD_VALUE, LabelScope, Part};
use crate::library::CachedPart;
use crate::units::Point;

use super::connectivity::{ElemKind, SheetConn, UnionFind, hier_key, join_key};
use super::erc::{self, NetFacts, PinSite, Placed, Report};
use super::{Compiled, Net, NetNode, NetlistPart, SheetInstance, SheetRef, is_unannotated, natural_cmp, norm_path};

struct Inst {
    path: String,
    sheet: usize,
    parent: Option<usize>,
    /// Index into the parent's `SheetConn::sheets`.
    sheet_info: Option<usize>,
    depth: usize,
}

struct Walk<'s, 'a> {
    sheets: &'s [SheetRef<'a>],
    conns: &'s [SheetConn<'a>],
    index: HashMap<String, usize>,
    insts: Vec<Inst>,
    stack: Vec<usize>,
}

impl Walk<'_, '_> {
    fn walk(&mut self, sheet: usize, parent: Option<usize>, sheet_info: Option<usize>, path: String, r: &mut Report) {
        let me = self.insts.len();
        self.insts.push(Inst { path: path.clone(), sheet, parent, sheet_info, depth: self.stack.len() });
        self.stack.push(sheet);
        let conns = self.conns;
        for (si, s) in conns[sheet].sheets.iter().enumerate() {
            let file = norm_path(&s.sheet.file);
            let here = &self.sheets[sheet].path;
            match self.index.get(&file).copied() {
                None => r.error(
                    "missing-sheet",
                    format!("Sheet '{}' refers to missing file '{}'", s.sheet.name, s.sheet.file),
                    here,
                    vec![s.item],
                    Some(s.sheet.pos),
                ),
                Some(c) if self.stack.contains(&c) => r.error(
                    "sheet-recursion",
                    format!("Sheet '{}' instantiates '{}', which contains itself", s.sheet.name, s.sheet.file),
                    here,
                    vec![s.item],
                    Some(s.sheet.pos),
                ),
                Some(c) => {
                    let name = match s.sheet.name.trim() {
                        "" => file_stem(&file),
                        n => n.to_string(),
                    };
                    self.walk(c, Some(me), Some(si), format!("{path}{name}/"), r);
                }
            }
        }
        self.stack.pop();
    }
}

fn file_stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.split('.').next().unwrap_or(base).to_string()
}

#[derive(Default)]
struct NetAcc {
    pins: Vec<(usize, usize, usize)>, // (instance, part, pin)
    items: BTreeSet<(usize, ItemId)>, // (sheet index, item)
    global: BTreeSet<String>,
    /// First site of each global name: (sheet, item, pos).
    global_sites: BTreeMap<String, (usize, ItemId, Point)>,
    /// (depth, rank, full name) — lowest wins.
    local: Vec<(usize, u8, String)>,
    has_label: bool,
    has_port: bool,
    has_entry: bool,
}

/// Compiles the given sheets into a netlist with ERC results.
pub fn compile(sheets: &[SheetRef]) -> Compiled {
    let conns: Vec<SheetConn> = sheets.iter().map(|s| SheetConn::build(s.doc)).collect();
    let mut report = Report::default();
    let files: Vec<String> = sheets.iter().map(|s| norm_path(&s.path)).collect();

    // ----- Hierarchy -----
    let mut index = HashMap::new();
    for (i, f) in files.iter().enumerate() {
        index.entry(f.clone()).or_insert(i);
    }
    let referenced: HashSet<String> = conns.iter().flat_map(|c| c.sheets.iter().map(|s| norm_path(&s.sheet.file))).collect();
    let mut roots: Vec<usize> = (0..sheets.len()).filter(|&i| !referenced.contains(&files[i]) && index[&files[i]] == i).collect();
    if roots.is_empty() && !sheets.is_empty() {
        roots.push(0);
    }
    let mut walk = Walk { sheets, conns: &conns, index, insts: Vec::new(), stack: Vec::new() };
    for r in roots {
        walk.walk(r, None, None, "/".into(), &mut report);
    }
    let insts = walk.insts;

    for (i, c) in conns.iter().enumerate() {
        erc::sheet_checks(&sheets[i].path, c, &mut report);
    }
    hierarchy_checks(sheets, &conns, &insts, &mut report);
    let aliases = super::bus::aliases(sheets, &mut report);
    let buses: Vec<_> = sheets.iter().zip(&conns).map(|(sheet, conn)| super::bus::build(sheet.doc, conn, &aliases, &sheet.path, &mut report)).collect();

    // ----- Merge groups across instances -----
    let mut base = Vec::with_capacity(insts.len());
    let mut total = 0;
    for inst in &insts {
        base.push(total);
        total += conns[inst.sheet].group_count;
    }
    let mut bus_base = Vec::with_capacity(insts.len());
    for inst in &insts { bus_base.push(total); total += buses[inst.sheet].signals.len(); }
    let mut uf = UnionFind::new(total);
    let mut keys: HashMap<String, usize> = HashMap::new();
    for (i, inst) in insts.iter().enumerate() {
        let c = &conns[inst.sheet];
        for e in 0..c.elems.len() {
            if buses[inst.sheet].ports.contains(&e) { continue; }
            if let Some(k) = c.name_key(e, i) {
                join_key(&mut uf, &mut keys, k, base[i] + c.group[e]);
                if let ElemKind::Label {scope: LabelScope::Hierarchical,name} = c.elems[e].kind {
                    join_key(&mut uf, &mut keys, format!("l\0{i}\0{name}"), base[i] + c.group[e]);
                }
            }
        }
        for (signal_index, signal) in buses[inst.sheet].signals.iter().enumerate() {
            let node = bus_base[i] + signal_index;
            join_key(&mut uf, &mut keys, format!("bus\0{i}\0{}\0{}", signal.group, signal.key), node);
            if let Some(scope) = signal.scope {
                // Inside the sheet, scalar member labels use the expanded local name.
                join_key(&mut uf, &mut keys, format!("l\0{i}\0{}", signal.name), node);
                match scope {
                    LabelScope::Global => join_key(&mut uf, &mut keys, super::connectivity::global_key(&signal.name), node),
                    LabelScope::Hierarchical => join_key(&mut uf, &mut keys, hier_key(i, &signal.name), node),
                    LabelScope::Local => {},
                }
            }
        }
        if let (Some(p), Some(si)) = (inst.parent, inst.sheet_info) {
            let pc = &conns[insts[p].sheet];
            let info = &pc.sheets[si];
            for (k, &e) in info.entries.iter().enumerate() {
                let signals: Vec<_> = buses[insts[p].sheet].signals.iter().enumerate().filter(|(_, signal)| signal.element == e).collect();
                if !signals.is_empty() {
                    for (index, signal) in signals { join_key(&mut uf, &mut keys, hier_key(i, &signal.name), bus_base[p] + index); }
                    continue;
                }
                let name = info.sheet.entries[k].name.trim();
                if !name.is_empty() {
                    join_key(&mut uf, &mut keys, hier_key(i, name), base[p] + pc.group[e]);
                }
            }
        }
    }

    // ----- Collect nets -----
    let mut accs: Vec<NetAcc> = Vec::new();
    let mut by_root: HashMap<usize, usize> = HashMap::new();
    for (i, inst) in insts.iter().enumerate() {
        let c = &conns[inst.sheet];
        for (e, el) in c.elems.iter().enumerate() {
            if el.isolated || buses[inst.sheet].ports.contains(&e) {
                continue;
            }
            let root = uf.find(base[i] + c.group[e]);
            let next = accs.len();
            let ai = *by_root.entry(root).or_insert(next);
            if ai == accs.len() {
                accs.push(NetAcc::default());
            }
            let acc = &mut accs[ai];
            match &el.kind {
                ElemKind::Wire | ElemKind::BusEntry | ElemKind::Junction => {
                    acc.items.insert((inst.sheet, el.item));
                }
                ElemKind::Label { scope, name } => {
                    acc.items.insert((inst.sheet, el.item));
                    acc.has_label = true;
                    if name.is_empty() {
                        continue;
                    }
                    match scope {
                        LabelScope::Global => {
                            acc.global.insert(name.to_string());
                            acc.global_sites.entry(name.to_string()).or_insert((inst.sheet, el.item, el.a));
                        }
                        LabelScope::Local => acc.local.push((inst.depth, 0, scoped_name(&inst.path, name))),
                        LabelScope::Hierarchical => acc.local.push((inst.depth, 1, scoped_name(&inst.path, name))),
                    }
                }
                ElemKind::Power { name, .. } => {
                    acc.items.insert((inst.sheet, el.item));
                    acc.has_port = true;
                    if !name.is_empty() {
                        acc.global.insert(name.clone());
                        acc.global_sites.entry(name.clone()).or_insert((inst.sheet, el.item, el.a));
                    }
                }
                ElemKind::Pin { part, pin } => {
                    acc.pins.push((i, *part, *pin));
                    let p = c.pin(*part, *pin);
                    if let Some(k) = c.name_key(p.elem, i) {
                        let name = k.trim_start_matches("g\u{0}").to_string();
                        acc.global_sites.entry(name.clone()).or_insert((inst.sheet, el.item, p.world));
                        acc.global.insert(name);
                    }
                }
                ElemKind::SheetEntry => acc.has_entry = true,
                ElemKind::NoConnect => {}
            }
        }
    }

    // Bus labels contribute names to their resolved scalar nets, never a short between members.
    for (i, inst) in insts.iter().enumerate() {
        for (index, signal) in buses[inst.sheet].signals.iter().enumerate() {
            let Some(&ai) = by_root.get(&uf.find(bus_base[i] + index)) else { continue };
            let acc = &mut accs[ai];
            let el = &conns[inst.sheet].elems[signal.element];
            match signal.scope {
                Some(LabelScope::Global) => {
                    acc.global.insert(signal.name.clone());
                    acc.global_sites.entry(signal.name.clone()).or_insert((inst.sheet, el.item, el.a));
                },
                Some(scope) => acc.local.push((inst.depth, if scope == LabelScope::Local {0} else {1}, scoped_name(&inst.path, &signal.name))),
                None => {},
            }
        }
    }

    // ----- Build nets -----
    let mut nets: Vec<Net> = Vec::new();
    for acc in &accs {
        if acc.pins.is_empty() {
            continue;
        }
        let pins: Vec<PinSite> = acc.pins.iter().map(|&r| pin_site(sheets, &conns, &insts, r)).collect();
        // A lone pin touching nothing is not a net, but it is still checked.
        if acc.pins.len() == 1 && acc.items.is_empty() && acc.global.is_empty() && !acc.has_entry {
            erc::net_checks(&NetFacts { name: "", pins, has_label: false, has_port: false, has_entry: false }, &mut report);
            continue;
        }
        let mut nodes: Vec<NetNode> = acc
            .pins
            .iter()
            .map(|&(i, part, pin)| {
                let inst = &insts[i];
                let c = &conns[inst.sheet];
                let pi = &c.parts[part];
                let p = c.pin(part, pin).pin;
                NetNode {
                    designator: pi.designator().to_string(),
                    pin_number: p.number.clone(),
                    pin_name: p.name.clone(),
                    pin_type: p.electrical,
                    pads: pi.cached.map(|cp| cp.component.pads_for(pi.part.unit, &p.number)).unwrap_or_default(),
                    sheet: sheets[inst.sheet].path.clone(),
                    part: pi.item,
                }
            })
            .collect();
        nodes.sort_by(|a, b| natural_cmp(&a.designator, &b.designator).then_with(|| natural_cmp(&a.pin_number, &b.pin_number)));

        let name = if let Some(first) = acc.global.iter().next() {
            if acc.global.len() > 1 {
                let names: Vec<&str> = acc.global.iter().map(String::as_str).collect();
                let (s, _, pos) = acc.global_sites[names[1]];
                let items: Vec<ItemId> = acc.global_sites.values().filter(|v| v.0 == s).map(|v| v.1).collect::<BTreeSet<_>>().into_iter().collect();
                report.warning(
                    "multiple-net-names",
                    format!("Net has multiple names ({}); using '{first}'", names.join(", ")),
                    &sheets[s].path,
                    items,
                    Some(pos),
                );
            }
            first.clone()
        } else if let Some((_, _, n)) =
            acc.local.iter().min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| natural_cmp(&a.2, &b.2)))
        {
            n.clone()
        } else {
            let n = &nodes[0];
            let pad = n.pads.first().unwrap_or(&n.pin_number);
            format!("Net-({}-Pad{})", n.designator, pad)
        };

        erc::net_checks(
            &NetFacts { name: &name, pins, has_label: acc.has_label, has_port: acc.has_port, has_entry: acc.has_entry },
            &mut report,
        );

        let items = acc.items.iter().map(|&(s, id)| (sheets[s].path.clone(), id)).collect();
        nets.push(Net { name, nodes, items });
    }
    unique_names(&mut nets);
    nets.sort_by(|a, b| natural_cmp(&a.name, &b.name));

    // ----- Parts & designators -----
    let mut placed = Vec::new();
    let mut parts: Vec<NetlistPart> = Vec::new();
    let mut by_des: HashMap<String, usize> = HashMap::new();
    for inst in &insts {
        let c = &conns[inst.sheet];
        let file = sheets[inst.sheet].path.as_str();
        for pi in &c.parts {
            if pi.is_power {
                continue;
            }
            let des = pi.designator();
            let unit = (file.to_string(), pi.item, pi.part.unit);
            if !is_unannotated(des) {
                placed.push(Placed { designator: des, lib_ref: &pi.part.lib_ref, unit: pi.part.unit, file, item: pi.item, pos: pi.part.pos });
                if let Some(&k) = by_des.get(des) {
                    if !parts[k].units.contains(&unit) {
                        parts[k].units.push(unit);
                    }
                    continue;
                }
                by_des.insert(des.to_string(), parts.len());
            }
            parts.push(netlist_part(des, pi.part, pi.cached, unit));
        }
    }
    erc::designator_checks(&placed, &mut report);
    for p in &mut parts {
        p.units.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    }
    parts.sort_by(|a, b| natural_cmp(&a.designator, &b.designator));

    let instances = insts
        .iter()
        .map(|inst| SheetInstance {
            path: inst.path.clone(),
            sheet: sheets[inst.sheet].path.clone(),
            parent: inst.parent.map(|p| insts[p].path.clone()),
            sheet_item: match (inst.parent, inst.sheet_info) {
                (Some(p), Some(si)) => Some(conns[insts[p].sheet].sheets[si].item),
                _ => None,
            },
        })
        .collect();

    Compiled { nets, parts, violations: report.finish(), instances }
}

fn scoped_name(path: &str, name: &str) -> String {
    if path == "/" { name.to_string() } else { format!("{path}{name}") }
}

/// Distinct nets must have distinct names (e.g. same local label on two
/// top-level sheets): later duplicates get a numeric suffix.
fn unique_names(nets: &mut [Net]) {
    let mut taken: HashSet<String> = nets.iter().map(|n| n.name.clone()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    for net in nets.iter_mut() {
        if seen.insert(net.name.clone()) {
            continue;
        }
        let mut k = 2;
        let fresh = loop {
            let cand = format!("{}_{k}", net.name);
            if !taken.contains(&cand) {
                break cand;
            }
            k += 1;
        };
        taken.insert(fresh.clone());
        seen.insert(fresh.clone());
        net.name = fresh;
    }
}

const RESERVED: [&str; 4] = [FIELD_DESIGNATOR, FIELD_VALUE, FIELD_FOOTPRINT, "Description"];

fn netlist_part(des: &str, part: &Part, cached: Option<&CachedPart>, unit: (String, ItemId, u32)) -> NetlistPart {
    let field = |name: &str| Some(part.field_value(name).trim()).filter(|v| !v.is_empty()).map(str::to_string);
    let comp = cached.map(|c| &c.component);
    let value = field(FIELD_VALUE).or_else(|| comp.and_then(|c| c.parameter(FIELD_VALUE)).map(str::to_string)).unwrap_or_default();
    let footprint = field(FIELD_FOOTPRINT).or_else(|| comp.and_then(|c| c.footprints.first().cloned())).unwrap_or_default();
    let description = field("Description")
        .or_else(|| comp.and_then(|c| c.parameter("Description")).map(str::to_string))
        .or_else(|| comp.map(|c| c.description.clone()))
        .unwrap_or_default();
    let mut parameters: Vec<(String, String)> = Vec::new();
    let mut set = |name: &str, value: &str| {
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
            return;
        }
        match parameters.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some(p) => p.1 = value.to_string(),
            None => parameters.push((name.to_string(), value.to_string())),
        }
    };
    if let Some(c) = comp {
        for p in &c.parameters {
            set(&p.name, &p.value);
        }
    }
    for f in &part.fields {
        if !f.value.trim().is_empty() {
            set(&f.name, &f.value);
        }
    }
    NetlistPart {
        designator: des.to_string(),
        value,
        footprint,
        lib_ref: part.lib_ref.clone(),
        component: comp.map(|c| c.name.clone()).unwrap_or_default(),
        library: cached.map(|c| c.library.clone()).unwrap_or_default(),
        description,
        units: vec![unit],
        parameters,
    }
}

/// Hierarchical labels ↔ sheet entries.
fn hierarchy_checks(sheets: &[SheetRef], conns: &[SheetConn], insts: &[Inst], r: &mut Report) {
    let hier_labels = |c: &SheetConn| -> Vec<(String, ItemId, Point)> {
        c.elems
            .iter()
            .filter_map(|e| match e.kind {
                ElemKind::Label { scope: LabelScope::Hierarchical, name } => Some((name.to_string(), e.item, e.a)),
                _ => None,
            })
            .collect()
    };
    for inst in insts {
        let c = &conns[inst.sheet];
        let file = &sheets[inst.sheet].path;
        let labels = hier_labels(c);
        match (inst.parent, inst.sheet_info) {
            (Some(p), Some(si)) => {
                let pc = &conns[insts[p].sheet];
                let info = &pc.sheets[si];
                let entry_names: HashSet<&str> = info.sheet.entries.iter().map(|e| e.name.trim()).collect();
                for (name, item, pos) in &labels {
                    if !entry_names.contains(name.as_str()) {
                        r.error(
                            "missing-sheet-entry",
                            format!("Hierarchical label '{name}' has no matching entry on sheet symbol '{}'", info.sheet.name),
                            file,
                            vec![*item],
                            Some(*pos),
                        );
                    }
                }
                let label_names: HashSet<&str> = labels.iter().map(|l| l.0.as_str()).collect();
                for e in &info.sheet.entries {
                    if !label_names.contains(e.name.trim()) {
                        r.warning(
                            "unused-sheet-entry",
                            format!("Sheet entry '{}' has no matching hierarchical label in '{}'", e.name, info.sheet.file),
                            &sheets[insts[p].sheet].path,
                            vec![info.item],
                            Some(info.sheet.entry_point(e)),
                        );
                    }
                }
            }
            _ => {
                for (name, item, pos) in &labels {
                    r.warning(
                        "missing-sheet-entry",
                        format!("Hierarchical label '{name}' is on a top-level sheet and has no parent sheet entry"),
                        file,
                        vec![*item],
                        Some(*pos),
                    );
                }
            }
        }
    }
}

fn pin_site<'s>(sheets: &'s [SheetRef], conns: &'s [SheetConn], insts: &[Inst], (i, part, pin): (usize, usize, usize)) -> PinSite<'s> {
    let sheet = insts[i].sheet;
    let c = &conns[sheet];
    let pi = &c.parts[part];
    let info = c.pin(part, pin);
    PinSite {
        file: &sheets[sheet].path,
        part: pi.item,
        designator: pi.designator(),
        number: &info.pin.number,
        name: &info.pin.name,
        electrical: info.pin.electrical,
        hidden: info.pin.hidden,
        world: info.world,
        nc_marker: info.nc_marker,
        local_size: c.group_size[c.group[info.elem]],
    }
}
