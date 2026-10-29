//! Import a complete project to a new local folder and record board measurements.
use std::{env, fs, path::Path, collections::BTreeMap};
use pcbplannercad_core::{io, pcb::{drc, length, geometry::board_settings}};
fn main() {
    let args:Vec<_>=env::args().collect();
    assert_eq!(args.len(),3,"Usage: project_audit <source-project> <new-output-folder>");
    let dest=Path::new(&args[2]);
    assert!(!dest.exists(),"Choose a new output folder; existing designs are never overwritten");
    let mut imported=pcbplannercad_import::structured::import_project(Path::new(&args[1])).expect("import project");
    let mut writes=vec![];
    for sheet in &imported.schematic.sheets {writes.push((sheet.file_name.clone(),io::document_to_string(&sheet.document).unwrap()));}
    let mut libs=imported.schematic.libraries.clone();
    if let Some(b)=&imported.board {for lib in &b.libraries {match libs.iter_mut().find(|l|l.name==lib.name){Some(l)=>l.merge(lib.clone()),None=>libs.push(lib.clone())}}}
    for (i,lib) in libs.iter().enumerate(){let name=format!("libraries/imported-{i}.arlib");writes.push((name.clone(),io::library_to_string(lib)));imported.project.libraries.push(name);}
    let compiled=pcbplannercad_core::schematic::compile(&imported.schematic.sheets.iter().map(|s|pcbplannercad_core::schematic::SheetRef::new(&s.file_name,&s.document)).collect::<Vec<_>>());
    let reopened:Vec<_>=imported.schematic.sheets.iter().map(|s|(s.file_name.clone(),io::document_from_str(pcbplannercad_core::DocKind::Schematic,&io::document_to_string(&s.document).unwrap()).unwrap())).collect();
    let reopened_compiled=pcbplannercad_core::schematic::compile(&reopened.iter().map(|(name,doc)|pcbplannercad_core::schematic::SheetRef::new(name,doc)).collect::<Vec<_>>());
    let reopened_matches=serde_json::to_value(&compiled).unwrap()==serde_json::to_value(reopened_compiled).unwrap();
    assert!(reopened_matches,"saved schematic netlist differs after reopening");
    writes.push(("schematic-erc.json".into(),serde_json::to_string_pretty(&compiled).unwrap()));
    println!("Schematic: {} parts, {} nets, {} findings",compiled.parts.len(),compiled.nets.len(),compiled.violations.len());
    let mut notes=String::new();
    for m in &imported.schematic.report.messages {notes.push_str(&format!("{m:?}\n"));}
    if let Some(b)=&imported.board {
        writes.push((b.file_name.clone(),io::document_to_string(&b.document).unwrap()));
        for m in &b.report.messages {notes.push_str(&format!("{m:?}\n"));}
        let eco=pcbplannercad_core::pcb::sync::plan(&compiled,&b.document,&|part|b.document.cached_footprint(&part.footprint).cloned().map(|f|(part.footprint.clone(),f)));
        writes.push(("schematic-board-plan.json".into(),serde_json::to_string_pretty(&eco).unwrap()));
        let mut expected = BTreeMap::<(String,String),String>::new();
        for net in &compiled.nets { for node in &net.nodes { for pad in &node.pads {
            expected.insert((node.designator.clone(),pad.clone()),net.name.clone());
        }}}
        let mut actual = BTreeMap::<String,Vec<(String,String)>>::new();
        let mut board_parts = std::collections::BTreeSet::new();
        let mut independent = vec![];
        for (_,item) in b.document.items() { if let pcbplannercad_core::Item::Footprint(f)=item {
            board_parts.insert(f.designator.clone());
            if f.board_only { independent.push(f.designator.clone()); }
            for (pad,net) in &f.pad_nets { if !net.is_empty() { actual.entry(net.clone()).or_default().push((f.designator.clone(),pad.clone())); } }
        }}
        let mut parity = vec![];
        for (net,pads) in &actual {
            let mut targets = BTreeMap::<String,Vec<&(String,String)>>::new();
            let mut absent = vec![];
            for pad in pads { match expected.get(pad) { Some(want)=>targets.entry(want.clone()).or_default().push(pad),None=>absent.push(pad) } }
            let state = if targets.len()>1 {"split"} else if !absent.is_empty() {"missingSchematicPads"}
                else if targets.keys().next().is_some_and(|want|want!=net) {"differentName"} else {"matching"};
            if state!="matching" { parity.push(serde_json::json!({"boardNet":net,"state":state,"schematicTargets":targets,"padsMissingFromSchematic":absent})); }
        }
        let source_only:Vec<_>=compiled.parts.iter().filter(|p|!p.designator.starts_with('#')&&!p.excluded_from_board()&&!board_parts.contains(&p.designator)).map(|p|&p.designator).collect();
        let recompiled=pcbplannercad_core::schematic::compile(&imported.schematic.sheets.iter().map(|s|pcbplannercad_core::schematic::SheetRef::new(&s.file_name,&s.document)).collect::<Vec<_>>());
        writes.push(("schematic-board-parity.json".into(),serde_json::to_string_pretty(&serde_json::json!({"updatesApplied":false,"reopenedNetlistMatches":reopened_matches,"reviewMatchesRecompile":eco.is_current(&recompiled,&b.document),"boardOnlyFootprints":independent,"sourceOnlyParts":source_only,"excludedFromBoard":compiled.parts.iter().filter(|p|p.excluded_from_board()).map(|p|&p.designator).collect::<Vec<_>>(),"netDifferences":parity})).unwrap()));
        let mut probes=vec![];
        let settings=board_settings(&b.document).unwrap().1;
        let mut tested=std::collections::HashSet::new();
        for (id,item) in b.document.items() {
            let pcbplannercad_core::Item::Track(t)=item else{continue};
            if tested.len()>=8{break;}
            if !settings.diff_pairs.iter().any(|p|p.positive==t.net)||((t.b.x-t.a.x) as f64).hypot((t.b.y-t.a.y) as f64)<2e6||tested.contains(&t.net){continue;}
            tested.insert(t.net.clone());
            let req=pcbplannercad_core::pcb::router::RouteRequest{net:t.net.clone(),layer:t.layer,start:t.a,end:t.b,width:t.width,mode:pcbplannercad_core::pcb::router::RouteMode::Walkaround,corner:pcbplannercad_core::pcb::router::CornerStyle::Diagonal45,ignore:vec![id],posture_flip:false};
            let start=std::time::Instant::now();
            let routed=pcbplannercad_core::pcb::router::route(&b.document,&req);
            let route_ms=start.elapsed().as_secs_f64()*1000.0;
            let pair=settings.diff_pairs.iter().find(|p|p.positive==t.net).unwrap();
            let gap=settings.net_classes.iter().find(|c|c.nets.contains(&t.net)).unwrap_or(&settings.net_classes[0]).diff_pair_gap;
            let (positive,negative)=pcbplannercad_core::pcb::router::route_diff_pair(&b.document,&req,&pair.negative,gap);
            let chain_length=((t.b.x-t.a.x) as f64).hypot((t.b.y-t.a.y) as f64);
            let tune=pcbplannercad_core::pcb::router::tune_length(&b.document,&pcbplannercad_core::pcb::router::TuneRequest{tracks:vec![id],target_length:(chain_length*1.05).round() as i64,amplitude:200000,spacing:300000,style:pcbplannercad_core::pcb::router::TuneStyle::Accordion});
            let tuned=tune.map(|ts|ts.iter().map(|t|((t.b.x-t.a.x)as f64).hypot((t.b.y-t.a.y)as f64)).sum::<f64>());
            probes.push(serde_json::json!({"net":t.net,"track":id,"routeReached":routed.reached,"routeMessage":routed.message,"routeMs":route_ms,"pairPositiveReached":positive.reached,"pairNegativeReached":negative.reached,"pairMessage":negative.message,"tunedLengthNm":tuned}));
        }
        writes.push(("routing-tuning-probes.json".into(),serde_json::to_string_pretty(&probes).unwrap()));
        let report=drc::run(&b.document);
        writes.push(("board-drc.json".into(),serde_json::to_string_pretty(&report).unwrap()));
        let totals: BTreeMap<_,_>=length::measure(&b.document).into_iter().map(|(n,m)|(n,m.total.round() as i64)).collect();
        writes.push(("net-lengths-nm.json".into(),serde_json::to_string_pretty(&totals).unwrap()));
        if let Some((_,board))=board_settings(&b.document){println!("Board: {} items, {} copper layers, {} net classes, {} pairs, {} rules, {} measured nets, {} DRC findings",b.document.len(),board.stackup.copper_layers().len(),board.net_classes.len(),board.diff_pairs.len(),board.rules.len(),totals.len(),report.violations.len());}
    }
    writes.push(("import-notes.txt".into(),notes));
    writes.push((format!("{}.arproj",imported.project.name),io::project_to_string(&imported.project)));
    fs::create_dir_all(dest.join("libraries")).unwrap();
    for (name,text) in writes {let path=dest.join(name);if let Some(parent)=path.parent(){fs::create_dir_all(parent).unwrap();}fs::write(path,text).unwrap();}
    println!("Imported {} schematic sheets and {} editable libraries to {}",imported.schematic.sheets.len(),libs.len(),dest.display());
}
