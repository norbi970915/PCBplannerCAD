//! Length tuning: one-sided meanders ("bumps") inserted along a chain of
//! same-net segments. Candidate slots are laid out every `2 * spacing` along
//! each segment (away from its ends); each slot is tried on either side at
//! its required height and bounded smaller heights against copper, the outline
//! and the chain itself. Existing contacts are protected, and the complete
//! rounded result is checked again before it can replace the source tracks.

use crate::document::{Document, ItemId, LayerId};
use crate::item::Item;
use crate::pcb::items::Track;
use crate::units::{Coord, Point};

use super::geom::{octant, seg_seg_dist, FxMap, V};
use super::world::{Probe, World};
use super::{TuneRequest, TuneStyle};

/// Relative length tolerance.
pub(super) const TOLERANCE: f64 = 0.005;

/// A segment of the ordered chain with its integer lattice frame.
struct Seg {
    t: Track,
    len: f64,
    /// Lattice direction / unit length (octilinear) or float frame.
    e: (Coord, Coord),
    unit: f64,
    u: V,
    oct: bool,
}

impl Seg {
    fn new(t: Track) -> Seg {
        let (dx, dy) = (t.b.x - t.a.x, t.b.y - t.a.y);
        let len = (dx as f64).hypot(dy as f64);
        let oct = octant(dx, dy).is_some();
        let e = (dx.signum(), dy.signum());
        let unit = if e.0 != 0 && e.1 != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
        Seg { len, e, unit, u: V::new(dx as f64 / len, dy as f64 / len), oct, t }
    }

    /// World point at distance `t` along and `h` to the left (side ±1).
    fn at(&self, t: f64, h: f64, side: f64) -> Point {
        if self.oct {
            let i = (t / self.unit).round() as Coord;
            let j = (h * side / self.unit).round() as Coord;
            Point::new(self.t.a.x + self.e.0 * i - self.e.1 * j, self.t.a.y + self.e.1 * i + self.e.0 * j)
        } else {
            (V::of(self.t.a) + self.u * t + self.u.perp() * (h * side)).round()
        }
    }

    /// Snaps a length to the segment's lattice.
    fn snap(&self, x: f64) -> f64 {
        if self.oct { (x / self.unit).round() * self.unit } else { x.round() }
    }
}

fn chamfer(style: TuneStyle, s: f64, h: f64) -> f64 {
    match style {
        TuneStyle::Trombone => 0.0,
        TuneStyle::Accordion => s.min(h) * 0.25,
    }
}

/// Length a bump of height `h` adds.
fn extra(style: TuneStyle, s: f64, h: f64) -> f64 {
    2.0 * h - chamfer(style, s, h) * (8.0 - 4.0 * std::f64::consts::SQRT_2)
}

fn bump(seg: &Seg, style: TuneStyle, t0: f64, s: f64, h: f64, side: f64) -> Vec<Point> {
    let c = seg.snap(chamfer(style, s, h));
    let (h, s) = (seg.snap(h), seg.snap(s));
    let local: Vec<(f64, f64)> = if c > 0.0 {
        vec![(t0 - c, 0.0), (t0, c), (t0, h - c), (t0 + c, h), (t0 + s - c, h), (t0 + s, h - c), (t0 + s, c), (t0 + s + c, 0.0)]
    } else {
        vec![(t0, 0.0), (t0, h), (t0 + s, h), (t0 + s, 0.0)]
    };
    local.into_iter().map(|(t, y)| seg.at(t, y, side)).collect()
}

/// Orders the tracks into a chain (endpoint-to-endpoint).
fn order_chain(doc: &Document, ids: &[ItemId]) -> Result<Vec<Track>, String> {
    let mut tracks = Vec::new();
    let mut unique = super::geom::FxSet::default();
    for id in ids {
        if !unique.insert(*id) { return Err("select each track only once".into()); }
        match doc.get(*id) {
            Some(Item::Track(t)) if t.locked => return Err(format!("track {} is locked; unlock it before tuning", id.0)),
            Some(Item::Track(t)) if t.a == t.b || t.width <= 0 => return Err("degenerate tracks cannot be tuned".into()),
            Some(Item::Track(t)) => tracks.push(t.clone()),
            Some(_) => return Err(format!("item {} is not a track", id.0)),
            None => return Err(format!("item {} does not exist", id.0)),
        }
    }
    if tracks.is_empty() {
        return Err("no tracks to tune".into());
    }
    if tracks.iter().any(|t| t.net != tracks[0].net) {
        return Err("tracks belong to different nets".into());
    }
    let mut ends: FxMap<(Coord, Coord), Vec<usize>> = FxMap::default();
    for (i, t) in tracks.iter().enumerate() {
        ends.entry((t.a.x, t.a.y)).or_default().push(i);
        ends.entry((t.b.x, t.b.y)).or_default().push(i);
    }
    if ends.values().any(|v| v.len() > 2) {
        return Err("tracks branch; select a single chain".into());
    }
    let start = tracks.iter().flat_map(|t| [t.a, t.b]).find(|p| ends[&(p.x, p.y)].len() == 1).ok_or("tracks form a loop")?;
    let mut used = vec![false; tracks.len()];
    let mut out = Vec::with_capacity(tracks.len());
    let mut at = start;
    while let Some(&i) = ends[&(at.x, at.y)].iter().find(|&&i| !used[i]) {
        used[i] = true;
        let mut t = tracks[i].clone();
        if t.a != at {
            std::mem::swap(&mut t.a, &mut t.b);
        }
        at = t.b;
        out.push(t);
    }
    if out.len() != tracks.len() {
        return Err("tracks are not one connected chain".into());
    }
    Ok(out)
}

pub(super) fn tune(doc: &Document, req: &TuneRequest) -> Result<Vec<Track>, String> {
    if req.target_length <= 0 { return Err("target length must be positive".into()); }
    if req.amplitude <= 0 || req.spacing <= 0 { return Err("amplitude and spacing must be positive".into()); }
    let chain = order_chain(doc, &req.tracks)?;
    let target = req.target_length as f64;
    let current: f64 = chain.iter().map(|t| (V::of(t.b) - V::of(t.a)).len()).sum();
    let tol = target * TOLERANCE;
    if (current - target).abs() <= tol { return Ok(chain); }
    if current > target { return Err(format!("chain is already {:.3} mm, longer than the {:.3} mm target", current / 1e6, target / 1e6)); }
    let need = target - current;
    let (s, amp, style) = (req.spacing as f64, req.amplitude as f64, req.style);
    let net = chain[0].net.clone();
    let segs: Vec<Seg> = chain.into_iter().map(Seg::new).collect();
    // Pours refill around committed tracks; fixed copper and keepouts remain obstacles.
    let ignored = super::routing_ignored(doc, &req.tracks);
    let mut worlds: FxMap<LayerId, (World, u32)> = FxMap::default();
    for sg in &segs {
        worlds.entry(sg.t.layer).or_insert_with(|| { let mut w = World::build(doc, sg.t.layer, &ignored); let id = w.net_id(&net); (w, id) });
    }
    for sg in &segs {
        let (world, nid) = &worlds[&sg.t.layer];
        let minimum = sg.t.width as f64 + world.track_clearance(*nid);
        if s + 1.0 < minimum { return Err(format!("meander spacing is too small: use at least {:.3} mm for the track width and clearance", minimum/1e6)); }
    }

    // Each accepted slot stores its actual height. Never validate one shape and
    // then shrink it into a different, unchecked obstacle configuration.
    let mut slots: Vec<(usize, f64, f64, f64)> = Vec::new();
    let mut accepted: Vec<(LayerId, f64, Vec<Point>)> = Vec::new();
    let mut supplied = 0.0;
    let mut side = 1.0;
    let mut attempts = 0;
    'segs: for (si, sg) in segs.iter().enumerate() {
        let (world, nid) = &worlds[&sg.t.layer];
        let width = sg.t.width as f64;
        let clearance = world.track_clearance(*nid);
        let margin = sg.snap(s.max(width + clearance) + chamfer(style, s, amp));
        let excl = [*nid];
        let probe = Probe { hw: width / 2.0, net: *nid, excl: &excl };
        let fixed: Vec<_> = world.obstacles.iter().filter(|o| o.net == *nid && *nid != super::world::NO_NET
            && matches!(o.kind, super::world::ObKind::Pad | super::world::ObKind::Via | super::world::ObKind::Track | super::world::ObKind::Arc)).collect();
        let mut t0 = margin;
        while t0 + s + margin <= sg.len {
            attempts += 1;
            if attempts > 16384 { break 'segs; }
            let remaining = need - supplied;
            if remaining <= tol { break 'segs; }
            let (mut lo, mut hi) = (0.0, amp);
            for _ in 0..48 { let mid = (lo+hi)/2.0; if extra(style,s,mid)<remaining {lo=mid;} else {hi=mid;} }
            let mut best: Option<(f64,f64,f64,Vec<Point>)> = None;
            for sd in [side,-side] {
                let mut height = hi.min(amp);
                // Amplitude is a maximum, not a requirement to reach full height.
                for _ in 0..12 {
                    if height < 1.0 { break; }
                    let mut pts = bump(sg, style, sg.snap(t0), s, height, sd);pts.dedup();
                    if pts.len()<3 { height*=0.5;continue; }
                    let first=V::of(pts[0]);let last=V::of(*pts.last().unwrap());
                    let gain=pts.windows(2).map(|p|(V::of(p[1])-V::of(p[0])).len()).sum::<f64>()-(last-first).len();
                    // Retain any existing physical contact on the portion being replaced.
                    let protected = fixed.iter().any(|o| o.core.seg_dist(first,last) <= o.r + width/2.0 + 1.0);
                    let clean = !protected && gain>0.5 && gain<=remaining+tol && world.path_clear(&pts,&probe)
                        && pts.windows(2).all(|p| {
                            let (a,b)=(V::of(p[0]),V::of(p[1]));
                            segs.iter().enumerate().all(|(k,other)| k==si || other.t.layer!=sg.t.layer
                                || seg_seg_dist(a,b,V::of(other.t.a),V::of(other.t.b)) >= (width+other.t.width as f64)/2.0+clearance)
                            && accepted.iter().all(|(layer,w,acc)| *layer!=sg.t.layer || acc.windows(2).all(|q|
                                seg_seg_dist(a,b,V::of(q[0]),V::of(q[1])) >= (width+w)/2.0+clearance))
                            && fixed.iter().all(|o| o.core.seg_dist(a,b) >= o.r+width/2.0+world.clr(o.key,*nid))
                        });
                    if clean {
                        if best.as_ref().is_none_or(|(_,_,old_gain,_)|gain>*old_gain) { best=Some((height,sd,gain,pts)); }
                        break;
                    }
                    height*=0.5;
                }
            }
            if let Some((height,sd,gain,pts))=best {
                side=sd;supplied+=gain;slots.push((si,sg.snap(t0),sd,height));accepted.push((sg.t.layer,width,pts));
                if (need-supplied).abs()<=tol {break 'segs;}
            }
            t0 += 2.0*s;
        }
    }
    if attempts>16384 && (supplied-need).abs()>tol { return Err("Tuning search limit reached; select a shorter chain or reduce the target".into()); }
    if (supplied-need).abs()>tol { return Err(format!("not enough room for meanders: can add {:.3} mm of the {:.3} mm needed", supplied/1e6, need/1e6)); }
    let mut out = Vec::new();
    for (si, sg) in segs.iter().enumerate() {
        let mut pts = vec![sg.t.a];
        for &(_,t0,sd,height) in slots.iter().filter(|slot|slot.0==si) {pts.extend(bump(sg,style,t0,s,height,sd));}
        pts.push(sg.t.b);pts.dedup();
        out.extend(pts.windows(2).map(|p|Track{a:p[0],b:p[1],..sg.t.clone()}));
    }
    // Recheck the complete result, including unchanged connectors and rounded corners.
    for track in &out {
        let (world,nid)=&worlds[&track.layer];let excl=[*nid];
        if !world.path_clear(&[track.a,track.b],&Probe{hw:track.width as f64/2.0,net:*nid,excl:&excl}) {
            return Err("final tuning geometry violates clearance; source tracks were kept".into());
        }
    }
    let total: f64 = out.iter().map(|t|(V::of(t.b)-V::of(t.a)).len()).sum();
    if (total-target).abs()>tol {return Err(format!("could not reach the target length ({:.3} of {:.3} mm)",total/1e6,target/1e6));}
    Ok(out)
}
