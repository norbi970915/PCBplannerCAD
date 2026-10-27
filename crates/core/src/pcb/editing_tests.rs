use crate::{Command, Session};
use crate::document::{Document, IdItem};
use crate::item::Item;
use crate::layer::{TOP_COPPER, BOTTOM_COPPER};
use crate::units::{mm, Point};
use super::connectivity::fixtures::{doc, track, via, fp, zone};
use super::router::{RouteRequest, RouteMode, CornerStyle};
fn content(d:&Document)->String { crate::io::document_to_string(d).unwrap() }

#[test]
fn locked_copper_rejects_transform_delete_and_cleanup_until_explicit_unlock() {
    let mut copper=track((0.,0.),(5.,0.),0.2,TOP_COPPER,"A");
    if let Item::Track(t)=&mut copper { t.locked=true; }
    let mut locked_via=via(20.,0.,"B");if let Item::Via(v)=&mut locked_via {v.locked=true;}
    let mut session=Session::with_document(doc(vec![copper,locked_via]));
    let id=session.document().items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
    let before=content(session.document());
    for command in [Command::Delete{ids:vec![id]},Command::Move{ids:vec![id],dx:mm(1.),dy:0},Command::Route{delete:vec![id],add:vec![],replace:vec![]}] {
        assert!(session.execute(command).is_err());assert_eq!(content(session.document()),before);
    }
    let opts=super::cleanup::CleanupOptions{zero_length:true,duplicates:true,dangling:true,unconnected_vias:true,merge_collinear:true};
    assert!(super::cleanup::cleanup(session.document(),&opts).delete.is_empty());
    let mut unlocked=session.document().get(id).unwrap().clone();if let Item::Track(t)=&mut unlocked {t.locked=false;}
    session.execute(Command::Replace{id,item:unlocked}).unwrap();
    session.execute(Command::Move{ids:vec![id],dx:mm(1.),dy:0}).unwrap();
    session.undo();session.undo();assert_eq!(content(session.document()),before);
}

#[test]
fn automatic_teardrops_follow_routes_and_undo_as_one_transaction() {
    let mut session=Session::with_document(doc(vec![fp("J1","L/TH",0.,0.,&[("1","A")]),via(10.,0.,"A")]));
    let (bid,mut board)=session.document().items().find(|(_,i)|matches!(i,Item::Board(_))).map(|(id,i)|(id,i.clone())).unwrap();
    let Item::Board(settings)=&mut board else{panic!()};
    settings.auto_teardrops=Some(super::teardrop::TeardropOptions{vias:true,pth_pads:true,smd_pads:true,length_ratio:0.5,width_ratio:1.,max_length:mm(1.),max_width:mm(2.),curved:true,max_track_ratio:0.9});
    session.execute(Command::Replace{id:bid,item:board}).unwrap();
    let before=content(session.document());
    session.execute(Command::Route{delete:vec![],replace:vec![],add:vec![track((0.,0.),(10.,0.),0.2,TOP_COPPER,"A")]}).unwrap();
    assert_eq!(super::teardrop::teardrops(session.document()).len(),2);
    let routed=content(session.document());
    session.undo();assert_eq!(content(session.document()),before);
    session.redo();assert_eq!(content(session.document()),routed);
    let track_id=session.document().items().find(|(_,i)|matches!(i,Item::Track(_))).unwrap().0;
    session.execute(Command::Delete{ids:vec![track_id]}).unwrap();
    assert!(super::teardrop::teardrops(session.document()).is_empty());
    session.undo();assert_eq!(content(session.document()),routed);
    // Unrelated metadata leaves existing teardrop IDs intact.
    let ids=super::teardrop::teardrops(session.document());
    let board=session.document().get(bid).unwrap().clone();session.execute(Command::Replace{id:bid,item:board}).unwrap();
    assert_eq!(ids,super::teardrop::teardrops(session.document()));
}

#[test]
fn pour_property_changes_and_overlapping_refills_share_undo() {
    let mut session=Session::with_document(doc(vec![zone(0.,0.,10.,10.,TOP_COPPER,"A"),zone(5.,0.,15.,10.,TOP_COPPER,"B")]));
    let before=content(session.document());
    let replacements=session.document().items().filter_map(|(id,i)|if let Item::Zone(z)=i{let mut z=z.clone();z.priority=if z.net=="B"{20}else{10};z.clearance=mm(0.4);z.remove_islands=false;Some(IdItem{id,item:Item::Zone(z)})}else{None}).collect();
    session.execute(Command::EditPours{items:replacements,refill:Some(vec![])}).unwrap();
    assert!(session.document().items().any(|(_,i)|matches!(i,Item::Zone(z)if !z.fill.is_empty())));
    let after=content(session.document());session.undo();assert_eq!(before,content(session.document()));session.redo();assert_eq!(after,content(session.document()));
}

#[test]
fn via_shove_moves_attached_tracks_across_its_span_and_keeps_fixed_ends() {
    let d=doc(vec![via(5.,0.,"B"),track((5.,0.),(5.,5.),0.2,TOP_COPPER,"B"),track((5.,0.),(5.,-5.),0.2,BOTTOM_COPPER,"B")]);
    let request=RouteRequest{net:"A".into(),layer:TOP_COPPER,start:Point::new(0,0),end:Point::new(mm(10.),0),width:mm(0.25),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
    let result=super::router::route(&d,&request);assert!(result.reached,"{}",result.message);
    let shifted=result.moved.iter().find_map(|r|if let Item::Via(v)=&r.item{Some(v.pos)}else{None}).expect("via was shoved");
    assert_ne!(shifted,Point::new(mm(5.),0));
    for r in &result.moved {if let Item::Track(t)=&r.item{assert!(t.a==shifted||t.b==shifted);assert!(t.a==Point::new(mm(5.),mm(5.))||t.b==Point::new(mm(5.),mm(5.))||t.a==Point::new(mm(5.),mm(-5.))||t.b==Point::new(mm(5.),mm(-5.)));}}
    assert_eq!(result.moved.iter().filter(|r|matches!(r.item,Item::Track(_))).count(),2);
    let mut session=Session::with_document(d);
    let delete=result.moved.iter().map(|r|r.id).collect();
    let add=result.moved.into_iter().map(|r|r.item).chain(result.tracks.into_iter().map(Item::Track)).collect();
    session.execute(Command::Route{delete,add,replace:vec![]}).unwrap();
    assert!(super::drc::run(session.document()).violations.iter().all(|v|v.severity!=super::drc::Severity::Error));
}

#[test]
fn selected_pour_refill_preserves_unrelated_fill() {
    let mut a=zone(0.,0.,5.,5.,TOP_COPPER,"A");if let Item::Zone(z)=&mut a{z.remove_islands=false;}
    let mut b=zone(100.,100.,105.,105.,TOP_COPPER,"B");if let Item::Zone(z)=&mut b{z.remove_islands=false;}
    let mut session=Session::with_document(doc(vec![a,b]));
    let zones:Vec<_>=session.document().items().filter(|(_,i)|matches!(i,Item::Zone(_))).map(|(id,_)|id).collect();
    let untouched=session.document().get(zones[1]).unwrap().clone();
    session.execute(Command::EditPours{items:vec![],refill:Some(vec![zones[0]])}).unwrap();
    assert!(matches!(session.document().get(zones[0]),Some(Item::Zone(z))if !z.fill.is_empty()));
    assert_eq!(session.document().get(zones[1]),Some(&untouched));
}
