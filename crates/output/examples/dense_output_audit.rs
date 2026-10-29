//! Exports and reopens manufacturing data from a disposable native board.
use std::{env,fs,path::PathBuf,time::Instant};
use pcbplannercad_core::{io,DocKind,Item,item::PadKind,pcb::geometry::{all_pads,board_settings}};
use pcbplannercad_output::{fab::{self,FabOptions},assembly::{PnpFormat,BomFormat},gerber_read,ipc2581};
use serde_json::json;
fn main() {
 let args:Vec<_>=env::args().collect();
 let doc=io::document_from_str(DocKind::Pcb,&fs::read_to_string(&args[1]).unwrap()).unwrap();
 let before=io::document_to_string(&doc).unwrap();let out=PathBuf::from(&args[2]);let now=Instant::now();
 let options=FabOptions{gerber:pcbplannercad_output::gerber::GerberOptions{project_name:"dense-board".into(),..Default::default()},pnp:Some(PnpFormat::Generic),bom:Some(BomFormat::Generic),zip:true};
 let paths=fab::write_package(&doc,&out,&options).unwrap();
 let mut files=vec![];let mut copper=0;let mut drill_hits=0;
 for path in &paths {
  let name=path.file_name().unwrap().to_string_lossy();let ext=path.extension().unwrap_or_default().to_string_lossy();
  if matches!(ext.as_ref(),"zip"|"csv"|"txt"){continue;}
  let text=fs::read_to_string(path).unwrap();let parsed=if ext=="drl" {drill_hits+=text.lines().filter(|l|l.starts_with('X')).count();gerber_read::parse_excellon(&name,&text)}else{if text.contains("TF.FileFunction,Copper,"){copper+=1;}gerber_read::parse_gerber(&name,&text)}.unwrap();
  assert!(parsed.warnings.is_empty(),"{name}: {:?}",parsed.warnings);files.push(json!({"name":name,"bytes":text.len(),"warnings":parsed.warnings}));
 }
 let board=board_settings(&doc).unwrap().1;assert_eq!(copper,board.stackup.copper_layers().len());
 let physical=all_pads(&doc,board).into_iter().filter(|p|p.kind!=PadKind::Smd&&p.drill.is_some()).count()+doc.items().filter(|(_,i)|matches!(i,Item::Via(_))).count();assert_eq!(drill_hits,physical);
 let reopened=gerber_read::read_dir_or_zip(&out.join("dense-board_fab.zip")).unwrap();assert_eq!(reopened.len(),files.len());assert!(reopened.iter().all(|f|f.warnings.is_empty()));
 let xml=ipc2581::write_ipc2581(&doc,"dense-board").unwrap();fs::write(out.join("dense-board.xml"),&xml).unwrap();
 assert_eq!(before,io::document_to_string(&doc).unwrap());
 fs::write(out.join("audit.json"),serde_json::to_string_pretty(&json!({"copperLayers":copper,"physicalDrills":physical,"drillHits":drill_hits,"zipReopenedFiles":reopened.len(),"files":files,"xmlBytes":xml.len(),"ms":now.elapsed().as_millis()})).unwrap()).unwrap();
 println!("{} copper layers, {} drill hits, {} CAM files reopened",copper,drill_hits,reopened.len());
}
