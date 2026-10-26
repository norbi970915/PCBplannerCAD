//! IPC surface between the UI and the workspace. Every command locks the
//! workspace once, does its work and returns plain serializable data.

pub mod docs;
pub mod imports;
pub mod libs;
pub mod outputs;
pub mod pcb;
pub mod project;

use std::sync::MutexGuard;

use tauri::State;

use crate::AppState;
use crate::workspace::Workspace;

pub type Res<T> = Result<T, String>;

#[track_caller]
pub fn lock<'a>(state: &'a State<AppState>) -> MutexGuard<'a, Workspace> {
    // TEMP timing
    let t = std::time::Instant::now();
    // Commands validate before mutating, so a poisoned lock is safe to reuse.
    let g = state.workspace.lock().unwrap_or_else(|e| e.into_inner());
    let w = t.elapsed();
    eprintln!("TIMING lock at {} waited {:?}", std::panic::Location::caller(), w);
    g
}
