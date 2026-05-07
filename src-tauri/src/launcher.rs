use crate::config::{AppConfig, Item, ItemType};
use std::collections::HashMap;
use std::process::Command;

// ── Snapshot-based window positioning (Windows only) ─────────────────────────
//
// Snapshot all visible HWNDs before launch, then poll for any new HWND that
// appears afterward. This works for Store/packaged apps where the launched
// process is an activator and the real window lives in a hosted process —
// PID-matching fundamentally breaks for those, but a new HWND is always new.

#[cfg(target_os = "windows")]
fn collect_visible_hwnds() -> std::collections::HashSet<usize> {
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
    }

    unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, data: isize) -> i32 {
        let set = &mut *(data as *mut std::collections::HashSet<usize>);
        if IsWindowVisible(hwnd) != 0 {
            set.insert(hwnd as usize);
        }
        1
    }

    let mut set = std::collections::HashSet::new();
    unsafe { EnumWindows(cb, &mut set as *mut _ as isize); }
    set
}

#[cfg(target_os = "windows")]
fn get_hwnd_pid(hwnd_usize: usize) -> u32 {
    extern "system" {
        fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, pid: *mut u32) -> u32;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd_usize as *mut _, &mut pid); }
    pid
}

// Returns the lowercase exe filename of the process that owns hwnd.
// Used as a fallback when PID matching fails (e.g. Store apps that host their
// window in a different process than the one we spawned).
#[cfg(target_os = "windows")]
fn get_hwnd_exe(hwnd_usize: usize) -> Option<String> {
    extern "system" {
        fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, pid: *mut u32) -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn QueryFullProcessImageNameW(process: *mut std::ffi::c_void, flags: u32, name: *mut u16, size: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd_usize as *mut _, &mut pid);
        if pid == 0 { return None; }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return None; }
        let mut buf = [0u16; 1024];
        let mut len = 1024u32;
        QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
    }
}

// Searches for a new visible HWND (not in `before`) using a three-tier strategy:
//   1. PID match  — works for regular apps (Command::spawn gives the right PID)
//   2. Exe match  — works for Store apps whose window lives in a hosted process
//   3. Any new    — last-resort on the final poll only
#[cfg(target_os = "windows")]
fn poll_for_new_window(
    before: &std::collections::HashSet<usize>,
    preferred_pid: Option<u32>,
    preferred_exe: Option<&str>,
    polls: usize,
) -> Option<usize> {
    use std::thread;
    use std::time::Duration;
    for i in 0..polls {
        thread::sleep(Duration::from_millis(300));
        let new_hwnds: Vec<usize> = collect_visible_hwnds()
            .into_iter()
            .filter(|h| !before.contains(h))
            .collect();
        if new_hwnds.is_empty() { continue; }
        // Tier 1: PID
        if let Some(pid) = preferred_pid {
            if let Some(&h) = new_hwnds.iter().find(|&&h| get_hwnd_pid(h) == pid) {
                return Some(h);
            }
        }
        // Tier 2: exe name (handles Store apps with hosted window process)
        if let Some(exe) = preferred_exe {
            if let Some(&h) = new_hwnds.iter().find(|&&h| get_hwnd_exe(h).as_deref() == Some(exe)) {
                return Some(h);
            }
        }
        // Tier 3: any new window — only on the last poll
        if i == polls - 1 {
            return new_hwnds.into_iter().next();
        }
    }
    None
}

// Phase 1 runs synchronously on the caller's thread (up to 1.5 s / 5 polls) so
// the next item in the group isn't launched until we've claimed this window —
// preventing concurrent launches from stealing each other's windows.
// Phase 2 continues in a background thread for slow/Store apps that take longer.
#[cfg(target_os = "windows")]
fn position_window_by_snapshot(
    before: std::collections::HashSet<usize>,
    preferred_pid: Option<u32>,
    preferred_exe: Option<String>,
    x: i32, y: i32, w: Option<u32>, h: Option<u32>,
) {
    use std::thread;
    use std::time::Duration;

    // --- Phase 1: synchronous (caller blocks here) ---
    if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), 5) {
        place_window(found as *mut _, x, y, w, h);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(1000));
            place_window(found as *mut _, x, y, w, h);
            thread::sleep(Duration::from_millis(2000));
            place_window(found as *mut _, x, y, w, h);
        });
        return;
    }

    // --- Phase 2: background fallback for slow apps (>1.5 s to show window) ---
    thread::spawn(move || {
        if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), 15) {
            place_window(found as *mut _, x, y, w, h);
            thread::sleep(Duration::from_millis(1000));
            place_window(found as *mut _, x, y, w, h);
            thread::sleep(Duration::from_millis(2000));
            place_window(found as *mut _, x, y, w, h);
        }
    });
}

#[cfg(target_os = "windows")]
fn place_window(hwnd: *mut std::ffi::c_void, x: i32, y: i32, w: Option<u32>, h: Option<u32>) {
    extern "system" {
        fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd: i32) -> i32;
        fn SetWindowPos(
            hwnd: *mut std::ffi::c_void,
            insert: *mut std::ffi::c_void,
            x: i32, y: i32, cx: i32, cy: i32,
            flags: u32,
        ) -> i32;
    }
    const SW_RESTORE: i32 = 9;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    unsafe {
        // Restore first — SetWindowPos silently fails on maximized windows
        ShowWindow(hwnd, SW_RESTORE);
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
            if item.launch_x.is_some() { continue; }

            let url_list: Vec<String> = if !item.urls.is_empty() {
                item.urls.clone()
            } else if let Some(v) = &item.value {
                vec![v.clone()]
            } else {
                continue;
            };

            let browser = item.path.as_deref().or(preferred_browser);
            match browser {
                Some(b) => browser_urls.entry(b.to_string()).or_default().extend(url_list),
                None => fallback_urls.extend(url_list),
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
            #[cfg(target_os = "windows")]
            let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
            let child = cmd.spawn().map_err(|e| format!("Failed to launch app '{}': {}", path, e))?;
            #[cfg(target_os = "windows")]
            if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                let exe = std::path::Path::new(path)
                    .file_name().and_then(|n| n.to_str())
                    .map(|s| s.to_ascii_lowercase());
                position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height);
            }
        }
        ItemType::File | ItemType::Folder => {
            let path = item.path.as_ref().ok_or("Item is missing a path")?;
            #[cfg(target_os = "windows")]
            let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
            open::that(path).map_err(|e| format!("Failed to open '{}': {}", path, e))?;
            #[cfg(target_os = "windows")]
            if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height);
            }
        }
        ItemType::Url => {
            let url_owned: String;
            let url: &str = if !item.urls.is_empty() {
                &item.urls[0]
            } else {
                url_owned = item.value.clone().ok_or("URL item is missing a value")?;
                &url_owned
            };
            let browser = item.path.as_deref().or(preferred_browser.as_deref());

            if let (Some(bp), Some(x), Some(y)) = (browser, item.launch_x, item.launch_y) {
                if is_chromium_based(bp) {
                    // On Windows: use snapshot + SetWindowPos instead of --window-position flags.
                    // Chromium flags are ignored when the browser is already running (the new
                    // process hands off to the existing instance and exits), so SetWindowPos
                    // is the only reliable approach.
                    #[cfg(target_os = "windows")]
                    {
                        let before = collect_visible_hwnds();
                        let child = Command::new(bp)
                            .args(["--new-window", url])
                            .stderr(std::process::Stdio::null())
                            .spawn()
                            .map_err(|e| format!("Failed to open URL: {}", e))?;
                        let exe = std::path::Path::new(bp)
                            .file_name().and_then(|n| n.to_str())
                            .map(|s| s.to_ascii_lowercase());
                        position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height);
                        return Ok(());
                    }
                    // Non-Windows: fall through to the flag-based launch below
                    #[cfg(not(target_os = "windows"))]
                    {
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

            if !item.run_in_terminal {
                #[cfg(target_os = "windows")]
                let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
                open::that(path).map_err(|e| format!("Failed to open script '{}': {}", path, e))?;
                #[cfg(target_os = "windows")]
                if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                    position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height);
                }
                return Ok(());
            }

            // run_in_terminal = true: execute via cmd/powershell (existing behavior)
            #[cfg(target_os = "windows")]
            let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
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
            if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                let launcher_exe = if path.to_lowercase().ends_with(".ps1") {
                    Some("powershell.exe".to_string())
                } else {
                    Some("cmd.exe".to_string())
                };
                position_window_by_snapshot(before, Some(child.id()), launcher_exe, x, y, item.launch_width, item.launch_height);
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
        let item = Item { item_type: ItemType::App, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_url_missing_value_returns_error() {
        let item = Item { item_type: ItemType::Url, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a value"));
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error() {
        let item = Item { item_type: ItemType::Script, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
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
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://a.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://b.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("firefox.exe".to_string()), value: Some("https://c.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["chrome.exe"].len(), 2);
        assert_eq!(map["firefox.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_fall_back_to_preferred_browser() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://x.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, Some("edge.exe"));
        assert_eq!(map["edge.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_with_no_browser_go_to_fallback() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://y.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert!(map.is_empty());
        assert_eq!(fallback.len(), 1);
    }

    #[test]
    fn test_collect_browser_urls_uses_urls_field_when_populated() {
        let items = vec![
            Item {
                item_type: ItemType::Url,
                path: Some("chrome.exe".into()),
                value: Some("https://old.com".into()),
                urls: vec!["https://a.com".into(), "https://b.com".into()],
                icon_data: None, browser_name: None, run_in_terminal: true,
                launch_desktop: None, launch_x: None, launch_y: None,
                launch_width: None, launch_height: None,
            },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["chrome.exe"], vec!["https://a.com", "https://b.com"]);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_collect_browser_urls_falls_back_to_value_when_urls_empty() {
        let items = vec![
            Item {
                item_type: ItemType::Url,
                path: Some("firefox.exe".into()),
                value: Some("https://fallback.com".into()),
                urls: vec![],
                icon_data: None, browser_name: None, run_in_terminal: true,
                launch_desktop: None, launch_x: None, launch_y: None,
                launch_width: None, launch_height: None,
            },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["firefox.exe"], vec!["https://fallback.com"]);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error_regardless_of_run_flag() {
        let item = Item {
            item_type: ItemType::Script,
            path: None, value: None,
            urls: vec![], icon_data: None, browser_name: None,
            run_in_terminal: false,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }
}
