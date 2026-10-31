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


// A rejected edit must preserve pending Redo as well as saved geometry and IDs.
fn assert_rejected_without_history_change(document: Document, command: Command) {
    let mut session=Session::with_document(document);
    session.execute(Command::Add{items:vec![track((80.,80.),(81.,80.),0.2,TOP_COPPER,"TEST")]}).unwrap();
    let redo_target=session.document().clone();
    session.undo().unwrap();session.mark_saved();
    let before=session.document().clone();
    assert!(session.execute_with_via_checks(command,true).is_err());
    assert_eq!(session.document(),&before);
    assert!(!session.is_dirty());assert!(!session.can_undo());assert!(session.can_redo());
    session.redo().unwrap();assert_eq!(session.document(),&redo_target);
}

#[test]
fn unlocking_cannot_sneak_geometry_or_net_changes_into_locked_items() {
    let mut items=vec![track((0.,0.),(5.,0.),0.2,TOP_COPPER,"A"),via(10.,0.,"A"),fp("J1","L/F",20.,0.,&[("1","A")]),zone(30.,0.,40.,10.,TOP_COPPER,"A")];
    items.push(Item::ArcTrack(super::ArcTrack{locked:false,center:Point::new(mm(50.),0),radius:mm(5.),start_angle:0.,sweep:90.,width:mm(0.2),layer:TOP_COPPER,net:"A".into()}));
    for mut item in items {
        match &mut item {Item::Track(v)=>v.locked=true,Item::Via(v)=>v.locked=true,Item::Footprint(v)=>v.locked=true,Item::Zone(v)=>v.locked=true,Item::ArcTrack(v)=>v.locked=true,_=>unreachable!()}
        let document=doc(vec![item.clone()]);
        let id=document.items().find(|(_,i)|*i==&item).unwrap().0;
        let mut changed=item.clone();
        match &mut changed {Item::Track(v)=>{v.locked=false;v.net="B".into();},Item::Via(v)=>{v.locked=false;v.pos.x+=mm(1.);},Item::Footprint(v)=>{v.locked=false;v.pos.x+=mm(1.);},Item::Zone(v)=>{v.locked=false;v.priority+=1;},Item::ArcTrack(v)=>{v.locked=false;v.width+=mm(0.1);},_=>unreachable!()}
        for command in [Command::Replace{id,item:changed.clone()},Command::ReplaceMany{items:vec![IdItem{id,item:changed.clone()}]},Command::Route{delete:vec![],add:vec![],replace:vec![IdItem{id,item:changed.clone()}]}] {
            assert_rejected_without_history_change(document.clone(),command);
        }
        let mut unlocked=item.clone();
        match &mut unlocked {Item::Track(v)=>v.locked=false,Item::Via(v)=>v.locked=false,Item::Footprint(v)=>v.locked=false,Item::Zone(v)=>v.locked=false,Item::ArcTrack(v)=>v.locked=false,_=>unreachable!()}
        let mut session=Session::with_document(document);
        session.execute_with_via_checks(Command::Replace{id,item:unlocked},true).unwrap();
        session.undo().unwrap();assert_eq!(session.document().get(id),Some(&item));
    }
}

#[test]
fn mixed_bulk_edit_with_locked_change_rejects_every_replacement() {
    let mut locked=track((0.,0.),(5.,0.),0.2,TOP_COPPER,"A");if let Item::Track(v)=&mut locked{v.locked=true;}
    let document=doc(vec![track((0.,10.),(5.,10.),0.2,TOP_COPPER,"A"),locked]);
    let items=document.items().filter_map(|(id,i)|if let Item::Track(t)=i{let mut t=t.clone();t.locked=false;t.width=mm(0.5);Some(IdItem{id,item:Item::Track(t)})}else{None}).collect();
    assert_rejected_without_history_change(document,Command::ReplaceMany{items});
}

#[test]
fn repeated_targets_and_delete_replace_conflicts_are_rejected_before_editing() {
    let item=track((0.,0.),(5.,0.),0.2,TOP_COPPER,"A");let document=doc(vec![item.clone()]);
    let id=document.items().find(|(_,i)|*i==&item).unwrap().0;
    let replacement=IdItem{id,item:item.translated(0,mm(1.))};
    for command in [Command::Delete{ids:vec![id,id]},Command::Move{ids:vec![id,id],dx:mm(1.),dy:0},Command::ReplaceMany{items:vec![replacement.clone(),replacement.clone()]},Command::Change{delete:vec![id,id],add:vec![item.clone()]},Command::Route{delete:vec![id],replace:vec![replacement],add:vec![item]}] {
        assert_rejected_without_history_change(document.clone(),command);
    }
}

#[test]
fn nonfinite_rotation_is_rejected_without_collapsing_geometry() {
    let item=track((10.,10.),(15.,10.),0.2,TOP_COPPER,"A");let document=doc(vec![item.clone()]);
    let id=document.items().find(|(_,i)|*i==&item).unwrap().0;
    for angle in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY] {
        assert_rejected_without_history_change(document.clone(),Command::Rotate{ids:vec![id],center:Point::default(),angle});
    }
}


#[test]
fn rotation_that_rounds_a_tiny_segment_to_zero_length_is_atomic() {
    let item=Item::Track(super::Track{locked:false,a:Point::new(1,0),b:Point::new(2,0),width:mm(0.2),layer:TOP_COPPER,net:"A".into()});
    let document=doc(vec![item.clone()]);let id=document.items().find(|(_,i)|*i==&item).unwrap().0;
    assert_rejected_without_history_change(document,Command::Rotate{ids:vec![id],center:Point::default(),angle:45.});
}

#[test]
fn routed_surface_copper_refills_only_its_layer_with_full_priority_dependencies(){
    let mut session=Session::with_document(doc(vec![zone(0.,0.,10.,10.,TOP_COPPER,"A"),zone(5.,0.,15.,10.,TOP_COPPER,"B"),zone(0.,0.,15.,10.,BOTTOM_COPPER,"C")]));
    session.execute(Command::EditPours{items:vec![],refill:None}).unwrap();let before=content(session.document());
    let bottom=session.document().items().find(|(_,i)|matches!(i,Item::Zone(z)if z.layer==BOTTOM_COPPER)).map(|(id,i)|(id,i.clone())).unwrap();
    let delta=session.execute(Command::Route{delete:vec![],replace:vec![],add:vec![track((2.,2.),(8.,8.),0.25,TOP_COPPER,"OTHER")]}).unwrap();
    assert!(!delta.upserted.iter().any(|i|i.id==bottom.0));assert_eq!(session.document().get(bottom.0),Some(&bottom.1));
    for (id,fill) in super::fill::fill_all(session.document()) {let Some(Item::Zone(zone))=session.document().get(id)else{panic!()};assert_eq!(zone.fill,fill,"priority-dependent fill changed");}
    let after=content(session.document());session.undo();assert_eq!(content(session.document()),before);session.redo();assert_eq!(content(session.document()),after);
}