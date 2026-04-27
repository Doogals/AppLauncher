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

fn browser_candidates() -> Vec<BrowserInfo> {
    let mut result = Vec::new();

    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        result.push(BrowserInfo {
            name: "Google Chrome".to_string(),
            path: local
                .join(r"Google\Chrome\Application\chrome.exe")
                .to_string_lossy()
                .into_owned(),
        });
        result.push(BrowserInfo {
            name: "Microsoft Edge".to_string(),
            path: local
                .join(r"Microsoft\Edge\Application\msedge.exe")
                .to_string_lossy()
                .into_owned(),
        });
        result.push(BrowserInfo {
            name: "Brave".to_string(),
            path: local
                .join(r"BraveSoftware\Brave-Browser\Application\brave.exe")
                .to_string_lossy()
                .into_owned(),
        });
        result.push(BrowserInfo {
            name: "Vivaldi".to_string(),
            path: local
                .join(r"Vivaldi\Application\vivaldi.exe")
                .to_string_lossy()
                .into_owned(),
        });
    }

    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        result.push(BrowserInfo {
            name: "Opera".to_string(),
            path: appdata
                .join(r"Opera Software\Opera Stable\opera.exe")
                .to_string_lossy()
                .into_owned(),
        });
    }

    for path in [
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
    ] {
        if Path::new(path).exists() {
            result.push(BrowserInfo {
                name: "Mozilla Firefox".to_string(),
                path: path.to_string(),
            });
            break;
        }
    }

    result
}

fn chromium_bookmark_path(browser_path: &str) -> Option<PathBuf> {
    let exe = Path::new(browser_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let local  = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
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
            let title = node
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let url = node
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
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

fn get_firefox_bookmarks() -> Vec<BookmarkItem> {
    try_get_firefox_bookmarks().unwrap_or_default()
}

fn try_get_firefox_bookmarks() -> Option<Vec<BookmarkItem>> {
    let db_path = firefox_places_path()?;

    // Use a PID-unique name to avoid collisions; copy because Firefox locks the file
    let temp_path = std::env::temp_dir()
        .join(format!("app_launcher_places_{}.sqlite", std::process::id()));
    std::fs::copy(&db_path, &temp_path).ok()?;

    let result = (|| -> Option<Vec<BookmarkItem>> {
        let conn = rusqlite::Connection::open_with_flags(
            &temp_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()?;

        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(NULLIF(b.title,''), NULLIF(p.title,''), p.url), p.url
                 FROM moz_bookmarks b
                 JOIN moz_places p ON b.fk = p.id
                 WHERE b.type = 1 AND p.url NOT LIKE 'place:%'",
            )
            .ok()?;

        let items: Vec<BookmarkItem> = stmt
            .query_map([], |row| {
                Ok(BookmarkItem {
                    title: row.get(0).unwrap_or_default(),
                    url:   row.get(1).unwrap_or_default(),
                })
            })
            .ok()?
            .flatten()
            .collect();

        let mut sorted = items;
        sorted.sort_by(|a, b| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()));
        Some(sorted)
    })();

    // Always clean up temp file, regardless of whether the query succeeded
    let _ = std::fs::remove_file(&temp_path);
    result
}

fn firefox_places_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    let profiles_dir = appdata.join("Mozilla").join("Firefox").join("Profiles");

    let mut fallback: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&profiles_dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let places = path.join("places.sqlite");
        if !places.exists() {
            continue;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        if name.ends_with(".default-release") {
            return Some(places);
        }
        fallback = Some(places);
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_candidates_returns_nonempty_list() {
        let candidates = browser_candidates();
        assert!(!candidates.is_empty());
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
        assert_eq!(out[1].title, "https://bare.com"); // empty name falls back to url
        assert_eq!(out[2].title, "Nested");
    }

    #[test]
    fn get_browser_bookmarks_returns_empty_for_nonexistent_path() {
        let result = get_browser_bookmarks(r"C:\nonexistent\chrome.exe");
        assert!(result.is_empty());
    }
}
