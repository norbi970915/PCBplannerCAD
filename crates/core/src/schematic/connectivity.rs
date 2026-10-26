//! Per-sheet connectivity: which wires, pins, labels, ports, junctions and
//! sheet entries of one schematic document touch each other.

use std::collections::{BTreeSet, HashMap};

use crate::document::{Document, ItemId};
use crate::geom::on_segment;
use crate::item::{FIELD_VALUE, Item, LabelScope, Part, Pin, PinType, Sheet};
use crate::library::CachedPart;
use crate::units::{Coord, Point};

// ---------- Union-find ----------

pub(crate) struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect(), rank: vec![0; n] }
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

// ---------- Spatial index for segments ----------

const CELL: Coord = 5_080_000;
const MAX_CELLS: i64 = 4096;

#[derive(Default)]
struct SegIndex {
    cells: HashMap<(i64, i64), Vec<usize>>,
    big: Vec<usize>,
}

impl SegIndex {
    fn cell(p: Point) -> (i64, i64) {
        (p.x.div_euclid(CELL), p.y.div_euclid(CELL))
    }

    fn insert(&mut self, idx: usize, a: Point, b: Point) {
        let (c0, c1) = (Self::cell(Point::new(a.x.min(b.x), a.y.min(b.y))), Self::cell(Point::new(a.x.max(b.x), a.y.max(b.y))));
        if (c1.0 - c0.0 + 1).saturating_mul(c1.1 - c0.1 + 1) > MAX_CELLS {
            self.big.push(idx);
            return;
        }
        for x in c0.0..=c1.0 {
            for y in c0.1..=c1.1 {
                self.cells.entry((x, y)).or_default().push(idx);
            }
        }
    }

    fn query(&self, p: Point) -> impl Iterator<Item = usize> + '_ {
        self.cells.get(&Self::cell(p)).into_iter().flatten().chain(self.big.iter()).copied()
    }
}

// ---------- Elements ----------

#[derive(Clone, Debug)]
pub(crate) enum ElemKind<'a> {
    Wire,
    BusEntry,
    Label { scope: LabelScope, name: &'a str },
    /// A power port, or a pin of a power-symbol part (`part` is then set).
    Power { name: String, part: Option<usize> },
    Junction,
    NoConnect,
    /// Pin `pin` of `parts[part]`.
    Pin { part: usize, pin: usize },
    /// An entry of a sheet symbol (see [`SheetInfo::entries`]).
    SheetEntry,
}

#[derive(Clone, Debug)]
pub(crate) struct Elem<'a> {
    pub item: ItemId,
    pub kind: ElemKind<'a>,
    /// Anchor point (or segment start).
    pub a: Point,
    /// Segment end for wires and bus entries.
    pub b: Option<Point>,
    /// Isolated elements (no-connect markers, no-connect pins) never join
    /// groups; what they touch is recorded in [`SheetConn::touches`].
    pub isolated: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PinInfo<'a> {
    pub pin: &'a Pin,
    pub world: Point,
    pub elem: usize,
    /// A NoConnect marker sits on this pin.
    pub nc_marker: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PartInfo<'a> {
    pub item: ItemId,
    pub part: &'a Part,
    pub cached: Option<&'a CachedPart>,
    /// The unit index is valid for the cached symbol.
    pub unit_ok: bool,
    pub is_power: bool,
    pub pins: Vec<PinInfo<'a>>,
}

impl PartInfo<'_> {
    pub fn designator(&self) -> &str {
        self.part.designator().trim()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SheetInfo<'a> {
    pub item: ItemId,
    pub sheet: &'a Sheet,
    /// Element index for each of `sheet.entries`.
    pub entries: Vec<usize>,
}

/// Connectivity of one schematic document.
pub(crate) struct SheetConn<'a> {
    pub elems: Vec<Elem<'a>>,
    /// Element → group index (`0..group_count`).
    pub group: Vec<usize>,
    pub group_count: usize,
    pub group_size: Vec<usize>,
    /// For each element: the elements it touches through an isolated element.
    pub touches: Vec<Vec<usize>>,
    pub parts: Vec<PartInfo<'a>>,
    pub sheets: Vec<SheetInfo<'a>>,
    /// Wire endpoints touching nothing.
    pub dangling: Vec<(ItemId, Point)>,
    /// Point elements lying on a bus.
    pub on_bus: Vec<bool>,
}

impl<'a> SheetConn<'a> {
    pub fn build(doc: &'a Document) -> SheetConn<'a> {
        let mut elems: Vec<Elem<'a>> = Vec::new();
        let mut parts: Vec<PartInfo<'a>> = Vec::new();
        let mut sheets: Vec<SheetInfo<'a>> = Vec::new();
        let mut buses: Vec<(Point, Point)> = Vec::new();

        let point = |item: ItemId, kind: ElemKind<'a>, a: Point, isolated: bool| Elem { item, kind, a, b: None, isolated };
        for (id, item) in doc.items() {
            match item {
                Item::Wire(w) => elems.push(Elem { item: id, kind: ElemKind::Wire, a: w.a, b: Some(w.b), isolated: false }),
                Item::BusEntry(e) => elems.push(Elem {
                    item: id,
                    kind: ElemKind::BusEntry,
                    a: e.pos,
                    b: Some(e.pos.translated(e.size.x, e.size.y)),
                    isolated: false,
                }),
                Item::Bus(b) => buses.push((b.a, b.b)),
                Item::Junction(j) => elems.push(point(id, ElemKind::Junction, j.pos, false)),
                Item::NoConnect(n) => elems.push(point(id, ElemKind::NoConnect, n.pos, true)),
                Item::NetLabel(l) => elems.push(point(id, ElemKind::Label { scope: l.scope, name: l.text.trim() }, l.pos, false)),
                Item::PowerPort(p) => elems.push(point(id, ElemKind::Power { name: p.net.trim().to_string(), part: None }, p.pos, false)),
                Item::Part(p) => {
                    let cached = doc.cached_part(&p.lib_ref);
                    let unit_ok = cached.is_some_and(|c| p.unit >= 1 && (p.unit as usize) <= c.symbol.units.len());
                    let is_power = cached.is_some_and(|c| c.symbol.is_power);
                    // Flag symbols (PWR_FLAG) keep real pins: they drive a net, not name it.
                    let names_net = cached.is_some_and(|c| c.symbol.names_global_net());
                    let pi = parts.len();
                    let place = p.placement();
                    let mut pins = Vec::new();
                    if let (Some(c), true) = (cached, unit_ok) {
                        for pin in c.symbol.unit_pins(p.unit) {
                            let world = place.apply(pin.pos);
                            let elem = elems.len();
                            if names_net {
                                let value = p.field_value(FIELD_VALUE).trim();
                                let name = if value.is_empty() { pin.name.trim() } else { value };
                                elems.push(point(id, ElemKind::Power { name: name.to_string(), part: Some(pi) }, world, false));
                            } else {
                                let kind = ElemKind::Pin { part: pi, pin: pins.len() };
                                elems.push(point(id, kind, world, pin.electrical == PinType::NoConnect));
                            }
                            pins.push(PinInfo { pin, world, elem, nc_marker: false });
                        }
                    }
                    parts.push(PartInfo { item: id, part: p, cached, unit_ok, is_power, pins });
                }
                Item::Sheet(s) => {
                    let mut entries = Vec::new();
                    for e in &s.entries {
                        entries.push(elems.len());
                        elems.push(point(id, ElemKind::SheetEntry, s.entry_point(e), false));
                    }
                    sheets.push(SheetInfo { item: id, sheet: s, entries });
                }
                _ => {}
            }
        }

        let n = elems.len();
        let mut uf = UnionFind::new(n);
        let mut touches: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut connect = |uf: &mut UnionFind, x: usize, y: usize| {
            if x == y {
                return;
            }
            if elems[x].isolated || elems[y].isolated {
                touches[x].push(y);
                touches[y].push(x);
            } else {
                uf.union(x, y);
            }
        };

        let mut index = SegIndex::default();
        let segs: Vec<usize> = (0..n).filter(|&i| elems[i].b.is_some()).collect();
        for &s in &segs {
            index.insert(s, elems[s].a, elems[s].b.unwrap_or(elems[s].a));
        }
        // End touched flags per segment element: [a, b].
        let mut touched: HashMap<usize, [bool; 2]> = segs.iter().map(|&s| (s, [false; 2])).collect();

        // Segment endpoints on other segments (endpoint-endpoint and T-junctions).
        for &s in &segs {
            let ends = [elems[s].a, elems[s].b.unwrap_or(elems[s].a)];
            for (k, e) in ends.into_iter().enumerate() {
                for t in index.query(e) {
                    if t != s && on_segment(e, elems[t].a, elems[t].b.unwrap_or(elems[t].a)) {
                        connect(&mut uf, s, t);
                        if let Some(f) = touched.get_mut(&s) {
                            f[k] = true;
                        }
                    }
                }
            }
        }

        // Point anchors on segments, and point anchors at the same position.
        let mut at: HashMap<Point, Vec<usize>> = HashMap::new();
        for i in 0..n {
            if elems[i].b.is_some() {
                continue;
            }
            let p = elems[i].a;
            at.entry(p).or_default().push(i);
            for t in index.query(p) {
                let (a, b) = (elems[t].a, elems[t].b.unwrap_or(elems[t].a));
                if on_segment(p, a, b) {
                    connect(&mut uf, i, t);
                    if let Some(f) = touched.get_mut(&t) {
                        f[0] |= p == a;
                        f[1] |= p == b;
                    }
                }
            }
        }
        let mut buckets: Vec<&Vec<usize>> = at.values().filter(|v| v.len() > 1).collect();
        buckets.sort();
        for bucket in buckets {
            for (k, &x) in bucket.iter().enumerate() {
                for &y in &bucket[k + 1..] {
                    connect(&mut uf, x, y);
                }
            }
        }
        for t in &mut touches {
            t.sort_unstable();
            t.dedup();
        }

        // No-connect markers flag the pins they sit on.
        for i in 0..n {
            if !matches!(elems[i].kind, ElemKind::NoConnect) {
                continue;
            }
            for &t in &touches[i] {
                if let ElemKind::Pin { part, pin } = elems[t].kind {
                    parts[part].pins[pin].nc_marker = true;
                }
            }
        }

        // Dangling wire ends (bus entries are exempt: one end sits on a bus).
        let mut dangling = Vec::new();
        for &s in &segs {
            if !matches!(elems[s].kind, ElemKind::Wire) {
                continue;
            }
            let f = touched[&s];
            if !f[0] {
                dangling.push((elems[s].item, elems[s].a));
            }
            if !f[1] {
                dangling.push((elems[s].item, elems[s].b.unwrap_or(elems[s].a)));
            }
        }

        let on_bus = elems.iter().map(|e| e.b.is_none() && buses.iter().any(|&(a, b)| on_segment(e.a, a, b))).collect();

        // Compact group indices.
        let mut map: HashMap<usize, usize> = HashMap::new();
        let mut group = Vec::with_capacity(n);
        for i in 0..n {
            let r = uf.find(i);
            let next = map.len();
            group.push(*map.entry(r).or_insert(next));
        }
        let group_count = map.len();
        let mut group_size = vec![0; group_count];
        for &g in &group {
            group_size[g] += 1;
        }

        SheetConn { elems, group, group_count, group_size, touches, parts, sheets, dangling, on_bus }
    }

    pub fn pin(&self, part: usize, pin: usize) -> &PinInfo<'a> {
        &self.parts[part].pins[pin]
    }

    /// The name key an element contributes for name-based merging, if any.
    /// `inst` scopes local and hierarchical names.
    pub fn name_key(&self, e: usize, inst: usize) -> Option<String> {
        let el = &self.elems[e];
        if el.isolated {
            return None;
        }
        match &el.kind {
            ElemKind::Label { scope: LabelScope::Global, name } if !name.is_empty() => Some(global_key(name)),
            ElemKind::Label { scope: LabelScope::Local, name } if !name.is_empty() => Some(format!("l\u{0}{inst}\u{0}{name}")),
            ElemKind::Label { scope: LabelScope::Hierarchical, name } if !name.is_empty() => Some(hier_key(inst, name)),
            ElemKind::Power { name, .. } if !name.is_empty() => Some(global_key(name)),
            ElemKind::Pin { part, pin } => {
                let p = self.pin(*part, *pin).pin;
                (p.hidden && p.electrical == PinType::PowerIn && !p.name.trim().is_empty()).then(|| global_key(p.name.trim()))
            }
            _ => None,
        }
    }
}

pub(crate) fn global_key(name: &str) -> String {
    format!("g\u{0}{name}")
}

pub(crate) fn hier_key(inst: usize, name: &str) -> String {
    format!("h\u{0}{inst}\u{0}{name}")
}

/// Adds group `g` to the set named `key`.
pub(crate) fn join_key(uf: &mut UnionFind, keys: &mut HashMap<String, usize>, key: String, g: usize) {
    match keys.get(&key) {
        Some(&other) => uf.union(other, g),
        None => {
            keys.insert(key, g);
        }
    }
}

/// Item ids on one sheet electrically connected to `item` (including the
/// item itself), for net highlighting. Same-named labels and power ports on
/// the sheet count as connected; other sheets are not considered.
pub fn connected_items(sheet: &Document, item: ItemId) -> Vec<ItemId> {
    let conn = SheetConn::build(sheet);
    let mut uf = UnionFind::new(conn.group_count);
    let mut keys = HashMap::new();
    for e in 0..conn.elems.len() {
        if let Some(k) = conn.name_key(e, 0) {
            join_key(&mut uf, &mut keys, k, conn.group[e]);
        }
    }
    let mut roots = BTreeSet::new();
    for (e, el) in conn.elems.iter().enumerate() {
        if el.item != item {
            continue;
        }
        if el.isolated {
            for &t in &conn.touches[e] {
                if !conn.elems[t].isolated {
                    roots.insert(uf.find(conn.group[t]));
                }
            }
        } else {
            roots.insert(uf.find(conn.group[e]));
        }
    }
    if roots.is_empty() {
        return Vec::new();
    }
    let mut out = BTreeSet::new();
    for (e, el) in conn.elems.iter().enumerate() {
        let hit = if el.isolated {
            conn.touches[e].iter().any(|&t| !conn.elems[t].isolated && roots.contains(&uf.find(conn.group[t])))
        } else {
            roots.contains(&uf.find(conn.group[e]))
        };
        if hit {
            out.insert(el.item);
        }
    }
    out.insert(item);
    out.into_iter().collect()
}
