//! Obstacle world of one copper layer: every copper object as a "core"
//! (segment, convex polygon or polygon edge) grown by a radius, in an R-tree.
//! Filled areas (zone fills, keep-outs, the board outline) are stored as edges
//! plus an [`Area`] record for inside tests (ray casting through the tree).
//!
//! Clearances are looked up lazily through [`RuleSet::clearance`] and memoized
//! per (obstacle rule key, querying net), so the rule evaluator runs a handful
//! of times per call instead of per distance test.
//!
//! The shove engine overlays edits on the world: `removed` obstacles are
//! skipped and `added` segments (shoved track pieces) are scanned linearly.

use std::cell::RefCell;
use std::sync::OnceLock;

use rstar::{AABB, RTree, RTreeObject};

use crate::document::{Document, ItemId, LayerId};
use crate::geom::rotate;
use crate::item::{Item, PadKind, PadShape};
use crate::layer;
use crate::pcb::board::{BoardSettings, RuleKind};
use crate::pcb::geometry::{all_pads, board_settings, outline_loops, WorldPad};
use crate::pcb::items::Track;
use crate::pcb::query::{Obj, ObjKind, RuleSet};
use crate::units::{mm, Coord, Point};
use super::geom::{point_poly_dist, point_seg_dist, seg_poly_dist, seg_seg_dist, FxMap, FxSet, V};

/// Net id used for unconnected copper: never "same net" as anything.
pub(super) const NO_NET: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObKind {
    Track,
    Arc,
    Via,
    Pad,
    Zone,
    Keepout,
    Outline,
}

impl ObKind {
    pub fn name(self) -> &'static str {
        match self {
            ObKind::Track => "track",
            ObKind::Arc => "arc",
            ObKind::Via => "via",
            ObKind::Pad => "pad",
            ObKind::Zone => "zone",
            ObKind::Keepout => "keep-out",
            ObKind::Outline => "board outline",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Core {
    Seg(V, V),
    /// Convex polygon.
    Poly(Vec<V>),
    /// Edge of area `.2`.
    Edge(V, V, u32),
}

impl Core {
    fn bbox(&self) -> (V, V) {
        let pts: Vec<V> = match self {
            Core::Seg(a, b) | Core::Edge(a, b, _) => vec![*a, *b],
            Core::Poly(p) => p.clone(),
        };
        let mut lo = V::new(f64::INFINITY, f64::INFINITY);
        let mut hi = V::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in pts {
            lo = V::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = V::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (lo, hi)
    }

    pub fn point_dist(&self, p: V) -> f64 {
        match self {
            Core::Seg(a, b) | Core::Edge(a, b, _) => point_seg_dist(p, *a, *b),
            Core::Poly(poly) => point_poly_dist(p, poly),
        }
    }

    pub fn seg_dist(&self, a: V, b: V) -> f64 {
        match self {
            Core::Seg(c, d) | Core::Edge(c, d, _) => seg_seg_dist(a, b, *c, *d),
            Core::Poly(poly) => seg_poly_dist(a, b, poly),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Obstacle {
    pub core: Core,
    /// Copper radius around the core (half width).
    pub r: f64,
    pub net: u32,
    /// Rule key (index into `World::keys`).
    pub key: u32,
    pub kind: ObKind,
    pub id: Option<ItemId>,
    /// Original track (shovable obstacles only).
    pub track: Option<Track>,
}

#[derive(Clone, Debug)]
pub(super) struct Area {
    pub lo: V,
    pub hi: V,
    /// Board outline: being *outside* is the violation.
    pub invert: bool,
    pub net: u32,
    pub kind: ObKind,
}

/// Facts the clearance rules need about an obstacle.
#[derive(Clone, Debug)]
pub(super) struct RuleKey {
    pub kind: ObjKind,
    pub special: Option<ObKind>,
    pub net: String,
    pub designator: Option<String>,
    pub footprint: Option<String>,
    pub smd: bool,
}

struct Entry {
    env: AABB<[f64; 2]>,
    idx: u32,
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

/// What is asking: a copper centerline of half width `hw` on net `net`
/// (for clearance lookups); nets in `excl` are not obstacles.
#[derive(Clone, Copy, Debug)]
pub(super) struct Probe<'x> {
    pub hw: f64,
    pub net: u32,
    pub excl: &'x [u32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    Ob(u32),
    Added(u32),
    Area(u32),
}

pub(super) struct World<'a> {
    pub rules: RuleSet<'a>,
    pub layer: LayerId,
    pub obstacles: Vec<Obstacle>,
    pub areas: Vec<Area>,
    tree: RTree<Entry>,
    keys: Vec<RuleKey>,
    nets: FxMap<String, u32>,
    net_names: Vec<String>,
    memo: RefCell<FxMap<(u32, u32), f64>>,
    pub max_clr: f64,
    pub board_box: Option<(V, V)>,
    pub removed: FxSet<u32>,
    pub added: Vec<Obstacle>,
}

fn default_board() -> &'static BoardSettings {
    static B: OnceLock<BoardSettings> = OnceLock::new();
    B.get_or_init(BoardSettings::default)
}

pub(super) fn board_of(doc: &Document) -> &BoardSettings {
    match board_settings(doc) {
        Some((_, b)) => b,
        None => default_board(),
    }
}

impl<'a> World<'a> {
    /// Indexes every copper obstacle of `layer`, skipping `ignore` (also
    /// matches footprint ids, which skips their pads).
    pub fn build(doc: &'a Document, layer_id: LayerId, ignore: &[ItemId]) -> World<'a> {
        let board = board_of(doc);
        let mut w = World {
            rules: RuleSet::new(board),
            layer: layer_id,
            obstacles: Vec::new(),
            areas: Vec::new(),
            tree: RTree::new(),
            keys: Vec::new(),
            nets: FxMap::default(),
            net_names: Vec::new(),
            memo: RefCell::new(FxMap::default()),
            max_clr: 0.0,
            board_box: None,
            removed: FxSet::default(),
            added: Vec::new(),
        };
        let mut simple_keys: FxMap<(u8, u32), u32> = FxMap::default();
        let ignored: FxSet<ItemId> = ignore.iter().copied().collect();
        let mut zone_extra: Coord = 0;
        for (id, item) in doc.items() {
            if ignored.contains(&id) {
                continue;
            }
            match item {
                Item::Track(t) if t.layer == layer_id => {
                    let net = w.net_id(&t.net);
                    let key = w.simple_key(&mut simple_keys, ObjKind::Track, net);
                    w.obstacles.push(Obstacle {
                        core: Core::Seg(V::of(t.a), V::of(t.b)),
                        r: t.width as f64 / 2.0,
                        net,
                        key,
                        kind: ObKind::Track,
                        id: Some(id),
                        track: (!t.locked).then(|| t.clone()),
                    });
                }
                Item::ArcTrack(a) if a.layer == layer_id => {
                    let net = w.net_id(&a.net);
                    let key = w.simple_key(&mut simple_keys, ObjKind::Arc, net);
                    let n = ((a.sweep.abs() / 5.0).ceil() as usize).max(1);
                    let step = a.sweep / n as f64;
                    let sag = a.radius as f64 * (1.0 - (step.abs() / 2.0).to_radians().cos());
                    let pt = |deg: f64| {
                        let (s, c) = deg.to_radians().sin_cos();
                        V::new(a.center.x as f64 + a.radius as f64 * c, a.center.y as f64 + a.radius as f64 * s)
                    };
                    for i in 0..n {
                        let (p, q) = (pt(a.start_angle + step * i as f64), pt(a.start_angle + step * (i + 1) as f64));
                        w.obstacles.push(Obstacle { core: Core::Seg(p, q), r: a.width as f64 / 2.0 + sag, net, key, kind: ObKind::Arc, id: Some(id), track: None });
                    }
                }
                Item::Via(v) if board.stackup.span(v.from_layer, v.to_layer).contains(&layer_id) => {
                    let net = w.net_id(&v.net);
                    let key = w.simple_key(&mut simple_keys, ObjKind::Via, net);
                    let p = V::of(v.pos);
                    w.obstacles.push(Obstacle { core: Core::Seg(p, p), r: v.diameter as f64 / 2.0, net, key, kind: ObKind::Via, id: Some(id), track: None });
                }
                Item::Zone(z) if z.keepout && (z.layer == layer_id || z.layer == layer::KEEP_OUT || z.layer == layer::MULTI_LAYER) => {
                    let key = w.special_key(ObKind::Keepout);
                    w.add_area(std::slice::from_ref(&z.outline), false, NO_NET, key, ObKind::Keepout, Some(id));
                }
                Item::Zone(z) if !z.keepout && z.layer == layer_id => {
                    let net = w.net_id(&z.net);
                    let key = w.simple_key(&mut simple_keys, ObjKind::Zone, net);
                    zone_extra = zone_extra.max(z.clearance);
                    for poly in &z.fill {
                        let mut loops = vec![poly.outer.clone()];
                        loops.extend(poly.holes.iter().cloned());
                        w.add_area(&loops, false, net, key, ObKind::Zone, Some(id));
                    }
                }
                Item::Line(l) if l.layer == layer::KEEP_OUT => {
                    let key = w.special_key(ObKind::Keepout);
                    w.obstacles.push(Obstacle { core: Core::Seg(V::of(l.a), V::of(l.b)), r: l.width as f64 / 2.0, net: NO_NET, key, kind: ObKind::Keepout, id: Some(id), track: None });
                }
                Item::Polyline(p) if p.layer == layer::KEEP_OUT => {
                    let key = w.special_key(ObKind::Keepout);
                    let n = p.points.len();
                    let edges = if p.closed { n } else { n.saturating_sub(1) };
                    for i in 0..edges {
                        let (a, b) = (V::of(p.points[i]), V::of(p.points[(i + 1) % n]));
                        w.obstacles.push(Obstacle { core: Core::Seg(a, b), r: p.width as f64 / 2.0, net: NO_NET, key, kind: ObKind::Keepout, id: Some(id), track: None });
                    }
                }
                _ => {}
            }
        }
        for pad in all_pads(doc, board) {
            if ignored.contains(&pad.owner) {
                continue;
            }
            let on = pad.on_layer(layer_id) || (pad.kind == PadKind::Npth && board.stackup.is_copper(layer_id));
            if on {
                w.add_pad(&pad);
            }
        }
        let loops = outline_loops(doc, 1000);
        if !loops.is_empty() {
            let key = w.special_key(ObKind::Outline);
            for (i, lp) in loops.iter().enumerate() {
                w.add_area(std::slice::from_ref(lp), i == 0, NO_NET, key, ObKind::Outline, None);
            }
            let a = &w.areas[w.areas.len() - loops.len()];
            w.board_box = Some((a.lo, a.hi));
        }
        // Largest clearance any rule can produce (bounds the query envelopes).
        let mut max_clr: Coord = mm(0.2);
        for r in &board.rules {
            match &r.kind {
                RuleKind::Clearance { min, .. } => max_clr = max_clr.max(*min),
                RuleKind::BoardOutlineClearance { min } => max_clr = max_clr.max(*min),
                _ => {}
            }
        }
        for c in &board.net_classes {
            max_clr = max_clr.max(c.clearance);
        }
        w.max_clr = (max_clr + zone_extra) as f64;
        let entries: Vec<Entry> = w
            .obstacles
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let (lo, hi) = o.core.bbox();
                Entry { env: AABB::from_corners([lo.x - o.r, lo.y - o.r], [hi.x + o.r, hi.y + o.r]), idx: i as u32 }
            })
            .collect();
        w.tree = RTree::bulk_load(entries);
        w
    }

    pub fn net_id(&mut self, net: &str) -> u32 {
        if net.is_empty() {
            return NO_NET;
        }
        if let Some(&id) = self.nets.get(net) {
            return id;
        }
        let id = self.net_names.len() as u32;
        self.net_names.push(net.to_string());
        self.nets.insert(net.to_string(), id);
        id
    }

    pub fn net_name(&self, id: u32) -> &str {
        self.net_names.get(id as usize).map_or("", |s| s.as_str())
    }

    fn simple_key(&mut self, cache: &mut FxMap<(u8, u32), u32>, kind: ObjKind, net: u32) -> u32 {
        let tag = kind as u8;
        if let Some(&k) = cache.get(&(tag, net)) {
            return k;
        }
        let k = self.keys.len() as u32;
        let name = self.net_name(net).to_string();
        self.keys.push(RuleKey { kind, special: None, net: name, designator: None, footprint: None, smd: false });
        cache.insert((tag, net), k);
        k
    }

    fn special_key(&mut self, kind: ObKind) -> u32 {
        if let Some(i) = self.keys.iter().position(|k| k.special == Some(kind)) {
            return i as u32;
        }
        self.keys.push(RuleKey { kind: ObjKind::Zone, special: Some(kind), net: String::new(), designator: None, footprint: None, smd: false });
        (self.keys.len() - 1) as u32
    }

    fn add_area(&mut self, loops: &[Vec<Point>], invert: bool, net: u32, key: u32, kind: ObKind, id: Option<ItemId>) {
        let ai = self.areas.len() as u32;
        let mut lo = V::new(f64::INFINITY, f64::INFINITY);
        let mut hi = V::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for lp in loops {
            let n = lp.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let (a, b) = (V::of(lp[i]), V::of(lp[(i + 1) % n]));
                lo = V::new(lo.x.min(a.x), lo.y.min(a.y));
                hi = V::new(hi.x.max(a.x), hi.y.max(a.y));
                if a != b {
                    self.obstacles.push(Obstacle { core: Core::Edge(a, b, ai), r: 0.0, net, key, kind, id, track: None });
                }
            }
        }
        if lo.x.is_finite() {
            self.areas.push(Area { lo, hi, invert, net, kind });
        }
    }

    fn add_pad(&mut self, pad: &WorldPad) {
        let net = self.net_id(&pad.net);
        let key = self.keys.len() as u32;
        self.keys.push(RuleKey {
            kind: ObjKind::Pad,
            special: None,
            net: pad.net.clone(),
            designator: Some(pad.designator.clone()),
            footprint: None,
            smd: pad.kind == PadKind::Smd,
        });
        let c = V::of(pad.pos);
        let (core, r) = match pad.shape {
            PadShape::Circle => (Core::Seg(c, c), pad.width as f64 / 2.0),
            PadShape::Oval => {
                let (long, short) = (pad.width.max(pad.height), pad.width.min(pad.height));
                let along = if pad.width >= pad.height { 0.0 } else { 90.0 } + pad.rotation;
                let half = (long - short) / 2;
                let a = rotate(pad.pos.translated(-half, 0), pad.pos, along);
                let b = rotate(pad.pos.translated(half, 0), pad.pos, along);
                (Core::Seg(V::of(a), V::of(b)), short as f64 / 2.0)
            }
            PadShape::Rect | PadShape::RoundRect => {
                let rr = if pad.shape == PadShape::RoundRect { (pad.width.min(pad.height) as f64 * pad.corner_ratio.clamp(0.0, 0.5)).max(0.0) } else { 0.0 };
                let (hw, hh) = (pad.width as f64 / 2.0 - rr, pad.height as f64 / 2.0 - rr);
                let (s, co) = pad.rotation.to_radians().sin_cos();
                let poly = [(hw, hh), (-hw, hh), (-hw, -hh), (hw, -hh)].iter().map(|&(x, y)| V::new(c.x + x * co - y * s, c.y + x * s + y * co)).collect();
                (Core::Poly(poly), rr)
            }
        };
        let (core, r) = if pad.kind == PadKind::Npth && pad.copper.is_empty() {
            // Bare hole: keep clear of the drill (or the pad outline if larger).
            let dr = pad.drill.map_or(0.0, |d| d.width.max(d.height) as f64 / 2.0);
            if dr > r && matches!(core, Core::Seg(..)) { (core, dr) } else { (core, r) }
        } else {
            (core, r)
        };
        self.obstacles.push(Obstacle { core, r, net, key, kind: ObKind::Pad, id: Some(pad.owner), track: None });
    }

    /// Clearance between an obstacle (rule key) and copper of net `qnet`.
    pub fn clr(&self, key: u32, qnet: u32) -> f64 {
        if let Some(&c) = self.memo.borrow().get(&(key, qnet)) {
            return c;
        }
        let k = &self.keys[key as usize];
        let c = match k.special {
            Some(ObKind::Outline) => self.rules.outline_clearance() as f64,
            Some(ObKind::Keepout) => 0.0,
            _ => {
                let me = Obj::new(ObjKind::Track, self.net_name(qnet), Some(self.layer));
                let other = Obj { kind: k.kind, net: &k.net, layer: Some(self.layer), designator: k.designator.as_deref(), footprint: k.footprint.as_deref(), smd: k.smd };
                self.rules.clearance(&me, &other) as f64
            }
        };
        self.memo.borrow_mut().insert((key, qnet), c);
        c
    }

    fn skip(&self, net: u32, excl: &[u32]) -> bool {
        net != NO_NET && excl.contains(&net)
    }

    /// Even-odd containment in area `ai` by casting a ray to +X.
    pub fn in_area(&self, ai: u32, p: V) -> bool {
        let a = &self.areas[ai as usize];
        if p.x < a.lo.x || p.x > a.hi.x || p.y < a.lo.y || p.y > a.hi.y {
            return false;
        }
        let env = AABB::from_corners([p.x, p.y], [a.hi.x + 1.0, p.y]);
        let mut inside = false;
        for e in self.tree.locate_in_envelope_intersecting(env) {
            if let Core::Edge(e0, e1, k) = &self.obstacles[e.idx as usize].core
                && *k == ai
                && ((e0.y > p.y) != (e1.y > p.y))
            {
                let x = e0.x + (p.y - e0.y) * (e1.x - e0.x) / (e1.y - e0.y);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    fn area_violated(&self, ai: u32, p: V) -> bool {
        let a = &self.areas[ai as usize];
        self.in_area(ai, p) != a.invert
    }

    /// Smallest `distance - required` from point `p` to any obstacle, capped
    /// at `cap`; negative means the point violates clearance.
    pub fn point_slack(&self, p: V, probe: &Probe, cap: f64) -> f64 {
        let reach = probe.hw + self.max_clr + cap;
        let env = AABB::from_corners([p.x - reach, p.y - reach], [p.x + reach, p.y + reach]);
        let mut best = cap;
        for e in self.tree.locate_in_envelope_intersecting(env) {
            let o = &self.obstacles[e.idx as usize];
            if self.skip(o.net, probe.excl) || (!self.removed.is_empty() && self.removed.contains(&e.idx)) {
                continue;
            }
            let req = probe.hw + o.r + self.clr(o.key, probe.net);
            let d = o.core.point_dist(p) - req;
            if d < best {
                best = d;
                if best < 0.0 {
                    return best;
                }
            }
        }
        for o in &self.added {
            if self.skip(o.net, probe.excl) {
                continue;
            }
            let req = probe.hw + o.r + self.clr(o.key, probe.net);
            best = best.min(o.core.point_dist(p) - req);
        }
        if best < 0.0 {
            return best;
        }
        for (ai, a) in self.areas.iter().enumerate() {
            if self.skip(a.net, probe.excl) {
                continue;
            }
            if (a.invert || (p.x >= a.lo.x && p.x <= a.hi.x && p.y >= a.lo.y && p.y <= a.hi.y)) && self.area_violated(ai as u32, p) {
                return -1.0;
            }
        }
        best
    }

    /// Calls `f` for every obstacle the segment (centerline `a`–`b`, half
    /// width `probe.hw`) violates; `f` returns `true` to stop early.
    pub fn seg_hits(&self, a: V, b: V, probe: &Probe, f: &mut dyn FnMut(Hit) -> bool) {
        let reach = probe.hw + self.max_clr;
        let env = AABB::from_corners([a.x.min(b.x) - reach, a.y.min(b.y) - reach], [a.x.max(b.x) + reach, a.y.max(b.y) + reach]);
        for e in self.tree.locate_in_envelope_intersecting(env) {
            let o = &self.obstacles[e.idx as usize];
            if self.skip(o.net, probe.excl) || (!self.removed.is_empty() && self.removed.contains(&e.idx)) {
                continue;
            }
            let req = probe.hw + o.r + self.clr(o.key, probe.net);
            let d = o.core.seg_dist(a, b);
            if d < req || (d == 0.0 && matches!(o.core, Core::Edge(..))) {
                let hit = match o.core {
                    Core::Edge(_, _, ai) => Hit::Area(ai),
                    _ => Hit::Ob(e.idx),
                };
                if f(hit) {
                    return;
                }
            }
        }
        for (i, o) in self.added.iter().enumerate() {
            if self.skip(o.net, probe.excl) {
                continue;
            }
            let req = probe.hw + o.r + self.clr(o.key, probe.net);
            if o.core.seg_dist(a, b) < req && f(Hit::Added(i as u32)) {
                return;
            }
        }
        let mid = (a + b) * 0.5;
        let (lo, hi) = (V::new(a.x.min(b.x), a.y.min(b.y)), V::new(a.x.max(b.x), a.y.max(b.y)));
        for (ai, ar) in self.areas.iter().enumerate() {
            if self.skip(ar.net, probe.excl) {
                continue;
            }
            let overlaps = hi.x >= ar.lo.x && lo.x <= ar.hi.x && hi.y >= ar.lo.y && lo.y <= ar.hi.y;
            if (ar.invert || overlaps) && self.area_violated(ai as u32, mid) && f(Hit::Area(ai as u32)) {
                return;
            }
        }
    }

    pub fn seg_clear(&self, a: V, b: V, probe: &Probe) -> bool {
        let mut clear = true;
        self.seg_hits(a, b, probe, &mut |_| {
            clear = false;
            true
        });
        clear
    }

    pub fn path_clear(&self, pts: &[Point], probe: &Probe) -> bool {
        if pts.len() == 1 {
            return self.point_slack(V::of(pts[0]), probe, 1.0) >= 0.0;
        }
        pts.windows(2).all(|w| self.seg_clear(V::of(w[0]), V::of(w[1]), probe))
    }

    pub fn seg_collisions(&self, a: V, b: V, probe: &Probe) -> Vec<Hit> {
        let mut out = Vec::new();
        self.seg_hits(a, b, probe, &mut |h| {
            if !out.contains(&h) {
                out.push(h);
            }
            false
        });
        out
    }

    /// Kind name of a hit, for messages.
    pub fn hit_kind(&self, h: Hit) -> ObKind {
        match h {
            Hit::Ob(i) => self.obstacles[i as usize].kind,
            Hit::Added(_) => ObKind::Track,
            Hit::Area(a) => self.areas[a as usize].kind,
        }
    }

    /// Default clearance between new copper of `net` and other tracks.
    pub fn track_clearance(&self, net: u32) -> f64 {
        let me = Obj::new(ObjKind::Track, self.net_name(net), Some(self.layer));
        let other = Obj::new(ObjKind::Track, "", Some(self.layer));
        self.rules.clearance(&me, &other) as f64
    }
}
