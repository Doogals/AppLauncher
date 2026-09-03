use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SteamGame {
    pub appid: String,
    pub name: String,
    pub icon_data: Option<String>,
}

pub fn get_steam_path() -> Option<String> {
    #[cfg(target_os = "windows")]
    return get_steam_path_windows();
    #[cfg(not(target_os = "windows"))]
    return None;
}

#[cfg(target_os = "windows")]
fn get_steam_path_windows() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    fn to_wide(s: &str) -> Vec<u16> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    extern "system" {
        fn RegOpenKeyExW(
            hkey: *mut std::ffi::c_void,
            sub_key: *const u16,
            options: u32,
            desired: u32,
            result: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn RegQueryValueExW(
            hkey: *mut std::ffi::c_void,
            value_name: *const u16,
            reserved: *mut u32,
            typ: *mut u32,
            data: *mut u8,
            data_size: *mut u32,
        ) -> i32;
        fn RegCloseKey(hkey: *mut std::ffi::c_void) -> i32;
    }

    const HKEY_CURRENT_USER: *mut std::ffi::c_void = 0x8000_0001usize as *mut _;
    const KEY_READ: u32 = 0x2_0019;

    unsafe {
        let sub_key = to_wide("Software\\Valve\\Steam");
        let value_name = to_wide("SteamPath");
        let mut hkey: *mut std::ffi::c_void = std::ptr::null_mut();

        if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }

        let mut buf = vec![0u8; 1024];
        let mut size = buf.len() as u32;
        let mut typ = 0u32;

        let ret = RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut typ,
            buf.as_mut_ptr(),
            &mut size,
        );
        RegCloseKey(hkey);

        if ret != 0 { return None; }

        // REG_SZ is UTF-16LE; size includes the null terminator
        let byte_len = (size as usize).min(buf.len()) & !1; // round down to even
        let wchars: Vec<u16> = buf[..byte_len]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let end = wchars.iter().position(|&c| c == 0).unwrap_or(wchars.len());
        Some(OsString::from_wide(&wchars[..end]).to_string_lossy().into_owned())
    }
}

pub fn get_installed_steam_games() -> Vec<SteamGame> {
    let steam_path = match get_steam_path() {
        Some(p) => p,
        None => return vec![],
    };

    let steamapps = std::path::Path::new(&steam_path).join("steamapps");
    let entries = match std::fs::read_dir(&steamapps) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut games: Vec<SteamGame> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("appmanifest_") && name.ends_with(".acf")
        })
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            let (appid, name) = parse_acf(&content)?;
            if is_non_launchable_appid(&appid) { return None; }
            let icon_data = load_icon_base64(&steam_path, &appid);
            Some(SteamGame { appid, name, icon_data })
        })
        .collect();

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

/// Steam depots that appear in appmanifest files but are not launchable.
///
/// These are shared dependencies and tooling installed alongside real games —
/// they have no executable, so `steam://rungameid/<id>` does nothing at all.
/// Steamworks Common Redistributables (228980) is the one that shows up on
/// essentially every Windows install; it is the DirectX / Visual C++ runtime
/// bundle that games pull in.
///
/// Matched on appid rather than name: appids are stable, whereas names are
/// localised and occasionally renamed, and a name-substring rule risks hiding a
/// real game that happens to contain a matching word.
fn is_non_launchable_appid(appid: &str) -> bool {
    matches!(
        appid,
        "228980"    // Steamworks Common Redistributables
        | "1070560" // Steam Linux Runtime
        | "1391110" // Steam Linux Runtime - Soldier
        | "1628350" // Steam Linux Runtime - Sniper
        | "1493710" // Proton Experimental
        | "2180100" // Proton Hotfix
        | "858280"  // Proton 3.7 / compatibility tool
        | "961940"  // Proton 4.2
        | "1054830" // Proton 4.11
        | "1113280" // Proton 5.0
        | "1245040" // Proton 5.13
        | "1420170" // Proton 6.3
        | "1580130" // Proton 7.0
        | "2230260" // Proton 8.0
        | "2805730" // Proton 9.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_steamworks_redistributables() {
        assert!(is_non_launchable_appid("228980"));
    }

    #[test]
    fn keeps_real_games() {
        // Half-Life 2, Counter-Strike 2, Age of Mythology: Retold.
        assert!(!is_non_launchable_appid("220"));
        assert!(!is_non_launchable_appid("730"));
        assert!(!is_non_launchable_appid("1934680"));
    }
}

fn parse_acf(content: &str) -> Option<(String, String)> {
    let mut appid = None;
    let mut name = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if appid.is_none() {
            if let Some(v) = extract_acf_value(trimmed, "appid") {
                appid = Some(v);
            }
        }
        if name.is_none() {
            if let Some(v) = extract_acf_value(trimmed, "name") {
                name = Some(v);
            }
        }
        if appid.is_some() && name.is_some() { break; }
    }
    Some((appid?, name?))
}

fn extract_acf_value(line: &str, key: &str) -> Option<String> {
    let key_pat = format!("\"{}\"", key);
    if !line.to_lowercase().starts_with(&key_pat.to_lowercase()) {
        return None;
    }
    let rest = line[key_pat.len()..].trim();
    if rest.len() >= 2 && rest.starts_with('"') && rest.ends_with('"') {
        Some(rest[1..rest.len() - 1].to_string())
    } else {
        None
    }
}

/// Ranks a file inside an app's librarycache folder as an icon candidate.
/// Lower is better; `None` means "never use this one".
///
/// Steam caches several pieces of art per app and only some are square. A
/// header or hero image dropped into a 20px slot reads as an unrecognisable
/// smear, so those are excluded outright rather than ranked last.
fn rank_icon_file(name: &str) -> Option<u8> {
    let lower = name.to_ascii_lowercase();

    if !(lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".ico"))
    {
        return None;
    }

    const WRONG_SHAPE: [&str; 6] = [
        "header",
        "hero",
        "capsule",
        "library_600x900",
        "page_bg",
        "broadcast",
    ];
    if WRONG_SHAPE.iter().any(|bad| lower.contains(bad)) {
        return None;
    }

    if lower.contains("icon") {
        Some(0)
    } else if lower.contains("logo") {
        Some(1)
    } else {
        // Newer clients name some cached assets by content hash, so an
        // unrecognised name is still worth trying — just not first.
        Some(2)
    }
}

/// Where the icon lives has changed between Steam client versions:
///
///   older     appcache/librarycache/<appid>_icon.jpg
///   current   appcache/librarycache/<appid>/<something>.jpg
///
/// The per-app folder does not use one fixed filename across installs, so
/// after trying the names that are common we scan the folder and take the
/// best-ranked image, breaking ties by file size — an icon is by far the
/// smallest asset Steam caches.
fn load_icon_base64(steam_path: &str, appid: &str) -> Option<String> {
    let cache = std::path::Path::new(steam_path)
        .join("appcache")
        .join("librarycache");

    let read_nonempty = |p: std::path::PathBuf| -> Option<Vec<u8>> {
        let bytes = std::fs::read(p).ok()?;
        if bytes.is_empty() { None } else { Some(bytes) }
    };

    if let Some(bytes) = read_nonempty(cache.join(format!("{}_icon.jpg", appid))) {
        return Some(base64_encode(&bytes));
    }

    let dir = cache.join(appid);
    for name in ["icon.jpg", "icon.png", "logo.png", "logo.jpg"] {
        if let Some(bytes) = read_nonempty(dir.join(name)) {
            return Some(base64_encode(&bytes));
        }
    }

    let mut best: Option<(u8, u64, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().map(|f| f.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(rank) = rank_icon_file(&file_name) else {
            continue;
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
        if size == 0 {
            continue;
        }
        let better = match &best {
            None => true,
            Some((best_rank, best_size, _)) => (rank, size) < (*best_rank, *best_size),
        };
        if better {
            best = Some((rank, size, path));
        }
    }

    let (_, _, path) = best?;
    let bytes = read_nonempty(path)?;
    Some(base64_encode(&bytes))
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acf_extracts_appid_and_name() {
        let content = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"Universe\"\t\"1\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"installdir\"\t\"csgo\"\n}";
        let result = parse_acf(content);
        assert!(result.is_some());
        let (appid, name) = result.unwrap();
        assert_eq!(appid, "730");
        assert_eq!(name, "Counter-Strike 2");
    }

    #[test]
    fn test_parse_acf_returns_none_when_name_missing() {
        let content = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n}";
        assert!(parse_acf(content).is_none());
    }

    #[test]
    fn test_parse_acf_returns_none_on_empty() {
        assert!(parse_acf("").is_none());
    }

    #[test]
    fn test_get_installed_steam_games_returns_vec_without_panic() {
        let games = get_installed_steam_games();
        let _ = games.len();
    }

    #[test]
    fn test_rank_icon_file_prefers_icon_then_logo() {
        assert_eq!(rank_icon_file("icon.jpg"), Some(0));
        assert_eq!(rank_icon_file("730_icon.jpg"), Some(0));
        assert_eq!(rank_icon_file("logo.png"), Some(1));
        assert!(rank_icon_file("icon.jpg") < rank_icon_file("logo.png"));
    }

    #[test]
    fn test_rank_icon_file_accepts_unknown_hash_names_last() {
        assert_eq!(rank_icon_file("8f3c1a90b2.jpg"), Some(2));
    }

    #[test]
    fn test_rank_icon_file_rejects_wrong_shaped_art() {
        for name in [
            "header.jpg",
            "library_hero.jpg",
            "library_600x900.jpg",
            "capsule_616x353.jpg",
            "page_bg_raw.jpg",
        ] {
            assert_eq!(rank_icon_file(name), None, "{name} should be rejected");
        }
    }

    #[test]
    fn test_rank_icon_file_rejects_non_images() {
        assert_eq!(rank_icon_file("icon.json"), None);
        assert_eq!(rank_icon_file("something.txt"), None);
        assert_eq!(rank_icon_file(""), None);
    }

    #[test]
    fn test_load_icon_base64_missing_steam_path_is_none() {
        assert!(load_icon_base64("Z:\\definitely\\not\\steam", "730").is_none());
    }
}
