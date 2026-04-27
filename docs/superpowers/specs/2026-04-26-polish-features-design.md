# Design: Polish Features — Tray Icon, Native Context Menu, URL Picker

**Date:** 2026-04-26  
**Status:** Approved

---

## Feature 1 — System Tray Icon

### Problem
There is no way to quit the app or hide the widget without killing the process from Task Manager.

### Design
Add a system tray icon using `tauri-plugin-tray`. The icon reuses the existing app icon from `src-tauri/icons/32x32.png`. Right-clicking the tray icon shows a native OS menu with two items:

- **Show/Hide Widget** — toggles the widget window visibility (`window.show()` / `window.hide()`)
- **Quit** — calls `app.exit(0)`

The tray is set up in the Tauri `setup` hook in `lib.rs`. No frontend changes required.

**Dependencies added:** `tauri-plugin-tray = "2"` in `Cargo.toml`, `"tray-icon"` feature in `tauri` dependency, plugin registered with `.plugin(tauri_plugin_tray::init())` in `run()`.

**Capabilities:** Add `"tray-icon:default"` to `src-tauri/capabilities/default.json`.

---

## Feature 2 — Native Right-Click Context Menu

### Problem
The current HTML `#context-menu` div is clipped by the widget window boundary. Since the widget is a thin bar (~80px tall), any menu that renders below the cursor is cut off.

### Design
Replace the HTML context menu with Tauri's native OS menu.

**Rust side:** New command `show_group_context_menu(group_id: String, app: AppHandle)` that:
1. Builds a native `Menu` with two `MenuItem`s: "Edit Group" and "Delete Group"
2. Shows it via `app.get_webview_window("widget").unwrap().popup_menu(&menu)`
3. Each item click emits a Tauri event to the widget frontend:
   - Edit → `emit("context-menu:edit", group_id)`
   - Delete → `emit("context-menu:delete", group_id)`

**Frontend side (widget.js):**
- Remove the `#context-menu` HTML div and all JS that builds/positions/hides it
- Replace `showContextMenu()` with a call to `invoke('show_group_context_menu', { groupId })`
- Add two event listeners at startup: `listen('context-menu:edit', ...)` and `listen('context-menu:delete', ...)` which call the existing `openConfig(groupId)` and `deleteGroup(groupId)` functions

**widget.html:** Remove `<div id="context-menu">` and its CSS.

**Dependencies:** `use tauri::{menu::{Menu, MenuItem}, Manager};` — all in Tauri core, no new crates.

---

## Feature 3 — URL Picker with Browser Detection, Bookmarks, and Multi-Select

### Problem
The current URL flow uses `window.prompt()` (ugly native dialog) and requires the user to know and type the URL. There's no way to pick from existing bookmarks or open multiple URLs as tabs.

### Flow
1. User clicks "🌐 URL" in the add-type-menu
2. Modal opens showing detected installed browsers (same searchable card pattern as Windows Apps picker)
3. User picks a browser
4. Second view replaces browser list: shows a custom URL text input at the top + a flat checkbox list of all bookmarks from that browser (title displayed, URL shown as subtitle)
5. User checks any number of bookmarks and/or types a custom URL
6. Clicks **"Add Selected"** button → all selected items added to `currentItems` at once → modal closes

### Data Model
Each URL item stores:
- `path` = browser exe path (e.g. `C:\Program Files\Google\Chrome\Application\chrome.exe`)
- `value` = URL string

Backwards compatible: existing items with `path = null` fall back to global `preferred_browser` or `open::that`.

### Multi-Tab Launch
`launch_group` in `launcher.rs` is updated to batch URL items by browser:
1. Collect all URL items in the group
2. Group by `item.path` (browser exe)
3. For each browser → `Command::new(browser).args(urls).spawn()` (all URLs as args = one window, multiple tabs)
4. For items with no browser path → `open::that(url)` per item (fallback)

Non-URL items are launched individually as before.

### Rust: Two New Commands

**`get_installed_browsers() -> Vec<BrowserInfo>`**

Struct: `BrowserInfo { name: String, path: String }`

Detection strategy (Windows):
- Check known exe paths for: Chrome (`%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe`), Edge (`C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe`), Brave (`%LOCALAPPDATA%\BraveSoftware\Brave-Browser\Application\brave.exe`), Firefox (`C:\Program Files\Mozilla Firefox\firefox.exe` and `C:\Program Files (x86)\Mozilla Firefox\firefox.exe`), Opera (`%LOCALAPPDATA%\Programs\Opera\opera.exe`), Vivaldi (`%LOCALAPPDATA%\Vivaldi\Application\vivaldi.exe`)
- Skip entries where the exe path does not exist on disk
- Returns only browsers actually installed

**`get_browser_bookmarks(browser_path: String) -> Vec<BookmarkItem>`**

Struct: `BookmarkItem { title: String, url: String }`

Browser type detected from exe filename:
- **Chromium-based** (chrome.exe, msedge.exe, brave.exe, opera.exe, vivaldi.exe): reads `Bookmarks` JSON file from the default profile dir. Exact paths per browser:
  - Chrome: `%LOCALAPPDATA%\Google\Chrome\User Data\Default\Bookmarks`
  - Edge: `%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Bookmarks`
  - Brave: `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\Default\Bookmarks`
  - Opera: `%APPDATA%\Opera Software\Opera Stable\Bookmarks`
  - Vivaldi: `%LOCALAPPDATA%\Vivaldi\User Data\Default\Bookmarks`
  
  Recursively flattens the bookmark tree, collecting `{ name, url }` from all nodes of type `"url"`. Skips folders.
- **Firefox** (firefox.exe): finds default profile dir from `%APPDATA%\Mozilla\Firefox\profiles.ini`, reads `places.sqlite` using `rusqlite` crate, queries: `SELECT b.title, p.url FROM moz_bookmarks b JOIN moz_places p ON b.fk = p.id WHERE b.type = 1 AND p.url NOT LIKE 'place:%'`. Skips internal Firefox "place:" entries.

Returns flat list sorted by title (case-insensitive). Items with empty title fall back to URL as display text.

**New dependency:** `rusqlite = { version = "0.31", features = ["bundled"] }` in `Cargo.toml`. The `bundled` feature includes SQLite statically so no system SQLite is required.

### Frontend: URL Picker Modal

New function `showUrlPicker()` in `config.js`, replaces the current `type === 'url'` branch in `addItem()`.

**Step 1 — Browser selection:**
Same card structure as `showWinAppPicker`. Calls `invoke('get_installed_browsers')`. Each row shows browser name. Clicking a row transitions to Step 2 (does not close modal).

**Step 2 — Bookmarks + custom URL:**
- Header shows back button (←) and selected browser name
- Custom URL input at top with placeholder "https://..."
- Checkbox list below: each row has a checkbox, the bookmark title, and the URL as a smaller subtitle
- "Add Selected" button at bottom, disabled when nothing is checked and custom URL input is empty, shows count when items are checked (e.g. "Add 3 Selected")
- Clicking "Add Selected":
  - For each checked bookmark: push `{ item_type: 'url', path: browser_path, value: bookmark.url }`
  - If custom URL input is non-empty: push `{ item_type: 'url', path: browser_path, value: custom_url }`
  - Call `renderItems()`, close modal
- Back button returns to Step 1 (browser list)
- Escape key and backdrop click close the modal entirely

**CSS:** New styles for `.url-picker-*` — checkbox rows, subtitle text, back button, disabled Add button state. Added to `styles.css`.

---

## Files Changed

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-tray`, `rusqlite` |
| `src-tauri/src/lib.rs` | Tray setup in `setup` hook; `show_group_context_menu` command; `get_installed_browsers` + `get_browser_bookmarks` commands |
| `src-tauri/src/browsers.rs` | New file — `BrowserInfo`, `BookmarkItem`, `get_installed_browsers()`, `get_browser_bookmarks()` |
| `src-tauri/capabilities/default.json` | Add `tray-icon:default` |
| `src/widget.html` | Remove `#context-menu` div |
| `src/widget.js` | Replace HTML context menu with `invoke('show_group_context_menu')` + event listeners |
| `src/config.js` | Replace `addItem` url branch with `showUrlPicker()`; add `showUrlPicker()` function |
| `src/styles.css` | Add `.url-picker-*` styles; remove `.context-menu` styles |

---

## Out of Scope
- Multiple Firefox profiles (uses default profile only)
- Browser bookmark folders/hierarchy (flat list only)
- Bookmark favicons in the picker
- Synced bookmarks from browser account (reads local profile only)
