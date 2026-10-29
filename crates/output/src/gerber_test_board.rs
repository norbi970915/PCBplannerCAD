//! Test fixtures shared by the gerber / excellon / assembly / fab tests.

use std::collections::BTreeMap;

use pcbplannercad_core::document::{DocKind, Document};
use pcbplannercad_core::item::{Item, Line, Pad, PadKind, PadShape};
use pcbplannercad_core::layer;
use pcbplannercad_core::library::Footprint;
use pcbplannercad_core::pcb::{BoardSettings, PlacedFootprint, Side};
use pcbplannercad_core::units::{Point, mm};

/// A PCB document with board settings, a 50×30 mm outline, `items` and the
/// given footprint cache entries.
pub(crate) fn board_doc(items: Vec<Item>, footprints: &[(&str, Footprint)]) -> Document {
    let pts = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
    let mut all = vec![Item::Board(BoardSettings::default())];
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        all.push(Item::Line(Line { a: Point::new(mm(a.0), mm(a.1)), b: Point::new(mm(b.0), mm(b.1)), width: mm(0.1), layer: layer::BOARD_OUTLINE }));
    }
    all.extend(items);
    let mut doc = Document::from_item_list(DocKind::Pcb, all);
    for (k, f) in footprints {
        doc.put_cached_footprint(*k, f.clone());
    }
    doc
}

pub(crate) fn footprint_with(items: Vec<Item>) -> Footprint {
    let mut f = Footprint::new("F");
    f.items = items;
    f
}

pub(crate) fn smd_pad(number: &str, x: f64, w: f64, h: f64, shape: PadShape, rotation: f64) -> Item {
    Item::Pad(Pad {
        number: number.into(),
        pos: Point::new(mm(x), 0),
        rotation,
        shape,
        width: mm(w),
        height: mm(h),
        corner_ratio: 0.25,
        kind: PadKind::Smd,
        layer: layer::TOP_COPPER,
        drill: None,
     copper_shapes: Default::default(),})
}

pub(crate) fn place(designator: &str, key: &str, pos: Point, rotation: f64) -> Item {
    Item::Footprint(PlacedFootprint {
        board_only: false,
        designator: designator.into(),
        value: String::new(),
        footprint: key.into(),
        pos,
        rotation,
        side: Side::Top,
        locked: false,
        pad_nets: BTreeMap::new(),
        component: String::new(),
        parameters: BTreeMap::new(),
    })
}

pub(crate) fn place_full(designator: &str, value: &str, key: &str, pos: Point, rotation: f64, side: Side, params: &[(&str, &str)]) -> Item {
    Item::Footprint(PlacedFootprint {
        board_only: false,
        designator: designator.into(),
        value: value.into(),
        footprint: key.into(),
        pos,
        rotation,
        side,
        locked: false,
        pad_nets: BTreeMap::new(),
        component: String::new(),
        parameters: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    })
}
