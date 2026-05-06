# App Launcher Feature Set Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three features — installer kills running AppLauncher before upgrading, URL items support multiple URLs with browser icons, and script items get an open/run toggle.

**Architecture:** Feature 1 is NSIS installer config only. Features 2 and 3 share a single `Item` struct change in `config.rs`, then diverge into `launcher.rs` (Rust, TDD) and `config.js` (UI, manual verify). Icon extraction lives in a new `icons.rs` module using raw Win32 calls (same pattern as `launcher.rs`) plus the `image` crate for PNG encoding.

**Tech Stack:** Tauri v2, Rust, Vanilla JS/HTML, Win32 API, NSIS

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Create | `src-tauri/preinstall.nsh` | NSIS kill-before-install hook |
| Modify | `src-tauri/tauri.conf.json` | Wire NSIS hook |
| Modify | `src-tauri/Cargo.toml` | Add `image` crate |
| Modify | `src-tauri/src/config.rs` | Add `urls`, `icon_data`, `browser_name`, `run_in_terminal` to `Item` |
| Create | `src-tauri/src/icons.rs` | `get_file_icon` Win32 implementation |
| Modify | `src-tauri/src/lib.rs` | `mod icons`, register `get_file_icon` command |
| Modify | `src-tauri/src/launcher.rs` | Multi-URL launch + `run_in_terminal` branch |
| Modify | `src/config.js` | Script checkbox, URL item row redesign, URL picker → single item |

---

## Task 1: NSIS Pre-Install Kill Hook

**Files:**
- Create: `src-tauri/preinstall.nsh`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Create the NSIS hook file**

Create `src-tauri/preinstall.nsh`:

```nsis
; Kill any running App Launcher process before installing.
; nsExec::Exec is fire-and-forget — non-zero exit (nothing running) is ignored.
nsExec::Exec 'taskkill /F /IM "App Launcher.exe"'
Pop $0
```

- [ ] **Step 2: Wire it in tauri.conf.json**

In `src-tauri/tauri.conf.json`, add an `"nsis"` object inside `"bundle" > "windows"`:

```json
"windows": {
  "certificateThumbprint": null,
  "digestAlgorithm": "sha256",
  "timestampUrl": "",
  "nsis": {
    "preinstallSection": "preinstall.nsh"
  }
}
```

- [ ] **Step 3: Verify config parses**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher"
npm run tauri build -- --no-bundle 2>&1 | Select-String "error"
```

Expected: no config parse errors. (Full installer build not required — this just confirms the config is valid Tauri JSON.)

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/preinstall.nsh src-tauri/tauri.conf.json
git commit -m "feat: kill running AppLauncher before NSIS install"
```

---

## Task 2: Add New Fields to Item in config.rs

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write failing tests**

Add these tests to the `#[cfg(test)]` block at the bottom of `src-tauri/src/config.rs`:

```rust
#[test]
fn test_item_run_in_terminal_defaults_to_true_when_absent() {
    let json = r#"{"item_type":"script","path":"/foo.bat","value":null}"#;
    let item: Item = serde_json::from_str(json).unwrap();
    assert!(item.run_in_terminal, "run_in_terminal should default to true");
}

#[test]
fn test_item_urls_defaults_to_empty_when_absent() {
    let json = r#"{"item_type":"url","path":null,"value":"https://a.com"}"#;
    let item: Item = serde_json::from_str(json).unwrap();
    assert!(item.urls.is_empty(), "urls should default to empty vec");
    assert!(item.icon_data.is_none());
    assert!(item.browser_name.is_none());
}

#[test]
fn test_item_new_fields_roundtrip() {
    let item = Item {
        item_type: ItemType::Url,
        path: Some("chrome.exe".into()),
        value: Some("https://a.com".into()),
        urls: vec!["https://a.com".into(), "https://b.com".into()],
        icon_data: Some("abc123".into()),
        browser_name: Some("Chrome".into()),
        run_in_terminal: false,
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let json = serde_json::to_string(&item).unwrap();
    let loaded: Item = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.urls, vec!["https://a.com", "https://b.com"]);
    assert_eq!(loaded.icon_data.as_deref(), Some("abc123"));
    assert_eq!(loaded.browser_name.as_deref(), Some("Chrome"));
    assert!(!loaded.run_in_terminal);
}
```

- [ ] **Step 2: Run tests — expect compile failure**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "error\[|FAILED"
```

Expected: compile errors about unknown fields `run_in_terminal`, `urls`, `icon_data`, `browser_name` on `Item`.

- [ ] **Step 3: Add fields to the Item struct**

In `src-tauri/src/config.rs`, replace the `Item` struct with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub item_type: ItemType,
    pub path: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub icon_data: Option<String>,
    #[serde(default)]
    pub browser_name: Option<String>,
    #[serde(default = "default_true")]
    pub run_in_terminal: bool,
    #[serde(default)]
    pub launch_desktop: Option<u32>,
    #[serde(default)]
    pub launch_x: Option<i32>,
    #[serde(default)]
    pub launch_y: Option<i32>,
    #[serde(default)]
    pub launch_width: Option<u32>,
    #[serde(default)]
    pub launch_height: Option<u32>,
}
```

- [ ] **Step 4: Fix all Item struct literal callsites in tests**

The existing tests in config.rs and launcher.rs construct `Item { ... }` with named fields. Every such literal now needs the new fields added. Find them all:

```powershell
grep -n "Item {" src-tauri/src/config.rs src-tauri/src/launcher.rs
```

For every `Item { item_type: ..., path: ..., value: ..., launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None }` literal, add:

```rust
urls: vec![],
icon_data: None,
browser_name: None,
run_in_terminal: true,
```

- [ ] **Step 5: Run tests — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.` No failures.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/config.rs
git commit -m "feat: add urls, icon_data, browser_name, run_in_terminal to Item"
```

---

## Task 3: Add get_file_icon Command

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/icons.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the `image` crate to Cargo.toml**

In `src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
image = { version = "0.25", default-features = false, features = ["png"] }
```

- [ ] **Step 2: Create src-tauri/src/icons.rs**

Create `src-tauri/src/icons.rs` with the full content below:

```rust
pub fn get_file_icon(path: String) -> Option<String> {
    #[cfg(target_os = "windows")]
    return get_file_icon_windows(&path);
    #[cfg(not(target_os = "windows"))]
    return None;
}

#[cfg(target_os = "windows")]
fn get_file_icon_windows(path: &str) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShFileInfoW {
        h_icon: *mut std::ffi::c_void,
        i_icon: i32,
        dw_attributes: u32,
        sz_display_name: [u16; 260],
        sz_type_name: [u16; 80],
    }

    #[repr(C)]
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        h_bm_mask: *mut std::ffi::c_void,
        h_bm_color: *mut std::ffi::c_void,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    extern "system" {
        fn SHGetFileInfoW(
            psz_path: *const u16,
            dw_file_attributes: u32,
            psfi: *mut ShFileInfoW,
            cb_file_info: u32,
            u_flags: u32,
        ) -> usize;
        fn DestroyIcon(h_icon: *mut std::ffi::c_void) -> i32;
        fn GetIconInfo(h_icon: *mut std::ffi::c_void, p_icon_info: *mut IconInfo) -> i32;
        fn CreateCompatibleDC(hdc: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GetDIBits(
            hdc: *mut std::ffi::c_void,
            hbm: *mut std::ffi::c_void,
            start: u32,
            c_lines: u32,
            lpv_bits: *mut std::ffi::c_void,
            lpbmi: *mut BitmapInfoHeader,
            usage: u32,
        ) -> i32;
        fn DeleteDC(hdc: *mut std::ffi::c_void) -> i32;
        fn DeleteObject(hgdiobj: *mut std::ffi::c_void) -> i32;
    }

    const SHGFI_ICON: u32 = 0x0000_0100;
    const SHGFI_LARGEICON: u32 = 0x0000_0000;
    const DIB_RGB_COLORS: u32 = 0;
    const SIZE: u32 = 32;

    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sfi = ShFileInfoW {
        h_icon: std::ptr::null_mut(),
        i_icon: 0,
        dw_attributes: 0,
        sz_display_name: [0u16; 260],
        sz_type_name: [0u16; 80],
    };

    let result = unsafe {
        SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut sfi,
            std::mem::size_of::<ShFileInfoW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };

    if result == 0 || sfi.h_icon.is_null() {
        return None;
    }

    let h_icon = sfi.h_icon;

    let rgba_pixels = unsafe {
        let mut icon_info = IconInfo {
            f_icon: 0,
            x_hotspot: 0,
            y_hotspot: 0,
            h_bm_mask: std::ptr::null_mut(),
            h_bm_color: std::ptr::null_mut(),
        };

        if GetIconInfo(h_icon, &mut icon_info) == 0 {
            DestroyIcon(h_icon);
            return None;
        }

        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            DestroyIcon(h_icon);
            DeleteObject(icon_info.h_bm_mask);
            if !icon_info.h_bm_color.is_null() {
                DeleteObject(icon_info.h_bm_color);
            }
            return None;
        }

        let mut bmi = BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: SIZE as i32,
            bi_height: -(SIZE as i32), // negative = top-down scan order
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0, // BI_RGB
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };

        let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
        GetDIBits(
            dc,
            icon_info.h_bm_color,
            0,
            SIZE,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        DeleteDC(dc);
        DeleteObject(icon_info.h_bm_mask);
        if !icon_info.h_bm_color.is_null() {
            DeleteObject(icon_info.h_bm_color);
        }

        // GDI returns BGRA; convert to RGBA for the image crate
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        pixels
    };

    unsafe { DestroyIcon(h_icon); }

    // Encode RGBA pixels as PNG
    let img = image::RgbaImage::from_raw(SIZE, SIZE, rgba_pixels)?;
    let mut png_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .ok()?;

    // Base64 encode (inline — no extra crate needed)
    Some(base64_encode(&png_bytes))
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
```

- [ ] **Step 3: Register the module and command in lib.rs**

At the top of `src-tauri/src/lib.rs`, add after the existing `mod` declarations:

```rust
mod icons;
```

Add a new command wrapper just before the `generate_handler![]` macro area (e.g. after `get_browser_bookmarks`):

```rust
#[tauri::command]
fn get_file_icon(path: String) -> Option<String> {
    icons::get_file_icon(path)
}
```

Add `get_file_icon` to the `generate_handler![]` list:

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_group,
    delete_group,
    launch_group,
    set_preferred_browser,
    activate_license,
    deactivate_license,
    check_license_status,
    reorder_items,
    save_widget_position,
    save_widget_color,
    set_launch_on_startup,
    show_widget_context_menu,
    export_config,
    import_config,
    set_hotkey,
    get_monitors,
    get_window_frame_rect,
    resize_widget,
    get_installed_apps,
    show_group_context_menu,
    get_installed_browsers,
    get_browser_bookmarks,
    send_feedback,
    open_url,
    download_and_install_update,
    get_file_icon,         // ← new
])
```

- [ ] **Step 4: Build to verify it compiles**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo build 2>&1 | Select-String "error\["
```

Expected: no errors.

- [ ] **Step 5: Smoke test icon extraction**

Run the app (`npm run tauri dev`) and open the browser console. In the config window, run:

```js
await window.__TAURI__.core.invoke('get_file_icon', { path: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe' })
```

Expected: a long base64 string (PNG data), or `null` if Chrome isn't installed (try another browser path).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/src/icons.rs src-tauri/src/lib.rs
git commit -m "feat: add get_file_icon command for browser icon extraction"
```

---

## Task 4: Update Launcher for Multi-URL and run_in_terminal

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block in `src-tauri/src/launcher.rs`:

```rust
#[test]
fn test_collect_browser_urls_uses_urls_field_when_populated() {
    let items = vec![
        Item {
            item_type: ItemType::Url,
            path: Some("chrome.exe".into()),
            value: Some("https://old.com".into()),
            urls: vec!["https://a.com".into(), "https://b.com".into()],
            icon_data: None, browser_name: None, run_in_terminal: true,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        },
    ];
    let (map, fallback) = collect_browser_urls(&items, None);
    assert_eq!(map["chrome.exe"], vec!["https://a.com", "https://b.com"]);
    assert!(fallback.is_empty());
}

#[test]
fn test_collect_browser_urls_falls_back_to_value_when_urls_empty() {
    let items = vec![
        Item {
            item_type: ItemType::Url,
            path: Some("firefox.exe".into()),
            value: Some("https://fallback.com".into()),
            urls: vec![],
            icon_data: None, browser_name: None, run_in_terminal: true,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        },
    ];
    let (map, fallback) = collect_browser_urls(&items, None);
    assert_eq!(map["firefox.exe"], vec!["https://fallback.com"]);
    assert!(fallback.is_empty());
}

#[test]
fn test_launch_item_script_missing_path_returns_error_regardless_of_run_flag() {
    let item = Item {
        item_type: ItemType::Script,
        path: None, value: None,
        urls: vec![], icon_data: None, browser_name: None,
        run_in_terminal: false,
        launch_desktop: None, launch_x: None, launch_y: None,
        launch_width: None, launch_height: None,
    };
    let result = launch_item(&item, &None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing a path"));
}
```

- [ ] **Step 2: Run tests — expect failures**

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "FAILED|error\["
```

Expected: the two new `collect_browser_urls` tests fail because the function doesn't use `urls` yet.

- [ ] **Step 3: Update collect_browser_urls to use urls field**

In `src-tauri/src/launcher.rs`, replace the `collect_browser_urls` function body:

```rust
fn collect_browser_urls(
    items: &[Item],
    preferred_browser: Option<&str>,
) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut browser_urls: HashMap<String, Vec<String>> = HashMap::new();
    let mut fallback_urls: Vec<String> = Vec::new();

    for item in items {
        if let ItemType::Url = &item.item_type {
            if item.launch_x.is_some() { continue; }

            // Use urls vec if populated; fall back to value for old single-URL items
            let url_list: Vec<String> = if !item.urls.is_empty() {
                item.urls.clone()
            } else if let Some(v) = &item.value {
                vec![v.clone()]
            } else {
                continue;
            };

            let browser = item.path.as_deref().or(preferred_browser);
            match browser {
                Some(b) => browser_urls.entry(b.to_string()).or_default().extend(url_list),
                None => fallback_urls.extend(url_list),
            }
        }
    }
    (browser_urls, fallback_urls)
}
```

- [ ] **Step 4: Update launch_item Url branch to use urls field**

In `src-tauri/src/launcher.rs`, inside `launch_item`, replace the opening line of the `ItemType::Url` arm:

```rust
ItemType::Url => {
    // Use first URL from urls vec; fall back to value for old single-URL items
    let url: &str = if !item.urls.is_empty() {
        &item.urls[0]
    } else {
        item.value.as_deref().ok_or("URL item is missing a value")?
    };
    let browser = item.path.as_deref().or(preferred_browser.as_deref());
    // ... rest of the existing Url arm is unchanged, just replace `url` references
    // (the variable is now `url: &str` instead of `let url = item.value.as_ref()...`)
```

Specifically, replace this existing block at the top of the `ItemType::Url =>` arm:

```rust
        ItemType::Url => {
            let url = item.value.as_ref().ok_or("URL item is missing a value")?;
            let browser = item.path.as_deref().or(preferred_browser.as_deref());
```

With:

```rust
        ItemType::Url => {
            let url_owned: String;
            let url: &str = if !item.urls.is_empty() {
                &item.urls[0]
            } else {
                url_owned = item.value.clone().ok_or("URL item is missing a value")?;
                &url_owned
            };
            let browser = item.path.as_deref().or(preferred_browser.as_deref());
```

- [ ] **Step 5: Add run_in_terminal branch to Script arm**

In `src-tauri/src/launcher.rs`, inside `launch_item`, replace the start of the `ItemType::Script =>` arm:

```rust
        ItemType::Script => {
            let path = item.path.as_ref().ok_or("Script item is missing a path")?;

            // If run_in_terminal is false, just open the file in its default app
            if !item.run_in_terminal {
                #[cfg(target_os = "windows")]
                let before = if item.launch_x.is_some() { Some(collect_visible_hwnds()) } else { None };
                open::that(path).map_err(|e| format!("Failed to open script '{}': {}", path, e))?;
                #[cfg(target_os = "windows")]
                if let (Some(before), Some(x), Some(y)) = (before, item.launch_x, item.launch_y) {
                    position_window_by_snapshot(before, None, None, x, y, item.launch_width, item.launch_height);
                }
                return Ok(());
            }

            // run_in_terminal = true: execute via cmd/powershell (existing behavior below)
```

The rest of the Script arm (cmd/powershell spawn logic) is unchanged.

- [ ] **Step 6: Run all tests — expect pass**

```powershell
cargo test 2>&1 | Select-String "test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/launcher.rs
git commit -m "feat: multi-URL launch from urls field, script run_in_terminal toggle"
```

---

## Task 5: Script Run Toggle UI in config.js

**Files:**
- Modify: `src/config.js`

- [ ] **Step 1: Add run_in_terminal default when adding a script item**

In `src/config.js`, find the `addItem` function. Find this line:

```js
  currentItems.push({ item_type: type, path: selected, value: null });
```

Replace it with:

```js
  const newItem = { item_type: type, path: selected, value: null };
  if (type === 'script') newItem.run_in_terminal = true;
  currentItems.push(newItem);
```

- [ ] **Step 2: Add checkbox to buildExpandPanel for script items**

In `src/config.js`, find the `buildExpandPanel` function. After the existing event listener for `.pick-btn` (the last line before `return panel;`), add:

```js
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
```

- [ ] **Step 3: Verify in the app**

Run `npm run tauri dev`. Open a group with a script item (or add one). In the expand panel, confirm:
- A "▶ Run via cmd" checkbox appears below the position picker
- It is checked by default for new script items
- Toggling it and saving the group persists the value (check config.json in `%LOCALAPPDATA%\AppLauncher\`)

- [ ] **Step 4: Commit**

```powershell
git add src/config.js
git commit -m "feat: add run_in_terminal checkbox to script items in config UI"
```

---

## Task 6: URL Item UI Redesign in config.js

**Files:**
- Modify: `src/config.js`

This is the largest task. Work through it step by step.

- [ ] **Step 1: Add two helper functions near the top of config.js (after the EMOJIS constant)**

```js
function urlHostname(url) {
  try { return new URL(url).hostname.replace(/^www\./, ''); }
  catch { return url; }
}

function browserDisplayName(item) {
  if (item.browser_name) return item.browser_name;
  if (!item.path) return 'Browser';
  const NAMES = {
    'chrome.exe': 'Chrome', 'msedge.exe': 'Edge', 'brave.exe': 'Brave',
    'firefox.exe': 'Firefox', 'opera.exe': 'Opera', 'operagx.exe': 'Opera GX',
    'vivaldi.exe': 'Vivaldi', 'arc.exe': 'Arc', 'thorium.exe': 'Thorium',
  };
  const exe = item.path.replace(/.*[/\\]/, '').toLowerCase();
  return NAMES[exe] || exe.replace(/\.exe$/i, '');
}
```

- [ ] **Step 2: Update showUrlPicker to accept an optional edit context**

Replace the existing `showUrlPicker` function with:

```js
async function showUrlPicker(editContext = null) {
  // editContext = { item: currentItems[idx], idx } when editing an existing URL item

  // In edit mode: skip browser selection, jump straight to bookmark step
  if (editContext) {
    const { item, idx } = editContext;
    const modal = document.createElement('div');
    modal.className = 'winapp-modal';
    modal.innerHTML = `<div class="winapp-card"></div>`;
    document.body.appendChild(modal);
    const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
    const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
    modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
    document.addEventListener('keydown', onKeyDown);
    const browser = { name: browserDisplayName(item), path: item.path || '' };
    await showBookmarkStep(modal, browser, closeModal, item, idx);
    return;
  }

  // Normal mode: show browser list first
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
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  document.getElementById('url-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let browsers;
  try {
    browsers = await invoke('get_installed_browsers');
  } catch (e) {
    console.error('get_installed_browsers failed:', e);
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
    row.addEventListener('click', () => showBookmarkStep(modal, browser, closeModal, null, null));
    browserList.appendChild(row);
  });
}
```

- [ ] **Step 3: Replace showBookmarkStep to create a single multi-URL item**

Replace the existing `showBookmarkStep` function with:

```js
async function showBookmarkStep(modal, browser, closeModal, existingItem = null, existingIdx = null) {
  const isEdit = existingItem !== null && existingIdx !== null;
  const existingUrls = isEdit
    ? (existingItem.urls?.length > 0 ? existingItem.urls : (existingItem.value ? [existingItem.value] : []))
    : [];

  const card = modal.querySelector('.winapp-card');
  const safeBrowserName = browser.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  card.innerHTML = `
    <div class="winapp-header">
      ${isEdit ? '' : '<button class="url-back-btn" id="url-back">←</button>'}
      <span class="url-step-title">${safeBrowserName} Bookmarks</span>
      <button class="winapp-close" id="url-close2">✕</button>
    </div>
    <div class="url-custom">
      <input type="text" id="bookmark-search" placeholder="Search bookmarks..." autocomplete="off" />
    </div>
    <div class="winapp-list" id="bookmark-list">
      <div class="winapp-empty">Loading bookmarks...</div>
    </div>
    <div class="url-entry">
      <input type="text" id="custom-url-input" placeholder="Or enter a custom URL: https://..." autocomplete="off" />
    </div>
    <div class="url-footer">
      <button class="btn btn-save" id="add-selected-btn" disabled>${isEdit ? 'Save' : 'Add Selected'}</button>
    </div>
  `;

  if (!isEdit) {
    document.getElementById('url-back').addEventListener('click', () => {
      closeModal();
      showUrlPicker();
    });
  }
  document.getElementById('url-close2').addEventListener('click', closeModal);

  const customInput = document.getElementById('custom-url-input');
  const addBtn = document.getElementById('add-selected-btn');

  function updateAddBtn() {
    const checkedCount = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none').length;
    const hasCustom = customInput.value.trim().length > 0;
    const total = checkedCount + (hasCustom ? 1 : 0);
    if (isEdit) {
      addBtn.disabled = false; // always enabled in edit mode
      addBtn.textContent = total > 0 ? `Save (${total} URL${total === 1 ? '' : 's'})` : 'Save';
    } else {
      addBtn.disabled = total === 0;
      addBtn.textContent = total > 0 ? `Add ${total} Selected` : 'Add Selected';
    }
  }

  let bookmarks;
  try {
    bookmarks = await invoke('get_browser_bookmarks', { browserPath: browser.path });
  } catch (e) {
    console.error('get_browser_bookmarks failed:', e);
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
      const safeTitle = bm.title.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeUrl   = bm.url.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      label.innerHTML = `
        <input type="checkbox" class="bookmark-checkbox" />
        <div class="bookmark-info">
          <div class="bookmark-title">${safeTitle}</div>
          <div class="bookmark-url">${safeUrl}</div>
        </div>
      `;
      const cb = label.querySelector('.bookmark-checkbox');
      cb.dataset.url = bm.url;
      // In edit mode, pre-check bookmarks that are already in this item
      if (isEdit && existingUrls.includes(bm.url)) cb.checked = true;
      cb.addEventListener('change', updateAddBtn);
      list.appendChild(label);
    });
  }

  document.getElementById('bookmark-search').addEventListener('input', (e) => {
    const q = e.target.value.trim().toLowerCase();
    modal.querySelectorAll('.bookmark-row').forEach(row => {
      const title = row.querySelector('.bookmark-title')?.textContent.toLowerCase() || '';
      const url   = row.querySelector('.bookmark-url')?.textContent.toLowerCase() || '';
      row.style.display = (!q || title.includes(q) || url.includes(q)) ? '' : 'none';
    });
    updateAddBtn();
  });

  customInput.addEventListener('input', updateAddBtn);

  addBtn.addEventListener('click', async () => {
    const checked = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none');
    const urls = checked.map(cb => cb.dataset.url);
    const customUrl = customInput.value.trim();
    if (customUrl) urls.push(customUrl);

    if (!isEdit && urls.length === 0) return;

    // Fetch browser icon (non-blocking — null on failure)
    let icon_data = null;
    try { icon_data = await invoke('get_file_icon', { path: browser.path }); } catch {}

    const newItem = {
      item_type: 'url',
      path: browser.path,
      browser_name: browser.name,
      urls,
      value: urls[0] || null, // backward compat field
      icon_data,
      launch_desktop: null,
      launch_x: null,
      launch_y: null,
      launch_width: null,
      launch_height: null,
    };

    if (isEdit) {
      // Preserve launch position from existing item
      newItem.launch_desktop = existingItem.launch_desktop ?? null;
      newItem.launch_x       = existingItem.launch_x ?? null;
      newItem.launch_y       = existingItem.launch_y ?? null;
      newItem.launch_width   = existingItem.launch_width ?? null;
      newItem.launch_height  = existingItem.launch_height ?? null;
      currentItems[existingIdx] = newItem;
    } else {
      currentItems.push(newItem);
    }

    renderItems();
    closeModal();
  });

  customInput.focus();
  updateAddBtn();
}
```

- [ ] **Step 4: Update renderItems to display URL items with icon, label, subtitle, and Edit button**

In `src/config.js`, replace the `renderItems` function with:

```js
function renderItems() {
  const list = document.getElementById('items-list');
  list.innerHTML = '';

  currentItems.forEach((item, idx) => {
    const wrapper = document.createElement('div');

    const row = document.createElement('div');
    row.className = 'item-row';

    if (item.item_type === 'url') {
      // URL item: icon + browser label + subtitle + edit button + remove button
      const allUrls = (item.urls && item.urls.length > 0) ? item.urls : (item.value ? [item.value] : []);
      const count = allUrls.length;
      const name = browserDisplayName(item);
      const label = `${name} (${count} URL${count === 1 ? '' : 's'})`;
      const hostnames = allUrls.slice(0, 2).map(urlHostname);
      const subtitle = hostnames.join(', ') + (allUrls.length > 2 ? ` +${allUrls.length - 2}` : '');

      const iconHtml = item.icon_data
        ? `<img src="data:image/png;base64,${item.icon_data}" style="width:16px;height:16px;object-fit:contain;vertical-align:middle;" alt="" />`
        : '<span>🌐</span>';

      const safeLabel    = label.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeSubtitle = subtitle.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

      row.innerHTML = `
        ${iconHtml}
        <div class="item-label-multi" title="${safeSubtitle}" style="flex:1;min-width:0;overflow:hidden;">
          <div class="item-label">${safeLabel}</div>
          <div style="font-size:10px;color:#888;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${safeSubtitle}</div>
        </div>
        <button class="edit-url-btn" style="font-size:11px;padding:2px 6px;margin-right:4px;">✏</button>
        <button class="remove-btn">✕</button>
      `;

      row.querySelector('.edit-url-btn').onclick = () => showUrlPicker({ item, idx });
      row.querySelector('.remove-btn').onclick = () => { currentItems.splice(idx, 1); renderItems(); };

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

    row.setAttribute('draggable', 'true');
    row.dataset.index = idx;
    row.addEventListener('dragstart', e => e.dataTransfer.setData('text/plain', idx));
    row.addEventListener('dragover', e => { e.preventDefault(); row.style.opacity = '0.5'; });
    row.addEventListener('dragleave', () => { row.style.opacity = '1'; });
    row.addEventListener('drop', e => {
      e.preventDefault();
      row.style.opacity = '1';
      const fromIdx = parseInt(e.dataTransfer.getData('text/plain'));
      if (fromIdx !== idx) {
        const [moved] = currentItems.splice(fromIdx, 1);
        currentItems.splice(idx, 0, moved);
        renderItems();
      }
    });

    wrapper.appendChild(row);
    wrapper.appendChild(buildExpandPanel(item, idx));
    list.appendChild(wrapper);
  });

  fitWindow();
}
```

- [ ] **Step 5: Verify in the app**

Run `npm run tauri dev`. Open a group editor. Test:

1. **Add a new URL item** — click Add → URL/Bookmark, pick a browser, select 2+ bookmarks + 1 custom URL, click Add. Verify: one item appears in the list showing `Chrome (3 URLs)` with a hostname subtitle and browser icon (or 🌐 fallback). The position picker expand panel is present.

2. **Edit a URL item** — click the ✏ button on a URL item. Verify: the picker opens directly on the bookmark step for that browser, existing bookmark URLs are pre-checked.

3. **Script items** — existing scripts and URL items still work (drag-to-reorder, remove, position picker).

- [ ] **Step 6: Commit**

```powershell
git add src/config.js
git commit -m "feat: URL items support multiple URLs with browser icons and edit button"
```

---

## Self-Review Checklist

After all tasks complete, run:

```powershell
cd "C:\Users\dougb\Desktop\AppLauncher\src-tauri"
cargo test 2>&1 | Select-String "test result|FAILED"
```

Expected: `test result: ok.`

Then test the full app flow manually:
- [ ] Install over a running AppLauncher — should not get stuck (NSIS test, requires building installer)
- [ ] URL item with 3 URLs launches all 3 in one browser call
- [ ] Script item with "▶ Run via cmd" unchecked → opens file in default app (e.g. Notepad for .bat); checked → runs via cmd.exe
- [ ] Old config.json items (no `urls` field) still work: `value` field used as fallback
