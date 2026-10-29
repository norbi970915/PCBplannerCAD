//! Whole-net routed copper measurements. Vias and pad/package transit are excluded.
use std::collections::BTreeMap;
use serde::Serialize;
use crate::{Document, Item, ItemId, Point, units::Coord};
use super::{board::BoardSettings, geometry::board_settings, query::{Obj, ObjKind, RuleSet}};

#[derive(Clone, Debug, Serialize)]
pub struct LengthConstraint {
    pub min: Coord,
    pub max: Coord,
    pub rule: String,
}

/// Length rules resolve on a whole-net track context, with no layer or owner.
/// Net and net-class scopes apply; layer/component scopes do not describe a net total.
pub fn constraint(doc: &Document, net: &str) -> Option<LengthConstraint> {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map(|(_, b)| b).unwrap_or(&fallback);
    RuleSet::new(board).length_rule(&Obj::new(ObjKind::Track, net, None))
        .map(|((min, max), source)| LengthConstraint { min, max, rule: source.to_string() })
}

pub struct Measurement {
    pub total: f64,
    pub items: Vec<ItemId>,
    pub pos: Point,
}

/// One pass, including pad-only nets so an unrouted constrained net measures zero.
pub fn measure(doc: &Document) -> BTreeMap<String, Measurement> {
    let mut out = BTreeMap::<String, Measurement>::new();
    for (id, item) in doc.items() {
        let (nets, total, pos) = match item {
            Item::Track(t) => (vec![t.net.as_str()], ((t.b.x-t.a.x) as f64).hypot((t.b.y-t.a.y) as f64), t.a),
            Item::ArcTrack(a) => (vec![a.net.as_str()], a.radius as f64 * a.sweep.abs().to_radians(), a.center),
            Item::Via(v) => (vec![v.net.as_str()], 0.0, v.pos),
            Item::Footprint(f) => (f.pad_nets.values().map(String::as_str).collect(), 0.0, f.pos),
            _ => continue,
        };
        for net in nets {
            if net.is_empty() { continue; }
            let m = out.entry(net.to_string()).or_insert_with(|| Measurement {total:0.0, items:vec![], pos});
            m.total += total;
            if m.items.last() != Some(&id) { m.items.push(id); }
        }
    }
    out
}
