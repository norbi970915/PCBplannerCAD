//! Bounded obstacle-corner search before the grid fallback. Nodes sit on
//! clearance contours, avoiding expansion of thousands of empty grid cells.
use super::*;
use std::collections::HashSet;
const MAX_HULLS:usize=6;
const MAX_STATES:usize=96;
const MAX_EXPANSIONS:usize=16;
struct Candidate {path:Vec<Point>,cost:f64}
fn add_hulls(world:&World,probe:&Probe,hits:Vec<super::super::world::Hit>,seen:&mut Vec<super::super::world::Hit>,corners:&mut Vec<Point>){
    for hit in hits {
        if seen.len()>=MAX_HULLS{break;}
        if seen.contains(&hit){continue;}seen.push(hit);
        if let Some(hull)=world.clearance_hull(hit,probe){for point in hull{if !corners.contains(&point){corners.push(point);}}}
    }
}
fn collisions(world:&World,probe:&Probe,path:&[Point],remaining:&mut usize)->Option<Vec<super::super::world::Hit>>{
    let cost=path.len().saturating_sub(1);
    if *remaining<cost||!super::super::engine::consume_work(cost){return None;}
    *remaining-=cost;
    Some(path.windows(2).flat_map(|w|world.seg_collisions(V::of(w[0]),V::of(w[1]),probe)).collect())
}
pub(super) fn walk(world:&World,probe:&Probe,start:Point,end:Point,style:CornerStyle,flip:bool)->Option<Vec<Point>>{
    let mut remaining=128;
    let mut goal_remaining=64;
    let mut states=vec![Candidate{path:vec![start],cost:0.}];
    let mut queue=BinaryHeap::new();queue.push(Reverse(((V::of(end)-V::of(start)).len().to_bits(),0usize)));
    let mut seen=Vec::new();let mut corners=Vec::new();let mut best=FxMap::default();let mut expanded=HashSet::new();
    for _ in 0..MAX_EXPANSIONS {
        let Some(Reverse((_,index)))=queue.pop()else{break};
        let path=states[index].path.clone();let cost=states[index].cost;let current=*path.last()?;
        let incoming=if path.len()>1{vec_of(path[path.len()-2],current)}else{(0,0)};
        if !expanded.insert((current,(incoming.0.signum(),incoming.1.signum()))){continue;}
        for connection in connections(current,end,style,flip){
            let blocked=collisions(world,probe,&connection,&mut goal_remaining)?;
            if blocked.is_empty()&&path_turns_ok(incoming,&connection,(0,0),style){
                let mut joined=path.clone();joined.extend(connection.iter().skip(1).copied());
                let joined=smooth(world,probe,&joined,style,flip);
                if crate::pcb::elastic::simple_path(&joined)&&world.path_clear(&joined,probe){return Some(joined);}
            }
            add_hulls(world,probe,blocked,&mut seen,&mut corners);
        }
        let mut ordinal=0;
        'corners: while ordinal<corners.len() {
            let target=corners[ordinal];ordinal+=1;
            if target==current||path.contains(&target){continue;}
            for connection in connections(current,target,style,flip){
                if !path_turns_ok(incoming,&connection,(0,0),style){continue;}
                let Some(blocked)=collisions(world,probe,&connection,&mut remaining)else{break 'corners;};
                if !blocked.is_empty(){add_hulls(world,probe,blocked,&mut seen,&mut corners);continue;}
                let mut joined=path.clone();joined.extend(connection.iter().skip(1).copied());joined=simplify(&joined);
                if joined.len()>24||!crate::pcb::elastic::simple_path(&joined){continue;}
                let heading=vec_of(joined[joined.len()-2],target);
                let next_cost=cost+connection.windows(2).map(|w|(V::of(w[1])-V::of(w[0])).len()).sum::<f64>()+connection.len() as f64*probe.hw*0.05;
                let key=(target,(heading.0.signum(),heading.1.signum()));
                if best.get(&key).is_some_and(|old:&f64|*old<=next_cost){continue;}
                best.insert(key,next_cost);
                if states.len()>=MAX_STATES{break 'corners;}
                let priority=next_cost+(V::of(end)-V::of(target)).len();let id=states.len();states.push(Candidate{path:joined,cost:next_cost});queue.push(Reverse((priority.to_bits(),id)));
            }
        }
    }
    None
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{Document,DocKind,Item,LayerId,pcb::Via,units::mm};
    fn p(x:f64,y:f64)->Point{Point::new(mm(x),mm(y))}
    #[test]
    fn clearance_contours_route_around_round_and_overlapping_obstacles_without_grid_search(){
        for reverse in [false,true]{
            let via=|pos|Item::Via(Via{pos,diameter:mm(0.8),drill:mm(0.3),from_layer:LayerId(0),to_layer:LayerId(1),net:"B".into(),locked:true});
            let doc=Document::from_item_list(DocKind::Pcb,[Item::Board(Default::default()),via(p(4.,0.)),via(p(5.,0.6))]);
            let mut world=World::build(&doc,LayerId(0),&[]);let net=world.net_id("A");let excluded=[net];let probe=Probe{hw:mm(0.25)as f64/2.,net,excl:&excluded};
            let (a,b)=if reverse{(p(10.,0.),p(0.,0.))}else{(p(0.,0.),p(10.,0.))};
            let path=walk(&world,&probe,a,b,CornerStyle::Diagonal45,false).expect("local obstacle contours should connect");
            assert_eq!(path[0],a);assert_eq!(*path.last().unwrap(),b);assert!(world.path_clear(&path,&probe));assert!(path_turns_ok((0,0),&path,(0,0),CornerStyle::Diagonal45));
        }
    }
}