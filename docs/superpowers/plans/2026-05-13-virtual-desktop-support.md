# Virtual Desktop Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow App Launcher items to remember which Windows virtual desktop they should open on, configurable via a dropdown in the layout editor, and honored automatically at launch time.

**Architecture:** New `virtual_desktop.rs` module encapsulates all COM (`IVirtualDesktopManager`) and registry (`VirtualDesktopIDs`) access. A `launch_virtual_desktop: Option<Vec<u8>>` field on `Item` stores the 16-byte GUID. The layout editor shows a "Launch on desktop" dropdown per window; selecting an option immediately moves that window and saves the preference. At launch, `position_window_by_snapshot` calls `move_window_to_virtual_desktop` after positioning.

**Tech Stack:** Tauri v2, Rust (raw `extern "system"` COM vtable calls), Win32 Registry API, Vanilla JS/HTML

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Modify | `src-tauri/src/config.rs` | Add `launch_virtual_desktop: Option<Vec<u8>>` to Item |
| Create | `src-tauri/src/virtual_desktop.rs` | VirtualDesktop struct, COM get/move, registry enum |
| Modify | `src-tauri/src/lib.rs` | `mod virtual_desktop`, 3 new commands, extend LayoutSavePayload + complete_layout_save |
| Modify | `src-tauri/src/launcher.rs` | Add virtual_desktop param to position_window_by_snapshot, call move after place_window |
| Modify | `src/layout-item.html` | Add desktop dropdown row |
| Modify | `src/layout-item.js` | Populate dropdown, set current, handle change |
| Modify | `src/config.js` | Save launch_virtual_desktop from layout-save payload |

---

## Task 1: Add launch_virtual_desktop to Item

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block in `src-tauri/src/config.rs`:

```rust
#[test]
fn test_launch_virtual_desktop_defaults_to_none_when_absent() {
    let json = r#"{"item_type":"app","path":"C:\\foo.exe","value":null}"#;
    let item: Item = serde_json::from_str(json).unwrap();
    assert!(item.launch_virtual_desktop.is_none());
}

#[test]
fn test_launch_virtual_desktop_roundtrip() {
    let guid: Vec<u8> = (0u8..16).collect();
    let item = Item {
        item_type: ItemType::App,
        path: Some("C:\\foo.exe".into()),
        value: None,
        urls: vec![], icon_data: None, browser_name: None,
        run_in_terminal: true, run_as_admin: false,
        launch_virtual_desktop: Some(guid.clone()),
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let json = serde_json::to_string(&item).unwrap();
    let loaded: Item = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.launch_virtual_desktop, Some(guid));
}
```

- [ ] **Step 2: Run — expect compile failure**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile error — `launch_virtual_desktop` not on Item.

- [ ] **Step 3: Add field to Item struct**

In `src-tauri/src/config.rs`, add after `run_as_admin`:

```rust
#[serde(default)]
pub launch_virtual_desktop: Option<Vec<u8>>,
```

- [ ] **Step 4: Fix all Item struct literal callsites**

```powershell
grep -n "run_as_admin:" src-tauri/src/config.rs src-tauri/src/launcher.rs
```

For every Item literal with `run_as_admin:` but no `launch_virtual_desktop:`, add:
```rust
launch_virtual_desktop: None,
```
after `run_as_admin`.

- [ ] **Step 5: Run — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/config.rs src-tauri/src/launcher.rs
git commit -m "feat: add launch_virtual_desktop field to Item"
```

---

## Task 2: Create virtual_desktop.rs

**Files:**
- Create: `src-tauri/src/virtual_desktop.rs`

- [ ] **Step 1: Create the file with the full implementation**

Create `src-tauri/src/virtual_desktop.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VirtualDesktop {
    pub index: u32,
    pub guid: Vec<u8>,
    pub name: String,
}

// ── COM helpers (Windows only) ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod com {
    // CLSID_VirtualDesktopManager: {AA509086-5CA9-4C25-8F95-589D3C07B48A}
    // Mixed-endian COM byte order: first DWORD + two WORDs in LE, rest in BE
    pub const CLSID: [u8; 16] = [
        0x86, 0x90, 0x50, 0xAA, 0xA9, 0x5C, 0x25, 0x4C,
        0x8F, 0x95, 0x58, 0x9D, 0x3C, 0x07, 0xB4, 0x8A,
    ];
    // IID_IVirtualDesktopManager: {A5CD92FF-29BE-454C-8D04-D82879FB3F1B}
    pub const IID: [u8; 16] = [
        0xFF, 0x92, 0xCD, 0xA5, 0xBE, 0x29, 0x4C, 0x45,
        0x8D, 0x04, 0xD8, 0x28, 0x79, 0xFB, 0x3F, 0x1B,
    ];
    pub const CLSCTX_INPROC_SERVER: u32 = 1;
    pub const COINIT_APARTMENTTHREADED: u32 = 0x2;

    #[repr(C)]
    pub struct IVdmVtbl {
        pub query_interface: *const std::ffi::c_void,
        pub add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        pub release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        pub is_on_current: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut i32) -> i32,
        pub get_desktop_id: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut [u8; 16]) -> i32,
        pub move_to_desktop: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const [u8; 16]) -> i32,
    }

    #[repr(C)]
    pub struct IVdm {
        pub vtbl: *const IVdmVtbl,
    }

    extern "system" {
        pub fn CoInitializeEx(reserved: *mut std::ffi::c_void, co_init: u32) -> i32;
        pub fn CoCreateInstance(
            rclsid: *const [u8; 16],
            punk_outer: *mut std::ffi::c_void,
            dwclsctx: u32,
            riid: *const [u8; 16],
            ppv: *mut *mut std::ffi::c_void,
        ) -> i32;
    }

    /// Create IVirtualDesktopManager. Returns null on failure; caller must Release.
    pub unsafe fn create_vdm() -> *mut IVdm {
        CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        let mut ptr: *mut IVdm = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID,
            &mut ptr as *mut *mut IVdm as *mut *mut std::ffi::c_void,
        );
        if hr != 0 { std::ptr::null_mut() } else { ptr }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns the 16-byte GUID of the virtual desktop the window is on, or None.
pub fn get_window_virtual_desktop(hwnd: *mut std::ffi::c_void) -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    unsafe {
        let vdm = com::create_vdm();
        if vdm.is_null() { return None; }
        let mut guid = [0u8; 16];
        let hr = ((*(*vdm).vtbl).get_desktop_id)(vdm as *mut _, hwnd, &mut guid);
        ((*(*vdm).vtbl).release)(vdm as *mut _);
        if hr != 0 { return None; }
        Some(guid.to_vec())
    }
    #[cfg(not(target_os = "windows"))]
    None
}

/// Moves a window to the virtual desktop identified by the 16-byte GUID.
/// Silent no-op on failure or if guid is not 16 bytes.
pub fn move_window_to_virtual_desktop(hwnd: *mut std::ffi::c_void, guid: &[u8]) {
    if guid.len() != 16 { return; }
    #[cfg(target_os = "windows")]
    unsafe {
        let vdm = com::create_vdm();
        if vdm.is_null() { return; }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(guid);
        ((*(*vdm).vtbl).move_to_desktop)(vdm as *mut _, hwnd, &arr);
        ((*(*vdm).vtbl).release)(vdm as *mut _);
    }
}

/// Returns the list of virtual desktops from the registry, in Task View order.
/// Each entry has a 1-based index, its GUID, and a display name like "Desktop 1".
pub fn get_virtual_desktops() -> Vec<VirtualDesktop> {
    #[cfg(target_os = "windows")]
    return get_virtual_desktops_windows();
    #[cfg(not(target_os = "windows"))]
    return vec![];
}

#[cfg(target_os = "windows")]
fn get_virtual_desktops_windows() -> Vec<VirtualDesktop> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    extern "system" {
        fn RegOpenKeyExW(
            hkey: *mut std::ffi::c_void, sub_key: *const u16,
            options: u32, desired: u32, result: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn RegQueryValueExW(
            hkey: *mut std::ffi::c_void, value_name: *const u16,
            reserved: *mut u32, typ: *mut u32, data: *mut u8, data_size: *mut u32,
        ) -> i32;
        fn RegCloseKey(hkey: *mut std::ffi::c_void) -> i32;
    }

    const HKEY_CURRENT_USER: *mut std::ffi::c_void = 0x8000_0001usize as *mut _;
    const KEY_READ: u32 = 0x2_0019;

    unsafe {
        let sub_key = to_wide(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops"
        );
        let value_name = to_wide("VirtualDesktopIDs");
        let mut hkey: *mut std::ffi::c_void = std::ptr::null_mut();

        if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return vec![];
        }

        let mut buf = vec![0u8; 1024];
        let mut size = buf.len() as u32;
        let mut typ = 0u32;
        let ret = RegQueryValueExW(
            hkey, value_name.as_ptr(), std::ptr::null_mut(), &mut typ,
            buf.as_mut_ptr(), &mut size,
        );
        RegCloseKey(hkey);

        if ret != 0 { return vec![]; }

        // REG_BINARY: packed 16-byte GUIDs in Task View order
        buf[..size as usize]
            .chunks_exact(16)
            .enumerate()
            .map(|(i, chunk)| VirtualDesktop {
                index: (i + 1) as u32,
                guid: chunk.to_vec(),
                name: format!("Desktop {}", i + 1),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_virtual_desktops_returns_vec_without_panic() {
        let desktops = get_virtual_desktops();
        let _ = desktops.len();
    }

    #[test]
    fn test_get_window_virtual_desktop_null_hwnd_does_not_panic() {
        let _ = get_window_virtual_desktop(std::ptr::null_mut());
    }

    #[test]
    fn test_move_window_to_virtual_desktop_wrong_guid_length_is_noop() {
        move_window_to_virtual_desktop(std::ptr::null_mut(), &[1, 2, 3]);
    }

    #[test]
    fn test_virtual_desktop_desktops_have_16_byte_guids() {
        for d in get_virtual_desktops() {
            assert_eq!(d.guid.len(), 16, "Desktop {} GUID should be 16 bytes", d.index);
        }
    }
}
```

- [ ] **Step 2: Add mod declaration to lib.rs**

In `src-tauri/src/lib.rs`, add after the existing `mod` declarations:

```rust
pub(crate) mod virtual_desktop;
```

(`pub(crate)` lets `launcher.rs` access it via `crate::virtual_desktop::*`.)

- [ ] **Step 3: Run tests — expect pass**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test virtual_desktop 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok. 4 passed`

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/virtual_desktop.rs src-tauri/src/lib.rs
git commit -m "feat: add virtual_desktop.rs — COM get/move, registry enumeration"
```

---

## Task 3: Extend lib.rs — new commands + LayoutSavePayload + complete_layout_save

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Update LayoutSavePayload**

Find `struct LayoutSavePayload` in `src-tauri/src/lib.rs` and replace it:

```rust
#[derive(serde::Serialize, Clone)]
struct LayoutSavePayload {
    positions: Vec<[i32; 4]>,
    virtual_desktops: Vec<Option<Vec<u8>>>,
}
```

- [ ] **Step 2: Update complete_layout_save to collect virtual desktop GUIDs**

Find `fn complete_layout_save` and replace it entirely:

```rust
#[tauri::command]
fn complete_layout_save(app: tauri::AppHandle, labels: Vec<String>) {
    #[cfg(target_os = "windows")]
    let (positions, virtual_desktops): (Vec<[i32; 4]>, Vec<Option<Vec<u8>>>) = {
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }
        labels.iter().map(|label| {
            let pos = app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .map(|hwnd| {
                    let mut rect = [0i32; 4];
                    unsafe { GetWindowRect(hwnd.0 as *mut _, &mut rect); }
                    ([rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]],
                     crate::virtual_desktop::get_window_virtual_desktop(hwnd.0 as *mut _))
                })
                .unwrap_or(([0, 0, 0, 0], None));
            pos
        }).unzip()
    };
    #[cfg(not(target_os = "windows"))]
    let (positions, virtual_desktops): (Vec<[i32; 4]>, Vec<Option<Vec<u8>>>) = labels.iter().map(|label| {
        let pos = app.get_webview_window(label)
            .and_then(|w| {
                let p = w.outer_position().ok()?;
                let s = w.outer_size().ok()?;
                Some([p.x, p.y, s.width as i32, s.height as i32])
            })
            .unwrap_or([0, 0, 0, 0]);
        (pos, None)
    }).unzip();

    let _ = app.emit("layout-save", LayoutSavePayload { positions, virtual_desktops });
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.close();
        }
    }
}
```

- [ ] **Step 3: Add three new commands**

Add these functions immediately after `complete_layout_cancel`:

```rust
#[tauri::command]
fn get_virtual_desktops() -> Vec<virtual_desktop::VirtualDesktop> {
    virtual_desktop::get_virtual_desktops()
}

#[tauri::command]
fn get_current_window_desktop(window: tauri::WebviewWindow) -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        window.hwnd().ok().and_then(|hwnd|
            virtual_desktop::get_window_virtual_desktop(hwnd.0 as *mut _)
        )
    }
    #[cfg(not(target_os = "windows"))]
    None
}

#[tauri::command]
fn move_layout_window_to_desktop(app: tauri::AppHandle, label: String, guid: Vec<u8>) {
    #[cfg(target_os = "windows")]
    if let Some(window) = app.get_webview_window(&label) {
        if let Ok(hwnd) = window.hwnd() {
            virtual_desktop::move_window_to_virtual_desktop(hwnd.0 as *mut _, &guid);
        }
    }
}
```

- [ ] **Step 4: Register in generate_handler![]**

Add after `complete_layout_cancel`:
```rust
    complete_layout_cancel,
    get_virtual_desktops,
    get_current_window_desktop,
    move_layout_window_to_desktop,
```

- [ ] **Step 5: Build to verify**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo build 2>&1 | Select-String "error\["
```

Expected: no errors.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat: extend LayoutSavePayload with virtual_desktops, add vd commands"
```

---

## Task 4: Update launcher.rs — virtual desktop at launch time

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write failing test**

Add to `#[cfg(test)]` block in `src-tauri/src/launcher.rs`:

```rust
#[test]
fn test_launch_item_app_with_virtual_desktop_field_no_crash() {
    let item = Item {
        item_type: ItemType::App,
        path: Some("C:\\nonexistent.exe".into()),
        value: None,
        urls: vec![], icon_data: None, browser_name: None,
        run_in_terminal: true, run_as_admin: false,
        launch_virtual_desktop: Some(vec![0u8; 16]),
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let result = launch_item(&item, &None);
    // Should error on missing exe, not on virtual_desktop field
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run — confirm test compiles and passes**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.` (The field exists already, test should pass.)

- [ ] **Step 3: Update position_window_by_snapshot signature**

In `src-tauri/src/launcher.rs`, find `fn position_window_by_snapshot(` and replace the entire function with:

```rust
#[cfg(target_os = "windows")]
fn position_window_by_snapshot(
    before: std::collections::HashSet<usize>,
    preferred_pid: Option<u32>,
    preferred_exe: Option<String>,
    x: i32, y: i32, w: Option<u32>, h: Option<u32>,
    virtual_desktop: Option<Vec<u8>>,
) {
    use std::thread;
    use std::time::Duration;

    // --- Phase 1: synchronous ---
    if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), 5) {
        place_window(found as *mut _, x, y, w, h);
        if let Some(ref guid) = virtual_desktop {
            crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
        }
        let vd = virtual_desktop.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(1000));
            place_window(found as *mut _, x, y, w, h);
            if let Some(ref guid) = vd {
                crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
            }
            thread::sleep(Duration::from_millis(2000));
            place_window(found as *mut _, x, y, w, h);
            if let Some(ref guid) = vd {
                crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
            }
        });
        return;
    }

    // --- Phase 2: background fallback ---
    thread::spawn(move || {
        if let Some(found) = poll_for_new_window(&before, preferred_pid, preferred_exe.as_deref(), 15) {
            place_window(found as *mut _, x, y, w, h);
            if let Some(ref guid) = virtual_desktop {
                crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
            }
            thread::sleep(Duration::from_millis(1000));
            place_window(found as *mut _, x, y, w, h);
            if let Some(ref guid) = virtual_desktop {
                crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
            }
            thread::sleep(Duration::from_millis(2000));
            place_window(found as *mut _, x, y, w, h);
            if let Some(ref guid) = virtual_desktop {
                crate::virtual_desktop::move_window_to_virtual_desktop(found as *mut _, guid);
            }
        }
    });
}
```

- [ ] **Step 4: Update all 5 callsites of position_window_by_snapshot**

Search for all calls:

```powershell
grep -n "position_window_by_snapshot" src-tauri/src/launcher.rs
```

For each callsite, add `item.launch_virtual_desktop.clone()` as the final argument. There are 5 callsites:

**Callsite 1** (App arm, Phase 1 — after `child.id()`):
```rust
position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
```

**Callsite 2** (File/Folder arm):
```rust
position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
```

**Callsite 3** (URL Chromium arm):
```rust
position_window_by_snapshot(before, Some(child.id()), exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
```

**Callsite 4** (URL non-Chromium open arm):
```rust
position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
```

**Callsite 5** (Script arm):
```rust
position_window_by_snapshot(before, Some(child.id()), launcher_exe, x, y, item.launch_width, item.launch_height, item.launch_virtual_desktop.clone());
```

- [ ] **Step 5: Run all tests — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/launcher.rs
git commit -m "feat: move launched windows to saved virtual desktop after positioning"
```

---

## Task 5: UI — desktop dropdown in layout editor + config.js save

**Files:**
- Modify: `src/layout-item.html`
- Modify: `src/layout-item.js`
- Modify: `src/config.js`

- [ ] **Step 1: Add desktop dropdown to layout-item.html**

In `src/layout-item.html`, find `<div id="pk-footer">` and add directly before it:

```html
  <div id="pk-desktop-row" style="width:100%;display:flex;align-items:center;justify-content:center;gap:8px;margin-bottom:8px;">
    <span style="font-size:0.75rem;color:#aaa;">Launch on:</span>
    <select id="pk-desktop-sel" style="background:#16213e;color:#c8c8d8;border:1px solid #0f3460;border-radius:4px;font-size:0.75rem;padding:3px 6px;cursor:pointer;min-width:110px;">
      <option value="">Any desktop</option>
    </select>
  </div>
```

- [ ] **Step 2: Update layout-item.js to populate and handle the dropdown**

Replace the entire contents of `src/layout-item.js` with:

```js
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);

document.getElementById('pk-name').textContent = name;

const labels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);

// Populate the desktop dropdown and pre-select the current desktop
async function initDesktopDropdown() {
  const sel = document.getElementById('pk-desktop-sel');
  if (!sel) return;

  let desktops = [];
  try { desktops = await invoke('get_virtual_desktops'); } catch {}

  desktops.forEach(d => {
    const opt = document.createElement('option');
    opt.value = JSON.stringify(d.guid); // store guid as JSON array
    opt.textContent = d.name;
    sel.appendChild(opt);
  });

  // Pre-select whichever desktop this window is currently on
  try {
    const currentGuid = await invoke('get_current_window_desktop');
    if (currentGuid) {
      const currentJson = JSON.stringify(currentGuid);
      for (const opt of sel.options) {
        if (opt.value === currentJson) { opt.selected = true; break; }
      }
    }
  } catch {}

  sel.addEventListener('change', async (e) => {
    if (!e.target.value) return; // "Any desktop" — no move needed
    try {
      const guid = JSON.parse(e.target.value);
      const label = getCurrentWindow().label;
      await invoke('move_layout_window_to_desktop', { label, guid });
    } catch {}
  });
}

initDesktopDropdown();

// Rust handles: collect positions + virtual desktops → emit layout-save → close all windows
document.getElementById('pk-save').addEventListener('click', async () => {
  await invoke('complete_layout_save', { labels });
});

// Rust handles: emit layout-cancel → close all windows
document.getElementById('pk-cancel').addEventListener('click', async () => {
  await invoke('complete_layout_cancel', { labels });
});
```

- [ ] **Step 3: Update config.js layout-save listener to save launch_virtual_desktop**

In `src/config.js`, find the `layout-save` listener inside `showLayoutEditor`:

```js
  const unlistenSave = await listen('layout-save', ({ payload: { positions } }) => {
    positions.forEach(([x, y, w, h], i) => {
      if (i < currentItems.length && w > 0 && h > 0) {
        currentItems[i].launch_x = x;
        currentItems[i].launch_y = y;
        currentItems[i].launch_width = w;
        currentItems[i].launch_height = h;
      }
    });
```

Replace with:

```js
  const unlistenSave = await listen('layout-save', ({ payload }) => {
    const { positions, virtual_desktops } = payload;
    positions.forEach(([x, y, w, h], i) => {
      if (i < currentItems.length && w > 0 && h > 0) {
        currentItems[i].launch_x = x;
        currentItems[i].launch_y = y;
        currentItems[i].launch_width = w;
        currentItems[i].launch_height = h;
        currentItems[i].launch_virtual_desktop = virtual_desktops?.[i] ?? null;
      }
    });
```

- [ ] **Step 4: Build frontend to verify**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher"
npm run build 2>&1 | Select-String "error|Error" | Where-Object { $_ -notmatch "stderr|ErrorRecord" }
```

Expected: no errors.

- [ ] **Step 5: Verify in the app**

Run `npm run tauri dev`. Open a group editor → "📐 Edit Layout". Each layout-item window should show a "Launch on: [Any desktop ▼]" dropdown. If you have multiple virtual desktops, they should appear as "Desktop 1", "Desktop 2", etc. Selecting one should immediately move that layout-item window to that desktop. Click "Save All Positions" — save the group — launch the group — the app window should appear on the saved desktop.

- [ ] **Step 6: Commit**

```powershell
git add src/layout-item.html src/layout-item.js src/config.js
git commit -m "feat: desktop dropdown in layout editor, save virtual desktop with positions"
```
