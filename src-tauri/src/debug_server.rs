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
            .route("/state",              get(get_state))
            .route("/launch/:group_id",   post(do_launch))
            .route("/edit/:group_id",     post(do_edit))
            .route("/reload",             post(do_reload))
            .route("/log",                get(get_log))
            .route("/windows",            get(get_windows))
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
    let config = app.state::<crate::AppState>().0.lock().unwrap().clone();
    Json(config)
}

// POST /launch/:group_id
async fn do_launch(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap().clone();
    match crate::launcher::launch_group(&group_id, &config) {
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
    *app.state::<crate::AppState>().0.lock().unwrap() = new_config;
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
