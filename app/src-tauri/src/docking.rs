use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{webview::NewWindowResponse, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn allowed_popup(url: &tauri::Url, main: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "tauri")
        && url.scheme() == main.scheme()
        && url.host_str() == main.host_str()
        && url.port_or_known_default() == main.port_or_known_default()
        && url.path() == "/popout.html"
        && url.query().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}
pub fn create_main(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();
    WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
        .on_new_window(move |url, features| {
            let Some(main) = handle.get_webview_window("main") else {
                return NewWindowResponse::Deny;
            };
            let Ok(source) = main.url() else {
                return NewWindowResponse::Deny;
            };
            if !allowed_popup(&url, &source) {
                return NewWindowResponse::Deny;
            }
            let label = format!("dock-{}", NEXT.fetch_add(1, Ordering::Relaxed));
            let script = format!(
                "window.__PCAD_DOCK_LABEL__={};",
                serde_json::to_string(&label).unwrap()
            );
            let close_handle = handle.clone();
            let close_label = label.clone();
            let builder = WebviewWindowBuilder::new(
                &handle,
                &label,
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .window_features(features)
            .disable_drag_drop_handler()
            .min_inner_size(260., 180.)
            .initialization_script(script)
            .title("pCAD · Detached view")
            .on_navigation(move |url| url.as_str() == "about:blank" || allowed_popup(url, &source));
            match builder.build() {
                Ok(window) => {
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = close_handle.emit_to("main", "pcad-dock-close", &close_label);
                        }
                    });
                    NewWindowResponse::Create { window }
                }
                Err(_) => NewWindowResponse::Deny,
            }
        })
        .build()?
        .on_window_event({
            let handle = app.handle().clone();
            move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    for (label, window) in handle.webview_windows() {
                        if valid_label(&label) {
                            let _ = window.destroy();
                        }
                    }
                }
            }
        });
    Ok(())
}
fn valid_label(label: &str) -> bool {
    label
        .strip_prefix("dock-")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}
#[tauri::command]
pub fn dock_close_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if !valid_label(&label) {
        return Err("Only detached view windows can be closed".into());
    }
    if let Some(window) = app.get_webview_window(&label) {
        window.destroy().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn dock_focus_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if !valid_label(&label) {
        return Err("Only detached view windows can be focused".into());
    }
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detached_windows_are_restricted_to_the_same_origin_blank_view() {
        for main in [
            "http://localhost:1420/",
            "http://tauri.localhost/",
            "tauri://localhost/",
        ] {
            let main: tauri::Url = main.parse().unwrap();
            assert!(allowed_popup(&main.join("/popout.html").unwrap(), &main));
            for url in [
                "https://outside.example/popout.html",
                "javascript:alert(1)",
                "data:text/html,x",
                "file:///popout.html",
                "http://localhost:9999/popout.html",
            ] {
                assert!(!allowed_popup(&url.parse().unwrap(), &main));
            }
            assert!(!allowed_popup(&main.join("/index.html").unwrap(), &main));
            assert!(!allowed_popup(
                &main.join("/popout.html?target=remote").unwrap(),
                &main
            ));
        }
    }
    #[test]
    fn native_window_commands_cannot_target_the_main_editor() {
        for bad in ["main", "dock-", "dock-../main", "dock-other", "other-1"] {
            assert!(!valid_label(bad));
        }
        assert!(valid_label("dock-17"));
    }
}
