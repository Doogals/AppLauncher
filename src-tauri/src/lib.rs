mod config;
mod launcher;
mod license;
mod apps;
mod browsers;

use config::{AppConfig, Group, Item};
use apps::InstalledApp;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

struct AppState(Mutex<AppConfig>);

#[tauri::command]
fn open_url(url: String) {
    let _ = open::that(url);
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
fn save_group(group: Group, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    let limit = license::group_limit(&config.license_key, &config.license_instance_id);
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

#[tauri::command]
fn save_widget_color(color: String, state: State<AppState>) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    config.widget_color = Some(color);
    config::save_config(&config)
}

#[tauri::command]
fn get_installed_apps() -> Vec<InstalledApp> {
    apps::get_installed_apps()
}

#[tauri::command]
fn get_installed_browsers() -> Vec<browsers::BrowserInfo> {
    browsers::get_installed_browsers()
}

#[tauri::command]
fn get_browser_bookmarks(browser_path: String) -> Vec<browsers::BookmarkItem> {
    browsers::get_browser_bookmarks(&browser_path)
}

#[tauri::command]
fn send_feedback(message: String) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("Message is empty.".to_string());
    }

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
    use tauri::menu::{Menu, MenuItem};
    let edit = MenuItem::with_id(
        &app,
        format!("ctx-edit:{}", group_id),
        "Edit Group",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let delete = MenuItem::with_id(
        &app,
        format!("ctx-delete:{}", group_id),
        "Delete Group",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&edit, &delete]).map_err(|e| e.to_string())?;
    if let Some(window) = app.get_webview_window("widget") {
        window.popup_menu(&menu).map_err(|e| e.to_string())?;
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

#[cfg(target_os = "linux")]
fn register_autostart(exe_path: &str) {
    let Some(config_dir) = dirs::config_dir() else { return };
    let autostart_dir = config_dir.join("autostart");
    let _ = std::fs::create_dir_all(&autostart_dir);
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=App Launcher\nExec={}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n",
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
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
                } else {
                    #[cfg(debug_assertions)]
                    eprintln!("[menu] unhandled event id: {:?}", id);
                }
            });

            // Build tray menu
            let show_hide = MenuItem::with_id(app, "show_hide", "Show/Hide Widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_hide, &quit])?;

            // Create tray icon
            TrayIconBuilder::new()
                .icon(tauri::include_image!("icons/32x32.png"))
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
            #[cfg(all(any(target_os = "windows", target_os = "linux", target_os = "macos"), not(debug_assertions)))]
            {
                if let Ok(exe) = std::env::current_exe() {
                    register_autostart(&exe.to_string_lossy());
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
            deactivate_license,
            check_license_status,
            reorder_items,
            save_widget_position,
            save_widget_color,
            resize_widget,
            get_installed_apps,
            show_group_context_menu,
            get_installed_browsers,
            get_browser_bookmarks,
            send_feedback,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
