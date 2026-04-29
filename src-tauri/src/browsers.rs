use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct BrowserInfo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BookmarkItem {
    pub title: String,
    pub url: String,
}

pub fn get_installed_browsers() -> Vec<BrowserInfo> {
    browser_candidates()
        .into_iter()
        .filter(|b| Path::new(&b.path).exists())
        .collect()
}

pub fn get_browser_bookmarks(browser_path: &str) -> Vec<BookmarkItem> {
    let lower = browser_path.to_ascii_lowercase();
    if lower.contains("firefox") {
        get_firefox_bookmarks()
    } else {
        get_chromium_bookmarks(browser_path)
    }
}

// ── Browser candidates ───────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn browser_candidates() -> Vec<BrowserInfo> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();

    let defs: Vec<(&str, Vec<String>)> = vec![
        ("Google Chrome", vec![
            local.join(r"Google\Chrome\Application\chrome.exe").to_string_lossy().into_owned(),
            r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
        ]),
        ("Microsoft Edge", vec![
            local.join(r"Microsoft\Edge\Application\msedge.exe").to_string_lossy().into_owned(),
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".to_string(),
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe".to_string(),
        ]),
        ("Brave", vec![
            local.join(r"BraveSoftware\Brave-Browser\Application\brave.exe").to_string_lossy().into_owned(),
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe".to_string(),
            r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe".to_string(),
        ]),
        ("Vivaldi", vec![
            local.join(r"Vivaldi\Application\vivaldi.exe").to_string_lossy().into_owned(),
            r"C:\Program Files\Vivaldi\Application\vivaldi.exe".to_string(),
        ]),
        ("Opera", vec![
            appdata.join(r"Opera Software\Opera Stable\opera.exe").to_string_lossy().into_owned(),
            r"C:\Program Files\Opera\opera.exe".to_string(),
        ]),
        ("Mozilla Firefox", vec![
            r"C:\Program Files\Mozilla Firefox\firefox.exe".to_string(),
            r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe".to_string(),
        ]),
    ];

    candidates_from_defs(defs)
}

#[cfg(target_os = "linux")]
fn browser_candidates() -> Vec<BrowserInfo> {
    let defs: Vec<(&str, Vec<String>)> = vec![
        ("Google Chrome", vec![
            "/usr/bin/google-chrome".to_string(),
            "/usr/bin/google-chrome-stable".to_string(),
            "/opt/google/chrome/chrome".to_string(),
        ]),
        ("Chromium", vec![
            "/usr/bin/chromium".to_string(),
            "/usr/bin/chromium-browser".to_string(),
        ]),
        ("Mozilla Firefox", vec![
            "/usr/bin/firefox".to_string(),
            "/usr/lib/firefox/firefox".to_string(),
        ]),
        ("Brave", vec![
            "/usr/bin/brave-browser".to_string(),
            "/usr/bin/brave-browser-stable".to_string(),
            "/opt/brave.com/brave/brave".to_string(),
        ]),
        ("Vivaldi", vec![
            "/usr/bin/vivaldi".to_string(),
            "/usr/bin/vivaldi-stable".to_string(),
        ]),
        ("Opera", vec![
            "/usr/bin/opera".to_string(),
        ]),
    ];

    candidates_from_defs(defs)
}

#[cfg(target_os = "macos")]
fn browser_candidates() -> Vec<BrowserInfo> {
    let defs: Vec<(&str, Vec<String>)> = vec![
        ("Google Chrome", vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(),
        ]),
        ("Safari", vec![
            "/Applications/Safari.app/Contents/MacOS/Safari".to_string(),
        ]),
        ("Mozilla Firefox", vec![
            "/Applications/Firefox.app/Contents/MacOS/firefox".to_string(),
        ]),
        ("Brave Browser", vec![
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser".to_string(),
        ]),
        ("Vivaldi", vec![
            "/Applications/Vivaldi.app/Contents/MacOS/Vivaldi".to_string(),
        ]),
        ("Opera", vec![
            "/Applications/Opera.app/Contents/MacOS/Opera".to_string(),
        ]),
    ];

    candidates_from_defs(defs)
}

fn candidates_from_defs(defs: Vec<(&str, Vec<String>)>) -> Vec<BrowserInfo> {
    let mut result = Vec::new();
    for (name, paths) in &defs {
        if let Some(path) = paths.iter().find(|p| Path::new(p.as_str()).exists()) {
            result.push(BrowserInfo {
                name: name.to_string(),
                path: path.clone(),
            });
        }
    }
    result
}

// ── Chromium bookmarks ───────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn chromium_bookmark_path(browser_path: &str) -> Option<PathBuf> {
    let exe = Path::new(browser_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let local   = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;

    let path = if exe.contains("chrome") {
        local.join(r"Google\Chrome\User Data\Default\Bookmarks")
    } else if exe.contains("msedge") || exe.contains("edge") {
        local.join(r"Microsoft\Edge\User Data\Default\Bookmarks")
    } else if exe.contains("brave") {
        local.join(r"BraveSoftware\Brave-Browser\User Data\Default\Bookmarks")
    } else if exe.contains("vivaldi") {
        local.join(r"Vivaldi\User Data\Default\Bookmarks")
    } else if exe.contains("opera") {
        appdata.join(r"Opera Software\Opera Stable\Bookmarks")
    } else {
        return None;
    };

    if path.exists() { Some(path) } else { None }
}

#[cfg(target_os = "linux")]
fn chromium_bookmark_path(browser_path: &str) -> Option<PathBuf> {
    let lower = browser_path.to_ascii_lowercase();
    let home = dirs::home_dir()?;
    let config = home.join(".config");

    let path = if lower.contains("chrome") && !lower.contains("chromium") {
        config.join("google-chrome/Default/Bookmarks")
    } else if lower.contains("chromium") {
        config.join("chromium/Default/Bookmarks")
    } else if lower.contains("brave") {
        config.join("BraveSoftware/Brave-Browser/Default/Bookmarks")
    } else if lower.contains("vivaldi") {
        config.join("vivaldi/Default/Bookmarks")
    } else if lower.contains("opera") {
        config.join("opera/Default/Bookmarks")
    } else {
        return None;
    };

    if path.exists() { Some(path) } else { None }
}

#[cfg(target_os = "macos")]
fn chromium_bookmark_path(browser_path: &str) -> Option<PathBuf> {
    let lower = browser_path.to_ascii_lowercase();
    let support = dirs::data_dir()?; // ~/Library/Application Support

    let path = if lower.contains("chrome") && !lower.contains("chromium") {
        support.join("Google/Chrome/Default/Bookmarks")
    } else if lower.contains("brave") {
        support.join("BraveSoftware/Brave-Browser/Default/Bookmarks")
    } else if lower.contains("vivaldi") {
        support.join("Vivaldi/Default/Bookmarks")
    } else if lower.contains("opera") {
        support.join("com.operasoftware.Opera/Default/Bookmarks")
    } else {
        return None;
    };

    if path.exists() { Some(path) } else { None }
}

fn get_chromium_bookmarks(browser_path: &str) -> Vec<BookmarkItem> {
    let path = match chromium_bookmark_path(browser_path) {
        Some(p) => p,
        None => return vec![],
    };
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    let json: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let mut items = Vec::new();
    if let Some(roots) = json.get("roots").and_then(|r| r.as_object()) {
        for root_value in roots.values() {
            flatten_chromium(root_value, &mut items);
        }
    }
    items.sort_by(|a, b| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()));
    items
}

fn flatten_chromium(node: &serde_json::Value, out: &mut Vec<BookmarkItem>) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("url") => {
            let title = node.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let url   = node.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
            if !url.is_empty() {
                out.push(BookmarkItem {
                    title: if title.is_empty() { url.clone() } else { title },
                    url,
                });
            }
        }
        Some("folder") => {
            if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    flatten_chromium(child, out);
                }
            }
        }
        _ => {}
    }
}

// ── Firefox bookmarks ────────────────────────────────────────────────────────

fn get_firefox_bookmarks() -> Vec<BookmarkItem> {
    try_get_firefox_bookmarks().unwrap_or_default()
}

fn try_get_firefox_bookmarks() -> Option<Vec<BookmarkItem>> {
    let db_path = firefox_places_path()?;

    let temp_path = std::env::temp_dir()
        .join(format!("app_launcher_places_{}.sqlite", std::process::id()));
    std::fs::copy(&db_path, &temp_path).ok()?;

    let result = (|| -> Option<Vec<BookmarkItem>> {
        let conn = rusqlite::Connection::open_with_flags(
            &temp_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).ok()?;

        let mut stmt = conn.prepare(
            "SELECT COALESCE(NULLIF(b.title,''), NULLIF(p.title,''), p.url), p.url
             FROM moz_bookmarks b
             JOIN moz_places p ON b.fk = p.id
             WHERE b.type = 1 AND p.url NOT LIKE 'place:%'",
        ).ok()?;

        let items: Vec<BookmarkItem> = stmt
            .query_map([], |row| Ok(BookmarkItem {
                title: row.get(0).unwrap_or_default(),
                url:   row.get(1).unwrap_or_default(),
            }))
            .ok()?
            .flatten()
            .collect();

        let mut sorted = items;
        sorted.sort_by(|a, b| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()));
        Some(sorted)
    })();

    let _ = std::fs::remove_file(&temp_path);
    result
}

#[cfg(target_os = "windows")]
fn firefox_places_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    find_firefox_profile(appdata.join("Mozilla").join("Firefox").join("Profiles"))
}

#[cfg(target_os = "linux")]
fn firefox_places_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    find_firefox_profile(home.join(".mozilla").join("firefox"))
}

#[cfg(target_os = "macos")]
fn firefox_places_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    find_firefox_profile(
        home.join("Library").join("Application Support").join("Firefox").join("Profiles")
    )
}

fn find_firefox_profile(profiles_dir: PathBuf) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&profiles_dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let places = path.join("places.sqlite");
        if !places.exists() { continue; }
        let name = path.file_name()?.to_string_lossy().into_owned();
        if name.ends_with(".default-release") {
            return Some(places);
        }
        fallback = Some(places);
    }
    fallback
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_candidates_returns_vec_without_panic() {
        let _ = browser_candidates();
    }

    #[test]
    fn get_installed_browsers_only_returns_existing_paths() {
        let browsers = get_installed_browsers();
        for b in &browsers {
            assert!(
                Path::new(&b.path).exists(),
                "Browser path does not exist: {}",
                b.path
            );
        }
    }

    #[test]
    fn get_installed_browsers_returns_vec_without_panic() {
        let _ = get_installed_browsers();
    }

    #[test]
    fn flatten_chromium_extracts_url_nodes() {
        let json: serde_json::Value = serde_json::json!({
            "type": "folder",
            "children": [
                { "type": "url", "name": "Google", "url": "https://google.com" },
                { "type": "url", "name": "", "url": "https://bare.com" },
                {
                    "type": "folder",
                    "children": [
                        { "type": "url", "name": "Nested", "url": "https://nested.com" }
                    ]
                }
            ]
        });
        let mut out = Vec::new();
        flatten_chromium(&json, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].title, "Google");
        assert_eq!(out[1].title, "https://bare.com");
        assert_eq!(out[2].title, "Nested");
    }

    #[test]
    fn get_browser_bookmarks_returns_empty_for_nonexistent_path() {
        let result = get_browser_bookmarks("/nonexistent/browser");
        assert!(result.is_empty());
    }
}
