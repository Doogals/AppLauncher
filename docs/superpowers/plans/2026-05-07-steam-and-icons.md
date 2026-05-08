# Steam Game Launcher + Universal Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Steam game item type (picker, per-monitor launch targeting, game icon) and populate real icons on all item types (App, WinApp, File, Folder, Script).

**Architecture:** New `ItemType::Steam` variant reuses existing `Item` fields (`value` = appid, `path` = name, `icon_data` = base64 JPEG, `launch_desktop` = monitor index). New `steam.rs` reads the registry for Steam path, scans `steamapps/*.acf` files, and reads game icons from Steam's local cache. Monitor targeting uses `SetCursorPos` before launch. Icon population for existing item types wires the already-built `get_file_icon` command into `addItem` and `showWinAppPicker`.

**Tech Stack:** Tauri v2, Rust, Vanilla JS, Win32 API (registry + SetCursorPos)

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Modify | `src-tauri/src/config.rs` | Add `Steam` to `ItemType` enum |
| Create | `src-tauri/src/steam.rs` | Registry read, ACF parse, icon load, `SteamGame` struct |
| Modify | `src-tauri/src/lib.rs` | `mod steam`, register `get_installed_steam_games` |
| Modify | `src-tauri/src/launcher.rs` | `ItemType::Steam` arm + `set_cursor_to_monitor_center` |
| Modify | `src-tauri/src/config.html` | Add `🎮 Steam Game` to Add Item menu |
| Modify | `src/config.js` | `showSteamPicker`, `buildExpandPanel` Steam branch, `renderItems` icons, `addItem` icon population, `showWinAppPicker` icon population |

---

## Task 1: Add Steam to ItemType enum

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write failing test**

Add to the `#[cfg(test)]` block in `src-tauri/src/config.rs`:

```rust
#[test]
fn test_steam_item_type_serializes_correctly() {
    let item = Item {
        item_type: ItemType::Steam,
        path: Some("Counter-Strike 2".into()),
        value: Some("730".into()),
        urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true,
        launch_desktop: Some(0), launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"steam\""), "item_type should serialize as 'steam'");
    let loaded: Item = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.item_type, ItemType::Steam);
    assert_eq!(loaded.value.as_deref(), Some("730"));
    assert_eq!(loaded.path.as_deref(), Some("Counter-Strike 2"));
    assert_eq!(loaded.launch_desktop, Some(0));
}
```

- [ ] **Step 2: Run — expect compile failure**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile error — `Steam` not found in `ItemType`.

- [ ] **Step 3: Add Steam variant to ItemType**

In `src-tauri/src/config.rs`, replace the `ItemType` enum with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    App,
    File,
    Url,
    Folder,
    Script,
    Steam,
}
```

- [ ] **Step 4: Run — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/config.rs
git commit -m "feat: add Steam variant to ItemType"
```

---

## Task 2: Create steam.rs

**Files:**
- Create: `src-tauri/src/steam.rs`

- [ ] **Step 1: Write failing tests**

Create `src-tauri/src/steam.rs` with just the tests first:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SteamGame {
    pub appid: String,
    pub name: String,
    pub icon_data: Option<String>,
}

pub fn get_steam_path() -> Option<String> { todo!() }
pub fn get_installed_steam_games() -> Vec<SteamGame> { todo!() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acf_extracts_appid_and_name() {
        let content = r#""AppState"
{
    "appid"     "730"
    "Universe"  "1"
    "name"      "Counter-Strike 2"
    "installdir" "Counter-Strike Global Offensive"
}"#;
        let result = parse_acf(content);
        assert!(result.is_some());
        let (appid, name) = result.unwrap();
        assert_eq!(appid, "730");
        assert_eq!(name, "Counter-Strike 2");
    }

    #[test]
    fn test_parse_acf_returns_none_when_name_missing() {
        let content = r#""AppState"
{
    "appid"     "730"
}"#;
        assert!(parse_acf(content).is_none());
    }

    #[test]
    fn test_parse_acf_returns_none_on_empty() {
        assert!(parse_acf("").is_none());
    }

    #[test]
    fn test_get_installed_steam_games_returns_vec_without_panic() {
        // Just verify it doesn't panic when Steam is not installed
        let games = get_installed_steam_games();
        assert!(games.len() >= 0); // always true — verifies no panic
    }
}
```

- [ ] **Step 2: Run — expect compile failure on todo!()**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test steam 2>&1 | Select-String "error\[|FAILED|panicked"
```

Expected: tests run but `parse_acf` is not defined (compile error), OR `todo!()` panics.

- [ ] **Step 3: Implement steam.rs fully**

Replace `src-tauri/src/steam.rs` with the complete implementation:

```rust
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
        let _ = games.len(); // verifies no panic
    }
}
```

- [ ] **Step 4: Run tests — expect pass**

```powershell
cargo test steam 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/steam.rs
git commit -m "feat: add steam.rs — registry, ACF parsing, icon loading"
```

---

## Task 3: Register command in lib.rs + Steam arm in launcher.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write failing launcher test**

Add to the `#[cfg(test)]` block in `src-tauri/src/launcher.rs`:

```rust
#[test]
fn test_launch_item_steam_missing_appid_returns_error() {
    let item = Item {
        item_type: ItemType::Steam,
        path: Some("Counter-Strike 2".into()),
        value: None, // missing appid
        urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true,
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let result = launch_item(&item, &None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing appid"));
}
```

- [ ] **Step 2: Run — expect compile failure**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile error — `ItemType::Steam` not handled in `launch_item` match.

- [ ] **Step 3: Add mod steam and get_installed_steam_games to lib.rs**

At the top of `src-tauri/src/lib.rs`, add after the existing `mod` declarations:

```rust
mod steam;
```

Add the command wrapper after the `get_browser_bookmarks` wrapper:

```rust
#[tauri::command]
fn get_installed_steam_games() -> Vec<steam::SteamGame> {
    steam::get_installed_steam_games()
}
```

Add `get_installed_steam_games` to the `generate_handler![]` list (append after `get_file_icon`):

```rust
    get_file_icon,
    get_installed_steam_games,
```

- [ ] **Step 4: Add Steam arm and cursor helper to launcher.rs**

In `src-tauri/src/launcher.rs`, add this helper function before `launch_group` (e.g. after `place_window`):

```rust
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
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), cb, &mut target as *mut _ as isize);
        if target.found {
            SetCursorPos(target.x, target.y);
        }
    }
}
```

In `launch_item`, add the `ItemType::Steam` arm inside the `match &item.item_type` block, after the `ItemType::Script` arm:

```rust
        ItemType::Steam => {
            let appid = item.value.as_ref().ok_or("Steam item is missing appid")?;

            // Move cursor to chosen monitor center before launch.
            // Most Steam games open on whichever monitor the cursor is on at launch.
            #[cfg(target_os = "windows")]
            if let Some(monitor_idx) = item.launch_desktop {
                set_cursor_to_monitor_center(monitor_idx);
            }

            open::that(format!("steam://rungameid/{}", appid))
                .map_err(|e| format!("Failed to launch Steam game '{}': {}", appid, e))?;
        }
```

- [ ] **Step 5: Run all tests — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Build to verify**

```powershell
cargo build 2>&1 | Select-String "error\["
```

Expected: no errors.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/launcher.rs
git commit -m "feat: register get_installed_steam_games command and add Steam launch arm"
```

---

## Task 4: UI — Steam picker, monitor dropdown, universal icons

**Files:**
- Modify: `src-tauri/src/config.html`
- Modify: `src/config.js`

This is the largest task. Work through it step by step.

- [ ] **Step 1: Add Steam to config.html Add Item menu**

In `src-tauri/src/config.html`, find:

```html
        <div class="context-menu-item" data-type="script">⚡ Script (.bat / .ps1)</div>
```

Add directly after it:

```html
        <div class="context-menu-item" data-type="steam">🎮 Steam Game</div>
```

- [ ] **Step 2: Add showSteamPicker function to config.js**

Add `showSteamPicker` after the `showUrlPicker` function (before `fitWindow`):

```js
async function showSteamPicker() {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <input type="text" id="steam-search" placeholder="Search games..." autocomplete="off" />
        <button class="winapp-close" id="steam-close">✕</button>
      </div>
      <div class="winapp-list" id="steam-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  document.getElementById('steam-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let games;
  try {
    games = await invoke('get_installed_steam_games');
  } catch (e) {
    games = [];
  }

  function renderGames(filter) {
    const list = document.getElementById('steam-list');
    if (!list) return;
    const filtered = filter
      ? games.filter(g => g.name.toLowerCase().includes(filter.toLowerCase()))
      : games;

    if (filtered.length === 0) {
      list.innerHTML = games.length === 0
        ? '<div class="winapp-empty">Steam not found or no games installed.</div>'
        : '<div class="winapp-empty">No games match your search.</div>';
      return;
    }

    list.innerHTML = '';
    filtered.forEach(game => {
      const row = document.createElement('div');
      row.className = 'winapp-row';
      row.style.display = 'flex';
      row.style.alignItems = 'center';
      row.style.gap = '8px';

      const iconEl = game.icon_data
        ? `<img src="data:image/jpeg;base64,${game.icon_data}" style="width:20px;height:20px;object-fit:contain;border-radius:3px;flex-shrink:0;" alt="" />`
        : `<span style="width:20px;text-align:center;flex-shrink:0;">🎮</span>`;

      const safeName = game.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      row.innerHTML = `${iconEl}<span>${safeName}</span>`;

      row.addEventListener('click', () => {
        if (!currentItems.some(i => i.item_type === 'steam' && i.value === game.appid)) {
          currentItems.push({
            item_type: 'steam',
            value: game.appid,
            path: game.name,
            icon_data: game.icon_data || null,
            launch_desktop: null,
            launch_x: null, launch_y: null, launch_width: null, launch_height: null,
          });
          renderItems();
        }
        closeModal();
      });
      list.appendChild(row);
    });
  }

  renderGames('');
  const searchInput = document.getElementById('steam-search');
  searchInput.addEventListener('input', (e) => renderGames(e.target.value));
  searchInput.focus();
}
```

- [ ] **Step 3: Wire steam into addItem**

In `src/config.js`, find the `addItem` function. Find this block:

```js
  if (type === 'url') {
    await showUrlPicker();
    return;
  }
```

Add directly after it:

```js
  if (type === 'steam') {
    await showSteamPicker();
    return;
  }
```

- [ ] **Step 4: Populate icon_data for non-URL, non-WinApp items in addItem**

In `src/config.js`, find the end of `addItem` where the item is pushed:

```js
  const newItem = { item_type: type, path: selected, value: null };
  if (type === 'script') newItem.run_in_terminal = true;
  currentItems.push(newItem);
```

Replace with:

```js
  let icon_data = null;
  try { icon_data = await invoke('get_file_icon', { path: selected }); } catch {}
  const newItem = { item_type: type, path: selected, value: null, icon_data };
  if (type === 'script') newItem.run_in_terminal = true;
  currentItems.push(newItem);
```

- [ ] **Step 5: Populate icon_data for WinApp items in showWinAppPicker**

In `src/config.js`, find the row click handler inside `showWinAppPicker`:

```js
      row.addEventListener('click', () => {
        if (!currentItems.some(i => i.path === app.path)) {
          currentItems.push({ item_type: 'app', path: app.path, value: app.args || null });
        }
        renderItems();
        closeModal();
      });
```

Replace with:

```js
      row.addEventListener('click', async () => {
        if (!currentItems.some(i => i.path === app.path)) {
          let icon_data = null;
          try { icon_data = await invoke('get_file_icon', { path: app.path }); } catch {}
          currentItems.push({ item_type: 'app', path: app.path, value: app.args || null, icon_data });
        }
        renderItems();
        closeModal();
      });
```

- [ ] **Step 6: Update renderItems to show icons on all item types and handle Steam**

In `src/config.js`, find the `else` branch in `renderItems` (the non-URL branch):

```js
    } else {
      // Non-URL items: unchanged behavior
      const rawLabel = item.path || '';
      const safeLabel = rawLabel.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const typeIcon = { app: '🖥️', file: '📄', folder: '📁', script: '⚡' }[item.item_type] || '•';
      row.innerHTML = `
        <span>${typeIcon}</span>
        <span class="item-label" title="${safeLabel}">${safeLabel}</span>
        <button class="remove-btn">✕</button>
      `;
      row.querySelector('.remove-btn').onclick = () => { currentItems.splice(idx, 1); renderItems(); };
    }
```

Replace with:

```js
    } else if (item.item_type === 'steam') {
      const gameName = item.path || 'Unknown Game';
      const safeLabel = gameName.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const iconHtml = item.icon_data
        ? `<img src="data:image/jpeg;base64,${item.icon_data}" style="width:16px;height:16px;object-fit:contain;vertical-align:middle;border-radius:2px;" alt="" />`
        : '<span>🎮</span>';
      row.innerHTML = `
        ${iconHtml}
        <span class="item-label" title="${safeLabel}">${safeLabel} <span style="color:#1b9fdb;font-size:10px;font-weight:400;">Steam</span></span>
        <button class="remove-btn">✕</button>
      `;
      row.querySelector('.remove-btn').onclick = () => { currentItems.splice(idx, 1); renderItems(); };

    } else {
      const rawLabel = item.path || '';
      const safeLabel = rawLabel.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const typeEmoji = { app: '🖥️', file: '📄', folder: '📁', script: '⚡' }[item.item_type] || '•';
      const iconHtml = item.icon_data
        ? `<img src="data:image/png;base64,${item.icon_data}" style="width:16px;height:16px;object-fit:contain;vertical-align:middle;" alt="" />`
        : `<span>${typeEmoji}</span>`;
      row.innerHTML = `
        ${iconHtml}
        <span class="item-label" title="${safeLabel}">${safeLabel}</span>
        <button class="remove-btn">✕</button>
      `;
      row.querySelector('.remove-btn').onclick = () => { currentItems.splice(idx, 1); renderItems(); };
    }
```

- [ ] **Step 7: Add Steam monitor dropdown to buildExpandPanel**

In `src/config.js`, find the `buildExpandPanel` function. It starts with:

```js
function buildExpandPanel(item, idx) {
  const panel = document.createElement('div');
  panel.className = 'item-expand';
  const hasCoord = item.launch_x != null && item.launch_y != null;
```

Replace the **entire function** with:

```js
function buildExpandPanel(item, idx) {
  const panel = document.createElement('div');
  panel.className = 'item-expand';

  if (item.item_type === 'steam') {
    // Steam items: monitor dropdown instead of position picker
    const monRow = document.createElement('div');
    monRow.className = 'item-expand-row';
    monRow.innerHTML = `
      <span>Launch on screen</span>
      <select class="steam-monitor-sel" style="flex:1;max-width:180px;background:#1e1e3e;border:1px solid #3a3a6a;border-radius:4px;color:#c8c8d8;font-size:11px;padding:3px 6px;cursor:pointer;">
        <option value="">Any screen (default)</option>
      </select>
    `;
    const sel = monRow.querySelector('.steam-monitor-sel');
    invoke('get_monitors').then(monitors => {
      monitors.forEach(m => {
        const opt = document.createElement('option');
        opt.value = String(m.index);
        opt.textContent = m.is_primary
          ? `Primary (${m.width}×${m.height})`
          : `${m.name} (${m.width}×${m.height})`;
        if (item.launch_desktop !== null && item.launch_desktop !== undefined && item.launch_desktop === m.index) {
          opt.selected = true;
        }
        sel.appendChild(opt);
      });
    }).catch(() => {});
    sel.addEventListener('change', e => {
      currentItems[idx].launch_desktop = e.target.value === '' ? null : parseInt(e.target.value, 10);
    });
    panel.appendChild(monRow);
    return panel;
  }

  // All non-Steam items: position picker
  const hasCoord = item.launch_x != null && item.launch_y != null;
  const hasSize = item.launch_width != null && item.launch_height != null;
  const coordText = hasCoord
    ? `x:${item.launch_x} y:${item.launch_y}${hasSize ? `  ${item.launch_width}\xd7${item.launch_height}` : ''}`
    : 'not set';
  panel.innerHTML = `
    <div class="item-expand-row">
      <span>Launch at</span>
      <span class="coord-display${hasCoord ? '' : ' coord-empty'}">${coordText}</span>
      ${hasCoord ? '<button class="coord-clear" title="Clear">✕</button>' : ''}
      <button class="pick-btn">&#x1f4cd; Pick</button>
    </div>
  `;

  const clearBtn = panel.querySelector('.coord-clear');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      currentItems[idx].launch_x = null;
      currentItems[idx].launch_y = null;
      currentItems[idx].launch_width = null;
      currentItems[idx].launch_height = null;
      renderItems();
    });
  }

  panel.querySelector('.pick-btn').addEventListener('click', () => showPickerWindow(idx));

  if (item.item_type === 'script') {
    const runRow = document.createElement('div');
    runRow.className = 'item-expand-row';
    const checked = item.run_in_terminal !== false ? 'checked' : '';
    runRow.innerHTML = `
      <label class="run-toggle">
        <input type="checkbox" class="run-checkbox" ${checked} />
        &#x25B6; Run via cmd
      </label>
    `;
    runRow.querySelector('.run-checkbox').addEventListener('change', (e) => {
      currentItems[idx].run_in_terminal = e.target.checked;
    });
    panel.appendChild(runRow);
  }

  return panel;
}
```

- [ ] **Step 8: Verify in the app**

Run `npm run tauri dev`. Test:

1. **Add Steam game** — click Add → Steam Game. Picker loads with game icons and names. Click a game — it appears in the list with its icon and a "Steam" badge. The expand panel shows a monitor dropdown (not a position picker).

2. **Monitor dropdown** — if you have multiple monitors, dropdown shows them all. Selecting one saves `launch_desktop` on the item (verify via saving and re-opening the group).

3. **Icons on all items** — add an App, File, Folder, and Script item. Each should show its real extracted icon instead of emoji.

4. **WinApp icons** — open Windows Apps picker, add a Store app. It should show the app's icon in the item row.

5. **Backward compat** — existing items without `icon_data` still show their emoji fallback.

- [ ] **Step 9: Commit**

```powershell
git add src-tauri/src/config.html src/config.js
git commit -m "feat: Steam picker, monitor dropdown, icons on all item types"
```
