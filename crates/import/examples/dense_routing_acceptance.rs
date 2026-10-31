//! Read-only routing acceptance probes on an owner-supplied native board copy.
use std::{collections::{BTreeSet,HashSet},env,fs,time::Instant};
use pcbplannercad_core::{io,Command,Document,DocKind,IdItem,Item,Session,units::{Point,mm},pcb::{drc,elastic::{drag_tracks,TrackRequest},geometry::{all_pads,board_settings},query::{Obj,ObjKind,RuleSet},router::{self,RouteRequest,RouteMode,CornerStyle,PairViaRequest},Track}};
use serde_json::json;

fn snapshot(doc:&Document)->String{io::document_to_string(doc).unwrap()}
fn add(a:Point,dx:f64,dy:f64)->Point{Point::new(a.x+dx.round()as i64,a.y+dy.round()as i64)}
fn route_command(tracks:Vec<Track>,moved:Vec<IdItem>)->Command{
    Command::Route{delete:moved.iter().map(|m|m.id).collect::<BTreeSet<_>>().into_iter().collect(),replace:vec![],add:moved.into_iter().map(|m|m.item).chain(tracks.into_iter().map(Item::Track)).collect()}
}
fn validate_history(doc:&Document,command:Command)->Result<(),String>{
    let before=snapshot(doc);let mut session=Session::with_document(doc.clone());
    let delta=session.execute_with_via_checks(command,true).map_err(|e|e.to_string())?;
    let touched:Vec<_>=delta.upserted.iter().filter(|i|matches!(i.item,Item::Track(_)|Item::ArcTrack(_)|Item::Via(_))).map(|i|i.id).collect();
    let errors:Vec<_>=drc::run_near(session.document(),&touched).into_iter().filter(|v|v.severity==drc::Severity::Error).map(|v|format!("{}: {}",v.code,v.message)).collect();
    if !errors.is_empty(){return Err(errors.join("; "));}
    let after=snapshot(session.document());session.undo().ok_or("missing Undo")?;assert_eq!(snapshot(session.document()),before);assert!(!session.is_dirty());
    session.redo().ok_or("missing Redo")?;assert_eq!(snapshot(session.document()),after);Ok(())
}
fn main(){
    let args:Vec<_>=env::args().collect();let input=fs::read_to_string(&args[1]).unwrap();let doc=io::document_from_str(DocKind::Pcb,&input).unwrap();let original=snapshot(&doc);
    let stage=args.get(3).map_or("all",String::as_str);let board=board_settings(&doc).unwrap().1;let rules=RuleSet::new(board);let pads=all_pads(&doc,board);
    let mut pad_cases=vec![];let mut drag_cases=vec![];let mut pair_cases=vec![];let mut controlled=vec![];let mut unexpected=vec![];
    if stage=="all"||stage=="pads"{
        let mut owners=HashSet::new();
        for pad in pads.iter().filter(|p|!p.net.is_empty()&&p.net!="GND"&&!p.net.starts_with('+')&&p.width.max(p.height)>p.width.min(p.height)*3/2){
            if !owners.insert(pad.owner){continue;}if owners.len()>12{break;}
            let layer=pad.copper[0];let width=rules.width(&Obj::new(ObjKind::Track,&pad.net,Some(layer))).1;
            let angle=(pad.rotation+if pad.height>pad.width{90.}else{0.}).to_radians();let step=(pad.width.max(pad.height)as f64/2.+mm(0.6)as f64).max(mm(0.8)as f64);
            for turn in [0.,std::f64::consts::PI,std::f64::consts::FRAC_PI_2,-std::f64::consts::FRAC_PI_2]{
                let a=angle+turn;let end=add(pad.pos,a.cos()*step,a.sin()*step);
                let req=RouteRequest{net:pad.net.clone(),layer,start:pad.pos,end,width,mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
                let start=Instant::now();let plan=router::route(&doc,&req);let micros=start.elapsed().as_micros();
                let mut record=json!({"pad":format!("{}.{}",pad.designator,pad.number),"net":pad.net,"layer":layer,"from":pad.pos,"to":end,"reached":plan.reached,"planUs":micros,"message":plan.message});
                if plan.reached {
                    assert_eq!(plan.tracks.first().unwrap().a,pad.pos);assert_eq!(plan.tracks.last().unwrap().b,end);
                    let count=plan.tracks.len();let pushed=plan.moved.len();let result=validate_history(&doc,route_command(plan.tracks,plan.moved));
                    record["segments"]=json!(count);record["pushedPieces"]=json!(pushed);record["safe"]=json!(result.is_ok());
                    if let Err(error)=result{record["error"]=json!(error);unexpected.push(record.clone());}
                }
                pad_cases.push(record);
            }
        }
        println!("pad starts: {} attempts, {} reached, {} unsafe",pad_cases.len(),pad_cases.iter().filter(|v|v["reached"]==true).count(),unexpected.len());
    }
    if stage=="all"||stage=="drags"{
        let mut candidates:Vec<_>=doc.items().filter_map(|(id,item)|if let Item::Track(t)=item{let length=((t.b.x-t.a.x)as f64).hypot((t.b.y-t.a.y)as f64);(!t.locked&&t.net!="GND"&&length>mm(1.)as f64).then_some((id,t.clone(),length))}else{None}).collect();
        candidates.sort_by(|a,b|b.2.total_cmp(&a.2));let mut used=HashSet::new();let mut accepted=0;
        for (id,initial,length) in candidates.into_iter().filter(|(_,t,_)|used.insert(t.net.clone())).take(16){
            let normal=(-(initial.b.y-initial.a.y)as f64/length,(initial.b.x-initial.a.x)as f64/length);let origin=Point::new((initial.a.x+initial.b.x)/2,(initial.a.y+initial.b.y)/2);
            let mut session=Session::with_document(doc.clone());let mut at=origin;let mut steps=vec![];let initial_count=doc.items().count();let mut gesture=vec![];
            for amount in [mm(0.03),mm(0.03),-mm(0.06),-mm(0.03),mm(0.03)]{
                let delta=Point::new((normal.0*amount as f64).round()as i64,(normal.1*amount as f64).round()as i64);
                let req=TrackRequest{ids:vec![id],point:at,delta,mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:1000};
                let start=Instant::now();let result=drag_tracks(session.document(),&req);let micros=start.elapsed().as_micros();
                match result{
                    Err(error)=>{steps.push(json!({"accepted":false,"message":error,"planUs":micros}));break;},
                    Ok(plan)=>{
                        let command=Command::Route{delete:plan.delete,replace:plan.replace,add:plan.add};
                        if let Err(error)=validate_history(session.document(),command.clone()){unexpected.push(json!({"kind":"drag","id":id,"error":error}));break;}
                        session.execute_with_via_checks(command,true).unwrap();at=add(at,delta.x as f64,delta.y as f64);
                        steps.push(json!({"accepted":true,"planUs":micros,"growth":session.document().items().count()as i64-initial_count as i64}));
                    }
                }
            }
            let success=steps.iter().filter(|s|s["accepted"]==true).count();if success>0{
                accepted+=1;let mut seen=std::collections::BTreeMap::new();
                for amount in [mm(0.03),mm(0.06),mm(0.03),0,mm(0.06),0]{
                    let req=TrackRequest{ids:vec![id],point:origin,delta:Point::new((normal.0*amount as f64).round()as i64,(normal.1*amount as f64).round()as i64),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:1000};
                    let start=Instant::now();match drag_tracks(&doc,&req){
                        Err(error)=>gesture.push(json!({"amount":amount,"accepted":false,"message":error})),
                        Ok(plan)=>{let encoded=serde_json::to_string(&plan).unwrap();if let Some(previous)=seen.insert(amount,encoded.clone()){assert_eq!(previous,encoded,"retreat changed a reviewed plan");}
                            if amount==0{assert!(plan.replace.is_empty()&&plan.delete.is_empty()&&plan.add.is_empty());}
                            gesture.push(json!({"amount":amount,"accepted":true,"noop":amount==0,"planUs":start.elapsed().as_micros()}));
                        }
                    }
                }
            }
            drag_cases.push(json!({"id":id,"net":initial.net,"layer":initial.layer,"steps":steps,"gesture":gesture,"acceptedSteps":success,"complete":success==5}));
            if accepted>=5{break;}
        }
        println!("track drags: {} selections, {} accepted steps",drag_cases.len(),drag_cases.iter().map(|v|v["acceptedSteps"].as_u64().unwrap()).sum::<u64>());
    }
    if stage=="all"||stage=="pairs"{
        let mut owners=HashSet::new();let mut successes=0;
        for pad in pads.iter().filter(|p|!p.net.is_empty()&&p.net!="GND"&&!p.net.starts_with('+')){
            if owners.contains(&pad.owner){continue;}
            let layer=pad.copper[0];let partner=pads.iter().filter(|n|n.owner==pad.owner&&n.net!=pad.net&&!n.net.is_empty()&&n.net!="GND"&&!n.net.starts_with('+')&&n.on_layer(layer)).min_by_key(|n|{let x=(n.pos.x-pad.pos.x)as i128;let y=(n.pos.y-pad.pos.y)as i128;x*x+y*y});
            let Some(partner)=partner else{continue};let (width,gap)=rules.diff_pair(&Obj::new(ObjKind::Track,&pad.net,Some(layer)));
            let dx=(partner.pos.x-pad.pos.x)as f64;let dy=(partner.pos.y-pad.pos.y)as f64;let distance=dx.hypot(dy);if distance<1.||distance>mm(2.)as f64{continue;}
            owners.insert(pad.owner);if owners.len()>16{break;}
            for direction in [(-dy/distance,dx/distance),(dy/distance,-dx/distance)]{
                for reach in [1.0,2.0,3.0,4.0]{
                let end=add(pad.pos,direction.0*mm(reach)as f64,direction.1*mm(reach)as f64);let req=RouteRequest{net:pad.net.clone(),layer,start:pad.pos,end,width,mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
                let start=Instant::now();let (p,n)=router::route_diff_pair_from(&doc,&req,&partner.net,gap,Some(partner.pos));let mut record=json!({"pad":format!("{}.{}",pad.designator,pad.number),"mate":format!("{}.{}",partner.designator,partner.number),"nets":[pad.net,partner.net],"reached":p.reached&&n.reached,"planUs":start.elapsed().as_micros(),"message":p.message,"transitions":[]});
                if p.reached&&n.reached {
                    assert_eq!(p.tracks.first().unwrap().a,pad.pos);assert_eq!(n.tracks.first().unwrap().a,partner.pos);
                    let positive=p.tracks.last().unwrap().b;let negative=n.tracks.last().unwrap().b;let last=p.tracks.last().unwrap();let vector=Point::new(positive.x-last.a.x,positive.y-last.a.y);
                    let tracks:Vec<_>=p.tracks.into_iter().chain(n.tracks).collect();let moved=p.moved;
                    if let Err(error)=validate_history(&doc,route_command(tracks.clone(),moved.clone())){record["error"]=json!(error);unexpected.push(record.clone());}
                    else{
                        successes+=1;
                        let targets:Vec<_>=board.stackup.copper_layers().into_iter().filter(|l|*l!=layer).take(3).collect();let mut transitions=vec![];
                        for target in targets{
                            let request=PairViaRequest{positive:router::via_between(&doc,&pad.net,positive,layer,target),negative:router::via_between(&doc,&partner.net,negative,layer,target),source:layer,target,direction:vector,width,corner:CornerStyle::Diagonal45,mode:RouteMode::Walkaround,tracks:tracks.clone(),moved:moved.clone()};
                            let start=Instant::now();match router::pair_via_plan(&doc,&request){
                                Err(error)=>transitions.push(json!({"target":target,"accepted":false,"message":error,"planUs":start.elapsed().as_micros()})),
                                Ok(plan)=>{
                                    let micros=start.elapsed().as_micros();let command=Command::Route{delete:moved.iter().map(|m|m.id).collect::<BTreeSet<_>>().into_iter().collect(),replace:vec![],add:moved.iter().map(|m|m.item.clone()).chain(plan.tracks.iter().cloned().map(Item::Track)).chain(plan.vias.iter().cloned().map(Item::Via)).collect()};
                                    let result=validate_history(&doc,command);let record=json!({"target":target,"accepted":true,"safe":result.is_ok(),"planUs":micros,"message":result.err(),"vias":plan.vias});if record["safe"]!=true{unexpected.push(record.clone());}transitions.push(record);
                                }
                            }
                        }
                        record["transitions"]=json!(transitions);
                    }
                }
                pair_cases.push(record);
                }
            }
            if successes>=8{break;}
        }
        println!("paired routes: {} attempts, {} safe starts",pair_cases.len(),successes);
    }
    if stage=="all"||stage=="transitions"{
        let outline=pcbplannercad_core::pcb::geometry::outline_loops(&doc,1000);let points:Vec<_>=outline.iter().flatten().collect();
        let min_x=points.iter().map(|p|p.x).min().unwrap();let max_x=points.iter().map(|p|p.x).max().unwrap();let min_y=points.iter().map(|p|p.y).min().unwrap();let max_y=points.iter().map(|p|p.y).max().unwrap();
        let source=board.stackup.copper_layers()[0];let (width,gap)=rules.diff_pair(&Obj::new(ObjKind::Track,"__accept_pair_P",Some(source)));let spacing=width+gap;
        let test_doc=doc.clone();let mut settings=board.clone();
        settings.diff_pairs.push(pcbplannercad_core::pcb::board::DiffPair{name:"Acceptance pair".into(),positive:"__accept_pair_P".into(),negative:"__accept_pair_N".into()});
        let id=board_settings(&test_doc).unwrap().0;let mut setup=Session::with_document(test_doc);setup.execute(Command::Replace{id,item:Item::Board(settings)}).unwrap();let doc=setup.document().clone();
        'corners: for (x,y,sx,sy) in [(min_x,min_y,1.,1.),(min_x,max_y,1.,-1.),(max_x,min_y,-1.,1.),(max_x,max_y,-1.,-1.)]{
            for inset in [1.2,2.0,3.0]{
                let start=Point::new(x+(sx*mm(0.8)as f64)as i64,y+(sy*mm(inset)as f64)as i64);let end=add(start,sx*mm(0.6)as f64,0.);let other=add(end,0.,sy*spacing as f64);
                let tracks=vec![Track{a:start,b:end,width,layer:source,net:"__accept_pair_P".into(),locked:false},Track{a:add(start,0.,sy*spacing as f64),b:other,width,layer:source,net:"__accept_pair_N".into(),locked:false}];
                let layers=board.stackup.copper_layers();let targets:Vec<_>=[layers[1],layers[layers.len()/2],*layers.last().unwrap()].into_iter().collect();
                for target in targets{
                    let req=PairViaRequest{positive:router::via_between(&doc,"__accept_pair_P",end,source,target),negative:router::via_between(&doc,"__accept_pair_N",other,source,target),source,target,direction:Point::new((sx*mm(1.)as f64)as i64,0),width,corner:CornerStyle::Diagonal45,mode:RouteMode::Walkaround,tracks:tracks.clone(),moved:vec![]};
                    let begin=Instant::now();let plan=match router::pair_via_plan(&doc,&req){Ok(p)=>p,Err(error)=>{controlled.push(json!({"target":target,"accepted":false,"message":error,"synthetic":true}));continue;}};
                    let micros=begin.elapsed().as_micros();let command=Command::Route{delete:vec![],replace:vec![],add:plan.tracks.iter().cloned().map(Item::Track).chain(plan.vias.iter().cloned().map(Item::Via)).collect()};
                    if let Err(error)=validate_history(&doc,command.clone()){unexpected.push(json!({"kind":"controlled-transition","error":error}));continue;}
                    let mut session=Session::with_document(doc.clone());session.execute_with_via_checks(command,true).unwrap();let mut continuation=false;let mut message=String::new();
                    for (dx,dy) in [(sx,0.),(0.,sy),(0.,-sy)]{
                        let request=RouteRequest{net:plan.vias[0].net.clone(),layer:target,start:plan.vias[0].pos,end:add(plan.vias[0].pos,dx*mm(1.)as f64,dy*mm(1.)as f64),width,mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
                        let (p,n)=router::route_diff_pair_from(session.document(),&request,&plan.vias[1].net,gap,Some(plan.vias[1].pos));
                        if p.reached&&n.reached{
                            match validate_history(session.document(),route_command(p.tracks.into_iter().chain(n.tracks).collect(),p.moved)) {Ok(())=>{continuation=true;break;},Err(error)=>{unexpected.push(json!({"kind":"continuation","error":error}));}}
                        }else{message=p.message;}
                    }
                    let out=std::path::Path::new(&args[2]).with_file_name("controlled-pair-transition.arpcb");fs::write(out,snapshot(session.document())).unwrap();
                    controlled.push(json!({"target":target,"accepted":true,"safe":true,"synthetic":true,"continuation":continuation,"message":message,"planUs":micros,"vias":plan.vias}));
                }
                if controlled.iter().any(|c|c["accepted"]==true){break 'corners;}
            }
        }
        println!("controlled dense-background transitions: {} accepted / {} tested",controlled.iter().filter(|c|c["accepted"]==true).count(),controlled.len());
    }
    assert_eq!(snapshot(&doc),original);assert_eq!(fs::read_to_string(&args[1]).unwrap(),input);
    fs::write(&args[2],serde_json::to_string_pretty(&json!({"objects":doc.items().count(),"copperLayers":board.stackup.copper_layers().len(),"configuredPairs":board.diff_pairs.len(),"pairTestsUseAdjacentSignalPads":true,"pads":pad_cases,"drags":drag_cases,"pairs":pair_cases,"controlledTransitions":controlled,"unexpected":unexpected})).unwrap()).unwrap();
    println!("report saved; source unchanged");
}
