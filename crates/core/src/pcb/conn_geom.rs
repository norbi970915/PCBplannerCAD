//! Distance kernel for connectivity and DRC: shapes are decomposed into
//! capsules (segment + radius; circles are zero-length capsules), exact circular
//! arcs and filled
//! polygons (with holes, even-odd), and [`gap`] measures the copper-to-copper
//! distance between two such sets (negative = overlap). Large polygons get an
//! R-tree over their edges so zone fills stay cheap.

use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};

#[path="conn_arc.rs"]
mod circular;
pub use circular::CircularArc;

pub type P = [f64; 2];

/// Gaps below this many nanometers count as touching.
pub const TOUCH: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bb {
    pub min: P,
    pub max: P,
}

impl Bb {
    pub const EMPTY: Bb = Bb { min: [f64::INFINITY, f64::INFINITY], max: [f64::NEG_INFINITY, f64::NEG_INFINITY] };

    pub fn of(a: P, b: P) -> Bb {
        Bb { min: [a[0].min(b[0]), a[1].min(b[1])], max: [a[0].max(b[0]), a[1].max(b[1])] }
    }

    pub fn add(&mut self, p: P) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }

    pub fn union(self, o: Bb) -> Bb {
        Bb { min: [self.min[0].min(o.min[0]), self.min[1].min(o.min[1])], max: [self.max[0].max(o.max[0]), self.max[1].max(o.max[1])] }
    }

    pub fn grow(self, d: f64) -> Bb {
        Bb { min: [self.min[0] - d, self.min[1] - d], max: [self.max[0] + d, self.max[1] + d] }
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1]
    }

    pub fn intersects(&self, o: &Bb) -> bool {
        self.min[0] <= o.max[0] && o.min[0] <= self.max[0] && self.min[1] <= o.max[1] && o.min[1] <= self.max[1]
    }

    pub fn aabb(&self) -> AABB<P> {
        AABB::from_corners(self.min, self.max)
    }
}

pub type Entry = GeomWithData<Rectangle<P>, usize>;

pub fn entry(bb: Bb, data: usize) -> Entry {
    GeomWithData::new(Rectangle::from_corners(bb.min, bb.max), data)
}

/// Spatial index over boxes carrying an index.
pub struct BoxTree(RTree<Entry>);

impl BoxTree {
    pub fn new(boxes: impl IntoIterator<Item = (Bb, usize)>) -> BoxTree {
        BoxTree(RTree::bulk_load(boxes.into_iter().filter(|(b, _)| !b.is_empty()).map(|(b, i)| entry(b, i)).collect()))
    }

    pub fn query(&self, bb: Bb) -> impl Iterator<Item = usize> + '_ {
        self.0.locate_in_envelope_intersecting(bb.aabb()).map(|e| e.data)
    }
}

// ---------- primitive geometry ----------

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub fn mid(a: P, b: P) -> P {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// Distance from `p` to segment `a`–`b` and the closest point on it.
pub fn point_seg(p: P, a: P, b: P) -> (f64, P) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 == 0.0 { 0.0 } else { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) };
    let c = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    (dist(p, c), c)
}

/// Segment–segment distance with closest points; `cross` is a proper
/// (interior) crossing.
pub fn seg_seg(a: P, b: P, c: P, d: P) -> (f64, P, P, bool) {
    let (ab, cd) = (sub(b, a), sub(d, c));
    let d1 = cross(ab, sub(c, a));
    let d2 = cross(ab, sub(d, a));
    let d3 = cross(cd, sub(a, c));
    let d4 = cross(cd, sub(b, c));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        let t = d3 / (d3 - d4);
        let ip = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        return (0.0, ip, ip, true);
    }
    let mut best = {
        let (dd, q) = point_seg(a, c, d);
        (dd, a, q)
    };
    for (dd, p, q) in [
        {
            let (dd, q) = point_seg(b, c, d);
            (dd, b, q)
        },
        {
            let (dd, q) = point_seg(c, a, b);
            (dd, q, c)
        },
        {
            let (dd, q) = point_seg(d, a, b);
            (dd, q, d)
        },
    ] {
        if dd < best.0 {
            best = (dd, p, q);
        }
    }
    (best.0, best.1, best.2, false)
}

// ---------- polygons ----------

type Edge = GeomWithData<Rectangle<P>, (u32, u32)>;

/// Filled polygon region: rings (outer + holes), even-odd fill.
pub struct PolyGeo {
    pub rings: Vec<Vec<P>>,
    pub bb: Bb,
    edges: usize,
    index: Option<RTree<Edge>>,
}

const INDEX_EDGES: usize = 24;

impl PolyGeo {
    pub fn new(rings: Vec<Vec<P>>) -> PolyGeo {
        let rings: Vec<Vec<P>> = rings.into_iter().filter(|r| r.len() >= 3).collect();
        let mut bb = Bb::EMPTY;
        rings.iter().flatten().for_each(|p| bb.add(*p));
        let edges = rings.iter().map(|r| r.len()).sum();
        let index = (edges > INDEX_EDGES).then(|| {
            let mut list = Vec::with_capacity(edges);
            for (ri, r) in rings.iter().enumerate() {
                for i in 0..r.len() {
                    let (a, b) = (r[i], r[(i + 1) % r.len()]);
                    let e = Bb::of(a, b);
                    list.push(GeomWithData::new(Rectangle::from_corners(e.min, e.max), (ri as u32, i as u32)));
                }
            }
            RTree::bulk_load(list)
        });
        PolyGeo { rings, bb, edges, index }
    }

    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    fn edge(&self, r: u32, i: u32) -> (P, P) {
        let ring = &self.rings[r as usize];
        (ring[i as usize], ring[(i as usize + 1) % ring.len()])
    }

    /// Calls `f` for every edge whose box intersects `bb` (all edges when small).
    fn edges_near(&self, bb: &Bb, mut f: impl FnMut(P, P)) {
        match &self.index {
            Some(t) => {
                for e in t.locate_in_envelope_intersecting(bb.aabb()) {
                    let (a, b) = self.edge(e.data.0, e.data.1);
                    f(a, b);
                }
            }
            None => {
                for r in &self.rings {
                    for i in 0..r.len() {
                        let (a, b) = (r[i], r[(i + 1) % r.len()]);
                        if Bb::of(a, b).intersects(bb) {
                            f(a, b);
                        }
                    }
                }
            }
        }
    }

    /// Even-odd point containment (ray towards +x).
    pub fn contains(&self, p: P) -> bool {
        if !self.bb.intersects(&Bb::of(p, p)) {
            return false;
        }
        let mut inside = false;
        let ray = Bb { min: p, max: [self.bb.max[0] + 1.0, p[1]] };
        self.edges_near(&ray, |a, b| {
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if p[0] < x {
                    inside = !inside;
                }
            }
        });
        inside
    }

    fn first_vertex(&self) -> P {
        self.rings[0][0]
    }
}

// ---------- shapes ----------

pub enum Prim {
    Arc(CircularArc),
    Cap { a: P, b: P, r: f64 },
    Poly(Box<PolyGeo>),
}

impl Prim {
    pub fn bb(&self) -> Bb {
        match self {
            Prim::Cap { a, b, r } => Bb::of(*a, *b).grow(*r),
            Prim::Poly(p) => p.bb,
            Prim::Arc(a) => a.bounds(),
        }
    }
}

/// A shape as a union of primitives.
pub struct Geo {
    pub prims: Vec<(Prim, Bb)>,
    pub bb: Bb,
}

impl Geo {
    pub fn new(prims: Vec<Prim>) -> Geo {
        let prims: Vec<(Prim, Bb)> = prims
            .into_iter()
            .filter(|p| !matches!(p, Prim::Poly(g) if g.is_empty()))
            .map(|p| {
                let b = p.bb();
                (p, b)
            })
            .collect();
        let bb = prims.iter().fold(Bb::EMPTY, |acc, (_, b)| acc.union(*b));
        Geo { prims, bb }
    }

    pub fn cap(a: P, b: P, r: f64) -> Geo {
        Geo::new(vec![Prim::Cap { a, b, r }])
    }

    /// A point inside the shape (for containment tests).
    pub fn anchor(&self) -> Option<P> {
        self.prims.first().map(|(p, _)| match p {
            Prim::Cap { a, .. } => *a,
            Prim::Poly(g) => g.first_vertex(),
            Prim::Arc(a) => a.point(a.start),
        })
    }

    /// Whether `p` lies inside the shape.
    pub fn contains(&self, p: P) -> bool {
        gap(self, &Geo::cap(p, p, 0.0), 0.0).is_some_and(|(g, _)| g <= 0.0)
    }
}

/// Polyline points of an arc with chord error ≤ 0.5 µm.
pub fn arc_polyline(center: P, radius: f64, start_deg: f64, sweep_deg: f64) -> Vec<P> {
    let sweep = sweep_deg.to_radians();
    let r = radius.max(1.0);
    let step = 2.0 * (1.0 - (500.0 / r).min(1.0)).acos();
    let n = ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 720);
    let a0 = start_deg.to_radians();
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        })
        .collect()
}

pub fn polyline_caps(pts: &[P], r: f64, closed: bool, out: &mut Vec<Prim>) {
    if pts.len() == 1 {
        out.push(Prim::Cap { a: pts[0], b: pts[0], r });
    }
    for w in pts.windows(2) {
        out.push(Prim::Cap { a: w[0], b: w[1], r });
    }
    if closed && pts.len() > 2 {
        out.push(Prim::Cap { a: pts[pts.len() - 1], b: pts[0], r });
    }
}

// ---------- distances ----------

/// Gap result: distance between the boundaries (negative = overlap) and a
/// representative location.
pub type Gap = (f64, P);

fn cap_cap(a: P, b: P, ra: f64, c: P, d: P, rb: f64, limit: f64) -> Option<Gap> {
    let (dd, p, q, _) = seg_seg(a, b, c, d);
    let g = dd - ra - rb;
    (g <= limit).then(|| (g, mid(p, q)))
}

fn cap_poly(a: P, b: P, r: f64, poly: &PolyGeo, limit: f64) -> Option<Gap> {
    let q = Bb::of(a, b).grow(r + limit.max(0.0));
    if !q.intersects(&poly.bb) {
        return None;
    }
    let mut best = (f64::INFINITY, a, a, false);
    poly.edges_near(&q, |e0, e1| {
        if best.3 {
            return;
        }
        let s = seg_seg(a, b, e0, e1);
        if s.3 || s.0 < best.0 {
            best = s;
        }
    });
    if best.3 {
        return Some((-r - 1.0, best.1));
    }
    if poly.contains(a) {
        return Some((-r - 1.0, a));
    }
    let g = best.0 - r;
    (g <= limit).then(|| (g, mid(best.1, best.2)))
}

fn poly_poly(pa: &PolyGeo, pb: &PolyGeo, limit: f64) -> Option<Gap> {
    if !pa.bb.grow(limit.max(0.0)).intersects(&pb.bb) {
        return None;
    }
    let (small, big) = if pa.edges <= pb.edges { (pa, pb) } else { (pb, pa) };
    let mut best = (f64::INFINITY, [0.0; 2], [0.0; 2], false);
    let reach = big.bb.grow(limit.max(0.0));
    'outer: for r in &small.rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            let eb = Bb::of(a, b);
            if !eb.intersects(&reach) {
                continue;
            }
            big.edges_near(&eb.grow(limit.max(0.0).min(best.0)), |c, d| {
                if best.3 {
                    return;
                }
                let s = seg_seg(a, b, c, d);
                if s.3 || s.0 < best.0 {
                    best = s;
                }
            });
            if best.3 {
                break 'outer;
            }
        }
    }
    if best.3 {
        return Some((-1.0, best.1));
    }
    if big.contains(small.first_vertex()) {
        return Some((-1.0, small.first_vertex()));
    }
    if small.contains(big.first_vertex()) {
        return Some((-1.0, big.first_vertex()));
    }
    (best.0 <= limit).then(|| (best.0, mid(best.1, best.2)))
}

fn prim_gap(x: &Prim, y: &Prim, limit: f64) -> Option<Gap> {
    match (x, y) {
        (Prim::Arc(a),Prim::Arc(b)) => circular::arc_arc(a,b,limit),
        (Prim::Arc(arc),Prim::Cap{a,b,r}) | (Prim::Cap{a,b,r},Prim::Arc(arc)) => circular::arc_cap(arc,*a,*b,*r,limit),
        (Prim::Arc(arc),Prim::Poly(p)) | (Prim::Poly(p),Prim::Arc(arc)) => circular::arc_poly(arc,p,limit),
        (Prim::Cap { a, b, r }, Prim::Cap { a: c, b: d, r: r2 }) => cap_cap(*a, *b, *r, *c, *d, *r2, limit),
        (Prim::Cap { a, b, r }, Prim::Poly(p)) | (Prim::Poly(p), Prim::Cap { a, b, r }) => cap_poly(*a, *b, *r, p, limit),
        (Prim::Poly(p), Prim::Poly(q)) => poly_poly(p, q, limit),
    }
}

/// Smallest gap between two shapes if it is ≤ `limit` (else `None`).
pub fn gap(x: &Geo, y: &Geo, limit: f64) -> Option<Gap> {
    if !x.bb.grow(limit.max(0.0)).intersects(&y.bb) {
        return None;
    }
    let mut best: Option<Gap> = None;
    for (px, bx) in &x.prims {
        let lim = best.map_or(limit, |b| b.0.min(limit));
        let reach = bx.grow(lim.max(0.0));
        if !reach.intersects(&y.bb) {
            continue;
        }
        for (py, by) in &y.prims {
            let lim = best.map_or(limit, |b| b.0.min(limit));
            if !bx.grow(lim.max(0.0)).intersects(by) {
                continue;
            }
            if let Some(g) = prim_gap(px, py, lim)
                && best.is_none_or(|b| g.0 < b.0)
            {
                best = Some(g);
            }
        }
        if best.is_some_and(|b| b.0 < 0.0) {
            break; // overlapping: exact depth does not matter
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> PolyGeo {
        PolyGeo::new(vec![vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]])
    }

    #[test]
    fn segment_distances() {
        let (d, _, _, c) = seg_seg([0.0, 0.0], [10.0, 0.0], [5.0, -5.0], [5.0, 5.0]);
        assert!(c && d == 0.0);
        let (d, ..) = seg_seg([0.0, 0.0], [10.0, 0.0], [12.0, 3.0], [12.0, 9.0]);
        assert!((d - 13f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn polygon_queries() {
        let mut big = vec![];
        for i in 0..100 {
            let a = std::f64::consts::TAU * i as f64 / 100.0;
            big.push([1000.0 * a.cos(), 1000.0 * a.sin()]);
        }
        let hole: Vec<P> = big.iter().map(|p| [p[0] * 0.5, p[1] * 0.5]).collect();
        let ring = PolyGeo::new(vec![big, hole]);
        assert!(ring.index.is_some());
        assert!(ring.contains([750.0, 0.0]));
        assert!(!ring.contains([0.0, 0.0]));
        assert!(!ring.contains([2000.0, 0.0]));
        let g = Geo::new(vec![Prim::Poly(Box::new(ring))]);
        // Track inside the hole: 400 - 100 width/2 → gap ≈ 500 - 50 - 100 = 350 from the hole edge.
        let t = Geo::cap([-50.0, 0.0], [100.0, 0.0], 50.0);
        let (gp, _) = gap(&g, &t, 1000.0).unwrap();
        assert!((gp - 350.0).abs() < 2.0, "{gp}");
        assert!(gap(&g, &t, 100.0).is_none());
        // Track fully inside the copper ring overlaps.
        assert!(gap(&g, &Geo::cap([700.0, 0.0], [710.0, 0.0], 5.0), 0.0).unwrap().0 < 0.0);
        // Squares: abutting = 0, overlapping < 0, apart = distance.
        let a = Geo::new(vec![Prim::Poly(Box::new(square(0.0, 0.0, 10.0)))]);
        let b = Geo::new(vec![Prim::Poly(Box::new(square(10.0, 0.0, 10.0)))]);
        let c = Geo::new(vec![Prim::Poly(Box::new(square(5.0, 5.0, 10.0)))]);
        let d = Geo::new(vec![Prim::Poly(Box::new(square(2.0, 2.0, 2.0)))]);
        assert_eq!(gap(&a, &b, 1.0).unwrap().0, 0.0);
        assert!(gap(&a, &c, 1.0).unwrap().0 < 0.0);
        assert!(gap(&a, &d, 1.0).unwrap().0 < 0.0);
        let e = Geo::new(vec![Prim::Poly(Box::new(square(13.0, 0.0, 10.0)))]);
        assert_eq!(gap(&a, &e, 5.0).unwrap().0, 3.0);
        assert!(a.contains([5.0, 5.0]) && !a.contains([15.0, 5.0]));
    }

    #[test]
    fn arcs_are_finely_segmented() {
        let pts = arc_polyline([0.0, 0.0], 5e6, 0.0, 90.0);
        assert!(pts.len() > 20);
        let (x, y) = (pts.last().unwrap()[0], pts.last().unwrap()[1]);
        assert!(x.abs() < 1e-3 && (y - 5e6).abs() < 1e-3);
    }
}
