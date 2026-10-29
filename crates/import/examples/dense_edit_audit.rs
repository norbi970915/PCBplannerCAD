//! Transactional pour and connected-placement probes on a disposable native board.
use std::{env,fs,time::Instant};
use pcbplannercad_core::{io,DocKind,Session,Command,Item,units::Point,pcb::{fill,drag::{drag_footprints,DragRequest,MoveMode},router::{RouteMode,CornerStyle}}};
use serde_json::json;
fn snap(d:&pcbplannercad_core::Document)->String{io::document_to_string(d).unwrap()}
fn main(){let a:Vec<_>=env::args().collect();let doc=io::document_from_str(DocKind::Pcb,&fs::read_to_string(&a[1]).unwrap()).unwrap();let before=snap(&doc);let mut session=Session::with_document(doc.clone());let now=Instant::now();
 let delta=session.execute(Command::EditPours{items:vec![],refill:None}).unwrap();let filled=snap(session.document());let changed=delta.upserted.len();
 if changed>0{session.undo().unwrap();assert_eq!(snap(session.document()),before);assert!(!session.can_undo());session.redo().unwrap();assert_eq!(snap(session.document()),filled);}
 let repeated=fill::fill_all(session.document());assert!(repeated.iter().all(|(id,fill)|matches!(session.document().get(*id),Some(Item::Zone(z))if z.fill==*fill)));let repeat=session.execute(Command::EditPours{items:vec![],refill:None}).unwrap();assert!(repeat.upserted.is_empty()&&repeat.removed.is_empty());
 let refill_ms=now.elapsed().as_millis();let mut moves=vec![];
 for (id,item) in doc.items().filter(|(_,i)|matches!(i,Item::Footprint(fp)if !fp.pad_nets.is_empty())).take(8){
  let Item::Footprint(fp)=item else{continue};
  for (angle,delta) in [(0.,Point::new(10000,0)),(90.,Point::default())]{let start=Instant::now();let plan=drag_footprints(&doc,&DragRequest{angle,flipped:false,center:fp.pos,ids:vec![id],delta,mode:MoveMode::PreserveAngles,route_mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45});let ms=start.elapsed().as_millis();
   if plan.failure.is_some()||plan.blocked>0{assert!(plan.replace.is_empty()&&plan.add.is_empty()&&plan.delete.is_empty());moves.push(json!({"component":fp.designator,"angle":angle,"accepted":false,"ms":ms}));continue;}
   let mut work=Session::with_document(doc.clone());let changed=work.execute_with_via_checks(Command::Route{delete:plan.delete,add:plan.add,replace:plan.replace},true);
   match changed {Ok(_)=>{let after=snap(work.document());work.undo().unwrap();assert_eq!(snap(work.document()),before);assert!(!work.can_undo());work.redo().unwrap();assert_eq!(snap(work.document()),after);moves.push(json!({"component":fp.designator,"angle":angle,"accepted":true,"ms":ms}));},Err(e)=>{assert_eq!(snap(work.document()),before);assert!(!work.can_undo());moves.push(json!({"component":fp.designator,"angle":angle,"accepted":false,"ms":ms,"message":e.to_string()}));}}
  }
 }
 let mut arrays=vec![];
 if let Some(path)=a.get(3).filter(|path|path.as_str()!="-") {
  let plans:serde_json::Value=serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
  for plan in plans.as_array().unwrap() {
   let add:Vec<Item>=serde_json::from_value(plan["items"].clone()).unwrap();let count=add.len();
   let mut work=Session::with_document(doc.clone());let start=Instant::now();
   match work.execute_with_via_checks(Command::Add{items:add},true) {
    Ok(delta)=>{
     let ids:Vec<_>=delta.upserted.iter().filter(|i|matches!(i.item,Item::Via(_))).map(|i|i.id).collect();
     let errors:Vec<_>=pcbplannercad_core::pcb::drc::run_near(work.document(),&ids).into_iter().filter(|v|v.severity==pcbplannercad_core::pcb::drc::Severity::Error).collect();assert!(errors.is_empty(),"{errors:?}");
     let after=snap(work.document());work.undo().unwrap();assert_eq!(snap(work.document()),before);assert!(!work.can_undo());work.redo().unwrap();assert_eq!(snap(work.document()),after);
     arrays.push(json!({"name":plan["name"],"accepted":true,"copies":count,"ms":start.elapsed().as_millis()}));
    },
    Err(e)=>{assert_eq!(snap(work.document()),before);assert!(!work.can_undo());assert!(!work.is_dirty());arrays.push(json!({"name":plan["name"],"accepted":false,"copies":count,"ms":start.elapsed().as_millis(),"message":e.to_string()}));}
   }
  }
 }
 let mut tuning=vec![];
 if a.iter().any(|arg|arg=="--tune") {
  use pcbplannercad_core::pcb::{router,query::{RuleSet,Obj,ObjKind},drc};
  let mut tracks:Vec<_>=doc.items().filter_map(|(id,item)|if let Item::Track(t)=item{let length=((t.b.x-t.a.x)as f64).hypot((t.b.y-t.a.y)as f64);(length>3e6&&!t.locked).then_some((id,t,length))}else{None}).collect();
  tracks.sort_by(|a,b|b.2.total_cmp(&a.2));let mut seen=std::collections::BTreeSet::new();
  let board=pcbplannercad_core::pcb::geometry::board_settings(&doc).unwrap().1;let rules=RuleSet::new(board);
  for (id,track,length) in tracks.into_iter().filter(|(_,t,_)|seen.insert(t.net.clone())).take(8) {
   let object=Obj::new(ObjKind::Track,&track.net,Some(track.layer));let clearance=rules.clearance(&object,&Obj::new(ObjKind::Track,"",Some(track.layer)));
   for style in [router::TuneStyle::Accordion,router::TuneStyle::Trombone] {
    let req=router::TuneRequest{tracks:vec![id],target_length:(length*1.05).round()as i64,amplitude:2_000_000,spacing:(track.width+clearance).max(300_000),style};let start=Instant::now();
    match router::tune_length(&doc,&req) {
     Err(message)=>tuning.push(json!({"net":track.net,"track":id,"style":style,"accepted":false,"message":message,"ms":start.elapsed().as_millis()})),
     Ok(preview)=>{
      let actual:f64=preview.iter().map(|t|((t.b.x-t.a.x)as f64).hypot((t.b.y-t.a.y)as f64)).sum();assert!((actual-req.target_length as f64).abs()<=req.target_length as f64*0.005);
      let command=router::tune_length_command(&doc,&req,Some(&preview)).unwrap();let mut work=Session::with_document(doc.clone());let delta=work.execute(command).unwrap();let ids:Vec<_>=delta.upserted.iter().filter(|i|matches!(i.item,Item::Track(_))).map(|i|i.id).collect();
      let errors:Vec<_>=drc::run_near(work.document(),&ids).into_iter().filter(|v|v.severity==drc::Severity::Error).collect();assert!(errors.is_empty(),"{errors:?}");
      let after=snap(work.document());let reopened=io::document_from_str(DocKind::Pcb,&after).unwrap();assert_eq!(snap(&reopened),after);work.undo().unwrap();assert_eq!(snap(work.document()),before);assert!(!work.can_undo());work.redo().unwrap();assert_eq!(snap(work.document()),after);
      tuning.push(json!({"net":track.net,"track":id,"style":style,"accepted":true,"segments":preview.len(),"actualNm":actual,"targetNm":req.target_length,"undoRedoAndReopen":true,"ms":start.elapsed().as_millis()}));
     }
    }
   }
  }
 }
 if let Some(path)=a.get(4).filter(|path|path.as_str()!="-") {assert!(!std::path::Path::new(path).exists(),"choose a new filled-board path");fs::write(path,&filled).unwrap();}
 assert_eq!(snap(&doc),before);fs::write(&a[2],serde_json::to_string_pretty(&json!({"refilledZones":changed,"refillUndoRedoAndIdempotence":true,"refillMs":refill_ms,"moves":moves,"arrays":arrays,"tuning":tuning})).unwrap()).unwrap();println!("{} zones refilled, {} placement probes",changed,moves.len());}
