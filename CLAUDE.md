# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Dev (hot-reload Rust + Vite frontend together)
npm run tauri dev

# Release build (produces MSI + NSIS in src-tauri/target/release/bundle/)
npm run tauri build

# Rust tests only (fast, no Tauri build needed)
cargo test --manifest-path src-tauri/Cargo.toml

# Single test module
cargo test --manifest-path src-tauri/Cargo.toml config::tests
cargo test --manifest-path src-tauri/Cargo.toml launcher::tests

# Deploy (build → sign → GitHub release → update latest.json)
# Use the hub MCP tool: deploy_app(app="app-launcher")
```

**Signing note:** `TAURI_SIGNING_PRIVATE_KEY_PATH` env vars are NOT picked up by `npm run tauri build`. Sign post-build with `npx tauri signer sign`. MSI filename must have no spaces — copy to `app.msi` first. Key at `C:\Users\dougb\.tauri\applauncher.key` (no password).

## Architecture

This is a **Tauri v2** app. Rust handles all system logic; the frontend (Vite/vanilla JS) is purely UI. They communicate exclusively via Tauri's `invoke()` / `emit()` IPC.

### Windows

Three persistent HTML entry points, all under `src/`:
- **`widget.html`** — the always-visible launcher bar. Renders groups as buttons, handles drag, right-click menu, update banner.
- **`config.html`** — group editor. Opens as a separate window via `open_config_window` Rust command. Auto-sizes to content.
- **`layout-item.html`** — one window spawned per item during the layout editor session. User drags it to set launch position; virtual desktop dropdown sets target desktop.

Vite root is `src/`, outDir is `dist/` — this is intentional and must not change.

### Rust modules (`src-tauri/src/`)

- **`lib.rs`** — all Tauri commands, menu handlers, tray, app setup, updater. The `AppState(Mutex<AppConfig>)` is the single source of truth for config, managed by Tauri's state system. `LayoutDesktops(Mutex<HashMap<String, Vec<u8>>>)` is transient state for the layout editor session (maps window label → virtual desktop GUID).
- **`config.rs`** — `AppConfig`, `Group`, `Item` structs (serde serialize/deserialize), `load_config()` / `save_config()`. Config persists at `%LOCALAPPDATA%\AppLauncher\config.json`.
- **`launcher.rs`** — all launch logic. `launch_group` orchestrates virtual-desktop switching then `launch_item` per item. Window positioning uses a snapshot-before/poll-after approach (`collect_visible_hwnds` → spawn → `poll_for_new_window`) because PID matching fails for Store/UWP apps.
- **`virtual_desktop.rs`** — Windows virtual desktop support. `get_virtual_desktops()` reads ordered GUID list from registry. `switch_virtual_desktop(from, to)` simulates Win+Ctrl+Arrow via `SendInput`, polls registry to confirm completion. `get_current_virtual_desktop_guid()` reads `CurrentVirtualDesktop` from registry.
- **`license.rs`** — `group_limit()` enforces free tier (1 group). `is_licensed()` checks both key and instance ID are present.
- **`icons.rs`** — `get_file_icon`: Win32 `SHGetFileInfoW` → BGRA → RGBA → PNG → base64.
- **`steam.rs`** — reads installed Steam games from `steamapps/appmanifest_*.acf`, icons from library cache.
- **`apps.rs`**, **`browsers.rs`** — enumerate installed Win32 apps and browsers.

### Key design patterns

**Context menus must run on the main thread.** Always use `app.run_on_main_thread(...)` and defer event handlers to `tauri::async_runtime::spawn`. Never call menu APIs directly from a command handler.

**Config mutations follow a read-modify-write pattern:**
```rust
let mut config = state.0.lock().unwrap().clone(); // clone out
// mutate config
save_config(&config)?;
*state.0.lock().unwrap() = config; // write back
```

**Virtual desktop switching — critical gotchas:**
- `IVirtualDesktopManager::MoveWindowToDesktop` silently fails cross-process — do not use.
- `SendInput` must send each key event **individually** with 15ms delays between them. Batching all 6 events in one `SendInput` call causes Windows to open the Start menu instead of switching desktops.
- Read the current desktop GUID **once** at the start of `launch_group` and track it manually. Re-reading the registry mid-sequence returns stale data after a switch.
- After each keypress, poll the registry (every 50ms, up to 600ms) to confirm the switch completed before launching.

**Window positioning** uses physical pixels (from `GetWindowRect`) for storage, but `WebviewWindow` creation uses logical pixels (divide by DPI). The layout editor converts on save.

### Capabilities

All windows covered by `src-tauri/capabilities/default.json`. Dynamically-created layout editor windows are covered by the `"layout-item-*"` wildcard. Adding new window-level APIs requires adding the permission here (e.g., `core:window:allow-destroy` was a past gotcha).

### Licensing

Free tier: 1 group (enforced in `license.rs::group_limit()`). Paid: unlimited groups via LemonSqueezy → Cloudflare Worker proxy (`WORKER_URL` in `lib.rs`). The Worker handles `/activate`, `/deactivate`, `/validate`, `/feedback`.

### Item struct

`Item` in `config.rs` has these fields that determine launch behavior:
- `item_type`: App | File | Url | Folder | Script | Steam
- `path` / `value` / `urls`: where to launch
- `launch_virtual_desktop`: `Option<Vec<u8>>` — 16-byte GUID, set via layout editor
- `launch_desktop`: `Option<u32>` — monitor index, Steam items only
- `launch_x/y/width/height`: saved window position in physical pixels
- `run_as_admin`: uses `ShellExecuteExW` with "runas" verb
- `run_in_terminal`: script items — run via cmd/powershell vs open in default app
