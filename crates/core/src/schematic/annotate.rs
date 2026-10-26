//! Designator annotation.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::document::{IdItem, ItemId};
use crate::geom::{Placement, normalize_deg};
use crate::item::{FIELD_DESIGNATOR, Field, Item, Part};
use crate::library::CachedPart;
use crate::units::Point;

use super::{SheetRef, is_unannotated};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotateMode {
    /// Only parts whose designator is empty or ends in '?'; existing numbers are kept.
    Incremental,
    /// Renumber every part from 1.
    All,
}

struct Entry<'a> {
    sheet: usize,
    id: ItemId,
    part: &'a Part,
    cached: Option<&'a CachedPart>,
    prefix: String,
    multi_unit: bool,
}

/// Splits "R12" into ("R", Some(12)); "R?" into ("R", None).
fn split_designator(d: &str) -> (&str, Option<u32>) {
    let d = d.trim().trim_end_matches('?');
    let digits = d.bytes().rev().take_while(u8::is_ascii_digit).count();
    let (head, tail) = d.split_at(d.len() - digits);
    (head, tail.parse().ok())
}

fn leading_letters(d: &str) -> &str {
    let n = d.bytes().take_while(u8::is_ascii_alphabetic).count();
    &d[..n]
}

/// Assigns designators. Returns, per sheet with changes, full replacement
/// Part items (only the parts whose designator changed).
pub fn annotate(sheets: &[SheetRef], mode: AnnotateMode) -> Vec<(String, Vec<IdItem>)> {
    let mut entries: Vec<Entry> = Vec::new();
    for (si, s) in sheets.iter().enumerate() {
        let mut list: Vec<Entry> = Vec::new();
        for (id, item) in s.doc.items() {
            let Item::Part(p) = item else { continue };
            let cached = s.doc.cached_part(&p.lib_ref);
            if cached.is_some_and(|c| c.symbol.is_power) {
                continue;
            }
            let symbol_prefix = cached.map(|c| c.symbol.prefix.trim()).filter(|p| !p.is_empty());
            let prefix = symbol_prefix
                .or_else(|| Some(leading_letters(p.designator().trim())).filter(|p| !p.is_empty()))
                .unwrap_or("U")
                .to_string();
            let multi_unit = cached.is_some_and(|c| c.symbol.units.len() > 1);
            list.push(Entry { sheet: si, id, part: p, cached, prefix, multi_unit });
        }
        // Top to bottom (world Y up), then left to right.
        list.sort_by_key(|e| (Reverse(e.part.pos.y), e.part.pos.x, e.id));
        entries.extend(list);
    }

    // Numbers in use per prefix, and units placed per (prefix, lib_ref, number).
    let mut used: HashMap<String, BTreeSet<u32>> = HashMap::new();
    let mut packs: HashMap<(String, String), BTreeMap<u32, BTreeSet<u32>>> = HashMap::new();
    let keep = |e: &Entry| mode == AnnotateMode::Incremental && !is_unannotated(e.part.designator());
    for e in entries.iter().filter(|e| keep(e)) {
        let (head, num) = split_designator(e.part.designator());
        if let Some(n) = num {
            used.entry(head.to_string()).or_default().insert(n);
            packs.entry((head.to_string(), e.part.lib_ref.clone())).or_default().entry(n).or_default().insert(e.part.unit);
        }
    }

    let mut changed: BTreeMap<usize, Vec<IdItem>> = BTreeMap::new();
    for e in entries.iter().filter(|e| !keep(e)) {
        let key = (e.prefix.clone(), e.part.lib_ref.clone());
        let packed = if e.multi_unit {
            packs.get(&key).and_then(|m| m.iter().find(|(_, units)| !units.contains(&e.part.unit)).map(|(n, _)| *n))
        } else {
            None
        };
        let n = packed.unwrap_or_else(|| {
            let set = used.entry(e.prefix.clone()).or_default();
            let n = (1..).find(|n| !set.contains(n)).unwrap_or(1);
            set.insert(n);
            n
        });
        packs.entry(key).or_default().entry(n).or_default().insert(e.part.unit);

        let des = format!("{}{n}", e.prefix);
        if e.part.field(FIELD_DESIGNATOR).is_some_and(|f| f.value == des) {
            continue;
        }
        let mut part = e.part.clone();
        if part.field(FIELD_DESIGNATOR).is_some() {
            part.set_field(FIELD_DESIGNATOR, des);
        } else {
            part.fields.push(new_designator_field(&part, e.cached, des));
        }
        changed.entry(e.sheet).or_default().push(IdItem { id: e.id, item: Item::Part(part) });
    }

    changed.into_iter().map(|(s, items)| (sheets[s].path.clone(), items)).collect()
}

/// A visible Designator field placed from the symbol's field template.
fn new_designator_field(part: &Part, cached: Option<&CachedPart>, value: String) -> Field {
    let template = cached.and_then(|c| c.symbol.fields.iter().find(|f| f.name == FIELD_DESIGNATOR));
    match template {
        Some(t) => {
            let orient = Placement { offset: Point::default(), rotation: part.rotation, mirror: part.mirror };
            let horizontal = {
                let d = normalize_deg(t.rotation);
                !(45.0..135.0).contains(&d) && !(225.0..315.0).contains(&d)
            };
            Field {
                name: FIELD_DESIGNATOR.into(),
                value,
                offset: orient.apply(t.offset),
                rotation: normalize_deg(t.rotation + part.rotation),
                height: t.height,
                h_align: if part.mirror && horizontal { t.h_align.flipped() } else { t.h_align },
                v_align: t.v_align,
                visible: true,
            }
        }
        None => {
            let mut tmp = Part { fields: Vec::new(), ..part.clone() };
            tmp.set_field(FIELD_DESIGNATOR, value);
            let mut f = tmp.fields.remove(0);
            f.visible = true;
            f
        }
    }
}
