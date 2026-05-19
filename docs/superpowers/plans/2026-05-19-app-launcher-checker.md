# App Launcher Checker — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a three-component dev tool — Tauri HTTP server (dev-only), structured debug log, and MCP tool — so Claude can trigger actions, observe state, and mutate config without user involvement.

**Architecture:** A minimal axum HTTP server starts on port 7891 in debug builds only; `debug_log.rs` appends timestamped entries at key execution points (no-op in release); `check_app_launcher.js` in local-hub calls the HTTP server for live interactions and reads/writes `config.json` directly for mutations.

**Tech Stack:** Rust (axum 0.7, tokio via Tauri, Win32 externs), Node.js (zod, global fetch, execSync, PowerShell for screenshots + registry reads)

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src-tauri/src/debug_log.rs` | **Create** | `write_debug_log(msg)` — appends timestamped line to `%TEMP%\applauncher-debug.log`; no-op in release |
| `src-tauri/src/debug_server.rs` | **Create** | axum server on port 7891; 6 endpoints; dev builds only |
| `src-tauri/src/lib.rs` | **Modify** | Make `AppState` + `open_config_window` `pub(crate)`; declare modules; start server on setup |
| `src-tauri/src/launcher.rs` | **Modify** | Add `write_debug_log` calls at group/item launch points |
| `src-tauri/src/virtual_desktop.rs` | **Modify** | Add `write_debug_log` calls at switch points |
| `src-tauri/Cargo.toml` | **Modify** | Add `axum = "0.7"` |
| `C:\Users\dougb\Desktop\Claude\tools\check_app_launcher.js` | **Create** | 18-action MCP tool |

---

## Task 1: Create `debug_log.rs`

**Files:**
- Create: `src-tauri/src/debug_log.rs`

- [ ] **Create `src-tauri/src/debug_log.rs` with this content:**

```rust
use std::io::Write;

/// Appends a timestamped line to %TEMP%\applauncher-debug.log.
/// Compiled to a no-op in release builds.
pub fn write_debug_log(msg: &str) {
    #[cfg(debug_assertions)]
    {
        let path = std::env::temp_dir().join("applauncher-debug.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "[{}] {}", local_time(), msg);
        }
    }
    // Release build: intentionally empty — zero overhead
    #[cfg(not(debug_assertions))]
    let _ = msg;
}

/// Truncates the log file.
pub fn clear_debug_log() {
    let path = std::env::temp_dir().join("applauncher-debug.log");
    let _ = std::fs::write(&path, "");
}

#[cfg(target_os = "windows")]
fn local_time() -> String {
    #[repr(C)]
    struct SYSTEMTIME { year: u16, month: u16, dow: u16, day: u16, hour: u16, min: u16, sec: u16, ms: u16 }
    extern "system" { fn GetLocalTime(t: *mut SYSTEMTIME); }
    let mut t = SYSTEMTIME { year: 0, month: 0, dow: 0, day: 0, hour: 0, min: 0, sec: 0, ms: 0 };
    unsafe { GetLocalTime(&mut t); }
    format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec)
}

#[cfg(not(target_os = "windows"))]
fn local_time() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:02}:{:02}:{:02}", (s % 86400) / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_debug_log_creates_file_and_appends() {
        #[cfg(debug_assertions)]
        {
            let path = std::env::temp_dir().join("applauncher-debug.log");
            let _ = std::fs::remove_file(&path);
            write_debug_log("TEST_ENTRY_A");
            write_debug_log("TEST_ENTRY_B");
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("TEST_ENTRY_A"), "log should contain first entry");
            assert!(content.contains("TEST_ENTRY_B"), "log should contain second entry");
            assert_eq!(content.lines().count(), 2, "should have exactly 2 lines");
        }
    }

    #[test]
    fn test_clear_debug_log_empties_file() {
        #[cfg(debug_assertions)]
        {
            write_debug_log("WILL_BE_CLEARED");
            clear_debug_log();
            let path = std::env::temp_dir().join("applauncher-debug.log");
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(content.is_empty(), "log should be empty after clear");
        }
    }
}
```

- [ ] **Declare module in `src-tauri/src/lib.rs`** — add after the existing `mod` declarations at the top:

```rust
mod debug_log;
```

- [ ] **Run tests:**

```
cargo test --manifest-path src-tauri/Cargo.toml debug_log
```

Expected: 2 tests pass.

- [ ] **Commit:**

```
git add src-tauri/src/debug_log.rs src-tauri/src/lib.rs
git commit -m "feat: add debug_log module with write_debug_log and clear_debug_log"
```

---

## Task 2: Add Log Calls to `launcher.rs`

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Add `write_debug_log` calls to `launch_group`.** Find the line `pub fn launch_group(group_id: &str, config: &AppConfig) -> Result<(), String> {` and insert log calls as shown. The final function opening should look like:

```rust
pub fn launch_group(group_id: &str, config: &AppConfig) -> Result<(), String> {
    let group = config
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| format!("Group '{}' not found", group_id))?;

    crate::debug_log::write_debug_log(&format!(
        "LAUNCH group \"{}\" ({} items)", group.name, group.items.len()
    ));
```

- [ ] **Add per-item log calls** inside the non-URL items loop. Find the loop that starts with `for item in &group.items { if !matches!(item.item_type, ItemType::Url)` and add logging before `launch_item`:

```rust
    for item in &group.items {
        if !matches!(item.item_type, ItemType::Url) {
            #[cfg(target_os = "windows")]
            if let Some(ref guid) = item.launch_virtual_desktop {
                if guid.as_slice() != current_desktop.as_slice() {
                    crate::debug_log::write_debug_log(&format!(
                        "LAUNCH item \"{}\" switching desktop",
                        item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                    ));
                    crate::virtual_desktop::switch_virtual_desktop(&current_desktop, guid);
                    current_desktop = guid.clone();
                }
            }
            crate::debug_log::write_debug_log(&format!(
                "LAUNCH item type={:?} path=\"{}\"",
                item.item_type,
                item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
            ));
            launch_item(item, &config.preferred_browser)?;
        }
    }
```

- [ ] **Add log call in `apply_window_placement`** — after `place_window(found as *mut _, x, y, w, h);`:

```rust
fn apply_window_placement(found: usize, x: i32, y: i32, w: Option<u32>, h: Option<u32>) {
    use std::thread;
    use std::time::Duration;
    place_window(found as *mut _, x, y, w, h);
    crate::debug_log::write_debug_log(&format!(
        "LAUNCH window HWND 0x{:X} positioned at ({}, {}) {}x{}",
        found, x, y,
        w.unwrap_or(0), h.unwrap_or(0)
    ));
```

- [ ] **Add log in `poll_for_new_window`** at the point where a window is found. Find the `return Some(h)` lines and wrap in a log:

  In the tier-1 PID match block:
  ```rust
  if let Some(&h) = new_hwnds.iter().find(|&&h| get_hwnd_pid(h) == pid) {
      crate::debug_log::write_debug_log(&format!("LAUNCH HWND 0x{:X} found (PID match) poll={}", h, i));
      return Some(h);
  }
  ```
  In the tier-2 exe match block:
  ```rust
  if let Some(&h) = new_hwnds.iter().find(|&&h| get_hwnd_exe(h).as_deref() == Some(exe)) {
      crate::debug_log::write_debug_log(&format!("LAUNCH HWND 0x{:X} found (exe match) poll={}", h, i));
      return Some(h);
  }
  ```
  In the tier-3 any-new block:
  ```rust
  if i == polls - 1 {
      let h = new_hwnds.into_iter().next();
      crate::debug_log::write_debug_log(&format!("LAUNCH HWND {:?} found (any-new fallback)", h));
      return h;
  }
  ```

- [ ] **Build to verify no errors:**

```
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: `Finished dev profile`.

- [ ] **Commit:**

```
git add src-tauri/src/launcher.rs
git commit -m "feat: add debug log calls to launcher.rs"
```

---

## Task 3: Add Log Calls to `virtual_desktop.rs`

**Files:**
- Modify: `src-tauri/src/virtual_desktop.rs`

- [ ] **Add log call at switch start** in `switch_vd_windows`, after `if current_idx == target_idx { return true; }`:

```rust
    if current_idx == target_idx { return true; }

    crate::debug_log::write_debug_log(&format!(
        "VD switch Desktop{}→Desktop{} ({} step(s))",
        current_idx + 1, target_idx + 1,
        if target_idx > current_idx { target_idx - current_idx } else { current_idx - target_idx }
    ));
```

- [ ] **Add log after each poll confirmation** in the polling loop. Find `if std::time::Instant::now() >= deadline { break; }` and add before it:

```rust
        let deadline = std::time::Instant::now() + Duration::from_millis(600);
        let poll_start = std::time::Instant::now();
        loop {
            thread::sleep(Duration::from_millis(50));
            if let Some(cur) = get_current_vd_windows() {
                if cur.as_slice() == expected_guid.as_slice() {
                    crate::debug_log::write_debug_log(&format!(
                        "VD switch confirmed after {}ms",
                        poll_start.elapsed().as_millis()
                    ));
                    break;
                }
            }
            if std::time::Instant::now() >= deadline {
                crate::debug_log::write_debug_log("VD switch timed out after 600ms — proceeding");
                break;
            }
        }
```

- [ ] **Build:**

```
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: `Finished dev profile`.

- [ ] **Commit:**

```
git add src-tauri/src/virtual_desktop.rs
git commit -m "feat: add debug log calls to virtual_desktop.rs"
```

---

## Task 4: Add `axum` and Expose `AppState` / `open_config_window`

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Add axum to `src-tauri/Cargo.toml`** — in the `[dependencies]` section:

```toml
axum = "0.7"
```

- [ ] **Make `AppState` pub(crate)** in `src-tauri/src/lib.rs`. Change:

```rust
struct AppState(Mutex<AppConfig>);
```
to:
```rust
pub(crate) struct AppState(Mutex<AppConfig>);
```

- [ ] **Make `open_config_window` pub(crate)** in `src-tauri/src/lib.rs`. Change:

```rust
fn open_config_window(app: tauri::AppHandle, group_id: Option<String>) {
```
to:
```rust
pub(crate) fn open_config_window(app: tauri::AppHandle, group_id: Option<String>) {
```

- [ ] **Build to verify axum resolves:**

```
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: `Finished dev profile` (axum downloads and compiles).

- [ ] **Commit:**

```
git add src-tauri/Cargo.toml src-tauri/src/lib.rs
git commit -m "feat: add axum dep, expose AppState and open_config_window as pub(crate)"
```

---

## Task 5: Create `debug_server.rs` — Core Endpoints

**Files:**
- Create: `src-tauri/src/debug_server.rs`

- [ ] **Create `src-tauri/src/debug_server.rs`:**

```rust
//! HTTP debug server — only compiled and started in debug builds.
//! Binds to 127.0.0.1:7891. Gives Claude a live API into the running app.

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use std::sync::Arc;
use tauri::Manager;

type AppHandle = Arc<tauri::AppHandle>;

pub fn start(app: tauri::AppHandle) {
    let state: AppHandle = Arc::new(app);
    tauri::async_runtime::spawn(async move {
        let router = Router::new()
            .route("/state",              get(get_state))
            .route("/launch/:group_id",   post(do_launch))
            .route("/edit/:group_id",     post(do_edit))
            .route("/reload",             post(do_reload))
            .route("/log",                get(get_log))
            .route("/windows",            get(get_windows))
            .with_state(state);

        match tokio::net::TcpListener::bind("127.0.0.1:7891").await {
            Ok(listener) => {
                eprintln!("[debug_server] Listening on http://127.0.0.1:7891");
                let _ = axum::serve(listener, router).await;
            }
            Err(e) => eprintln!("[debug_server] Failed to bind port 7891: {e}"),
        }
    });
}

// GET /state — full config from live AppState
async fn get_state(State(app): State<AppHandle>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap().clone();
    Json(config)
}

// POST /launch/:group_id
async fn do_launch(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    let config = app.state::<crate::AppState>().0.lock().unwrap().clone();
    match crate::launcher::launch_group(&group_id, &config) {
        Ok(_)  => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

// POST /edit/:group_id
async fn do_edit(State(app): State<AppHandle>, Path(group_id): Path<String>) -> impl IntoResponse {
    crate::open_config_window((*app).clone(), Some(group_id));
    Json(serde_json::json!({ "ok": true }))
}

// POST /reload — re-read config.json into AppState
async fn do_reload(State(app): State<AppHandle>) -> impl IntoResponse {
    let new_config = crate::config::load_config();
    *app.state::<crate::AppState>().0.lock().unwrap() = new_config;
    Json(serde_json::json!({ "ok": true }))
}

// GET /log — return debug log file contents
async fn get_log() -> impl IntoResponse {
    let path = std::env::temp_dir().join("applauncher-debug.log");
    std::fs::read_to_string(&path).unwrap_or_default()
}

// GET /windows — enumerate visible windows with title, rect, virtual desktop
async fn get_windows() -> impl IntoResponse {
    // Use spawn_blocking so COM (IVirtualDesktopManager) runs on a dedicated thread
    // where CoInitializeEx can succeed without conflicting with tokio's MTA threads.
    let windows = tokio::task::spawn_blocking(enumerate_windows)
        .await
        .unwrap_or_default();
    Json(windows)
}

fn enumerate_windows() -> Vec<serde_json::Value> {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn EnumWindows(cb: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, data: isize) -> i32;
            fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
            fn GetWindowTextW(hwnd: *mut std::ffi::c_void, buf: *mut u16, max: i32) -> i32;
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        }

        let mut results: Vec<serde_json::Value> = Vec::new();

        unsafe extern "system" fn cb(hwnd: *mut std::ffi::c_void, data: isize) -> i32 {
            if IsWindowVisible(hwnd) == 0 { return 1; }

            let results = &mut *(data as *mut Vec<serde_json::Value>);

            // Title — skip windows with no title
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len == 0 { return 1; }
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            // Rect
            let mut rect = [0i32; 4];
            GetWindowRect(hwnd, &mut rect);
            let (x, y, w, h) = (rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]);

            // Virtual desktop GUID (hex string, empty string if unavailable)
            let vd_guid = crate::virtual_desktop::get_window_virtual_desktop(hwnd)
                .map(|g| g.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(""))
                .unwrap_or_default();

            results.push(serde_json::json!({
                "title": title,
                "x": x, "y": y, "width": w, "height": h,
                "virtual_desktop_guid": vd_guid,
            }));
            1
        }

        unsafe { EnumWindows(cb, &mut results as *mut _ as isize); }
        results
    }

    #[cfg(not(target_os = "windows"))]
    vec![]
}
```

- [ ] **Declare module in `lib.rs`** — add inside the `#[cfg(debug_assertions)]` block (or at the top with the other mods, gated):

```rust
#[cfg(debug_assertions)]
mod debug_server;
```

Add this line after `mod debug_log;`.

- [ ] **Build:**

```
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: `Finished dev profile`.

- [ ] **Commit:**

```
git add src-tauri/src/debug_server.rs src-tauri/src/lib.rs
git commit -m "feat: add debug_server.rs with 6 HTTP endpoints (dev-only)"
```

---

## Task 6: Start Debug Server in `lib.rs` Setup

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Find the setup block** in `lib.rs` — it ends with `Ok(())` just before `.manage(AppState(...))`. Insert the server start inside `#[cfg(debug_assertions)]` at the end of the setup closure, before `Ok(())`:

```rust
            // Start debug HTTP server on port 7891 (dev builds only)
            #[cfg(debug_assertions)]
            debug_server::start(app.handle().clone());

            Ok(())
```

- [ ] **Run the app in dev mode and verify:**

```
npm run tauri dev
```

Expected: console prints `[debug_server] Listening on http://127.0.0.1:7891` within a few seconds of startup.

- [ ] **Smoke test the API** (in a separate terminal):

```powershell
Invoke-RestMethod http://127.0.0.1:7891/state | ConvertTo-Json -Depth 5
```

Expected: JSON output with `groups` array matching `%LOCALAPPDATA%\AppLauncher\config.json`.

- [ ] **Commit:**

```
git add src-tauri/src/lib.rs
git commit -m "feat: start debug_server on app launch in dev builds"
```

---

## Task 7: Create MCP Tool — Scaffold + Observe Actions

**Files:**
- Create: `C:\Users\dougb\Desktop\Claude\tools\check_app_launcher.js`

- [ ] **Create the file with scaffold + observe actions:**

```js
'use strict';
const { z } = require('zod');
const { execSync } = require('child_process');
const fs   = require('fs');
const path = require('path');

const API  = 'http://127.0.0.1:7891';
const CFG  = path.join(process.env.LOCALAPPDATA, 'AppLauncher', 'config.json');
const LOG  = path.join(process.env.TEMP, 'applauncher-debug.log');
const TEMP = process.env.TEMP;

// ── HTTP helpers ─────────────────────────────────────────────────────────────

async function api(method, endpoint, body) {
  let res;
  try {
    res = await fetch(`${API}${endpoint}`, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : {},
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch {
    throw new Error(
      `App Launcher dev server not responding on port 7891 — is \`npm run tauri dev\` running?`
    );
  }
  const ct = res.headers.get('content-type') || '';
  return ct.includes('json') ? res.json() : res.text();
}

// ── Config helpers ────────────────────────────────────────────────────────────

function readConfig() {
  return JSON.parse(fs.readFileSync(CFG, 'utf8'));
}

async function writeConfig(config) {
  fs.writeFileSync(CFG, JSON.stringify(config, null, 2), 'utf8');
  await api('POST', '/reload');
}

function findGroup(config, target) {
  const g = config.groups.find(
    g => g.name.toLowerCase() === target.toLowerCase() || g.id === target
  );
  if (!g) throw new Error(
    `Group "${target}" not found. Available: ${config.groups.map(g => g.name).join(', ')}`
  );
  return g;
}

function getItemOrThrow(config, target, itemIndex) {
  const g = findGroup(config, target);
  if (itemIndex < 0 || itemIndex >= g.items.length)
    throw new Error(`item_index ${itemIndex} out of range (group has ${g.items.length} items)`);
  return { group: g, item: g.items[itemIndex] };
}

// ── Screenshot ────────────────────────────────────────────────────────────────

function takeScreenshots(monitorIndex) {
  const scriptPath = path.join(TEMP, 'al_screenshot.ps1');
  const filterLine = monitorIndex != null
    ? `$screens = @([System.Windows.Forms.Screen]::AllScreens[${monitorIndex}])`
    : `$screens = [System.Windows.Forms.Screen]::AllScreens`;
  const script = `
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
${filterLine}
$paths = @()
$i = 0
foreach ($screen in $screens) {
    $bmp = New-Object System.Drawing.Bitmap($screen.Bounds.Width, $screen.Bounds.Height)
    $g   = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($screen.Bounds.Location, [System.Drawing.Point]::Empty, $screen.Bounds.Size)
    $p = "$env:TEMP\\al_check_$i.png"
    $bmp.Save($p)
    $g.Dispose(); $bmp.Dispose()
    $paths += $p
    $i++
}
$paths -join '|'
`;
  fs.writeFileSync(scriptPath, script, 'utf8');
  const out = execSync(
    `powershell -NoProfile -ExecutionPolicy Bypass -File "${scriptPath}"`,
    { timeout: 15000 }
  ).toString().trim();
  return out.split('|').filter(Boolean).map((p, i) => ({
    type: 'image',
    data: fs.readFileSync(p.trim()).toString('base64'),
    mimeType: 'image/png',
    description: `Monitor ${i}`,
  }));
}

// ── Desktop helpers ───────────────────────────────────────────────────────────

function listDesktops() {
  const scriptPath = path.join(TEMP, 'al_desktops.ps1');
  const script = `
$path = 'HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops'
$ids     = (Get-ItemProperty $path).VirtualDesktopIDs
$current = (Get-ItemProperty $path).CurrentVirtualDesktop
$result  = @()
for ($i = 0; $i -lt $ids.Length; $i += 16) {
    $bytes     = $ids[$i..($i+15)]
    $hex       = ($bytes | ForEach-Object { $_.ToString('X2') }) -join ''
    $curHex    = ($current | ForEach-Object { $_.ToString('X2') }) -join ''
    $result   += [PSCustomObject]@{
        index      = [int]($i/16) + 1
        name       = "Desktop $([int]($i/16)+1)"
        guid_hex   = $hex
        is_current = ($hex -eq $curHex)
    }
}
ConvertTo-Json $result -Compress
`;
  fs.writeFileSync(scriptPath, script, 'utf8');
  return JSON.parse(
    execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${scriptPath}"`, { timeout: 5000 }).toString().trim()
  );
}

function desktopIndexToGuid(desktopIndex) {
  // desktopIndex is 1-based
  const scriptPath = path.join(TEMP, 'al_get_guid.ps1');
  const script = `
$path  = 'HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops'
$ids   = (Get-ItemProperty $path).VirtualDesktopIDs
$start = (${desktopIndex - 1}) * 16
$bytes = $ids[$start..($start+15)]
$bytes -join ','
`;
  fs.writeFileSync(scriptPath, script, 'utf8');
  return execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${scriptPath}"`, { timeout: 5000 })
    .toString().trim().split(',').map(Number);
}

// ── Position comparison ───────────────────────────────────────────────────────

async function buildLaunchReport(groupName) {
  const [windows, config] = await Promise.all([
    api('GET', '/windows'),
    Promise.resolve(readConfig()),
  ]);
  const screenshots = takeScreenshots();
  const group = findGroup(config, groupName);
  const desktops = listDesktops();
  const desktopByGuid = Object.fromEntries(desktops.map(d => [d.guid_hex.toUpperCase(), d.name]));

  const rows = group.items.map((item, idx) => {
    const label = (item.path || item.value || `Item ${idx}`).split(/[/\\]/).pop();
    const stem  = label.replace(/\.[^.]+$/, '').toLowerCase();

    const match = windows.find(w =>
      w.title && (
        w.title.toLowerCase().includes(stem) ||
        stem.includes(w.title.toLowerCase().slice(0, 8))
      )
    );

    let row = `Item ${idx} "${label}"`;

    if (item.launch_x != null) {
      row += `\n  Config saved:  x=${item.launch_x} y=${item.launch_y} ${item.launch_width}×${item.launch_height}`;
      if (match) {
        const dx = Math.abs(match.x - item.launch_x);
        const dy = Math.abs(match.y - item.launch_y);
        const posOk = dx <= 5 && dy <= 5;
        row += `\n  OS reported:   x=${match.x} y=${match.y} ${match.width}×${match.height}  ${posOk ? '✓ match' : `✗ delta x=${dx} y=${dy}`}`;
      } else {
        row += `\n  OS reported:   (no window found — may be on a different desktop)`;
      }
    } else {
      row += `\n  (no saved position)`;
    }

    if (item.launch_virtual_desktop && match) {
      const savedHex   = item.launch_virtual_desktop.map(b => b.toString(16).padStart(2, '0').toUpperCase()).join('');
      const reportedHex = (match.virtual_desktop_guid || '').toUpperCase();
      const savedName  = desktopByGuid[savedHex]  || savedHex;
      const actualName = desktopByGuid[reportedHex] || reportedHex || '?';
      const vdOk = savedHex === reportedHex;
      row += `\n  Desktop:       expected=${savedName}  actual=${actualName}  ${vdOk ? '✓' : '✗ MISMATCH'}`;
    } else if (item.launch_virtual_desktop) {
      const savedHex  = item.launch_virtual_desktop.map(b => b.toString(16).padStart(2, '0').toUpperCase()).join('');
      const savedName = desktopByGuid[savedHex] || savedHex;
      row += `\n  Desktop:       expected=${savedName}  (window not found to verify)`;
    }

    return row;
  });

  const logTail = fs.existsSync(LOG)
    ? fs.readFileSync(LOG, 'utf8').split('\n').slice(-50).join('\n')
    : '(log empty)';

  return [
    { type: 'text', text: `=== Position Report for "${groupName}" ===\n\n${rows.join('\n\n')}` },
    { type: 'text', text: `=== Debug Log (last 50 lines) ===\n\n${logTail}` },
    ...screenshots,
  ];
}

// ── Main handler ──────────────────────────────────────────────────────────────

module.exports = {
  description:
    'Interact with the running App Launcher dev server for debugging and iteration. ' +
    'Use action + optional target (group name) + optional params. ' +
    'Requires `npm run tauri dev` to be running.',

  schema: {
    action: z.enum([
      // Observe
      'screenshot', 'log', 'clear_log', 'config',
      'list_groups', 'list_desktops', 'get_windows',
      // App interactions
      'launch', 'open_edit', 'reload',
      // Config mutations
      'add_group', 'delete_group', 'rename_group',
      'add_item', 'remove_item',
      'set_item_desktop', 'set_item_position', 'clear_item_position',
      // Utility
      'wait',
    ]).describe('Action to perform'),
    target: z.string().optional().describe('Group name (for most actions)'),
    params: z.record(z.any()).optional().describe(
      'Action-specific params. ' +
      'screenshot: {monitor?:number}  ' +
      'log: {lines?:number}  ' +
      'launch: {wait_ms?:number}  ' +
      'add_group: {name,icon}  ' +
      'rename_group: {name}  ' +
      'add_item: {item_type,path,value?}  ' +
      'remove_item/set_item_desktop/set_item_position/clear_item_position: {item_index}  ' +
      'set_item_desktop: +{desktop:number(1-based)}  ' +
      'set_item_position: +{x,y,width,height}  ' +
      'wait: {ms:number}'
    ),
  },

  handler: async ({ action, target, params = {} }) => {
    try {
      switch (action) {

        // ── Observe ─────────────────────────────────────────────────────────

        case 'screenshot':
          return { content: takeScreenshots(params.monitor) };

        case 'log': {
          const lines = params.lines ?? 100;
          const content = fs.existsSync(LOG)
            ? fs.readFileSync(LOG, 'utf8').split('\n').slice(-lines).join('\n')
            : '(log file does not exist yet)';
          return { content: [{ type: 'text', text: content }] };
        }

        case 'clear_log':
          fs.writeFileSync(LOG, '', 'utf8');
          return { content: [{ type: 'text', text: 'Log cleared.' }] };

        case 'config':
          return { content: [{ type: 'text', text: JSON.stringify(readConfig(), null, 2) }] };

        case 'list_groups': {
          const cfg = readConfig();
          const rows = cfg.groups.map(g =>
            `${g.icon} "${g.name}"  id=${g.id}  items=${g.items.length}`
          );
          return { content: [{ type: 'text', text: rows.join('\n') || '(no groups)' }] };
        }

        case 'list_desktops': {
          const desktops = listDesktops();
          const rows = desktops.map(d =>
            `Desktop ${d.index}${d.is_current ? ' (current)' : ''}  guid=${d.guid_hex}`
          );
          return { content: [{ type: 'text', text: rows.join('\n') }] };
        }

        case 'get_windows': {
          const windows = await api('GET', '/windows');
          const rows = windows.map(w =>
            `"${w.title}"  pos=(${w.x},${w.y}) size=${w.width}×${w.height}  vd=${w.virtual_desktop_guid || '?'}`
          );
          return { content: [{ type: 'text', text: rows.join('\n') || '(no windows)' }] };
        }

        // ── App interactions ─────────────────────────────────────────────────

        case 'launch': {
          if (!target) throw new Error('launch requires target (group name)');
          const cfg = readConfig();
          const group = findGroup(cfg, target);
          await api('POST', `/launch/${group.id}`);
          const waitMs = params.wait_ms ?? 3000;
          await new Promise(r => setTimeout(r, waitMs));
          return { content: await buildLaunchReport(target) };
        }

        case 'open_edit': {
          if (!target) throw new Error('open_edit requires target (group name)');
          const cfg = readConfig();
          const group = findGroup(cfg, target);
          await api('POST', `/edit/${group.id}`);
          await new Promise(r => setTimeout(r, 600));
          return { content: takeScreenshots() };
        }

        case 'reload': {
          await api('POST', '/reload');
          return { content: [{ type: 'text', text: 'App config reloaded from disk.' }] };
        }

        // ── Config mutations ─────────────────────────────────────────────────

        case 'add_group': {
          const { name, icon = '📁' } = params;
          if (!name) throw new Error('add_group requires params.name');
          const cfg = readConfig();
          cfg.groups.push({ id: crypto.randomUUID(), name, icon, items: [] });
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Group "${name}" added.` }] };
        }

        case 'delete_group': {
          if (!target) throw new Error('delete_group requires target (group name)');
          const cfg = readConfig();
          const before = cfg.groups.length;
          cfg.groups = cfg.groups.filter(
            g => g.name.toLowerCase() !== target.toLowerCase() && g.id !== target
          );
          if (cfg.groups.length === before) throw new Error(`Group "${target}" not found`);
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Group "${target}" deleted.` }] };
        }

        case 'rename_group': {
          if (!target) throw new Error('rename_group requires target');
          if (!params.name) throw new Error('rename_group requires params.name');
          const cfg = readConfig();
          const g = findGroup(cfg, target);
          g.name = params.name;
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Group renamed to "${params.name}".` }] };
        }

        case 'add_item': {
          if (!target) throw new Error('add_item requires target (group name)');
          const { item_type, path: itemPath, value = null } = params;
          if (!item_type || !itemPath) throw new Error('add_item requires params.item_type and params.path');
          const cfg = readConfig();
          const g = findGroup(cfg, target);
          g.items.push({
            item_type,
            path: itemPath,
            value,
            urls: [],
            icon_data: null,
            browser_name: null,
            run_in_terminal: true,
            run_as_admin: false,
            launch_virtual_desktop: null,
            launch_desktop: null,
            launch_x: null,
            launch_y: null,
            launch_width: null,
            launch_height: null,
          });
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Item "${itemPath}" added to "${target}".` }] };
        }

        case 'remove_item': {
          if (!target) throw new Error('remove_item requires target');
          const idx = params.item_index;
          if (idx == null) throw new Error('remove_item requires params.item_index');
          const cfg = readConfig();
          const g = findGroup(cfg, target);
          if (idx < 0 || idx >= g.items.length)
            throw new Error(`item_index ${idx} out of range`);
          const removed = g.items.splice(idx, 1)[0];
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Removed item ${idx} ("${removed.path || removed.value}") from "${target}".` }] };
        }

        case 'set_item_desktop': {
          if (!target) throw new Error('set_item_desktop requires target');
          const { item_index, desktop } = params;
          if (item_index == null || desktop == null)
            throw new Error('set_item_desktop requires params.item_index and params.desktop (1-based)');
          const cfg = readConfig();
          const { item } = getItemOrThrow(cfg, target, item_index);
          item.launch_virtual_desktop = desktopIndexToGuid(desktop);
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Item ${item_index} in "${target}" set to Desktop ${desktop}.` }] };
        }

        case 'set_item_position': {
          if (!target) throw new Error('set_item_position requires target');
          const { item_index, x, y, width, height } = params;
          if ([item_index, x, y, width, height].some(v => v == null))
            throw new Error('set_item_position requires params: item_index, x, y, width, height');
          const cfg = readConfig();
          const { item } = getItemOrThrow(cfg, target, item_index);
          item.launch_x = x;
          item.launch_y = y;
          item.launch_width = width;
          item.launch_height = height;
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Item ${item_index} in "${target}" position set to (${x},${y}) ${width}×${height}.` }] };
        }

        case 'clear_item_position': {
          if (!target) throw new Error('clear_item_position requires target');
          const { item_index } = params;
          if (item_index == null) throw new Error('clear_item_position requires params.item_index');
          const cfg = readConfig();
          const { item } = getItemOrThrow(cfg, target, item_index);
          item.launch_x = null;
          item.launch_y = null;
          item.launch_width = null;
          item.launch_height = null;
          item.launch_virtual_desktop = null;
          await writeConfig(cfg);
          return { content: [{ type: 'text', text: `Item ${item_index} in "${target}" position and desktop cleared.` }] };
        }

        // ── Utility ──────────────────────────────────────────────────────────

        case 'wait': {
          const ms = params.ms ?? 1000;
          await new Promise(r => setTimeout(r, ms));
          return { content: [{ type: 'text', text: `Waited ${ms}ms.` }] };
        }

        default:
          throw new Error(`Unknown action: ${action}`);
      }
    } catch (err) {
      return { content: [{ type: 'text', text: `Error: ${err.message}` }] };
    }
  },
};
```

- [ ] **Build check — verify the file parses without errors:**

```powershell
node -e "require('C:\\Users\\dougb\\Desktop\\Claude\\tools\\check_app_launcher.js'); console.log('OK')"
```

Expected: `OK`

- [ ] **Commit:**

```
git -C "C:\Users\dougb\Desktop\Claude" add tools/check_app_launcher.js
git -C "C:\Users\dougb\Desktop\Claude" commit -m "feat: add check_app_launcher MCP tool (18 actions)"
```

---

## Task 8: Restart Local-Hub + Integration Smoke Test

**Files:** none (runtime verification)

- [ ] **Restart the local-hub server** so it picks up the new tool:

```powershell
# Find and kill the current node process for index.js, then restart
Stop-Process -Name node -Force -ErrorAction SilentlyContinue
Start-Sleep 1
Start-Process node -ArgumentList "C:\Users\dougb\Desktop\Claude\index.js" -WindowStyle Hidden
```

Or simply restart it however you normally do.

- [ ] **Verify the tool appears** — in Claude Code, run:

```
/mcp
```

Expected: `check_app_launcher` appears in the tool list under `local-hub`.

- [ ] **Smoke test `list_groups`** (app must be running in dev mode):

```
check_app_launcher action=list_groups
```

Expected: lists your current groups (e.g., `💼 "Work"  id=...  items=2`).

- [ ] **Smoke test `screenshot`:**

```
check_app_launcher action=screenshot
```

Expected: one or more screenshots of your monitors returned as images.

- [ ] **Smoke test `launch` with position report:**

Start the app with `npm run tauri dev`, then:

```
check_app_launcher action=launch target=Work
```

Expected: waits 3 seconds, returns position comparison table + screenshots of each desktop + last 50 lines of debug log.

- [ ] **Commit the Tauri side changes together** (if not already committed per task):

```
git -C "C:\Users\dougb\Desktop\AppLauncher" add src-tauri/src/lib.rs src-tauri/src/launcher.rs src-tauri/src/virtual_desktop.rs src-tauri/src/debug_log.rs src-tauri/src/debug_server.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git -C "C:\Users\dougb\Desktop\AppLauncher" commit -m "feat: App Launcher Checker — debug server, debug log, launch tracing"
```

---

## Self-Review Notes

- `/windows` handler uses `spawn_blocking` so COM (`IVirtualDesktopManager::GetWindowDesktopId`) runs on a non-tokio thread — avoids MTA conflict.
- `do_edit` calls `open_config_window` which uses `tauri::async_runtime::spawn` internally — safe to call from axum handler.
- `desktopIndexToGuid` writes a temp PS1 file to avoid shell escaping issues with inline PowerShell commands.
- `buildLaunchReport` matches windows to items by title substring heuristic — may miss if app title is very different from filename. This is acceptable for a dev tool; Claude can read the screenshots to fill the gap.
- All mutations call `POST /reload` after writing — keeps AppState in sync so subsequent `/state` calls reflect changes.
