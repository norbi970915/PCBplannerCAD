//! Planar polygon helpers for the 3D board model: ring cleanup, nesting of
//! outline loops into solid regions, hole acceptance (drills, slots) and
//! earcut triangulation with T-junction repair. All coordinates are integer
//! nanometers; predicates use exact `i128` arithmetic.

use std::cmp::Reverse;
use std::collections::HashMap;

use pcbplannercad_core::units::{Coord, Point};

pub(super) type Ring = Vec<Point>;

/// Axis-aligned bounds `[min_x, min_y, max_x, max_y]`.
pub(super) type Bounds = [Coord; 4];

/// One solid piece of laminate: a counter-clockwise outer ring and clockwise holes.
#[derive(Clone, Debug, Default)]
pub(super) struct Region {
    pub outer: Ring,
    pub holes: Vec<Ring>,
}

/// Points closer than this are merged (keeps f32 output vertices distinct).
const MERGE_TOL: Coord = 1_000;

pub(super) fn bounds(r: &[Point]) -> Bounds {
    let mut b = [Coord::MAX, Coord::MAX, Coord::MIN, Coord::MIN];
    for p in r {
        b[0] = b[0].min(p.x);
        b[1] = b[1].min(p.y);
        b[2] = b[2].max(p.x);
        b[3] = b[3].max(p.y);
    }
    b
}

fn overlaps(a: &Bounds, b: &Bounds) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn contains_bounds(outer: &Bounds, inner: &Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

/// Twice the signed area (positive = counter-clockwise).
pub(super) fn area2(r: &[Point]) -> i128 {
    let n = r.len();
    (0..n)
        .map(|i| {
            let (a, b) = (r[i], r[(i + 1) % n]);
            a.x as i128 * b.y as i128 - b.x as i128 * a.y as i128
        })
        .sum()
}

fn cross(o: Point, a: Point, b: Point) -> i128 {
    (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
}

/// Removes near-duplicate consecutive points and exactly collinear points.
pub(super) fn clean_ring(pts: &[Point]) -> Ring {
    let near = |a: Point, b: Point| (a.x - b.x).abs() <= MERGE_TOL && (a.y - b.y).abs() <= MERGE_TOL;
    let mut r: Ring = Vec::with_capacity(pts.len());
    for &p in pts {
        if r.last().is_none_or(|&q| !near(p, q)) {
            r.push(p);
        }
    }
    while r.len() > 1 && near(r[0], *r.last().unwrap()) {
        r.pop();
    }
    // Drop collinear (and spike) vertices until stable.
    let mut changed = true;
    while changed && r.len() >= 3 {
        changed = false;
        let mut out: Ring = Vec::with_capacity(r.len());
        let n = r.len();
        for i in 0..n {
            let prev = out.last().copied().unwrap_or(r[(i + n - 1) % n]);
            let next = r[(i + 1) % n];
            if cross(prev, r[i], next) == 0 {
                changed = true;
            } else {
                out.push(r[i]);
            }
        }
        r = out;
    }
    if r.len() < 3 { Vec::new() } else { r }
}

/// Even-odd point-in-polygon test.
pub(super) fn point_in_ring(p: Point, r: &[Point]) -> bool {
    let n = r.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (r[i], r[j]);
        if (a.y > p.y) != (b.y > p.y) {
            // x of the edge at p.y compared exactly: sign of cross relative to edge direction.
            let c = cross(a, b, p);
            if (c > 0) == (b.y > a.y) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Closed-segment intersection (touching counts).
fn segments_intersect(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let d1 = cross(q1, q2, p1).signum();
    let d2 = cross(q1, q2, p2).signum();
    let d3 = cross(p1, p2, q1).signum();
    let d4 = cross(p1, p2, q2).signum();
    if d1 * d2 < 0 && d3 * d4 < 0 {
        return true;
    }
    (d1 == 0 && on_segment(q1, q2, p1)) || (d2 == 0 && on_segment(q1, q2, p2)) || (d3 == 0 && on_segment(p1, p2, q1)) || (d4 == 0 && on_segment(p1, p2, q2))
}

/// Whether any edge of `a` (bounds `ab`) touches any edge of `ring`.
fn ring_crosses(a: &[Point], ab: &Bounds, ring: &[Point]) -> bool {
    let n = ring.len();
    for i in 0..n {
        let (q1, q2) = (ring[i], ring[(i + 1) % n]);
        let eb = [q1.x.min(q2.x), q1.y.min(q2.y), q1.x.max(q2.x), q1.y.max(q2.y)];
        if !overlaps(&eb, ab) {
            continue;
        }
        let m = a.len();
        for j in 0..m {
            if segments_intersect(a[j], a[(j + 1) % m], q1, q2) {
                return true;
            }
        }
    }
    false
}

fn oriented(mut r: Ring, ccw: bool) -> Ring {
    if (area2(&r) > 0) != ccw {
        r.reverse();
    }
    r
}

/// Groups non-intersecting outline loops into regions: even nesting depth
/// loops are solid outers, odd depth loops are cutouts of their parent.
pub(super) fn nest(mut loops: Vec<Ring>) -> Vec<Region> {
    loops.retain(|r| r.len() >= 3);
    loops.sort_by_key(|r| Reverse(area2(r).abs()));
    let bbs: Vec<Bounds> = loops.iter().map(|r| bounds(r)).collect();
    let n = loops.len();
    let mut depth = vec![0usize; n];
    let mut region_of = vec![usize::MAX; n];
    let mut regions: Vec<Region> = Vec::new();
    for i in 0..n {
        // Loops are sorted by decreasing area, so the first container found
        // walking backwards is the innermost one.
        let parent = (0..i).rev().find(|&j| contains_bounds(&bbs[j], &bbs[i]) && point_in_ring(loops[i][0], &loops[j]));
        depth[i] = parent.map_or(0, |p| depth[p] + 1);
        if depth[i].is_multiple_of(2) {
            region_of[i] = regions.len();
            regions.push(Region { outer: oriented(loops[i].clone(), true), holes: Vec::new() });
        } else if let Some(p) = parent {
            regions[region_of[p]].holes.push(oriented(loops[i].clone(), false));
        }
    }
    regions
}

const CELL: Coord = 2_000_000;

fn cells(b: &Bounds) -> impl Iterator<Item = (i64, i64)> {
    let (x0, y0, x1, y1) = (b[0].div_euclid(CELL), b[1].div_euclid(CELL), b[2].div_euclid(CELL), b[3].div_euclid(CELL));
    (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
}

/// Adds candidate holes (drills/slots) to the region that fully contains
/// them. Holes crossing an outline, or overlapping an already accepted hole
/// or cutout, are dropped (largest holes are accepted first).
pub(super) fn add_holes(regions: &mut [Region], candidates: Vec<Ring>) -> usize {
    let outer_bb: Vec<Bounds> = regions.iter().map(|r| bounds(&r.outer)).collect();
    let mut hole_bb: Vec<Vec<Bounds>> = regions.iter().map(|r| r.holes.iter().map(|h| bounds(h)).collect()).collect();
    let mut grid: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    for (ri, hb) in hole_bb.iter().enumerate() {
        for (hi, b) in hb.iter().enumerate() {
            for c in cells(b) {
                grid.entry(c).or_default().push((ri, hi));
            }
        }
    }
    let mut cands: Vec<Ring> = candidates.into_iter().map(|r| clean_ring(&r)).filter(|r| r.len() >= 3).collect();
    cands.sort_by_key(|r| Reverse(area2(r).abs()));
    let mut accepted = 0;
    for cand in cands {
        let cb = bounds(&cand);
        let Some(ri) = (0..regions.len()).find(|&ri| {
            contains_bounds(&outer_bb[ri], &cb) && point_in_ring(cand[0], &regions[ri].outer) && !ring_crosses(&cand, &cb, &regions[ri].outer)
        }) else {
            continue;
        };
        let mut clash = false;
        'cells: for c in cells(&cb) {
            if let Some(list) = grid.get(&c) {
                for &(r2, hi) in list {
                    if r2 != ri || !overlaps(&hole_bb[r2][hi], &cb) {
                        continue;
                    }
                    let other = &regions[r2].holes[hi];
                    if ring_crosses(&cand, &cb, other) || point_in_ring(cand[0], other) || point_in_ring(other[0], &cand) {
                        clash = true;
                        break 'cells;
                    }
                }
            }
        }
        if clash {
            continue;
        }
        let hi = regions[ri].holes.len();
        regions[ri].holes.push(oriented(cand, false));
        hole_bb[ri].push(cb);
        for c in cells(&cb) {
            grid.entry(c).or_default().push((ri, hi));
        }
        accepted += 1;
    }
    accepted
}

/// Triangulates a region. Vertex indices refer to `outer` followed by each
/// hole in order; triangles are counter-clockwise seen from +Z. Every ring
/// vertex is referenced, so the faces stitch exactly onto the side walls.
pub(super) fn triangulate(r: &Region) -> Vec<[u32; 3]> {
    let b = bounds(&r.outer);
    let (ox, oy) = (b[0], b[1]);
    let mut rings: Vec<(usize, usize)> = vec![(0, r.outer.len())];
    let mut hole_idx: Vec<u32> = Vec::with_capacity(r.holes.len());
    let mut start = r.outer.len();
    for h in &r.holes {
        hole_idx.push(start as u32);
        rings.push((start, h.len()));
        start += h.len();
    }
    let pts = r.outer.iter().chain(r.holes.iter().flatten());
    let mut out: Vec<u32> = Vec::new();
    earcut::Earcut::<f64>::new().earcut(pts.map(|p| [(p.x - ox) as f64, (p.y - oy) as f64]), &hole_idx, &mut out);
    let mut tris: Vec<[u32; 3]> = out.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();

    let all: Vec<Point> = r.outer.iter().chain(r.holes.iter().flatten()).copied().collect();
    let total: i128 = tris.iter().map(|t| cross(all[t[0] as usize], all[t[1] as usize], all[t[2] as usize])).sum();
    if total < 0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    repair_dropped(&mut tris, &rings, all.len());
    tris
}

/// Earcut may drop vertices that become collinear after hole bridging; split
/// the triangle on the shortcut edge so every ring vertex is used again.
fn repair_dropped(tris: &mut Vec<[u32; 3]>, rings: &[(usize, usize)], n: usize) {
    let mut used = vec![false; n];
    for t in tris.iter() {
        for &v in t {
            used[v as usize] = true;
        }
    }
    if used.iter().all(|&u| u) || tris.is_empty() {
        return;
    }
    let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            edges.entry(key(t[k], t[(k + 1) % 3])).or_default().push(i);
        }
    }
    let mut dead = vec![false; tris.len()];
    for &(s, len) in rings {
        let Some(first) = (0..len).find(|&k| used[s + k]) else { continue };
        let mut k = first;
        loop {
            // Collect the run of unused vertices after the used vertex `k`.
            let mut run: Vec<u32> = Vec::new();
            let mut j = (k + 1) % len;
            while !used[s + j] {
                run.push((s + j) as u32);
                j = (j + 1) % len;
            }
            if !run.is_empty() {
                let (a, b) = ((s + k) as u32, (s + j) as u32);
                let ti = edges.get(&key(a, b)).and_then(|l| l.iter().copied().find(|&t| !dead[t]));
                if let Some(ti) = ti {
                    let t = tris[ti];
                    let e = (0..3).find(|&e| key(t[e], t[(e + 1) % 3]) == key(a, b)).unwrap();
                    let (x, y, c) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
                    let mut chain = vec![a];
                    chain.extend(&run);
                    chain.push(b);
                    if x != a {
                        chain.reverse();
                    }
                    debug_assert!(chain[0] == x && *chain.last().unwrap() == y);
                    dead[ti] = true;
                    for w in chain.windows(2) {
                        let nt = [w[0], w[1], c];
                        let idx = tris.len();
                        tris.push(nt);
                        dead.push(false);
                        for q in 0..3 {
                            edges.entry(key(nt[q], nt[(q + 1) % 3])).or_default().push(idx);
                        }
                    }
                    for &v in &run {
                        used[v as usize] = true;
                    }
                }
            }
            k = j;
            if k == first {
                break;
            }
        }
    }
    let mut i = 0;
    tris.retain(|_| {
        i += 1;
        !dead[i - 1]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point::new(x * 1_000_000, y * 1_000_000)
    }

    #[test]
    fn clean_removes_collinear_and_duplicates() {
        let r = clean_ring(&[p(0, 0), p(5, 0), p(10, 0), p(10, 0), p(10, 10), p(0, 10), p(0, 0)]);
        assert_eq!(r, vec![p(0, 0), p(10, 0), p(10, 10), p(0, 10)]);
    }

    #[test]
    fn nesting_assigns_cutouts_and_islands() {
        let sq = |x0, y0, s| vec![p(x0, y0), p(x0 + s, y0), p(x0 + s, y0 + s), p(x0, y0 + s)];
        let regions = nest(vec![sq(10, 10, 20), sq(0, 0, 100), sq(15, 15, 5), sq(200, 0, 10)]);
        assert_eq!(regions.len(), 3);
        assert_eq!(regions[0].holes.len(), 1);
        assert!(area2(&regions[0].outer) > 0 && area2(&regions[0].holes[0]) < 0);
    }

    #[test]
    fn repair_restores_dropped_vertices() {
        // Square split along the diagonal with a collinear vertex on one side dropped.
        let mut tris = vec![[0, 1, 3], [1, 2, 3]];
        // Ring of 5 vertices where vertex 4 lies on edge 3 → 0.
        repair_dropped(&mut tris, &[(0, 5)], 5);
        // Edge (3,0) belongs to triangle [0,1,3]; it is split into [3,4,1] and [4,0,1].
        assert_eq!(tris.len(), 3);
        assert!(tris.iter().any(|t| t.contains(&4)));
    }
}
