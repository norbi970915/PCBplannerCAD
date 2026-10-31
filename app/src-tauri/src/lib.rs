mod commands;
mod identity;
mod legal;
mod workspace;
mod local;
mod recovery;
mod docking;
mod routing;
use tauri::Manager;

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
        .setup(|app| { let migration=identity::migrate(app.handle()); app.manage(identity::MigrationStatus(migration)); app.manage(recovery::RecoveryState::new(app.path().app_local_data_dir()?.join("recovery")));docking::create_main(app)?;Ok(())})
        .manage(AppState { workspace: Mutex::new(ws) })
        .manage(routing::RoutingState::default())
        .invoke_handler(tauri::generate_handler![
            legal::legal_info,
            legal::open_legal_folder,
            docking::dock_close_window,
            docking::dock_focus_window,
            recovery::recovery_capture,
            recovery::recovery_list,
            recovery::recovery_restore,
            docs::doc_list,
            docs::doc_snapshot,
            docs::doc_new,
            docs::doc_open,
            docs::doc_execute,
            docs::doc_undo,
            docs::doc_redo,
            docs::doc_save,
            docs::doc_save_copy,
            docs::doc_close,
            docs::doc_cache_component,
            docs::doc_put_cache,
            libs::lib_list,
            libs::lib_set_notices,
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
            local::hub_browse,
            local::hub_preview,
            local::hub_create_folder,
            local::hub_create_project,
            local::project_checkpoint,
            local::project_revisions,
            local::project_compare_revisions,
            local::project_restore_copy,
            local::import_preview,
            local::import_discard,
            local::import_apply,
            imports::import_library,
            imports::import_design,
            imports::library_paths,
            imports::list_library_files,
            project::project_new_pcb,
            pcb::pcb_analysis,
            pcb::pcb_routing_defaults,
            pcb::pcb_validate_setup,
            pcb::pcb_drc,
            pcb::pcb_drc_approval,
            pcb::pcb_drc_near,
            pcb::pcb_repour,
            pcb::pcb_route,
            pcb::pcb_route_pair,
            pcb::pcb_track_edit_plan,
            pcb::pcb_drag_tracks,
            pcb::pcb_tune,
            pcb::pcb_tune_preview,
            pcb::pcb_length_constraint,
            pcb::pcb_net_length,
            pcb::pcb_via,
            pcb::pcb_pair_via_clearance,
            pcb::pcb_pair_via_plan,
            pcb::pcb_drag_footprints,
            pcb::pcb_drag_each,
            pcb::pcb_cleanup,
            pcb::pcb_cleanup_apply,
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
        .expect("error while running PCBPlanner Studio");
}
