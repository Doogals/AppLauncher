use crate::config::{AppConfig, Item, ItemType};
use std::collections::HashMap;
use std::process::Command;

// ── Shell-open a file and return its PID (Windows only) ──────────────────────

#[cfg(target_os = "windows")]
fn shell_open_with_pid(path: &str) -> Option<u32> {
    use std::ffi::c_void;

    extern "system" {
        fn ShellExecuteExW(info: *mut ShellExInfo) -> i32;
        fn GetProcessId(process: *mut c_void) -> u32;
        fn CloseHandle(object: *mut c_void) -> i32;
    }

    // Matches SHELLEXECUTEINFOW on 64-bit Windows (repr(C) inserts correct padding)
    #[repr(C)]
    struct ShellExInfo {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut c_void,
        lp_id_list: *mut c_void,
        lp_class: *const u16,
        hkey_class: *mut c_void,
        dw_hot_key: u32,
        h_icon: *mut c_void,
        h_process: *mut c_void,
    }

    const SW_SHOW: i32 = 5;
    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;

    let file: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();

    let mut info = ShellExInfo {
        cb_size: std::mem::size_of::<ShellExInfo>() as u32,
        f_mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: std::ptr::null_mut(),
        lp_directory: std::ptr::null_mut(),
        n_show: SW_SHOW,
        h_inst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null_mut(),
        hkey_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        h_icon: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok != 0 && !info.h_process.is_null() {
        let pid = unsafe { GetProcessId(info.h_process) };
        unsafe { CloseHandle(info.h_process) };
        if pid != 0 { return Some(pid); }
    }
    None
}

// ── Post-launch window positioning (Windows only) ────────────────────────────

#[cfg(target_os = "windows")]
fn position_window_for_item(pid: u32, x: i32, y: i32, w: Option<u32>, h: Option<u32>) {
    use std::thread;
    use std::time::Duration;

    thread::spawn(move || {
        let hwnd = (0..10).find_map(|_| {
            thread::sleep(Duration::from_millis(300));
            find_window_by_pid(pid)
        });
        if let Some(hwnd) = hwnd {
            place_window(hwnd, x, y, w, h);
            // Second pass: many apps restore their saved position ~1s after startup
            thread::sleep(Duration::from_millis(1500));
            place_window(hwnd, x, y, w, h);
        }
    });
}

#[cfg(target_os = "windows")]
fn place_window(hwnd: *mut std::ffi::c_void, x: i32, y: i32, w: Option<u32>, h: Option<u32>) {
    extern "system" {
        fn SetWindowPos(
            hwnd: *mut std::ffi::c_void,
            insert: *mut std::ffi::c_void,
            x: i32, y: i32, cx: i32, cy: i32,
            flags: u32,
        ) -> i32;
    }
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    unsafe {
        match (w, h) {
            (Some(cw), Some(ch)) => {
                SetWindowPos(hwnd, std::ptr::null_mut(), x, y, cw as i32, ch as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            _ => {
                SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn find_window_by_pid(target_pid: u32) -> Option<*mut std::ffi::c_void> {
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
    }

    struct State { pid: u32, result: *mut std::ffi::c_void }

    unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, data: isize) -> i32 {
        let state = &mut *(data as *mut State);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == state.pid && IsWindowVisible(hwnd) != 0 {
            state.result = hwnd;
            return 0;
        }
        1
    }

    let mut state = State { pid: target_pid, result: std::ptr::null_mut() };
    unsafe { EnumWindows(cb, &mut state as *mut _ as isize); }
    if state.result.is_null() { None } else { Some(state.result) }
}


fn is_chromium_based(path: &str) -> bool {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(name.as_str(),
        "chrome.exe" | "msedge.exe" | "brave.exe" | "chromium.exe" |
        "vivaldi.exe" | "opera.exe" | "operagx.exe" | "arc.exe" | "thorium.exe"
    )
}

fn collect_browser_urls(
    items: &[Item],
    preferred_browser: Option<&str>,
) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut browser_urls: HashMap<String, Vec<String>> = HashMap::new();
    let mut fallback_urls: Vec<String> = Vec::new();

    for item in items {
        if let ItemType::Url = &item.item_type {
            // Items with a saved position are launched individually with flags
            if item.launch_x.is_some() { continue; }
            if let Some(url) = &item.value {
                let browser = item.path.as_deref().or(preferred_browser);
                match browser {
                    Some(b) => browser_urls.entry(b.to_string()).or_default().push(url.clone()),
                    None => fallback_urls.push(url.clone()),
                }
            }
        }
    }
    (browser_urls, fallback_urls)
}

pub fn launch_group(group_id: &str, config: &AppConfig) -> Result<(), String> {
    let group = config
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| format!("Group '{}' not found", group_id))?;

    // Launch non-URL items individually
    for item in &group.items {
        if !matches!(item.item_type, ItemType::Url) {
            launch_item(item, &config.preferred_browser)?;
        }
    }

    // Launch URL items that have a saved position individually (with browser flags)
    for item in &group.items {
        if matches!(item.item_type, ItemType::Url) && item.launch_x.is_some() {
            launch_item(item, &config.preferred_browser)?;
        }
    }

    // Batch remaining URL items (no position) for multi-tab launch
    let (browser_urls, fallback_urls) =
        collect_browser_urls(&group.items, config.preferred_browser.as_deref());

    for (browser, urls) in &browser_urls {
        Command::new(browser)
            .args(urls)
            .spawn()
            .map_err(|e| format!("Failed to open URLs in '{}': {}", browser, e))?;
    }

    for url in &fallback_urls {
        open::that(url).map_err(|e| format!("Failed to open URL '{}': {}", url, e))?;
    }

    Ok(())
}

pub fn launch_item(item: &Item, preferred_browser: &Option<String>) -> Result<(), String> {
    match &item.item_type {
        ItemType::App => {
            let path = item.path.as_ref().ok_or("App item is missing a path")?;
            let mut cmd = Command::new(path);
            if let Some(args) = &item.value {
                if !args.is_empty() {
                    cmd.args(args.split_whitespace());
                }
            }
            let child = cmd.spawn()
                .map_err(|e| format!("Failed to launch app '{}': {}", path, e))?;
            #[cfg(target_os = "windows")]
            if let (Some(x), Some(y)) = (item.launch_x, item.launch_y) {
                position_window_for_item(child.id(), x, y, item.launch_width, item.launch_height);
            }
        }
        ItemType::File | ItemType::Folder => {
            let path = item.path.as_ref().ok_or("Item is missing a path")?;
            #[cfg(target_os = "windows")]
            if let (Some(x), Some(y)) = (item.launch_x, item.launch_y) {
                if let Some(pid) = shell_open_with_pid(path) {
                    position_window_for_item(pid, x, y, item.launch_width, item.launch_height);
                    return Ok(());
                }
            }
            open::that(path).map_err(|e| format!("Failed to open '{}': {}", path, e))?;
        }
        ItemType::Url => {
            let url = item.value.as_ref().ok_or("URL item is missing a value")?;
            let browser = item.path.as_deref().or(preferred_browser.as_deref());

            if let (Some(bp), Some(x), Some(y)) = (browser, item.launch_x, item.launch_y) {
                if is_chromium_based(bp) {
                    let mut args: Vec<String> = vec![
                        "--new-window".to_string(),
                        format!("--window-position={},{}", x, y),
                    ];
                    if let (Some(w), Some(h)) = (item.launch_width, item.launch_height) {
                        args.push(format!("--window-size={},{}", w, h));
                    }
                    args.push(url.to_string());
                    Command::new(bp)
                        .args(&args)
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map_err(|e| format!("Failed to open URL: {}", e))?;
                    return Ok(());
                }
            }

            // Non-Chromium fallback
            match browser {
                Some(bp) => {
                    Command::new(bp).arg(url).spawn()
                        .map_err(|e| format!("Failed to open URL in browser: {}", e))?;
                }
                None => {
                    open::that(url).map_err(|e| format!("Failed to open URL '{}': {}", url, e))?;
                }
            }
        }
        ItemType::Script => {
            let path = item.path.as_ref().ok_or("Script item is missing a path")?;
            let child = if path.to_lowercase().ends_with(".ps1") {
                Command::new("powershell")
                    .args(["-ExecutionPolicy", "Bypass", "-File", path])
                    .spawn()
                    .map_err(|e| format!("Failed to run PowerShell script: {}", e))?
            } else {
                Command::new("cmd")
                    .args(["/C", path])
                    .spawn()
                    .map_err(|e| format!("Failed to run script '{}': {}", path, e))?
            };
            #[cfg(target_os = "windows")]
            if let (Some(x), Some(y)) = (item.launch_x, item.launch_y) {
                position_window_for_item(child.id(), x, y, item.launch_width, item.launch_height);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, Group, Item, ItemType};

    fn make_config_with_group(items: Vec<Item>) -> (AppConfig, String) {
        let mut config = AppConfig::default();
        let group = Group {
            id: "group-1".to_string(),
            name: "Test".to_string(),
            icon: "🧪".to_string(),
            items,
        };
        let id = group.id.clone();
        config.groups.push(group);
        (config, id)
    }

    #[test]
    fn test_launch_group_not_found_returns_error() {
        let config = AppConfig::default();
        let result = launch_group("nonexistent-id", &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_launch_item_app_missing_path_returns_error() {
        let item = Item { item_type: ItemType::App, path: None, value: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_url_missing_value_returns_error() {
        let item = Item { item_type: ItemType::Url, path: None, value: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a value"));
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error() {
        let item = Item { item_type: ItemType::Script, path: None, value: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_group_with_empty_items_succeeds() {
        let (config, id) = make_config_with_group(vec![]);
        let result = launch_group(&id, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_url_items_with_same_browser_are_batched() {
        let items = vec![
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://a.com".to_string()), launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://b.com".to_string()), launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("firefox.exe".to_string()), value: Some("https://c.com".to_string()), launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["chrome.exe"].len(), 2);
        assert_eq!(map["firefox.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_fall_back_to_preferred_browser() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://x.com".to_string()), launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, Some("edge.exe"));
        assert_eq!(map["edge.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_with_no_browser_go_to_fallback() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://y.com".to_string()), launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert!(map.is_empty());
        assert_eq!(fallback.len(), 1);
    }
}
