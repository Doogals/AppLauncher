# Batched Desktop Launch + Desktop-1 Default — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate back-and-forth virtual desktop switching during group launch by batching items that share the same target desktop, and remove the "Any desktop" option from the layout editor so every item always has an explicit desktop assignment defaulting to Desktop 1.

**Architecture:** Two independent changes. Part 1 is a Rust-only change to `launch_group` in `launcher.rs` — replace the two sequential per-item loops with a batch-building pre-pass (group by target desktop, first-appearance order) and a batch-execution loop. Part 2 is a frontend change to `layout-item.html` and `layout-item.js` — remove the blank dropdown option, default new/null items to Desktop 1, and immediately register that default with Rust via `set_layout_item_desktop`. No data-model changes, no migration.

**Tech Stack:** Rust (`src-tauri/src/launcher.rs`), Vanilla JS + HTML (`src/layout-item.js`, `src/layout-item.html`)

---

### Task 1: Remove "Any desktop" from the layout editor dropdown

**Files:**
- Modify: `src/layout-item.html:75`
- Modify: `src/layout-item.js:33-45`

- [ ] **Step 1: Remove the blank option from the HTML**

Current content at `src/layout-item.html:74-76`:

```html
    <select id="pk-desktop-sel" style="background:#16213e;color:#c8c8d8;border:1px solid #0f3460;border-radius:4px;font-size:0.75rem;padding:3px 6px;cursor:pointer;min-width:110px;">
      <option value="">Any desktop</option>
    </select>
```

Replace with:

```html
    <select id="pk-desktop-sel" style="background:#16213e;color:#c8c8d8;border:1px solid #0f3460;border-radius:4px;font-size:0.75rem;padding:3px 6px;cursor:pointer;min-width:110px;">
    </select>
```

- [ ] **Step 2: Update `initDesktopDropdown` to default to Desktop 1**

Current content at `src/layout-item.js:33-45`:

```js
  // Pre-select the previously saved virtual desktop for this item (passed via URL param).
  const vdParam = params.get('vd');
  if (vdParam) {
    const savedGuid = JSON.parse(decodeURIComponent(vdParam));
    const savedStr = JSON.stringify(savedGuid);
    for (const opt of sel.options) {
      if (opt.value === savedStr) {
        opt.selected = true;
        // Store in Rust so complete_layout_save picks it up even if user doesn't change it.
        invoke('set_layout_item_desktop', { label, guid: savedGuid }).catch(() => {});
        break;
      }
    }
  }
```

Replace with:

```js
  // Pre-select the saved desktop, or default to Desktop 1 for items with no explicit target.
  const vdParam = params.get('vd');
  const savedGuid = vdParam
    ? JSON.parse(decodeURIComponent(vdParam))
    : desktops[0].guid;
  const savedStr = JSON.stringify(savedGuid);
  for (const opt of sel.options) {
    if (opt.value === savedStr) {
      opt.selected = true;
      break;
    }
  }
  // Always register with Rust so complete_layout_save captures it without requiring a change.
  invoke('set_layout_item_desktop', { label, guid: savedGuid }).catch(() => {});
```

- [ ] **Step 3: Verify in the browser**

Run the dev server (`npm run tauri dev`), open a group's edit window, click "📐 Edit Layout".

In the layout-item window's desktop dropdown:
- If the item has a saved desktop: correct desktop is pre-selected, no blank row visible
- If the item has no saved desktop (null): "Desktop 1" is pre-selected, no blank row visible

Open devtools on the layout-item window, run: `invoke('get_virtual_desktops')` — confirm first entry matches what the dropdown shows.

- [ ] **Step 4: Commit**

```
git add src/layout-item.html src/layout-item.js
git commit -m "Remove 'Any desktop' option; default layout-item desktop to Desktop 1"
```

---

### Task 2: Replace per-item launch loops with desktop-batched execution

**Files:**
- Modify: `src-tauri/src/launcher.rs:458-495`

- [ ] **Step 1: Replace the two sequential loops with the batched implementation**

Current content at `src-tauri/src/launcher.rs:458-495`:

```rust
    // Launch non-URL items: switch to target desktop first (using tracked current), then launch.
    for item in &group.items {
        if !matches!(item.item_type, ItemType::Url) {
            crate::debug_log::write_debug_log(&format!(
                "LAUNCH item type={:?} path=\"{}\"",
                item.item_type,
                item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
            ));
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
            launch_item(item, &config.preferred_browser)?;
        }
    }

    // URL items with a saved position or target desktop: launch individually.
    for item in &group.items {
        if matches!(item.item_type, ItemType::Url)
            && (item.launch_x.is_some() || item.launch_virtual_desktop.is_some())
        {
            #[cfg(target_os = "windows")]
            if let Some(ref guid) = item.launch_virtual_desktop {
                if guid.as_slice() != current_desktop.as_slice() {
                    crate::virtual_desktop::switch_virtual_desktop(&current_desktop, guid);
                    current_desktop = guid.clone();
                }
            }
            launch_item(item, &config.preferred_browser)?;
        }
    }
```

Replace with:

```rust
    // Windows: when at least one item has an explicit desktop target, build ordered batches
    // (grouped by target GUID, first-appearance order) and execute each batch with a single
    // desktop switch. Items with no explicit target are folded into the Desktop-1 batch.
    // When no items have explicit targets (needs_vd = false), fall through to the sequential
    // path below — identical to pre-batch behavior, no VD switches.
    #[cfg(target_os = "windows")]
    if needs_vd {
        let desktop_1_guid: Vec<u8> = {
            let desktops = crate::virtual_desktop::get_virtual_desktops();
            desktops.into_iter().next().map(|d| d.guid).unwrap_or_default()
        };
        let mut batches: Vec<(Vec<u8>, Vec<&Item>)> = Vec::new();
        for item in &group.items {
            // URL items without a position or explicit desktop skip the batch loop entirely;
            // they are collected into the multi-tab batch at the end of launch_group.
            let in_batch = !matches!(item.item_type, ItemType::Url)
                || item.launch_x.is_some()
                || item.launch_virtual_desktop.is_some();
            if !in_batch { continue; }
            let target = item.launch_virtual_desktop.clone()
                .unwrap_or_else(|| desktop_1_guid.clone());
            match batches.iter_mut().find(|(guid, _)| *guid == target) {
                Some(batch) => batch.1.push(item),
                None => batches.push((target, vec![item])),
            }
        }
        for (target_guid, items) in &batches {
            if target_guid.as_slice() != current_desktop.as_slice() {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH batch switching desktop ({} item(s))", items.len()
                ));
                crate::virtual_desktop::switch_virtual_desktop(&current_desktop, target_guid);
                current_desktop = target_guid.clone();
            }
            for item in items {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }

    // Sequential fallback — two cases:
    // (a) Windows + needs_vd = false: no desktop targets, launch items in order, no switching.
    // (b) Non-Windows: desktop targeting not supported, always sequential.
    #[cfg(target_os = "windows")]
    if !needs_vd {
        for item in &group.items {
            if !matches!(item.item_type, ItemType::Url) {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
        for item in &group.items {
            if matches!(item.item_type, ItemType::Url)
                && (item.launch_x.is_some() || item.launch_virtual_desktop.is_some())
            {
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for item in &group.items {
            if !matches!(item.item_type, ItemType::Url) {
                crate::debug_log::write_debug_log(&format!(
                    "LAUNCH item type={:?} path=\"{}\"",
                    item.item_type,
                    item.path.as_deref().or(item.value.as_deref()).unwrap_or("?")
                ));
                launch_item(item, &config.preferred_browser)?;
            }
        }
        for item in &group.items {
            if matches!(item.item_type, ItemType::Url)
                && (item.launch_x.is_some() || item.launch_virtual_desktop.is_some())
            {
                launch_item(item, &config.preferred_browser)?;
            }
        }
    }
```

- [ ] **Step 2: Verify the build compiles**

Run: `npm run tauri dev`

Expected: build succeeds, no errors. One existing warning (`clear_debug_log` unused) is pre-existing and acceptable. Any new errors or warnings must be resolved before continuing.

- [ ] **Step 3: Commit**

```
git add src-tauri/src/launcher.rs
git commit -m "Batch group launch by target desktop to eliminate back-and-forth switching"
```

---

### Task 3: Manual QA — end-to-end verification

**Files:** none (manual QA against the running dev build using the check_app_launcher tool)

> No JS test runner is configured in this project. Verification is done by exercising the real
> UI and using the MCP debug tool to set up controlled scenarios and inspect results.

**Setup:** The `mcp__local-hub__check_app_launcher` tool is used for all steps below.
The "PC Monitoring" group (2 items by default) is used as the test group. A 3rd item is
temporarily added for the batching test.

- [ ] **Step 1: Verify "Any desktop" is gone from the layout editor**

Action: `open_edit`, target: `"PC Monitoring"` → in the app, click "📐 Edit Layout".

Expected: the "Launch on:" dropdown in the layout-item window shows named desktops only
(e.g., "Desktop 1", "Desktop 2") with no blank/empty first row. Close the layout editor
via "Cancel All" when done.

- [ ] **Step 2: Verify a null-desktop item defaults to Desktop 1 in the layout editor**

First confirm item 0 currently has `launch_virtual_desktop: null` in the saved config
(if not, temporarily clear it via direct PowerShell edit):

```powershell
$path = "$env:LOCALAPPDATA\AppLauncher\config.json"
$raw = (Get-Content -Raw $path -Encoding UTF8).TrimStart([char]0xFEFF)
$cfg = $raw | ConvertFrom-Json
$g = $cfg.groups | Where-Object { $_.name -eq "PC Monitoring" }
Write-Output "Item 0 vd: $($g.items[0].launch_virtual_desktop)"
```

Expected output: `Item 0 vd: ` (empty/null).

Action: `open_edit`, target `"PC Monitoring"` → click "📐 Edit Layout" → observe item 0's
dropdown in the layout-item window.

Expected: dropdown shows "Desktop 1" selected (not blank). Click "Save All Positions".

Verify the saved config now has an explicit GUID for item 0:

```powershell
$raw = (Get-Content -Raw $path -Encoding UTF8).TrimStart([char]0xFEFF)
$cfg = $raw | ConvertFrom-Json
$g = $cfg.groups | Where-Object { $_.name -eq "PC Monitoring" }
Write-Output "Item 0 vd byte count: $($g.items[0].launch_virtual_desktop.Count)"
```

Expected: `Item 0 vd byte count: 16` (a 16-byte GUID, not 0/null).

Restore item 0 to null after this step (so subsequent steps start clean):

Tool: `clear_item_position`, target `"PC Monitoring"`, params `{"item_index": 0}` — then
`set_item_position` to restore the original coordinates `{"item_index":0,"x":1976,"y":94,"width":1828,"height":1429}`.

- [ ] **Step 3: Set up the [D2, D1, D2] batching scenario**

Add a temporary 3rd item to "PC Monitoring":

Tool: `add_item`, target `"PC Monitoring"`, params `{"item_type":"file","path":"C:\\Windows\\System32\\notepad.exe"}`

Assign desktops — item 0 → Desktop 2, item 1 → Desktop 1, item 2 → Desktop 2:

Tool: `set_item_desktop`, target `"PC Monitoring"`, params `{"item_index":0,"desktop":2}`
Tool: `set_item_desktop`, target `"PC Monitoring"`, params `{"item_index":1,"desktop":1}`
Tool: `set_item_desktop`, target `"PC Monitoring"`, params `{"item_index":2,"desktop":2}`

- [ ] **Step 4: Clear the log and launch**

Tool: `clear_log`
Tool: `launch`, target `"PC Monitoring"`, params `{"wait_ms":8000}`

- [ ] **Step 5: Inspect the log for batching evidence**

Tool: `log`, params `{"lines":60}`

Expected log pattern (exact timestamps will vary):

```
LAUNCH group "PC Monitoring" (3 items)
LAUNCH batch switching desktop (2 item(s))    ← D2 batch (items 0 + 2 grouped)
LAUNCH item type=File path="...netlify.png"
LAUNCH item type=File path="...notepad.exe"
LAUNCH batch switching desktop (1 item(s))    ← D1 batch (item 1)
LAUNCH item type=File path="...AppLauncher-Ideas.txt"
VD switch Desktop2→Desktop1 (1 step(s))       ← restore original desktop
VD switch confirmed after ...ms
```

Key check: "LAUNCH batch switching desktop (2 item(s))" must appear exactly once for
Desktop 2 — confirming items 0 and 2 are launched together in one batch rather than
requiring a return trip. There must be NO "LAUNCH item ... switching desktop" messages
(those appeared in the old per-item switch code and must not appear after this change).

If the log still shows per-item switch messages ("LAUNCH item ... switching desktop"),
the old code path is still running — check that the correct branch compiled.

- [ ] **Step 6: Verify desktop landing via position report**

The `launch` action's output already includes a position report. Confirm:

```
Item 0 "netlify.png"      Desktop: expected=Desktop 2  actual=Desktop 2  ✓
Item 1 "AppLauncher-Ideas.txt"  Desktop: expected=Desktop 1  actual=Desktop 1  ✓
Item 2 "notepad.exe"      Desktop: expected=Desktop 2  actual=Desktop 2  ✓
```

- [ ] **Step 7: Verify groups without desktop targets are unchanged**

Temporarily remove desktop assignments from all three items by editing config.json directly:

```powershell
$path = "$env:LOCALAPPDATA\AppLauncher\config.json"
$raw = (Get-Content -Raw $path -Encoding UTF8).TrimStart([char]0xFEFF)
$cfg = $raw | ConvertFrom-Json
$g = $cfg.groups | Where-Object { $_.name -eq "PC Monitoring" }
$g.items | ForEach-Object { $_.launch_virtual_desktop = $null }
[System.IO.File]::WriteAllText($path, ($cfg | ConvertTo-Json -Depth 20), (New-Object System.Text.UTF8Encoding($false)))
```

Tool: `reload`
Tool: `clear_log`
Tool: `launch`, target `"PC Monitoring"`, params `{"wait_ms":5000}`
Tool: `log`

Expected: NO "LAUNCH batch switching desktop" messages, NO "VD switch" messages. Items
launch sequentially without any desktop switching — confirming the `!needs_vd` fallback
path is active and correct.

- [ ] **Step 8: Clean up test state**

Remove the temporary 3rd item:

Tool: `remove_item`, target `"PC Monitoring"`, params `{"item_index":2}`

Restore item 0's saved position (was cleared during Step 2):

Tool: `set_item_position`, target `"PC Monitoring"`, params `{"item_index":0,"x":1976,"y":94,"width":1828,"height":1429}`

Verify final state of "PC Monitoring" matches pre-test (2 items, positions preserved, no desktop assignments):

```powershell
$raw = (Get-Content -Raw "$env:LOCALAPPDATA\AppLauncher\config.json" -Encoding UTF8).TrimStart([char]0xFEFF)
$cfg = $raw | ConvertFrom-Json
$g = $cfg.groups | Where-Object { $_.name -eq "PC Monitoring" }
for ($i = 0; $i -lt $g.items.Count; $i++) {
    $it = $g.items[$i]
    $vd = if ($it.launch_virtual_desktop) { "SET" } else { "null" }
    Write-Output "[$i] x=$($it.launch_x) y=$($it.launch_y) vd=$vd"
}
```

Expected:
```
[0] x=1976 y=94 vd=null
[1] x=118 y=116 vd=null
```
