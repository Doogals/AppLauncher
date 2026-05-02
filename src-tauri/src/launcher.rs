use crate::config::{AppConfig, Item, ItemType};
use std::collections::HashMap;
use std::process::Command;

// ── Post-launch window positioning (Windows only) ────────────────────────────

#[cfg(target_os = "windows")]
fn position_window_for_item(pid: u32, item: &Item) {
    use std::thread;
    use std::time::Duration;

    let launch_monitor = item.launch_monitor;
    let launch_position = item.launch_position.clone();

    thread::spawn(move || {
        // Wait for the window to appear — retry up to 10 times
        let hwnd = (0..10).find_map(|_| {
            thread::sleep(Duration::from_millis(300));
            find_window_by_pid(pid)
        });

        if let Some(hwnd) = hwnd {
            apply_window_position(hwnd, launch_monitor, launch_position.as_deref());
        }
    });
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

#[cfg(target_os = "windows")]
fn apply_window_position(hwnd: *mut std::ffi::c_void, monitor_index: Option<u32>, position: Option<&str>) {
    extern "system" {
        fn EnumDisplayMonitors(
            hdc: *mut std::ffi::c_void,
            clip: *const std::ffi::c_void,
            cb: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut [i32; 4], isize) -> i32,
            data: isize,
        ) -> i32;
        fn SetWindowPos(hwnd: *mut std::ffi::c_void, insert: *mut std::ffi::c_void, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
    }

    // Collect monitors
    let mut monitors: Vec<[i32; 4]> = Vec::new();
    unsafe extern "system" fn mon_cb(_: *mut std::ffi::c_void, _: *mut std::ffi::c_void, rect: *mut [i32; 4], data: isize) -> i32 {
        let v = &mut *(data as *mut Vec<[i32; 4]>);
        v.push(*rect);
        1
    }
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), mon_cb, &mut monitors as *mut _ as isize); }

    if monitors.is_empty() { return; }

    let mon = monitors.get(monitor_index.unwrap_or(0) as usize)
        .or_else(|| monitors.first())
        .copied()
        .unwrap_or([0, 0, 1920, 1080]);

    let mon_x = mon[0];
    let mon_y = mon[1];
    let mon_w = mon[2] - mon[0];
    let mon_h = mon[3] - mon[1];

    // Get current window size
    let mut win_rect = [0i32; 4];
    unsafe { GetWindowRect(hwnd, &mut win_rect); }
    let win_w = win_rect[2] - win_rect[0];
    let win_h = win_rect[3] - win_rect[1];

    let (x, y) = match position.unwrap_or("center") {
        "top-left"      => (mon_x,                              mon_y),
        "top-center"    => (mon_x + (mon_w - win_w) / 2,       mon_y),
        "top-right"     => (mon_x + mon_w - win_w,             mon_y),
        "center-left"   => (mon_x,                              mon_y + (mon_h - win_h) / 2),
        "center"        => (mon_x + (mon_w - win_w) / 2,       mon_y + (mon_h - win_h) / 2),
        "center-right"  => (mon_x + mon_w - win_w,             mon_y + (mon_h - win_h) / 2),
        "bottom-left"   => (mon_x,                              mon_y + mon_h - win_h),
        "bottom-center" => (mon_x + (mon_w - win_w) / 2,       mon_y + mon_h - win_h),
        "bottom-right"  => (mon_x + mon_w - win_w,             mon_y + mon_h - win_h),
        _               => (mon_x + (mon_w - win_w) / 2,       mon_y + (mon_h - win_h) / 2),
    };

    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE); }
}

fn collect_browser_urls(
    items: &[Item],
    preferred_browser: Option<&str>,
) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut browser_urls: HashMap<String, Vec<String>> = HashMap::new();
    let mut fallback_urls: Vec<String> = Vec::new();

    for item in items {
        if let ItemType::Url = &item.item_type {
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

    // Batch URL items by browser for multi-tab launch
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
            if item.launch_monitor.is_some() || item.launch_position.is_some() {
                position_window_for_item(child.id(), item);
            }
        }
        ItemType::File | ItemType::Folder => {
            let path = item.path.as_ref().ok_or("Item is missing a path")?;
            open::that(path).map_err(|e| format!("Failed to open '{}': {}", path, e))?;
        }
        ItemType::Url => {
            let url = item.value.as_ref().ok_or("URL item is missing a value")?;
            match preferred_browser {
                Some(browser) => {
                    Command::new(browser)
                        .arg(url)
                        .spawn()
                        .map_err(|e| format!("Failed to open URL in browser: {}", e))?;
                }
                None => {
                    open::that(url)
                        .map_err(|e| format!("Failed to open URL '{}': {}", url, e))?;
                }
            }
        }
        ItemType::Script => {
            let path = item.path.as_ref().ok_or("Script item is missing a path")?;
            if path.to_lowercase().ends_with(".ps1") {
                Command::new("powershell")
                    .args(["-ExecutionPolicy", "Bypass", "-File", path])
                    .spawn()
                    .map_err(|e| format!("Failed to run PowerShell script: {}", e))?;
            } else {
                Command::new("cmd")
                    .args(["/C", path])
                    .spawn()
                    .map_err(|e| format!("Failed to run script '{}': {}", path, e))?;
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
        let item = Item { item_type: ItemType::App, path: None, value: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_url_missing_value_returns_error() {
        let item = Item { item_type: ItemType::Url, path: None, value: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a value"));
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error() {
        let item = Item { item_type: ItemType::Script, path: None, value: None };
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
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://a.com".to_string()) },
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://b.com".to_string()) },
            Item { item_type: ItemType::Url, path: Some("firefox.exe".to_string()), value: Some("https://c.com".to_string()) },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["chrome.exe"].len(), 2);
        assert_eq!(map["firefox.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_fall_back_to_preferred_browser() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://x.com".to_string()) },
        ];
        let (map, fallback) = collect_browser_urls(&items, Some("edge.exe"));
        assert_eq!(map["edge.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_with_no_browser_go_to_fallback() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://y.com".to_string()) },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert!(map.is_empty());
        assert_eq!(fallback.len(), 1);
    }
}
