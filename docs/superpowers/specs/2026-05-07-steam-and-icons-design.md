# Steam Game Launcher + Universal Item Icons — Design Spec

**Date:** 2026-05-07
**Project:** App Launcher (Tauri v2 + Rust + Vanilla JS)
**Goal:** Two features — icons on all item types, and a new Steam game item type with monitor targeting.

---

## Feature A — Icons on All Item Types

### Problem
Only URL items currently show a real icon (browser icon extracted via `get_file_icon`). All other item types show generic emoji (🖥️, 📄, 📁, ⚡). The `icon_data: Option<String>` field already exists on every `Item` — it just isn't populated for non-URL items.

### Solution
When any non-URL item is added (App, WinApp, File, Folder, Script), immediately call `get_file_icon(path)` and store the result in `item.icon_data`. `SHGetFileInfoW` works on any file path including folders, scripts, and executables.

`renderItems` already checks `item.icon_data` for URL items — extend this to all item types. Emoji fallback remains for items where icon extraction fails (e.g. path not found).

### Files Changed
- `src/config.js` — populate `icon_data` in `addItem` for all non-URL, non-WinApp types; populate for WinApp items in `showWinAppPicker`; extend `renderItems` icon logic to all item types

---

## Feature B — Steam Game Item Type

### Overview
New `ItemType::Steam` variant. Users pick from their installed Steam games in a dedicated picker. Games launch via `steam://rungameid/APPID`. Each item stores the app ID, game name, and game icon. A monitor dropdown replaces the position/size picker in the expand panel.

### Data Model

**`src-tauri/src/config.rs` — `ItemType` enum:** Add `Steam` variant:
```rust
pub enum ItemType {
    App,
    File,
    Url,
    Folder,
    Script,
    Steam,
}
```

**`Item` struct:** No new fields. Reuse existing fields:
- `value: Option<String>` — stores the Steam App ID (e.g. `"730"`)
- `path: Option<String>` — stores the game name (used as display label)
- `icon_data: Option<String>` — base64-encoded JPEG icon from Steam's local cache
- `launch_desktop: Option<u32>` — 0-based monitor index for launch targeting (None = any/default)

### New Module: steam.rs

**`src-tauri/src/steam.rs`** — two public functions:

```rust
pub struct SteamGame {
    pub appid: String,
    pub name: String,
    pub icon_data: Option<String>, // base64 JPEG from librarycache
}

pub fn get_installed_steam_games() -> Vec<SteamGame>
pub fn get_steam_path() -> Option<String>
```

**`get_steam_path()`** — reads `HKEY_CURRENT_USER\Software\Valve\Steam\SteamPath` from the Windows registry. Returns `None` if Steam is not installed. Non-Windows: always returns `None`.

**`get_installed_steam_games()`** — calls `get_steam_path()`, then:
1. Scans `{steam_path}/steamapps/appmanifest_*.acf` files
2. Parses `"appid"` and `"name"` key-value pairs from each file (ACF format: `"key"  "value"` per line)
3. For each game, reads `{steam_path}/appcache/librarycache/{appid}_icon.jpg` and base64-encodes it (plain file read — no Win32 API needed)
4. Returns games sorted alphabetically by name
5. Returns empty vec if Steam not installed or steamapps folder not found

**ACF parsing:** Simple line-by-line scan — no full ACF parser needed. Just extract lines matching `"appid"` and `"name"` keys.

### Launcher Changes

**`src-tauri/src/launcher.rs` — `launch_item` Steam arm:**
```rust
ItemType::Steam => {
    let appid = item.value.as_ref().ok_or("Steam item is missing appid")?;

    // Move cursor to target monitor center before launch — many games
    // open on whichever monitor the cursor is on at launch time.
    #[cfg(target_os = "windows")]
    if let Some(monitor_idx) = item.launch_desktop {
        if let Ok(monitors) = get_monitors_list() {
            if let Some(m) = monitors.get(monitor_idx as usize) {
                set_cursor_to_monitor_center(m);
            }
        }
    }

    open::that(format!("steam://rungameid/{}", appid))
        .map_err(|e| format!("Failed to launch Steam game: {}", e))?;
}
```

`set_cursor_to_monitor_center` uses `SetCursorPos` Win32 API with the monitor's center coordinate. `get_monitors_list` reuses the existing monitor detection logic from `lib.rs`.

### New Tauri Commands

In `src-tauri/src/lib.rs`:
```rust
#[tauri::command]
fn get_installed_steam_games() -> Vec<steam::SteamGame>
```
Add to `generate_handler![]`.

### UI Changes

**`src-tauri/src/config.html`** — add to the Add Item menu:
```html
<div class="context-menu-item" data-type="steam">🎮 Steam Game</div>
```

**`src/config.js` — `addItem('steam')`:** Opens `showSteamPicker()`.

**`src/config.js` — `showSteamPicker()`:** Modal matching the WinApp picker pattern:
- Search box to filter games
- Scrollable list — each row: game icon (`<img>` from base64) + game name
- Click a row to add the item: `{ item_type: 'steam', value: game.appid, path: game.name, icon_data: game.icon_data, launch_desktop: null }`
- Loading/empty states ("Steam not installed" if games list is empty)

**`src/config.js` — `buildExpandPanel`:** For Steam items, replace the position picker row with a monitor dropdown:
```html
<div class="item-expand-row">
  <span>Launch on screen</span>
  <select class="monitor-select">
    <option value="">Any screen (default)</option>
    <!-- populated dynamically from get_monitors -->
  </select>
</div>
```
Dropdown is populated by calling `invoke('get_monitors')` and rendering one option per monitor (Primary, Monitor 2, etc.). Change handler updates `currentItems[idx].launch_desktop`.

**`src/config.js` — `renderItems` Steam items:** Icon from `item.icon_data`, label from `item.path` (game name), small muted "Steam" tag. No Edit button (no multi-value to edit). Standard remove button and expand panel.

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Modify | `src-tauri/src/config.rs` | Add `Steam` to `ItemType` |
| Create | `src-tauri/src/steam.rs` | `get_installed_steam_games`, `get_steam_path` |
| Modify | `src-tauri/src/lib.rs` | `mod steam`, register `get_installed_steam_games` |
| Modify | `src-tauri/src/launcher.rs` | Add `ItemType::Steam` arm with cursor-based monitor targeting |
| Modify | `src-tauri/src/config.html` | Add Steam to Add Item menu |
| Modify | `src/config.js` | `addItem`, `showSteamPicker`, `buildExpandPanel` Steam branch, `renderItems` all-icon logic |

---

## Out of Scope
- Non-Windows Steam support (returns empty list gracefully)
- Library folders outside the default steamapps path (common for multi-drive setups) — v1 reads only the primary steamapps folder
- Steam games without a cached icon (renders without icon, no fallback needed beyond the name)
- Exclusive fullscreen monitor enforcement (cursor positioning is best-effort; works for most games)
