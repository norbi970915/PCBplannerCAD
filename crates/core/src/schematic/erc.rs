//! Electrical rule checks. Sheet-local checks run once per document; net
//! and designator checks run on the compiled hierarchy.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::document::ItemId;
use crate::item::{LabelScope, PinType};
use crate::units::Point;

use super::connectivity::{ElemKind, SheetConn};
use super::{Severity, Violation, is_unannotated};

#[derive(Default)]
pub(crate) struct Report {
    pub list: Vec<Violation>,
}

impl Report {
    pub fn push(&mut self, severity: Severity, code: &'static str, message: String, sheet: &str, items: Vec<ItemId>, pos: Option<Point>) {
        self.list.push(Violation { severity, code, message, sheet: sheet.to_string(), items, pos });
    }

    pub fn error(&mut self, code: &'static str, message: String, sheet: &str, items: Vec<ItemId>, pos: Option<Point>) {
        self.push(Severity::Error, code, message, sheet, items, pos);
    }

    pub fn warning(&mut self, code: &'static str, message: String, sheet: &str, items: Vec<ItemId>, pos: Option<Point>) {
        self.push(Severity::Warning, code, message, sheet, items, pos);
    }

    /// Drops exact duplicates (e.g. from a sheet instantiated twice).
    pub fn finish(self) -> Vec<Violation> {
        let mut seen = HashSet::new();
        self.list
            .into_iter()
            .filter(|v| seen.insert((v.code, v.sheet.clone(), v.items.clone(), v.message.clone())))
            .collect()
    }
}

/// Checks that only depend on one document's geometry.
pub(crate) fn sheet_checks(file: &str, conn: &SheetConn, r: &mut Report) {
    for p in &conn.parts {
        let des = p.designator();
        match p.cached {
            None => r.error(
                "missing-symbol",
                format!("Part {} refers to '{}', which is not in the sheet's part cache", display_des(des), p.part.lib_ref),
                file,
                vec![p.item],
                Some(p.part.pos),
            ),
            Some(c) if !p.unit_ok => r.error(
                "missing-symbol",
                format!("Part {} uses unit {} but symbol '{}' has {} unit(s)", display_des(des), p.part.unit, c.symbol.name, c.symbol.units.len()),
                file,
                vec![p.item],
                Some(p.part.pos),
            ),
            _ => {}
        }
        if !p.is_power && is_unannotated(des) {
            r.error("unannotated", format!("Part {} is not annotated", display_des(des)), file, vec![p.item], Some(p.part.pos));
        }
    }

    for &(item, pos) in &conn.dangling {
        r.warning("dangling-wire", "Wire end is not connected to anything".into(), file, vec![item], Some(pos));
    }

    for (e, el) in conn.elems.iter().enumerate() {
        let what = match &el.kind {
            ElemKind::Label { scope, name } => format!("{} label '{}'", scope_name(*scope), name),
            ElemKind::Power { name, part: None } => format!("Power port '{name}'"),
            _ => continue,
        };
        if conn.group_size[conn.group[e]] == 1 && conn.touches[e].is_empty() && !conn.on_bus[e] {
            r.warning("unconnected-label", format!("{what} is not connected to any wire or pin"), file, vec![el.item], Some(el.a));
        }
    }
}

fn scope_name(s: LabelScope) -> &'static str {
    match s {
        LabelScope::Local => "Net",
        LabelScope::Global => "Global",
        LabelScope::Hierarchical => "Hierarchical",
    }
}

pub(crate) fn display_des(d: &str) -> String {
    if d.is_empty() { "(no designator)".into() } else { d.to_string() }
}

/// A pin of a placed part, as seen by net checks.
pub(crate) struct PinSite<'a> {
    pub file: &'a str,
    pub part: ItemId,
    pub designator: &'a str,
    pub number: &'a str,
    pub name: &'a str,
    pub electrical: PinType,
    pub hidden: bool,
    pub world: Point,
    pub nc_marker: bool,
    /// Size of the pin's sheet-local group (1 = touches nothing).
    pub local_size: usize,
}

impl PinSite<'_> {
    fn label(&self) -> String {
        if self.name.is_empty() || self.name == self.number {
            format!("{} pin {}", display_des(self.designator), self.number)
        } else {
            format!("{} pin {} ({})", display_des(self.designator), self.number, self.name)
        }
    }
}

/// What a compiled net is made of, beyond its pins.
pub(crate) struct NetFacts<'a> {
    pub name: &'a str,
    pub pins: Vec<PinSite<'a>>,
    pub has_label: bool,
    pub has_port: bool,
    pub has_entry: bool,
}

pub(crate) fn net_checks(net: &NetFacts, r: &mut Report) {
    let attached = net.has_label || net.has_port || net.has_entry;
    let mut single_reported = false;
    for p in &net.pins {
        if p.hidden {
            continue;
        }
        let others = net.pins.len() > 1 || attached;
        if p.nc_marker {
            if others || p.local_size > 1 {
                r.warning(
                    "no-connect-connected",
                    format!("{} has a no-connect marker but is connected", p.label()),
                    p.file,
                    vec![p.part],
                    Some(p.world),
                );
            }
            continue;
        }
        if others {
            continue;
        }
        if p.local_size == 1 {
            let sev = if matches!(p.electrical, PinType::Input | PinType::PowerIn) { Severity::Error } else { Severity::Warning };
            r.push(sev, "unconnected-pin", format!("{} is not connected", p.label()), p.file, vec![p.part], Some(p.world));
        } else if !single_reported {
            single_reported = true;
            r.warning(
                "single-pin-net",
                format!("Net '{}' only connects {}", net.name, p.label()),
                p.file,
                vec![p.part],
                Some(p.world),
            );
        }
    }

    let drivers: Vec<&PinSite> = net.pins.iter().filter(|p| matches!(p.electrical, PinType::Output | PinType::PowerOut)).collect();
    if drivers.len() >= 2 {
        let at = drivers[1];
        let names: Vec<String> = drivers.iter().map(|p| p.label()).collect();
        r.error(
            "output-conflict",
            format!("Net '{}' is driven by several outputs: {}", net.name, names.join(", ")),
            at.file,
            items_on(&drivers, at.file),
            Some(at.world),
        );
    }

    let power_in: Vec<&PinSite> = net.pins.iter().filter(|p| p.electrical == PinType::PowerIn).collect();
    let driven = net.has_port || net.pins.iter().any(|p| p.electrical == PinType::PowerOut);
    if let Some(at) = power_in.first().filter(|_| !driven) {
        r.warning(
            "power-not-driven",
            format!("Power input {} on net '{}' is not driven by a power output or power port", at.label(), net.name),
            at.file,
            items_on(&power_in, at.file),
            Some(at.world),
        );
    }
}

fn items_on(pins: &[&PinSite], file: &str) -> Vec<ItemId> {
    pins.iter().filter(|p| p.file == file).map(|p| p.part).collect::<BTreeSet<_>>().into_iter().collect()
}

/// A placed, annotated unit, for duplicate-designator checks.
pub(crate) struct Placed<'a> {
    pub designator: &'a str,
    pub lib_ref: &'a str,
    pub unit: u32,
    pub file: &'a str,
    pub item: ItemId,
    pub pos: Point,
}

pub(crate) fn designator_checks(placed: &[Placed], r: &mut Report) {
    let mut groups: Vec<(&str, Vec<&Placed>)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for p in placed {
        let i = *index.entry(p.designator).or_insert_with(|| {
            groups.push((p.designator, Vec::new()));
            groups.len() - 1
        });
        groups[i].1.push(p);
    }
    for (des, list) in groups {
        let first = list[0];
        for (k, p) in list.iter().enumerate().skip(1) {
            let message = if p.lib_ref != first.lib_ref {
                format!("Designator {des} is used by different components ({} and {})", first.lib_ref, p.lib_ref)
            } else if list[..k].iter().any(|o| o.lib_ref == p.lib_ref && o.unit == p.unit) {
                format!("Unit {} of {des} is placed more than once", unit_letter(p.unit))
            } else {
                continue;
            };
            let mut items: Vec<ItemId> = list.iter().filter(|o| o.file == p.file).map(|o| o.item).collect();
            items.sort();
            items.dedup();
            r.error("duplicate-designator", message, p.file, items, Some(p.pos));
        }
    }
}

pub(crate) fn unit_letter(unit: u32) -> String {
    let mut n = unit;
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}
