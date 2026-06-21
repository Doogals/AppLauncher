use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub path: String,
    pub args: String,
    /// Pre-fetched icon (base64 PNG), populated for packaged apps since their
    /// icon comes from reading an asset file rather than the usual exe-icon
    /// extraction path. Traditional apps leave this None and fetch on demand.
    #[serde(default)]
    pub icon_data: Option<String>,
    /// True if `path` is an AUMID (PackageFamilyName!AppId) meant to be
    /// launched via shell:AppsFolder, rather than a real exe path.
    #[serde(default)]
    pub is_packaged: bool,
}

// Windows: scan Start Menu .lnk shortcuts via IShellLink
#[cfg(target_os = "windows")]
pub fn get_installed_apps() -> Vec<InstalledApp> {
    use std::collections::HashSet;
    use std::path::PathBuf;

    let should_uninit = unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok()
    };

    let mut lnk_files = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        collect_lnk_files(&path, &mut lnk_files);
    }

    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        let path = PathBuf::from(programdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        collect_lnk_files(&path, &mut lnk_files);
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut apps: Vec<InstalledApp> = lnk_files
        .iter()
        .filter_map(|lnk| {
            let name = lnk.file_stem()?.to_string_lossy().into_owned();
            let (target, args) = resolve_lnk(lnk)?;
            if seen.insert(target.to_ascii_lowercase()) {
                Some(InstalledApp { name, path: target, args, icon_data: None, is_packaged: false })
            } else {
                None
            }
        })
        .collect();

    apps.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    if should_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }

    apps
}

#[cfg(target_os = "windows")]
fn collect_lnk_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
            out.push(path);
        }
    }
}

#[cfg(target_os = "windows")]
fn resolve_lnk(lnk_path: &std::path::Path) -> Option<(String, String)> {
    use windows::{
        core::{Interface, PCWSTR},
        Win32::Storage::FileSystem::WIN32_FIND_DATAW,
        Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, IPersistFile, STGM_READ},
        Win32::UI::Shell::{IShellLinkW, ShellLink},
    };

    unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let wide: Vec<u16> = lnk_path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        persist_file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut buf = [0u16; 1024];
        let mut find_data = WIN32_FIND_DATAW::default();
        shell_link.GetPath(&mut buf, &mut find_data, 0).ok()?;

        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let target = String::from_utf16_lossy(&buf[..end]);

        if target.is_empty() || !target.to_ascii_lowercase().ends_with(".exe") {
            return None;
        }

        let mut arg_buf = [0u16; 1024];
        let _ = shell_link.GetArguments(&mut arg_buf);
        let arg_end = arg_buf.iter().position(|&c| c == 0).unwrap_or(arg_buf.len());
        let args = String::from_utf16_lossy(&arg_buf[..arg_end]).trim().to_owned();

        Some((target, args))
    }
}

// Linux/macOS: Windows Apps picker not supported — return empty
#[cfg(not(target_os = "windows"))]
pub fn get_installed_apps() -> Vec<InstalledApp> {
    vec![]
}

// Curated list of well-known apps people commonly want one-click access to.
// Matched against the exe filename (case-insensitive) of whatever is actually
// installed — we never go digging through Windows usage history, just a
// straightforward "is this on the machine" check via the same Start Menu
// shortcut scan used by the Windows Apps picker.
const CURATED_EXE_NAMES: &[&str] = &[
    // Browsers
    "chrome.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "msedge.exe",
    // AI apps that ship as a normal .exe (packaged/MSIX ones like Claude,
    // ChatGPT, and Copilot are matched separately — see CURATED_PACKAGED_KEYWORDS)
    // Communication
    "discord.exe", "slack.exe", "zoom.exe", "teams.exe", "ms-teams.exe", "skype.exe",
    // Gaming
    "steam.exe", "epicgameslauncher.exe", "battle.net.exe", "galaxyclient.exe", "riotclientservices.exe",
    // Media
    "spotify.exe", "vlc.exe", "itunes.exe",
    // Productivity
    "winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe", "onenote.exe", "notion.exe", "evernote.exe",
    // Dev tools
    "code.exe", "sublime_text.exe", "notepad++.exe", "githubdesktop.exe", "postman.exe", "docker desktop.exe",
    // Creative
    "photoshop.exe", "illustrator.exe", "premiere pro.exe", "obs64.exe", "figma.exe", "blender.exe",
    // Cloud storage
    "dropbox.exe", "onedrive.exe", "googledrivefs.exe",
];

/// Some Electron apps (Discord, Slack, older Teams) use the Squirrel.Windows
/// updater — their Start Menu shortcut points at `Update.exe` with a
/// `--processStart <RealApp>.exe` argument instead of pointing at the real
/// exe directly. Extracts that real exe filename so we can match it against
/// the curated list even though the shortcut's literal target is Update.exe.
fn extract_process_start_target(args: &str) -> Option<String> {
    let lower = args.to_ascii_lowercase();
    let idx = lower.find("--processstart")?;
    let after = lower[idx + "--processstart".len()..].trim_start();
    let token = after.split_whitespace().next()?.trim_matches('"');
    Some(token.to_string())
}

/// Squirrel.Windows installs the real app into versioned "app-X.Y.Z" folders
/// next to Update.exe (e.g. %LOCALAPPDATA%\Discord\app-1.0.9001\Discord.exe).
/// The shortcut's own icon is just Update.exe's generic stub icon, so for
/// icon extraction specifically we resolve through to the real exe and use
/// its actual icon instead. Launch behavior (Update.exe + args) is untouched
/// — this is only used to pick a better icon to display.
pub fn resolve_icon_source_path(path: &str, args: &str) -> String {
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if file_name != "update.exe" {
        return path.to_string();
    }

    let Some(real_exe) = extract_process_start_target(args) else {
        return path.to_string();
    };

    let Some(parent) = std::path::Path::new(path).parent() else {
        return path.to_string();
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return path.to_string();
    };

    let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    for entry in entries.flatten() {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let dir_name = dir_path
            .file_name()
            .map(|f| f.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !dir_name.starts_with("app-") {
            continue;
        }
        let candidate = dir_path.join(&real_exe);
        if !candidate.is_file() {
            continue;
        }
        let modified = std::fs::metadata(&candidate)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if best.as_ref().map_or(true, |(_, t)| modified > *t) {
            best = Some((candidate, modified));
        }
    }

    best.map(|(p, _)| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Keywords matched against a packaged app's Start Menu display name
/// (case-insensitive substring match), for AI apps that ship as MSIX
/// packages rather than a traditional .exe (Claude, ChatGPT, Copilot).
const CURATED_PACKAGED_KEYWORDS: &[&str] = &["claude", "chatgpt", "copilot"];

/// Returns installed apps that match the curated "well-known" list.
/// Cross-references real Start Menu shortcuts (already scanned for the
/// Windows Apps picker) against CURATED_EXE_NAMES — no usage history involved.
/// Also includes packaged/MSIX apps matched separately (see get_packaged_apps).
pub fn get_suggested_apps() -> Vec<InstalledApp> {
    let mut results: Vec<InstalledApp> = get_installed_apps()
        .into_iter()
        .filter(|app| {
            let exe_name = std::path::Path::new(&app.path)
                .file_name()
                .map(|f| f.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();

            if CURATED_EXE_NAMES.contains(&exe_name.as_str()) {
                return true;
            }

            // Squirrel.Windows proxy launch (Update.exe --processStart X.exe)
            if exe_name == "update.exe" {
                if let Some(real_exe) = extract_process_start_target(&app.args) {
                    return CURATED_EXE_NAMES.contains(&real_exe.as_str());
                }
            }

            false
        })
        .collect();

    results.extend(get_packaged_apps());
    results
}

/// Reads the small "app list" logo out of a packaged app's AppxManifest.xml
/// and returns it as a base64 PNG. No GDI/COM image rendering involved — the
/// manifest just points at a real PNG asset already sitting on disk, so this
/// is a plain XML text search + file read.
#[cfg(target_os = "windows")]
fn read_packaged_icon(install_location: &str) -> Option<String> {
    let manifest_path = std::path::Path::new(install_location).join("AppxManifest.xml");
    let manifest = std::fs::read_to_string(&manifest_path).ok()?;

    let logo_rel = extract_xml_attr(&manifest, "Square44x44Logo")
        .or_else(|| extract_xml_attr(&manifest, "Square150x150Logo"))?;
    let logo_rel = logo_rel.replace('/', "\\");
    let logo_path = std::path::Path::new(&logo_rel);

    let dir = match logo_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => std::path::Path::new(install_location).join(p),
        _ => std::path::Path::new(install_location).to_path_buf(),
    };
    let stem = logo_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())?;

    let entries = std::fs::read_dir(&dir).ok()?;
    let mut candidate: Option<std::path::PathBuf> = None;
    for entry in entries.flatten() {
        let p = entry.path();
        let Some(fname) = p.file_name().and_then(|f| f.to_str()) else { continue };
        let fname_lower = fname.to_ascii_lowercase();
        if !fname_lower.starts_with(&stem) || !fname_lower.ends_with(".png") {
            continue;
        }
        // Prefer a modestly-sized variant if multiple scale/target sizes exist
        if fname_lower.contains("scale-100") || fname_lower.contains("targetsize-48") {
            candidate = Some(p);
            break;
        }
        if candidate.is_none() {
            candidate = Some(p);
        }
    }

    let bytes = std::fs::read(candidate?).ok()?;
    Some(base64_encode(&bytes))
}

/// Naive attribute-value extraction from manifest XML. Attribute names in
/// AppxManifest.xml aren't namespace-prefixed even when their element is, so
/// a plain substring search is reliable enough without a full XML parser.
#[cfg(target_os = "windows")]
fn extract_xml_attr(xml: &str, attr_name: &str) -> Option<String> {
    let marker = format!("{}=\"", attr_name);
    let idx = xml.find(&marker)?;
    let start = idx + marker.len();
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string())
}

#[cfg(target_os = "windows")]
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(CHARS[b0 >> 2] as char);
        out.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 { CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[b2 & 63] as char } else { '=' });
    }
    out
}

/// Enumerates packaged/MSIX apps visible in the Start Menu via PowerShell's
/// `Get-StartApps` (no WinRT/COM bindings needed). Packaged apps report an
/// AppID like "PackageFamilyName!AppId" — traditional Win32 apps report a
/// literal exe path instead, which is how the two are told apart here.
/// Cross-references each match's InstallLocation (via Get-AppxPackage) to
/// pull its real icon out of AppxManifest.xml.
#[cfg(target_os = "windows")]
pub fn get_packaged_apps() -> Vec<InstalledApp> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Get-AppxPackage has no -PackageFamilyName parameter (that belongs to a
    // different Appx cmdlet) — filtering by it silently fails to bind and
    // returns nothing, which is why InstallLocation always came back null.
    // Fetch the full package list once and filter via Where-Object instead.
    let script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$allPackages = Get-AppxPackage
$apps = Get-StartApps | Where-Object { $_.AppID -like '*!*' }
$result = foreach ($a in $apps) {
    $pfn = ($a.AppID -split '!')[0]
    $pkg = $allPackages | Where-Object { $_.PackageFamilyName -eq $pfn } | Select-Object -First 1
    [PSCustomObject]@{ Name = $a.Name; AppID = $a.AppID; InstallLocation = $pkg.InstallLocation }
}
$result | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let Ok(output) = output else { return vec![] };
    let Ok(text) = String::from_utf8(output.stdout) else { return vec![] };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else { return vec![] };

    let entries: Vec<serde_json::Value> = match value {
        serde_json::Value::Array(arr) => arr,
        serde_json::Value::Object(_) => vec![value],
        _ => vec![],
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.get("Name")?.as_str()?.to_string();
            let app_id = entry.get("AppID")?.as_str()?.to_string();
            let name_lower = name.to_ascii_lowercase();
            if !CURATED_PACKAGED_KEYWORDS.iter().any(|kw| name_lower.contains(kw)) {
                return None;
            }
            let icon_data = entry
                .get("InstallLocation")
                .and_then(|v| v.as_str())
                .and_then(read_packaged_icon);
            Some(InstalledApp {
                name,
                path: app_id,
                args: String::new(),
                icon_data,
                is_packaged: true,
            })
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn get_packaged_apps() -> Vec<InstalledApp> {
    vec![]
}

#[cfg(test)]
mod suggested_tests {
    use super::*;

    #[test]
    fn extracts_process_start_target() {
        assert_eq!(
            extract_process_start_target("--processStart Discord.exe"),
            Some("discord.exe".to_string())
        );
        assert_eq!(
            extract_process_start_target("--processStart \"Discord.exe\" --process-start-args \"\""),
            Some("discord.exe".to_string())
        );
        assert_eq!(extract_process_start_target("--silent"), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_installed_apps_returns_vec_without_panic() {
        let apps = get_installed_apps();
        #[cfg(target_os = "windows")]
        for app in &apps {
            assert!(
                app.path.to_ascii_lowercase().ends_with(".exe"),
                "Non-exe path: {}",
                app.path
            );
        }
        #[cfg(not(target_os = "windows"))]
        assert!(apps.is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn collect_lnk_files_finds_lnk_and_ignores_others() {
        use std::fs;
        let dir = std::env::temp_dir().join("app_launcher_lnk_test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.lnk"), b"").unwrap();
        fs::write(dir.join("readme.txt"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "app.lnk");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn collect_lnk_files_recurses_into_subdirs() {
        use std::fs;
        let dir = std::env::temp_dir().join("app_launcher_lnk_recurse_test");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.lnk"), b"").unwrap();
        fs::write(sub.join("b.lnk"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);
        assert_eq!(found.len(), 2);

        fs::remove_dir_all(&dir).unwrap();
    }
}
