//! Read-only acceptance of repeated coupled drags on a board copy.
use std::{env,fs,time::Instant};
use pcbplannercad_core::{io,Command,DocKind,Document,Item,ItemId,IdItem,Point,Session,pcb::{self,board::{DiffPair,Rule,RuleKind},geometry::board_settings,elastic::TrackRequest,router::{RouteMode,CornerStyle,RoutingEngine},drc}};
use serde_json::{json,Value};
fn snapshot(d:&Document)->String{io::document_to_string(d).unwrap()}
// A split may change a finding's representative point without changing copper.
// Retain source findings only when every participant is exact source geometry.
fn source_coverage(part:&Item,source:&Item)->bool{
 if part==source{return true;}
 let(Item::Track(a),Item::Track(b))=(part,source)else{return false;};
 if a.net!=b.net||a.layer!=b.layer||a.width!=b.width||b.a==b.b{return false;}
 let on=|p:Point|{let x=b.b.x as i128-b.a.x as i128;let y=b.b.y as i128-b.a.y as i128;x*(p.y as i128-b.a.y as i128)==y*(p.x as i128-b.a.x as i128)&&p.x>=b.a.x.min(b.b.x)&&p.x<=b.a.x.max(b.b.x)&&p.y>=b.a.y.min(b.b.y)&&p.y<=b.a.y.max(b.b.y)};
 on(a.a)&&on(a.b)
}
fn retained(doc:&Document,after:&Document,v:&drc::DrcViolation,baseline:&[drc::DrcViolation])->bool{
 baseline.iter().any(|old|old.code==v.code&&old.rule==v.rule&&old.layer==v.layer&&old.message.split(':').next()==v.message.split(':').next()&&old.items.len()==v.items.len()&&v.items.iter().all(|id|after.get(*id).is_some_and(|item|old.items.iter().any(|source|doc.get(*source).is_some_and(|original|source_coverage(item,original))))))
}
fn validate(doc:&Document,plan:pcb::drag::DragResult)->Value{
 let baseline:Vec<_>=drc::run(doc).violations.into_iter().filter(|v|v.severity==drc::Severity::Error).collect();
 let before=snapshot(doc);let mut s=Session::with_document(doc.clone());let now=Instant::now();let delta=s.execute_with_via_checks(Command::Route{replace:plan.replace,delete:plan.delete,add:plan.add},true).unwrap();let commit=now.elapsed().as_secs_f64()*1000.;
 let touched:Vec<_>=delta.upserted.iter().filter(|r|matches!(r.item,Item::Track(_)|Item::ArcTrack(_)|Item::Via(_))).map(|r|r.id).collect();let all_errors:Vec<_>=drc::run_near(s.document(),&touched).into_iter().filter(|v|v.severity==drc::Severity::Error).collect();let retained_count=all_errors.iter().filter(|v|retained(doc,s.document(),v,&baseline)).count();let problems:Vec<_>=all_errors.iter().filter(|v|!retained(doc,s.document(),v,&baseline)).map(|v|format!("{}: {} at {:?}",v.code,v.message,v.pos)).collect();
 for (id,item) in doc.items().filter(|(_,i)|matches!(i,Item::Via(_)|Item::Footprint(_))){assert_eq!(s.document().get(id),Some(item),"fixed anchor moved");}
 let mut changed_nets=std::collections::BTreeSet::new();for record in &delta.upserted{let net=match &record.item{Item::Track(t)=>&t.net,Item::ArcTrack(a)=>&a.net,_=>continue};if !doc.items().any(|(_,source)|source_coverage(&record.item,source)){changed_nets.insert(net.clone());}}
 let after=snapshot(s.document());s.undo().unwrap();assert_eq!(snapshot(s.document()),before);s.redo().unwrap();assert_eq!(snapshot(s.document()),after);
 json!({"commitMs":commit,"errors":problems,"existingErrorsRetained":retained_count,"changedNets":changed_nets,"exactHistory":true,"fixedAnchors":true})
}
fn req(ids:Vec<ItemId>,point:Point,delta:Point)->TrackRequest{TrackRequest{ids,point,delta,mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:1000}}
fn main(){
 let args:Vec<_>=env::args().collect();let ids:Vec<ItemId>=args[3].split(',').map(|v|ItemId(v.parse().unwrap())).collect();assert_eq!(ids.len(),2);
 let doc=io::document_from_str(DocKind::Pcb,&fs::read_to_string(&args[1]).unwrap()).unwrap();let original=snapshot(&doc);let(Some(Item::Track(t)),Some(Item::Track(u)))=(doc.get(ids[0]),doc.get(ids[1]))else{panic!("Select two straight conductors")};let(t,u)=(t.clone(),u.clone());let dx=(t.b.x-t.a.x)as f64;let dy=(t.b.y-t.a.y)as f64;let len=dx.hypot(dy);let gap=(((u.a.x-t.a.x)as f64*dy-(u.a.y-t.a.y)as f64*dx)/len).abs()-t.width as f64;
 let mut session=Session::with_document(doc.clone());let(bid,b)=board_settings(session.document()).unwrap();let mut b=b.clone();b.diff_pairs.push(DiffPair{name:"Temporary probe pair".into(),positive:t.net.clone(),negative:u.net.clone()});b.rules.insert(0,Rule{name:"Temporary probe spacing".into(),enabled:true,kind:RuleKind::DiffPair{scope:"All".into(),width:t.width,gap:gap.round()as i64}});session.execute(Command::ReplaceMany{items:vec![IdItem{id:bid,item:Item::Board(b)}]}).unwrap();let configured=snapshot(session.document());
 let amount:f64=args.get(4).map(|v|v.parse().unwrap()).unwrap_or(20000.);let repetitions:usize=args.get(5).map(|v|v.parse().unwrap()).unwrap_or(1);assert!((1..=64).contains(&repetitions));let mut frames=Vec::new();let mut applied=0;
 for index in 0..repetitions{
  let work=session.document().clone();let mut engine=RoutingEngine::new(work.clone());let Some(Item::Track(selected))=work.get(ids[0])else{panic!("Selected conductor lost its identity")};let shift=if index%2==0{amount}else{-amount};let request=req(vec![ids[0]],Point::new((selected.a.x+selected.b.x)/2,(selected.a.y+selected.b.y)/2),Point::new((-dy/len*shift).round()as i64,(dx/len*shift).round()as i64));let now=Instant::now();let plan=engine.drag(&request);let elapsed=now.elapsed().as_secs_f64()*1000.;assert_eq!(format!("{plan:?}"),format!("{:?}",engine.drag(&request)),"cached preview changed");assert_eq!(snapshot(engine.document()),snapshot(&work));
  match plan{Ok(plan)=>{let original_peer_touched=plan.replace.iter().any(|r|r.id==ids[1])||plan.delete.contains(&ids[1]);let validation=validate(&work,plan.clone());if !validation["changedNets"].as_array().unwrap().iter().any(|net|net==&u.net){frames.push(json!({"index":index,"accepted":false,"solverAccepted":true,"planMs":elapsed,"message":"Partner geometry did not move","validation":validation}));break;}let command=Command::Route{replace:plan.replace,delete:plan.delete,add:plan.add};session.execute_with_via_checks(command,true).unwrap();applied+=1;frames.push(json!({"index":index,"accepted":true,"originalPeerTouched":original_peer_touched,"planMs":elapsed,"validation":validation}));},Err(error)=>{frames.push(json!({"index":index,"accepted":false,"planMs":elapsed,"message":error}));break;}}
 }
 let after=snapshot(session.document());for _ in 0..applied{session.undo().unwrap();}assert_eq!(snapshot(session.document()),configured);for _ in 0..applied{session.redo().unwrap();}assert_eq!(snapshot(session.document()),after);assert_eq!(snapshot(&doc),original);
 let accepted=frames.len()==repetitions&&frames.iter().all(|v|v["accepted"]==true&&v["validation"]["errors"].as_array().is_some_and(|e|e.is_empty()));let output=json!({"ids":ids,"amount":amount,"repetitions":repetitions,"accepted":accepted,"frames":frames,"sourceUnchanged":true,"exactSequenceHistory":true});fs::write(&args[2],serde_json::to_string_pretty(&output).unwrap()).unwrap();println!("{output}");assert!(accepted,"Requested coupled drag sequence did not pass validation");
}
