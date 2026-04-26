# Design: Three Fixes — Icon Picker, Widget Drag, Windows Apps Picker

**Date:** 2026-04-26  
**Status:** Approved

---

## Fix 1 — Emoji Icon Picker

### Problem
`icon-input` in `config.html` is a plain `<input type="text">`. The title says "Click to change emoji" but clicking it does nothing special — no picker appears.

### Design
- Clicking the icon input opens a floating `<div>` emoji grid positioned below the input.
- The grid contains ~45 curated emojis covering common use cases (productivity, entertainment, dev, etc.).
- Clicking an emoji writes it to `icon-input` and closes the grid.
- Clicking anywhere outside the grid also closes it.
- The input remains editable as a fallback (user can still type/paste any emoji directly).
- No new dependencies — pure HTML/CSS/JS in `config.js` and `config.html`.

### Curated emoji set (example)
💼 📁 🗂️ 🖥️ 🌐 📧 📅 📝 🔧 ⚙️ 🚀 🎮 🎵 🎬 📷 💰 🏠 🏢 📚 🔬 🧪 🛒 🤝 📊 📈 ⚡ 🔒 🛡️ 🗑️ 📌 🔗 💡 🎯 🧩 🐍 🦀 🌙 ☀️ 🔔 📣 🗺️ 🎨 🖊️ 📦 🧰

---

## Fix 2 — Widget Drag

### Problem
`widget.js` drag check: `if (e.target === widget)` — this only triggers when the mousedown lands exactly on the widget's background div. Any click on a child element (button span, icon, button background) sets `e.target` to that child, so drag never starts in practice.

### Design
Change the condition to:

```js
if (!e.target.closest('.group-btn')) {
  getCurrentWindow().startDragging();
}
```

This starts dragging on any mousedown that is NOT on a `.group-btn` (which covers both group buttons and the add button, since add-btn also has class `group-btn`). One-line change in `widget.js`.

---

## Fix 3 — Windows Apps Picker

### Problem
Adding an "App / Executable" item requires the user to navigate through File Explorer to find the `.exe`. This is unfriendly — users often don't know where apps are installed.

### Design

#### Rust: `get_installed_apps` command
- Scans two Start Menu shortcut directories:
  - `%APPDATA%\Microsoft\Windows\Start Menu\Programs\**\*.lnk` (user-installed)
  - `C:\ProgramData\Microsoft\Windows\Start Menu\Programs\**\*.lnk` (system-wide)
- For each `.lnk` file:
  - Resolves the target path using `IShellLink` + `IPersistFile` via the Windows COM API
  - Skips entries whose target is not an `.exe` file
  - Uses the `.lnk` filename (without extension) as the display name
- Deduplicates by resolved exe path (same exe may appear in multiple shortcut folders)
- Returns `Vec<InstalledApp>` sorted alphabetically by name
- New struct: `pub struct InstalledApp { pub name: String, pub path: String }`

**Cargo.toml additions** (windows crate feature flags):
```
"Win32_UI_Shell",
"Win32_System_Com",
"Win32_System_Com_StructuredStorage",
```

**New file:** `src-tauri/src/apps.rs` — keeps the COM/lnk logic isolated from `lib.rs`.

#### Frontend: Windows Apps menu item + search modal
- Add `🪟 Windows Apps` entry to `add-type-menu` in `config.html` with `data-type="winapp"`.
- In `config.js`, handle `type === 'winapp'` in `addItem()`:
  1. Call `invoke('get_installed_apps')` — show a loading state.
  2. Render a modal overlay (`<div id="winapp-modal">`) over the config window containing:
     - A text `<input>` for filtering (filters by display name, case-insensitive)
     - A scrollable `<div>` list of matching apps — each row shows the app name
     - Clicking a row adds `{ item_type: 'app', path: <exe_path>, value: null }` to `currentItems`, closes the modal, re-renders items
     - An `✕` button or clicking outside the modal closes it without adding anything
  3. The modal is styled consistent with the existing dark theme.

---

## Files Changed

| File | Change |
|------|--------|
| `src/config.html` | Add emoji grid div; add `🪟 Windows Apps` to add-type-menu |
| `src/config.js` | Emoji picker open/close logic; `winapp` handler; modal render |
| `src/styles.css` | Styles for emoji grid and winapp modal |
| `src/widget.js` | One-line drag fix |
| `src-tauri/src/apps.rs` | New file — `get_installed_apps` via IShellLink |
| `src-tauri/src/lib.rs` | `mod apps`; register `get_installed_apps` command |
| `src-tauri/Cargo.toml` | Add Win32_UI_Shell, Win32_System_Com features |
| `src-tauri/capabilities/default.json` | No changes expected |

---

## Out of Scope
- Showing app icons in the winapp picker (icon extraction from exe requires additional COM work)
- Persisting the last-used filter in the winapp modal
- Keyboard navigation in the winapp modal
