mod commands;
mod workspace;

use std::sync::Mutex;

use commands::{docs, imports, libs, outputs, pcb, project};
use workspace::Workspace;

pub struct AppState {
    pub workspace: Mutex<Workspace>,
}

pub fn run() {
    let mut ws = Workspace::default();
    libs::ensure_builtin(&mut ws);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { workspace: Mutex::new(ws) })
        .invoke_handler(tauri::generate_handler![
            docs::doc_list,
            docs::doc_snapshot,
            docs::doc_new,
            docs::doc_open,
            docs::doc_execute,
            docs::doc_undo,
            docs::doc_redo,
            docs::doc_save,
            docs::doc_close,
            docs::doc_cache_component,
            docs::doc_put_cache,
            libs::lib_list,
            libs::lib_get,
            libs::lib_new,
            libs::lib_open,
            libs::lib_save,
            libs::lib_close,
            libs::lib_component_index,
            libs::lib_cached_part,
            libs::lib_find_footprint,
            libs::lib_new_symbol,
            libs::lib_new_footprint,
            libs::lib_put_component,
            libs::lib_update_symbol,
            libs::lib_update_footprint,
            libs::lib_delete_entry,
            libs::lib_duplicate_entry,
            libs::lib_copy_components,
            libs::lib_open_symbol,
            libs::lib_add_symbol_unit,
            libs::lib_remove_symbol_unit,
            libs::lib_open_footprint,
            libs::wizard_families,
            libs::wizard_default_name,
            libs::wizard_preview,
            libs::wizard_generate,
            libs::symbol_wizard,
            project::project_info,
            project::project_open,
            project::project_new,
            project::project_close,
            project::project_add_sheet,
            project::project_remove_sheet,
            project::project_add_library,
            project::project_remove_library,
            project::project_save_all,
            project::project_compile,
            project::project_annotate,
            project::project_export_bom,
            project::sheet_connected_items,
            project::project_sheet_doc,
            imports::import_library,
            imports::import_design,
            imports::library_paths,
            imports::list_library_files,
            project::project_new_pcb,
            pcb::pcb_analysis,
            pcb::pcb_drc,
            pcb::pcb_drc_near,
            pcb::pcb_repour,
            pcb::pcb_route,
            pcb::pcb_route_pair,
            pcb::pcb_tune,
            pcb::pcb_net_length,
            pcb::pcb_via,
            pcb::pcb_drag_footprints,
            pcb::pcb_drag_each,
            pcb::pcb_cleanup,
            pcb::pcb_stitch,
            pcb::pcb_teardrops,
            pcb::pcb_object_at,
            pcb::pcb_net_items,
            pcb::pcb_nets,
            pcb::pcb_cache_footprint,
            pcb::pcb_eco_plan,
            pcb::pcb_eco_apply,
            pcb::pcb_footprints,
            pcb::pcb_board_settings,
            pcb::pcb_panelize,
            project::project_set_variants,
            outputs::out_fab,
            outputs::out_pnp,
            outputs::out_board_bom,
            outputs::out_ipc2581,
            outputs::out_board_model,
            outputs::out_step,
            outputs::out_read_gerbers,
            outputs::write_file_bytes,
            outputs::read_file_bytes,
            outputs::resolve_model_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PCBPlannerCAD");
}
