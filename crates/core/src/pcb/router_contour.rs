//! Local connected-path deformation along convex clearance boundaries.
use crate::Point;
use super::geom::V;
use crate::pcb::elastic::{simple_path,simplify};

fn cross(a:V,b:V)->f64{a.x*b.y-a.y*b.x}
fn orientation(hull:&[Point])->f64{
 let origin=V::of(hull[0]);hull.iter().zip(hull.iter().cycle().skip(1)).take(hull.len()).map(|(a,b)|cross(V::of(*a)-origin,V::of(*b)-origin)).sum::<f64>().signum()
}
fn inside(point:V,hull:&[Point],sign:f64)->bool{
 hull.iter().zip(hull.iter().cycle().skip(1)).take(hull.len()).all(|(a,b)|cross(V::of(*b)-V::of(*a),point-V::of(*a))*sign>0.)
}
// Clip to the convex interior without stepping along the route or a grid.
fn clip(a:V,b:V,hull:&[Point],sign:f64)->Option<(f64,f64)>{
 let(mut lo,mut hi)=(0f64,1f64);
 for(p,q)in hull.iter().zip(hull.iter().cycle().skip(1)).take(hull.len()){
  let edge=V::of(*q)-V::of(*p);if edge.len()<1.{continue;}
  let offset=cross(edge,a-V::of(*p))*sign;let change=cross(edge,b-a)*sign;
  if change==0.{if offset<0.{return None;}}else if change>0.{lo=lo.max(-offset/change);}else{hi=hi.min(-offset/change);}
  if lo>=hi{return None;}
 }
 let middle=a+(b-a)*((lo+hi)/2.);inside(middle,hull,sign).then_some((lo,hi))
}
fn boundary_position(point:V,hull:&[Point],lengths:&[f64])->f64{
 let(mut best,mut position)=(f64::INFINITY,0.);
 for i in 0..hull.len(){let a=V::of(hull[i]);let edge=V::of(hull[(i+1)%hull.len()])-a;let len=edge.len();if len<1.{continue;}let t=((point-a).dot(edge)/(len*len)).clamp(0.,1.);let gap=(point-(a+edge*t)).len();if gap<best{best=gap;position=lengths[i]+t*len;}}
 position
}
fn splice(path:&[Point],hull:&[Point],clockwise:bool)->Option<Vec<Point>>{
 if hull.len()<3||path.len()<2{return None;}let sign=orientation(hull);if sign==0.{return None;}
 let mut first=None;let mut last=None;
 for(i,w)in path.windows(2).enumerate(){if let Some((lo,hi))=clip(V::of(w[0]),V::of(w[1]),hull,sign){first.get_or_insert((i,lo));last=Some((i,hi));}}
 let Some((entry,lo))=first else{return Some(path.to_vec());};let(exit,hi)=last?;
 if inside(V::of(path[0]),hull,sign)||inside(V::of(*path.last()?),hull,sign){return None;}
 let a=V::of(path[entry])+(V::of(path[entry+1])-V::of(path[entry]))*lo;
 let b=V::of(path[exit])+(V::of(path[exit+1])-V::of(path[exit]))*hi;
 let mut lengths=vec![0.];for i in 0..hull.len(){lengths.push(lengths[i]+(V::of(hull[(i+1)%hull.len()])-V::of(hull[i])).len());}let perimeter=*lengths.last()?;if perimeter<1.{return None;}
 let start=boundary_position(a,hull,&lengths);let end=boundary_position(b,hull,&lengths);
 let forward=|from:f64,to:f64|if clockwise{(from-to).rem_euclid(perimeter)}else{(to-from).rem_euclid(perimeter)};
 let span=forward(start,end);let mut vertices:Vec<_>=hull.iter().enumerate().map(|(i,p)|(forward(start,lengths[i]),*p)).filter(|(d,_)|*d>1.&&*d<span-1.).collect();vertices.sort_by(|a,b|a.0.total_cmp(&b.0));
 let mut result=path[..=entry].to_vec();result.push(a.round());result.extend(vertices.into_iter().map(|(_,p)|p));result.push(b.round());result.extend_from_slice(&path[exit+1..]);Some(simplify(&result))
}
/// Keep legal prefixes and tails, replacing only portions inside a push contour.
pub(super) fn walk(path:&[Point],hulls:&[Vec<Point>],clockwise:bool,reverse:bool)->Option<Vec<Point>>{
 if hulls.len()>128{return None;}let mut current=path.to_vec();
 for _ in 0..3{
  if !super::engine::consume_work(current.len().saturating_sub(1)){return None;}
  let mut changed=false;
  for ordinal in 0..hulls.len(){
   let i=if reverse{hulls.len()-1-ordinal}else{ordinal};let next=splice(&current,&hulls[i],clockwise)?;
   changed|=next!=current;current=next;if current.len()>256{return None;}
  }
  if !changed{break;}
 }
 (current!=path&&simple_path(&current)).then_some(current)
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::{Document,DocKind,Item,LayerId,units::mm,pcb::Track};
 use super::super::world::{World,Probe,Hit};
 fn p(x:f64,y:f64)->Point{Point::new(mm(x),mm(y))}
 fn fixture(tracks:&[(Point,Point)])->Document{Document::from_item_list(DocKind::Pcb,std::iter::once(Item::Board(Default::default())).chain(tracks.iter().map(|(a,b)|Item::Track(Track{a:*a,b:*b,width:mm(0.25),net:"B".into(),layer:LayerId(0),locked:true}))))}
 #[test]
 fn local_contours_keep_unchanged_corners_and_endpoints_in_both_path_directions(){
  let doc=fixture(&[(p(4.,2.),p(6.,2.))]);let mut world=World::build(&doc,LayerId(0),&[]);let net=world.net_id("A");let excluded=[net];let probe=Probe{hw:mm(0.25)as f64/2.,net,excl:&excluded};let hulls:Vec<_>=(0..world.obstacles.len()).filter_map(|i|world.clearance_hull(Hit::Ob(i as u32),&probe)).collect();
  for reverse in [false,true]{let mut path=vec![p(0.,0.),p(4.,0.),p(5.,1.),p(5.,5.),p(10.,5.)];if reverse{path.reverse();}
   for clockwise in [false,true]{let changed=walk(&path,&hulls,clockwise,false).expect("local bend contour");assert_eq!(&changed[..2],&path[..2]);assert_eq!(&changed[changed.len()-2..],&path[path.len()-2..]);assert!(world.path_clear(&changed,&probe));assert!(simple_path(&changed));}
  }
 }
 #[test]
 fn overlapping_clearance_contours_preserve_fixed_anchors(){
  let doc=fixture(&[(p(4.,-0.5),p(4.,0.5)),(p(4.5,-0.8),p(4.5,0.8))]);let mut world=World::build(&doc,LayerId(0),&[]);let net=world.net_id("A");let excluded=[net];let probe=Probe{hw:mm(0.25)as f64/2.,net,excl:&excluded};let hulls:Vec<_>=(0..world.obstacles.len()).filter_map(|i|world.clearance_hull(Hit::Ob(i as u32),&probe)).collect();
  for reverse in [false,true]{for clockwise in [false,true]{let path=vec![p(0.,0.),p(10.,0.)];let changed=walk(&path,&hulls,clockwise,reverse).expect("overlapping contour walk");assert_eq!(changed[0],path[0]);assert_eq!(*changed.last().unwrap(),path[1]);assert!(world.path_clear(&changed,&probe));}}
 }
 #[test]
 fn a_physically_clear_fixed_endpoint_is_not_rejected_by_extra_contour_padding(){
  let doc=fixture(&[(p(0.,0.),p(4.,0.))]);let mut world=World::build(&doc,LayerId(0),&[]);let net=world.net_id("A");let excluded=[net];let probe=Probe{hw:mm(0.25)as f64/2.,net,excl:&excluded};
  let path=vec![p(-2.,0.),p(-0.5,0.),p(-0.5,0.186395),p(4.45,0.186395)];assert!(world.point_slack(V::of(*path.last().unwrap()),&probe,1.)>0.);
  let padded=world.clearance_hull(Hit::Ob(0),&probe).unwrap();let exact=world.clearance_hull_with_margin(Hit::Ob(0),&probe,0.).unwrap();assert!(walk(&path,&[padded],false,false).is_none());
  let candidates:Vec<_>=[false,true].into_iter().filter_map(|side|walk(&path,&[exact.clone()],side,false)).filter(|p|world.path_clear(p,&probe)).collect();assert!(!candidates.is_empty());for changed in candidates{assert_eq!(changed[0],path[0]);assert_eq!(changed.last(),path.last());}
 }
 #[test]
 fn a_contour_never_relocates_a_buried_endpoint(){
  let hull=vec![p(3.,-1.),p(5.,-1.),p(5.,1.),p(3.,1.)];for clockwise in [false,true]{for path in [vec![p(4.,0.),p(8.,0.)],vec![p(0.,0.),p(4.,0.)]]{assert!(walk(&path,&[hull.clone()],clockwise,false).is_none());}}
 }
}
