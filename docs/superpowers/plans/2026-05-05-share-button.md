# Share Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Share App Launcher" button to the Settings tab that copies the download URL to the clipboard and shows a 2-second green confirmation.

**Architecture:** Three small file changes — HTML adds the section markup, CSS adds two button states, JS wires the click handler inside the existing `initSettingsTab()` function. No Rust changes needed.

**Tech Stack:** Vanilla JS, HTML, CSS inside Tauri v2 WebView. `navigator.clipboard.writeText()` for clipboard access.

---

## Files

- Modify: `src/config.html` — add Share section markup inside `#tab-settings`
- Modify: `src/styles.css` — add `.btn-share` and `.btn-share--copied` styles
- Modify: `src/config.js:349-375` — add share click handler inside `initSettingsTab()`

---

### Task 1: Add Share section markup to config.html

**Files:**
- Modify: `src/config.html`

- [ ] **Step 1: Add the HTML**

  Open `src/config.html`. Find the closing `</div>` of the Config File `settings-section` (the one containing `export-btn` and `import-btn`). Add the new Share section immediately after it, before the closing `</div>` of `#tab-settings`:

  ```html
  <div class="settings-section">
    <p class="section-label">Share</p>
    <button class="btn btn-share" id="share-btn">📤 Share App Launcher</button>
  </div>
  ```

  The full `#tab-settings` block should now look like this:

  ```html
  <div id="tab-settings" style="display:none">
    <div class="settings-section">
      <p class="section-label">Global Hotkey</p>
      <div class="hotkey-row">
        <input type="text" class="hotkey-input" id="hotkey-input" placeholder="Ctrl+Alt+Space" autocomplete="off" />
        <button class="btn btn-save" id="hotkey-save-btn">Save</button>
      </div>
      <div class="hotkey-save-status" id="hotkey-save-status"></div>
    </div>
    <div class="settings-section">
      <p class="section-label">Config File</p>
      <div class="io-row">
        <button class="btn btn-cancel" id="export-btn">⬆ Export Config</button>
        <button class="btn btn-cancel" id="import-btn">⬇ Import Config</button>
      </div>
    </div>
    <div class="settings-section">
      <p class="section-label">Share</p>
      <button class="btn btn-share" id="share-btn">📤 Share App Launcher</button>
    </div>
  </div>
  ```

- [ ] **Step 2: Commit**

  ```bash
  git add src/config.html
  git commit -m "feat: add share button markup to settings tab"
  ```

---

### Task 2: Add button styles to styles.css

**Files:**
- Modify: `src/styles.css`

- [ ] **Step 1: Add the styles**

  Open `src/styles.css`. Find the `.feedback-btn` rule. Add the following block immediately after it:

  ```css
  .btn-share {
    width: 100%;
    background: #e07b39;
    color: #fff;
    border: none;
    border-radius: 5px;
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .btn-share:hover { background: #c96a2a; }
  .btn-share--copied {
    background: #1e3a1e;
    color: #6dbf6d;
    cursor: default;
  }
  .btn-share--copied:hover { background: #1e3a1e; }
  ```

- [ ] **Step 2: Commit**

  ```bash
  git add src/styles.css
  git commit -m "feat: add share button styles"
  ```

---

### Task 3: Wire up the click handler in config.js

**Files:**
- Modify: `src/config.js`

- [ ] **Step 1: Add the handler**

  Open `src/config.js`. Find `initSettingsTab()` (line ~349). Add the share handler at the end of the function, just before its closing `}`:

  ```js
  const shareBtn = document.getElementById('share-btn');
  shareBtn.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText('https://tonic-tech.com/app-launcher');
      shareBtn.textContent = '✓ Link copied!';
      shareBtn.classList.add('btn-share--copied');
      setTimeout(() => {
        shareBtn.textContent = '📤 Share App Launcher';
        shareBtn.classList.remove('btn-share--copied');
      }, 2000);
    } catch {
      // clipboard unavailable — fail silently
    }
  });
  ```

  The end of `initSettingsTab()` should now look like:

  ```js
  document.getElementById('import-btn').addEventListener('click', () =>
    invoke('import_config').catch(e => console.error('Import failed:', e))
  );

  const shareBtn = document.getElementById('share-btn');
  shareBtn.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText('https://tonic-tech.com/app-launcher');
      shareBtn.textContent = '✓ Link copied!';
      shareBtn.classList.add('btn-share--copied');
      setTimeout(() => {
        shareBtn.textContent = '📤 Share App Launcher';
        shareBtn.classList.remove('btn-share--copied');
      }, 2000);
    } catch {
      // clipboard unavailable — fail silently
    }
  });
}
  ```

- [ ] **Step 2: Commit**

  ```bash
  git add src/config.js
  git commit -m "feat: wire share button clipboard handler"
  ```

---

### Task 4: Manual test

**Files:** none

- [ ] **Step 1: Run the app**

  ```bash
  npm run tauri dev
  ```

- [ ] **Step 2: Open config and go to Settings tab**

  Right-click the widget → **⚙️ App Settings…** → click the **Settings** tab. Confirm the Share section appears at the bottom with the orange `📤 Share App Launcher` button.

- [ ] **Step 3: Click the button**

  Click it. Confirm:
  - Button turns green and shows `✓ Link copied!`
  - After 2 seconds it resets to orange `📤 Share App Launcher`

- [ ] **Step 4: Verify clipboard contents**

  Open Notepad (or any text field) and paste (`Ctrl+V`). Confirm the pasted text is exactly:
  ```
  https://tonic-tech.com/app-launcher
  ```

- [ ] **Step 5: Final commit if any fixes were needed, then done**

  ```bash
  git add -p
  git commit -m "fix: <describe what needed fixing>"
  ```
