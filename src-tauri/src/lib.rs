pub mod engine;

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use engine::error::EngineError;
use engine::model::{AddRequest, Download, EngineEvent, ProbeInfo, Settings, Snapshot};
use engine::{EventSink, Manager};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager as _, RunEvent, Runtime, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "main";

struct TauriSink {
    app: AppHandle,
    last_title: Mutex<Option<String>>,
}

impl EventSink for TauriSink {
    fn emit(&self, event: EngineEvent) {
        let _ = self.app.emit("engine", event);
    }

    fn completed(&self, d: &Download) {
        let _ = self
            .app
            .notification()
            .builder()
            .title("Download complete")
            .body(&d.file_name)
            .show();
    }

    fn activity(&self, active: usize, total_speed: u64) {
        let title = (active > 0).then(|| format_speed(total_speed));
        let mut last = self.last_title.lock().unwrap();
        if *last == title {
            return;
        }
        if let Some(tray) = self.app.tray_by_id(TRAY_ID) {
            let tooltip = match &title {
                Some(speed) => format!("Downpour: {active} active, {speed}"),
                None => "Downpour".to_string(),
            };
            let _ = tray.set_tooltip(Some(&tooltip));
            #[cfg(target_os = "macos")]
            let _ = tray.set_title(title.as_deref());
        }
        *last = title;
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut v = bytes_per_sec as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

type Mgr<'a> = State<'a, Manager>;

#[tauri::command]
fn get_snapshot(m: Mgr) -> Snapshot {
    m.snapshot()
}

#[tauri::command]
async fn probe_url(
    m: Mgr<'_>,
    url: String,
    referer: Option<String>,
) -> Result<ProbeInfo, EngineError> {
    m.probe(&url, referer.as_deref()).await
}

#[tauri::command]
fn add_downloads(m: Mgr, requests: Vec<AddRequest>) -> Result<Vec<Download>, EngineError> {
    requests.into_iter().map(|r| m.add(r)).collect()
}

#[tauri::command]
fn pause_downloads(m: Mgr, ids: Vec<String>) {
    m.pause(&ids);
}

#[tauri::command]
fn resume_downloads(m: Mgr, ids: Vec<String>) {
    m.resume(&ids);
}

#[tauri::command]
fn remove_downloads(m: Mgr, ids: Vec<String>, delete_files: bool) {
    m.remove(&ids, delete_files);
}

#[tauri::command]
fn reorder_download(m: Mgr, id: String, before_id: Option<String>) {
    m.reorder(&id, before_id.as_deref());
}

#[tauri::command]
fn update_settings(m: Mgr, settings: Settings) -> Result<Settings, EngineError> {
    m.update_settings(settings)
}

#[tauri::command]
fn open_download<R: Runtime>(app: AppHandle<R>, m: Mgr, id: String) -> Result<(), String> {
    let path = m.path_of(&id).ok_or("download not found")?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_download<R: Runtime>(app: AppHandle<R>, m: Mgr, id: String) -> Result<(), String> {
    let path = m.path_of(&id).ok_or("download not found")?;
    let target = if path.exists() {
        path
    } else {
        path.parent().map(|p| p.to_path_buf()).ok_or("no folder")?
    };
    app.opener()
        .reveal_item_in_dir(target)
        .map_err(|e| e.to_string())
}

/// Every command the frontend can call; shared with the IPC tests.
fn commands<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_snapshot,
        probe_url,
        add_downloads,
        pause_downloads,
        resume_downloads,
        remove_downloads,
        reorder_download,
        update_settings,
        open_download,
        reveal_download,
    ]
}

/// Generated once: the macro embeds the macOS Info.plist and may only expand once per crate.
fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Downpour", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume_all", "Resume all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Downpour", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &pause, &resume, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Downpour")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "pause_all" => app.state::<Manager>().pause_all(),
            "resume_all" => app.state::<Manager>().resume_all(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            let download_dir = app
                .path()
                .download_dir()
                .or_else(|_| app.path().home_dir().map(|h| h.join("Downloads")))?;
            let sink = Arc::new(TauriSink {
                app: handle.clone(),
                last_title: Mutex::new(None),
            });
            let manager = tauri::async_runtime::block_on(async move {
                Manager::new(data_dir, download_dir, sink)
            });
            manager.start();
            app.manage(manager);
            build_tray(&handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window
                    .app_handle()
                    .state::<Manager>()
                    .settings()
                    .close_to_tray
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(commands())
        .build(context())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        RunEvent::Exit => app.state::<Manager>().shutdown(Duration::from_millis(1500)),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::format_speed;

    #[test]
    fn speed_formatting() {
        assert_eq!(format_speed(512), "512 B/s");
        assert_eq!(format_speed(1536), "1.5 KB/s");
        assert_eq!(format_speed(5 * 1024 * 1024), "5.0 MB/s");
    }
}

/// Calls each command through Tauri's IPC layer with the exact JSON shapes
/// `src/api.ts` sends, so a renamed field on either side fails here.
#[cfg(test)]
mod ipc_tests {
    use super::*;
    use serde_json::{Value, json};
    use std::path::Path;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
    use tauri::webview::InvokeRequest;
    use tauri::{App, WebviewWindow, WebviewWindowBuilder};

    struct Quiet;
    impl EventSink for Quiet {
        fn emit(&self, _event: EngineEvent) {}
    }

    fn app(root: &Path) -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
        let app = mock_builder()
            .invoke_handler(commands())
            .build(context())
            .unwrap();
        let (data, downloads) = (root.join("data"), root.join("downloads"));
        let manager = tauri::async_runtime::block_on(async move {
            Manager::new(data, downloads, Arc::new(Quiet))
        });
        app.manage(manager);
        let window = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }

    fn call(w: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(
            w,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                // The app's own origin; the ACL rejects commands from anywhere else.
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<Value>().unwrap())
    }

    fn keys(v: &Value) -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    fn sorted(list: &[&str]) -> Vec<String> {
        let mut k: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        k.sort();
        k
    }

    #[test]
    fn frontend_contract_round_trip() {
        let root = std::env::temp_dir().join(format!("downpour-ipc-{}", uuid::Uuid::new_v4()));
        let (_app, w) = app(&root);

        let snap = call(&w, "get_snapshot", json!({})).unwrap();
        // Field names from the `Settings` interface in src/api.ts.
        assert_eq!(
            keys(&snap["settings"]),
            sorted(&[
                "downloadDir",
                "categoryFolders",
                "maxConcurrent",
                "connections",
                "speedLimit",
                "maxRetries",
                "userAgent",
                "notifyOnComplete",
                "closeToTray",
                "watchClipboard",
                "theme",
            ])
        );

        // AddDialog sends this shape. Port 9 refuses connections, so nothing leaves the machine.
        let added = call(
            &w,
            "add_downloads",
            json!({ "requests": [{ "url": "http://127.0.0.1:9/files/report.zip", "start": false, "connections": 4, "fileName": "Q3 report.zip" }] }),
        )
        .unwrap();
        let d = &added[0];
        for field in [
            "id",
            "url",
            "finalUrl",
            "fileName",
            "saveDir",
            "totalSize",
            "downloaded",
            "resumable",
            "status",
            "error",
            "segments",
            "connections",
            "category",
            "referer",
            "contentType",
            "createdAt",
            "completedAt",
            "queuePosition",
        ] {
            assert!(
                d.get(field).is_some(),
                "Download is missing `{field}` that src/api.ts reads"
            );
        }
        assert_eq!(d["fileName"], "Q3 report.zip");
        assert_eq!(d["status"], "paused");
        assert_eq!(d["connections"], 4);
        assert_eq!(d["category"], "compressed");
        let id = d["id"].as_str().unwrap().to_string();

        call(
            &w,
            "reorder_download",
            json!({ "id": id, "beforeId": null }),
        )
        .unwrap();
        call(&w, "resume_downloads", json!({ "ids": [id] })).unwrap();
        let status = call(&w, "get_snapshot", json!({})).unwrap()["downloads"][0]["status"].clone();
        assert_ne!(status, "paused", "resume did not reach the engine");
        call(&w, "pause_downloads", json!({ "ids": [id] })).unwrap();

        let mut settings = snap["settings"].clone();
        settings["maxConcurrent"] = json!(99);
        settings["speedLimit"] = json!(1048576);
        let saved = call(&w, "update_settings", json!({ "settings": settings })).unwrap();
        assert_eq!(
            saved["maxConcurrent"], 20,
            "out-of-range values are clamped"
        );
        assert_eq!(saved["speedLimit"], 1048576);

        let err = call(&w, "probe_url", json!({ "url": "notaurl" })).unwrap_err();
        assert!(err.as_str().unwrap().contains("invalid URL"), "{err}");

        call(
            &w,
            "remove_downloads",
            json!({ "ids": [id], "deleteFiles": true }),
        )
        .unwrap();
        assert_eq!(
            call(&w, "get_snapshot", json!({})).unwrap()["downloads"],
            json!([])
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
