use super::*;
use crate::{Command,DocKind,Session,units::mm};
use crate::pcb::{board::DiffPair,connectivity,drc,items::Via};
fn p(x:f64,y:f64)->Point{Point::new(mm(x),mm(y))}
fn track(net:&str,a:Point,b:Point)->Item{Item::Track(Track{a,b,width:mm(0.25),layer:LayerId(0),net:net.into(),locked:false})}
fn request(ids:Vec<ItemId>,point:Point,delta:Point)->TrackRequest{TrackRequest{ids,point,delta,mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:1000}}
fn fixture(items:Vec<Item>)->(Session,Vec<ItemId>){
    let mut s=Session::new(DocKind::Pcb);let mut all=vec![Item::Board(Default::default()),Item::Rect(crate::item::Rect{a:p(-20.,-20.),b:p(30.,30.),width:mm(0.1),layer:crate::layer::BOARD_OUTLINE,fill:crate::item::FillStyle::None})];all.extend(items);let delta=s.execute(Command::Add{items:all}).unwrap();let ids=delta.upserted.iter().skip(2).map(|r|r.id).collect();(s,ids)
}
fn apply(s:&mut Session,plan:DragResult){s.execute(Command::Route{replace:plan.replace,delete:plan.delete,add:plan.add}).unwrap();}
fn undo_redo(s:&mut Session,before:&str){let after=crate::io::document_to_string(s.document()).unwrap();s.undo().unwrap();assert_eq!(crate::io::document_to_string(s.document()).unwrap(),before);s.redo().unwrap();assert_eq!(crate::io::document_to_string(s.document()).unwrap(),after);}
fn assert_clear(s:&Session){let errors:Vec<_>=drc::run(s.document()).violations.into_iter().filter(|v|v.severity==drc::Severity::Error).collect();assert!(errors.is_empty(),"{errors:?}");}
fn via(net:&str,at:Point)->Item{Item::Via(Via{pos:at,diameter:mm(0.6),drill:mm(0.3),from_layer:LayerId(0),to_layer:LayerId(1),net:net.into(),locked:false})}
#[test]
fn grouped_chain_keeps_shape_fixed_vias_connectivity_and_exact_history(){
    let points=[p(0.,0.),p(2.,2.),p(3.,2.),p(4.,3.),p(6.,3.),p(9.,0.)];
    let mut items=vec![via("N",points[0]),via("N",points[5])];items.extend(points.windows(2).map(|w|track("N",w[0],w[1])));let (mut s,ids)=fixture(items);
    let before=crate::io::document_to_string(s.document()).unwrap();let req=request(ids[3..6].to_vec(),p(4.,3.),p(0.,0.4));let plan=drag_tracks(s.document(),&req).unwrap();assert_eq!(plan.replace.len(),3);apply(&mut s,plan);
    let c=connectivity::analyze(s.document());assert!(c.connected((ids[0],None),(ids[1],None)));for id in &ids[..2]{assert!(matches!(s.document().get(*id),Some(Item::Via(_))));}assert_clear(&s);undo_redo(&mut s,&before);
}
#[test]
fn coupled_drag_rebuilds_offset_bends_in_both_directions(){
    for shift in [-0.8,-0.3,0.3,0.8]{
        let mut items=Vec::new();for (net,y) in [("P",2.),("N",2.45)]{for (a,b) in [(0.,2.),(2.,6.),(6.,8.)]{items.push(track(net,p(a,y),p(b,y)));}}
        let (mut s,ids)=fixture(items);let (board_id,board)=board_settings(s.document()).unwrap();let mut board=board.clone();board.diff_pairs.push(DiffPair{name:"D".into(),positive:"P".into(),negative:"N".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:board_id,item:Item::Board(board)}]}).unwrap();
        let before=crate::io::document_to_string(s.document()).unwrap();let req=request(vec![ids[1]],p(4.,2.),p(0.,shift));let plan=drag_tracks(s.document(),&req).unwrap_or_else(|e|panic!("shift {shift}: {e}"));assert!(plan.replace.iter().any(|r|r.id==ids[4]),"partner must be part of the same edit");apply(&mut s,plan);assert_clear(&s);
        let c=connectivity::analyze(s.document());assert_eq!(c.clusters().len(),2);let pts:Vec<_>=s.document().items().filter_map(|(_,i)|if let Item::Track(t)=i{Some(t.clone())}else{None}).collect();
        for t in pts.iter().filter(|t|t.net=="P"&&t.a.y==t.b.y&&(t.a.x+t.b.x)/2==mm(4.)) {assert!(pts.iter().any(|u|u.net=="N"&&u.a.y==u.b.y&&(u.a.y-t.a.y-mm(0.45)).abs()<=2));}
        undo_redo(&mut s,&before);
    }
}
#[test]
fn locked_partner_rejects_the_whole_move_without_changes(){
    let (mut s,ids)=fixture(vec![track("P",p(0.,2.),p(8.,2.)),track("N",p(0.,2.45),p(8.,2.45))]);let (bid,board)=board_settings(s.document()).unwrap();let mut board=board.clone();board.diff_pairs.push(DiffPair{name:"D".into(),positive:"P".into(),negative:"N".into()});let mut locked=s.document().get(ids[1]).unwrap().clone();if let Item::Track(t)=&mut locked{t.locked=true;}s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(board)},IdItem{id:ids[1],item:locked}]}).unwrap();let before=s.document().clone();assert!(drag_tracks(s.document(),&request(vec![ids[0]],p(4.,2.),p(0.,1.))).unwrap_err().contains("both conductors"));assert_eq!(s.document(),&before);
}
fn arc()->Item{Item::ArcTrack(ArcTrack{center:p(4.,4.),radius:mm(2.),start_angle:180.,sweep:-180.,width:mm(0.25),layer:LayerId(0),net:"N".into(),locked:false})}
#[test]
fn arc_body_reshape_keeps_endpoints_circular_copper_and_history(){
    let (mut s,ids)=fixture(vec![via("N",p(2.,4.)),via("N",p(6.,4.)),arc()]);let before=crate::io::document_to_string(s.document()).unwrap();let req=request(vec![ids[2]],p(4.,6.),p(0.,0.5));let plan=drag_tracks(s.document(),&req).unwrap();assert!(matches!(plan.replace[0].item,Item::ArcTrack(_)));apply(&mut s,plan);assert!(connectivity::analyze(s.document()).connected((ids[0],None),(ids[1],None)));assert_clear(&s);undo_redo(&mut s,&before);
}
#[test]
fn arc_can_flatten_and_grouped_curves_keep_their_exact_shape(){
    let (mut s,ids)=fixture(vec![arc()]);let plan=drag_tracks(s.document(),&request(vec![ids[0]],p(4.,6.),p(0.,-2.))).unwrap();assert!(matches!(plan.replace[0].item,Item::Track(_)));apply(&mut s,plan);assert_clear(&s);
    let (s,ids)=fixture(vec![arc(),track("N",p(6.,4.),p(8.,4.))]);let req=request(ids.clone(),p(4.,6.),p(1.,0.));let plan=drag_tracks(s.document(),&req).unwrap();assert_eq!(plan.replace.len(),2);assert!(plan.replace.iter().any(|r|matches!(&r.item,Item::ArcTrack(a)if a.center==p(5.,4.)&&a.radius==mm(2.)&&a.sweep == -180.)));
}
#[test]
fn curve_collisions_and_invalid_groups_never_modify_the_source(){
    let mut obstacle=track("OTHER",p(0.,6.8),p(8.,6.8));if let Item::Track(t)=&mut obstacle{t.locked=true;}let (s,ids)=fixture(vec![arc(),obstacle]);let before=s.document().clone();assert!(drag_tracks(s.document(),&request(vec![ids[0]],p(4.,6.),p(0.,0.8))).is_err());assert_eq!(s.document(),&before);
    assert!(drag_tracks(s.document(),&request(vec![ids[0],ids[0]],p(4.,6.),p(0.,0.1))).is_err());
}

#[test]
fn curve_body_preserves_one_or_two_straight_tangents_and_fixed_anchors(){
 for both in [false,true]{
  let a=Item::ArcTrack(ArcTrack{center:p(4.,4.),radius:mm(2.),start_angle:-90.,sweep:90.,width:mm(0.25),layer:LayerId(0),net:"N".into(),locked:false});
  let mut items=vec![via("N",p(0.,2.)),via("N",if both{p(6.,8.)}else{p(6.,4.)}),track("N",p(0.,2.),p(4.,2.)),a];
  if both{items.push(track("N",p(6.,4.),p(6.,8.)));}
  let (mut s,ids)=fixture(items);let before=crate::io::document_to_string(s.document()).unwrap();
  let plan=drag_tracks(s.document(),&request(vec![ids[3]],p(5.414214,2.585786),p(0.15,-0.15))).unwrap();assert_eq!(plan.replace.len(),if both{3}else{2});apply(&mut s,plan);assert_clear(&s);
  let Some(Item::ArcTrack(a))=s.document().get(ids[3])else{panic!()};let (_,endpoints)=ends(&Item::ArcTrack(a.clone())).unwrap();
  let Some(Item::Track(t))=s.document().get(ids[2])else{panic!()};assert_eq!(t.a,p(0.,2.));assert_eq!(t.b,endpoints[0]);assert!((endpoints[0].y-a.center.y+a.radius).abs()<=2);assert_eq!(endpoints[0].y,mm(2.));
  if both{let Some(Item::Track(t))=s.document().get(ids[4])else{panic!()};assert_eq!(t.a,endpoints[1]);assert_eq!(t.b,p(6.,8.));assert_eq!(endpoints[1].x,mm(6.));}else{assert!(distance(endpoints[1],p(6.,4.))<=2.);}
  assert!(connectivity::analyze(s.document()).connected((ids[0],None),(ids[1],None)));undo_redo(&mut s,&before);
 }
}

#[test]
fn grouped_copper_refuses_a_locked_obstacle_and_retains_unchanged_ids(){
 let mut obstacle=track("OTHER",p(-2.,3.),p(12.,3.));if let Item::Track(t)=&mut obstacle{t.locked=true;}
 let (s,ids)=fixture(vec![track("N",p(0.,0.),p(2.,2.)),track("N",p(2.,2.),p(6.,2.)),track("N",p(6.,2.),p(8.,0.)),obstacle]);let before=s.document().clone();
 assert!(drag_tracks(s.document(),&request(ids[..3].to_vec(),p(4.,2.),p(0.,1.))).is_err());assert_eq!(s.document(),&before);
 let plan=drag_tracks(s.document(),&request(ids[..3].to_vec(),p(4.,2.),p(0.,-1.))).unwrap();assert_eq!(plan.replace.iter().map(|r|r.id).collect::<HashSet<_>>(),ids[..3].iter().copied().collect());
}
#[test]
fn routing_uses_even_odd_material_for_disjoint_outlines_and_nested_cutouts(){
 use crate::item::{Rect,FillStyle};
 let outline=|a,b|Item::Rect(Rect{a,b,width:mm(0.1),layer:crate::layer::BOARD_OUTLINE,fill:FillStyle::None});
 let (s,_)=fixture(vec![outline(p(40.,0.),p(60.,20.)),outline(p(44.,4.),p(56.,16.)),outline(p(48.,8.),p(52.,12.))]);
 for (a,b,expected) in [(p(41.,2.),p(59.,2.),true),(p(49.,9.),p(51.,11.),true),(p(45.,5.),p(46.,6.),false),(p(32.,0.),p(35.,0.),false)]{
  let result=router::route(s.document(),&router::RouteRequest{net:"A".into(),layer:LayerId(0),start:a,end:b,width:mm(0.25),mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false});assert_eq!(result.reached,expected,"{a:?} -> {b:?}: {}",result.message);
 }
}
#[test]
fn explicit_coupled_corner_groups_and_concentric_curves_move_as_one_edit(){
 for curves in [false,true]{
  let items=if curves{vec![arc(),Item::ArcTrack(ArcTrack{radius:mm(2.45),net:"M".into(),..if let Item::ArcTrack(a)=arc(){a}else{unreachable!()}})]}else{vec![track("N",p(0.,0.),p(4.,0.)),track("N",p(4.,0.),p(4.,4.)),track("M",p(0.,0.45),p(3.55,0.45)),track("M",p(3.55,0.45),p(3.55,4.))]};
  let (mut s,ids)=fixture(items);let (bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();
  let before=crate::io::document_to_string(s.document()).unwrap();let selected=if curves{vec![ids[0]]}else{ids[..2].to_vec()};let plan=drag_tracks(s.document(),&request(selected,p(4.,2.),p(0.5,0.5))).unwrap();assert_eq!(plan.replace.len(),ids.len());for r in &plan.replace{assert_eq!(r.item,translated(s.document().get(r.id).unwrap(),p(0.5,0.5)));}apply(&mut s,plan);assert_clear(&s);undo_redo(&mut s,&before);
 }
}
#[test]
fn a_locked_tangent_cannot_silently_become_a_kink_during_curve_drag(){
 let mut fixed=track("N",p(0.,2.),p(4.,2.));if let Item::Track(t)=&mut fixed{t.locked=true;}
 let (s,ids)=fixture(vec![fixed,Item::ArcTrack(ArcTrack{center:p(4.,4.),radius:mm(2.),start_angle:-90.,sweep:90.,width:mm(0.25),layer:LayerId(0),net:"N".into(),locked:false}),track("N",p(6.,4.),p(6.,8.))]);let before=s.document().clone();let result=drag_tracks(s.document(),&request(vec![ids[1]],p(5.414214,2.585786),p(0.15,-0.15)));assert!(result.unwrap_err().contains("locked straight tangent"));assert_eq!(s.document(),&before);
}
#[test]
fn short_curve_caps_still_have_a_stable_draggable_body(){
 let item=Item::ArcTrack(ArcTrack{center:p(4.,4.),radius:mm(0.3),start_angle:0.,sweep:45.,width:mm(0.15),layer:LayerId(0),net:"N".into(),locked:false});let (mut s,ids)=fixture(vec![item]);let before=crate::io::document_to_string(s.document()).unwrap();
 let plan=drag_tracks(s.document(),&request(vec![ids[0]],p(4.277164,4.114805),p(0.01,0.005))).unwrap();assert_eq!(plan.replace.len(),1);apply(&mut s,plan);assert_clear(&s);undo_redo(&mut s,&before);
}

#[test]
fn attached_coupled_curves_reshape_with_exact_gap_and_four_straight_tangents(){
 let mut items=Vec::new();for (net,radius) in [("N",2.),("M",2.45)]{items.push(track(net,p(0.,4.-radius),p(4.,4.-radius)));items.push(Item::ArcTrack(ArcTrack{center:p(4.,4.),radius:mm(radius),start_angle:-90.,sweep:90.,width:mm(0.25),layer:LayerId(0),net:net.into(),locked:false}));items.push(track(net,p(4.+radius,4.),p(4.+radius,8.)));}
 let (mut s,ids)=fixture(items);let (bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();
 let before=crate::io::document_to_string(s.document()).unwrap();let plan=drag_tracks(s.document(),&request(vec![ids[1]],p(5.414214,2.585786),p(0.15,-0.15))).unwrap();assert_eq!(plan.replace.len(),6);apply(&mut s,plan);assert_clear(&s);
 let Some(Item::ArcTrack(a))=s.document().get(ids[1])else{panic!()};let Some(Item::ArcTrack(b))=s.document().get(ids[4])else{panic!()};assert_eq!(a.center,b.center);assert_eq!(b.radius-a.radius,mm(0.45));assert_eq!(a.start_angle,b.start_angle);assert_eq!(a.sweep,b.sweep);
 for id in [ids[0],ids[2],ids[3],ids[5]]{let Some(Item::Track(t))=s.document().get(id)else{panic!()};assert!(t.a.x==t.b.x||t.a.y==t.b.y);}
 undo_redo(&mut s,&before);
}
#[test]
fn staggered_pair_leaders_can_use_aligned_local_gates_without_moving_the_far_tails(){
 let mut items=vec![track("N",p(2.,0.),p(2.,1.)),track("N",p(2.,1.),p(2.,7.)),track("N",p(2.,7.),p(2.,8.)),track("M",p(2.45,-2.),p(2.45,0.)),track("M",p(2.45,0.),p(2.45,8.)),track("M",p(2.45,8.),p(2.45,10.))];
 for index in [0,2,3,5]{if let Item::Track(t)=&mut items[index]{t.locked=true;}}
 let (mut s,ids)=fixture(items);let (bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=crate::io::document_to_string(s.document()).unwrap();let request=request(vec![ids[1]],p(2.,4.),p(0.1,0.));
 let plan=paired_local(s.document(),&request,&[ids[1],ids[4]]).expect("local gates exist").unwrap();let fixed:Vec<_>=[0,2,3,5].iter().map(|index|(ids[*index],s.document().get(ids[*index]).unwrap().clone())).collect();apply(&mut s,plan);assert_clear(&s);for(id,item)in fixed{assert_eq!(s.document().get(id),Some(&item));}undo_redo(&mut s,&before);
}
#[test]
fn tiny_pair_drags_keep_simple_bends_in_every_orientation(){
 for octant in 0..8{for shift in [-0.02,-0.005,0.005,0.02]{
  let orient=|mut point:Point|{if octant>=4{point.x=-point.x;}for _ in 0..octant%4{point=Point::new(-point.y,point.x);}point};
  let mut items=Vec::new();for(net,y)in[("N",2.),("M",2.45)]{for(a,b)in[(0.,2.),(2.,6.),(6.,8.)]{items.push(track(net,orient(p(a,y)),orient(p(b,y))));}}
  let (mut s,ids)=fixture(items);let(bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=crate::io::document_to_string(s.document()).unwrap();
  let plan=drag_tracks(s.document(),&request(vec![ids[1]],orient(p(4.,2.)),orient(p(0.,shift)))).unwrap_or_else(|e|panic!("orientation {octant}, shift {shift}: {e}"));assert!(plan.replace.iter().any(|r|r.id==ids[4]));apply(&mut s,plan);assert_clear(&s);undo_redo(&mut s,&before);
 }}
}

#[test]
fn blocked_conductor_plan_identifies_its_fixed_via_and_layer(){
 let(s,ids)=fixture(vec![track("N",p(0.,0.),p(2.,0.)),via("OTHER",p(1.,2.))]);let error=router::plan_track_edit(s.document(),vec![IdItem{id:ids[0],item:track("N",p(0.,2.),p(2.,2.))}],vec![],RouteMode::Shove).unwrap_err();assert!(error.contains(&format!("via #{}",ids[1].0)));assert!(error.contains("'OTHER'"));assert!(error.contains("layer 0"));
}
#[test]
fn reversed_diagonal_pair_rounding_retains_exact_fixed_endpoints(){
 for turn in 0..4{for shift in [-0.02,0.02]{
  let orient=|mut point:Point|{for _ in 0..turn{point=Point::new(-point.y,point.x);}point};
  let a=p(3.123457,2.345679);let b=p(4.123457,1.345679);let qa=p(4.892868,1.115091);let qb=p(2.892868,3.115091);
  let mut items=vec![track("N",orient(p(2.623457,2.845679)),orient(a)),track("N",orient(a),orient(b)),track("N",orient(b),orient(p(4.623457,0.845679))),track("M",orient(p(5.392868,0.615091)),orient(qa)),track("M",orient(qa),orient(qb)),track("M",orient(qb),orient(p(2.392868,3.615091)))];
  for(index,item)in items.iter_mut().enumerate(){if let Item::Track(t)=item{t.width=mm(0.254);t.locked=[0,2,3,5].contains(&index);}}
  let (mut s,ids)=fixture(items);let(bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});b.rules.insert(0,crate::pcb::board::Rule{name:"Pair spacing".into(),enabled:true,kind:crate::pcb::board::RuleKind::DiffPair{scope:"All".into(),width:mm(0.254),gap:127005}});b.rules.insert(0,crate::pcb::board::Rule{name:"Copper spacing".into(),enabled:true,kind:crate::pcb::board::RuleKind::Clearance{first:"All".into(),second:"All".into(),min:mm(0.127)}});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();
  let before=crate::io::document_to_string(s.document()).unwrap();let fixed:Vec<_>=[0,2,3,5].iter().map(|i|(ids[*i],s.document().get(ids[*i]).unwrap().clone())).collect();let amount=mm(shift)/2;let delta=orient(Point::new(amount,amount));let plan=drag_tracks(s.document(),&request(vec![ids[1]],orient(p(3.623457,1.845679)),delta)).unwrap_or_else(|e|panic!("turn {turn} shift {shift}: {e}"));apply(&mut s,plan);assert_clear(&s);for(id,item)in fixed{assert_eq!(s.document().get(id),Some(&item));}undo_redo(&mut s,&before);
 }}
}
#[test]
fn exact_fixed_tail_coverage_keeps_existing_findings_outside_a_local_drag(){
 let(mut s,ids)=fixture(vec![track("N",p(0.,0.),p(10.,0.)),via("OTHER",p(9.,0.55))]);
 let original=crate::io::document_to_string(s.document()).unwrap();let old_errors:Vec<_>=drc::run(s.document()).violations.into_iter().filter(|v|v.severity==drc::Severity::Error).collect();assert_eq!(old_errors.len(),1);assert_eq!(old_errors[0].code,"clearance");
 let extra=vec![track("N",p(0.,0.),p(2.,0.)),track("N",p(2.,0.),p(2.02,0.02)),track("N",p(2.02,0.02),p(3.,0.02)),track("N",p(7.,0.02),p(7.98,0.02)),track("N",p(7.98,0.02),p(8.,0.)),track("N",p(8.,0.),p(10.,0.))];
 let plan=router::plan_track_edit(s.document(),vec![IdItem{id:ids[0],item:track("N",p(3.,0.02),p(7.,0.02))}],extra,RouteMode::Shove).unwrap();apply(&mut s,plan);
 let errors:Vec<_>=drc::run(s.document()).violations.into_iter().filter(|v|v.severity==drc::Severity::Error).collect();assert_eq!(errors.len(),old_errors.len());assert_eq!(errors[0].code,old_errors[0].code);assert_eq!(errors[0].message.split(':').next(),old_errors[0].message.split(':').next());assert_eq!(errors[0].pos,old_errors[0].pos);undo_redo(&mut s,&original);
}

#[test]
fn source_coverage_never_waives_new_motion_or_crossings_of_fixed_tails(){
 let(s,ids)=fixture(vec![track("N",p(0.,0.),p(10.,0.)),via("OTHER",p(9.,0.55)),track("M",p(8.5,-2.),p(9.5,-2.))]);
 let source=s.document().clone();
 // Even a one-nanometre displacement is new copper and must satisfy clearance.
 assert!(router::plan_track_edit(s.document(),vec![IdItem{id:ids[0],item:track("N",Point::new(0,1),Point::new(mm(10.),1))}],vec![],RouteMode::Shove).is_err());
 // A retained source fragment remains an obstacle to another moving net.
 let result=router::plan_track_edit(s.document(),vec![IdItem{id:ids[0],item:track("N",p(3.,0.02),p(7.,0.02))},IdItem{id:ids[2],item:track("M",p(8.5,-0.5),p(8.5,0.5))}],vec![track("N",p(0.,0.),p(2.,0.)),track("N",p(2.,0.),p(2.02,0.02)),track("N",p(2.02,0.02),p(3.,0.02)),track("N",p(7.,0.02),p(7.98,0.02)),track("N",p(7.98,0.02),p(8.,0.)),track("N",p(8.,0.),p(10.,0.))],RouteMode::Shove);
 assert!(result.is_err());assert_eq!(s.document(),&source);
}
#[test]
fn a_smaller_coupled_window_avoids_a_fixed_via_at_its_far_end(){
 let(mut s,ids)=fixture(vec![track("N",p(2.,0.),p(2.,1.)),track("N",p(2.,1.),p(2.,7.)),track("N",p(2.,7.),p(2.,8.)),track("M",p(2.45,-2.),p(2.45,0.)),track("M",p(2.45,0.),p(2.45,8.)),track("M",p(2.45,8.),p(2.45,10.)),via("OTHER",p(1.35,4.7))]);
 let(bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=crate::io::document_to_string(s.document()).unwrap();
 let request=request(vec![ids[1]],p(2.,4.),p(-0.1,0.));assert_clear(&s);assert!(paired_local_window(s.document(),&request,&[ids[1],ids[4]],3.).unwrap().is_err());
 let plan=paired_local(s.document(),&request,&[ids[1],ids[4]]).unwrap().unwrap();apply(&mut s,plan);assert_clear(&s);undo_redo(&mut s,&before);
}


#[test]
fn short_paired_bodies_keep_their_partner_when_bends_are_consumed(){
 let(mut s,ids)=fixture(vec![track("N",p(2.,0.),p(2.,2.)),track("N",p(2.,2.),p(2.,2.1)),track("N",p(2.,2.1),p(2.,4.)),track("M",p(2.45,0.),p(2.45,2.)),track("M",p(2.45,2.),p(2.45,2.1)),track("M",p(2.45,2.1),p(2.45,4.))]);let(bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=crate::io::document_to_string(s.document()).unwrap();
 assert_eq!(coupled(s.document(),ids[1]).unwrap(),Some(ids[4]));let plan=drag_tracks(s.document(),&request(vec![ids[1]],p(2.,2.05),p(0.02,0.))).unwrap();assert!(plan.replace.iter().any(|r|matches!(&r.item,Item::Track(t)if t.net=="M")));apply(&mut s,plan);assert_clear(&s);undo_redo(&mut s,&before);
}
#[test]
fn repeated_coupled_drags_consume_old_bends_without_losing_coupling(){
 let(mut s,ids)=fixture(vec![track("N",p(2.,0.),p(2.,1.)),track("N",p(2.,1.),p(2.,7.)),track("N",p(2.,7.),p(2.,8.)),track("M",p(2.45,-2.),p(2.45,0.)),track("M",p(2.45,0.),p(2.45,8.)),track("M",p(2.45,8.),p(2.45,10.))]);let(bid,b)=board_settings(s.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"D".into(),positive:"N".into(),negative:"M".into()});s.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let before=crate::io::document_to_string(s.document()).unwrap();
 for step in 0..8{let Some(Item::Track(t))=s.document().get(ids[1])else{panic!("dragged body identity lost")};let point=Point::new((t.a.x+t.b.x)/2,(t.a.y+t.b.y)/2);assert!(coupled(s.document(),ids[1]).unwrap().is_some(),"coupling lost at step {step}");let plan=drag_tracks(s.document(),&request(vec![ids[1]],point,p(if step%2==0{0.02}else{-0.02},0.))).unwrap_or_else(|e|panic!("step {step}: {e}"));assert!(plan.replace.iter().any(|r|matches!(&r.item,Item::Track(t)if t.net=="M")));apply(&mut s,plan);assert_clear(&s);}
 let after=crate::io::document_to_string(s.document()).unwrap();for _ in 0..8{s.undo().unwrap();}assert_eq!(crate::io::document_to_string(s.document()).unwrap(),before);for _ in 0..8{s.redo().unwrap();}assert_eq!(crate::io::document_to_string(s.document()).unwrap(),after);
}
