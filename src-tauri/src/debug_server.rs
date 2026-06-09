//! HTTP debug server — only compiled and started in debug builds.
//! Binds to 127.0.0.1:7891. Gives Claude a live API into the running app.

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use std::sync::Arc;
use tauri::Manager;

type AppHandle = Arc<tauri::AppHandle>;

pub fn start(app: tauri::AppHandle) {
    let state: AppHandle = Arc::new(app);
    tauri::async_runtime::spawn(async move {
        let router = Router::new()
            .route("/state",                  get(get_state))
            .route("/launch/:group_id",       post(do_launch))
            .route("/edit/:group_id",         post(do_edit))
            .route("/reload",                 post(do_reload))
            .route("/log",                    get(get_log))
            .route("/windows",                get(get_windows))
            .route("/close_group/:group_id",  post(close_group_windows))
            .with_state(state);

        match tokio::net::TcpListener::bind("127.0.0.1:7891").await {
            Ok(listener) => {
                eprintln!("[debug_server] Listening on http://127.0.0.1:7891");
                let _ = axum::serve(listener, router).await;
            }
            Err(e) => eprintln!("[debug_server] Failed to bind port 7891: {e}"),
        }
    });
}

// GET /state — full config from live AppState
async fn get_state(State(app): State<AppHandle>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Json(config)
}

// POST /launch/:group_id
async fn do_launch(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // Show launch overlay (same logic as the Tauri command)
    let label = config.groups.iter()
        .find(|g| g.id == group_id)
        .map(|g| format!("{} {}", g.icon, g.name))
        .unwrap_or_else(|| "Apps".to_string());
    let url = format!("launch-overlay.html?label={}", crate::percent_encode(&label));
    let app2 = (*app).clone();
    let _ = app2.clone().run_on_main_thread(move || {
        if let Some(old) = app2.get_webview_window("launch-overlay") { let _ = old.close(); }
        let _ = tauri::WebviewWindowBuilder::new(&app2, "launch-overlay", tauri::WebviewUrl::App(url.into()))
            .title("").inner_size(320.0, 72.0).center()
            .decorations(false).resizable(false).always_on_top(true)
            .skip_taskbar(true).build();
    });

    let result = crate::launcher::launch_group(&group_id, &config);

    let app3 = (*app).clone();
    let _ = app3.clone().run_on_main_thread(move || {
        if let Some(w) = app3.get_webview_window("launch-overlay") { let _ = w.close(); }
    });

    match result {
        Ok(_)  => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

// POST /edit/:group_id
async fn do_edit(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    crate::open_config_window_inner((*app).clone(), Some(group_id));
    Json(serde_json::json!({ "ok": true }))
}

// POST /reload — re-read config.json into AppState
async fn do_reload(State(app): State<AppHandle>) -> impl IntoResponse {
    let new_config = crate::config::load_config();
    *app.state::<crate::AppState>().0.lock().unwrap_or_else(|e| e.into_inner()) = new_config;
    Json(serde_json::json!({ "ok": true }))
}

// GET /log — return debug log file contents
async fn get_log() -> impl IntoResponse {
    let path = std::env::temp_dir().join("applauncher-debug.log");
    std::fs::read_to_string(&path).unwrap_or_default()
}

// GET /windows — enumerate visible windows with title, rect, virtual desktop
async fn get_windows() -> impl IntoResponse {
    // Use spawn_blocking so COM (IVirtualDesktopManager) runs on a dedicated thread
    // where CoInitializeEx can succeed without conflicting with tokio's MTA threads.
    let windows = tokio::task::spawn_blocking(enumerate_windows)
        .await
        .unwrap_or_default();
    Json(windows)
}

fn enumerate_windows() -> Vec<serde_json::Value> {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn EnumWindows(cb: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
            fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
            fn GetWindowTextW(hwnd: *mut std::ffi::c_void, buf: *mut u16, max: i32) -> i32;
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }

        let mut results: Vec<serde_json::Value> = Vec::new();

        unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, data: isize) -> i32 {
            if IsWindowVisible(hwnd) == 0 { return 1; }

            let results = &mut *(data as *mut Vec<serde_json::Value>);

            // Title — skip windows with no title
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len == 0 { return 1; }
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            // Rect
            let mut rect = [0i32; 4];
            GetWindowRect(hwnd, &mut rect);
            let (x, y, w, h) = (rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]);

            // Virtual desktop GUID (hex string, empty string if unavailable)
            let vd_guid = crate::virtual_desktop::get_window_virtual_desktop(hwnd)
                .map(|g| g.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(""))
                .unwrap_or_default();

            results.push(serde_json::json!({
                "title": title,
                "x": x, "y": y, "width": w, "height": h,
                "virtual_desktop_guid": vd_guid,
            }));
            1
        }

        unsafe { EnumWindows(cb, &mut results as *mut _ as isize); }
        results
    }

    #[cfg(not(target_os = "windows"))]
    vec![]
}

// POST /close_group/:group_id — close all visible windows whose titles match group item filenames.
// Sends WM_CLOSE (graceful) to each matching window. Returns count of windows closed.
async fn close_group_windows(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let group = match config.groups.iter().find(|g| g.id == group_id) {
        Some(g) => g.clone(),
        None => return Json(serde_json::json!({ "error": "group not found" })),
    };

    // Collect lowercase file stems from each item's path (e.g. "test-script" from "test-script.bat")
    let keywords: Vec<String> = group.items.iter()
        .filter_map(|item| item.path.as_ref())
        .filter_map(|p| std::path::Path::new(p).file_stem()?.to_str().map(|s| s.to_lowercase()))
        .filter(|s| !s.is_empty())
        .collect();

    let closed = tokio::task::spawn_blocking(move || close_windows_by_keywords(&keywords))
        .await
        .unwrap_or(0);

    Json(serde_json::json!({ "closed": closed }))
}

fn close_windows_by_keywords(keywords: &[String]) -> u32 {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn EnumWindows(cb: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
            fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
            fn GetWindowTextW(hwnd: *mut std::ffi::c_void, buf: *mut u16, max: i32) -> i32;
            fn PostMessageW(hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize) -> i32;
        }
        const WM_CLOSE: u32 = 0x0010;

        struct Data { keywords: Vec<String>, count: u32 }
        let mut data = Data { keywords: keywords.to_vec(), count: 0 };

        unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, param: isize) -> i32 {
            if IsWindowVisible(hwnd) == 0 { return 1; }
            let data = &mut *(param as *mut Data);
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len == 0 { return 1; }
            let title = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            for kw in &data.keywords {
                if title.contains(kw.as_str()) {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    data.count += 1;
                    break;
                }
            }
            1
        }

        unsafe { EnumWindows(cb, &mut data as *mut _ as isize); }
        return data.count;
    }

    #[cfg(not(target_os = "windows"))]
    { let _ = keywords; 0 }
}
