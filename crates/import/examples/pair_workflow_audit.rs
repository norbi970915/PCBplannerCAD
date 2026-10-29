//! Exercise pad starts and frontend-generated via transitions on disposable native documents.
use std::{env,fs};
use pcbplannercad_core::{io,Document,DocKind,Item,Session,Command,LayerId,units::{Point,mm},pcb::{geometry::{board_settings,all_pads},router::{self,RouteRequest,RouteMode,CornerStyle},drc}};
use serde_json::{json,Value};
fn snapshot(doc:&Document)->String {io::document_to_string(doc).unwrap()}
fn errors(doc:&Document,ids:&[pcbplannercad_core::ItemId])->Vec<String>{drc::run_near(doc,&ids.iter().copied().filter(|id|matches!(doc.get(*id),Some(Item::Track(_)|Item::ArcTrack(_)|Item::Via(_)))).collect::<Vec<_>>()).into_iter().filter(|v|v.severity==drc::Severity::Error).map(|v|v.message).collect()}
fn main(){
 let args:Vec<_>=env::args().collect();let doc=io::document_from_str(DocKind::Pcb,&fs::read_to_string(&args[1]).unwrap()).unwrap();let before=snapshot(&doc);
 let board=board_settings(&doc).unwrap().1;let layers=board.stackup.copper_layers();
 if args.len()==3 {
  let pads=all_pads(&doc,board);let mut cases=vec![];let mut candidates=vec![];
  for pair in board.diff_pairs.iter().take(12){
   let class=board.net_classes.iter().find(|c|c.nets.contains(&pair.positive)).unwrap_or(&board.net_classes[0]);let width=class.diff_pair_width;let gap=class.diff_pair_gap;
   let Some(pad)=pads.iter().find(|p|p.net==pair.positive&&pads.iter().any(|n|n.owner==p.owner&&n.net==pair.negative)) else{continue};
   let layer=pad.copper[0];let mut attempts=vec![];
   for (dx,dy) in [(1,0),(0,1),(-1,0),(0,-1),(1,1),(-1,1),(-1,-1),(1,-1)]{
    let req=RouteRequest{net:pair.positive.clone(),layer,start:pad.pos,end:Point::new(pad.pos.x+dx*mm(3.),pad.pos.y+dy*mm(3.)),width,mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
    let (p,n)=router::route_diff_pair(&doc,&req,&pair.negative,gap);
    if !p.reached||!n.reached {attempts.push(p.message);continue;}
    let tracks:Vec<Item>=p.tracks.iter().chain(&n.tracks).cloned().map(Item::Track).collect();
    let mut work=Session::with_document(doc.clone());let delta=work.execute(Command::Route{delete:vec![],replace:vec![],add:tracks.clone()}).unwrap();let violations=errors(work.document(),&delta.upserted.iter().map(|i|i.id).collect::<Vec<_>>());
    if !violations.is_empty(){attempts.extend(violations);continue;}
    let end=p.tracks.last().unwrap().b;let negative=n.tracks.last().unwrap().b;let last=p.tracks.last().unwrap();
    assert_eq!(p.tracks[0].a,pad.pos);
    for target in layers.iter().filter(|l|**l!=layer){
     let positive=router::via_between(&doc,&pair.positive,end,layer,*target);let negative=router::via_between(&doc,&pair.negative,negative,layer,*target);let clearance=router::pair_via_clearance(&doc,&positive,&negative,layer).unwrap();
     candidates.push(json!({"net":pair.positive,"otherNet":pair.negative,"pad":format!("{}.{}",pad.designator,pad.number),"layer":layer,"target":target,"width":width,"gap":gap,"tracks":tracks,"direction":{"x":end.x-last.a.x,"y":end.y-last.a.y},"positive":Item::Via(positive),"negative":Item::Via(negative),"clearance":clearance}));
    }
    attempts.push("accepted".into());break;
   }
   cases.push(json!({"net":pair.positive,"pad":format!("{}.{}",pad.designator,pad.number),"attempts":attempts}));
  }
  fs::write(&args[2],serde_json::to_string_pretty(&json!({"pads":cases,"candidates":candidates})).unwrap()).unwrap();println!("{} pad cases, {} via candidates",cases.len(),candidates.len());
 }else{
  let plans:Value=serde_json::from_str(&fs::read_to_string(&args[2]).unwrap()).unwrap();let mut results=vec![];
  for plan in plans.as_array().unwrap(){
   let items:Vec<Item>=serde_json::from_value(plan["items"].clone()).unwrap();let mut session=Session::with_document(doc.clone());
   let command=Command::Route{delete:vec![],replace:vec![],add:items.clone()};
   let outcome=session.execute_with_via_checks(command.clone(),true);
   match outcome {
    Err(e)=>{assert_eq!(snapshot(session.document()),before);assert!(!session.can_undo());results.push(json!({"net":plan["net"],"target":plan["target"],"accepted":false,"message":e.to_string()}));},
    Ok(delta)=>{
     assert!(errors(session.document(),&delta.upserted.iter().map(|i|i.id).collect::<Vec<_>>()).is_empty());let after=snapshot(session.document());session.undo().unwrap();assert_eq!(snapshot(session.document()),before);assert!(!session.can_undo());session.redo().unwrap();assert_eq!(snapshot(session.document()),after);
     let vias:Vec<_>=items.iter().filter_map(|i|if let Item::Via(v)=i{Some(v)}else{None}).collect();assert_eq!(vias.len(),2);
     let direction:Point=serde_json::from_value(plan["direction"].clone()).unwrap();let len=(direction.x as f64).hypot(direction.y as f64).max(1.);
     let req=RouteRequest{net:vias[0].net.clone(),layer:LayerId(plan["target"].as_u64().unwrap()as u16),start:vias[0].pos,end:Point::new(vias[0].pos.x+(direction.x as f64/len*mm(3.) as f64)as i64,vias[0].pos.y+(direction.y as f64/len*mm(3.) as f64)as i64),width:plan["width"].as_i64().unwrap(),mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
     let dx=req.end.x-req.start.x;let dy=req.end.y-req.start.y;let mut continued=false;let mut messages=vec![];
     for (x,y) in [(dx,dy),(-dy,dx),(dy,-dx),(-dx,-dy)] {
      let mut request=req.clone();request.end=Point::new(req.start.x+x,req.start.y+y);
      let (p,n)=router::route_diff_pair_from(session.document(),&request,&vias[1].net,plan["gap"].as_i64().unwrap(),Some(vias[1].pos));
      if p.reached&&n.reached {let mut check=Session::with_document(session.document().clone());let delta=check.execute(Command::Route{delete:vec![],replace:vec![],add:p.tracks.into_iter().chain(n.tracks).map(Item::Track).collect()}).unwrap();let violations=errors(check.document(),&delta.upserted.iter().map(|i|i.id).collect::<Vec<_>>());if violations.is_empty(){continued=true;break;}messages.extend(violations);}else{messages.push(p.message);}
     }
     results.push(json!({"net":plan["net"],"target":plan["target"],"accepted":true,"continuation":continued,"messages":messages,"undoRedo":true}));
    }
   }
  }
  fs::write(&args[3],serde_json::to_string_pretty(&results).unwrap()).unwrap();println!("{} transitions accepted / {} tested",results.iter().filter(|r|r["accepted"]==true).count(),results.len());
 }
 assert_eq!(snapshot(&doc),before);
}
