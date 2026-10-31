//! Read-only native routing latency and plan-equivalence checks on a board copy.
use std::{env,fs,time::Instant};
use pcbplannercad_core::{io,DocKind,Point,Item,pcb::{geometry::{all_pads,board_settings},router::{self,RouteRequest,RouteMode,CornerStyle,RoutingEngine},elastic::{drag_tracks,TrackRequest}}};
use serde_json::json;
fn summary(samples:&[f64])->serde_json::Value{
    let mut times=samples.to_vec();times.sort_by(f64::total_cmp);
    let at=|q:f64|times[((times.len()as f64*q)as usize).min(times.len()-1)];json!({"samples":times.len(),"medianMs":at(0.5),"p95Ms":at(0.95)})
}
fn main(){
    let args:Vec<_>=env::args().collect();let text=fs::read_to_string(&args[1]).unwrap();let doc=io::document_from_str(DocKind::Pcb,&text).unwrap();
    let original=io::document_to_string(&doc).unwrap();let mut engine=RoutingEngine::new(doc.clone());let board=board_settings(&doc).unwrap().1;
    let pads=all_pads(&doc,board);let mut cold=vec![];let mut warm=vec![];let mut replay=vec![];let mut reached=0;let mut drags=0;
    for pad in pads.iter().filter(|p|!p.net.is_empty()&&p.net!="GND").step_by(11).take(8){
        for (dx,dy) in [(800_000,0),(0,800_000),(-800_000,0),(0,-800_000)]{
            let req=RouteRequest{net:pad.net.clone(),layer:pad.copper[0],start:pad.pos,end:Point::new(pad.pos.x+dx,pad.pos.y+dy),width:200_000,mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
            let now=Instant::now();let snapshot=doc.clone();let before=router::route(&snapshot,&req);cold.push(now.elapsed().as_secs_f64()*1000.);
            let now=Instant::now();let snapshot=doc.clone();assert!(engine.matches(&snapshot));let after=engine.route(&req);warm.push(now.elapsed().as_secs_f64()*1000.);
            assert_eq!(serde_json::to_string(&before).unwrap(),serde_json::to_string(&after).unwrap(),"snapshot route changed geometry");
            let now=Instant::now();let snapshot=doc.clone();assert!(engine.matches(&snapshot));let repeated=engine.route(&req);replay.push(now.elapsed().as_secs_f64()*1000.);
            assert_eq!(serde_json::to_string(&after).unwrap(),serde_json::to_string(&repeated).unwrap());if after.reached{reached+=1;}
        }
    }
    for (id,item) in doc.items().filter(|(_,i)|matches!(i,Item::Track(t)if !t.locked&&t.net!="GND")).step_by(73).take(6){
        let Item::Track(t)=item else{unreachable!()};let dx=(t.b.x-t.a.x)as f64;let dy=(t.b.y-t.a.y)as f64;let length=dx.hypot(dy);if length<500_000.{continue;}
        let req=TrackRequest{ids:vec![id],point:Point::new((t.a.x+t.b.x)/2,(t.a.y+t.b.y)/2),delta:Point::new((-dy/length*30_000.).round()as i64,(dx/length*30_000.).round()as i64),mode:RouteMode::Shove,corner:CornerStyle::Diagonal45,tolerance:1000};
        assert_eq!(format!("{:?}",drag_tracks(&doc,&req)),format!("{:?}",engine.drag(&req)));drags+=1;
    }
    assert_eq!(io::document_to_string(engine.document()).unwrap(),original);
    let result=json!({"objects":doc.len(),"layers":board.stackup.copper_layers().len(),"routes":cold.len(),"reached":reached,"dragEquivalenceChecks":drags,"uncachedIncludingSnapshot":summary(&cold),"indexedIncludingSnapshotAndValidation":summary(&warm),"revisitedIncludingSnapshotAndValidation":summary(&replay),"engine":engine.stats(),"sourceUnchanged":true,"note":"Native planning and source-copy CPU latency only; not display FPS or full pointer-to-present latency."});
    if let Some(path)=args.get(2){fs::write(path,serde_json::to_string_pretty(&result).unwrap()).unwrap();}println!("{}",serde_json::to_string_pretty(&result).unwrap());
}