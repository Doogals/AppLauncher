//! "Capture Current" — turn the windows that are open right now into group items.
//!
//! The scan itself is `launcher::enumerate_candidate_windows`, which already
//! drops tool windows, DWM-cloaked windows, untitled windows and anything under
//! 50x50. This module adds the classification on top: which process owns each
//! window, what kind of item it should become, and where it currently sits.
//!
//! Nothing here writes to the config. The command returns candidates, the user
//! confirms them in the preview window, and the config editor adds the chosen
//! ones to its in-memory list like any other add path.

use serde::Serialize;

/// One open window, as offered to the user in the capture preview.
#[derive(Debug, Clone, Serialize)]
pub struct CapturedWindow {
    /// Stable only for this scan — the preview uses it to track selection.
    pub hwnd: String,
    /// Window title, used as the row label.
    pub title: String,
    /// Friendly name for the resulting item (exe stem, title-cased-ish).
    pub display_name: String,
    /// Full exe path. Empty when the process could not be opened.
    pub path: String,
    /// "app" | "uwp" | "unknown" — matches ItemType on the JS side.
    pub kind: String,
    /// Why this window cannot be captured, if it cannot. Shown in the preview.
    pub blocked_reason: Option<String>,
    /// True for browsers, so the preview can say tabs are not included.
    pub is_browser: bool,
    pub launch_x: Option<i32>,
    pub launch_y: Option<i32>,
    pub launch_width: Option<u32>,
    pub launch_height: Option<u32>,
    /// 16-byte virtual desktop GUID, if the window is on a resolvable desktop.
    pub launch_virtual_desktop: Option<Vec<u8>>,
}

/// Shell surfaces that are windows but are never a thing anyone wants to launch.
///
/// Matched on lowercase exe filename. `explorer.exe` is deliberately NOT here:
/// a File Explorer folder window is a legitimate capture (as a Folder item once
/// the shell COM work lands), and the desktop/taskbar windows it also owns are
/// already filtered out by `is_real_app_window`.
const NEVER_CAPTURE: &[&str] = &[
    // NOTE: applicationframehost.exe is deliberately NOT here. It hosts the
    // windows of older UWP apps (Calculator, Photos, Settings), and
    // aumid_for_hwnd resolves through it to the real app. A frame-host window
    // that yields no AUMID is dropped in capture_open_windows instead — it
    // would otherwise become an item pointing at the host process.
    "textinputhost.exe",
    "shellexperiencehost.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "systemsettings.exe",
    "lockapp.exe",
    "peopleexperiencehost.exe",
    "widgets.exe",
    "widgetboard.exe",
];

/// Browsers get flagged so the preview can be honest about tabs.
///
/// A window title is the page title, never the URL, and reading the address bar
/// means UI Automation plus per-browser quirks that break whenever a browser
/// reshuffles its accessibility tree. Capturing a browser gives you the browser,
/// not the session.
const BROWSER_EXES: &[&str] = &[
    "chrome.exe", "msedge.exe", "brave.exe", "firefox.exe",
    "opera.exe", "vivaldi.exe", "arc.exe", "librewolf.exe",
];

fn file_name_lower(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Turns `Code.exe` into `Code`. The window title is usually a better label but
/// it changes with whatever document is open, so the exe stem is the stable
/// fallback.
fn display_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Whether a scanned window should be offered at all.
///
/// Pure so it can be tested without a desktop full of windows.
pub fn should_offer(exe_lower: &str, owner_pid: u32, our_pid: u32) -> bool {
    if owner_pid == our_pid {
        // TakeOff's own windows: the widget, this config window, layout editors,
        // detached pills. Capturing the launcher into a group would be absurd.
        return false;
    }
    if exe_lower.is_empty() {
        return true; // unresolved: still offered, but blocked, so the user sees why
    }
    !NEVER_CAPTURE.contains(&exe_lower)
}

pub fn is_browser(exe_lower: &str) -> bool {
    BROWSER_EXES.contains(&exe_lower)
}

/// Store/MSIX apps live under `C:\Program Files\WindowsApps\...`.
///
/// Launching one needs its AUMID, not its exe path, and the AUMID is not
/// recoverable from the exe: the packaged list `apps.rs` builds is keyed by
/// package name and carries no exe paths. Rather than capture something that
/// would fail to launch, these are offered as blocked with a pointer to the
/// Windows Apps picker, which does have the AUMID.
fn is_store_app(path: &str) -> bool {
    path.to_ascii_lowercase().contains(r"\windowsapps\")
}

#[cfg(target_os = "windows")]
pub fn capture_open_windows() -> Vec<CapturedWindow> {
    let our_pid = std::process::id();
    let mut out = Vec::new();

    for hwnd in crate::launcher::enumerate_candidate_windows() {
        let pid = crate::launcher::get_hwnd_pid(hwnd);
        let exe_path = crate::launcher::exe_path_for_hwnd(hwnd);
        let exe_lower = exe_path.as_deref().map(file_name_lower).unwrap_or_default();

        if !should_offer(&exe_lower, pid, our_pid) {
            continue;
        }

        let title = crate::launcher::window_title(hwnd).unwrap_or_default();

        // Unresolvable means elevated, almost always. Offered but blocked, so
        // the user understands why their admin PowerShell is missing rather
        // than assuming capture is broken.
        let Some(path) = exe_path else {
            out.push(CapturedWindow {
                hwnd: hwnd.to_string(),
                display_name: if title.is_empty() { "Unknown".into() } else { title.clone() },
                title,
                path: String::new(),
                kind: "unknown".into(),
                blocked_reason: Some(
                    "Runs as administrator — TakeOff can't read its program path.".into(),
                ),
                is_browser: false,
                launch_x: None, launch_y: None,
                launch_width: None, launch_height: None,
                launch_virtual_desktop: None,
            });
            continue;
        };

        // Packaged apps first. Their exe under WindowsApps\ cannot be launched
        // directly, but GetApplicationUserModelId gives us the AUMID, which
        // launches via shell:AppsFolder exactly like an item added from the
        // Windows Apps picker.
        let aumid = crate::launcher::aumid_for_hwnd(hwnd);

        // A frame-host window with no app behind it is a leftover shell
        // surface, not something to offer. Dropped silently rather than
        // blocked — there is nothing the user could do about it.
        if aumid.is_none() && exe_lower == "applicationframehost.exe" {
            continue;
        }

        // Only unreachable if the app is packaged AND refused to give up its
        // AUMID — rare, and better admitted than captured as a path that would
        // fail to launch.
        if aumid.is_none() && is_store_app(&path) {
            out.push(CapturedWindow {
                hwnd: hwnd.to_string(),
                display_name: display_name_from_path(&path),
                title,
                path: String::new(),
                kind: "unknown".into(),
                blocked_reason: Some(
                    "Microsoft Store app — couldn't read its app ID. Add it from the Windows Apps list instead.".into(),
                ),
                is_browser: false,
                launch_x: None, launch_y: None,
                launch_width: None, launch_height: None,
                launch_virtual_desktop: None,
            });
            continue;
        }

        let placement = crate::launcher::window_placement(hwnd);
        let desktop = crate::virtual_desktop::get_window_virtual_desktop(hwnd as *mut _);

        // For a frame-host app the exe stem is "ApplicationFrameHost", which
        // names the shell rather than the app — the window title ("Calculator",
        // "Photos") is the only useful label there.
        let display_name = match (&aumid, title.trim().is_empty()) {
            (Some(_), false) => title.clone(),
            _ => display_name_from_path(&path),
        };

        out.push(CapturedWindow {
            hwnd: hwnd.to_string(),
            display_name,
            title,
            kind: if aumid.is_some() { "uwp".into() } else { "app".into() },
            path: aumid.unwrap_or(path),
            blocked_reason: None,
            is_browser: is_browser(&exe_lower),
            launch_x: placement.map(|p| p.0),
            launch_y: placement.map(|p| p.1),
            launch_width: placement.map(|p| p.2),
            launch_height: placement.map(|p| p.3),
            launch_virtual_desktop: desktop,
        });
    }

    out
}

#[cfg(not(target_os = "windows"))]
pub fn capture_open_windows() -> Vec<CapturedWindow> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_windows_are_never_offered() {
        assert!(!should_offer("takeoff.exe", 1234, 1234));
        // Same exe name, different process, is someone else's problem — offer it.
        assert!(should_offer("takeoff.exe", 4321, 1234));
    }

    #[test]
    fn shell_surfaces_are_filtered() {
        assert!(!should_offer("textinputhost.exe", 1, 999));
        assert!(!should_offer("searchhost.exe", 1, 999));
        // explorer.exe is deliberately NOT on the denylist - a folder window is
        // a legitimate capture.
        assert!(should_offer("explorer.exe", 1, 999));
        // Nor is applicationframehost.exe: aumid_for_hwnd resolves through it
        // to the hosted UWP app. Windows with no app behind them are dropped
        // later, once that lookup has actually been tried.
        assert!(should_offer("applicationframehost.exe", 1, 999));
    }

    #[test]
    fn unresolved_processes_are_still_offered_so_they_can_be_explained() {
        assert!(should_offer("", 1, 999));
    }

    #[test]
    fn browsers_are_flagged() {
        assert!(is_browser("chrome.exe"));
        assert!(is_browser("msedge.exe"));
        assert!(!is_browser("code.exe"));
    }

    #[test]
    fn display_name_uses_the_exe_stem() {
        assert_eq!(display_name_from_path(r"C:\Program Files\VS\Code.exe"), "Code");
        assert_eq!(display_name_from_path(""), "Unknown");
    }

    #[test]
    fn store_apps_are_detected_by_path() {
        assert!(is_store_app(r"C:\Program Files\WindowsApps\Foo_1.0\foo.exe"));
        assert!(is_store_app(r"c:\program files\windowsapps\bar\bar.exe"));
        assert!(!is_store_app(r"C:\Program Files\Normal\app.exe"));
    }

    #[test]
    fn file_name_lower_handles_odd_input() {
        assert_eq!(file_name_lower(r"C:\X\Foo.EXE"), "foo.exe");
        assert_eq!(file_name_lower(""), "");
    }
}
