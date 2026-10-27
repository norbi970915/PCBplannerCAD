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
    // Commands validate before mutating, so a poisoned lock is safe to reuse.
    state.workspace.lock().unwrap_or_else(|e| e.into_inner())
}
