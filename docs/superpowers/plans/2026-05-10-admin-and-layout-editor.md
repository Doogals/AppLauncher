# Run as Administrator + Layout Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-app UAC elevation checkbox and replace the single-item position picker with a full multi-window layout editor.

**Architecture:** Feature 1 adds `run_as_admin: bool` to `Item` and uses `ShellExecuteExW` with "runas" verb in the App launch arm. Feature 2 creates a new `layout-item.html/js` window type, a `get_all_layout_positions` Tauri command, and a `showLayoutEditor()` function in config.js that opens one window per group item for simultaneous drag-to-position editing — replacing picker.html/js entirely.

**Tech Stack:** Tauri v2, Rust (Win32 ShellExecuteExW + GetWindowRect), Vanilla JS/HTML, Vite multi-page build

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
| Modify | `vite.config.js` | Swap picker entry for layout-item |
| Delete | `src/picker.html` | Replaced by layout-item.html |
| Delete | `src/picker.js` | Replaced by layout-item.js |

---

## Task 1: Add run_as_admin to Item

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write failing test**

Add to the `#[cfg(test)]` block in `src-tauri/src/config.rs`:

```rust
#[test]
fn test_run_as_admin_defaults_to_false_when_absent() {
    let json = r#"{"item_type":"app","path":"C:\\foo.exe","value":null}"#;
    let item: Item = serde_json::from_str(json).unwrap();
    assert!(!item.run_as_admin, "run_as_admin should default to false");
}

#[test]
fn test_run_as_admin_roundtrip() {
    let item = Item {
        item_type: ItemType::App,
        path: Some("C:\\foo.exe".into()),
        value: None,
        urls: vec![], icon_data: None, browser_name: None,
        run_in_terminal: true, run_as_admin: true,
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let json = serde_json::to_string(&item).unwrap();
    let loaded: Item = serde_json::from_str(&json).unwrap();
    assert!(loaded.run_as_admin);
}
```

- [ ] **Step 2: Run — expect compile failure**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile error — `run_as_admin` not found on `Item`.

- [ ] **Step 3: Add run_as_admin to Item struct**

In `src-tauri/src/config.rs`, add after `run_in_terminal`:

```rust
#[serde(default)]
pub run_as_admin: bool,
```

The full Item struct should now have these fields in order after `value`:
```rust
#[serde(default)]
pub urls: Vec<String>,
#[serde(default)]
pub icon_data: Option<String>,
#[serde(default)]
pub browser_name: Option<String>,
#[serde(default = "default_true")]
pub run_in_terminal: bool,
#[serde(default)]
pub run_as_admin: bool,
#[serde(default)]
pub launch_desktop: Option<u32>,
// ... launch_x/y/width/height unchanged
```

- [ ] **Step 4: Fix Item literal callsites in tests**

Search for all Item struct literals that don't include `run_as_admin`:

```powershell
grep -n "run_in_terminal:" src-tauri/src/config.rs src-tauri/src/launcher.rs
```

For every literal that has `run_in_terminal:` but not `run_as_admin:`, add `run_as_admin: false,` after `run_in_terminal`.

- [ ] **Step 5: Run — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/config.rs
git commit -m "feat: add run_as_admin field to Item"
```

---

## Task 2: shell_execute_runas + App arm check in launcher.rs

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write failing test**

Add to the `#[cfg(test)]` block in `src-tauri/src/launcher.rs`:

```rust
#[test]
fn test_launch_item_app_missing_path_still_errors_with_run_as_admin() {
    let item = Item {
        item_type: ItemType::App,
        path: None, value: None,
        urls: vec![], icon_data: None, browser_name: None,
        run_in_terminal: true, run_as_admin: true,
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let result = launch_item(&item, &None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing a path"));
}
```

- [ ] **Step 2: Run — expect compile failure on new field**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile error — `run_as_admin` field not yet in existing Item literals in launcher.rs tests (already fixed in Task 1 Step 4, so this should just FAIL on the test assertion if the old arm still runs).

- [ ] **Step 3: Add shell_execute_runas helper**

Add this function in `src-tauri/src/launcher.rs` just before `launch_group`:

```rust
#[cfg(target_os = "windows")]
fn shell_execute_runas(path: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut std::ffi::c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut std::ffi::c_void,
        lp_id_list: *mut std::ffi::c_void,
        lp_class: *const u16,
        h_key_class: *mut std::ffi::c_void,
        dw_hot_key: u32,
        _union_padding: u32,
        h_monitor: *mut std::ffi::c_void,
        h_process: *mut std::ffi::c_void,
    }

    extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }

    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
    const SW_SHOWNORMAL: i32 = 1;

    let verb = to_wide("runas");
    let file = to_wide(path);

    let mut info = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        f_mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: std::ptr::null(),
        lp_directory: std::ptr::null(),
        n_show: SW_SHOWNORMAL,
        h_inst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null(),
        h_key_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        _union_padding: 0,
        h_monitor: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        Err(format!(
            "Failed to launch '{}' as administrator (user may have cancelled UAC prompt)",
            path
        ))
    } else {
        Ok(())
    }
}
```

- [ ] **Step 4: Add run_as_admin check to the App arm in launch_item**

In `src-tauri/src/launcher.rs`, inside `launch_item`, find the start of the `ItemType::App` arm:

```rust
        ItemType::App => {
            let path = item.path.as_ref().ok_or("App item is missing a path")?;
            let mut cmd = Command::new(path);
```

Replace with:

```rust
        ItemType::App => {
            let path = item.path.as_ref().ok_or("App item is missing a path")?;

            // If run_as_admin is requested, use ShellExecuteExW with "runas" verb
            // to trigger UAC elevation. This bypasses Command::spawn() entirely.
            #[cfg(target_os = "windows")]
            if item.run_as_admin {
                return shell_execute_runas(path);
            }

            let mut cmd = Command::new(path);
```

- [ ] **Step 5: Run all tests — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/launcher.rs
git commit -m "feat: add shell_execute_runas and run_as_admin check in App launch arm"
```

---

## Task 3: Run as Admin Checkbox UI

**Files:**
- Modify: `src/config.js`

- [ ] **Step 1: Add admin checkbox to buildExpandPanel for App and WinApp items**

In `src/config.js`, find the `buildExpandPanel` function. After the existing position picker setup (after `panel.querySelector('.pick-btn').addEventListener(...)` and before `if (item.item_type === 'script') {`), add:

```js
  if (item.item_type === 'app') {
    const adminRow = document.createElement('div');
    adminRow.className = 'item-expand-row';
    const checked = item.run_as_admin ? 'checked' : '';
    adminRow.innerHTML = `
      <label class="run-toggle">
        <input type="checkbox" class="admin-checkbox" ${checked} />
        🛡 Run as admin
      </label>
    `;
    adminRow.querySelector('.admin-checkbox').addEventListener('change', (e) => {
      currentItems[idx].run_as_admin = e.target.checked;
    });
    panel.appendChild(adminRow);
  }
```

Note: `item_type` is `'app'` for both regular App items (from file picker) and WinApp items (both get stored as `item_type: 'app'`).

- [ ] **Step 2: Verify in the app**

Run `npm run tauri dev`. Open a group with an App item. Expand the item panel — a "🛡 Run as admin" checkbox should appear below the position picker. Toggle it, save the group, and verify `run_as_admin` appears in `%LOCALAPPDATA%\AppLauncher\config.json`.

- [ ] **Step 3: Commit**

```powershell
git add src/config.js
git commit -m "feat: add run_as_admin checkbox to App item expand panel"
```

---

## Task 4: get_all_layout_positions Command

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the command after get_window_frame_rect**

In `src-tauri/src/lib.rs`, add this function immediately after `get_window_frame_rect`:

```rust
#[tauri::command]
fn get_all_layout_positions(app: tauri::AppHandle, labels: Vec<String>) -> Vec<[i32; 4]> {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }
        labels.iter().map(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .map(|hwnd| {
                    let mut rect = [0i32; 4];
                    unsafe { GetWindowRect(hwnd.0 as *mut _, &mut rect); }
                    [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]
                })
                .unwrap_or([0, 0, 0, 0])
        }).collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        labels.iter().map(|label| {
            app.get_webview_window(label)
                .and_then(|w| {
                    let pos = w.outer_position().ok()?;
                    let size = w.outer_size().ok()?;
                    Some([pos.x, pos.y, size.width as i32, size.height as i32])
                })
                .unwrap_or([0, 0, 0, 0])
        }).collect()
    }
}
```

- [ ] **Step 2: Register in generate_handler![]**

In `src-tauri/src/lib.rs`, add `get_all_layout_positions` to the `generate_handler![]` list after `get_window_frame_rect`:

```rust
    get_window_frame_rect,
    get_all_layout_positions,
```

- [ ] **Step 3: Build to verify**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo build 2>&1 | Select-String "error\["
```

Expected: no errors.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat: add get_all_layout_positions command"
```

---

## Task 5: Create layout-item.html and layout-item.js

**Files:**
- Create: `src/layout-item.html`
- Create: `src/layout-item.js`
- Modify: `vite.config.js`

- [ ] **Step 1: Create src/layout-item.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Layout Editor</title>
  <style>
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
    html, body { height: 100%; }
    body {
      background: rgba(15, 32, 64, 0.97);
      color: #e0e0e0;
      font-family: 'Segoe UI', sans-serif;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: space-between;
      padding: 24px 20px 16px;
      user-select: none;
    }
    #pk-body {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
      flex: 1;
      justify-content: center;
    }
    #pk-name {
      font-size: 0.95rem;
      font-weight: 600;
      color: #fff;
      text-align: center;
      max-width: 100%;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    #pk-cross { font-size: 2.4rem; color: #4a9eff; line-height: 1; }
    #pk-hint {
      font-size: 0.75rem;
      color: #aaa;
      text-align: center;
      line-height: 1.5;
    }
    #pk-hint small { color: #666; }
    #pk-footer { display: flex; gap: 8px; width: 100%; }
    button {
      flex: 1;
      padding: 7px 0;
      border-radius: 5px;
      border: 1px solid #0f3460;
      cursor: pointer;
      font-size: 0.82rem;
      font-family: inherit;
    }
    #pk-cancel  { background: #16213e; color: #aaa; }
    #pk-cancel:hover  { border-color: #e94560; color: #e94560; }
    #pk-save { background: #e07b39; color: #fff; border-color: #e07b39; }
    #pk-save:hover { background: #c96a2a; }
  </style>
</head>
<body>
  <div id="pk-body">
    <div id="pk-name"></div>
    <div id="pk-cross">&#x2316;</div>
    <div id="pk-hint">
      Drag &amp; resize this window to set its launch position<br>
      <small>Move all windows, then click Save All</small>
    </div>
  </div>
  <div id="pk-footer">
    <button id="pk-cancel">Cancel All</button>
    <button id="pk-save">Save All Positions</button>
  </div>
  <script type="module" src="layout-item.js"></script>
</body>
</html>
```

- [ ] **Step 2: Create src/layout-item.js**

```js
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);

document.getElementById('pk-name').textContent = name;

// Derive all window labels from the deterministic pattern
const labels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);

async function closeAll() {
  for (const label of labels) {
    try {
      const win = WebviewWindow.getByLabel(label);
      if (win) await win.close();
    } catch {}
  }
}

document.getElementById('pk-save').addEventListener('click', async () => {
  const positions = await invoke('get_all_layout_positions', { labels });
  await emit('layout-save', { positions });
  await closeAll();
});

document.getElementById('pk-cancel').addEventListener('click', async () => {
  await emit('layout-cancel');
  await closeAll();
});
```

- [ ] **Step 3: Update vite.config.js**

In `vite.config.js`, find the `rollupOptions.input` block:

```js
      input: {
        widget: resolve(__dirname, 'src/widget.html'),
        config: resolve(__dirname, 'src/config.html'),
        picker: resolve(__dirname, 'src/picker.html'),
      },
```

Replace with:

```js
      input: {
        widget: resolve(__dirname, 'src/widget.html'),
        config: resolve(__dirname, 'src/config.html'),
        layoutItem: resolve(__dirname, 'src/layout-item.html'),
      },
```

- [ ] **Step 4: Build to verify**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher"
npm run build 2>&1 | Select-String "error|Error"
```

Expected: no errors. `dist/layout-item.html` should exist in the output.

- [ ] **Step 5: Commit**

```powershell
git add src/layout-item.html src/layout-item.js vite.config.js
git commit -m "feat: add layout-item window for layout editor"
```

---

## Task 6: Config.js Layout Editor + Cleanup

**Files:**
- Modify: `src-tauri/src/config.html`
- Modify: `src/config.js`
- Delete: `src/picker.html`
- Delete: `src/picker.js`

This is the largest task. Work through it step by step.

- [ ] **Step 1: Add "Edit Layout" button to config.html**

In `src-tauri/src/config.html`, find the btn-row:

```html
    <div class="btn-row">
      <button class="btn btn-cancel" id="cancel-btn">Cancel</button>
      <button class="btn btn-save" id="save-btn">Save</button>
    </div>
```

Replace with:

```html
    <div class="btn-row">
      <button class="btn btn-cancel" id="cancel-btn">Cancel</button>
      <button class="btn btn-cancel" id="layout-btn">📐 Edit Layout</button>
      <button class="btn btn-save" id="save-btn">Save</button>
    </div>
```

- [ ] **Step 2: Add showLayoutEditor function to config.js**

Add `showLayoutEditor` after the `showSteamPicker` function (before `fitWindow`):

```js
async function showLayoutEditor() {
  if (currentItems.length === 0) return;

  let monitors;
  try { monitors = await invoke('get_monitors'); } catch { monitors = []; }
  const primary = monitors.find(m => m.is_primary) || { x: 0, y: 0, width: 1920, height: 1080 };
  const centerX = primary.x + Math.floor(primary.width / 2) - 400;
  const centerY = primary.y + Math.floor(primary.height / 2) - 300;
  const total = currentItems.length;

  for (let idx = 0; idx < currentItems.length; idx++) {
    const item = currentItems[idx];
    const hasPos = item.launch_x != null && item.launch_y != null;
    const x = hasPos ? item.launch_x : centerX + idx * 30;
    const y = hasPos ? item.launch_y : centerY + idx * 30;
    const w = Math.max(item.launch_width || 800, 300);
    const h = Math.max(item.launch_height || 600, 200);

    const rawName = item.item_type === 'steam'
      ? (item.path || 'Steam Game')
      : item.item_type === 'url'
        ? browserDisplayName(item)
        : (item.path || item.value || 'Item');
    const safeName = encodeURIComponent(rawName);

    new WebviewWindow(`layout-item-${idx}`, {
      url: `layout-item.html?idx=${idx}&name=${safeName}&total=${total}`,
      title: rawName,
      x, y,
      width: w,
      height: h,
      resizable: true,
      decorations: true,
      alwaysOnTop: true,
    });
  }

  const unlistenSave = await listen('layout-save', ({ payload: { positions } }) => {
    positions.forEach(([x, y, w, h], i) => {
      if (i < currentItems.length && w > 0 && h > 0) {
        currentItems[i].launch_x = x;
        currentItems[i].launch_y = y;
        currentItems[i].launch_width = w;
        currentItems[i].launch_height = h;
      }
    });
    unlistenSave();
    unlistenCancel();
    renderItems();
  });

  const unlistenCancel = await listen('layout-cancel', () => {
    unlistenSave();
    unlistenCancel();
  });
}
```

- [ ] **Step 3: Wire Edit Layout button and remove picker in config.js**

Find this line in `config.js`:

```js
document.getElementById('cancel-btn').onclick = async () => {
  await getCurrentWindow().close();
};
```

Add after it:

```js
document.getElementById('layout-btn').onclick = () => showLayoutEditor();
```

Find and remove `showPickerWindow`:

```js
function showPickerWindow(idx) {
  new WebviewWindow('picker', {
    url: `picker.html?idx=${idx}`,
    title: 'Pick Launch Position',
    width: 480,
    height: 260,
    resizable: true,
    decorations: true,
    alwaysOnTop: true,
    center: true,
  });
}
```

Delete the entire function.

Find and remove the `picker-result` event listener in the `init` function:

```js
  listen('picker-result', ({ payload: { idx, x, y, w, h } }) => {
    currentItems[idx].launch_x = x;
    currentItems[idx].launch_y = y;
    currentItems[idx].launch_width = w;
    currentItems[idx].launch_height = h;
    renderItems();
  });
```

Delete those lines.

- [ ] **Step 4: Replace buildExpandPanel entirely**

Replace the **entire** `buildExpandPanel` function with the version below. Key changes: remove coord display and Pick button; add "✕ Clear" if position is saved; keep script checkbox and Steam dropdown; add admin checkbox for app items.

```js
function buildExpandPanel(item, idx) {
  const panel = document.createElement('div');
  panel.className = 'item-expand';

  if (item.item_type === 'steam') {
    // Steam items: monitor dropdown
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

  // All non-Steam items: optional "clear position" row + type-specific options

  const hasPos = item.launch_x != null && item.launch_y != null;
  if (hasPos) {
    const posRow = document.createElement('div');
    posRow.className = 'item-expand-row';
    posRow.innerHTML = `
      <span style="color:#888;font-size:11px;">Position saved</span>
      <button class="coord-clear" style="background:none;border:none;color:#555;font-size:11px;cursor:pointer;padding:0 4px;" title="Clear">✕ Clear</button>
    `;
    posRow.querySelector('.coord-clear').addEventListener('click', () => {
      currentItems[idx].launch_x = null;
      currentItems[idx].launch_y = null;
      currentItems[idx].launch_width = null;
      currentItems[idx].launch_height = null;
      renderItems();
    });
    panel.appendChild(posRow);
  }

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

  if (item.item_type === 'app') {
    const adminRow = document.createElement('div');
    adminRow.className = 'item-expand-row';
    const checked = item.run_as_admin ? 'checked' : '';
    adminRow.innerHTML = `
      <label class="run-toggle">
        <input type="checkbox" class="admin-checkbox" ${checked} />
        🛡 Run as admin
      </label>
    `;
    adminRow.querySelector('.admin-checkbox').addEventListener('change', (e) => {
      currentItems[idx].run_as_admin = e.target.checked;
    });
    panel.appendChild(adminRow);
  }

  return panel;
}
```

- [ ] **Step 5: Delete picker.html and picker.js**

```powershell
Remove-Item "C:\Users\dougb\Desktop\AppLauncher\src\picker.html"
Remove-Item "C:\Users\dougb\Desktop\AppLauncher\src\picker.js"
```

- [ ] **Step 6: Build to verify**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher"
npm run build 2>&1 | Select-String "error|Error"
```

Expected: no errors. `dist/picker.html` should NOT exist; `dist/layout-item.html` should exist.

- [ ] **Step 7: Verify in the app**

Run `npm run tauri dev`. Open a group editor. Test:

1. **Edit Layout button** — click "📐 Edit Layout". One window opens per item, each labeled with the item name. Drag them around. Click "Save All Positions" — all windows close and the positions persist in the group (save the group and verify via `config.json`).

2. **Cancel** — click "📐 Edit Layout" again, then click "Cancel All" on any window — all windows close, no positions changed.

3. **Run as admin** — expand an App item — "🛡 Run as admin" checkbox appears. Toggle it, save group, verify `run_as_admin: true` in config.json.

4. **Clear position** — if an item has a saved position, expand it and verify "Position saved ✕ Clear" appears. Click Clear — position is removed.

5. **Script and Steam items** — verify their expand panels still work (Run via cmd checkbox, monitor dropdown).

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/config.html src/config.js vite.config.js
git add -u src/picker.html src/picker.js
git commit -m "feat: layout editor replaces per-item picker, adds Edit Layout button"
```
