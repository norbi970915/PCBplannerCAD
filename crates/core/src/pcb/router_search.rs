//! Walkaround: A* over an implicit octilinear (or rectilinear) grid aligned
//! to the start point, followed by greedy corner-style smoothing.
//!
//! Grid nodes store their *clearance slack* (distance to the nearest
//! obstacle minus the required distance, capped). Because distance is
//! 1-Lipschitz, an edge of length `L` between nodes with slacks `su`, `sv` is
//! provably clear when `su + sv >= L`; only edges near obstacles pay for an
//! exact segment test. States carry the arrival direction so bends cost
//! extra and only 45° (or 90° for orthogonal) turns are generated.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::units::Point;

use super::geom::{corner_path, octant_diff, simplify, turn_ok, vec_of, FxMap, DIRS, V};
use super::world::{Probe, World};
use super::CornerStyle;

pub(super) struct Walk {
    pub path: Vec<Point>,
    pub reached: bool,
    pub blocked_at: Option<Point>,
    pub message: String,
}

/// Coarse-pass expansion budget; the fine pass gets twice as many.
const BUDGET: usize = 12_000;
/// A* heuristic weight (>1 trades optimality for speed; smoothing recovers).
const H_WEIGHT: f64 = 1.25;

/// Corner-style candidates from `a` to `b`, preferred posture first.
pub(super) fn connections(a: Point, b: Point, style: CornerStyle, flip: bool) -> Vec<Vec<Point>> {
    let mut out = Vec::with_capacity(3);
    match style {
        CornerStyle::Any => {
            out.push(vec![a, b]);
            out.push(corner_path(a, b, CornerStyle::Diagonal45, flip));
            out.push(corner_path(a, b, CornerStyle::Diagonal45, !flip));
        }
        _ => {
            out.push(corner_path(a, b, style, flip));
            out.push(corner_path(a, b, style, !flip));
        }
    }
    out.dedup();
    out
}

fn path_turns_ok(incoming: (i64, i64), c: &[Point], outgoing: (i64, i64), style: CornerStyle) -> bool {
    let mut prev = incoming;
    for w in c.windows(2) {
        let v = vec_of(w[0], w[1]);
        if !turn_ok(prev, v, style) {
            return false;
        }
        prev = v;
    }
    turn_ok(prev, outgoing, style)
}

#[derive(Clone, Copy)]
struct St {
    i: i32,
    j: i32,
    dir: u8,
    g: f64,
    parent: u32,
}

struct Search<'s, 'w> {
    world: &'s World<'w>,
    probe: &'s Probe<'s>,
    origin: Point,
    step: i64,
    lo: V,
    hi: V,
    cap: f64,
    slack: FxMap<(i32, i32), f64>,
    edges: FxMap<(i32, i32, u8), bool>,
}

impl<'s, 'w> Search<'s, 'w> {
    fn pt(&self, i: i32, j: i32) -> Point {
        self.origin.translated(i as i64 * self.step, j as i64 * self.step)
    }

    fn slack(&mut self, i: i32, j: i32) -> f64 {
        if let Some(&s) = self.slack.get(&(i, j)) {
            return s;
        }
        let p = V::of(self.pt(i, j));
        let s = if p.x < self.lo.x || p.x > self.hi.x || p.y < self.lo.y || p.y > self.hi.y { -1.0 } else { self.world.point_slack(p, self.probe, self.cap) };
        self.slack.insert((i, j), s);
        s
    }

    fn reconstruct(states: &[St], mut idx: u32, origin: Point, step: i64) -> Vec<Point> {
        let mut pts = Vec::new();
        loop {
            let s = states[idx as usize];
            pts.push(origin.translated(s.i as i64 * step, s.j as i64 * step));
            if s.parent == u32::MAX {
                break;
            }
            idx = s.parent;
        }
        pts.reverse();
        pts
    }

    /// Returns (path, reached).
    fn astar(&mut self, goal: Point, style: CornerStyle, flip: bool, budget: usize) -> (Vec<Point>, bool) {
        let ortho = style == CornerStyle::Orthogonal90;
        let step = self.step as f64;
        let pen = step * 0.6;
        let gv = V::of(goal);
        let heur = |p: Point| {
            let (dx, dy) = (((goal.x - p.x) as f64).abs(), ((goal.y - p.y) as f64).abs());
            if ortho { dx + dy } else { dx.max(dy) - dx.min(dy) + dx.min(dy) * std::f64::consts::SQRT_2 }
        };
        let mut states: Vec<St> = vec![St { i: 0, j: 0, dir: 8, g: 0.0, parent: u32::MAX }];
        let mut closed: Vec<bool> = vec![false];
        let mut index: FxMap<(i32, i32, u8), u32> = FxMap::default();
        index.insert((0, 0, 8), 0);
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((heur(self.origin).to_bits(), 0u32)));
        let mut best = ((V::of(self.origin) - gv).len(), 0u32);
        let mut pops = 0;
        while let Some(Reverse((_, idx))) = heap.pop() {
            if closed[idx as usize] {
                continue;
            }
            closed[idx as usize] = true;
            pops += 1;
            if pops > budget {
                break;
            }
            let st = states[idx as usize];
            let p = self.pt(st.i, st.j);
            let d = (V::of(p) - gv).len();
            if d < best.0 {
                best = (d, idx);
            }
            if (p.x - goal.x).abs() as f64 <= 1.5 * step && (p.y - goal.y).abs() as f64 <= 1.5 * step {
                let mut path = Self::reconstruct(&states, idx, self.origin, self.step);
                if p == goal {
                    return (path, true);
                }
                let incoming = if st.dir < 8 { DIRS[st.dir as usize] } else { (0, 0) };
                for c in connections(p, goal, style, flip) {
                    if path_turns_ok(incoming, &c, (0, 0), style) && self.world.path_clear(&c, self.probe) {
                        path.extend_from_slice(&c[1..]);
                        return (path, true);
                    }
                }
            }
            let su = self.slack(st.i, st.j);
            for nd in 0..8u8 {
                let diag = nd % 2 == 1;
                if ortho && diag {
                    continue;
                }
                let turn = if st.dir < 8 { octant_diff(st.dir, nd) } else { 0 };
                let allowed = if ortho { turn == 0 || turn == 2 } else { turn <= 1 };
                if !allowed {
                    continue;
                }
                let (dx, dy) = DIRS[nd as usize];
                let (ni, nj) = (st.i + dx as i32, st.j + dy as i32);
                let sv = self.slack(ni, nj);
                if sv < 0.0 {
                    continue;
                }
                let len = if diag { step * std::f64::consts::SQRT_2 } else { step };
                if su + sv < len {
                    let ok = match self.edges.get(&(st.i, st.j, nd)) {
                        Some(&ok) => ok,
                        None => {
                            let ok = self.world.seg_clear(V::of(p), V::of(self.pt(ni, nj)), self.probe);
                            self.edges.insert((st.i, st.j, nd), ok);
                            ok
                        }
                    };
                    if !ok {
                        continue;
                    }
                }
                let g = st.g + len + if turn > 0 { pen } else { 0.0 };
                let key = (ni, nj, nd);
                let k = match index.get(&key) {
                    Some(&k) => {
                        if closed[k as usize] || states[k as usize].g <= g {
                            continue;
                        }
                        states[k as usize].g = g;
                        states[k as usize].parent = idx;
                        k
                    }
                    None => {
                        let k = states.len() as u32;
                        states.push(St { i: ni, j: nj, dir: nd, g, parent: idx });
                        closed.push(false);
                        index.insert(key, k);
                        k
                    }
                };
                let f = g + H_WEIGHT * heur(self.pt(ni, nj));
                heap.push(Reverse((f.to_bits(), k)));
            }
        }
        (Self::reconstruct(&states, best.1, self.origin, self.step), false)
    }
}

/// Greedy far-first shortcutting with corner-style connections that keep the
/// turn rules at both junctions; merges the jogs A* leaves behind.
pub(super) fn smooth(world: &World, probe: &Probe, pts: &[Point], style: CornerStyle, flip: bool) -> Vec<Point> {
    let pts = simplify(pts);
    let n = pts.len();
    if n <= 2 {
        return pts;
    }
    let mut out = vec![pts[0]];
    let mut i = 0;
    while i < n - 1 {
        let incoming = if out.len() >= 2 { vec_of(out[out.len() - 2], out[out.len() - 1]) } else { (0, 0) };
        let mut advanced = false;
        'outer: for j in (i + 1..n).rev() {
            let outgoing = if j < n - 1 { vec_of(pts[j], pts[j + 1]) } else { (0, 0) };
            for c in connections(pts[i], pts[j], style, flip) {
                if path_turns_ok(incoming, &c, outgoing, style) && (j == i + 1 && c.len() == 2 || world.path_clear(&c, probe)) {
                    out.extend_from_slice(&c[1..]);
                    i = j;
                    advanced = true;
                    break 'outer;
                }
            }
        }
        if !advanced {
            out.push(pts[i + 1]);
            i += 1;
        }
    }
    simplify(&out)
}

/// Nearest clear point around `p` (spiral search), for blocked end points.
fn free_near(world: &World, probe: &Probe, p: Point, pitch: f64) -> Option<Point> {
    let r_step = pitch / 4.0;
    for r in 1..=32 {
        let radius = r_step * r as f64;
        let k = 8 * r;
        for s in 0..k {
            let a = std::f64::consts::TAU * s as f64 / k as f64;
            let q = (V::of(p) + V::new(a.cos(), a.sin()) * radius).round();
            if world.point_slack(V::of(q), probe, 1.0) >= 0.0 {
                return Some(q);
            }
        }
    }
    None
}

pub(super) fn walkaround(world: &World, probe: &Probe, start: Point, end: Point, style: CornerStyle, flip: bool) -> Walk {
    if start == end {
        return Walk { path: vec![start], reached: true, blocked_at: None, message: String::new() };
    }
    if world.point_slack(V::of(start), probe, 1.0) < 0.0 {
        return Walk { path: vec![start], reached: false, blocked_at: Some(start), message: "start point violates clearance".into() };
    }
    for c in connections(start, end, style, flip) {
        if world.path_clear(&c, probe) {
            return Walk { path: c, reached: true, blocked_at: None, message: String::new() };
        }
    }
    let pitch = (probe.hw + world.track_clearance(probe.net)).max(5_000.0);
    let dist = (V::of(end) - V::of(start)).len();
    let coarse = pitch.max(dist / 1200.0).round() as i64;
    let (goal, end_blocked) = if world.point_slack(V::of(end), probe, 1.0) < 0.0 {
        match free_near(world, probe, end, pitch) {
            Some(g) => (g, true),
            None => (end, true),
        }
    } else {
        (end, false)
    };
    let margin = (dist * 0.5).max(40.0 * pitch);
    let mut lo = V::new(start.x.min(goal.x) as f64 - margin, start.y.min(goal.y) as f64 - margin);
    let mut hi = V::new(start.x.max(goal.x) as f64 + margin, start.y.max(goal.y) as f64 + margin);
    if let Some((blo, bhi)) = world.board_box {
        lo = V::new(lo.x.max(blo.x - pitch), lo.y.max(blo.y - pitch));
        hi = V::new(hi.x.min(bhi.x + pitch), hi.y.min(bhi.y + pitch));
    }
    let mut best: Option<(Vec<Point>, f64)> = None;
    let mut reached = false;
    for (step, budget) in [(coarse, BUDGET), ((coarse / 3).max(1_000), 2 * BUDGET)] {
        let mut s = Search { world, probe, origin: start, step, lo, hi, cap: step as f64 * 1.5, slack: FxMap::default(), edges: FxMap::default() };
        let (path, ok) = s.astar(goal, style, flip, budget);
        if ok {
            best = Some((path, 0.0));
            reached = true;
            break;
        }
        let rest = (V::of(*path.last().unwrap()) - V::of(goal)).len();
        if best.as_ref().is_none_or(|b| rest < b.1) {
            best = Some((path, rest));
        }
    }
    let path = smooth(world, probe, &best.map(|b| b.0).unwrap_or_else(|| vec![start]), style, flip);
    let last = *path.last().unwrap();
    if reached && !end_blocked {
        Walk { path, reached: true, blocked_at: None, message: String::new() }
    } else {
        let message = if end_blocked { "end point violates clearance; routed as close as possible" } else { "no clear path to the end point; routed as close as possible" };
        Walk { path, reached: false, blocked_at: Some(last), message: message.into() }
    }
}
