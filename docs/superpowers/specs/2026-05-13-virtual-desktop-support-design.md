# Virtual Desktop Support — Design Spec

**Date:** 2026-05-13
**Project:** App Launcher (Tauri v2 + Rust + Vanilla JS)
**Goal:** When the user positions a layout window on a Windows virtual desktop (Task View), save that desktop preference on the item and move the launched app window to that desktop at launch time.

---

## Background

Windows virtual desktops are identified by GUIDs managed via the `IVirtualDesktopManager` COM interface:
- `GetWindowDesktopId(hwnd, &guid)` — reads which virtual desktop a window is currently on
- `MoveWindowToDesktop(hwnd, &guid)` — moves a window to a specific virtual desktop

The user's current view does NOT switch when `MoveWindowToDesktop` is called — apps open silently on the target desktop, and the user switches there manually.

---

## Data Model

**`src-tauri/src/config.rs` — `Item` struct:** Add one field:

```rust
#[serde(default)]
pub launch_virtual_desktop: Option<Vec<u8>>,  // 16-byte COM GUID, or None
```

Default `None` — existing items are unaffected. `Vec<u8>` serializes as a JSON number array in config.json.

---

## New Module: virtual_desktop.rs

**`src-tauri/src/virtual_desktop.rs`** — two public functions, Windows-only.

### GUID constants

```
CLSID_VirtualDesktopManager: {AA509086-5CA9-4C25-8F95-589D3C07B48A}
  bytes: [0x86, 0x90, 0x50, 0xAA, 0xA9, 0x5C, 0x25, 0x4C, 0x8F, 0x95, 0x58, 0x9D, 0x3C, 0x07, 0xB4, 0x8A]

IID_IVirtualDesktopManager: {A5CD92FF-29BE-454C-8D04-D82879FB3F1B}
  bytes: [0xFF, 0x92, 0xCD, 0xA5, 0xBE, 0x29, 0x4C, 0x45, 0x8D, 0x04, 0xD8, 0x28, 0x79, 0xFB, 0x3F, 0x1B]
```

GUIDs use COM mixed-endian byte order (first DWORD + two WORDs in little-endian, remaining bytes big-endian).

### IVirtualDesktopManager vtable layout

```
Index 0: QueryInterface
Index 1: AddRef
Index 2: Release
Index 3: IsWindowOnCurrentVirtualDesktop(hwnd, *mut i32) -> i32
Index 4: GetWindowDesktopId(hwnd, *mut [u8; 16]) -> i32
Index 5: MoveWindowToDesktop(hwnd, *const [u8; 16]) -> i32
```

### `get_window_virtual_desktop(hwnd: *mut c_void) -> Option<Vec<u8>>`

1. `CoInitializeEx(null, COINIT_APARTMENTTHREADED)` — initialize COM
2. `CoCreateInstance(CLSID, null, CLSCTX_INPROC_SERVER, IID, &mut ptr)` — create interface
3. Call `vtbl.get_window_desktop_id(ptr, hwnd, &mut guid)` — read GUID
4. Call `vtbl.release(ptr)` — release COM object
5. Return `Some(guid.to_vec())` on success, `None` on any failure

Non-Windows: return `None`.

### `move_window_to_virtual_desktop(hwnd: *mut c_void, guid: &[u8])`

1. Validate guid is 16 bytes — early return if not
2. `CoInitializeEx` + `CoCreateInstance` (same as above)
3. Call `vtbl.move_window_to_desktop(ptr, hwnd, guid_ptr)` — move the window
4. Release COM object
5. Silent no-op on any failure

Non-Windows: no-op.

---

## Saving: complete_layout_save (lib.rs)

`LayoutSavePayload` gains a second field:

```rust
#[derive(serde::Serialize, Clone)]
struct LayoutSavePayload {
    positions: Vec<[i32; 4]>,
    virtual_desktops: Vec<Option<Vec<u8>>>,
}
```

In `complete_layout_save`, for each label:
1. Get position via `GetWindowRect` (existing)
2. Get HWND → call `virtual_desktop::get_window_virtual_desktop(hwnd)` → store result
3. Include both in the payload

Add `mod virtual_desktop;` to `lib.rs`.

---

## Config.js: layout-save listener

The `layout-save` listener in `showLayoutEditor` receives the extended payload. Update the forEach:

```js
positions.forEach(([x, y, w, h], i) => {
  if (i < currentItems.length && w > 0 && h > 0) {
    currentItems[i].launch_x = x;
    currentItems[i].launch_y = y;
    currentItems[i].launch_width = w;
    currentItems[i].launch_height = h;
    currentItems[i].launch_virtual_desktop = payload.virtual_desktops?.[i] ?? null;
  }
});
```

---

## Launching: position_window_by_snapshot (launcher.rs)

Add `virtual_desktop: Option<Vec<u8>>` parameter to `position_window_by_snapshot`.

After each `place_window(found as *mut _, x, y, w, h)` call, if `virtual_desktop` is `Some(guid)`:

```rust
virtual_desktop::move_window_to_virtual_desktop(found as *mut _, &guid);
```

Apply to all three `place_window` calls (Phase 1 synchronous, Phase 2 background × 2). The repeated calls reinforce the position — do the same for virtual desktop.

Update all callsites of `position_window_by_snapshot` in `launch_item` to pass `item.launch_virtual_desktop.clone()`.

Also add `mod virtual_desktop;` to `launcher.rs`... actually since both lib.rs and launcher.rs need it, put `mod virtual_desktop;` in `lib.rs` only and expose it as `pub(crate)` functions. `launcher.rs` accesses it via `crate::virtual_desktop::*` (since launcher is a module of the crate).

Actually: `launcher.rs` already imports `crate::config::...`. So declare `mod virtual_desktop;` in `lib.rs` and use `crate::virtual_desktop::move_window_to_virtual_desktop` in `launcher.rs`.

---

## Desktop Picker in Layout Editor Windows

Not all users know how to drag windows between virtual desktops in Task View. Each `layout-item` window gets a small "Launch on desktop" dropdown so users can pick the target desktop directly from the UI. Changing the dropdown immediately moves that layout-item window to the selected desktop.

### Enumerating virtual desktops

Windows stores virtual desktop GUIDs in the registry at:
`HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops\VirtualDesktopIDs`

This is a `REG_BINARY` value containing 16-byte GUIDs concatenated in order — Desktop 1, Desktop 2, etc.

**New function in `virtual_desktop.rs`:**

```rust
pub struct VirtualDesktop {
    pub index: u32,       // 1-based
    pub guid: Vec<u8>,    // 16 bytes
    pub name: String,     // "Desktop 1", "Desktop 2", etc.
}

pub fn get_virtual_desktops() -> Vec<VirtualDesktop>
```

Reads `VirtualDesktopIDs` from registry, parses into 16-byte chunks, returns a sorted list. Names are "Desktop 1", "Desktop 2", etc. Non-Windows returns empty vec.

### New Tauri commands (lib.rs)

```rust
#[tauri::command]
fn get_virtual_desktops() -> Vec<virtual_desktop::VirtualDesktop>

#[tauri::command]
fn move_layout_window_to_desktop(app: tauri::AppHandle, label: String, guid: Vec<u8>)
```

`move_layout_window_to_desktop` looks up the window by label, gets its HWND, calls `virtual_desktop::move_window_to_virtual_desktop(hwnd, &guid)`.

Both registered in `generate_handler![]`.

### layout-item.html changes

Add a desktop dropdown row above the footer buttons:

```html
<div id="pk-desktop-row" style="width:100%;display:flex;align-items:center;justify-content:center;gap:8px;margin-bottom:8px;">
  <span style="font-size:0.75rem;color:#aaa;">Launch on:</span>
  <select id="pk-desktop-sel" style="background:#16213e;color:#c8c8d8;border:1px solid #0f3460;border-radius:4px;font-size:0.75rem;padding:3px 6px;cursor:pointer;">
    <option value="">Any desktop</option>
  </select>
</div>
```

### layout-item.js changes

On load:
1. Call `invoke('get_virtual_desktops')` → populate dropdown with options
2. Call `invoke('get_current_window_desktop')` → set the selected option to match current desktop

On dropdown change:
```js
sel.addEventListener('change', async (e) => {
  const guid = JSON.parse(e.target.value); // stored as JSON array
  const label = await getCurrentWindow().label;
  await invoke('move_layout_window_to_desktop', { label, guid });
});
```

**New Tauri command `get_current_window_desktop`:**

```rust
#[tauri::command]
fn get_current_window_desktop(window: tauri::WebviewWindow) -> Option<Vec<u8>>
```

Gets the calling window's HWND, calls `get_window_virtual_desktop(hwnd)`, returns the GUID. Used by layout-item.js to pre-select the correct dropdown option.

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Modify | `src-tauri/src/config.rs` | Add `launch_virtual_desktop: Option<Vec<u8>>` to Item |
| Create | `src-tauri/src/virtual_desktop.rs` | COM interface: get + move + enumerate virtual desktops |
| Modify | `src-tauri/src/lib.rs` | `mod virtual_desktop`, extend LayoutSavePayload, update complete_layout_save, add 3 new commands |
| Modify | `src-tauri/src/launcher.rs` | Add virtual_desktop param to position_window_by_snapshot, call move after place_window |
| Modify | `src/config.js` | Save launch_virtual_desktop from layout-save payload |
| Modify | `src/layout-item.html` | Add desktop dropdown row |
| Modify | `src/layout-item.js` | Populate dropdown, handle change → move window immediately |

---

## Out of Scope
- Custom desktop names (shows "Desktop 1", "Desktop 2" etc. — not user-renamed names)
- Switching the user's view to the target desktop at launch time (stays on current desktop)
- Non-Windows virtual desktop support (macOS Spaces, Linux workspaces)
