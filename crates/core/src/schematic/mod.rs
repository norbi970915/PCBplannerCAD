//! Schematic compilation: connectivity, netlist, ERC, annotation, BOM.
//!
//! [`compile`] turns a set of schematic sheets into a hierarchical netlist
//! and a list of electrical-rule violations. Connectivity is resolved per
//! sheet document ([`connectivity`]) and then merged across sheet instances
//! by name (power ports, global/local/hierarchical labels, hidden power pins)
//! in [`netlist`].

mod annotate;
mod bom;
mod connectivity;
mod erc;
mod netlist;
#[cfg(test)]
mod tests;

use std::cmp::Ordering;

use serde::Serialize;

use crate::document::{Document, ItemId};
use crate::item::PinType;
use crate::units::Point;

pub use annotate::{AnnotateMode, annotate};
pub use bom::bom_csv;
pub use connectivity::connected_items;
pub use netlist::compile;

/// One schematic document of the project together with its project-relative path.
#[derive(Clone, Debug)]
pub struct SheetRef<'a> {
    /// Project-relative file path; matched against [`crate::item::Sheet::file`].
    pub path: String,
    pub doc: &'a Document,
}

impl<'a> SheetRef<'a> {
    pub fn new(path: impl Into<String>, doc: &'a Document) -> Self {
        SheetRef { path: path.into(), doc }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compiled {
    /// Nets with at least one part pin, naturally sorted by name.
    pub nets: Vec<Net>,
    /// Netlist parts (units sharing a designator merged), sorted by designator.
    pub parts: Vec<NetlistPart>,
    pub violations: Vec<Violation>,
    /// Sheet instances in depth-first order, roots first.
    pub instances: Vec<SheetInstance>,
}

impl Compiled {
    pub fn net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }

    /// The net a part pin is on (by designator and pin number).
    pub fn net_of(&self, designator: &str, pin_number: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.nodes.iter().any(|p| p.designator == designator && p.pin_number == pin_number))
    }

    pub fn part(&self, designator: &str) -> Option<&NetlistPart> {
        self.parts.iter().find(|p| p.designator == designator)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    pub name: String,
    /// Sorted by designator, then pin number (natural order).
    pub nodes: Vec<NetNode>,
    /// Wires, bus entries, junctions, labels and ports forming the net, as
    /// `(sheet file, item)`; for highlighting.
    pub items: Vec<(String, ItemId)>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetNode {
    pub designator: String,
    pub pin_number: String,
    pub pin_name: String,
    pub pin_type: PinType,
    pub pads: Vec<String>,
    /// Sheet file the part is placed on.
    pub sheet: String,
    pub part: ItemId,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetlistPart {
    pub designator: String,
    pub value: String,
    pub footprint: String,
    pub lib_ref: String,
    pub component: String,
    pub library: String,
    pub description: String,
    /// Placed units as `(sheet file, part item, unit)`.
    pub units: Vec<(String, ItemId, u32)>,
    /// Component parameters overridden by part fields, excluding
    /// Designator/Value/Footprint/Description.
    pub parameters: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    /// Sheet file the violation is located on.
    pub sheet: String,
    pub items: Vec<ItemId>,
    pub pos: Option<Point>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetInstance {
    /// Hierarchical path: "/" for a top-level sheet, "/Power/" for a child.
    pub path: String,
    /// Sheet file of this instance.
    pub sheet: String,
    /// Path of the parent instance.
    pub parent: Option<String>,
    /// The Sheet item (in the parent's file) that instantiates this sheet.
    pub sheet_item: Option<ItemId>,
}

/// Normalizes a project-relative path for comparison.
pub(crate) fn norm_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    let mut s = p.as_str();
    while let Some(rest) = s.strip_prefix("./") {
        s = rest;
    }
    s.to_string()
}

/// Natural ("human") string ordering: digit runs compare numerically, so
/// `R2 < R10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    // Case only breaks ties between otherwise equal strings.
    let mut tie = Ordering::Equal;
    loop {
        match (a.first(), b.first()) {
            (None, None) => return tie,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let la = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let lb = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (da, db) = (&a[..la], &b[..lb]);
                let ta = trim_zeros(da);
                let tb = trim_zeros(db);
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| la.cmp(&lb));
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[la..];
                b = &b[lb..];
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                tie = tie.then(x.cmp(y));
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let n = d.iter().take_while(|c| **c == b'0').count();
    &d[n.min(d.len().saturating_sub(1))..]
}

/// Whether a designator still needs annotation ("" or ending in '?').
pub(crate) fn is_unannotated(designator: &str) -> bool {
    let d = designator.trim();
    d.is_empty() || d.ends_with('?')
}
