# Zone Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fullscreen overlay location picker with a 3×3 zone grid embedded in the item expand panel in the config window.

**Architecture:** The `Item` struct swaps raw `launch_x`/`launch_y` pixel fields for a single `launch_position: Option<String>` zone key (`"top-left"`, `"center"`, etc.). A new `zone_to_coords` function in `launcher.rs` maps the zone to pixel coordinates at launch time using `GetSystemMetrics` for primary screen dimensions. The config window renders a 3×3 clickable grid inline — no overlay window, no Tauri commands for picking.

**Tech Stack:** Rust (Tauri v2), vanilla JS/HTML/CSS (Vite)

---

### Task 1: Migrate Item struct — swap x/y for launch_position

**Files:**
- Modify: `src-tauri/src/config.rs` (Item struct)
- Modify: `src-tauri/src/launcher.rs` (all test Item literals)

- [ ] **Step 1: Update Item struct in config.rs**

In `src-tauri/src/config.rs`, replace the two raw coordinate fields with one zone field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub item_type: ItemType,
    pub path: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub launch_desktop: Option<u32>,
    #[serde(default)]
    pub launch_position: Option<String>,
}
```

Remove `launch_x` and `launch_y` entirely. The `#[serde(default)]` on `launch_position` means existing config files with neither field (or with the old x/y fields) deserialize cleanly — old `launch_x`/`launch_y` keys are simply ignored by serde.

- [ ] **Step 2: Fix all Item literals in launcher.rs tests**

In `src-tauri/src/launcher.rs`, every test constructs an `Item` directly. Find all of them (search for `launch_x: None`) and update each to use `launch_position: None` instead of the two old fields.

Current pattern (appears ~6 times):
```rust
Item { item_type: ItemType::App, path: None, value: None, launch_desktop: None, launch_x: None, launch_y: None }
```

Replace every occurrence with:
```rust
Item { item_type: ItemType::App, path: None, value: None, launch_desktop: None, launch_position: None }
```

Do the same for `ItemType::Url`, `ItemType::Script` literals.

Also update the default item literal in `config.rs` if one exists (the test default group):
```rust
Item { item_type: ItemType::App, path: Some("C:\\slack.exe".to_string()), value: None, launch_desktop: None, launch_position: None },
Item { item_type: ItemType::Url, path: None, value: Some("https://github.com".to_string()), launch_desktop: None, launch_position: None },
```

- [ ] **Step 3: Run cargo check — expect compile errors only in launch_item (next task)**

```
cd src-tauri && cargo check
```

Expected: errors referencing `item.launch_x` / `item.launch_y` in `launcher.rs` and `lib.rs`. All test literals should be clean. If any other file references `launch_x`/`launch_y`, fix those now.

- [ ] **Step 4: Commit**

```
git add src-tauri/src/config.rs src-tauri/src/launcher.rs
git commit -m "refactor: replace launch_x/launch_y with launch_position on Item"
```

---

### Task 2: Add zone_to_coords and update launch_item

**Files:**
- Modify: `src-tauri/src/launcher.rs`

- [ ] **Step 1: Write a failing test for zone_to_coords**

Add this test to the `#[cfg(test)]` block at the bottom of `src-tauri/src/launcher.rs`:

```rust
#[test]
fn test_zone_to_coords_known_zones_return_some() {
    let zones = [
        "top-left", "top-center", "top-right",
        "center-left", "center", "center-right",
        "bottom-left", "bottom-center", "bottom-right",
    ];
    for zone in zones {
        assert!(zone_to_coords(zone, 1920, 1080).is_some(), "zone '{}' returned None", zone);
    }
}

#[test]
fn test_zone_to_coords_unknown_zone_returns_none() {
    assert!(zone_to_coords("invalid", 1920, 1080).is_none());
}

#[test]
fn test_zone_to_coords_top_left_is_origin() {
    assert_eq!(zone_to_coords("top-left", 1920, 1080), Some((0, 0)));
}

#[test]
fn test_zone_to_coords_bottom_right_is_two_thirds() {
    assert_eq!(zone_to_coords("bottom-right", 1920, 1080), Some((1280, 720)));
}
```

Note: the tests call `zone_to_coords(zone, w, h)` with explicit dimensions so the function is testable without WinAPI. The real caller passes live screen dimensions.

- [ ] **Step 2: Run tests — expect them to fail (function doesn't exist yet)**

```
cd src-tauri && cargo test zone_to_coords
```

Expected: compile error "cannot find function `zone_to_coords`".

- [ ] **Step 3: Add zone_to_coords and update position_window_for_item**

In `src-tauri/src/launcher.rs`, replace the top section (the Windows positioning block) with:

```rust
// ── Post-launch window positioning (Windows only) ────────────────────────────

fn zone_to_coords(zone: &str, screen_w: i32, screen_h: i32) -> Option<(i32, i32)> {
    let (col, row) = match zone {
        "top-left"      => (0, 0),
        "top-center"    => (1, 0),
        "top-right"     => (2, 0),
        "center-left"   => (0, 1),
        "center"        => (1, 1),
        "center-right"  => (2, 1),
        "bottom-left"   => (0, 2),
        "bottom-center" => (1, 2),
        "bottom-right"  => (2, 2),
        _ => return None,
    };
    Some((col * screen_w / 3, row * screen_h / 3))
}

#[cfg(target_os = "windows")]
fn position_window_for_item(pid: u32, x: i32, y: i32) {
    use std::thread;
    use std::time::Duration;

    thread::spawn(move || {
        let hwnd = (0..10).find_map(|_| {
            thread::sleep(Duration::from_millis(300));
            find_window_by_pid(pid)
        });
        if let Some(hwnd) = hwnd {
            move_window_to(hwnd, x, y);
        }
    });
}
```

`zone_to_coords` has no `#[cfg]` — it's pure math and fully testable on any platform.

- [ ] **Step 4: Update launch_item to use launch_position**

In `launch_item`, replace the old x/y branch:

```rust
// OLD — remove this:
#[cfg(target_os = "windows")]
if let (Some(x), Some(y)) = (item.launch_x, item.launch_y) {
    position_window_for_item(child.id(), x, y);
}

// NEW — replace with:
#[cfg(target_os = "windows")]
if let Some(zone) = &item.launch_position {
    extern "system" {
        fn GetSystemMetrics(n_index: i32) -> i32;
    }
    let (sw, sh) = unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) };
    if let Some((x, y)) = zone_to_coords(zone, sw, sh) {
        position_window_for_item(child.id(), x, y);
    }
}
```

`GetSystemMetrics(0)` = `SM_CXSCREEN` (primary screen width), `GetSystemMetrics(1)` = `SM_CYSCREEN` (primary screen height).

- [ ] **Step 5: Run tests — expect all to pass**

```
cd src-tauri && cargo test
```

Expected: all tests pass including the four new `zone_to_coords` tests.

- [ ] **Step 6: Commit**

```
git add src-tauri/src/launcher.rs
git commit -m "feat: zone-based window positioning replaces pixel coordinates"
```

---

### Task 3: Remove picker commands from lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Delete the three picker command functions**

In `src-tauri/src/lib.rs`, find and delete these three functions in their entirety:
- `fn start_location_picker(app: tauri::AppHandle) -> Result<(), String>`
- `fn finish_location_picker(x: i32, y: i32, app: tauri::AppHandle) -> Result<(), String>`
- `fn cancel_location_picker(app: tauri::AppHandle) -> Result<(), String>`

- [ ] **Step 2: Remove them from the invoke_handler list**

Find the `invoke_handler` / `generate_handler!` call (near the bottom of `lib.rs`). Remove these three entries:
- `start_location_picker`
- `finish_location_picker`
- `cancel_location_picker`

- [ ] **Step 3: Run cargo check — expect clean**

```
cd src-tauri && cargo check
```

Expected: same 4 pre-existing warnings, zero errors.

- [ ] **Step 4: Commit**

```
git add src-tauri/src/lib.rs
git commit -m "chore: remove fullscreen picker Tauri commands"
```

---

### Task 4: Replace picker UI in config.js

**Files:**
- Modify: `src/config.js`

- [ ] **Step 1: Remove picker state and event listener**

At the top of `src/config.js`:
- Remove the line: `let pendingPickIdx = null;`
- Remove the `listen` import (line 2 — `import { listen } from '@tauri-apps/api/event';`) since it's only used by the picker

At the bottom of `src/config.js`, delete the entire `listen('location-picked', ...)` block:
```js
// DELETE THIS ENTIRE BLOCK:
listen('location-picked', (event) => {
  if (pendingPickIdx === null) return;
  const { x, y } = event.payload;
  currentItems[pendingPickIdx].launch_x = x;
  currentItems[pendingPickIdx].launch_y = y;
  pendingPickIdx = null;
  renderItems();
});
```

- [ ] **Step 2: Replace buildExpandPanel with zone grid version**

Find `function buildExpandPanel(item, idx)` and replace the entire function body with:

```js
function buildExpandPanel(item, idx) {
  const ZONES = [
    ['top-left', '↖'], ['top-center', '↑'], ['top-right', '↗'],
    ['center-left', '←'], ['center', '·'], ['center-right', '→'],
    ['bottom-left', '↙'], ['bottom-center', '↓'], ['bottom-right', '↘'],
  ];

  const panel = document.createElement('div');
  panel.className = 'item-expand';

  const row = document.createElement('div');
  row.className = 'item-expand-row';
  row.appendChild(Object.assign(document.createElement('span'), { textContent: 'Launch at' }));

  const grid = document.createElement('div');
  grid.className = 'zone-grid';

  ZONES.forEach(([zone, arrow]) => {
    const cell = document.createElement('button');
    cell.className = 'zone-cell' + (item.launch_position === zone ? ' active' : '');
    cell.title = zone;
    cell.textContent = arrow;
    cell.addEventListener('click', () => {
      currentItems[idx].launch_position =
        currentItems[idx].launch_position === zone ? null : zone;
      renderItems();
    });
    grid.appendChild(cell);
  });

  row.appendChild(grid);
  panel.appendChild(row);
  return panel;
}
```

Clicking an already-active cell deselects it (toggle behavior). No Tauri invoke needed.

- [ ] **Step 3: Verify no remaining references to old picker**

```
grep -n "launch_x\|launch_y\|pendingPickIdx\|start_location_picker\|location-picked\|pick-btn\|coord-display" src/config.js
```

Expected: no matches.

- [ ] **Step 4: Commit**

```
git add src/config.js
git commit -m "feat: replace overlay picker with inline 3x3 zone grid"
```

---

### Task 5: Update styles.css

**Files:**
- Modify: `src/styles.css`

- [ ] **Step 1: Remove old picker styles**

Find and delete these CSS rule blocks entirely:
- `.coord-display { ... }` and `.coord-display.coord-empty { ... }`
- `.coord-clear { ... }` and `.coord-clear:hover { ... }`
- `.pick-btn { ... }` and `.pick-btn:hover { ... }`

- [ ] **Step 2: Add zone grid styles**

In place of the deleted rules, add:

```css
.zone-grid {
  display: grid;
  gap: 2px;
  grid-template-columns: repeat(3, 1fr);
}

.zone-cell {
  background: #16213e;
  border: 1px solid #0f3460;
  border-radius: 3px;
  color: #555;
  cursor: pointer;
  font-size: 0.75rem;
  padding: 3px 6px;
  text-align: center;
}

.zone-cell:hover {
  border-color: #e94560;
  color: #e0e0e0;
}

.zone-cell.active {
  background: #0f3460;
  border-color: #4caf50;
  color: #4caf50;
}
```

- [ ] **Step 3: Commit**

```
git add src/styles.css
git commit -m "style: swap coord/pick-btn styles for zone-grid"
```

---

### Task 6: Delete picker files

**Files:**
- Delete: `src/picker.html`
- Delete: `src/picker.js`

- [ ] **Step 1: Delete both files**

```
git rm src/picker.html src/picker.js
```

- [ ] **Step 2: Run cargo check one final time**

```
cd src-tauri && cargo check
```

Expected: same 4 pre-existing warnings, zero errors.

- [ ] **Step 3: Commit**

```
git commit -m "chore: delete fullscreen picker overlay files"
```

---

### Task 7: Smoke test

- [ ] **Step 1: Run full test suite**

```
cd src-tauri && cargo test
```

Expected: all tests pass.

- [ ] **Step 2: Run dev build and open config window**

```
npm run tauri dev
```

Open the widget, right-click → App Settings, open a group, expand an item with the chevron. Verify:
- A 3×3 arrow grid appears under "Launch at"
- Clicking a cell highlights it green (active state)
- Clicking the same cell again deselects it
- Clicking a different cell moves the highlight
- Saving the group and reopening it retains the selected zone

- [ ] **Step 3: Verify old coords in config.json are gracefully ignored**

If `%LOCALAPPDATA%\AppLauncher\config.json` has `launch_x`/`launch_y` fields on any item, confirm the app still loads without error (serde ignores unknown fields by default).
