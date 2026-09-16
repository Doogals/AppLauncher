# Capture Current — build a group from what's already open

**Status:** preliminary plan, awaiting approval
**Target:** 0.6.5 or 0.6.6

## What it does

A **Capture Current** button in the group editor. Press it and TakeOff scans
every open window, works out what launched it, and offers to add those as items
— including **where each window is**: monitor, position, size, and virtual
desktop.

That last part is the real value. Adding apps to a group is already easy. What's
tedious is arranging six windows across two desktops and then reproducing it by
hand in the layout editor. Capture Current says "however this looks right now,
make that the group."

It re-scans on every press, so it works at any time with the editor open.

## 1. Is it a preview, or does it just add everything?

**Recommendation: a preview window with checkboxes.**

A blind add is one button press away from "captured 14 items including Task
Manager, Notepad and a Steam update popup", and then the user has to delete
them one at a time from the list. The scan cannot be perfectly accurate — see
the limits in §4 — so the user needs to see what it found.

The preview also gives us somewhere honest to say *"Chrome — 5 tabs not
captured"* rather than silently producing a browser item that opens a blank
window.

Layout: reuse the `add-item.html` pattern — its own window, emits the chosen
items back to the config window. One row per window: icon, title, what it will
become, the desktop/monitor it is on, and a checkbox. Everything ticked by
default except the categories we know are unreliable.

## 2. Detection pipeline

Nearly all of this already exists in `launcher.rs`:

1. `enumerate_candidate_windows()` — Z-ordered, already filters `WS_EX_TOOLWINDOW`,
   DWM-cloaked windows, untitled windows and anything under 50×50.
2. For each HWND, resolve the owning process to a **full exe path**. The existing
   `exe_name_for_hwnd` returns only a lowercased filename; we need a sibling that
   keeps the full path from `QueryFullProcessImageNameW`.
3. Drop anything owned by **our own PID** — the widget, the config window, the
   layout editors, detached pills.
4. Drop a small denylist of shell surfaces that are never launchable items:
   `explorer.exe` desktop/taskbar windows, `ApplicationFrameHost.exe` shells with
   no resolvable child, `TextInputHost.exe`, `ShellExperienceHost.exe`.
5. Classify (§3).
6. Capture geometry with **`GetWindowPlacement`, not `GetWindowRect`**. A
   minimized window's `GetWindowRect` returns roughly `-32000, -32000`; saving
   that would place the item off-screen forever. `GetWindowPlacement` gives the
   restored rect, which is what we want to reproduce.
7. Capture the virtual desktop via the existing
   `virtual_desktop::get_window_virtual_desktop(hwnd)`.

Windows on **all** virtual desktops are captured, each tagged with its own
desktop — one press reproduces a whole multi-desktop arrangement.

## 3. What each window becomes

| Window | Item | Notes |
|---|---|---|
| Ordinary exe | `App` | `path` = full exe path |
| Packaged / MSIX | `Uwp` | Exe path matched against the packaged-app list `apps.rs` already builds, to recover the AUMID. Falls back to `App` if no match |
| Browser | `App` | Opens the browser with no tabs — see §4 |
| File Explorer | `Folder` | Needs the folder path — deferred, see §4 |
| Anything unresolved | skipped | Listed in the preview as "couldn't identify", greyed out |

Display name comes from the window title where it is sane, otherwise the
`.lnk`-style friendly name. Icons reuse `get_file_icon` with the same
`icon_source` chain built in 0.6.3.

## 4. Limits — state these plainly in the preview

**Browser tabs are not captured.** A window title gives the page title, never the
URL. Reading the address bar means UI Automation and per-browser quirks, and it
breaks whenever a browser reshuffles its accessibility tree. Capture Current
produces a plain browser item; the user adds URLs with the existing picker. The
preview row says so explicitly. A later phase could attempt UIA extraction as a
best-effort extra.

**Elevated apps may not resolve.** `OpenProcess` with
`PROCESS_QUERY_LIMITED_INFORMATION` generally fails against a higher-integrity
process from a medium-integrity caller. Those appear as "couldn't identify". We
do not prompt for elevation — a capture button is not worth a UAC prompt.

**File Explorer folder paths need shell COM** (`IShellWindows` to map an HWND to
its current folder). Worth doing, but it is a self-contained chunk; deferred to
phase 2 rather than blocking the feature.

**Documents open in an app** capture as the app, not the document. Word with a
file open becomes a Word item. Getting the document path means per-app
automation, which is out of scope.

## 5. Duplicates

Two questions, different answers:

- **Same app open twice** (two Explorer windows, two VS Code windows): capture
  both. They have different positions, and reproducing both is the point.
- **Already in the group**: flag it. The preview marks a row "already in this
  group" and unticks it by default, so pressing Capture twice does not silently
  double everything. The user can still tick it if they genuinely want two.

## 6. Where the button goes

In the group editor's `#add-item-area`, beside **+ Add Item**, as a secondary
button — `📸 Capture Current`. It is a peer action to adding an item, not a
save-row action, and putting it there means it sits directly above the list it
appends to.

Available for new groups too: the capture writes into `currentItems` in the
frontend, exactly like every other add path, so nothing needs the group to exist
on disk first.

## 7. Files touched

- `src-tauri/src/launcher.rs` — `exe_path_for_hwnd()` (full path), and
  `window_placement()` using `GetWindowPlacement`
- `src-tauri/src/capture.rs` **(new)** — the scan, classification, self-PID and
  denylist filtering, returning `Vec<CapturedWindow>`
- `src-tauri/src/lib.rs` — `capture_current_windows` command,
  `open_capture_window` command, register both
- `src/capture.html` / `src/capture.js` **(new)** — the preview picker
- `src/config.js` — the button, and receiving the chosen items
- `src/config.html`, `src/styles.css` — button + preview styling
- `vite.config.js`, `src-tauri/capabilities/default.json` — register the window

## 8. Rollout

**Phase 1 — the feature.** Everything above except the deferred items. Ships
behind no flag; it is additive and cannot damage an existing group, since
nothing is written until the user confirms the preview and then saves the group.

**Phase 2 — accuracy.** Explorer folder paths via `IShellWindows`; optional
best-effort browser URL capture via UI Automation. Both are independent and
either can be dropped without affecting phase 1.

**Licensing:** free by default. It makes a single group faster to build, which
is the free tier's whole job, and gating it would blunt the best demo TakeOff
has. Worth revisiting only if it turns out to be the thing people upgrade for.

## 9. Testing

`cargo test` cannot run here (see CLAUDE.md) — use `cargo check --tests`.

Unit-testable pure logic:
- the denylist and self-PID filter, given a synthetic window list
- classification: exe path → `ItemType`, including the packaged-app AUMID match
- the "already in this group" comparison

Manual:
1. Two apps on desktop 1, two on desktop 2, one maximized, one minimized →
   capture → every item lands on the right desktop with a sensible rect, and the
   minimized one is **not** at −32000
2. TakeOff's own windows never appear
3. An elevated app appears as "couldn't identify", not as a broken item
4. Capture twice → second run unticks everything already added
5. Capture into a brand-new unsaved group → Save & Close → items persist
6. Capture, then launch the group → windows land where they were captured

## 10. Open questions for Doug

1. Preview picker, or add everything immediately?
2. Capture positions and desktops by default, or only the apps?
3. Free feature, or licensed?
