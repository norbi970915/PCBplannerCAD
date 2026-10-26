//! Push-and-shove. Every other-net track the new route violates is pushed
//! sideways just far enough: the conflicting part of the segment is replaced
//! by a raised run joined with 45° ramps (one segment → five: kept head,
//! ramp, offset run, ramp, kept tail), so its endpoints and connections stay
//! put. Pushed pieces become pushers themselves and the process recurses to
//! the tracks they now violate, up to [`MAX_DEPTH`]. Pads, vias, arcs, zones,
//! keep-outs and the outline are never moved: hitting one aborts the shove.

use std::collections::VecDeque;

use crate::document::ItemId;
use crate::pcb::items::Track;
use crate::units::{Coord, Point};

use super::geom::{seg_seg_dist, clip_box, octant, FxSet, V};
use super::world::{Core, Hit, ObKind, Obstacle, Probe, World};

pub(super) const MAX_DEPTH: usize = 5;

/// A segment that pushes: centerline, half width, net.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pusher {
    pub a: V,
    pub b: V,
    pub hw: f64,
    pub net: u32,
}

enum Detour {
    NotNeeded,
    Points(Vec<Point>),
}

/// Raised detour of track `a`–`b` clearing every pusher (`(a, b, required
/// centerline distance)`), or `None` when it cannot fit inside the segment.
fn detour(a: Point, b: Point, pushers: &[(V, V, f64)]) -> Option<Detour> {
    let (av, bv) = (V::of(a), V::of(b));
    let len = (bv - av).len();
    if len < 1.0 {
        return None;
    }
    let conflicts: Vec<&(V, V, f64)> = pushers.iter().filter(|p| seg_seg_dist(av, bv, p.0, p.1) < p.2).collect();
    if conflicts.is_empty() {
        return Some(Detour::NotNeeded);
    }
    let u = (bv - av) * (1.0 / len);
    let req_max = pushers.iter().map(|p| p.2).fold(0.0, f64::max);
    let oct = octant(b.x - a.x, b.y - a.y).is_some();
    let mut best: Option<(f64, Vec<Point>)> = None;
    for sigma in [1.0, -1.0] {
        let n = u.perp() * sigma;
        let local = |p: V| V::new((p - av).dot(u), (p - av).dot(n));
        // Initial window: conflicting pusher parts near the original line.
        let (mut lo, mut hi, mut d) = (f64::INFINITY, f64::NEG_INFINITY, 0.0f64);
        for p in &conflicts {
            if let Some((c0, c1)) = clip_box(local(p.0), local(p.1), -p.2, len + p.2, -p.2, p.2) {
                lo = lo.min(c0.x.min(c1.x));
                hi = hi.max(c0.x.max(c1.x));
            }
        }
        if !lo.is_finite() {
            continue;
        }
        for _attempt in 0..3 {
            // Grow window and offset until no pusher part above the base line
            // (or within clearance below it) remains unaccounted for.
            for _ in 0..12 {
                let (mut nlo, mut nhi, mut nd) = (lo, hi, d);
                for p in pushers {
                    if let Some((c0, c1)) = clip_box(local(p.0), local(p.1), lo - p.2 - d, hi + p.2 + d, -p.2 + 0.5, f64::INFINITY) {
                        nlo = nlo.min(c0.x.min(c1.x));
                        nhi = nhi.max(c0.x.max(c1.x));
                        nd = nd.max(c0.y.max(c1.y) + p.2);
                    }
                }
                let stable = nlo >= lo - 0.5 && nhi <= hi + 0.5 && nd <= d + 0.5;
                lo = nlo;
                hi = nhi;
                d = nd;
                if stable {
                    break;
                }
            }
            let dd = d + 3.0;
            let ta = lo - 0.5 * req_max - 3.0;
            let tb = hi + 0.5 * req_max + 3.0;
            let pts = if oct { lattice_points(a, b, sigma, ta, tb, dd) } else { float_points(av, u, n, len, ta, tb, dd) };
            let Some(pts) = pts else { break };
            let clear = pts.windows(2).all(|w| pushers.iter().all(|p| seg_seg_dist(V::of(w[0]), V::of(w[1]), p.0, p.1) >= p.2));
            if clear {
                if best.as_ref().is_none_or(|b| dd < b.0) {
                    best = Some((dd, pts));
                }
                break;
            }
            lo -= req_max * 0.5;
            hi += req_max * 0.5;
            d += req_max * 0.25;
        }
    }
    best.map(|b| Detour::Points(b.1))
}

/// Detour points for an octilinear track using exact integer 45° steps.
fn lattice_points(a: Point, b: Point, sigma: f64, ta: f64, tb: f64, d: f64) -> Option<Vec<Point>> {
    let e = ((b.x - a.x).signum(), (b.y - a.y).signum());
    let unit = if e.0 != 0 && e.1 != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
    let f = if sigma > 0.0 { (-e.1, e.0) } else { (e.1, -e.0) };
    let ilen = (b.x - a.x).abs().max((b.y - a.y).abs());
    let j = (d / unit).ceil() as Coord;
    let i_ta = (ta / unit).floor() as Coord;
    let i_tb = (tb / unit).ceil() as Coord;
    let (i_a1, i_b1) = (i_ta - j, i_tb + j);
    if i_a1 < 0 || i_b1 > ilen || j <= 0 {
        return None;
    }
    let at = |i: Coord, jj: Coord| Point::new(a.x + e.0 * i + f.0 * jj, a.y + e.1 * i + f.1 * jj);
    let mut pts = vec![a, at(i_a1, 0), at(i_ta, j), at(i_tb, j), at(i_b1, 0), b];
    pts.dedup();
    Some(pts)
}

fn float_points(av: V, u: V, n: V, len: f64, ta: f64, tb: f64, d: f64) -> Option<Vec<Point>> {
    let (a1, b1) = (ta - d, tb + d);
    if a1 < 0.0 || b1 > len || d <= 0.0 {
        return None;
    }
    let at = |t: f64, h: f64| (av + u * t + n * h).round();
    let mut pts = vec![av.round(), at(a1, 0.0), at(ta, d), at(tb, d), at(b1, 0.0), (av + u * len).round()];
    pts.dedup();
    Some(pts)
}

/// Shoves tracks out of the way of `path` (net `probe.net`). On success the
/// world overlay holds the edits; returns (original id, replacement chain).
pub(super) fn shove(world: &mut World, probe: &Probe, path: &[Point]) -> Result<Vec<(ItemId, Vec<Track>)>, String> {
    let mut pushers: Vec<Pusher> = path.windows(2).map(|w| Pusher { a: V::of(w[0]), b: V::of(w[1]), hw: probe.hw, net: probe.net }).collect();
    let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
    let mut queued: FxSet<u32> = FxSet::default();
    let blocked = |world: &World, h: Hit| format!("blocked by a {}", world.hit_kind(h).name());
    for p in &pushers {
        for h in world.seg_collisions(p.a, p.b, probe) {
            match h {
                Hit::Ob(i) if world.obstacles[i as usize].kind == ObKind::Track => {
                    if queued.insert(i) {
                        queue.push_back((i, 1));
                    }
                }
                _ => return Err(blocked(world, h)),
            }
        }
    }
    let mut moved: Vec<(ItemId, Vec<Track>)> = Vec::new();
    while let Some((i, depth)) = queue.pop_front() {
        if depth > MAX_DEPTH {
            return Err("shove depth limit reached".into());
        }
        let ob = world.obstacles[i as usize].clone();
        let track = ob.track.clone().ok_or("not a track")?;
        let reqs: Vec<(V, V, f64)> = pushers.iter().filter(|p| p.net != ob.net || p.net == super::world::NO_NET).map(|p| (p.a, p.b, ob.r + p.hw + world.clr(ob.key, p.net))).collect();
        let pts = match detour(track.a, track.b, &reqs) {
            Some(Detour::NotNeeded) => continue,
            Some(Detour::Points(p)) => p,
            None => return Err("no room to push a track".into()),
        };
        world.removed.insert(i);
        let pieces: Vec<Track> = pts.windows(2).map(|w| Track { a: w[0], b: w[1], ..track.clone() }).collect();
        for t in &pieces {
            world.added.push(Obstacle { core: Core::Seg(V::of(t.a), V::of(t.b)), r: ob.r, net: ob.net, key: ob.key, kind: ObKind::Track, id: ob.id, track: None });
            pushers.push(Pusher { a: V::of(t.a), b: V::of(t.b), hw: ob.r, net: ob.net });
        }
        let excl = [ob.net];
        let tprobe = Probe { hw: ob.r, net: ob.net, excl: &excl };
        for t in &pieces {
            for h in world.seg_collisions(V::of(t.a), V::of(t.b), &tprobe) {
                match h {
                    Hit::Ob(j) if world.obstacles[j as usize].kind == ObKind::Track && !world.removed.contains(&j) => {
                        if queued.insert(j) {
                            queue.push_back((j, depth + 1));
                        }
                    }
                    Hit::Ob(j) if world.obstacles[j as usize].kind == ObKind::Track => return Err("a track would be pushed twice".into()),
                    Hit::Added(_) => return Err("a track would be pushed twice".into()),
                    _ => return Err(blocked(world, h)),
                }
            }
        }
        if let Some(id) = ob.id {
            moved.push((id, pieces));
        }
    }
    // The route itself must now be clean against the edited world.
    for w in path.windows(2) {
        if !world.seg_clear(V::of(w[0]), V::of(w[1]), probe) {
            return Err("shoved tracks still collide".into());
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detour_clears_a_parallel_pusher() {
        let (a, b) = (Point::new(0, 0), Point::new(20_000, 0));
        let pusher = (V::new(8_000.0, -500.0), V::new(12_000.0, -500.0), 1_000.0);
        let Some(Detour::Points(p)) = detour(a, b, &[pusher]) else { panic!("no detour") };
        assert_eq!(p.first(), Some(&a));
        assert_eq!(p.last(), Some(&b));
        assert_eq!(p.len(), 6);
        for w in p.windows(2) {
            assert!(seg_seg_dist(V::of(w[0]), V::of(w[1]), pusher.0, pusher.1) >= pusher.2);
            assert!(octant(w[1].x - w[0].x, w[1].y - w[0].y).is_some());
        }
    }
}
