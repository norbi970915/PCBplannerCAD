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

pub(super) enum Detour {
    NotNeeded,
    Points(Vec<Point>),
}

/// Raised detour of track `a`–`b` clearing every pusher (`(a, b, required
/// centerline distance)`), or `None` when it cannot fit inside the segment.
pub(super) fn detour(a: Point, b: Point, pushers: &[(V, V, f64)]) -> Option<Detour> {
    detour_side(a,b,pushers,None)
}

pub(super) fn detour_side(a: Point, b: Point, pushers: &[(V, V, f64)], side: Option<f64>) -> Option<Detour> {
    let (av, bv) = (V::of(a), V::of(b));
    let len = (bv - av).len();
    if len < 1.0 {
        return None;
    }
    // Charge batches of pusher scans, including repeated window growth and
    // candidate verification. Local detours share the walkaround allowance.
    let scan = || super::engine::consume_work(pushers.len().div_ceil(32).max(1));
    if !scan() { return None; }
    let conflicts: Vec<&(V, V, f64)> = pushers.iter().filter(|p| seg_seg_dist(av, bv, p.0, p.1) < p.2).collect();
    if conflicts.is_empty() {
        return Some(Detour::NotNeeded);
    }
    let u = (bv - av) * (1.0 / len);
    let req_max = pushers.iter().map(|p| p.2).fold(0.0, f64::max);
    let oct = octant(b.x - a.x, b.y - a.y).is_some();
    let mut best: Option<(f64, Vec<Point>)> = None;
    for sigma in [1.0, -1.0] {
        if side.is_some_and(|s|s!=sigma) {continue;}
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
                if !scan() { return None; }
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
            if !scan() { return None; }
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

/// Treat every interior contact as an exact endpoint of independent subspans.
/// A free section may deform while fixed branch, pad and via centers stay put.
pub(super) fn anchored_detour(a:Point,b:Point,joints:&[Point],pushers:&[(V,V,f64)],side:Option<f64>)->Option<Detour>{
    if joints.is_empty(){return match side{None=>detour(a,b,pushers),_=>detour_side(a,b,pushers,side)};}
    let mut anchors=joints.to_vec();
    let dx=b.x as i128-a.x as i128;let dy=b.y as i128-a.y as i128;
    anchors.sort_by(|p,q|if dx.abs()>=dy.abs(){p.x.cmp(&q.x)}else{p.y.cmp(&q.y)});
    if (if dx.abs()>=dy.abs(){dx}else{dy})<0 {anchors.reverse();}
    anchors.dedup();anchors.insert(0,a);anchors.push(b);
    let mut path=vec![a];let mut changed=false;
    for span in anchors.windows(2) {
        match detour_side(span[0],span[1],pushers,side)? {
            Detour::NotNeeded=>path.push(span[1]),
            Detour::Points(points)=>{changed=true;path.extend(points.into_iter().skip(1));}
        }
    }
    if !changed{return Some(Detour::NotNeeded);}
    // Separate subspans must not fold back across each other.
    crate::pcb::elastic::simple_path(&path).then_some(Detour::Points(path))
}

/// Generate each direction only when the preceding candidate was rejected.
pub(super) fn alternatives<'a>(a:Point,b:Point,joints:&'a [Point],pushers:&'a [(V,V,f64)],mut nearest:Option<Detour>,alternate:bool)->impl Iterator<Item=Vec<Point>>+'a {
    let mut order=[0,1,2];if alternate{order.reverse();}
    order.into_iter().filter_map(move |choice|{
        let candidate=match choice{0=>nearest.take(),1=>anchored_detour(a,b,joints,pushers,Some(-1.)),_=>anchored_detour(a,b,joints,pushers,Some(1.))};
        match candidate{Some(Detour::Points(points))=>Some(points),_=>None}
    })
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
    let pushers: Vec<Pusher> = path.windows(2).map(|w| Pusher { a: V::of(w[0]), b: V::of(w[1]), hw: probe.hw, net: probe.net }).collect();
    shove_many(world, &pushers)
}

/// Plan all selected conductors together so pushed tracks clear the entire drag.
pub(super) fn shove_many(world: &mut World, roots: &[Pusher]) -> Result<Vec<(ItemId, Vec<Track>)>, String> {
    let _work=super::engine::WorkScope::enter();
    let removed=world.removed.clone();let added=world.added.clone();
    let first={let _quick=super::engine::WorkSlice::reserving(128);shove_attempt(world,roots,false)};
    if first.is_ok(){return first;}
    world.removed=removed.clone();world.added=added.clone();
    let second={let _quick=super::engine::WorkSlice::reserving(128);shove_attempt(world,roots,true)};
    if second.is_ok(){return second;}
    world.removed=removed;world.added=added;
    let alternative=super::backtrack::solve(world,roots);
    match alternative{
        Ok(moves)=>Ok(moves),
        Err(super::backtrack::Failure::Limit)=>Err("Routing search limit reached while reconsidering shove dependencies".into()),
        Err(super::backtrack::Failure::Blocked)=>first,
    }
}

fn shove_attempt(world: &mut World, roots: &[Pusher], alternate: bool) -> Result<Vec<(ItemId, Vec<Track>)>, String> {
    let topology=super::engine::cached_topology(world.doc).unwrap_or_else(||std::sync::Arc::new(crate::pcb::elastic::Topology::new(world.doc)));
    let movable=|world:&World,index:u32|world.obstacles[index as usize].track.is_some();
    let mut pushers = roots.to_vec();
    let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
    let mut queued: FxSet<u32> = FxSet::default();
    let blocked = |world: &World, h: Hit| {
        let detail=match h {Hit::Ob(index)=>{let ob=&world.obstacles[index as usize];format!("{}{} on net '{}'",ob.kind.name(),ob.id.map(|id|format!(" #{}",id.0)).unwrap_or_default(),world.net_name(ob.net))},_=>world.hit_kind(h).name().to_string()};
        format!("blocked by a {detail} on layer {}",world.layer.0)
    };
    for p in &pushers {
        let excluded = [p.net];
        let probe = Probe { hw:p.hw, net:p.net, excl:&excluded };
        for h in world.seg_collisions(p.a, p.b, &probe) {
            match h {
                Hit::Ob(i) if movable(world,i) => {
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
        if world.removed.contains(&i) {continue;}
        let ob = world.obstacles[i as usize].clone();
        let track = ob.track.clone().ok_or("locked track cannot be pushed")?;
        let reqs: Vec<(V, V, f64)> = pushers.iter().filter(|p| p.net != ob.net || p.net == super::world::NO_NET).map(|p| (p.a, p.b, ob.r + p.hw + world.clr(ob.key, p.net))).collect();
        let joints=ob.id.map_or(&[][..],|id|topology.interior_joints(id));
        let nearest=anchored_detour(track.a,track.b,joints,&reqs,None);
        if matches!(nearest,Some(Detour::NotNeeded)){continue;}
        let excluded=[ob.net];let candidate_probe=Probe{hw:ob.r,net:ob.net,excl:&excluded};
        let local=alternatives(track.a,track.b,joints,&reqs,nearest,alternate).find_map(|points|{
            // Fixed copper rules out this side before propagating neighboring tracks.
            let clear=points.windows(2).all(|w|world.seg_collisions(V::of(w[0]),V::of(w[1]),&candidate_probe).into_iter().all(|hit|matches!(hit,Hit::Ob(j)if movable(world,j)&&!world.removed.contains(&j))));
            clear.then_some(points)
        });
        let (indices,points)=if let Some(path)=local {(vec![i],path)}else{
            super::chain::displace(world,i,&pushers,alternate).ok_or_else(||{
                let reason=if super::engine::work_exhausted(){"Routing search limit reached while displacing"}else{"No clear corridor is available for"};
                format!("{reason} connected track{} on net '{}' on layer {}",ob.id.map(|id|format!(" #{}",id.0)).unwrap_or_default(),track.net,world.layer.0)
            })?
        };
        let pts=super::chain::assign(points,indices.len()).ok_or("Connected track collapsed below routing resolution")?;
        for index in &indices {world.removed.insert(*index);}
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
                    Hit::Ob(j) if movable(world,j) && !world.removed.contains(&j) => {
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
        for (ordinal,index) in indices.iter().enumerate() {
            if let Some(id)=world.obstacles[*index as usize].id {
                moved.push((id,pieces.iter().enumerate().filter(|(piece,_)|piece%indices.len()==ordinal).map(|(_,track)|track.clone()).collect()));
            }
        }
    }
    // The route itself must now be clean against the edited world.
    for p in roots {
        let excluded = [p.net];
        let probe = Probe { hw:p.hw, net:p.net, excl:&excluded };
        if !world.seg_clear(p.a, p.b, &probe) {
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
    #[test]
    fn independent_shove_dependencies_need_mixed_corridor_choices() {
        use crate::{Command,DocKind,Item,Session,LayerId,units::mm};
        let p=|x:f64,y:f64|Point::new(mm(x),mm(y));
        let track=|net:&str,a,b|Item::Track(Track{a,b,width:mm(0.25),layer:LayerId(0),net:net.into(),locked:false});
        let mut session=Session::new(DocKind::Pcb);
        session.execute(Command::Add{items:vec![Item::Board(Default::default()),
            track("LEFT",p(-10.,0.),p(-2.,0.)),track("RIGHT",p(2.,0.),p(10.,0.)),
            track("LOWER_STOP",p(-6.2,-0.6),p(-5.8,-0.6)),track("UPPER_STOP",p(5.8,1.),p(6.2,1.))
        ]}).unwrap();
        let source=session.document().clone();let before=crate::io::document_to_string(&source).unwrap();
        let mut world=World::build(&source,LayerId(0),&[]);let net=world.net_id("ROOT");
        let roots=[Pusher{a:V::of(p(-7.,0.2)),b:V::of(p(-5.,0.2)),hw:mm(0.25)as f64/2.,net},Pusher{a:V::of(p(5.,0.2)),b:V::of(p(7.,0.2)),hw:mm(0.25)as f64/2.,net}];
        let moved=shove_many(&mut world,&roots).expect("each independent dependency has a clear side");
        assert!(moved.iter().flat_map(|(_,tracks)|tracks).any(|t|t.net=="LEFT"&&(t.a.y>mm(0.4)||t.b.y>mm(0.4))));
        assert!(moved.iter().flat_map(|(_,tracks)|tracks).any(|t|t.net=="RIGHT"&&(t.a.y<0||t.b.y<0)));
        let delete=moved.iter().map(|(id,_)|*id).collect();
        let mut add:Vec<_>=moved.into_iter().flat_map(|(_,tracks)|tracks.into_iter().map(Item::Track)).collect();
        add.extend(roots.iter().map(|r|track("ROOT",r.a.round(),r.b.round())));
        let delta=session.execute_with_via_checks(Command::Route{replace:vec![],delete,add},true).unwrap();
        let touched:Vec<_>=delta.upserted.iter().map(|r|r.id).collect();
        let errors:Vec<_>=crate::pcb::drc::run_near(session.document(),&touched).into_iter().filter(|v|v.severity==crate::pcb::drc::Severity::Error).collect();
        assert!(errors.is_empty(),"{errors:?}");
        let after=crate::io::document_to_string(session.document()).unwrap();
        session.undo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),before);
        session.redo().unwrap();assert_eq!(crate::io::document_to_string(session.document()).unwrap(),after);
    }
    #[test]
    fn simple_shove_cannot_bypass_an_exhausted_operation_allowance() {
        use crate::{DocKind,Document,Item,LayerId,units::mm};
        let p=|x:f64,y:f64|Point::new(mm(x),mm(y));
        let source=Document::from_item_list(DocKind::Pcb,[
            Item::Board(Default::default()),
            Item::Track(Track{a:p(-10.,0.),b:p(10.,0.),width:mm(0.25),layer:LayerId(0),net:"OTHER".into(),locked:false})
        ]);
        let original=crate::io::document_to_string(&source).unwrap();
        let mut world=World::build(&source,LayerId(0),&[]);
        let net=world.net_id("ROOT");
        let roots=[Pusher{a:V::of(p(-1.,0.2)),b:V::of(p(1.,0.2)),hw:mm(0.25)as f64/2.,net}];
        {
            let _work=super::super::engine::WorkScope::enter();
            assert!(super::super::engine::consume_work(6000));
            let error=shove_many(&mut world,&roots).expect_err("even an easy shove must honor the enclosing allowance");
            assert!(error.contains("search limit"),"{error}");
            assert!(world.removed.is_empty());assert!(world.added.is_empty());
            assert!(!super::super::engine::consume_work(1));
        }
        assert!(!shove_many(&mut world,&roots).unwrap().is_empty(),"a fresh preview gets its own allowance");
        assert_eq!(crate::io::document_to_string(&source).unwrap(),original);
    }

    #[test]
    fn local_shove_limit_rolls_back_completed_prefixes_and_keeps_caller_copper() {
        use crate::{DocKind,Document,Item,LayerId,units::mm};
        let p=|x:f64,y:f64|Point::new(mm(x),mm(y));
        let source=Document::from_item_list(DocKind::Pcb,[
            Item::Board(Default::default()),
            Item::Track(Track{a:p(-20.,0.),b:p(-2.,0.),width:mm(0.25),layer:LayerId(0),net:"LEFT".into(),locked:false}),
            Item::Track(Track{a:p(2.,0.),b:p(20.,0.),width:mm(0.25),layer:LayerId(0),net:"RIGHT".into(),locked:false})
        ]);
        let original=crate::io::document_to_string(&source).unwrap();
        let mut world=World::build(&source,LayerId(0),&[]);let net=world.net_id("ROOT");let key=world.track_key(net);
        let roots=[(-12.,-10.),(10.,12.)].map(|(a,b)|Pusher{a:V::of(p(a,0.2)),b:V::of(p(b,0.2)),hw:mm(0.25)as f64/2.,net});
        world.added.push(Obstacle{core:Core::Seg(V::of(p(50.,50.)),V::of(p(51.,50.))),r:mm(0.25)as f64/2.,net,key,kind:ObKind::Track,id:None,track:None});
        let removed=world.removed.clone();let added=format!("{:?}",world.added);
        {
            let _work=super::super::engine::WorkScope::enter();let _slice=super::super::engine::WorkSlice::enter(12);
            assert!(shove_attempt(&mut world,&roots,false).is_err());
            assert!(!world.removed.is_empty(),"the first independent conductor must have been displaced before failure");
        }
        world.removed=removed.clone();world.added.truncate(1);
        {
            let _work=super::super::engine::WorkScope::enter();let _slice=super::super::engine::WorkSlice::enter(12);
            let error=shove_many(&mut world,&roots).unwrap_err();
            assert!(error.contains("search limit"),"{error}");
            assert_eq!(world.removed,removed);assert_eq!(format!("{:?}",world.added),added);
            assert!(super::super::engine::work_exhausted());
        }
        assert_eq!(shove_many(&mut world,&roots).unwrap().len(),2);
        assert_eq!(crate::io::document_to_string(&source).unwrap(),original);
    }

    #[test]
    fn an_accepted_local_corridor_leaves_work_for_later_candidates() {
        use crate::{DocKind,Document,Item,LayerId,units::mm};
        let p=|x:f64,y:f64|Point::new(mm(x),mm(y));
        let source=Document::from_item_list(DocKind::Pcb,[
            Item::Board(Default::default()),
            Item::Track(Track{a:p(-10.,0.),b:p(10.,0.),width:mm(0.25),layer:LayerId(0),net:"OTHER".into(),locked:false})
        ]);
        let mut world=World::build(&source,LayerId(0),&[]);let net=world.net_id("ROOT");
        let roots=[Pusher{a:V::of(p(-1.,0.2)),b:V::of(p(1.,0.2)),hw:mm(0.25)as f64/2.,net}];
        let _work=super::super::engine::WorkScope::enter();let _slice=super::super::engine::WorkSlice::enter(12);
        assert_eq!(shove_many(&mut world,&roots).unwrap().len(),1);
        assert!(super::super::engine::consume_work(1),"unused directions must not spend the rest of the candidate allowance");
    }

}
