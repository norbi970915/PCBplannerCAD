//! Connected conductor deformation inside a speculative obstacle-world branch.
use crate::{Item, ItemId, Point};
use super::{CornerStyle, geom::{seg_seg_dist,V}, shove::Pusher, world::{Core,Obstacle,ObKind,Probe,World}};
use crate::pcb::elastic::{Topology,Chain,simple_path,simplify};
use std::collections::{HashMap,HashSet};

fn boundary(doc:&crate::Document, start:Point, chain:&Chain, limit:usize)->(Point,Vec<ItemId>,Vec<Point>){
    let mut at=start;let mut ids=Vec::new();let mut tail=Vec::new();
    for (index,id) in chain.edges.iter().enumerate(){
        let Some(Item::Track(t))=doc.get(*id) else{break};
        let next=if t.a==at{t.b}else{t.a};
        if index<limit{ids.push(*id);}else{if tail.is_empty(){tail.push(at);}tail.push(next);}
        at=next;
    }
    let end=tail.first().copied().unwrap_or(at);
    if tail.is_empty(){tail.push(at);}
    (end,ids,tail)
}

/// Expand a collision window only as far as necessary. All candidate changes
/// live in the overlay and are restored before returning a proposal.
pub(super) fn displace(world:&mut World, index:u32, pushers:&[Pusher], alternate:bool)->Option<(Vec<u32>,Vec<Point>)>{
    let mut result=None;
    visit_displacements(world,index,pushers,alternate,|_,indices,path|{result=Some((indices,path));true});
    result
}

/// Visit alternatives lazily so a blocked descendant can roll back and choose
/// another connected corridor without precomputing expensive fallbacks.
/// The callback returns true to stop; it retains any accepted result itself.
pub(super) fn visit_displacements(world:&mut World,index:u32,pushers:&[Pusher],alternate:bool,mut visit:impl FnMut(&mut World,Vec<u32>,Vec<Point>)->bool)->bool{
    let obstacle=world.obstacles[index as usize].clone();
    let Some(track)=obstacle.track.as_ref()else{return false;};let Some(id)=obstacle.id else{return false;};
    let topology=super::engine::cached_topology(world.doc).unwrap_or_else(||std::sync::Arc::new(Topology::new(world.doc)));let mut used=HashSet::from([id]);
    let left=topology.walk(track.a,track,&used);used.extend(left.edges.iter().copied());
    let right=topology.walk(track.b,track,&used);
    let lookup:HashMap<_,_>=world.obstacles.iter().enumerate().filter_map(|(i,o)|o.track.as_ref().and(o.id).map(|id|(id,i as u32))).collect();
    let mut attempted=HashSet::new();
    for size in [1,2,3,5,8,usize::MAX]{
        if super::engine::work_exhausted(){break;}
        let (a,mut ids,left_tail)=boundary(world.doc,track.a,&left,size);
        let (b,right_ids,right_tail)=boundary(world.doc,track.b,&right,size);
        ids.push(id);ids.extend(right_ids);ids.sort();ids.dedup();
        if !attempted.insert(ids.clone()){continue;}
        let indices:Option<Vec<_>>=ids.iter().map(|id|lookup.get(id).copied()).collect();
        let Some(indices)=indices else{continue};
        // A connected window must not absorb an interior fixed junction.
        // Those tracks use independent anchored subspan detours instead.
        if indices.iter().any(|i|world.removed.contains(i)||world.obstacles[*i as usize].id.is_some_and(|id|!topology.interior_joints(id).is_empty())){continue;}
        let removed=world.removed.clone();let added=world.added.clone();
        // Unmoved tracks remain candidates for recursive displacement. Existing
        // overlay tracks and all fixed copper continue to constrain this search.
        for (i,other) in world.obstacles.iter().enumerate(){
            if other.track.is_some(){world.removed.insert(i as u32);}
        }
        let first_pusher=world.added.len();
        for p in pushers.iter().filter(|p|p.net!=obstacle.net||p.net==super::world::NO_NET){
            let key=world.track_key(p.net);
            world.added.push(Obstacle{core:Core::Seg(p.a,p.b),r:p.hw,net:p.net,key,kind:ObKind::Track,id:None,track:None});
        }
        let excluded=[obstacle.net];let probe=Probe{hw:obstacle.r,net:obstacle.net,excl:&excluded};
        let ordered:Vec<_>=left.edges.iter().take(size).rev().chain(std::iter::once(&id)).chain(right.edges.iter().take(size)).copied().collect();
        let mut original=vec![a];for source in &ordered{let Some(Item::Track(t))=world.doc.get(*source)else{continue};let at=*original.last().unwrap();original.push(if t.a==at{t.b}else{t.a});}
        let hulls:Vec<_>=(first_pusher..world.added.len()).filter_map(|i|world.clearance_hull_with_margin(super::world::Hit::Added(i as u32),&probe,0.)).collect();
        let search_removed=world.removed.clone();let search_added=world.added.clone();
        for contour_first in [true,false]{
            if !contour_first{world.removed=search_removed.clone();world.added=search_added.clone();}
            let mut paths=Vec::new();
            if contour_first{
                for reverse in [false,true]{for clockwise in [alternate,!alternate]{if let Some(path)=super::contour::walk(&original,&hulls,clockwise,reverse){paths.push(path);}}}
            }else{
                for flip in [alternate,!alternate]{let walk=super::search::walkaround(world,&probe,a,b,CornerStyle::Diagonal45,flip);if walk.reached{paths.push(simplify(&walk.path));}}
            }
            world.removed=removed.clone();world.added=added.clone();
        paths.sort_by(|a,b|{
            let length=|p:&Vec<Point>|p.windows(2).map(|w|(V::of(w[1])-V::of(w[0])).len()).sum::<f64>();
            length(a).total_cmp(&length(b))
        });
        for path in paths{
            let mut whole:Vec<_>=left_tail.iter().rev().copied().collect();whole.extend(path.iter().skip(1).copied());whole.extend(right_tail.iter().skip(1).copied());
            if path.len()<2||!simple_path(&simplify(&whole)){continue;}
            if !path.windows(2).all(|w|pushers.iter().filter(|p|p.net!=obstacle.net||p.net==super::world::NO_NET).all(|p|{
                seg_seg_dist(V::of(w[0]),V::of(w[1]),p.a,p.b)>=obstacle.r+p.hw+world.clr(obstacle.key,p.net)-1.
            })){continue;}
            // Never trade a connected-chain movement for a collision with fixed
            // copper or an already displaced conductor.
            let clear=path.windows(2).all(|w|world.seg_collisions(V::of(w[0]),V::of(w[1]),&probe).into_iter().all(|hit|{
                matches!(hit,super::world::Hit::Ob(i) if world.obstacles[i as usize].track.is_some()&&!world.removed.contains(&i))
            }));
            if clear{
                if visit(world,indices.clone(),path){return true;}
                world.removed=removed.clone();world.added=added.clone();
            }
        }
        }
    }
    false
}

/// Preserve every source identity in the existing route-edit protocol without
/// duplicate copper. Extra collinear vertices are stable and electrically inert.
pub(super) fn assign(mut path:Vec<Point>, count:usize)->Option<Vec<Point>>{
    while path.len().checked_sub(1)?<count{
        let gcd=|mut a:u128,mut b:u128|{while b!=0{let rest=a%b;a=b;b=rest;}a};
        let (i,_,steps)=path.windows(2).enumerate().filter_map(|(i,w)|{let dx=w[1].x as i128-w[0].x as i128;let dy=w[1].y as i128-w[0].y as i128;let steps=gcd(dx.unsigned_abs(),dy.unsigned_abs());(steps>=2).then_some((i,dx.unsigned_abs().max(dy.unsigned_abs()),steps as i128))}).max_by_key(|(_,length,_)|*length)?;
        let a=path[i];let b=path[i+1];let half=steps/2;
        path.insert(i+1,Point::new((a.x as i128+(b.x as i128-a.x as i128)/steps*half)as i64,(a.y as i128+(b.y as i128-a.y as i128)/steps*half)as i64));
    }
    Some(path)
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{Command,DocKind,Item,Session,units::mm,pcb::{Track,Via,connectivity}};
    fn p(x:f64,y:f64)->Point{Point::new(mm(x),mm(y))}
    #[test]
    fn identity_splits_preserve_arbitrary_straight_geometry_exactly(){
        for(a,b)in[(Point::new(7,3),Point::new(47,23)),(Point::new(47,23),Point::new(7,3)),(Point::new(-7,3),Point::new(-47,23))]{
            let path=assign(vec![a,b],8).unwrap();assert_eq!(path.len(),9);assert_eq!(path[0],a);assert_eq!(*path.last().unwrap(),b);assert!(simple_path(&path));for point in path{assert_eq!((b.x-a.x)as i128*(point.y-a.y)as i128,(b.y-a.y)as i128*(point.x-a.x)as i128);}
        }
        assert!(assign(vec![Point::new(0,0),Point::new(7,3)],2).is_none());
    }
    #[test]
    fn a_bend_can_move_as_a_chain_when_its_short_segment_cannot_detour(){
        let mut session=Session::new(DocKind::Pcb);
        let points=[p(0.,0.),p(2.,0.),p(3.,1.),p(3.,4.),p(8.,4.)];
        let via=|pos|Item::Via(Via{pos,diameter:mm(0.6),drill:mm(0.3),from_layer:crate::LayerId(0),to_layer:crate::LayerId(1),net:"B".into(),locked:false});
        let mut items=vec![Item::Board(Default::default()),via(points[0]),via(points[4])];
        items.extend(points.windows(2).map(|w|Item::Track(Track{a:w[0],b:w[1],width:mm(0.25),layer:crate::LayerId(0),net:"B".into(),locked:false})));
        session.execute(Command::Add{items}).unwrap();let source=session.document().clone();
        let mut world=World::build(&source,crate::LayerId(0),&[]);let net=world.net_id("A");
        let root=Pusher{a:V::of(p(2.3,-1.)),b:V::of(p(2.3,2.)),hw:mm(0.25)as f64/2.,net};
        let moved=super::super::shove::shove_many(&mut world,&[root]).unwrap();
        assert!(moved.len()>1,"neighboring bends were left pinned");
        let delete=moved.iter().map(|(id,_)|*id).collect();let add=moved.into_iter().flat_map(|(_,tracks)|tracks.into_iter().map(Item::Track)).collect();
        session.execute(Command::Route{replace:vec![],delete,add}).unwrap();
        assert_eq!(connectivity::ratsnest(session.document()).len(),0);
        for (_,item) in session.document().items(){if let Item::Track(t)=item{
            assert!(seg_seg_dist(V::of(t.a),V::of(t.b),root.a,root.b)>=mm(0.45)as f64-1.);
        }}
        session.undo().unwrap();assert_eq!(session.document().items().collect::<Vec<_>>(),source.items().collect::<Vec<_>>());
    }
    #[test]
    fn rejected_connected_candidates_restore_every_caller_overlay(){
        let mut session=Session::new(DocKind::Pcb);let points=[p(0.,0.),p(2.,0.),p(3.,1.),p(3.,4.),p(8.,4.)];
        let mut items=vec![Item::Board(Default::default())];
        items.extend(points.windows(2).map(|w|Item::Track(Track{a:w[0],b:w[1],width:mm(0.25),layer:crate::LayerId(0),net:"B".into(),locked:false})));
        session.execute(Command::Add{items}).unwrap();let source=session.document().clone();
        let before=crate::io::document_to_string(&source).unwrap();let _work=super::super::engine::WorkScope::enter();
        let mut world=World::build(&source,crate::LayerId(0),&[]);let net=world.net_id("A");
        let root=Pusher{a:V::of(p(2.3,-1.)),b:V::of(p(2.3,2.)),hw:mm(0.25)as f64/2.,net};
        let index=world.obstacles.iter().position(|o|o.track.as_ref().is_some_and(|t|t.a==p(2.,0.))).unwrap()as u32;
        let key=world.track_key(net);world.added.push(Obstacle{core:Core::Seg(V::of(p(50.,50.)),V::of(p(51.,50.))),r:0.,net,key,kind:ObKind::Track,id:None,track:None});
        let removed=world.removed.clone();let added=format!("{:?}",world.added);let mut visited=0;
        let stopped=visit_displacements(&mut world,index,&[root],false,|world,_,_|{visited+=1;world.removed.insert(u32::MAX);world.added.clear();false});
        assert!(!stopped);assert!(visited>0);assert_eq!(world.removed,removed);assert_eq!(format!("{:?}",world.added),added);
        assert_eq!(crate::io::document_to_string(&source).unwrap(),before);
    }

}