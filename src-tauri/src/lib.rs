mod config;
mod launcher;
mod license;
mod apps;
mod browsers;
mod icons;
mod steam;
pub(crate) mod virtual_desktop;
mod debug_log;
#[cfg(debug_assertions)]
mod debug_server;

use config::{AppConfig, Group, Item};
use apps::InstalledApp;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

pub(crate) struct AppState(Mutex<AppConfig>);

// Transient per-layout-editor session: maps window label → chosen VD GUID.
struct LayoutDesktops(Mutex<HashMap<String, Vec<u8>>>);

#[tauri::command]
fn open_url(url: String) {
    let _ = open::that(url);
}

#[tauri::command]
async fn download_and_install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

// Update this URL after deploying the Cloudflare Worker
const WORKER_URL: &str = "https://app-launcher-proxy.dougbreaultjr.workers.dev";

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum LicenseStatus {
    Licensed,
    Revoked,
    Unlicensed,
    Unreachable,
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn save_group(group: Group, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    let limit = license::group_limit(&config.license_key, &config.license_instance_id);
    if let Some(pos) = config.groups.iter().position(|g| g.id == group.id) {
        // Items can disappear from a group on save (removed via the ✕ button,
        // or replaced) — any app-managed command file they owned would
        // otherwise be orphaned in the scripts dir forever. Diffing against
        // the incoming group (rather than deleting immediately when an item
        // is removed in the UI) means a Remove-then-Cancel never deletes a
        // file that's still referenced by the unchanged saved config.
        let old_paths: Vec<Option<String>> = config.groups[pos].items.iter()
            .map(|i| i.command_file_path.clone())
            .collect();
        let new_paths: std::collections::HashSet<String> = group.items.iter()
            .filter_map(|i| i.command_file_path.clone())
            .collect();
        cleanup_orphaned_command_files(&old_paths, &new_paths);
        config.groups[pos] = group;
    } else {
        if config.groups.len() >= limit {
            return Err(format!(
                "Free tier limited to {} groups. Upgrade to add more.",
                limit
            ));
        }
        config.groups.push(group);
    }
    config::save_config(&config)?;
    let _ = app.emit("groups-updated", ());
    Ok(())
}

#[tauri::command]
fn delete_group(group_id: String, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    if let Some(group) = config.groups.iter().find(|g| g.id == group_id) {
        let paths: Vec<Option<String>> = group.items.iter()
            .map(|i| i.command_file_path.clone())
            .collect();
        cleanup_orphaned_command_files(&paths, &std::collections::HashSet::new());
    }
    config.groups.retain(|g| g.id != group_id);
    config::save_config(&config)?;
    let _ = app.emit("groups-updated", ());
    Ok(())
}

/// Deletes any app-managed (under scripts_dir) command file in `paths` that
/// isn't also present in `keep`. Used when saving a group (some items may
/// have been removed) or deleting one outright (keep is empty). Never
/// touches a file outside the app's own scripts dir — those are files the
/// user linked directly and this app doesn't own.
fn cleanup_orphaned_command_files(paths: &[Option<String>], keep: &std::collections::HashSet<String>) {
    let scripts_dir = config::scripts_dir();
    for path in paths.iter().flatten() {
        if keep.contains(path) {
            continue;
        }
        let target = std::path::Path::new(path);
        if target.starts_with(&scripts_dir) {
            let _ = std::fs::remove_file(target);
        }
    }
}

pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

#[tauri::command]
async fn launch_group(group_id: String, app: tauri::AppHandle) -> Result<(), String> {
    // Clone config while holding the lock, then release before any await.
    let config = app.state::<AppState>().0.lock().unwrap().clone();

    // Show a centered overlay while the group is launching.
    let label = config.groups.iter()
        .find(|g| g.id == group_id)
        .map(|g| format!("{} {}", g.icon, g.name))
        .unwrap_or_else(|| "Apps".to_string());
    let url = format!("launch-overlay.html?label={}", percent_encode(&label));
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(old) = app2.get_webview_window("launch-overlay") {
            let _ = old.close();
        }
        let _ = tauri::WebviewWindowBuilder::new(
            &app2,
            "launch-overlay",
            tauri::WebviewUrl::App(url.into()),
        )
        .title("")
        .inner_size(320.0, 112.0)
        .center()
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build();
    });

    // Run the blocking launch on a thread-pool thread so the main thread
    // stays free to paint the overlay and process the message pump.
    let app_for_launch = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        launcher::launch_group_with_handle(&group_id, &config, app_for_launch)
    }).await.map_err(|e| e.to_string())?;

    // Dismiss overlay.
    let app3 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app3.get_webview_window("launch-overlay") {
            let _ = w.close();
        }
    });

    result
}

#[tauri::command]
fn abort_launch() {
    launcher::request_abort();
}

#[tauri::command]
fn set_preferred_browser(path: String, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.preferred_browser = Some(path);
    config::save_config(&config)
}

#[tauri::command]
fn activate_license(key: String, state: State<AppState>) -> Result<(), String> {
    let machine_name = std::env::var("COMPUTERNAME")
        .unwrap_or_else(|_| "Unknown PC".to_string());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post(format!("{}/activate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_name": machine_name,
        }))
        .send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
        return Err(body["error"].as_str().unwrap_or("Activation failed").to_string());
    }

    let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
    let instance_id = body["instance_id"]
        .as_str()
        .ok_or("Invalid response from server")?
        .to_string();

    let mut config = state.0.lock().unwrap();
    config.license_key = Some(key);
    config.license_instance_id = Some(instance_id);
    config.license_machine_name = Some(machine_name);
    config::save_config(&config)
}

#[tauri::command]
fn deactivate_license(state: State<AppState>) -> Result<(), String> {
    let (key, instance_id) = {
        let config = state.0.lock().unwrap();
        (config.license_key.clone(), config.license_instance_id.clone())
    };
    let (key, instance_id) = match (key, instance_id) {
        (Some(k), Some(i)) => (k, i),
        _ => return Err("No active license to deactivate.".to_string()),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post(format!("{}/deactivate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_id": instance_id,
        }))
        .send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
        return Err(body["error"].as_str().unwrap_or("Deactivation failed").to_string());
    }

    let mut config = state.0.lock().unwrap();
    config.license_key = None;
    config.license_instance_id = None;
    config.license_machine_name = None;
    config::save_config(&config)
}

#[tauri::command]
fn check_license_status(state: State<AppState>) -> LicenseStatus {
    let (key, instance_id) = {
        let config = state.0.lock().unwrap();
        (config.license_key.clone(), config.license_instance_id.clone())
    };
    let (key, instance_id) = match (key, instance_id) {
        (Some(k), Some(i)) => (k, i),
        _ => return LicenseStatus::Unlicensed,
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return LicenseStatus::Unreachable,
    };

    let res = match client
        .post(format!("{}/validate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_id": instance_id,
        }))
        .send()
    {
        Ok(r) => r,
        Err(_) => return LicenseStatus::Unreachable,
    };

    let body: serde_json::Value = match res.json() {
        Ok(b) => b,
        Err(_) => return LicenseStatus::Unreachable,
    };

    if body["valid"].as_bool() == Some(true) {
        LicenseStatus::Licensed
    } else {
        LicenseStatus::Revoked
    }
}

#[tauri::command]
fn reorder_items(group_id: String, items: Vec<Item>, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    if let Some(group) = config.groups.iter_mut().find(|g| g.id == group_id) {
        group.items = items;
    }
    config::save_config(&config)
}

#[tauri::command]
fn save_widget_position(x: i32, y: i32, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.widget_x = Some(x);
    config.widget_y = Some(y);
    config::save_config(&config)
}

/// Checks whether the widget is currently visible on any connected monitor.
/// If it's off-screen (e.g. a display was disconnected while the app was
/// running), it moves the widget to a safe position on the primary monitor
/// and saves the new position to config.
#[tauri::command]
fn ensure_widget_on_screen(app: tauri::AppHandle, state: State<AppState>) {
    let Some(widget) = app.get_webview_window("widget") else { return };
    let Ok(pos) = widget.outer_position() else { return };
    let monitors = app.available_monitors().unwrap_or_default();
    if monitors.is_empty() { return; }
    let on_screen = monitors.iter().any(|m| {
        let p = m.position();
        let s = m.size();
        pos.x >= p.x && pos.x < p.x + s.width as i32
            && pos.y >= p.y && pos.y < p.y + s.height as i32
    });
    if on_screen { return; }
    // Off-screen — move to primary monitor (0,0) or first available monitor
    let safe = monitors
        .iter()
        .find(|m| m.position().x == 0 && m.position().y == 0)
        .or_else(|| monitors.first())
        .map(|m| tauri::PhysicalPosition::new(m.position().x + 100, m.position().y + 50));
    let Some(safe_pos) = safe else { return };
    let _ = widget.set_position(safe_pos);
    // Persist so next launch also starts on-screen
    let mut config = state.0.lock().unwrap().clone();
    config.widget_x = Some(safe_pos.x);
    config.widget_y = Some(safe_pos.y);
    let _ = config::save_config(&config);
    *state.0.lock().unwrap() = config;
}


#[tauri::command]
fn save_widget_color(color: String, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.widget_color = Some(color.clone());
    config::save_config(&config)?;
    let _ = app.emit("widget-color-changed", &color);
    Ok(())
}

/// Sets (or clears, if `color` is empty) a single group's custom button
/// color. Emits the same "groups-updated" event save_group/delete_group use
/// so the widget re-renders immediately.
#[tauri::command]
fn save_group_color(group_id: String, color: String, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    let Some(group) = config.groups.iter_mut().find(|g| g.id == group_id) else {
        return Err("Group not found".to_string());
    };
    group.color = if color.is_empty() { None } else { Some(color) };
    config::save_config(&config)?;
    let _ = app.emit("groups-updated", ());
    Ok(())
}

#[derive(serde::Serialize)]
struct MonitorInfo {
    index: u32,
    name: String,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    is_primary: bool,
}

#[tauri::command]
fn get_monitors() -> Vec<MonitorInfo> {
    #[cfg(target_os = "windows")]
    {
        use std::sync::Mutex;
        extern "system" {
            fn EnumDisplayMonitors(
                hdc: *mut std::ffi::c_void,
                clip: *const std::ffi::c_void,
                callback: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut [i32; 4], isize) -> i32,
                data: isize,
            ) -> i32;
        }
        static MONITORS: Mutex<Vec<MonitorInfo>> = Mutex::new(Vec::new());
        {
            let mut m = MONITORS.lock().unwrap();
            m.clear();
        }
        unsafe extern "system" fn monitor_cb(
            _hmon: *mut std::ffi::c_void,
            _hdc: *mut std::ffi::c_void,
            rect: *mut [i32; 4],
            data: isize,
        ) -> i32 {
            let monitors = &*(data as *const Mutex<Vec<MonitorInfo>>);
            let mut m = monitors.lock().unwrap();
            let r = &*rect;
            let index = m.len() as u32;
            m.push(MonitorInfo {
                index,
                name: format!("Display {}", index + 1),
                width: r[2] - r[0],
                height: r[3] - r[1],
                x: r[0],
                y: r[1],
                is_primary: r[0] == 0 && r[1] == 0,
            });
            1
        }
        let monitors_ref = &MONITORS as *const _ as isize;
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), monitor_cb, monitors_ref); }
        MONITORS.lock().unwrap().drain(..).collect()
    }
    #[cfg(not(target_os = "windows"))]
    vec![MonitorInfo { index: 0, name: "Display 1".to_string(), width: 1920, height: 1080, x: 0, y: 0, is_primary: true }]
}

#[tauri::command]
fn export_config(state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let json = {
        let config = state.0.lock().unwrap();
        serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?
    };
    if let Some(path) = app.dialog().file().add_filter("JSON", &["json"]).blocking_save_file() {
        let path_buf = path.into_path().map_err(|e| e.to_string())?;
        std::fs::write(path_buf, json).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn import_config(state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    if let Some(path) = app.dialog().file().add_filter("JSON", &["json"]).blocking_pick_file() {
        let path_buf = path.into_path().map_err(|e| e.to_string())?;
        let json = std::fs::read_to_string(path_buf).map_err(|e| e.to_string())?;
        let new_config: config::AppConfig = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let mut config = state.0.lock().unwrap();
        *config = new_config;
        config::save_config(&config)?;
    }
    Ok(())
}


// ── "Edit Command Line" (terminal items: cmd.exe / powershell.exe / pwsh.exe) ──
//
// Item::command_file_path always ends up pointing at a directly-launchable
// .bat/.ps1 once these are done — "Create" generates one and hands it to the
// user's own editor; "Link" either points straight at an existing matching
// script (live — edits to it are picked up at the next launch automatically)
// or, for any other file type, imports its content into a new app-managed
// copy once. Nothing here re-reads or rewrites anything at launch time.

// Windows' default "open" action for a .bat/.cmd/.ps1 file is to RUN it (in
// a console window), not edit it — open::that() follows that association,
// which is why Create/Edit were launching a console instead of a text
// editor. Opening notepad.exe directly, with the path as an argument,
// sidesteps the file association entirely and always edits.
fn open_in_notepad(path: &str) -> Result<(), String> {
    std::process::Command::new("notepad.exe")
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Filenames must be valid on Windows and not collide with another item's
// generated script in the shared scripts dir (e.g. two items both named
// "Command Prompt") — strips characters Windows forbids in filenames, then
// appends " (2)", " (3)", etc. only if the plain name is already taken.
fn sanitized_unique_script_path(dir: &std::path::Path, label: &str, ext: &str) -> std::path::PathBuf {
    let cleaned: String = label
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let base = cleaned.trim().trim_end_matches('.').to_string();
    let base = if base.is_empty() { "Command".to_string() } else { base };
    // Windows reserves these names outright (any extension) — e.g. an item
    // literally named "con" would otherwise silently fail to create its
    // script file. Vanishingly unlikely in practice, but cheap to guard.
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let base = if RESERVED.contains(&base.to_uppercase().as_str()) {
        format!("{}_", base)
    } else {
        base
    };

    let mut path = dir.join(format!("{}.{}", base, ext));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{} ({}).{}", base, n, ext));
        n += 1;
    }
    path
}

#[tauri::command]
fn create_command_file(shell_path: String, label: String) -> Result<String, String> {
    let shell = launcher::terminal_shell_kind(&shell_path)
        .ok_or_else(|| "Not a recognized terminal shell".to_string())?;
    let dir = config::scripts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = sanitized_unique_script_path(&dir, &label, shell.script_extension());
    let header = match shell {
        launcher::TerminalShell::Cmd =>
            // @echo off must be the very first line. Without it, cmd echoes
            // every line (including these rem comments) prefixed with the
            // current prompt before running it — this suppresses that
            // entirely, for comments and real commands alike, leaving just
            // their actual output. PowerShell has no equivalent issue (it
            // never echoes the lines it executes), so this is cmd-only.
            "@echo off\r\n\
             rem ================================================================\r\n\
             rem  COMMENT BLOCK -- lines starting with \"rem\" are ignored. They\r\n\
             rem  are notes, not commands, and will not run.\r\n\
             rem\r\n\
             rem  Write the commands you want to run BELOW this block, with ONE\r\n\
             rem  COMMAND PER LINE.\r\n\
             rem\r\n\
             rem  IMPORTANT: Save this file (Ctrl+S) before closing Notepad, or\r\n\
             rem  your changes will NOT be used the next time this item launches.\r\n\
             rem ================================================================\r\n\r\n",
        launcher::TerminalShell::PowerShell =>
            "# ================================================================\r\n\
             #  COMMENT BLOCK -- lines starting with \"#\" are ignored. They are\r\n\
             #  notes, not commands, and will not run.\r\n\
             #\r\n\
             #  Write the commands you want to run BELOW this block, with ONE\r\n\
             #  COMMAND PER LINE.\r\n\
             #\r\n\
             #  IMPORTANT: Save this file (Ctrl+S) before closing Notepad, or\r\n\
             #  your changes will NOT be used the next time this item launches.\r\n\
             # ================================================================\r\n\r\n",
    };
    std::fs::write(&path, header).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().into_owned();
    open_in_notepad(&path_str)?;
    Ok(path_str)
}

#[tauri::command]
fn duplicate_command_file(path: String) -> Result<String, String> {
    let target = std::path::Path::new(&path);
    let scripts_dir = config::scripts_dir();
    if !target.starts_with(&scripts_dir) {
        // An externally-linked file the user picked directly, used live —
        // safe for both items to share the same reference, since this app's
        // cleanup logic never deletes anything outside its own scripts dir.
        return Ok(path);
    }
    // App-managed file — give the duplicate its own independent copy rather
    // than the same path, so clearing or deleting either item's command
    // later never affects the other.
    let content = std::fs::read_to_string(target).map_err(|e| e.to_string())?;
    let stem = target.file_stem().and_then(|s| s.to_str()).unwrap_or("Command");
    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("bat");
    let new_path = sanitized_unique_script_path(&scripts_dir, &format!("{} (copy)", stem), ext);
    std::fs::write(&new_path, content).map_err(|e| e.to_string())?;
    Ok(new_path.to_string_lossy().into_owned())
}

#[tauri::command]
fn pick_command_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    match app.dialog().file().blocking_pick_file() {
        Some(picked) => {
            let path_buf = picked.into_path().map_err(|e| e.to_string())?;
            Ok(Some(path_buf.to_string_lossy().into_owned()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
fn import_linked_command_file(picked_path: String, shell_path: String, label: String) -> Result<String, String> {
    let shell = launcher::terminal_shell_kind(&shell_path)
        .ok_or_else(|| "Not a recognized terminal shell".to_string())?;
    let already_matches = std::path::Path::new(&picked_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            e.eq_ignore_ascii_case(shell.script_extension())
                || (shell == launcher::TerminalShell::Cmd && e.eq_ignore_ascii_case("cmd"))
        })
        .unwrap_or(false);
    if already_matches {
        // Already a launchable script for this shell — use it directly, live.
        return Ok(picked_path);
    }
    // Anything else (.txt, no extension, etc.) gets imported once into a new
    // app-managed copy, since cmd/PowerShell can't execute it by path otherwise.
    let content = std::fs::read_to_string(&picked_path).map_err(|e| e.to_string())?;
    let dir = config::scripts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = sanitized_unique_script_path(&dir, &label, shell.script_extension());
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn open_command_file(path: String) -> Result<(), String> {
    open_in_notepad(&path)
}

#[tauri::command]
fn clear_command_file(path: String) -> Result<(), String> {
    // Only delete files this app generated itself (under its own scripts
    // dir) — never touch a file the user linked directly from elsewhere.
    let target = std::path::Path::new(&path);
    if target.starts_with(config::scripts_dir()) {
        let _ = std::fs::remove_file(target); // best-effort, fine if already gone
    }
    Ok(())
}

#[tauri::command]
fn set_hotkey(hotkey: String, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let old_hotkey = {
        let mut config = state.0.lock().unwrap();
        let old = config.hotkey.clone();
        config.hotkey = hotkey.clone();
        config::save_config(&config)?;
        old
    };
    let _ = app.global_shortcut().unregister(old_hotkey.as_str());
    let handle = app.clone();
    app.global_shortcut().on_shortcut(hotkey.as_str(), move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(window) = handle.get_webview_window("widget") {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
    }).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_launch_on_startup(enabled: bool, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.launch_on_startup = enabled;
    config::save_config(&config)?;

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            register_autostart(&exe.to_string_lossy());
        }
    } else {
        deregister_autostart();
    }

    Ok(())
}

#[tauri::command]
fn set_low_profile(enabled: bool, state: State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.low_profile = enabled;
    config::save_config(&config)?;
    let _ = app.emit("low-profile-changed", enabled);
    Ok(())
}

// async — get_suggested_apps shells out to PowerShell (twice, for Get-StartApps
// and Get-AppxPackage) and blocks waiting on those processes. A plain `fn`
// command runs on Tauri's main thread, which also handles all window input —
// so a slow synchronous command here froze the whole window's clicks/dragging
// until it returned. spawn_blocking moves the actual wait off the main thread.
#[tauri::command]
async fn get_suggested_apps() -> Vec<InstalledApp> {
    tauri::async_runtime::spawn_blocking(apps::get_suggested_apps)
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn get_installed_apps() -> Vec<InstalledApp> {
    tauri::async_runtime::spawn_blocking(apps::get_installed_apps)
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn get_installed_browsers() -> Vec<browsers::BrowserInfo> {
    tauri::async_runtime::spawn_blocking(browsers::get_installed_browsers)
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn get_browser_bookmarks(browser_path: String) -> Vec<browsers::BookmarkItem> {
    tauri::async_runtime::spawn_blocking(move || browsers::get_browser_bookmarks(&browser_path))
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn get_file_icon(path: String, args: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = apps::resolve_icon_source_path(&path, args.as_deref().unwrap_or(""));
        icons::get_file_icon(resolved)
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
async fn get_installed_steam_games() -> Vec<steam::SteamGame> {
    tauri::async_runtime::spawn_blocking(steam::get_installed_steam_games)
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn send_feedback(message: String) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("Message is empty.".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        client
            .post(format!("{}/feedback", WORKER_URL))
            .json(&serde_json::json!({ "message": message }))
            .send()
            .map_err(|e| e.to_string())?;

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// Returns [x, y, width, height] of the calling window's outer frame in physical pixels.
// Using GetWindowRect (Win32) avoids all DPI/CSS-pixel issues with window.screenX etc.
#[tauri::command]
fn get_window_frame_rect(window: tauri::WebviewWindow) -> Result<[i32; 4], String> {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        let mut rect = [0i32; 4];
        unsafe { GetWindowRect(hwnd.0, &mut rect); }
        Ok([rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]])
    }
    #[cfg(not(target_os = "windows"))]
    {
        let pos = window.outer_position().map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        Ok([pos.x, pos.y, size.width as i32, size.height as i32])
    }
}

#[tauri::command]
fn get_all_layout_positions(app: tauri::AppHandle, labels: Vec<String>) -> Vec<[i32; 4]> {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }
        labels.iter().map(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .map(|hwnd| {
                    let mut rect = [0i32; 4];
                    unsafe { GetWindowRect(hwnd.0 as *mut _, &mut rect); }
                    [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]
                })
                .unwrap_or([0, 0, 0, 0])
        }).collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        labels.iter().map(|label| {
            app.get_webview_window(label)
                .and_then(|w| {
                    let pos = w.outer_position().ok()?;
                    let size = w.outer_size().ok()?;
                    Some([pos.x, pos.y, size.width as i32, size.height as i32])
                })
                .unwrap_or([0, 0, 0, 0])
        }).collect()
    }
}

#[tauri::command]
fn close_layout_windows(app: tauri::AppHandle, labels: Vec<String>) {
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.close();
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct LayoutSavePayload {
    positions: Vec<[i32; 4]>,
    virtual_desktops: Vec<Option<Vec<u8>>>,
    // Position (0-based) each saved GUID was at when this was saved. Virtual
    // desktop GUIDs aren't permanently stable across reboots/Explorer
    // restarts even when desktop count/order doesn't change — this lets
    // launch fall back to "whatever desktop is at this position" instead of
    // assuming a desktop was deleted just because its GUID no longer matches.
    virtual_desktop_indices: Vec<Option<u32>>,
}

// Collects positions + stored VD selections, emits layout-save, closes layout windows.
#[tauri::command]
fn complete_layout_save(app: tauri::AppHandle, labels: Vec<String>, ld: State<LayoutDesktops>) {
    let desktops = ld.0.lock().unwrap().clone();
    let all_desktops = crate::virtual_desktop::get_virtual_desktops();
    #[cfg(target_os = "windows")]
    let (positions, virtual_desktops): (Vec<[i32; 4]>, Vec<Option<Vec<u8>>>) = {
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }
        labels.iter().map(|label| {
            let rect = app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .map(|hwnd| {
                    let mut r = [0i32; 4];
                    unsafe { GetWindowRect(hwnd.0 as *mut _, &mut r); }
                    [r[0], r[1], r[2] - r[0], r[3] - r[1]]
                })
                .unwrap_or([0, 0, 0, 0]);
            let guid = desktops.get(label).cloned();
            (rect, guid)
        }).unzip()
    };
    #[cfg(not(target_os = "windows"))]
    let (positions, virtual_desktops): (Vec<[i32; 4]>, Vec<Option<Vec<u8>>>) = labels.iter().map(|label| {
        let pos = app.get_webview_window(label)
            .and_then(|w| {
                let p = w.outer_position().ok()?;
                let s = w.outer_size().ok()?;
                Some([p.x, p.y, s.width as i32, s.height as i32])
            })
            .unwrap_or([0, 0, 0, 0]);
        let guid = desktops.get(label).cloned();
        (pos, guid)
    }).unzip();

    let virtual_desktop_indices: Vec<Option<u32>> = virtual_desktops.iter().map(|guid| {
        guid.as_ref().and_then(|g| {
            all_desktops.iter().position(|d| d.guid.as_slice() == g.as_slice()).map(|i| i as u32)
        })
    }).collect();

    ld.0.lock().unwrap().clear();
    let _ = app.emit("layout-save", LayoutSavePayload { positions, virtual_desktops, virtual_desktop_indices });
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.close();
        }
    }
}

#[tauri::command]
fn complete_layout_cancel(app: tauri::AppHandle, labels: Vec<String>, ld: State<LayoutDesktops>) {
    ld.0.lock().unwrap().clear();
    let _ = app.emit("layout-cancel", ());
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.close();
        }
    }
}

#[tauri::command]
fn get_virtual_desktops() -> Vec<virtual_desktop::VirtualDesktop> {
    virtual_desktop::get_virtual_desktops()
}

#[tauri::command]
fn get_current_virtual_desktop_guid() -> Option<Vec<u8>> {
    virtual_desktop::get_current_virtual_desktop_guid()
}

// Stores the user's virtual desktop choice for a layout-item window.
// Called by each layout-item window when the dropdown changes.
// `guid: None` clears the entry (use "any desktop").
// Also immediately moves the window to the chosen desktop so the user sees it land there.
#[tauri::command]
fn set_layout_item_desktop(app: tauri::AppHandle, ld: State<LayoutDesktops>, label: String, guid: Option<Vec<u8>>) {
    let mut map = ld.0.lock().unwrap();
    match guid {
        Some(g) => {
            #[cfg(target_os = "windows")]
            if let Some(hwnd) = app.get_webview_window(&label).and_then(|w| w.hwnd().ok()) {
                crate::virtual_desktop::move_window_to_virtual_desktop(hwnd.0 as *mut _, &g);
            }
            map.insert(label, g);
        }
        None => { map.remove(&label); }
    }
}

// Opens the config window from Rust so its lifecycle is independent of widget.js.
#[tauri::command]
fn open_config_window(app: tauri::AppHandle, group_id: Option<String>) {
    open_config_window_inner(app, group_id);
}

pub(crate) fn open_config_window_inner(app: tauri::AppHandle, group_id: Option<String>) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(existing) = app2.get_webview_window("config") {
            let _ = existing.set_focus();
            return;
        }
        let url = match &group_id {
            Some(id) => format!("config.html?id={}", id),
            None => "config.html".to_string(),
        };
        let title = if group_id.is_some() { "Edit Group" } else { "New Group" };
        let _ = tauri::WebviewWindowBuilder::new(
            &app2,
            "config",
            tauri::WebviewUrl::App(url.into()),
        )
        .title(title)
        .inner_size(420.0, 580.0)
        .min_inner_size(360.0, 460.0)
        .center()
        .decorations(true)
        .resizable(true)
        .always_on_top(true)
        .build();
    });
}

#[tauri::command]
fn resize_widget(width: u32, height: u32, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("widget") {
        window
            .set_size(tauri::LogicalSize::new(width as f64, height as f64))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn show_group_context_menu(group_id: String, app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use tauri::menu::{Menu, MenuItem};
        let _ = (|| -> Result<(), String> {
            let edit = MenuItem::with_id(&handle, format!("ctx-edit:{}", group_id), "Edit Group", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let color = MenuItem::with_id(&handle, format!("ctx-color:{}", group_id), "🎨  Change Color", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let delete = MenuItem::with_id(&handle, format!("ctx-delete:{}", group_id), "Delete Group", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let menu = Menu::with_items(&handle, &[&edit, &color, &delete]).map_err(|e| e.to_string())?;
            if let Some(window) = handle.get_webview_window("widget") {
                force_foreground(&window);
                window.popup_menu(&menu).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
    }).map_err(|e| e.to_string())
}

/// Opens (or replaces, if one's already open) the Group Color window for the
/// Moves an already-created layout-editor window to the saved **physical** pixel
/// position and resizes it to the saved physical dimensions. Called from JS right
/// after `new WebviewWindow(...)` fires its `tauri://created` event, so the window
/// is guaranteed to exist by the time this runs.
///
/// Using physical pixels (matching GetWindowRect's saved values) sidesteps the
/// per-monitor DPR ambiguity of the old approach, where logical coordinates derived
/// from `window.devicePixelRatio` on the config window's monitor were wrong when
/// items were positioned on a different monitor.
#[tauri::command]
async fn set_layout_window_physics(
    app: tauri::AppHandle,
    label: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) {
    let Some(win) = app.get_webview_window(&label) else { return };
    // set_size on Windows maps to SetWindowPos with the outer rect dimensions,
    // which matches GetWindowRect — so the saved physical size restores correctly.
    let _ = win.set_size(tauri::PhysicalSize::new(width, height));
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

/// given group. Shared by the widget's right-click "Change Color" menu item
/// and the explicit "🎨 Color" button in the Edit Group window — the button
/// exists so color can be set during creation/editing too, not only after a
/// group has already been saved and is showing as a button on the widget.
async fn open_group_color_window_inner(app: &tauri::AppHandle, group_id: String) {
    if let Some(existing) = app.get_webview_window("group-color") {
        let _ = existing.close();
    }
    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "group-color",
        tauri::WebviewUrl::App(format!("group-color.html?mode=group&id={}", group_id).into()),
    )
    .title("Group Color")
    // Explicit dark background at the window level (not just in the page's
    // own CSS) — WebView2 can show its own default white background before/
    // around the page content if the window's actual rendered size doesn't
    // exactly match what the CSS expects, and the 20-color, 4-row grid this
    // window now shows is taller than the original 6-color version this size
    // was set for. Setting this removes any chance of white showing through
    // regardless of that timing/sizing, and the taller inner_size below
    // gives the 4-row grid proper room instead of being right at the edge.
    .background_color(tauri::webview::Color(0x1a, 0x1a, 0x2e, 255))
    .inner_size(280.0, 420.0)
    .center()
    .decorations(true)
    .resizable(false)
    .always_on_top(true)
    .build();
}

/// Callable directly from the Edit Group window's "🎨 Color" button.
#[tauri::command]
async fn open_group_color_window(app: tauri::AppHandle, group_id: String) -> Result<(), String> {
    open_group_color_window_inner(&app, group_id).await;
    Ok(())
}

/// Same tabbed color-picker window as the group's, reused in "widget" mode
/// (no group id, saves via save_widget_color instead) — avoids maintaining
/// two near-identical tabbed pickers for what's otherwise the same UI.
/// Callable from the "🎨 Choose Widget Color" button in App Settings.
#[tauri::command]
async fn open_widget_color_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("widget-color") {
        let _ = existing.close();
    }
    let _ = tauri::WebviewWindowBuilder::new(
        &app,
        "widget-color",
        tauri::WebviewUrl::App("group-color.html?mode=widget".into()),
    )
    .title("Widget Color")
    .background_color(tauri::webview::Color(0x1a, 0x1a, 0x2e, 255))
    .inner_size(280.0, 420.0)
    .center()
    .decorations(true)
    .resizable(false)
    .always_on_top(true)
    .build();
    Ok(())
}

#[tauri::command]
fn show_widget_context_menu(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let (launch_on_startup, low_profile) = {
        let config = state.0.lock().unwrap();
        (config.launch_on_startup, config.low_profile)
    };
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
        let _ = (|| -> Result<(), String> {
            let startup_label = if launch_on_startup { "✓  Launch on Startup" } else { "   Launch on Startup" };
            let startup = MenuItem::with_id(&handle, "widget-startup", startup_label, true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let low_profile_label = if low_profile { "✓  Low Profile" } else { "   Low Profile" };
            let low_profile_item = MenuItem::with_id(&handle, "widget-low-profile", low_profile_label, true, None::<&str>)
                .map_err(|e| e.to_string())?;
            // Moved here from the system tray — nobody was finding it there.
            // Reuses the "bring_send" id so the existing on_menu_event handler
            // (which already handles both tray and popup-menu events) just works.
            // Label reflects the widget's *actual current* z-order position
            // (checked live, right now) rather than a persisted sticky flag —
            // the widget can end up visually in front of other windows just
            // through normal use, with no "always on top" mode ever toggled.
            let currently_in_front = handle
                .get_webview_window("widget")
                .map(|w| is_widget_in_front(&w))
                .unwrap_or(false);
            // Same leading padding as the unchecked toggle items above, so the
            // text lines up regardless of whether a row has a checkmark or not.
            let bring_send_label = if currently_in_front { "   Send to Back" } else { "   Bring to Front" };
            let bring_send = MenuItem::with_id(&handle, "bring_send", bring_send_label, true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let sep1 = PredefinedMenuItem::separator(&handle).map_err(|e| e.to_string())?;
            let settings = MenuItem::with_id(&handle, "widget-settings", "⚙️  App Settings\u{2026}", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let sep2 = PredefinedMenuItem::separator(&handle).map_err(|e| e.to_string())?;
            let close = MenuItem::with_id(&handle, "widget-close", "   Close", true, None::<&str>)
                .map_err(|e| e.to_string())?;

            let items: Vec<&dyn tauri::menu::IsMenuItem<_>> = vec![
                &startup, &low_profile_item, &bring_send, &sep1, &settings, &sep2, &close
            ];

            let menu = Menu::with_items(&handle, &items).map_err(|e| e.to_string())?;
            if let Some(window) = handle.get_webview_window("widget") {
                force_foreground(&window);
                window.popup_menu(&menu).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
    }).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn deregister_autostart() {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegOpenKeyExW, RegDeleteValueW, HKEY, HKEY_CURRENT_USER, KEY_WRITE};
    let key_path = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = HSTRING::from("TakeOff");
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &key_path, 0, KEY_WRITE, &mut hkey).is_ok() {
            let _ = RegDeleteValueW(hkey, &value_name);
        }
    }
}

#[cfg(target_os = "linux")]
fn deregister_autostart() {
    if let Some(config_dir) = dirs::config_dir() {
        let _ = std::fs::remove_file(config_dir.join("autostart/app-launcher.desktop"));
    }
}

#[cfg(target_os = "macos")]
fn deregister_autostart() {
    if let Some(home) = dirs::home_dir() {
        let _ = std::fs::remove_file(home.join("Library/LaunchAgents/com.dougb.applauncher.plist"));
    }
}

#[cfg(target_os = "windows")]
fn register_autostart(exe_path: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ};

    let key_path = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = HSTRING::from("TakeOff");
    let value_data: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &key_path, 0, KEY_WRITE, &mut hkey).is_ok() {
            let _ = RegSetValueExW(
                hkey,
                &value_name,
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    value_data.as_ptr() as *const u8,
                    value_data.len() * 2,
                )),
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn register_autostart(exe_path: &str) {
    let Some(config_dir) = dirs::config_dir() else { return };
    let autostart_dir = config_dir.join("autostart");
    let _ = std::fs::create_dir_all(&autostart_dir);
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=TakeOff\nExec={}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n",
        exe_path
    );
    let _ = std::fs::write(autostart_dir.join("app-launcher.desktop"), desktop);
}

#[cfg(target_os = "macos")]
fn register_autostart(exe_path: &str) {
    let Some(home) = dirs::home_dir() else { return };
    let agents_dir = home.join("Library/LaunchAgents");
    let _ = std::fs::create_dir_all(&agents_dir);
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>com.dougb.applauncher</string>\n\
         <key>ProgramArguments</key><array><string>{}</string></array>\n\
         <key>RunAtLoad</key><true/>\n\
         </dict></plist>\n",
        exe_path
    );
    let _ = std::fs::write(agents_dir.join("com.dougb.applauncher.plist"), plist);
}



#[cfg(target_os = "windows")]
fn send_widget_to_back(window: &tauri::WebviewWindow) {
    extern "system" {
        fn SetWindowPos(
            hwnd: *mut std::ffi::c_void,
            hwnd_insert_after: *mut std::ffi::c_void,
            x: i32, y: i32, cx: i32, cy: i32,
            flags: u32,
        ) -> i32;
    }
    const HWND_BOTTOM: *mut std::ffi::c_void = 1usize as *mut _;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOACTIVATE: u32 = 0x0010;
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            SetWindowPos(hwnd.0, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// One-time raise to the top of the z-order — NOT a sticky "always on top"
/// (HWND_TOP, not HWND_TOPMOST). The widget behaves like a normal window
/// again immediately afterward; it just starts out in front.
#[cfg(target_os = "windows")]
fn bring_widget_to_front(window: &tauri::WebviewWindow) {
    extern "system" {
        fn SetWindowPos(
            hwnd: *mut std::ffi::c_void,
            hwnd_insert_after: *mut std::ffi::c_void,
            x: i32, y: i32, cx: i32, cy: i32,
            flags: u32,
        ) -> i32;
    }
    const HWND_TOP: *mut std::ffi::c_void = 0usize as *mut _;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOACTIVATE: u32 = 0x0010;
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            SetWindowPos(hwnd.0, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// Checks the widget's actual current z-order position: is any other visible,
/// non-minimized window currently overlapping it and rendered above it? Used
/// to dynamically label the menu item "Bring to Front" / "Send to Back"
/// based on real on-screen state, instead of a persisted sticky setting that
/// drifts out of sync with reality (e.g. widget happens to be on top of VS
/// Code right now, but the old "always on top" flag was never toggled).
#[cfg(target_os = "windows")]
fn is_widget_in_front(window: &tauri::WebviewWindow) -> bool {
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
        fn IsIconic(hwnd: *mut std::ffi::c_void) -> i32;
        fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
    }

    struct CheckData {
        target: usize,
        target_rect: [i32; 4],
        covered: bool,
    }

    unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, data: isize) -> i32 {
        let d = &mut *(data as *mut CheckData);
        if hwnd as usize == d.target {
            return 0; // reached our own window with nothing covering it first — stop
        }
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1; // continue
        }
        let mut rect = [0i32; 4];
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return 1;
        }
        let [l1, t1, r1, b1] = rect;
        let [l2, t2, r2, b2] = d.target_rect;
        if l1 < r2 && r1 > l2 && t1 < b2 && b1 > t2 {
            d.covered = true;
            return 0; // something visible overlaps us before we were reached — stop
        }
        1
    }

    let Ok(hwnd) = window.hwnd() else { return true };
    unsafe {
        let mut target_rect = [0i32; 4];
        if GetWindowRect(hwnd.0, &mut target_rect) == 0 {
            return true;
        }
        let mut data = CheckData { target: hwnd.0 as usize, target_rect, covered: false };
        EnumWindows(cb, &mut data as *mut _ as isize);
        !data.covered
    }
}

#[cfg(not(target_os = "windows"))]
fn is_widget_in_front(_window: &tauri::WebviewWindow) -> bool {
    true
}

/// TrackPopupMenu (what Tauri's popup_menu() wraps on Windows) needs the
/// owning window to actually be the foreground window when it's called —
/// otherwise Windows can still draw the menu, but mouse input doesn't route
/// to it, making it look "frozen" (visible, nothing highlights, nothing is
/// clickable). The widget is an always-on-top floating bar that deliberately
/// avoids stealing focus during normal use, so it's often NOT the foreground
/// window at the moment of a right-click — hence this being intermittent.
/// Calling this from within the same right-click's event handling is one of
/// the documented exceptions to Windows' foreground-lock restrictions.
#[cfg(target_os = "windows")]
fn force_foreground(window: &tauri::WebviewWindow) {
    extern "system" {
        fn SetForegroundWindow(hwnd: *mut std::ffi::c_void) -> i32;
    }
    if let Ok(hwnd) = window.hwnd() {
        unsafe { SetForegroundWindow(hwnd.0 as *mut _); }
    }
}

#[cfg(not(target_os = "windows"))]
fn force_foreground(_window: &tauri::WebviewWindow) {}

// "Send to Back" / "Bring to Front" moved to the widget's right-click menu
// (see show_widget_context_menu) — nobody was finding it in the tray.
fn build_tray_menu(app: &tauri::AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    use tauri::menu::{Menu, MenuItem};
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    Menu::with_items(app, &[&quit]).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[allow(unused_imports)]
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::TrayIconBuilder;

            // Global menu event handler — handles tray menu AND popup context menus
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if id == "quit" {
                    app.exit(0);
                } else if id == "bring_send" {
                    // One-time z-order push, not a sticky "always on top" toggle.
                    // Re-checks live state at click time (cheap, and avoids any
                    // staleness between when the menu opened and now).
                    if let Some(window) = app.get_webview_window("widget") {
                        #[cfg(target_os = "windows")]
                        {
                            if is_widget_in_front(&window) {
                                send_widget_to_back(&window);
                            } else {
                                bring_widget_to_front(&window);
                            }
                        }
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-edit:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:edit", group_id);
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-color:") {
                    let app2 = app.clone();
                    let group_id = group_id.to_string();
                    tauri::async_runtime::spawn(async move {
                        open_group_color_window_inner(&app2, group_id).await;
                    });
                } else if let Some(group_id) = id.strip_prefix("ctx-delete:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:delete", group_id);
                    }
                } else if id == "widget-startup" {
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app2.state::<AppState>();
                        let new_val = {
                            let mut config = state.0.lock().unwrap();
                            config.launch_on_startup = !config.launch_on_startup;
                            let _ = config::save_config(&config);
                            config.launch_on_startup
                        };
                        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                        if new_val {
                            if let Ok(exe) = std::env::current_exe() {
                                register_autostart(&exe.to_string_lossy());
                            }
                        } else {
                            deregister_autostart();
                        }
                    });
                } else if id == "widget-low-profile" {
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app2.state::<AppState>();
                        let new_val = {
                            let mut config = state.0.lock().unwrap();
                            config.low_profile = !config.low_profile;
                            let _ = config::save_config(&config);
                            config.low_profile
                        };
                        let _ = app2.emit("low-profile-changed", new_val);
                    });
                } else if id == "widget-settings" {
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(existing) = app2.get_webview_window("config") {
                            let _ = existing.set_focus();
                        } else {
                            let _ = tauri::WebviewWindowBuilder::new(
                                &app2,
                                "config",
                                tauri::WebviewUrl::App("config.html?tab=settings".into()),
                            )
                            .title("App Settings")
                            .inner_size(420.0, 460.0)
                            .center()
                            .decorations(true)
                            .resizable(false)
                            .always_on_top(true)
                            .build();
                        }
                    });
                } else if id == "widget-close" {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.close();
                    }
                } else {
                    #[cfg(debug_assertions)]
                    eprintln!("[menu] unhandled event id: {:?}", id);
                }
            });

            let handle = app.handle().clone();
            let tray_menu = build_tray_menu(&handle)?;

            // Create tray icon with stable ID so we can update its menu later
            TrayIconBuilder::with_id("main-tray")
                .icon(tauri::include_image!("icons/32x32.png"))
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .build(app)?;

            // Restore saved widget position and always-on-top state
            {
                let state = app.state::<AppState>();
                let cfg = state.0.lock().unwrap();
                if let Some(widget) = app.get_webview_window("widget") {
                    if let (Some(x), Some(y)) = (cfg.widget_x, cfg.widget_y) {
                        // Only restore the saved position if it falls within a
                        // currently-connected monitor. If the monitor it was on
                        // has since been disconnected the coordinates are off-
                        // screen, so skip the restore and let the OS place the
                        // widget on the primary display instead.
                        let monitors = app.available_monitors().unwrap_or_default();
                        let visible = monitors.is_empty() || monitors.iter().any(|m| {
                            let p = m.position();
                            let s = m.size();
                            x >= p.x && x < p.x + s.width as i32
                                && y >= p.y && y < p.y + s.height as i32
                        });
                        if visible {
                            let _ = widget.set_position(tauri::PhysicalPosition::new(x, y));
                        }
                    }
                    // No more sticky "always on top" on startup — Bring to
                    // Front / Send to Back is now a one-time z-order push,
                    // re-evaluated live each time the widget menu is opened.
                }
            }

            // Register auto-start only in release builds, only if user has it enabled
            #[cfg(all(any(target_os = "windows", target_os = "linux", target_os = "macos"), not(debug_assertions)))]
            {
                let launch_on_startup = app.state::<AppState>().0.lock().unwrap().launch_on_startup;
                if launch_on_startup {
                    if let Ok(exe) = std::env::current_exe() {
                        register_autostart(&exe.to_string_lossy());
                    }
                }
            }

            // Register global hotkey
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let hotkey = app.state::<AppState>().0.lock().unwrap().hotkey.clone();
                let handle = app.handle().clone();
                if let Err(e) = app.handle().global_shortcut().on_shortcut(hotkey.as_str(), move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Some(window) = handle.get_webview_window("widget") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                }) {
                    eprintln!("[hotkey] Failed to register {}: {}", hotkey, e);
                }
            }

            // Check for updates in the background (release builds only)
            #[cfg(not(debug_assertions))]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(updater) = handle.updater() {
                        if let Ok(Some(update)) = updater.check().await {
                            let _ = handle.emit("update-available", &update.version);
                        }
                    }
                });
            }

            // Start debug HTTP server on port 7891 (dev builds only)
            #[cfg(debug_assertions)]
            debug_server::start(app.handle().clone());

            Ok(())
        })
        .manage(AppState(Mutex::new(config)))
        .manage(LayoutDesktops(Mutex::new(HashMap::new())))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_group,
            delete_group,
            launch_group,
            abort_launch,
            set_preferred_browser,
            activate_license,
            deactivate_license,
            check_license_status,
            reorder_items,
            save_widget_position,
            ensure_widget_on_screen,
            save_widget_color,
            save_group_color,
            open_group_color_window,
            open_widget_color_window,
            set_launch_on_startup,
            set_low_profile,
            show_widget_context_menu,
            export_config,
            import_config,
            set_hotkey,
            get_monitors,
            get_window_frame_rect,
            get_all_layout_positions,
            close_layout_windows,
            complete_layout_save,
            complete_layout_cancel,
            get_virtual_desktops,
            get_current_virtual_desktop_guid,
            set_layout_item_desktop,
            set_layout_window_physics,
            open_config_window,
            resize_widget,
            get_installed_apps,
            get_suggested_apps,
            show_group_context_menu,
            get_installed_browsers,
            get_browser_bookmarks,
            get_file_icon,
            get_installed_steam_games,
            send_feedback,
            open_url,
            download_and_install_update,
            create_command_file,
            pick_command_file,
            import_linked_command_file,
            open_command_file,
            clear_command_file,
            duplicate_command_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
