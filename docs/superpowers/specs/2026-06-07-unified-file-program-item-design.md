# Unified "File / Program" item-menu entry — design

## Problem
The "Add Item" menu has separate rows for **App / Executable**, **File**, and **Script
(.bat / .ps1)**. All three are added the same way — pick a path via a native file
dialog — and the only real difference at add-time is what extension the user happens
to pick. Having three near-identical menu rows is unnecessary friction.

`Folder` is excluded from this merge: picking a folder requires switching the native
dialog into directory-browsing mode (`directory: true`), which on Windows is mutually
exclusive with file-picking/filtering in the same dialog call. It stays its own row.
`Windows Apps`, `URL / Bookmark`, and `Steam Game` use entirely different add-flows
(`showWinAppPicker`, `showUrlPicker`, `showSteamPicker`) and are unaffected.

## Design

### Menu (`config.html`)
Remove the **"🖥️ App / Executable"** (`data-type="app"`) and **"⚡ Script (.bat / .ps1)"**
(`data-type="script"`) rows. Rename the **"📄 File"** row (`data-type="file"`) to
**"📄 File / Program"**. Resulting menu, 7 rows → 5:

1. 📄 File / Program  (`data-type="file"`, merged)
2. 🪟 Windows Apps
3. 🌐 URL / Bookmark
4. 📁 Folder
5. 🎮 Steam Game

### Add-flow (`config.js` → `addItem`)
The merged entry opens the generic file dialog with **no extension filter** (same
picker behavior "File" already had — browse to any file, anywhere). Once a path is
selected, a new helper classifies it by extension:

```js
function detectItemType(path) {
  const ext = path.split('.').pop().toLowerCase();
  if (ext === 'exe') return 'app';
  if (['bat', 'cmd', 'ps1'].includes(ext)) return 'script';
  return 'file';
}
```

The detected `item_type` is then stored exactly as today (`app`/`script`/`file`),
including the existing `run_in_terminal: true` default applied when the result is
`script`. The old filter (`['exe','bat','ps1','cmd']`) used to restrict the App/Script
pickers is removed — the unified picker is intentionally extension-agnostic, since
classification now happens after selection.

Extension matching is case-insensitive (`MyApp.EXE` → `app`). Anything that isn't
`.exe`/`.bat`/`.cmd`/`.ps1` — text files, PDFs, images, `.lnk` shortcuts,
extension-less files, etc. — falls through to `file`, opened via the OS default
handler, identical to today's "File" behavior.

### Everything downstream is unchanged
No Rust/backend changes. `ItemType::App/File/Script` and their launch logic
(`launcher.rs:525-684`), the per-type expand-panel options ("Run as admin" for App,
"Run via cmd" for Script), icon fetching, and rendering all already key off
`item_type`, which this change does not alter. This is purely a change to *how the
type is chosen at add-time* — not how an item behaves once added.

## Out of scope
- No change to `Folder`, `Windows Apps`, `URL / Bookmark`, or `Steam Game` add-flows.
- No expansion of the App-detection set beyond `.exe` (e.g. `.com`, `.msi`) — kept to
  the current picker-filter mapping per user confirmation.
- No backend/data-model changes.
