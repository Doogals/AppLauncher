# Three Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the emoji icon picker, widget drag, and add a Windows Apps item picker backed by Start Menu .lnk resolution.

**Architecture:** Three independent changes — a one-line drag fix in widget.js, a JS-only emoji popup grid in config.js/html/css, and a new Rust command (`get_installed_apps`) that walks Start Menu shortcut folders and resolves each .lnk via IShellLink COM, paired with a searchable modal in config.js.

**Tech Stack:** Tauri v2, Rust (windows crate 0.58 — IShellLink COM), Vanilla JS/HTML/CSS

---

## File Map

| File | Change |
|------|--------|
| `src/widget.js` | One-line drag condition fix |
| `src/config.html` | Add `#emoji-grid` div; add `🪟 Windows Apps` menu item |
| `src/config.js` | Emoji picker open/close; `winapp` handler; `showWinAppPicker()` modal |
| `src/styles.css` | `.emoji-grid`, `.emoji-btn`, `.winapp-modal`, `.winapp-card`, `.winapp-row` |
| `src-tauri/Cargo.toml` | Add `Win32_UI_Shell`, `Win32_System_Com`, `Win32_Storage_FileSystem` features |
| `src-tauri/src/apps.rs` | New file — `InstalledApp` struct, `get_installed_apps()`, `collect_lnk_files()`, `resolve_lnk()` |
| `src-tauri/src/lib.rs` | `mod apps;` + register `get_installed_apps` command |

---

## Task 1: Fix Widget Drag

**Files:**
- Modify: `src/widget.js:9-12`

- [ ] **Step 1: Apply the fix**

In `src/widget.js`, the drag handler at lines 9–12 currently reads:

```js
widget.addEventListener('mousedown', (e) => {
  if (e.target === widget) {
    getCurrentWindow().startDragging();
  }
});
```

Replace with:

```js
widget.addEventListener('mousedown', (e) => {
  if (!e.target.closest('.group-btn')) {
    getCurrentWindow().startDragging();
  }
});
```

`e.target === widget` only fires when the click lands on the exact background pixel — any child element (button, span) breaks it. `closest('.group-btn')` checks whether the click was on a button or inside one; if not, we drag.

- [ ] **Step 2: Verify manually**

Run `npm run tauri dev` from `AppLauncher`. Click and drag on the empty space between or around the buttons — the window should move. Clicking buttons should still launch groups normally.

- [ ] **Step 3: Commit**

```bash
git add src/widget.js
git commit -m "fix: widget drag now works by checking closest .group-btn instead of exact target"
```

---

## Task 2: Emoji Picker Grid

**Files:**
- Modify: `src/config.html`
- Modify: `src/styles.css`
- Modify: `src/config.js`

- [ ] **Step 1: Add the emoji grid div to config.html**

In `src/config.html`, after the closing `</div>` of `.config-header` (line 14), add:

```html
    <div id="emoji-grid" class="emoji-grid" style="display:none"></div>
```

Full updated header section:

```html
  <div class="config-window">
    <div class="config-header">
      <input type="text" class="icon-picker" id="icon-input" value="📁" maxlength="2" title="Click to change emoji" />
      <input type="text" class="name-input" id="name-input" placeholder="Group name..." />
    </div>
    <div id="emoji-grid" class="emoji-grid" style="display:none"></div>
```

- [ ] **Step 2: Add emoji grid and winapp styles to styles.css**

Append to the bottom of `src/styles.css`:

```css
/* Emoji picker */
.emoji-grid {
  position: fixed;
  display: grid;
  grid-template-columns: repeat(9, 1fr);
  gap: 4px;
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 8px;
  padding: 8px;
  z-index: 100;
  box-shadow: 0 4px 16px rgba(0,0,0,0.5);
}

.emoji-btn {
  background: none;
  border: none;
  font-size: 1.2rem;
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  line-height: 1;
}

.emoji-btn:hover { background: #0f3460; }

/* Windows Apps picker modal */
.winapp-modal {
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}

.winapp-card {
  background: #1a1a2e;
  border: 1px solid #0f3460;
  border-radius: 10px;
  width: 360px;
  max-height: 380px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.winapp-header {
  display: flex;
  gap: 8px;
  padding: 12px;
  border-bottom: 1px solid #0f3460;
  flex-shrink: 0;
}

.winapp-header input {
  flex: 1;
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 6px;
  padding: 8px 12px;
  color: #e0e0e0;
  font-size: 0.9rem;
  outline: none;
}

.winapp-header input:focus { border-color: #e94560; }

.winapp-close {
  background: none;
  border: none;
  color: #888;
  cursor: pointer;
  font-size: 1rem;
  padding: 4px 8px;
  border-radius: 4px;
}

.winapp-close:hover { color: #e94560; }

.winapp-list {
  overflow-y: auto;
  flex: 1;
  padding: 4px 0;
}

.winapp-row {
  padding: 8px 16px;
  cursor: pointer;
  font-size: 0.85rem;
  color: #e0e0e0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.winapp-row:hover { background: #0f3460; }

.winapp-empty {
  padding: 20px;
  color: #888;
  text-align: center;
  font-size: 0.85rem;
}
```

- [ ] **Step 3: Add emoji picker logic to config.js**

At the **top** of `src/config.js`, after the existing imports and before the `const params` line, add the emoji constants and builder function:

```js
const EMOJIS = [
  '💼','📁','🗂️','🖥️','🌐','📧','📅','📝','🔧','⚙️',
  '🚀','🎮','🎵','🎬','📷','💰','🏠','🏢','📚','🔬',
  '🧪','🛒','🤝','📊','📈','⚡','🔒','🛡️','📌','🔗',
  '💡','🎯','🧩','🐍','🦀','🌙','☀️','🔔','📣','🗺️',
  '🎨','🖊️','📦','🧰','🖱️',
];

function buildEmojiGrid() {
  const grid = document.getElementById('emoji-grid');
  EMOJIS.forEach(emoji => {
    const btn = document.createElement('button');
    btn.className = 'emoji-btn';
    btn.textContent = emoji;
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      document.getElementById('icon-input').value = emoji;
      grid.style.display = 'none';
    });
    grid.appendChild(btn);
  });
}
```

Then, after the `buildEmojiGrid` function definition, add the click handlers (before `init()`):

```js
document.getElementById('icon-input').addEventListener('click', (e) => {
  e.stopPropagation();
  const grid = document.getElementById('emoji-grid');
  const rect = e.target.getBoundingClientRect();
  grid.style.top = (rect.bottom + 4) + 'px';
  grid.style.left = rect.left + 'px';
  grid.style.display = grid.style.display === 'none' ? 'grid' : 'none';
});

document.addEventListener('click', () => {
  document.getElementById('emoji-grid').style.display = 'none';
});

buildEmojiGrid();
```

- [ ] **Step 4: Verify manually**

Run `npm run tauri dev`. Open config window (click +). Click the emoji icon (📁) in the top-left — a grid of ~45 emojis should appear. Click one — it should update the icon and close the grid. Click elsewhere — grid should close. The name input and save/cancel should still work.

- [ ] **Step 5: Commit**

```bash
git add src/config.html src/config.js src/styles.css
git commit -m "feat: add emoji picker grid to config window icon input"
```

---

## Task 3: Update Cargo.toml Windows Features

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add required feature flags**

In `src-tauri/Cargo.toml`, update the `[target.'cfg(windows)'.dependencies]` block from:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Registry",
] }
```

to:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Registry",
  "Win32_UI_Shell",
  "Win32_System_Com",
  "Win32_Storage_FileSystem",
] }
```

`Win32_UI_Shell` provides `IShellLinkW` and `ShellLink`. `Win32_System_Com` provides `CoCreateInstance`, `CoInitializeEx`, `IPersistFile`, `STGM`. `Win32_Storage_FileSystem` provides `WIN32_FIND_DATAW` (needed as the `pfd` parameter in `GetPath`).

- [ ] **Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors (warnings about unused features are fine).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore: add Win32_UI_Shell, Win32_System_Com, Win32_Storage_FileSystem crate features"
```

---

## Task 4: Implement get_installed_apps Rust Command

**Files:**
- Create: `src-tauri/src/apps.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the test for collect_lnk_files**

Create `src-tauri/src/apps.rs` with the test first:

```rust
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub path: String,
}

pub fn get_installed_apps() -> Vec<InstalledApp> {
    todo!()
}

fn collect_lnk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    todo!()
}

fn resolve_lnk(_lnk_path: &Path) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn collect_lnk_files_finds_lnk_and_ignores_others() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.lnk"), b"").unwrap();
        fs::write(dir.join("readme.txt"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "app.lnk");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_lnk_files_recurses_into_subdirs() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_recurse_test");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.lnk"), b"").unwrap();
        fs::write(sub.join("b.lnk"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);
        assert_eq!(found.len(), 2);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn get_installed_apps_returns_vec_without_panic() {
        // Integration test — just verify it doesn't panic on this machine
        let apps = get_installed_apps();
        // There should be at least a few apps on any Windows machine
        assert!(!apps.is_empty(), "Expected at least one installed app");
        // Verify all paths end in .exe
        for app in &apps {
            assert!(
                app.path.to_ascii_lowercase().ends_with(".exe"),
                "Non-exe path: {}",
                app.path
            );
        }
    }
}
```

- [ ] **Step 2: Run tests — verify they fail**

```bash
cd src-tauri && cargo test apps
```

Expected: compile errors from `todo!()` macros (or test panics). That's fine — confirm tests exist and the module structure is valid.

- [ ] **Step 3: Implement collect_lnk_files and resolve_lnk**

Replace the `todo!()` stubs in `src-tauri/src/apps.rs` with the real implementations:

```rust
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub path: String,
}

pub fn get_installed_apps() -> Vec<InstalledApp> {
    let should_uninit = unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok()
    };

    let mut lnk_files = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        collect_lnk_files(&path, &mut lnk_files);
    }

    let system = Path::new(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs");
    collect_lnk_files(system, &mut lnk_files);

    let mut seen: HashSet<String> = HashSet::new();
    let mut apps: Vec<InstalledApp> = lnk_files
        .iter()
        .filter_map(|lnk| {
            let name = lnk.file_stem()?.to_string_lossy().into_owned();
            let target = resolve_lnk(lnk)?;
            if seen.insert(target.to_ascii_lowercase()) {
                Some(InstalledApp { name, path: target })
            } else {
                None
            }
        })
        .collect();

    apps.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    if should_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }

    apps
}

fn collect_lnk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
            out.push(path);
        }
    }
}

fn resolve_lnk(lnk_path: &Path) -> Option<String> {
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::Storage::FileSystem::WIN32_FIND_DATAW,
        Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, IPersistFile, STGM},
        Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_FLAGS},
    };

    unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let wide: Vec<u16> = lnk_path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        persist_file.Load(PCWSTR(wide.as_ptr()), STGM(0)).ok()?;

        let mut buf = [0u16; 260];
        let mut find_data = WIN32_FIND_DATAW::default();
        shell_link
            .GetPath(PWSTR(buf.as_mut_ptr()), 260, &mut find_data, SLGP_FLAGS(0))
            .ok()?;

        let end = buf.iter().position(|&c| c == 0).unwrap_or(260);
        let target = String::from_utf16_lossy(&buf[..end]);

        if target.is_empty() || !target.to_ascii_lowercase().ends_with(".exe") {
            return None;
        }
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn collect_lnk_files_finds_lnk_and_ignores_others() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.lnk"), b"").unwrap();
        fs::write(dir.join("readme.txt"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "app.lnk");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_lnk_files_recurses_into_subdirs() {
        let dir = std::env::temp_dir().join("app_launcher_lnk_recurse_test");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.lnk"), b"").unwrap();
        fs::write(sub.join("b.lnk"), b"").unwrap();

        let mut found = Vec::new();
        collect_lnk_files(&dir, &mut found);
        assert_eq!(found.len(), 2);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn get_installed_apps_returns_vec_without_panic() {
        let apps = get_installed_apps();
        assert!(!apps.is_empty(), "Expected at least one installed app");
        for app in &apps {
            assert!(
                app.path.to_ascii_lowercase().ends_with(".exe"),
                "Non-exe path: {}",
                app.path
            );
        }
    }
}
```

- [ ] **Step 4: Register the module and command in lib.rs**

In `src-tauri/src/lib.rs`, add `mod apps;` after `mod license;` (line 3):

```rust
mod config;
mod launcher;
mod license;
mod apps;
```

Add the import after the existing `use` statements (after line 7):

```rust
use apps::InstalledApp;
```

Add the new command function before `pub fn run()`:

```rust
#[tauri::command]
fn get_installed_apps() -> Vec<InstalledApp> {
    apps::get_installed_apps()
}
```

Add `get_installed_apps` to the `invoke_handler` list (in the `.invoke_handler(tauri::generate_handler![...])` block):

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_group,
    delete_group,
    launch_group,
    set_preferred_browser,
    activate_license,
    reorder_items,
    save_widget_position,
    resize_widget,
    get_installed_apps,
])
```

- [ ] **Step 5: Run tests**

```bash
cd src-tauri && cargo test apps
```

Expected: `collect_lnk_files_finds_lnk_and_ignores_others` — PASS, `collect_lnk_files_recurses_into_subdirs` — PASS, `get_installed_apps_returns_vec_without_panic` — PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/apps.rs src-tauri/src/lib.rs
git commit -m "feat: add get_installed_apps command via IShellLink Start Menu .lnk resolution"
```

---

## Task 5: Add Windows Apps Picker to Config Frontend

**Files:**
- Modify: `src/config.html`
- Modify: `src/config.js`

- [ ] **Step 1: Add Windows Apps to the add-type-menu in config.html**

In `src/config.html`, in the `#add-type-menu` div, add the Windows Apps option after the Script entry (after line 26):

```html
      <div id="add-type-menu" style="display:none; background:#16213e; border:1px solid #0f3460; border-radius:6px; padding:4px 0; margin-bottom:10px;">
        <div class="context-menu-item" data-type="app">🖥️ App / Executable</div>
        <div class="context-menu-item" data-type="winapp">🪟 Windows Apps</div>
        <div class="context-menu-item" data-type="file">📄 File</div>
        <div class="context-menu-item" data-type="url">🌐 URL</div>
        <div class="context-menu-item" data-type="folder">📁 Folder</div>
        <div class="context-menu-item" data-type="script">⚡ Script (.bat / .ps1)</div>
      </div>
```

- [ ] **Step 2: Add showWinAppPicker to config.js**

Add this function to `src/config.js` before `init()`:

```js
async function showWinAppPicker() {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <input type="text" id="winapp-search" placeholder="Search apps..." autocomplete="off" />
        <button class="winapp-close" id="winapp-close">✕</button>
      </div>
      <div class="winapp-list" id="winapp-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const closeModal = () => modal.remove();
  document.getElementById('winapp-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });

  let apps;
  try {
    apps = await invoke('get_installed_apps');
  } catch (e) {
    document.getElementById('winapp-list').innerHTML =
      '<div class="winapp-empty">Failed to load apps.</div>';
    return;
  }

  function renderApps(filter) {
    const list = document.getElementById('winapp-list');
    if (!list) return;
    const filtered = filter
      ? apps.filter(a => a.name.toLowerCase().includes(filter.toLowerCase()))
      : apps;

    if (filtered.length === 0) {
      list.innerHTML = '<div class="winapp-empty">No apps found</div>';
      return;
    }

    list.innerHTML = '';
    filtered.forEach(app => {
      const row = document.createElement('div');
      row.className = 'winapp-row';
      row.textContent = app.name;
      row.addEventListener('click', () => {
        currentItems.push({ item_type: 'app', path: app.path, value: null });
        renderItems();
        closeModal();
      });
      list.appendChild(row);
    });
  }

  renderApps('');
  const searchInput = document.getElementById('winapp-search');
  searchInput.addEventListener('input', (e) => renderApps(e.target.value));
  searchInput.focus();
}
```

- [ ] **Step 3: Wire winapp type in addItem**

In `src/config.js`, replace the entire `addItem` function with this version (adds the `winapp` early-return before the existing url/file branches):

```js
async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';

  if (type === 'winapp') {
    showWinAppPicker();
    return;
  }

  if (type === 'url') {
    const url = window.prompt('Enter URL:');
    if (!url) return;

    const config = await invoke('get_config');
    if (!config.preferred_browser) {
      const browser = await open({
        title: 'Select your preferred browser (.exe)',
        filters: [{ name: 'Executable', extensions: ['exe'] }],
      });
      if (browser) await invoke('set_preferred_browser', { path: browser });
    }
    currentItems.push({ item_type: 'url', path: null, value: url });
  } else {
    const filters = type === 'app' || type === 'script'
      ? [{ name: 'Executable', extensions: ['exe', 'bat', 'ps1', 'cmd'] }]
      : [];
    const selected = await open({
      title: `Select ${type}`,
      directory: type === 'folder',
      filters: filters.length ? filters : undefined,
    });
    if (!selected) return;
    currentItems.push({ item_type: type, path: selected, value: null });
  }

  renderItems();
}
```

- [ ] **Step 4: Verify manually**

Run `npm run tauri dev`. Open config window. Click `+ Add Item` — you should see `🪟 Windows Apps` in the menu. Click it — a modal should appear with a search box and a scrollable list of installed apps (things like Chrome, VS Code, Discord, etc.). Type a few letters to filter. Click an app — it should appear in the items list and the modal should close. Save the group and launch it — the app should open.

- [ ] **Step 5: Commit**

```bash
git add src/config.html src/config.js
git commit -m "feat: add Windows Apps picker with searchable Start Menu app list"
```

---

## Done

All three fixes are in. Run `npm run tauri dev` for a full end-to-end smoke test:
1. Drag the widget to a new position — should move freely
2. Open a group config — click the emoji, pick one, save
3. Add a new item via Windows Apps — pick an app, save, launch
