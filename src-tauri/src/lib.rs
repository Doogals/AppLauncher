mod config;
mod launcher;
mod license;
mod apps;
mod browsers;

use config::{AppConfig, Group, Item};
use apps::InstalledApp;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

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
                    let _ = window.set_always_on_top(false);
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
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use tauri::menu::{Menu, MenuItem};
        let _ = (|| -> Result<(), String> {
            let edit = MenuItem::with_id(&handle, format!("ctx-edit:{}", group_id), "Edit Group", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let delete = MenuItem::with_id(&handle, format!("ctx-delete:{}", group_id), "Delete Group", true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let menu = Menu::with_items(&handle, &[&edit, &delete]).map_err(|e| e.to_string())?;
            if let Some(window) = handle.get_webview_window("widget") {
                window.popup_menu(&menu).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
    }).map_err(|e| e.to_string())
}

#[tauri::command]
fn show_widget_context_menu(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let launch_on_startup = state.0.lock().unwrap().launch_on_startup;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
        let _ = (|| -> Result<(), String> {
            let colors: &[(&str, &str)] = &[
                ("🎨  Default",  "rgba(22,33,62,0.95)"),
                ("⬛  Charcoal", "rgba(30,30,30,0.95)"),
                ("🌲  Forest",   "rgba(15,40,25,0.95)"),
                ("🌙  Midnight", "rgba(20,10,40,0.95)"),
                ("🔴  Rust",     "rgba(60,25,10,0.95)"),
                ("🩶  Steel",    "rgba(20,30,45,0.95)"),
            ];
            let color_items: Vec<MenuItem<_>> = colors.iter()
                .map(|(label, value)| MenuItem::with_id(&handle, format!("widget-color:{}", value), *label, true, None::<&str>)
                    .map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;

            let sep1 = PredefinedMenuItem::separator(&handle).map_err(|e| e.to_string())?;
            let startup_label = if launch_on_startup { "✓  Launch on Startup" } else { "   Launch on Startup" };
            let startup = MenuItem::with_id(&handle, "widget-startup", startup_label, true, None::<&str>)
                .map_err(|e| e.to_string())?;
            let sep2 = PredefinedMenuItem::separator(&handle).map_err(|e| e.to_string())?;
            let close = MenuItem::with_id(&handle, "widget-close", "Close", true, None::<&str>)
                .map_err(|e| e.to_string())?;

            let mut items: Vec<&dyn tauri::menu::IsMenuItem<_>> = color_items.iter().map(|i| i as _).collect();
            items.extend_from_slice(&[&sep1, &startup, &sep2, &close]);

            let menu = Menu::with_items(&handle, &items).map_err(|e| e.to_string())?;
            if let Some(window) = handle.get_webview_window("widget") {
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
    let value_name = HSTRING::from("AppLauncher");
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

fn build_tray_menu(app: &tauri::AppHandle, on_top: bool) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    use tauri::menu::{Menu, MenuItem};
    let label = if on_top { "Send to Back" } else { "Bring to Front" };
    let bring_send = MenuItem::with_id(app, "bring_send", label, true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    Menu::with_items(app, &[&bring_send, &quit]).map_err(|e| e.to_string())
}

fn rebuild_tray_menu(app: &tauri::AppHandle, on_top: bool) -> Result<(), String> {
    let menu = build_tray_menu(app, on_top)?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::TrayIconBuilder;

            // Global menu event handler — handles tray menu AND popup context menus
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if id == "quit" {
                    app.exit(0);
                } else if id == "bring_send" {
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app2.state::<AppState>();
                        let new_on_top = {
                            let mut config = state.0.lock().unwrap();
                            config.widget_on_top = !config.widget_on_top;
                            let _ = config::save_config(&config);
                            config.widget_on_top
                        };
                        if let Some(window) = app2.get_webview_window("widget") {
                            let _ = window.set_always_on_top(new_on_top);
                            #[cfg(target_os = "windows")]
                            if !new_on_top {
                                send_widget_to_back(&window);
                            }
                        }
                        let _ = rebuild_tray_menu(&app2, new_on_top);
                    });
                } else if let Some(group_id) = id.strip_prefix("ctx-edit:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:edit", group_id);
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-delete:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:delete", group_id);
                    }
                } else if let Some(color) = id.strip_prefix("widget-color:") {
                    // Defer all post-menu work to async so we're clear of the
                    // popup_menu nested Windows message loop before touching WebView2
                    let color_str = color.to_string();
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app2.state::<AppState>();
                        {
                            let mut config = state.0.lock().unwrap();
                            config.widget_color = Some(color_str.clone());
                            let _ = config::save_config(&config);
                        }
                        if let Some(window) = app2.get_webview_window("widget") {
                            let _ = window.emit("widget-color-changed", &color_str);
                        }
                    });
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
                } else if id == "widget-close" {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.close();
                    }
                } else {
                    #[cfg(debug_assertions)]
                    eprintln!("[menu] unhandled event id: {:?}", id);
                }
            });

            // Build tray menu with initial label based on config
            let on_top = app.state::<AppState>().0.lock().unwrap().widget_on_top;
            let handle = app.handle().clone();
            let tray_menu = build_tray_menu(&handle, on_top)?;

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
                        let _ = widget.set_position(tauri::PhysicalPosition::new(x, y));
                    }
                    if cfg.widget_on_top {
                        let _ = widget.set_always_on_top(true);
                    }
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
                                let _ = window.set_always_on_top(false);
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
            set_launch_on_startup,
            show_widget_context_menu,
            export_config,
            import_config,
            set_hotkey,
            get_monitors,
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
