use crate::config::{AppConfig, Item, ItemType};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

// Tracks the most recent "placer generation" for each HWND (by usize address).
// When apply_window_placement claims a window it stores a generation counter.
// The background re-apply thread checks that it is still the current owner
// before re-applying — if a later item has since claimed the same HWND the
// re-apply is skipped rather than overwriting the newer placement.
static CLAIM_GEN: AtomicU64 = AtomicU64::new(0);
static CLAIMS: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn claims() -> &'static Mutex<HashMap<usize, u64>> {
    CLAIMS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
                crate::debug_log::write_debug_log(&format!("LAUNCH HWND 0x{:X} found (PID match) poll={}", h, i));
                return Some(h);
            }
        }
        // Tier 2: exe name (handles Store apps with hosted window process)
        if let Some(exe) = preferred_exe {
            if let Some(&h) = new_hwnds.iter().find(|&&h| get_hwnd_exe(h).as_deref() == Some(exe)) {
                crate::debug_log::write_debug_log(&format!("LAUNCH HWND 0x{:X} found (exe match) poll={}", h, i));
                return Some(h);
            }
        }
        // Tier 3: any new window.
        // Items with no PID/exe hint (File/Folder via open::that) have no specific match
        // to wait for — accept after 2 polls (600ms grace) so a brief transient window
        // doesn't get grabbed on the very first poll.
        // Items with PID/exe hints that haven't matched yet: last poll only (true last resort).
        let tier3_ready = if preferred_pid.is_none() && preferred_exe.is_none() {
            i >= 2
        } else {
            i == polls - 1
        };
        if tier3_ready {
            let h = new_hwnds.into_iter().next();
            crate::debug_log::write_debug_log(&format!("LAUNCH HWND {:?} found (any-new fallback)", h));
            return h;
        }
    }
    None
}

// Positions the window immediately and re-applies after short delays to handle apps
// that reset their own position after initialization.
// Desktop targeting is handled upstream (switch-before-launch in launch_group).
#[cfg(target_os = "windows")]
fn apply_window_placement(found: usize, x: i32, y: i32, w: Option<u32>, h: Option<u32>) {
    use std::thread;
    use std::time::Duration;

    // Walk up to the root-owner window. The PID-matched HWND may be an inner/embedded window
    // (e.g., a conhost pseudo-window inside Windows Terminal). GA_ROOTOWNER (3) follows both
    // the parent and owner chains to find the actual top-level visible frame.
    extern "system" {
        fn GetAncestor(hwnd: *mut std::ffi::c_void, flags: u32) -> *mut std::ffi::c_void;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
    }
    let target = unsafe {
        let root = GetAncestor(found as *mut _, 3 /* GA_ROOTOWNER */);
        if !root.is_null() && IsWindowVisible(root) != 0 && root as usize != found {
            crate::debug_log::write_debug_log(&format!(
                "LAUNCH GetAncestor: 0x{:X} → root 0x{:X} (using root)", found, root as usize
            ));
            root as usize
        } else {
            found
        }
    };

    // Claim this HWND with a unique generation counter. The background re-apply
    // thread checks the counter before each re-apply — if a later item claimed
    // the same HWND in the meantime the re-apply is skipped to avoid overwriting
    // a correct placement made by the subsequent item.
    let gen = CLAIM_GEN.fetch_add(1, Ordering::SeqCst);
    claims().lock().unwrap().insert(target, gen);

    place_window(target as *mut _, x, y, w, h);
    crate::debug_log::write_debug_log(&format!(
        "LAUNCH window HWND 0x{:X} positioned at ({}, {}) {}x{}",
        target, x, y,
        w.unwrap_or(0), h.unwrap_or(0)
    ));
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1000));
        if claims().lock().unwrap().get(&target) == Some(&gen) {
            place_window(target as *mut _, x, y, w, h);
        }
        thread::sleep(Duration::from_millis(2000));
        if claims().lock().unwrap().get(&target) == Some(&gen) {
            place_window(target as *mut _, x, y, w, h);
        }
    });
}

// Phase 1 runs synchronously on the caller's thread, blocking until the window appears.
// This is "launch confirmation": the next item is not launched (and no desktop switch happens)
// until the current item's window has been found. Fast apps exit at poll=0 (~300ms).
// Cold-start Windows Terminal takes 5-6s — with 50 polls × 300ms = 15s ceiling, even
// slow apps are confirmed before we switch away for the next item.
// Phase 2 is a background safety net for apps that somehow exceed the 15s ceiling.
#[cfg(target_os = "windows")]
fn position_window_by_snapshot(
    before: std::collections::HashSet<usize>,
    preferred_pid: Option<u32>,
    preferred_exe: Option<String>,
    x: i32, y: i32, w: Option<u32>, h: Option<u32>,
    virtual_desktop: Option<Vec<u8>>,
) {
    use std::thread;

    let _ = virtual_desktop; // desktop targeting now handled by switch-before-launch in launch_group

    // --- Phase 1: synchronous — wait for launch confirmation ---
    // Items with no PID/exe hint (File/Folder/Script-open) rely on tier-3 any-new,
    // which fires at poll ≥ 2 (600ms). If nothing appears by poll 10 (3s) the file
    // likely opened inside an existing app window (e.g. tabbed Notepad) — bail early.
    // Items with a hint (App/Script-run) keep 50 polls (15s) for slow cold-start apps.
    let phase1_polls = if preferred_pid.is_none() && preferred_exe.is_none() { 10 } else { 50 };
    if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), phase1_polls) {
        apply_window_placement(found, x, y, w, h);
        return;
    }

    // --- Phase 2: background fallback for slow apps ---
    // Only useful for items with a PID/exe hint. File/Folder items have no hint so
    // their any-new fallback in Phase 2 would race with the next item's launch and
    // steal its window. If Phase 1's 50-poll any-new couldn't find a window, Phase 2
    // won't do better — it will just claim whatever new window the next item opens.
    if preferred_pid.is_none() && preferred_exe.is_none() {
        return;
    }
    thread::spawn(move || {
        if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), 15) {
            apply_window_placement(found, x, y, w, h);
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
    // Do NOT call ShowWindow before SetWindowPos.
    // poll_for_new_window already verifies IsWindowVisible, so the window is visible.
    // Any ShowWindow call (SW_RESTORE or SW_SHOWNOACTIVATE) before SetWindowPos triggers
    // Windows 11's snap/restore tracking when the saved height exceeds the screen height,
    // causing the window to revert to its pre-snap position on subsequent calls.
    // SWP_FRAMECHANGED forces the window to recalculate its frame, clearing snap state.
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    unsafe {
        match (w, h) {
            (Some(cw), Some(ch)) => {
                SetWindowPos(hwnd, std::ptr::null_mut(), x, y, cw as i32, ch as i32, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
            _ => {
                SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_cursor_to_monitor_center(monitor_idx: u32) {
    extern "system" {
        fn EnumDisplayMonitors(
            hdc: *mut std::ffi::c_void,
            clip: *const std::ffi::c_void,
            callback: unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *mut std::ffi::c_void,
                *mut [i32; 4],
                isize,
            ) -> i32,
            data: isize,
        ) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }

    struct MonitorTarget {
        idx: u32,
        current: u32,
        x: i32,
        y: i32,
        found: bool,
    }

    unsafe extern "system" fn cb(
        _hmon: *mut std::ffi::c_void,
        _hdc: *mut std::ffi::c_void,
        rect: *mut [i32; 4],
        data: isize,
    ) -> i32 {
        let target = &mut *(data as *mut MonitorTarget);
        if target.current == target.idx {
            let r = &*rect;
            target.x = r[0] + (r[2] - r[0]) / 2;
            target.y = r[1] + (r[3] - r[1]) / 2;
            target.found = true;
        }
        target.current += 1;
        1
    }

    let mut target = MonitorTarget { idx: monitor_idx, current: 0, x: 0, y: 0, found: false };
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            cb,
            &mut target as *mut _ as isize,
        );
        if target.found {
            SetCursorPos(target.x, target.y);
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
            if item.launch_x.is_some() || item.launch_virtual_desktop.is_some() { continue; }

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

#[cfg(target_os = "windows")]
fn shell_execute_runas(path: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut std::ffi::c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut std::ffi::c_void,
        lp_id_list: *mut std::ffi::c_void,
        lp_class: *const u16,
        h_key_class: *mut std::ffi::c_void,
        dw_hot_key: u32,
        _union_padding: u32,
        h_monitor: *mut std::ffi::c_void,
        h_process: *mut std::ffi::c_void,
    }

    extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }

    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
    const SW_SHOWNORMAL: i32 = 1;

    let verb = to_wide("runas");
    let file = to_wide(path);

    let mut info = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        f_mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: std::ptr::null(),
        lp_directory: std::ptr::null(),
        n_show: SW_SHOWNORMAL,
        h_inst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null(),
        h_key_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        _union_padding: 0,
        h_monitor: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        Err(format!(
            "Failed to launch '{}' as administrator (user may have cancelled UAC prompt)",
            path
        ))
    } else {
        Ok(())
    }
}

pub fn launch_group(group_id: &str, config: &AppConfig) -> Result<(), String> {
    let group = config
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| format!("Group '{}' not found", group_id))?;
    crate::debug_log::write_debug_log(&format!(
        "LAUNCH group \"{}\" ({} items)", group.name, group.items.len()
    ));

    // Read current desktop once and track it manually throughout the launch sequence.
    // Re-reading from the registry mid-sequence returns stale data — the registry lags
    // behind keyboard-simulated switches, causing wrong step counts on the 3rd+ item.
    #[cfg(target_os = "windows")]
    let needs_vd = group.items.iter().any(|i| i.launch_virtual_desktop.is_some());
    #[cfg(target_os = "windows")]
    let original_desktop: Option<Vec<u8>> = if needs_vd {
        crate::virtual_desktop::get_current_virtual_desktop_guid()
    } else {
        None
    };
    #[cfg(target_os = "windows")]
    let mut current_desktop: Vec<u8> = original_desktop.clone().unwrap_or_default();

    // Windows: when at least one item has an explicit desktop target, build ordered batches
    // (grouped by target GUID, first-appearance order) and execute each batch with a single
    // desktop switch. Items with no explicit target are folded into the Desktop-1 batch.
    // When no items have explicit targets (needs_vd = false), fall through to the sequential
    // path below — identical to pre-batch behavior, no VD switches.
    #[cfg(target_os = "windows")]
    if needs_vd {
        let desktop_1_guid: Vec<u8> = {
            let desktops = crate::virtual_desktop::get_virtual_desktops();
            desktops.into_iter().next().map(|d| d.guid).unwrap_or_default()
        };
        let mut batches: Vec<(Vec<u8>, Vec<&Item>)> = Vec::new();
        for item in &group.items {
            // URL items without a position or explicit desktop skip the batch loop entirely;
            // they are collected into the multi-tab batch at the end of launch_group.
            let in_batch = !matches!(item.item_type, ItemType::Url)
                || item.launch_x.is_some()
                || item.launch_virtual_desktop.is_some();
            if !in_batch { continue; }
            let target = item.launch_virtual_desktop.clone()
                .unwrap_or_else(|| desktop_1_guid.clone());
            match batches.iter_mut().find(|(guid, _)| *guid == target) {
                Some(batch) => batch.1.push(item),
                None => batches.push((target, vec![item])),
            }
        }
        for (target_guid, items) in &batches {
            if target_guid.as_slice() != current_desktop.as_slice() {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH batch switching desktop ({} item(s))", items.len()
                ));
                let switched = crate::virtual_desktop::switch_virtual_desktop(&current_desktop, target_guid);
                if !switched {
                    crate::debug_log::write_debug_log("LAUNCH batch desktop switch timed out or failed");
                }
                current_desktop = target_guid.clone();
            }
            for item in items {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }

    // Sequential fallback — two cases:
    // (a) Windows + needs_vd = false: no desktop targets, launch items in order, no switching.
    // (b) Non-Windows: desktop targeting not supported, always sequential.
    #[cfg(target_os = "windows")]
    if !needs_vd {
        for item in &group.items {
            if !matches!(item.item_type, ItemType::Url) {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
        for item in &group.items {
            if matches!(item.item_type, ItemType::Url)
                && (item.launch_x.is_some() || item.launch_virtual_desktop.is_some())
            {
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for item in &group.items {
            if !matches!(item.item_type, ItemType::Url) {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
        for item in &group.items {
            if matches!(item.item_type, ItemType::Url)
                && (item.launch_x.is_some() || item.launch_virtual_desktop.is_some())
            {
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }

    // Restore original desktop before batching remaining URLs.
    #[cfg(target_os = "windows")]
    if let Some(ref orig) = original_desktop {
        if orig.as_slice() != current_desktop.as_slice() {
            crate::virtual_desktop::switch_virtual_desktop(&current_desktop, orig);
        }
    }

    // Batch remaining URL items (no position, no target desktop) for multi-tab launch.
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

            // If run_as_admin is requested, use ShellExecuteExW with "runas" verb
            // to trigger UAC elevation. This bypasses Command::spawn() entirely.
            #[cfg(target_os = "windows")]
            if item.run_as_admin {
                return shell_execute_runas(path);
            }

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
                position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
            }
        }
        ItemType::File | ItemType::Folder => {
            let path = item.path.as_ref().ok_or("Item is missing a path")?;
            #[cfg(target_os = "windows")]
            let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
            open::that(path).map_err(|e| format!("Failed to open '{}': {}", path, e))?;
            #[cfg(target_os = "windows")]
            if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
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
                        position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
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
                    position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
                }
                return Ok(());
            }

            // run_in_terminal = true: execute via cmd/powershell in its own console window.
            // CREATE_NEW_CONSOLE ensures the script always gets its own window regardless of
            // whether the parent process has a console (e.g. dev mode vs production build).
            #[cfg(target_os = "windows")]
            let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
            #[cfg(target_os = "windows")]
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
            let child = if path.to_lowercase().ends_with(".ps1") {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    Command::new("powershell")
                        .args(["-ExecutionPolicy", "Bypass", "-File", path])
                        .creation_flags(CREATE_NEW_CONSOLE)
                        .spawn()
                        .map_err(|e| format!("Failed to run PowerShell script: {}", e))?
                }
                #[cfg(not(target_os = "windows"))]
                Command::new("powershell")
                    .args(["-ExecutionPolicy", "Bypass", "-File", path])
                    .spawn()
                    .map_err(|e| format!("Failed to run PowerShell script: {}", e))?
            } else {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    Command::new("cmd")
                        .args(["/K", path])
                        .creation_flags(CREATE_NEW_CONSOLE)
                        .spawn()
                        .map_err(|e| format!("Failed to run script '{}': {}", path, e))?
                }
                #[cfg(not(target_os = "windows"))]
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
                position_window_by_snapshot(before, Some(child.id()), launcher_exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
            }
        }
        ItemType::Steam => {
            let appid = item.value.as_ref().ok_or("Steam item is missing appid")?;

            // Move cursor to chosen monitor center before launch.
            // Most Steam games open on whichever monitor the cursor is on at launch time.
            #[cfg(target_os = "windows")]
            if let Some(monitor_idx) = item.launch_desktop {
                set_cursor_to_monitor_center(monitor_idx);
            }

            open::that(format!("steam://rungameid/{}", appid))
                .map_err(|e| format!("Failed to launch Steam game '{}': {}", appid, e))?;
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
        let item = Item { item_type: ItemType::App, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_url_missing_value_returns_error() {
        let item = Item { item_type: ItemType::Url, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a value"));
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error() {
        let item = Item { item_type: ItemType::Script, path: None, value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None };
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
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://a.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://b.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            Item { item_type: ItemType::Url, path: Some("firefox.exe".to_string()), value: Some("https://c.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["chrome.exe"].len(), 2);
        assert_eq!(map["firefox.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_fall_back_to_preferred_browser() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://x.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
        ];
        let (map, fallback) = collect_browser_urls(&items, Some("edge.exe"));
        assert_eq!(map["edge.exe"].len(), 1);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_url_items_with_no_browser_go_to_fallback() {
        let items = vec![
            Item { item_type: ItemType::Url, path: None, value: Some("https://y.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, run_as_admin: false, launch_virtual_desktop: None, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
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
                run_as_admin: false,
                launch_virtual_desktop: None,
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
                run_as_admin: false,
                launch_virtual_desktop: None,
                launch_desktop: None, launch_x: None, launch_y: None,
                launch_width: None, launch_height: None,
            },
        ];
        let (map, fallback) = collect_browser_urls(&items, None);
        assert_eq!(map["firefox.exe"], vec!["https://fallback.com"]);
        assert!(fallback.is_empty());
    }

    #[test]
    fn test_launch_item_steam_missing_appid_returns_error() {
        let item = Item {
            item_type: ItemType::Steam,
            path: Some("Counter-Strike 2".into()),
            value: None, // missing appid
            urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true,
            run_as_admin: false,
            launch_virtual_desktop: None,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing appid"));
    }

    #[test]
    fn test_launch_item_app_missing_path_still_errors_with_run_as_admin() {
        let item = Item {
            item_type: ItemType::App,
            path: None, value: None,
            urls: vec![], icon_data: None, browser_name: None,
            run_in_terminal: true, run_as_admin: true,
            launch_virtual_desktop: None,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_script_missing_path_returns_error_regardless_of_run_flag() {
        let item = Item {
            item_type: ItemType::Script,
            path: None, value: None,
            urls: vec![], icon_data: None, browser_name: None,
            run_in_terminal: false,
            run_as_admin: false,
            launch_virtual_desktop: None,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let result = launch_item(&item, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing a path"));
    }

    #[test]
    fn test_launch_item_app_with_virtual_desktop_field_no_crash() {
        let item = Item {
            item_type: ItemType::App,
            path: Some("C:\\nonexistent.exe".into()),
            value: None,
            urls: vec![], icon_data: None, browser_name: None,
            run_in_terminal: true, run_as_admin: false,
            launch_virtual_desktop: Some(vec![0u8; 16]),
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let result = launch_item(&item, &None);
        assert!(result.is_err()); // nonexistent exe → error, not crash
    }
}
