# Run as Administrator + Layout Editor — Design Spec

**Date:** 2026-05-10
**Project:** App Launcher (Tauri v2 + Rust + Vanilla JS)
**Goal:** Two features — per-item UAC elevation for apps, and a full multi-window layout editor replacing the per-item position picker.

---

## Feature 1 — Run as Administrator

### Problem
Apps that require admin privileges launch silently with no feedback when called via `Command::spawn()` — Windows blocks the launch because App Launcher isn't elevated.

### Solution
Add a `run_as_admin: bool` field to `Item`. When true, the launcher uses `ShellExecuteExW` with the `"runas"` verb, which triggers the standard Windows UAC prompt for that specific app. The user clicks "Yes" in the UAC dialog and the app opens with admin rights.

### Data Model

**`src-tauri/src/config.rs` — `Item` struct:**
```rust
#[serde(default)]
pub run_as_admin: bool,
```
Default `false` — existing items are unaffected.

### Launcher Changes

**`src-tauri/src/launcher.rs` — `ItemType::App` arm:**

Add a check at the top of the App arm. If `run_as_admin` is true, use `ShellExecuteExW` with verb `"runas"` instead of `Command::spawn()`:

```rust
if item.run_as_admin {
    shell_execute_runas(path)?;
    return Ok(());
}
// else: existing Command::spawn() logic unchanged
```

`shell_execute_runas(path: &str)` uses raw `extern "system"` Win32 declarations (same pattern as rest of codebase):
- `ShellExecuteExW` with `SHELLEXECUTEINFOW { lpVerb: "runas", lpFile: path, nShow: SW_SHOWNORMAL, fMask: SEE_MASK_NOCLOSEPROCESS }`
- Returns `Err` if ShellExecuteEx returns false (e.g. user cancelled UAC)
- Only implemented on Windows; non-Windows: falls through to normal launch

### UI Changes

**`src/config.js` — `buildExpandPanel`:** For App and WinApp items, append a "🛡 Run as admin" checkbox row (same pattern as the "▶ Run via cmd" checkbox on script items):
```html
<label class="run-toggle">
  <input type="checkbox" class="admin-checkbox" ${item.run_as_admin ? 'checked' : ''} />
  🛡 Run as admin
</label>
```
Change handler: `currentItems[idx].run_as_admin = e.target.checked`

---

## Feature 2 — Layout Editor

### Problem
The per-item "Pick" button opens one isolated picker window at a time. Users can't see where other items will open, making it hard to arrange multiple windows without overlap.

### Solution
Replace the per-item picker with a single **"Edit Layout"** button on the group editor. Clicking it opens every item in the group as its own draggable, resizable `WebviewWindow`. Users arrange all windows simultaneously and save all positions at once.

### Removed

- The "📍 Pick" button is removed from every item's expand panel
- The position coordinate display is removed from the expand panel
- `showPickerWindow(idx)` in config.js is removed
- The `picker-result` event listener in config.js is removed
- `src/picker.html` and `src/picker.js` are deleted

The expand panel for non-Steam, non-script items becomes empty (only the script checkbox remains for script items; Steam items keep their monitor dropdown). Items that had a saved position keep it in their data — a "✕ Clear" link appears in the expand panel if a position is set, allowing users to remove it.

### Layout Editor Flow

1. User clicks **"Edit Layout"** in the group editor
2. Config.js opens one `WebviewWindow` per item:
   - **Has saved position** → window opens at `{ x: item.launch_x, y: item.launch_y, width: item.launch_width ?? 800, height: item.launch_height ?? 600 }`
   - **No saved position** → window opens at a staggered default: center of primary monitor, offset by `(idx * 30)px` so windows don't stack exactly on top of each other
3. Each window is `layout-item-{idx}` labeled, decorated (`decorations: true`), resizable, `alwaysOnTop: true`
4. Every window renders `layout-item.html` — shows the item name, a brief "drag to position" hint, and a **"Save All"** button
5. User drags/resizes windows to desired positions
6. Clicking **"Save All"** on any window:
   - Invokes a new Tauri command `get_all_layout_positions(labels: Vec<String>) -> Vec<[i32; 4]>` which calls `GetWindowRect` on each window label
   - Returns `[[x, y, w, h], ...]` in order
   - Config.js maps results back to item indices and updates `launch_x/y/width/height` on each item
   - All layout windows close
   - `renderItems()` is called to reflect cleared/updated coord displays
7. If a user closes an individual layout window manually (via title bar X), that item's position is unchanged — only Save All writes positions back.

### New Files

**`src/layout-item.html`** — window content for each item in the layout editor:
- Dark background matching app style
- Item name (passed via URL query param `?idx=N&label=...`)
- Hint text: "Drag & resize this window"
- "Save All" button → calls `invoke('get_all_layout_positions', { labels: [...] })`
- "Cancel" button → closes all layout windows without saving

**`src/layout-item.js`** — logic for layout-item.html

### New Tauri Command

**`src-tauri/src/lib.rs`:**
```rust
#[tauri::command]
fn get_all_layout_positions(app: tauri::AppHandle, labels: Vec<String>) -> Vec<[i32; 4]>
```
For each label, gets the `WebviewWindow` by label, calls `get_window_frame_rect` on it, returns the results in order. Windows not found return `[0, 0, 0, 0]`.

### config.html Changes

Add **"Edit Layout"** button to the group editor button row:
```html
<button class="btn btn-cancel" id="layout-btn">📐 Edit Layout</button>
```

### config.js Changes

- Remove `showPickerWindow`, `picker-result` listener
- Add `showLayoutEditor()` — opens all item windows, stores their labels, handles Save All result
- `buildExpandPanel` — remove coord display and Pick button; add "✕ Clear" link if position is set; keep script checkbox and Steam monitor dropdown unchanged

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Modify | `src-tauri/src/config.rs` | Add `run_as_admin: bool` to `Item` |
| Modify | `src-tauri/src/launcher.rs` | `shell_execute_runas` helper + App arm check |
| Modify | `src-tauri/src/lib.rs` | Add `get_all_layout_positions` command |
| Modify | `src-tauri/src/config.html` | Add "Edit Layout" button |
| Create | `src/layout-item.html` | Layout editor window content |
| Create | `src/layout-item.js` | Layout editor window logic |
| Modify | `src/config.js` | Admin checkbox, remove picker, add showLayoutEditor, update buildExpandPanel |
| Delete | `src/picker.html` | Replaced by layout editor |
| Delete | `src/picker.js` | Replaced by layout editor |

---

## Out of Scope
- Run as admin for Script items (scripts already have their own elevation path via cmd)
- Run as admin for URL/Steam/File/Folder items (not applicable)
- Snapping or alignment guides in the layout editor
- Undo/redo in the layout editor
