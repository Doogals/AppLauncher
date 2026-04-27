mod config;
mod launcher;
mod license;
mod apps;

use config::{AppConfig, Group, Item};
use apps::InstalledApp;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

struct AppState(Mutex<AppConfig>);

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn save_group(group: Group, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    let limit = license::group_limit(&config.license_key);
    if let Some(pos) = config.groups.iter().position(|g| g.id == group.id) {
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
    config::save_config(&config)
}

#[tauri::command]
fn delete_group(group_id: String, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.groups.retain(|g| g.id != group_id);
    config::save_config(&config)
}

#[tauri::command]
fn launch_group(group_id: String, state: State<AppState>) -> Result<(), String> {
    let config = state.0.lock().unwrap().clone();
    launcher::launch_group(&group_id, &config)
}

#[tauri::command]
fn set_preferred_browser(path: String, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.preferred_browser = Some(path);
    config::save_config(&config)
}

#[tauri::command]
fn activate_license(key: String, state: State<AppState>) -> Result<(), String> {
    if !license::validate_key(&key) {
        return Err("Invalid license key.".to_string());
    }
    let mut config = state.0.lock().unwrap();
    config.license_key = Some(key);
    config::save_config(&config)
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

#[tauri::command]
fn get_installed_apps() -> Vec<InstalledApp> {
    apps::get_installed_apps()
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

#[cfg(target_os = "windows")]
fn register_autostart(exe_path: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ};

    let key_path = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = HSTRING::from("AppLauncher");
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::TrayIconBuilder;

            // Global menu event handler — handles tray menu AND popup context menus
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if id == "quit" {
                    app.exit(0);
                } else if id == "show_hide" {
                    if let Some(window) = app.get_webview_window("widget") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-edit:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:edit", group_id);
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-delete:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:delete", group_id);
                    }
                }
            });

            // Build tray menu
            let show_hide = MenuItem::with_id(app, "show_hide", "Show/Hide Widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_hide, &quit])?;

            // Create tray icon
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .build(app)?;

            // Restore saved widget position
            {
                let state = app.state::<AppState>();
                let cfg = state.0.lock().unwrap();
                if let (Some(x), Some(y)) = (cfg.widget_x, cfg.widget_y) {
                    if let Some(widget) = app.get_webview_window("widget") {
                        let _ = widget.set_position(tauri::PhysicalPosition::new(x, y));
                    }
                }
            }

            // Register auto-start only in release builds
            #[cfg(all(target_os = "windows", not(debug_assertions)))]
            {
                if let Ok(exe) = std::env::current_exe() {
                    register_autostart(&exe.to_string_lossy());
                }
            }

            Ok(())
        })
        .manage(AppState(Mutex::new(config)))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_group,
            delete_group,
            launch_group,
            set_preferred_browser,
            activate_license,
            reorder_items,
            save_widget_position,
            resize_widget,
            get_installed_apps,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
