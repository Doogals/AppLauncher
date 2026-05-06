# App Launcher Feature Set — Design Spec

**Date:** 2026-05-06
**Project:** App Launcher (Tauri v2 + Rust + Vanilla JS)
**Goal:** Three independent improvements — installer upgrade safety, multi-URL items with browser icons, and script open/run toggle.

---

## Feature 1 — Installer Kills Running AppLauncher

### Problem
If the user runs the installer while AppLauncher.exe is already open, the MSI gets stuck because it can't overwrite a running executable.

### Solution
Add a pre-install NSIS hook that silently kills any running `App Launcher.exe` process before files are copied. The `taskkill` call is fire-and-forget — a non-zero exit code (nothing running) is ignored.

### Implementation

**New file:** `src-tauri/preinstall.nsh`
```nsis
!macro PreInstall
  nsExec::Exec 'taskkill /F /IM "App Launcher.exe"'
  Pop $0
!macroend
```

**`tauri.conf.json` change:** Add to `bundle.windows`:
```json
"nsis": {
  "preinstallSection": "preinstall.nsh"
}
```

### Scope
- Only applies to NSIS installer (primary deploy target)
- Does not affect WiX/MSI target
- No config changes, no Rust changes, no UI changes

---

## Feature 2 — Multi-URL Items with Browser Icons

### Problem
Each URL/bookmark item currently holds exactly one URL. Users who want to open a set of URLs in one browser must create one item per URL, cluttering the group editor. There's no way to see at a glance which browser each item uses.

### Design

**One URL item = one browser + one or more URLs.** All URLs in the item open together (same browser, same launch). Two URL items with different browsers launch independently.

The item row in the group editor shows:
- Browser icon (extracted from the .exe) instead of 🌐
- Label: `Chrome (3 URLs)`
- Subtitle: first 1–2 hostname previews, e.g. `gmail.com, calendar.google.com +1`
- ✏ Edit button — reopens the URL picker pre-populated with the existing browser and URL list
- ✕ Remove button
- Position picker expand panel (unchanged)

### Data Model Changes

**`src-tauri/src/config.rs` — `Item` struct:**

Add three fields:
```rust
#[serde(default)]
pub urls: Vec<String>,         // primary URL list (replaces per-item value for new items)
#[serde(default)]
pub icon_data: Option<String>, // base64 PNG icon from browser .exe
#[serde(default)]
pub browser_name: Option<String>, // display name of browser (e.g. "Chrome"), stored so widget doesn't re-derive
```

Backward compat: existing items with `value: Some(url)` and `urls: []` continue to work. The launcher checks `urls` first; falls back to `value` if `urls` is empty.

### New Tauri Command: `get_file_icon`

**`src-tauri/src/lib.rs`** — new command:
```rust
#[tauri::command]
fn get_file_icon(path: String) -> Option<String>
```

On Windows: uses `SHGetFileInfo` with `SHGFI_ICON | SHGFI_LARGEICON` to get the HICON, then converts to a base64 PNG using GDI (HBITMAP → DIB → PNG bytes → base64). Returns `None` on failure (frontend falls back to 🌐).

On non-Windows: returns `None`.

### Launcher Changes (`src-tauri/src/launcher.rs`)

**`collect_browser_urls`:** Updated to iterate `item.urls` if non-empty, else use `item.value`:
```rust
let url_list: Vec<&str> = if !item.urls.is_empty() {
    item.urls.iter().map(|s| s.as_str()).collect()
} else if let Some(v) = &item.value {
    vec![v.as_str()]
} else {
    continue;
};
```

**`launch_item` (Url branch):** Same update — use `item.urls` first, fall back to `item.value`.

### UI Changes (`src/config.js`)

**URL picker (`showUrlPicker` / `showBookmarkStep`):**
- Now accepts an optional existing item for edit mode
- On "Add" / confirm: creates or updates **one item** with `urls: [all selected]`, `path: browser.path`, `icon_data: await invoke('get_file_icon', { path: browser.path })`
- The "Add Selected" button text becomes "Save (N URLs)" in edit mode

**`renderItems`:**
- URL item row: render `<img>` from `item.icon_data` (base64) if present, else 🌐 emoji fallback
- Label: `${browserName} (${item.urls.length || 1} URL${...})`
- Subtitle: hostname previews of first 2 URLs, `+N more` if applicable
- Edit button: calls `showUrlPicker({ existingItem: item, idx })` to reopen pre-populated

**Hostname preview helper:**
```js
function urlHostname(url) {
  try { return new URL(url).hostname.replace(/^www\./, ''); }
  catch { return url; }
}
```

**Item label in widget (`src/widget.js`):** URL items currently show `item.value` as label. Update to show `browserName (N URLs)` using the same logic as config.js.

---

## Feature 3 — Script Open/Run Toggle

### Problem
Script items always execute via `cmd /C` or `powershell -File`. Sometimes a user just wants to open a script file in their editor, not run it.

### Design

Add a "▶ Run" checkbox to script item rows in the group editor. **Checked = run via cmd (current behavior). Unchecked = open in default app (e.g., Notepad for .bat).**

Default for new items: checked (matches current behavior, no regression for existing users).

### Data Model Changes

**`src-tauri/src/config.rs` — `Item` struct:**
```rust
#[serde(default = "default_true")]
pub run_in_terminal: bool,
```

`default_true()` already exists in config.rs. Existing script items deserialize without this field → `run_in_terminal = true` → behavior unchanged.

### Launcher Changes (`src-tauri/src/launcher.rs`)

**`launch_item` Script branch:**
```rust
ItemType::Script => {
    let path = item.path.as_ref().ok_or("Script item is missing a path")?;
    if !item.run_in_terminal {
        open::that(path).map_err(|e| format!("Failed to open script '{}': {}", path, e))?;
        return Ok(());
    }
    // existing cmd/powershell logic unchanged below...
}
```

### UI Changes (`src/config.js`)

**`renderItems` for script items:** Add a checkbox row inside the expand panel (alongside the position picker):
```html
<label class="run-toggle">
  <input type="checkbox" class="run-checkbox" ${item.run_in_terminal !== false ? 'checked' : ''} />
  ▶ Run via cmd
</label>
```

Checkbox change handler updates `currentItems[idx].run_in_terminal`.

**`addItem('script')`:** New script items get `run_in_terminal: true` by default.

---

## File Map

| File | Changes |
|------|---------|
| `src-tauri/preinstall.nsh` | **New** — NSIS pre-install kill hook |
| `src-tauri/tauri.conf.json` | Add `bundle.windows.nsis.preinstallSection` |
| `src-tauri/src/config.rs` | Add `urls`, `icon_data`, `browser_name`, `run_in_terminal` to `Item` |
| `src-tauri/src/lib.rs` | Add `get_file_icon` command + register it |
| `src-tauri/src/launcher.rs` | Update URL launch to use `urls`, add `run_in_terminal` branch |
| `src/config.js` | URL item row redesign, Edit button, script checkbox |
| `src/widget.js` | URL item label update |

---

## Out of Scope
- NSIS kill hook for WiX/MSI target
- Icon extraction on non-Windows platforms (returns None gracefully)
- Per-URL browser assignment within a single item (all URLs in one item use the same browser)
