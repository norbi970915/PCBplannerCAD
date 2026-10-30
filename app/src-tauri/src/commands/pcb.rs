//! Board editing: analysis (ratsnest, DRC), pours, routing, schematic → board
//! change orders and footprint placement.

use std::collections::BTreeMap;

use pcbplannercad_core::pcb::{connectivity, drc, fill, geometry, router, sync};
use pcbplannercad_core::schematic::{self, SheetRef};
use pcbplannercad_core::{Command, Delta, DocKind, Footprint, IdItem, Item, ItemId, LayerId, Point, units::Coord};
use serde::Serialize;
use tauri::State;

use super::{Res, lock};
use crate::AppState;
use crate::workspace::{DocInfo, Workspace};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    airwires: Vec<connectivity::AirWire>,
    stats: connectivity::RoutingStats,
}

fn pcb_doc<'a>(ws: &'a Workspace, id: &str) -> Res<&'a pcbplannercad_core::Document> {
    let d = ws.doc(id)?;
    if d.kind != DocKind::Pcb {
        return Err("not a PCB document".into());
    }
    Ok(d.session.document())
}

#[tauri::command]
pub fn pcb_analysis(state: State<AppState>, id: String) -> Res<Analysis> {
    let ws = lock(&state);
    let doc = pcb_doc(&ws, &id)?;
    Ok(Analysis { airwires: connectivity::ratsnest(doc), stats: connectivity::stats(doc) })
}

#[tauri::command]
pub fn pcb_routing_defaults(settings: pcbplannercad_core::pcb::board::BoardSettings, net: String, layer: LayerId) -> pcbplannercad_core::pcb::setup::RoutingDefaults { pcbplannercad_core::pcb::setup::routing_defaults(&settings,&net,layer) }

#[tauri::command]
pub fn pcb_validate_setup(settings: pcbplannercad_core::pcb::board::BoardSettings) -> Vec<String> { pcbplannercad_core::pcb::setup::validate(&settings) }

// DRC runs off the main thread on a snapshot, so editing continues while a (online) check is busy.
#[tauri::command(async)]
pub fn pcb_drc(state: State<AppState>, id: String) -> Res<drc::DrcReport> {
    let doc = pcb_doc(&lock(&state), &id)?.clone();
    Ok(drc::run(&doc))
}

#[tauri::command(async)]
pub fn pcb_drc_near(state: State<AppState>, id: String, items: Vec<ItemId>) -> Res<Vec<drc::DrcViolation>> {
    let doc = pcb_doc(&lock(&state), &id)?.clone();
    Ok(drc::run_near(&doc, &items))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocUpdate {
    delta: Delta,
    info: DocInfo,
}

fn run(ws: &mut Workspace, id: &str, cmd: Command) -> Res<DocUpdate> {
    let delta = ws.doc_mut(id)?.session.execute(cmd).map_err(|e| e.to_string())?;
    Ok(DocUpdate { delta, info: ws.doc_info(id)? })
}

/// Approval changes are merged under the document lock, without replacing unrelated settings.
#[tauri::command]
pub fn pcb_drc_approval(state: State<AppState>, id: String, approval: pcbplannercad_core::pcb::board::DrcApproval, approve: bool) -> Res<DocUpdate> {
    let mut ws = lock(&state);
    let (bid, settings) = geometry::board_settings(pcb_doc(&ws, &id)?).ok_or("Board settings are missing; reopen this board before approving findings.")?;
    let mut settings = settings.clone();
    settings.set_drc_approval(approval, approve);
    run(&mut ws, &id, Command::Replace { id: bid, item: Item::Board(settings) })
}

/// Recomputes pour fills (one zone, or all) as a single undo step.
#[tauri::command(async)]
pub fn pcb_repour(state: State<AppState>, id: String, zone: Option<ItemId>) -> Res<DocUpdate> {
    let mut ws = lock(&state);
    let doc = pcb_doc(&ws, &id)?;
    let fills = match zone {
        Some(z) => vec![(z, fill::fill_zone(doc, z))],
        None => fill::fill_all(doc),
    };
    let items: Vec<IdItem> = fills
        .into_iter()
        .filter_map(|(zid, polys)| match doc.get(zid) {
            Some(Item::Zone(z)) => Some(IdItem { id: zid, item: Item::Zone(pcbplannercad_core::pcb::Zone { fill: polys, ..z.clone() }) }),
            _ => None,
        })
        .collect();
    if items.is_empty() {
        return Ok(DocUpdate { delta: Delta::default(), info: ws.doc_info(&id)? });
    }
    run(&mut ws, &id, Command::ReplaceMany { items })
}

#[tauri::command(async)]
pub fn pcb_route(state: State<AppState>, id: String, req: router::RouteRequest) -> Res<router::RouteResult> {
    let ws = lock(&state);
    Ok(router::route(pcb_doc(&ws, &id)?, &req))
}

#[tauri::command(async)]
pub fn pcb_route_pair(state: State<AppState>, id: String, req: router::RouteRequest, neg_net: String, gap: Coord, neg_start: Option<Point>) -> Res<(router::RouteResult, router::RouteResult)> {
    let ws = lock(&state);
    Ok(router::route_diff_pair_from(pcb_doc(&ws, &id)?, &req, &neg_net, gap, neg_start))
}

#[tauri::command]
pub fn pcb_length_constraint(state: State<AppState>, id: String, net: String) -> Res<Option<pcbplannercad_core::pcb::length::LengthConstraint>> {
    let ws = lock(&state);
    Ok(pcbplannercad_core::pcb::length::constraint(pcb_doc(&ws, &id)?, &net))
}

/// Read-only plan: applying recomputes against the latest board.
#[tauri::command(async)]
pub fn pcb_tune_preview(state: State<AppState>, id: String, req: router::TuneRequest) -> Res<Vec<Item>> {
    let doc = pcb_doc(&lock(&state), &id)?.clone();
    Ok(router::tune_length(&doc, &req)?.into_iter().map(Item::Track).collect())
}

/// Replaces a track chain with a length-tuned meander chain.
#[tauri::command]
pub fn pcb_tune(state: State<AppState>, id: String, req: router::TuneRequest, expected: Option<Vec<pcbplannercad_core::pcb::Track>>) -> Res<DocUpdate> {
    let mut ws = lock(&state);
    let command = router::tune_length_command(pcb_doc(&ws, &id)?, &req, expected.as_deref())?;
    run(&mut ws, &id, command)
}

#[tauri::command]
pub fn pcb_net_length(state: State<AppState>, id: String, net: String) -> Res<Coord> {
    let ws = lock(&state);
    Ok(router::net_length(pcb_doc(&ws, &id)?, &net))
}

#[tauri::command]
pub fn pcb_via(state: State<AppState>, id: String, net: String, pos: Point, from: Option<LayerId>, to: Option<LayerId>) -> Res<pcbplannercad_core::pcb::Via> {
    let ws = lock(&state);
    let doc = pcb_doc(&ws, &id)?;
    Ok(match (from, to) {
        (Some(a), Some(b)) => router::via_between(doc, &net, pos, a, b),
        _ => router::via_for(doc, &net, pos),
    })
}

#[tauri::command(async)]
pub fn pcb_drag_tracks(state: State<AppState>, id:String, req:pcbplannercad_core::pcb::elastic::TrackRequest)->Res<pcbplannercad_core::pcb::drag::DragResult>{
    let doc=pcb_doc(&lock(&state),&id)?.clone();
    pcbplannercad_core::pcb::elastic::drag_tracks(&doc,&req)
}

#[tauri::command(async)]
pub fn pcb_track_edit_plan(state: State<AppState>, id:String, replace:Vec<IdItem>, add:Vec<Item>, mode:router::RouteMode)->Res<pcbplannercad_core::pcb::drag::DragResult>{
    let doc=pcb_doc(&lock(&state),&id)?.clone();
    router::plan_track_edit(&doc,replace,add,mode)
}

#[tauri::command]
pub fn pcb_pair_via_clearance(state: State<AppState>, id: String, positive: pcbplannercad_core::pcb::Via, negative: pcbplannercad_core::pcb::Via, source: LayerId) -> Res<Coord> {
    let ws=lock(&state);
    router::pair_via_clearance(pcb_doc(&ws,&id)?,&positive,&negative,source)
}

#[tauri::command]
pub fn pcb_object_at(state: State<AppState>, id: String, pos: Point, layer: LayerId) -> Res<Option<(ItemId, String)>> {
    let ws = lock(&state);
    Ok(connectivity::object_at(pcb_doc(&ws, &id)?, pos, layer))
}

#[tauri::command]
pub fn pcb_net_items(state: State<AppState>, id: String, net: String) -> Res<Vec<ItemId>> {
    let ws = lock(&state);
    Ok(connectivity::net_items(pcb_doc(&ws, &id)?, &net))
}

/// Nets present on the board (pads, tracks, vias, zones), sorted.
#[tauri::command]
pub fn pcb_nets(state: State<AppState>, id: String) -> Res<Vec<String>> {
    let ws = lock(&state);
    let doc = pcb_doc(&ws, &id)?;
    let mut nets = std::collections::BTreeSet::new();
    for (_, item) in doc.items() {
        match item {
            Item::Footprint(f) => nets.extend(f.pad_nets.values().filter(|n| !n.is_empty()).cloned()),
            Item::Track(t) if !t.net.is_empty() => {
                nets.insert(t.net.clone());
            }
            Item::Via(v) if !v.net.is_empty() => {
                nets.insert(v.net.clone());
            }
            Item::Zone(z) if !z.net.is_empty() => {
                nets.insert(z.net.clone());
            }
            _ => {}
        }
    }
    Ok(nets.into_iter().collect())
}

/// Copies a library footprint into the board's cache; returns its key.
#[tauri::command]
pub fn pcb_cache_footprint(state: State<AppState>, id: String, library: String, reference: String) -> Res<(String, Footprint)> {
    let mut ws = lock(&state);
    let (lib_id, fp) = ws.find_footprint(&library, &reference).ok_or_else(|| format!("footprint {reference} not found in open libraries"))?;
    let key = format!("{}/{}", ws.lib(&lib_id)?.lib.name, fp.name);
    let d = ws.doc_mut(&id)?;
    if d.kind != DocKind::Pcb {
        return Err("footprints can only be placed on boards".into());
    }
    d.session.put_cached_footprint(key.clone(), fp.clone());
    Ok((key, fp))
}

// ---------- schematic → board ----------

/// Resolves a schematic part's footprint ("Lib:Name" or "Name") in open libraries.
/// Boards imported from structured format key footprints "Lib:Name" while PCBPlanner Studio uses
/// "Lib/Name"; an existing key of either form is reused so unchanged parts
/// don't show up as footprint swaps.
fn resolver<'a>(ws: &'a Workspace, board: &'a pcbplannercad_core::Document) -> impl Fn(&schematic::NetlistPart) -> Option<(String, Footprint)> + 'a {
    move |p| {
        let reference = p.footprint.trim();
        if reference.is_empty() {
            return None;
        }
        // Prefer the library the component came from.
        let preferred = ws.libs.iter().find(|(_, l)| l.lib.name == p.library || (p.library == "pCAD Basic" && l.readonly && l.lib.name == pcbplannercad_core::libgen::starter::STARTER_LIBRARY_NAME)).map(|(id, _)| id.clone()).unwrap_or_default();
        let (lib_id, fp) = ws.find_footprint(&preferred, reference)?;
        let lib_name = ws.libs.get(&lib_id).map(|l| l.lib.name.clone()).unwrap_or_default();
        let candidates = [format!("{lib_name}/{}", fp.name), format!("{lib_name}:{}", fp.name), reference.to_string()];
        let key = candidates.iter().find(|k| board.cached_footprint(k).is_some()).cloned().unwrap_or_else(|| candidates[0].clone());
        Some((key, fp))
    }
}

fn compile_project(ws: &Workspace) -> Res<schematic::Compiled> {
    let sheets = ws.project_sheet_docs()?;
    let refs: Vec<SheetRef> = sheets.iter().map(|(rel, id)| Ok(SheetRef { path: rel.clone(), doc: ws.doc(id)?.session.document() })).collect::<Res<_>>()?;
    Ok(schematic::compile(&refs))
}

#[tauri::command]
pub fn pcb_eco_plan(state: State<AppState>, id: String) -> Res<Vec<sync::EcoChange>> {
    let mut ws = lock(&state);
    let compiled = compile_project(&ws).map_err(|e| format!("Update PCB needs an open project with schematics: {e}"))?;
    let plan = {
        let doc = pcb_doc(&ws, &id)?;
        sync::plan(&compiled, doc, &resolver(&ws, doc))
    };
    let changes = plan.changes.clone();
    ws.eco_plans.insert(id, plan);
    Ok(changes)
}

#[tauri::command]
pub fn pcb_eco_apply(state: State<AppState>, id: String, selected: Vec<usize>) -> Res<DocUpdate> {
    let mut ws = lock(&state);
    let plan = ws.eco_plans.remove(&id).ok_or("run Update PCB again — the change list is out of date")?;
    let compiled = compile_project(&ws).map_err(|e| format!("Cannot recheck the schematic: {e}"))?;
    let current = { let doc = pcb_doc(&ws, &id)?; sync::plan(&compiled, doc, &resolver(&ws, doc)) };
    if !plan.is_current(&compiled, pcb_doc(&ws, &id)?) || plan.changes != current.changes || plan.footprints != current.footprints {
        return Err("Board, schematic or footprint library changed after review; run Update PCB again".into());
    }
    let cmd = sync::apply(&plan, pcb_doc(&ws, &id)?, &selected);
    let d = ws.doc_mut(&id)?;
    for (key, fp) in plan.footprints.iter().cloned() {
        d.session.put_cached_footprint(key, fp);
    }
    run(&mut ws, &id, cmd)
}

/// Footprint cache of a board (for drawing after ECO/import).
#[tauri::command]
pub fn pcb_footprints(state: State<AppState>, id: String) -> Res<BTreeMap<String, Footprint>> {
    let ws = lock(&state);
    Ok(pcb_doc(&ws, &id)?.footprint_cache().clone())
}

/// Board settings item id (stack, rules, net classes), created if missing.
#[tauri::command]
pub fn pcb_board_settings(state: State<AppState>, id: String) -> Res<(ItemId, pcbplannercad_core::pcb::BoardSettings)> {
    let mut ws = lock(&state);
    if let Some((bid, b)) = geometry::board_settings(pcb_doc(&ws, &id)?) {
        return Ok((bid, b.clone()));
    }
    let delta = ws.doc_mut(&id)?.session.execute(Command::Add { items: vec![Item::Board(Default::default())] }).map_err(|e| e.to_string())?;
    let bid = delta.upserted[0].id;
    Ok((bid, Default::default()))
}

/// Builds a panel from a board as a new, untitled board document.
#[tauri::command]
pub fn pcb_panelize(state: State<AppState>, id: String, spec: pcbplannercad_core::pcb::panel::PanelSpec) -> Res<super::docs::Snapshot> {
    let mut ws = lock(&state);
    let panel = pcbplannercad_core::pcb::panel::panelize(pcb_doc(&ws, &id)?, &spec)?;
    let new_id = ws.add_doc(DocKind::Pcb, crate::workspace::Origin::File(None), panel);
    ws.doc_mut(&new_id)?.session.mark_dirty();
    super::docs::snapshot(&ws, &new_id)
}

/// Plans dragging footprints with their routing kept connected; the frontend commits it as a `route` command.
#[tauri::command(async)]
pub fn pcb_drag_footprints(state: State<AppState>, id: String, req: pcbplannercad_core::pcb::drag::DragRequest) -> Res<pcbplannercad_core::pcb::drag::DragResult> {
    let doc = pcb_doc(&lock(&state), &id)?.clone();
    Ok(pcbplannercad_core::pcb::drag::drag_footprints(&doc, &req))
}

/// Plans Cleanup Tracks & Vias; the frontend commits it as a `route` command.
#[tauri::command(async)]
pub fn pcb_cleanup(state: State<AppState>, id: String, opts: pcbplannercad_core::pcb::cleanup::CleanupOptions) -> Res<pcbplannercad_core::pcb::cleanup::CleanupResult> {
    let ws = lock(&state);
    Ok(pcbplannercad_core::pcb::cleanup::cleanup(pcb_doc(&ws, &id)?, &opts))
}

/// Plans stitching vias for a filled pour; the frontend commits them as a `route` command.
#[tauri::command]
pub fn pcb_stitch(state: State<AppState>, id: String, req: pcbplannercad_core::pcb::stitch::StitchRequest) -> Res<pcbplannercad_core::pcb::stitch::StitchResult> {
    let ws = lock(&state);
    pcbplannercad_core::pcb::stitch::stitch(pcb_doc(&ws, &id)?, &req)
}

/// Plans regenerating every teardrop; the frontend commits it as a `route` command.
#[tauri::command(async)]
pub fn pcb_teardrops(state: State<AppState>, id: String, opts: pcbplannercad_core::pcb::teardrop::TeardropOptions) -> Res<pcbplannercad_core::pcb::teardrop::TeardropPlan> {
    let ws = lock(&state);
    Ok(pcbplannercad_core::pcb::teardrop::plan(pcb_doc(&ws, &id)?, &opts))
}

/// Plans moving several footprints, each by its own offset, with their routing kept connected (Pack & Move).
#[tauri::command(async)]
pub fn pcb_drag_each(state: State<AppState>, id: String, req: pcbplannercad_core::pcb::drag::MultiDragRequest) -> Res<pcbplannercad_core::pcb::drag::DragResult> {
    let ws = lock(&state);
    Ok(pcbplannercad_core::pcb::drag::drag_each(pcb_doc(&ws, &id)?, &req))
}
