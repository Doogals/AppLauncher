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
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;

    fn to_wide(s: &str) -> Vec<u16> {
        use std::ffi::OsStr;
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
        let wchars: Vec<u16> = buf[..size as usize]
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
            let icon_data = load_icon_base64(&steam_path, &appid);
            Some(SteamGame { appid, name, icon_data })
        })
        .collect();

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
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

fn load_icon_base64(steam_path: &str, appid: &str) -> Option<String> {
    let path = format!(
        "{}/appcache/librarycache/{}_icon.jpg",
        steam_path, appid
    );
    let bytes = std::fs::read(&path).ok()?;
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
}
