# App Launcher Checker — Design Spec

**Date:** 2026-05-19  
**Status:** Approved  
**Purpose:** Give Claude a set of capabilities to trigger actions in the App Launcher, observe outcomes visually, read internal traces, and mutate config — so iterating on bugs doesn't require asking the user to test manually.

---

## Overview

Three components built together:

1. **Tauri debug HTTP server** — minimal REST API inside the app, dev builds only
2. **Debug log** — structured append-only log written by Rust at key execution points
3. **MCP tool** (`tools/check_app_launcher.js`) — single tool in local-hub with an `action` enum; Claude calls it repeatedly in whatever order the task demands

The tool is not a predefined workflow. Claude reasons about what to do and calls the tool as needed — take a screenshot, launch a group, read the log, compare positions, mutate config, relaunch.

---

## Component 1: Tauri Debug HTTP Server

**File:** `src-tauri/src/debug_server.rs`  
**Gate:** `#[cfg(debug_assertions)]` — compiled and started only in dev builds  
**Binding:** `127.0.0.1:7891`  
**Dependency:** `axum` (uses Tokio, already present via Tauri)  
**Start:** Called from `lib.rs` setup block inside `#[cfg(debug_assertions)]`

### Endpoints

| Method | Path | Behaviour |
|--------|------|-----------|
| `GET` | `/state` | Serialise full `AppState` (groups + items) as JSON |
| `POST` | `/launch/:group_id` | Call `launcher::launch_group` with current config |
| `POST` | `/edit/:group_id` | Call `open_config_window(Some(group_id))` |
| `POST` | `/reload` | Re-read `config.json` from disk into `AppState` |
| `GET` | `/log` | Read and return `%TEMP%\applauncher-debug.log` as plain text |
| `GET` | `/windows` | Enumerate all visible HWNDs via `EnumWindows`; return title + physical rect (`GetWindowRect`) + virtual desktop GUID (`IVirtualDesktopManager::GetWindowDesktopId`) for each |

All responses are JSON. Errors return `{ "error": "..." }` with a 4xx/5xx status.

### Access to AppState

The server receives a cloned `tauri::AppHandle` at startup and accesses `AppState` via `app.state::<AppState>()` — same pattern used by existing Tauri commands.

---

## Component 2: Debug Log

**Path:** `%TEMP%\applauncher-debug.log`  
**Format:** `[HH:MM:SS] CATEGORY message\n` — plain text, append-only  
**Function:** `fn write_debug_log(msg: &str)` in a new `src-tauri/src/debug_log.rs`  
**Gate:** Always compiled; in release builds it is a no-op (`#[cfg(not(debug_assertions))]` returns early)

### Log sites

**`virtual_desktop.rs`:**
```
[HH:MM:SS] VD switch D2→D1 sending Win+Ctrl+Left (1 step)
[HH:MM:SS] VD key event 1/6 sent
[HH:MM:SS] VD registry confirmed D1 after 312ms
[HH:MM:SS] VD switch timed out after 600ms — proceeding anyway
```

**`launcher.rs`:**
```
[HH:MM:SS] LAUNCH group "Work" (2 items)
[HH:MM:SS] LAUNCH item 0 "test-script.bat" type=script target_desktop=Desktop1
[HH:MM:SS] LAUNCH item 0 HWND 0x3A04 found after 280ms phase=1
[HH:MM:SS] LAUNCH item 0 positioned at (-11, 400) 800×600
[HH:MM:SS] LAUNCH item 0 phase1 timeout — spawning phase2 background thread
[HH:MM:SS] LAUNCH item 1 "Site-Tasks.txt" type=file target_desktop=Desktop2
[HH:MM:SS] LAUNCH group "Work" complete
```

---

## Component 3: MCP Tool

**File:** `C:\Users\dougb\Desktop\Claude\tools\check_app_launcher.js`  
**Registered automatically** by local-hub's tool auto-loader on server restart

### Schema

```js
{
  action: z.enum([18 values — see below]),
  target: z.string().optional(),   // group name for most actions
  params: z.record(z.any()).optional()  // action-specific extra args
}
```

`target` is always a **group name** (case-insensitive match). For item-level actions, `params.item_index` is the 0-based item index within that group.

### Actions

#### Observe

| Action | Target | Params | What it does |
|--------|--------|--------|--------------|
| `screenshot` | — | `{ monitor?: number }` | Capture all monitors (or a specific one) via PowerShell `System.Windows.Forms`. Returns base64 PNG(s) as `image` content blocks |
| `log` | — | `{ lines?: number }` | Read last N lines of debug log (default 100) |
| `clear_log` | — | — | Truncate debug log to empty |
| `config` | — | — | Read and return full `config.json` |
| `list_groups` | — | — | Return `[{ id, name, icon, item_count }]` |
| `list_desktops` | — | — | Read `VirtualDesktopIDs` from registry; return `[{ index, name, guid_hex }]` |
| `get_windows` | — | — | `GET /windows` → `[{ title, x, y, width, height }]` |

#### App Interactions

| Action | Target | Params | What it does |
|--------|--------|--------|--------------|
| `launch` | group name | `{ wait_ms?: number }` | `POST /launch/:id`; waits `wait_ms` (default 3000); then automatically runs `get_windows` + `screenshot` (all desktops) + `log` tail; returns combined report including position comparison table (see below) |
| `open_edit` | group name | — | `POST /edit/:id`; waits 500ms; takes screenshot |
| `reload` | — | — | `POST /reload` — tells app to re-read config from disk |

#### Config Mutations
All mutations: read `config.json` → apply change → write back → call `POST /reload`.

| Action | Target | Params | What it does |
|--------|--------|--------|--------------|
| `add_group` | — | `{ name, icon }` | Appends new group with `crypto.randomUUID()` |
| `delete_group` | group name | — | Removes group by name |
| `rename_group` | group name | `{ name }` | Updates group name |
| `add_item` | group name | `{ item_type, path, value? }` | Appends minimal item; other fields default to null/false |
| `remove_item` | group name | `{ item_index }` | Removes item at index |
| `set_item_desktop` | group name | `{ item_index, desktop }` | `desktop` is 1-based integer; tool reads registry to resolve to 16-byte GUID array; writes to `launch_virtual_desktop` |
| `set_item_position` | group name | `{ item_index, x, y, width, height }` | Sets physical-pixel position fields |
| `clear_item_position` | group name | `{ item_index }` | Nulls `launch_x/y/width/height` and `launch_virtual_desktop` |

#### Utility

| Action | Target | Params | What it does |
|--------|--------|--------|--------------|
| `wait` | — | `{ ms }` | Sleep for ms milliseconds |

---

## Position Comparison (automatic after `launch`)

After the wait period, the tool collects three sources and cross-references them:

1. **Config saved** — `launch_x/y/width/height` from each item in the launched group
2. **`GET /windows` reported** — live OS window positions; matched to items by executable name or title heuristic
3. **Screenshots** — one per desktop, returned as image content so Claude can visually verify the numbers

**Report format per item:**
```
Item 0 "test-script.bat"  target=Desktop1
  Config saved:    x=-11  y=400  800×600
  OS reported:     x=-11  y=400  800×600   ✓ match
  Desktop:         expected=Desktop1  actual=Desktop1  ✓

Item 1 "Site-Tasks.txt"   target=Desktop2
  Config saved:    x=2311  y=150  1200×800
  OS reported:     x=2309  y=152  1200×800  ✓ within tolerance (±5px)
  Desktop:         expected=Desktop2  actual=Desktop1  ✗ MISMATCH
```

Tolerance for position match: ±5 pixels (accounts for DPI rounding).  
Desktop match: compare the desktop GUID reported by `GetWindowDesktopId` (called per HWND in `/windows`) against `launch_virtual_desktop` in config.

Screenshots accompany the report so Claude can visually confirm window placement matches both the numbers and expectations.

---

## Error Handling

- If the HTTP server is unreachable (app not running): return a clear message — "App Launcher dev server not responding on port 7891 — is `npm run tauri dev` running?"
- If a group name doesn't match: list available group names in the error
- If a mutation would produce invalid config (e.g., negative item_index): validate before writing
- Screenshot failures are non-fatal — return whatever was captured with a note

---

## Files Changed / Created

| File | Change |
|------|--------|
| `src-tauri/src/debug_server.rs` | New — HTTP server |
| `src-tauri/src/debug_log.rs` | New — `write_debug_log` function |
| `src-tauri/src/lib.rs` | Start debug server + add `write_debug_log` calls |
| `src-tauri/src/launcher.rs` | Add `write_debug_log` calls |
| `src-tauri/src/virtual_desktop.rs` | Add `write_debug_log` calls |
| `src-tauri/Cargo.toml` | Add `axum` dependency |
| `C:\Users\dougb\Desktop\Claude\tools\check_app_launcher.js` | New — MCP tool |
