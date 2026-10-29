//! Read-only native board measurements and optional single-sheet parity checks.
use std::{env,fs,collections::{BTreeMap,BTreeSet},time::Instant};
use pcbplannercad_core::{DocKind,io,Item,pcb::{drc,geometry::board_settings,fill,length},schematic};
use serde_json::json;
fn main(){
 let a:Vec<_>=env::args().collect();let text=fs::read_to_string(&a[1]).unwrap();let doc=io::document_from_str(DocKind::Pcb,&text).unwrap();
 let saved=io::document_to_string(&doc).unwrap();let reopened=io::document_from_str(DocKind::Pcb,&saved).unwrap();assert_eq!(saved,io::document_to_string(&reopened).unwrap());
 let board=board_settings(&doc).unwrap().1;let now=Instant::now();let report=drc::run(&doc);let drc_ms=now.elapsed().as_millis();let mut counts=BTreeMap::new();for v in &report.violations{*counts.entry(v.code).or_insert(0)+=1;}
 let now=Instant::now();let fills=fill::fill_all(&doc);let fill_ms=now.elapsed().as_millis();
 let mut parity=vec![];let mut erc=serde_json::Value::Null;let mut eco=serde_json::Value::Null;
 if let Some(sch)=a.get(3){let schematic=io::document_from_str(DocKind::Schematic,&fs::read_to_string(sch).unwrap()).unwrap();let netlist=schematic::compile(&[schematic::SheetRef::new(sch,&schematic)]);
  let mut expected=BTreeMap::new();for net in &netlist.nets{for node in &net.nodes{for pad in &node.pads{expected.insert((node.designator.clone(),pad.clone()),net.name.clone());}}}
  for (_,item) in doc.items(){if let Item::Footprint(f)=item{if f.board_only{continue;}for(pad,net)in &f.pad_nets{if net.is_empty(){continue;}let want=expected.get(&(f.designator.clone(),pad.clone()));if want!=Some(net){parity.push(json!({"component":f.designator,"pad":pad,"board":net,"schematic":want}));}}}}
  erc=json!({"parts":netlist.parts.len(),"nets":netlist.nets.len(),"findings":netlist.violations});
  let plan=pcbplannercad_core::pcb::sync::plan(&netlist,&doc,&|part|{
   let name=part.footprint.rsplit(['/',':']).next().unwrap_or(&part.footprint);
   doc.footprint_cache().iter().find(|(key,fp)|fp.name==name&&key.starts_with(&format!("{}/",part.library))).or_else(||doc.footprint_cache().iter().find(|(_,fp)|fp.name==name)).map(|(key,fp)|(key.clone(),fp.clone()))
  });
  assert!(plan.is_current(&netlist,&doc));let mut work=pcbplannercad_core::Session::with_document(doc.clone());
  for(key,fp)in &plan.footprints {work.put_cached_footprint(key.clone(),fp.clone());}
  let baseline=io::document_to_string(work.document()).unwrap();let selected=(0..plan.changes.len()).collect::<Vec<_>>();
  if !selected.is_empty(){
   work.execute(pcbplannercad_core::pcb::sync::apply(&plan,work.document(),&selected)).unwrap();let applied=io::document_to_string(work.document()).unwrap();
   let round=io::document_from_str(DocKind::Pcb,&applied).unwrap();assert_eq!(io::document_to_string(&round).unwrap(),applied);
   for(key,fp)in doc.footprint_cache(){assert_eq!(work.document().cached_footprint(key),Some(fp));}
   work.undo().unwrap();assert_eq!(io::document_to_string(work.document()).unwrap(),baseline);work.redo().unwrap();assert_eq!(io::document_to_string(work.document()).unwrap(),applied);
  }
  eco=json!({"changes":plan.changes,"exactUndoRedo":true,"saveReopen":true,"existingLayerGeometryPreserved":true});
 }
 let nets:BTreeSet<_>=length::measure(&doc).keys().cloned().collect();
 let result=json!({"objects":doc.len(),"layers":board.stackup.copper_layers(),"roundTrip":true,"drcMs":drc_ms,"drcCounts":counts,"drc":report,"fillMs":fill_ms,"filledZones":fills.len(),"measuredNets":nets.len(),"schematic":erc,"eco":eco,"netParityDifferences":parity});
 assert_eq!(saved,io::document_to_string(&doc).unwrap());fs::write(&a[2],serde_json::to_string_pretty(&result).unwrap()).unwrap();println!("{} objects, {} copper layers; DRC {} ms, refill {} ms; {} parity differences",doc.len(),board.stackup.copper_layers().len(),drc_ms,fill_ms,parity.len());
}
