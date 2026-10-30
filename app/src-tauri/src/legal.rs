use std::path::PathBuf;
#[cfg(debug_assertions)]
use std::path::Path;
use serde::Serialize;
use tauri::Manager;

fn legal_folder(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let bundled = app.path().resource_dir().map_err(|e| e.to_string())?.join("legal");
    if bundled.join("generated/third-party-notices.txt").is_file() { return Ok(bundled); }
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE");
        if dev.join("generated/third-party-notices.txt").is_file() { return Ok(dev); }
    }
    Err("Legal files are missing. Rebuild this application with its complete release resources.".into())
}

#[derive(Serialize)]
pub struct LegalInfo { legal: String, data: String, migration: Vec<String> }

#[tauri::command]
pub fn legal_info(app: tauri::AppHandle) -> Result<LegalInfo, String> {
    Ok(LegalInfo {
        legal: legal_folder(&app)?.to_string_lossy().into_owned(),
        data: app.path().app_local_data_dir().map_err(|e| e.to_string())?.to_string_lossy().into_owned(),
        migration: app.state::<crate::identity::MigrationStatus>().0.clone(),
    })
}

#[tauri::command]
pub fn open_legal_folder(app: tauri::AppHandle) -> Result<(), String> {
    let folder = legal_folder(&app)?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer.exe").arg(folder).spawn().map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "windows"))]
    { let _ = folder; return Err("Open the legal-files path shown in About using your file manager.".into()); }
    Ok(())
}
