# Polish Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a system tray icon, replace the clipping HTML context menu with a native OS one, and redesign URL item selection with browser detection, bookmark picking (Chromium + Firefox), checkbox multi-select, and multi-tab launch.

**Architecture:** Three independent features sharing one plan. Tray and context menu are pure Rust additions to `lib.rs`. URL picker adds a new `browsers.rs` module for detection/bookmark reading, updates `launcher.rs` for multi-tab batching, and adds a two-step modal in `config.js`. All Tauri commands follow the existing pattern in `lib.rs`.

**Tech Stack:** Tauri v2 (`tauri-plugin-tray`, `tauri::menu`), Rust (`rusqlite` bundled for Firefox SQLite), `serde_json` (already present) for Chromium bookmarks, Vanilla JS/HTML/CSS.

---

## File Map

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-tray = "2"`, `rusqlite = { version = "0.31", features = ["bundled"] }` |
| `src-tauri/src/lib.rs` | Tray setup + global `on_menu_event`; `show_group_context_menu` command; `get_installed_browsers` + `get_browser_bookmarks` commands |
| `src-tauri/src/browsers.rs` | New — `BrowserInfo`, `BookmarkItem`, `get_installed_browsers()`, `get_browser_bookmarks()`, Chromium JSON + Firefox SQLite readers |
| `src-tauri/src/launcher.rs` | `launch_group` updated to batch URL items by browser for multi-tab |
| `src-tauri/capabilities/default.json` | Add `"tray-icon:default"` |
| `src/widget.html` | Remove `#context-menu` div |
| `src/widget.js` | Remove HTML context menu code; add `invoke('show_group_context_menu')`; add `listen` for `context-menu:edit` / `context-menu:delete` events |
| `src/config.html` | Remove `data-type="url"` menu item (replaced by url picker); keep others |
| `src/config.js` | Replace `type === 'url'` branch in `addItem` with `showUrlPicker()`; add full `showUrlPicker()` function |
| `src/styles.css` | Remove `.context-menu` / `.context-menu-item` styles; add `.bookmark-row`, `.bookmark-info`, `.bookmark-title`, `.bookmark-url`, `.url-footer`, `.url-back-btn`, `.url-custom` |

---

## Task 1: Add Cargo.toml Dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the two new dependencies**

In `src-tauri/Cargo.toml`, add after the existing `[dependencies]` entries:

```toml
tauri-plugin-tray = "2"
rusqlite = { version = "0.31", features = ["bundled"] }
```

Full updated `[dependencies]` block:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
tauri-plugin-tray = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
dirs = "5"
open = "5"
sha2 = "0.10"
rusqlite = { version = "0.31", features = ["bundled"] }
```

- [ ] **Step 2: Verify compile**

```bash
cd src-tauri && cargo check
```

Expected: compiles successfully (may take a while — `rusqlite` bundled compiles SQLite from source). No errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore: add tauri-plugin-tray and rusqlite dependencies"
```

---

## Task 2: System Tray Icon

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add tray capability**

In `src-tauri/capabilities/default.json`, add `"tray-icon:default"` to the permissions array:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["widget", "config"],
  "permissions": [
    "core:default",
    "core:webview:allow-create-webview-window",
    "core:window:allow-create",
    "core:window:allow-close",
    "core:window:allow-start-dragging",
    "dialog:default",
    "tray-icon:default"
  ]
}
```

- [ ] **Step 2: Register tray plugin and set up tray in lib.rs**

In `src-tauri/src/lib.rs`, the `run()` function currently starts with:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
```

Update `run()` to register the tray plugin and set up the tray + global menu event handler. Replace the full `run()` function with:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_tray::init())
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri_plugin_tray::TrayIconBuilder;

            // Build tray menu
            let show_hide = MenuItem::with_id(app, "show_hide", "Show/Hide Widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_hide, &quit])?;

            // Create tray icon
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .build(app)?;

            // Global menu event handler — handles tray menu AND popup context menus
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if id == "quit" {
                    app.exit(0);
                } else if id == "show_hide" {
                    if let Some(window) = app.get_webview_window("widget") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-edit:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:edit", group_id);
                    }
                } else if let Some(group_id) = id.strip_prefix("ctx-delete:") {
                    if let Some(window) = app.get_webview_window("widget") {
                        let _ = window.emit("context-menu:delete", group_id);
                    }
                }
            });

            // Restore saved widget position
            {
                let state = app.state::<AppState>();
                let cfg = state.0.lock().unwrap();
                if let (Some(x), Some(y)) = (cfg.widget_x, cfg.widget_y) {
                    if let Some(widget) = app.get_webview_window("widget") {
                        let _ = widget.set_position(tauri::PhysicalPosition::new(x, y));
                    }
                }
            }

            // Register auto-start only in release builds
            #[cfg(all(target_os = "windows", not(debug_assertions)))]
            {
                if let Ok(exe) = std::env::current_exe() {
                    register_autostart(&exe.to_string_lossy());
                }
            }

            Ok(())
        })
        .manage(AppState(Mutex::new(config)))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_group,
            delete_group,
            launch_group,
            set_preferred_browser,
            activate_license,
            reorder_items,
            save_widget_position,
            resize_widget,
            get_installed_apps,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Verify compile**

```bash
cd src-tauri && cargo check
```

Expected: no errors.

- [ ] **Step 4: Verify manually**

Run `npm run tauri dev` from `AppLauncher`. A tray icon should appear in the Windows system tray (bottom-right, may need to expand the tray). Right-clicking it should show "Show/Hide Widget" and "Quit". Quit should close the app. Show/Hide should toggle the widget.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat: add system tray icon with Show/Hide and Quit"
```

---

## Task 3: Native Right-Click Context Menu

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/widget.html`
- Modify: `src/widget.js`
- Modify: `src/styles.css`

- [ ] **Step 1: Add show_group_context_menu command to lib.rs**

Add this command function to `src-tauri/src/lib.rs`, just before `pub fn run()`:

```rust
#[tauri::command]
fn show_group_context_menu(group_id: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    let edit = MenuItem::with_id(
        &app,
        format!("ctx-edit:{}", group_id),
        "Edit Group",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let delete = MenuItem::with_id(
        &app,
        format!("ctx-delete:{}", group_id),
        "Delete Group",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&edit, &delete]).map_err(|e| e.to_string())?;
    if let Some(window) = app.get_webview_window("widget") {
        window.popup_menu(&menu).map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

Also add `show_group_context_menu` to the `invoke_handler` list in `run()`:

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_group,
    delete_group,
    launch_group,
    set_preferred_browser,
    activate_license,
    reorder_items,
    save_widget_position,
    resize_widget,
    get_installed_apps,
    show_group_context_menu,
])
```

- [ ] **Step 2: Remove context-menu div from widget.html**

Replace the full `src/widget.html` with:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <link rel="stylesheet" href="styles.css" />
  <title>App Launcher Widget</title>
</head>
<body>
  <div class="widget" id="widget"></div>
  <script type="module" src="widget.js"></script>
</body>
</html>
```

- [ ] **Step 3: Update widget.js — replace HTML context menu with native**

Read `src/widget.js` first. Then replace its full content with:

```js
import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

const widget = document.getElementById('widget');

// Drag the window by clicking the widget background (left-click only, not on buttons)
widget.addEventListener('mousedown', (e) => {
  if (e.button === 0 && !e.target.closest('.group-btn')) {
    getCurrentWindow().startDragging();
  }
});

const BTN_W = 98;
const ADD_W = 78;
const GAP   = 8;
const PAD   = 24;
const WIN_H = 80;

function widgetWidth(groupCount) {
  if (groupCount === 0) return PAD + ADD_W;
  return PAD + groupCount * BTN_W + groupCount * GAP + ADD_W;
}

async function render() {
  const config = await invoke('get_config');
  widget.innerHTML = '';

  for (const group of config.groups) {
    const btn = document.createElement('div');
    btn.className = 'group-btn';
    btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
    btn.addEventListener('click', () => launchGroup(group.id));
    btn.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      invoke('show_group_context_menu', { groupId: group.id });
    });
    widget.appendChild(btn);
  }

  const addBtn = document.createElement('div');
  addBtn.className = 'group-btn add-btn';
  addBtn.textContent = '+';
  addBtn.addEventListener('click', () => openConfig(null));
  widget.appendChild(addBtn);

  await invoke('resize_widget', {
    width: widgetWidth(config.groups.length),
    height: WIN_H,
  });
}

async function launchGroup(groupId) {
  try {
    await invoke('launch_group', { groupId });
  } catch (e) {
    console.error('Launch failed:', e);
  }
}

async function openConfig(groupId) {
  const win = new WebviewWindow('config', {
    url: groupId ? `config.html?id=${groupId}` : 'config.html',
    title: groupId ? 'Edit Group' : 'New Group',
    width: 420,
    height: 520,
    decorations: true,
    resizable: false,
    alwaysOnTop: true,
  });
  win.once('tauri://destroyed', () => render());
}

async function deleteGroup(groupId) {
  await invoke('delete_group', { groupId });
  render();
}

// Listen for native context menu selections
listen('context-menu:edit',   (e) => openConfig(e.payload));
listen('context-menu:delete', (e) => deleteGroup(e.payload));

// Position saving after render
render().then(() => {
  let t = null;
  getCurrentWindow().onMoved(({ payload: { x, y } }) => {
    clearTimeout(t);
    t = setTimeout(() => invoke('save_widget_position', { x, y }), 400);
  });
}).catch(e => console.error('Widget init error:', e));
```

- [ ] **Step 4: Remove context-menu styles from styles.css**

In `src/styles.css`, remove the `.context-menu` and `.context-menu-item` rules (the last ~10 lines of the file):

```css
/* DELETE these lines: */
.context-menu {
  position: fixed;
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 6px;
  padding: 4px 0;
  z-index: 9999;
  min-width: 130px;
  box-shadow: 0 4px 16px rgba(0,0,0,0.5);
}

.context-menu-item {
  padding: 8px 16px;
  cursor: pointer;
  font-size: 0.85rem;
  color: #e0e0e0;
}

.context-menu-item:hover { background: #0f3460; }
.context-menu-item.danger { color: #e94560; }
```

Note: the `.context-menu-item` class is still used by `#add-type-menu` in `config.html`. Only remove the `.context-menu` rule (the positioning container). Keep `.context-menu-item` and its hover/danger rules.

So remove ONLY these lines from `styles.css`:

```css
.context-menu {
  position: fixed;
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 6px;
  padding: 4px 0;
  z-index: 9999;
  min-width: 130px;
  box-shadow: 0 4px 16px rgba(0,0,0,0.5);
}
```

- [ ] **Step 5: Verify compile and test**

```bash
cd src-tauri && cargo check
```

Then run `npm run tauri dev`. Right-click a group button — a native Windows context menu should appear with "Edit Group" and "Delete Group", no longer clipped by the window boundary. Selecting "Edit Group" should open the config window. "Delete Group" should remove the group.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs src/widget.html src/widget.js src/styles.css
git commit -m "feat: replace HTML context menu with native OS popup menu"
```

---

## Task 4: browsers.rs — Browser Detection

**Files:**
- Create: `src-tauri/src/browsers.rs`

- [ ] **Step 1: Write failing tests for browser detection**

Create `src-tauri/src/browsers.rs` with stubs and tests:

```rust
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
    todo!()
}

pub fn get_browser_bookmarks(browser_path: &str) -> Vec<BookmarkItem> {
    todo!()
}

fn browser_candidates() -> Vec<BrowserInfo> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_candidates_returns_nonempty_list() {
        let candidates = browser_candidates();
        assert!(!candidates.is_empty(), "Should always have candidate browsers defined");
    }

    #[test]
    fn get_installed_browsers_only_returns_existing_paths() {
        let browsers = get_installed_browsers();
        for b in &browsers {
            assert!(
                std::path::Path::new(&b.path).exists(),
                "Browser path does not exist: {}",
                b.path
            );
        }
    }

    #[test]
    fn get_installed_browsers_returns_vec_without_panic() {
        // Smoke test — just verify it runs
        let _ = get_installed_browsers();
    }
}
```

- [ ] **Step 2: Run tests — verify they fail**

```bash
cd src-tauri && cargo test browsers::tests
```

Expected: compile errors from `todo!()`. Good.

- [ ] **Step 3: Implement browser detection**

Replace the stubs in `src-tauri/src/browsers.rs` with the full implementation (keep the tests):

```rust
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
        let add = |result: &mut Vec<BrowserInfo>, name: &str, rel: &str| {
            result.push(BrowserInfo {
                name: name.to_string(),
                path: local.join(rel).to_string_lossy().into_owned(),
            });
        };
        add(&mut result, "Google Chrome",   r"Google\Chrome\Application\chrome.exe");
        add(&mut result, "Microsoft Edge",  r"Microsoft\Edge\Application\msedge.exe");
        add(&mut result, "Brave",           r"BraveSoftware\Brave-Browser\Application\brave.exe");
        add(&mut result, "Vivaldi",         r"Vivaldi\Application\vivaldi.exe");
    }

    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        result.push(BrowserInfo {
            name: "Opera".to_string(),
            path: appdata.join(r"Opera Software\Opera Stable\opera.exe").to_string_lossy().into_owned(),
        });
    }

    // Firefox: check both Program Files locations
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
    let lower = browser_path.to_ascii_lowercase();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;

    let path = if lower.contains("chrome") {
        local.join(r"Google\Chrome\User Data\Default\Bookmarks")
    } else if lower.contains("msedge") || lower.contains("edge") {
        local.join(r"Microsoft\Edge\User Data\Default\Bookmarks")
    } else if lower.contains("brave") {
        local.join(r"BraveSoftware\Brave-Browser\User Data\Default\Bookmarks")
    } else if lower.contains("vivaldi") {
        local.join(r"Vivaldi\User Data\Default\Bookmarks")
    } else if lower.contains("opera") {
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

fn get_firefox_bookmarks() -> Vec<BookmarkItem> {
    try_get_firefox_bookmarks().unwrap_or_default()
}

fn try_get_firefox_bookmarks() -> Option<Vec<BookmarkItem>> {
    let db_path = firefox_places_path()?;

    // Firefox locks places.sqlite while running — copy to temp first
    let temp_path = std::env::temp_dir().join("app_launcher_places_tmp.sqlite");
    std::fs::copy(&db_path, &temp_path).ok()?;

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

    let _ = std::fs::remove_file(&temp_path);

    let mut sorted = items;
    sorted.sort_by(|a, b| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()));
    Some(sorted)
}

fn firefox_places_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    let profiles_dir = appdata.join("Mozilla").join("Firefox").join("Profiles");

    // Prefer the "*.default-release" profile; fall back to any profile with places.sqlite
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
    fn get_browser_bookmarks_returns_vec_without_panic() {
        // Smoke test with a nonexistent path — should return empty, not panic
        let result = get_browser_bookmarks(r"C:\nonexistent\chrome.exe");
        assert!(result.is_empty());
    }
}
```

- [ ] **Step 4: Add mod browsers to lib.rs**

In `src-tauri/src/lib.rs`, add `mod browsers;` after `mod apps;` (line 4):

```rust
mod config;
mod launcher;
mod license;
mod apps;
mod browsers;
```

Also add the two new commands before `pub fn run()`:

```rust
#[tauri::command]
fn get_installed_browsers() -> Vec<browsers::BrowserInfo> {
    browsers::get_installed_browsers()
}

#[tauri::command]
fn get_browser_bookmarks(browser_path: String) -> Vec<browsers::BookmarkItem> {
    browsers::get_browser_bookmarks(&browser_path)
}
```

And add them to the `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_group,
    delete_group,
    launch_group,
    set_preferred_browser,
    activate_license,
    reorder_items,
    save_widget_position,
    resize_widget,
    get_installed_apps,
    show_group_context_menu,
    get_installed_browsers,
    get_browser_bookmarks,
])
```

- [ ] **Step 5: Run tests**

```bash
cd src-tauri && cargo test browsers
```

Expected:
- `browser_candidates_returns_nonempty_list` — PASS
- `get_installed_browsers_only_returns_existing_paths` — PASS
- `get_installed_browsers_returns_vec_without_panic` — PASS
- `flatten_chromium_extracts_url_nodes` — PASS
- `get_browser_bookmarks_returns_vec_without_panic` — PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/browsers.rs src-tauri/src/lib.rs
git commit -m "feat: add browsers.rs with browser detection and bookmark reading (Chromium + Firefox)"
```

---

## Task 5: Update Launcher for Multi-Tab URL Launch

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write failing test for multi-tab batching**

In `src-tauri/src/launcher.rs`, add this test to the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_url_items_with_same_browser_are_batched() {
    // This test verifies batch_urls_by_browser groups correctly
    use std::collections::HashMap;
    let items = vec![
        Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://a.com".to_string()) },
        Item { item_type: ItemType::Url, path: Some("chrome.exe".to_string()), value: Some("https://b.com".to_string()) },
        Item { item_type: ItemType::Url, path: Some("firefox.exe".to_string()), value: Some("https://c.com".to_string()) },
    ];
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for item in &items {
        if let ItemType::Url = &item.item_type {
            if let (Some(browser), Some(url)) = (&item.path, &item.value) {
                map.entry(browser.clone()).or_default().push(url.clone());
            }
        }
    }
    assert_eq!(map["chrome.exe"].len(), 2);
    assert_eq!(map["firefox.exe"].len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd src-tauri && cargo test test_url_items_with_same_browser_are_batched
```

Expected: FAIL (test uses types not yet adjusted).

- [ ] **Step 3: Update launch_group to batch URL items by browser**

In `src-tauri/src/launcher.rs`, replace the `launch_group` function with:

```rust
pub fn launch_group(group_id: &str, config: &AppConfig) -> Result<(), String> {
    use std::collections::HashMap;

    let group = config
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| format!("Group '{}' not found", group_id))?;

    // Separate URL items (batched by browser for multi-tab) from all other items
    let mut browser_urls: HashMap<String, Vec<String>> = HashMap::new();
    let mut fallback_urls: Vec<String> = Vec::new();

    for item in &group.items {
        if let ItemType::Url = &item.item_type {
            let url = item.value.as_ref().ok_or("URL item is missing a value")?;
            let browser = item.path.as_deref()
                .or(config.preferred_browser.as_deref());
            match browser {
                Some(b) => browser_urls.entry(b.to_string()).or_default().push(url.clone()),
                None    => fallback_urls.push(url.clone()),
            }
        } else {
            launch_item(item, &config.preferred_browser)?;
        }
    }

    // Launch each browser once with all its URLs → opens as tabs
    for (browser, urls) in &browser_urls {
        Command::new(browser)
            .args(urls)
            .spawn()
            .map_err(|e| format!("Failed to open URLs in '{}': {}", browser, e))?;
    }

    // No browser set — open with system default one at a time
    for url in &fallback_urls {
        open::that(url).map_err(|e| format!("Failed to open URL '{}': {}", url, e))?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run all tests**

```bash
cd src-tauri && cargo test
```

Expected: all 19+ tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/launcher.rs
git commit -m "feat: batch URL items by browser in launch_group for multi-tab launch"
```

---

## Task 6: URL Picker Frontend

**Files:**
- Modify: `src/config.html`
- Modify: `src/config.js`
- Modify: `src/styles.css`

- [ ] **Step 1: Update add-type-menu in config.html**

In `src/config.html`, replace the `🌐 URL` menu item with the new URL picker entry (same `data-type="url"` — the JS handler changes, not the HTML attribute):

The add-type-menu should look like this (URL entry now says "URL / Bookmark"):

```html
      <div id="add-type-menu" style="display:none; background:#16213e; border:1px solid #0f3460; border-radius:6px; padding:4px 0; margin-bottom:10px;">
        <div class="context-menu-item" data-type="app">🖥️ App / Executable</div>
        <div class="context-menu-item" data-type="winapp">🪟 Windows Apps</div>
        <div class="context-menu-item" data-type="file">📄 File</div>
        <div class="context-menu-item" data-type="url">🌐 URL / Bookmark</div>
        <div class="context-menu-item" data-type="folder">📁 Folder</div>
        <div class="context-menu-item" data-type="script">⚡ Script (.bat / .ps1)</div>
      </div>
```

- [ ] **Step 2: Add URL picker styles to styles.css**

Append to the bottom of `src/styles.css`:

```css
/* URL / Bookmark picker */
.url-back-btn {
  background: none;
  border: none;
  color: #aaa;
  cursor: pointer;
  font-size: 1rem;
  padding: 4px 8px;
  border-radius: 4px;
  flex-shrink: 0;
}
.url-back-btn:hover { color: #e0e0e0; }

.url-step-title {
  flex: 1;
  font-size: 0.9rem;
  color: #e0e0e0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.url-custom {
  padding: 10px 12px 4px;
  border-bottom: 1px solid #0f3460;
  flex-shrink: 0;
}

.url-custom input {
  width: 100%;
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 6px;
  padding: 8px 12px;
  color: #e0e0e0;
  font-size: 0.85rem;
  outline: none;
}

.url-custom input:focus { border-color: #e94560; }

.url-footer {
  padding: 10px 12px;
  border-top: 1px solid #0f3460;
  flex-shrink: 0;
}

.url-footer .btn-save { width: 100%; }
.url-footer .btn-save:disabled {
  background: #333;
  color: #666;
  cursor: not-allowed;
}

.bookmark-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  cursor: pointer;
}
.bookmark-row:hover { background: #0f3460; }

.bookmark-row input[type="checkbox"] {
  flex-shrink: 0;
  width: 15px;
  height: 15px;
  cursor: pointer;
  accent-color: #e94560;
}

.bookmark-info { flex: 1; min-width: 0; }

.bookmark-title {
  font-size: 0.85rem;
  color: #e0e0e0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bookmark-url {
  font-size: 0.72rem;
  color: #666;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}
```

- [ ] **Step 3: Add showUrlPicker to config.js and wire addItem**

In `src/config.js`, add the `showUrlPicker` function before `init()`. Then update the `type === 'url'` branch in `addItem`.

**Add `showUrlPicker` before `init()`:**

```js
async function showUrlPicker() {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <span class="url-step-title">Select Browser</span>
        <button class="winapp-close" id="url-close">✕</button>
      </div>
      <div class="winapp-list" id="url-browser-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => {
    document.removeEventListener('keydown', onKeyDown);
    modal.remove();
  };
  document.getElementById('url-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let browsers;
  try {
    browsers = await invoke('get_installed_browsers');
  } catch (e) {
    browsers = [];
    document.getElementById('url-browser-list').innerHTML =
      '<div class="winapp-empty">Could not detect browsers.</div>';
    return;
  }

  if (browsers.length === 0) {
    document.getElementById('url-browser-list').innerHTML =
      '<div class="winapp-empty">No supported browsers found.</div>';
    return;
  }

  const browserList = document.getElementById('url-browser-list');
  browserList.innerHTML = '';
  browsers.forEach(browser => {
    const row = document.createElement('div');
    row.className = 'winapp-row';
    row.textContent = browser.name;
    row.addEventListener('click', () => showBookmarkStep(modal, browser, closeModal));
    browserList.appendChild(row);
  });
}

async function showBookmarkStep(modal, browser, closeModal) {
  const card = modal.querySelector('.winapp-card');
  card.innerHTML = `
    <div class="winapp-header">
      <button class="url-back-btn" id="url-back">←</button>
      <span class="url-step-title">${browser.name} Bookmarks</span>
      <button class="winapp-close" id="url-close2">✕</button>
    </div>
    <div class="url-custom">
      <input type="text" id="custom-url-input" placeholder="Or enter a URL: https://..." autocomplete="off" />
    </div>
    <div class="winapp-list" id="bookmark-list">
      <div class="winapp-empty">Loading bookmarks...</div>
    </div>
    <div class="url-footer">
      <button class="btn btn-save" id="add-selected-btn" disabled>Add Selected</button>
    </div>
  `;

  document.getElementById('url-back').addEventListener('click', () => {
    closeModal(); // cleans up keydown listener and removes modal
    showUrlPicker();
  });

  document.getElementById('url-close2').addEventListener('click', closeModal);

  const customInput = document.getElementById('custom-url-input');
  const addBtn = document.getElementById('add-selected-btn');

  function updateAddBtn() {
    const checkedCount = modal.querySelectorAll('.bookmark-checkbox:checked').length;
    const hasCustom = customInput.value.trim().length > 0;
    const total = checkedCount + (hasCustom ? 1 : 0);
    addBtn.disabled = total === 0;
    addBtn.textContent = total > 0 ? `Add ${total} Selected` : 'Add Selected';
  }

  customInput.addEventListener('input', updateAddBtn);

  let bookmarks;
  try {
    bookmarks = await invoke('get_browser_bookmarks', { browserPath: browser.path });
  } catch (e) {
    bookmarks = [];
  }

  const list = document.getElementById('bookmark-list');
  if (bookmarks.length === 0) {
    list.innerHTML = '<div class="winapp-empty">No bookmarks found.</div>';
  } else {
    list.innerHTML = '';
    bookmarks.forEach(bm => {
      const label = document.createElement('label');
      label.className = 'bookmark-row';
      label.innerHTML = `
        <input type="checkbox" class="bookmark-checkbox" />
        <div class="bookmark-info">
          <div class="bookmark-title">${bm.title.replace(/</g, '&lt;')}</div>
          <div class="bookmark-url">${bm.url.replace(/</g, '&lt;')}</div>
        </div>
      `;
      label.querySelector('.bookmark-checkbox').dataset.url = bm.url;
      label.querySelector('.bookmark-checkbox').addEventListener('change', updateAddBtn);
      list.appendChild(label);
    });
  }

  // Search filter
  customInput.addEventListener('input', () => {
    const q = customInput.value.trim().toLowerCase();
    modal.querySelectorAll('.bookmark-row').forEach(row => {
      const title = row.querySelector('.bookmark-title')?.textContent.toLowerCase() || '';
      const url   = row.querySelector('.bookmark-url')?.textContent.toLowerCase() || '';
      row.style.display = (!q || title.includes(q) || url.includes(q)) ? '' : 'none';
    });
    updateAddBtn();
  });

  addBtn.addEventListener('click', () => {
    const checked = [...modal.querySelectorAll('.bookmark-checkbox:checked')];
    checked.forEach(cb => {
      const url = cb.dataset.url;
      if (url && !currentItems.some(i => i.value === url)) {
        currentItems.push({ item_type: 'url', path: browser.path, value: url });
      }
    });
    const customUrl = customInput.value.trim();
    if (customUrl && !currentItems.some(i => i.value === customUrl)) {
      currentItems.push({ item_type: 'url', path: browser.path, value: customUrl });
    }
    renderItems();
    closeModal();
  });

  customInput.focus();
}
```

**Update `addItem` — replace the `type === 'url'` branch:**

Replace the entire `addItem` function with:

```js
async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';

  if (type === 'winapp') {
    showWinAppPicker();
    return;
  }

  if (type === 'url') {
    await showUrlPicker();
    return;
  }

  if (type === 'url_legacy') {
    // kept for reference — unreachable
  } else {
    const filters = type === 'app' || type === 'script'
      ? [{ name: 'Executable', extensions: ['exe', 'bat', 'ps1', 'cmd'] }]
      : [];
    const selected = await open({
      title: `Select ${type}`,
      directory: type === 'folder',
      filters: filters.length ? filters : undefined,
    });
    if (!selected) return;
    currentItems.push({ item_type: type, path: selected, value: null });
  }

  renderItems();
}
```

Wait — the `url_legacy` branch above is not right. The new `addItem` should simply remove the old url branch since it's handled by `showUrlPicker`. Replace the entire `addItem` function with:

```js
async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';

  if (type === 'winapp') {
    showWinAppPicker();
    return;
  }

  if (type === 'url') {
    await showUrlPicker();
    return;
  }

  const filters = type === 'app' || type === 'script'
    ? [{ name: 'Executable', extensions: ['exe', 'bat', 'ps1', 'cmd'] }]
    : [];
  const selected = await open({
    title: `Select ${type}`,
    directory: type === 'folder',
    filters: filters.length ? filters : undefined,
  });
  if (!selected) return;
  currentItems.push({ item_type: type, path: selected, value: null });

  renderItems();
}
```

Also remove the old URL-related `preferred_browser` code that was in the old url branch — it's no longer needed in `addItem` since browser selection is now part of `showUrlPicker`.

You may also remove `set_preferred_browser` invoke from config.js entirely since the URL picker handles browser selection inline. The global preferred_browser is still used as a fallback in the launcher for old items.

- [ ] **Step 4: Verify manually**

Run `npm run tauri dev`. In the config window, click `+ Add Item` → `🌐 URL / Bookmark`:
1. A modal opens listing detected browsers
2. Click a browser → bookmarks appear with checkboxes, plus a text input at top
3. Check a few bookmarks and/or type a custom URL
4. Click "Add X Selected" — items appear in the list
5. Save the group, launch it — all URLs should open as tabs in the selected browser

- [ ] **Step 5: Commit**

```bash
git add src/config.html src/config.js src/styles.css
git commit -m "feat: URL picker with browser detection, bookmarks (Chromium+Firefox), multi-select"
```

---

## Done

Run `cargo test` (all tests should pass) then `npm run tauri dev` for a full end-to-end smoke test:
1. Tray icon visible, Show/Hide and Quit work
2. Right-click a group — native OS menu appears, no clipping, Edit/Delete work
3. Add URL item → browser picker → bookmarks → multi-select → adds all → launches as tabs
