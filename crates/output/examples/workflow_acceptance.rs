use std::{collections::BTreeMap, fs, path::{Path,PathBuf}, time::Instant};
use pcbplannercad_core::{io, schematic::{self,SheetRef}, pcb::{self,router::{self,CornerStyle,RouteMode,RouteRequest}}, Command,DocKind,Document,Item,ItemId,Point,Session};
use pcbplannercad_core::units::mm;
use pcbplannercad_output::{fab::{self,FabOptions},assembly::{PnpFormat,BomFormat},gerber_read};
use serde_json::json;
fn load(path:&Path,kind:DocKind)->Document {io::document_from_str(kind,&fs::read_to_string(path).unwrap()).unwrap()}
fn snapshot(doc:&Document)->String {io::document_to_string(doc).unwrap()}
fn main(){
 let root=PathBuf::from(std::env::args().nth(1).expect("Fixture directory argument required"));
 let out=root.join("acceptance-output");fs::create_dir_all(&out).unwrap();
 let mut results=Vec::new();
 let clean=load(&root.join("clean-routing.arpcb"),DocKind::Pcb);
 let mut session=Session::with_document(clean);
 // Live tests may have saved tracks into this disposable fixture. Start clean.
 let existing_routing=session.document().items().filter_map(|(id,item)|matches!(item,Item::Track(_)|Item::Via(_)).then_some(id)).collect();
 session.execute(Command::Route{delete:existing_routing,replace:vec![],add:vec![]}).unwrap();
 let baseline=snapshot(session.document());
 let request=RouteRequest{net:"SIGNAL".into(),layer:pcbplannercad_core::layer::TOP_COPPER,start:Point::new(mm(9.2),mm(10.0)),end:Point::new(mm(9.2),mm(16.0)),width:mm(0.25),mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
 let route=router::route(session.document(),&request);assert!(route.reached,"clean-pad escape blocked: {}",route.message);assert!(!route.tracks.is_empty());
 let mut add:Vec<Item>=route.tracks.into_iter().map(Item::Track).collect();
 let via=router::via_between(session.document(),"SIGNAL",request.end,request.layer,pcbplannercad_core::layer::BOTTOM_COPPER);add.push(Item::Via(via));
 session.execute(Command::Route{delete:vec![],replace:vec![],add}).unwrap();
 let routed=snapshot(session.document());let report=pcb::drc::run(session.document());
 assert!(!report.violations.iter().any(|v|matches!(v.code,"short"|"clearance")),"clean route/via created a clearance defect");
 session.undo().unwrap();assert_eq!(snapshot(session.document()),baseline);session.redo().unwrap();assert_eq!(snapshot(session.document()),routed);
 let round=io::document_from_str(DocKind::Pcb,&routed).unwrap();assert_eq!(snapshot(&round),routed);
 fs::write(out.join("route-via-roundtrip.arpcb"),&routed).unwrap();results.push(json!({"check":"pad-route-via-undo-redo-save-reopen","status":"passed","objects":round.len()}));
 let id=ItemId(1);let p=if let Some(Item::Footprint(fp))=session.document().get(id){fp.pos}else{panic!("missing source")};
 let drag=pcb::drag::drag_footprints(session.document(),&pcb::drag::DragRequest{ids:vec![id],delta:Point::new(mm(2.0),mm(1.0)),angle:90.0,flipped:false,center:p,mode:pcb::drag::MoveMode::PreserveAngles,route_mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45});
 let blocked=drag.blocked;
 session.execute(Command::Route{delete:drag.delete,add:drag.add,replace:drag.replace}).unwrap();
 let connectivity=pcb::connectivity::analyze(session.document());
 let signal_copper:Vec<_>=session.document().items().filter_map(|(id,item)|match item {
  Item::Track(track) if track.net=="SIGNAL"=>Some(id),
  Item::Via(via) if via.net=="SIGNAL"=>Some(id),
  _=>None,
 }).collect();
 assert!(!signal_copper.is_empty());
 for copper in &signal_copper {
  assert!(connectivity.connected((id,Some("1")),(*copper,None)),"connected move detached SIGNAL copper {} from pad 1",copper.0);
 }
 session.undo().unwrap();assert_eq!(snapshot(session.document()),routed);
 results.push(json!({"check":"connected-rotate-move-undo","status":"passed","blockedConnections":blocked,"connectedCopperObjects":signal_copper.len()}));
 let schematic_path=root.join("clean-schematic.arsch");
 if schematic_path.exists(){
  let schematic=load(&schematic_path,DocKind::Schematic);
  let compiled=schematic::compile(&[SheetRef::new("clean-schematic.arsch",&schematic)]);
  assert_eq!(compiled.parts.len(),2);assert_eq!(compiled.nets.len(),2);assert!(compiled.violations.is_empty());
  results.push(json!({"check":"live-schematic-wire-save-compile","status":"passed","parts":2,"nets":2,"violations":0}));
 }
 let project_dir=root.join("full-project");let project_file=project_dir.join("kit-dev-coldfire-xilinx_5213.arproj");
 if project_file.exists(){
  let project=io::project_from_str(&fs::read_to_string(project_file).unwrap()).unwrap();
  let docs:Vec<_>=project.sheets.iter().map(|path|(path.clone(),load(&project_dir.join(path),DocKind::Schematic))).collect();
  let refs:Vec<_>=docs.iter().map(|(name,doc)|SheetRef::new(name,doc)).collect();let now=Instant::now();let compiled=schematic::compile(&refs);let seconds=now.elapsed().as_secs_f64();assert!(!compiled.parts.is_empty()&&!compiled.nets.is_empty());
  fs::write(out.join("compiled-project.json"),serde_json::to_string_pretty(&compiled).unwrap()).unwrap();fs::write(out.join("schematic-bom.csv"),schematic::bom_csv(&compiled)).unwrap();
  let reloaded:Vec<_>=docs.iter().map(|(name,doc)|(name.clone(),io::document_from_str(DocKind::Schematic,&snapshot(doc)).unwrap())).collect();let refs2:Vec<_>=reloaded.iter().map(|(name,doc)|SheetRef::new(name,doc)).collect();let again=schematic::compile(&refs2);assert_eq!(serde_json::to_value(&compiled).unwrap(),serde_json::to_value(&again).unwrap());
  results.push(json!({"check":"multisheet-compile-erc-bom-save-reopen","status":"passed","sheets":docs.len(),"parts":compiled.parts.len(),"nets":compiled.nets.len(),"violations":compiled.violations.len(),"compileSeconds":seconds}));
  let board=load(&project_dir.join(project.pcb.as_ref().unwrap()),DocKind::Pcb);let mut libs=BTreeMap::new();for library in &project.libraries{let lib=io::library_from_str(&fs::read_to_string(project_dir.join(library)).unwrap()).unwrap();libs.insert(lib.name.clone(),lib);}
  let resolver=|part:&schematic::NetlistPart|{for lib in libs.values(){if let Some(fp)=lib.footprints.iter().find(|fp|fp.name==part.footprint){return Some((format!("{}/{}",lib.name,part.footprint),fp.clone()));}}None};
  let plan=pcb::sync::plan(&compiled,&board,&resolver);let mut sync=Session::with_document(board.clone());for(key,fp)in &plan.footprints{sync.put_cached_footprint(key.clone(),fp.clone());}let before=snapshot(sync.document());let changes:Vec<_>=(0..plan.changes.len()).collect();if !changes.is_empty(){sync.execute(pcb::sync::apply(&plan,sync.document(),&changes)).unwrap();sync.undo().unwrap();assert_eq!(snapshot(sync.document()),before);}
  results.push(json!({"check":"schematic-board-eco-plan-undo","status":"passed","changes":plan.changes.len(),"warningChanges":plan.changes.iter().filter(|change|matches!(change,pcb::sync::EcoChange::Warning{..})).count()}));
 }
 let board=load(&root.join("video-workflow.arpcb"),DocKind::Pcb);let now=Instant::now();let check=pcb::drc::run(&board);let elapsed=now.elapsed().as_secs_f64();
 results.push(json!({"check":"large-board-full-drc","status":"completed","objects":board.len(),"seconds":elapsed,"violations":check.violations.len(),"ruleErrors":check.rule_errors}));
 let large_path=root.parent().unwrap().join("native/vme-wren.arpcb");
 if large_path.exists(){
  let now=Instant::now();let large=load(&large_path,DocKind::Pcb);let load_seconds=now.elapsed().as_secs_f64();
  let now=Instant::now();let check=pcb::drc::run(&large);
  results.push(json!({"check":"larger-board-load-full-drc","status":"completed","objects":large.len(),"loadSeconds":load_seconds,"drcSeconds":now.elapsed().as_secs_f64(),"violations":check.violations.len(),"ruleErrors":check.rule_errors}));
 }
 let mut options=FabOptions::default();options.gerber.project_name="workflow".into();options.zip=true;options.pnp=Some(PnpFormat::Generic);options.bom=Some(BomFormat::Generic);
 let now=Instant::now();let files=fab::write_package(&board,&out.join("fabrication"),&options).unwrap();let mut warnings=Vec::new();let mut gerbers=0;let mut drills=0;
 for path in &files {let name=path.file_name().unwrap().to_str().unwrap();let ext=path.extension().and_then(|e|e.to_str()).unwrap_or("");if ext=="zip"||ext=="csv"||ext=="txt"{continue;}let text=fs::read_to_string(path).unwrap();let parsed=if ext=="drl"{drills+=1;gerber_read::parse_excellon(name,&text)}else{gerbers+=1;gerber_read::parse_gerber(name,&text)}.unwrap();warnings.extend(parsed.warnings);}
 results.push(json!({"check":"manufacturing-export-reimport","status":"passed","files":files.len(),"gerbers":gerbers,"drillFiles":drills,"warnings":warnings,"seconds":now.elapsed().as_secs_f64()}));
 fs::write(out.join("results.json"),serde_json::to_string_pretty(&results).unwrap()).unwrap();println!("{}",serde_json::to_string_pretty(&results).unwrap());
}
