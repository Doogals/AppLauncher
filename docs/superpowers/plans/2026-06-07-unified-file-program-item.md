# Unified "File / Program" Item-Menu Entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the "App / Executable", "File", and "Script (.bat / .ps1)" rows in the
"Add Item" menu into a single "File / Program" entry that auto-detects the correct
`item_type` from the picked file's extension.

**Architecture:** Pure frontend change in the config window. The merged menu entry opens
the existing generic (unfiltered) file dialog; a new pure helper `detectItemType(path)`
classifies the chosen path by extension into `app`/`script`/`file`. `Folder`,
`Windows Apps`, `URL / Bookmark`, and `Steam Game` keep their own menu rows and add-flows
untouched. No Rust/backend or data-model changes — `Item.item_type` already supports all
three resulting values and every downstream consumer (launch logic, expand-panel options,
rendering, icons) already keys off `item_type`.

**Tech Stack:** Vanilla JS + HTML (Vite-served), `@tauri-apps/plugin-dialog` `open()`.

---

### Task 1: Collapse the Add-Item menu rows

**Files:**
- Modify: `src/config.html:29-35`

- [ ] **Step 1: Replace the seven menu rows with five**

Current content at `src/config.html:29-35`:

```html
        <div class="context-menu-item" data-type="app">🖥️ App / Executable</div>
        <div class="context-menu-item" data-type="winapp">🪟 Windows Apps</div>
        <div class="context-menu-item" data-type="file">📄 File</div>
        <div class="context-menu-item" data-type="url">🌐 URL / Bookmark</div>
        <div class="context-menu-item" data-type="folder">📁 Folder</div>
        <div class="context-menu-item" data-type="script">⚡ Script (.bat / .ps1)</div>
        <div class="context-menu-item" data-type="steam">🎮 Steam Game</div>
```

Replace it with:

```html
        <div class="context-menu-item" data-type="file">📄 File / Program</div>
        <div class="context-menu-item" data-type="winapp">🪟 Windows Apps</div>
        <div class="context-menu-item" data-type="url">🌐 URL / Bookmark</div>
        <div class="context-menu-item" data-type="folder">📁 Folder</div>
        <div class="context-menu-item" data-type="steam">🎮 Steam Game</div>
```

This removes the `data-type="app"` and `data-type="script"` rows entirely and renames
the `data-type="file"` row's label. The `data-type` values that remain
(`file`, `winapp`, `url`, `folder`, `steam`) are exactly what `addItem` will receive —
`app` and `script` will no longer be passed in from the menu.

- [ ] **Step 2: Commit**

```bash
git add src/config.html
git commit -m "Collapse App/File/Script add-item rows into single File/Program entry"
```

---

### Task 2: Add the `detectItemType` helper

**Files:**
- Modify: `src/config.js` (add new function immediately above `addItem`, currently at `src/config.js:796`)

- [ ] **Step 1: Add the helper function**

Insert this directly above the `async function addItem(type) {` line (`src/config.js:796`):

```js
function detectItemType(path) {
  const ext = path.split('.').pop().toLowerCase();
  if (ext === 'exe') return 'app';
  if (['bat', 'cmd', 'ps1'].includes(ext)) return 'script';
  return 'file';
}
```

This is a pure function: given a filesystem path, it returns one of `'app'`,
`'script'`, or `'file'` based on the extension, matching the mapping used by the old
App/Script picker filters (`['exe', 'bat', 'ps1', 'cmd']`). Anything that isn't
`.exe`/`.bat`/`.cmd`/`.ps1` falls through to `'file'`.

- [ ] **Step 2: Sanity-check the function in the browser console**

Run the dev server (`npm run tauri dev`), open the config window, open its devtools
(right-click → Inspect, or `Ctrl+Shift+I` if enabled), and in the console run:

```js
detectItemType('C:\\Program Files\\Foo\\Foo.exe')   // expect 'app'
detectItemType('C:\\Scripts\\backup.PS1')           // expect 'script' (case-insensitive)
detectItemType('C:\\Scripts\\run.bat')              // expect 'script'
detectItemType('C:\\Users\\me\\notes.txt')          // expect 'file'
detectItemType('C:\\Users\\me\\README')             // expect 'file' (no extension)
```

Expected: each call returns the value noted in the comment. If `detectItemType` is not
defined in the console, confirm the dev server picked up the file change (it hot-reloads
on save) and that the function was inserted at module scope, not nested inside another
function.

- [ ] **Step 3: Commit**

```bash
git add src/config.js
git commit -m "Add detectItemType helper for extension-based item classification"
```

---

### Task 3: Wire the merged entry into `addItem`

**Files:**
- Modify: `src/config.js:796-831`

- [ ] **Step 1: Replace the picker block in `addItem`**

Current content at `src/config.js:796-831`:

```js
async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';
  fitWindow();

  if (type === 'winapp') {
    await showWinAppPicker();
    return;
  }

  if (type === 'url') {
    await showUrlPicker();
    return;
  }

  if (type === 'steam') {
    await showSteamPicker();
    return;
  }

  const filters = type === 'app' || type === 'script'
    ? [{ name: 'Executable', extensions: ['exe', 'bat', 'ps1', 'cmd'] }]
    : [];
  const selected = await open({
    title: `Select ${type}`,
    directory: type === 'folder',
    filters: filters.length ? filters : undefined,
  });
  if (!selected) return;
  let icon_data = null;
  try { icon_data = await invoke('get_file_icon', { path: selected }); } catch {}
  const newItem = { item_type: type, path: selected, value: null, icon_data };
  if (type === 'script') newItem.run_in_terminal = true;
  currentItems.push(newItem);

  renderItems();
}
```

Replace the `filters`/`open`/`newItem` portion (everything from `const filters = ...`
through `currentItems.push(newItem);`) so the whole function reads:

```js
async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';
  fitWindow();

  if (type === 'winapp') {
    await showWinAppPicker();
    return;
  }

  if (type === 'url') {
    await showUrlPicker();
    return;
  }

  if (type === 'steam') {
    await showSteamPicker();
    return;
  }

  const selected = await open({
    title: type === 'folder' ? 'Select folder' : 'Select file or program',
    directory: type === 'folder',
  });
  if (!selected) return;

  const item_type = type === 'folder' ? 'folder' : detectItemType(selected);

  let icon_data = null;
  try { icon_data = await invoke('get_file_icon', { path: selected }); } catch {}
  const newItem = { item_type, path: selected, value: null, icon_data };
  if (item_type === 'script') newItem.run_in_terminal = true;
  currentItems.push(newItem);

  renderItems();
}
```

Two behavioral notes for the engineer:
- The only `type` values that can reach this point are now `'file'` and `'folder'`
  (menu no longer emits `'app'`/`'script'`, and `winapp`/`url`/`steam` return early
  above). The unfiltered dialog (no `filters` option) is intentional — classification
  now happens *after* the user picks, not before.
- `item_type` shadows the function parameter name `type` deliberately to make the
  rest of the function (icon fetch, `newItem` construction, `run_in_terminal` check)
  read identically to how it did before, just driven by the detected type instead of
  the raw menu selection.

- [ ] **Step 2: Commit**

```bash
git add src/config.js
git commit -m "Auto-detect App/Script/File type from extension in unified add-item picker"
```

---

### Task 4: Manual verification of the full add-item flow

**Files:** none (manual QA against the running app)

> This project's frontend has no JS test runner configured (`package.json` has no
> `vitest`/`jest`/etc.), and adding one for a single pure helper would be overkill —
> verify by exercising the real UI, consistent with how UI changes are validated
> elsewhere in this project.

- [ ] **Step 1: Launch the dev build**

Run: `npm run tauri dev`
Expected: the widget and config window build and launch without errors in the terminal.

- [ ] **Step 2: Confirm the menu now has 5 rows**

Open the config window for a group, click "+ Add Item".
Expected: menu shows exactly — 📄 File / Program, 🪟 Windows Apps, 🌐 URL / Bookmark,
📁 Folder, 🎮 Steam Game (in that order). No "App / Executable" or "Script" rows.

- [ ] **Step 3: Add an `.exe` and confirm it becomes an App item**

Click "📄 File / Program", pick any `.exe` (e.g. `C:\Windows\System32\notepad.exe` —
actually `notepad.exe` is in `System32`, any locally installed app's `.exe` works).
Expected: new row appears with the 🖥️ app emoji/label treatment used for App items
(per `typeEmoji` map at `src/config.js:750`), and expanding it shows the
"🛡 Run as admin" checkbox (the App-only option from `buildExpandPanel`,
`src/config.js:677-691`) — confirming `item_type` was set to `'app'`.

- [ ] **Step 4: Add a `.ps1` or `.bat` and confirm it becomes a Script item**

Click "📄 File / Program" again, pick a `.bat`, `.cmd`, or `.ps1` file (create a throwaway
`test.bat` containing `@echo hello` if you don't have one handy).
Expected: new row shows the ⚡ script emoji/label, and expanding it shows the
"▶ Run via cmd" checkbox, checked by default (`buildExpandPanel`, `src/config.js:661-675`,
and `run_in_terminal: true` set in `addItem`) — confirming `item_type` was set to
`'script'`.

- [ ] **Step 5: Add a plain file and confirm it becomes a File item**

Click "📄 File / Program" again, pick a non-executable file (e.g. a `.txt`, `.pdf`, or
any document).
Expected: new row shows the 📄 file emoji/label, and expanding it shows **no**
type-specific options (no admin checkbox, no run-via-cmd toggle) — confirming
`item_type` was set to `'file'`.

- [ ] **Step 6: Confirm Folder still works**

Click "📁 Folder", pick any directory.
Expected: item is added with the 📁 folder emoji/label and `item_type: 'folder'`,
identical to pre-change behavior (the `directory: true` dialog still opens, and the
ternary routes folder picks straight to `item_type: 'folder'` without going through
`detectItemType`).

- [ ] **Step 7: Confirm Windows Apps, URL/Bookmark, and Steam Game are untouched**

Add one item of each remaining type via their menu rows.
Expected: each opens its existing picker (`showWinAppPicker`/`showUrlPicker`/
`showSteamPicker`) exactly as before — these code paths were not modified.

- [ ] **Step 8: Launch one item of each newly-classified type from the widget**

Save the group, then from the widget launch the App item, the Script item, and the
File item added in Steps 3–5.
Expected: the App item launches the program directly (and, if "Run as admin" is
checked, triggers the UAC prompt); the Script item opens in its own console window;
the File item opens in its associated default application (e.g. `.txt` → Notepad).
This confirms `launch_item` (`src-tauri/src/launcher.rs:525-684`) treats these
auto-classified items identically to manually-typed ones — no backend changes were
needed because `item_type` is the only thing that mattered.

---

## Self-review notes (for the plan author — already checked)

- **Spec coverage:** Menu collapse (Task 1), detection helper (Task 2), wiring +
  filter removal (Task 3), Folder/WinApp/URL/Steam untouched (Tasks 1 & 4 Steps 6-7),
  case-insensitive + fallback-to-file behavior (Task 2 Step 2, Task 4 Step 5) — all
  spec sections have a corresponding task.
- **Type consistency:** `item_type` values (`'app'`, `'script'`, `'file'`, `'folder'`)
  and the helper name `detectItemType` are used identically across Tasks 2–4.
- **No backend changes** is verified, not assumed — Task 4 Step 8 exercises the actual
  launch path for each auto-classified type.
