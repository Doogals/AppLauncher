# Share Button — Design Spec
_2026-05-05_

## Summary

Add a "Share App Launcher" button to the Settings tab of the config window. When clicked, it copies the app's download page URL to the clipboard and briefly changes its appearance to confirm the action.

## Location

**Settings tab** (`config.html` → `#tab-settings`), as a new third section below the existing "Config File" section. Follows the same `settings-section` pattern as Hotkey and Config File.

## Behaviour

1. User opens config window → clicks **Settings** tab.
2. Sees a new **Share** section at the bottom with a single orange button: `📤 Share App Launcher`.
3. Clicking the button:
   - Calls `navigator.clipboard.writeText('https://tonic-tech.com/app-launcher')`
   - Button text changes to `✓ Link copied!` and turns green for **2 seconds**, then resets.
4. User pastes the link wherever they want (email, Discord, iMessage, etc.).

## Implementation

### `config.html`
Add a new `settings-section` div inside `#tab-settings`, after the Config File section:

```html
<div class="settings-section">
  <p class="section-label">Share</p>
  <button class="btn btn-share" id="share-btn">📤 Share App Launcher</button>
</div>
```

### `config.js`
Wire up the button in `initSettingsTab()`:

```js
const shareBtn = document.getElementById('share-btn');
shareBtn.addEventListener('click', async () => {
  await navigator.clipboard.writeText('https://tonic-tech.com/app-launcher');
  shareBtn.textContent = '✓ Link copied!';
  shareBtn.classList.add('btn-share--copied');
  setTimeout(() => {
    shareBtn.textContent = '📤 Share App Launcher';
    shareBtn.classList.remove('btn-share--copied');
  }, 2000);
});
```

### `styles.css`
Add button styles alongside the existing `.feedback-btn` styles:

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
  transition: background 0.15s;
}
.btn-share:hover { background: #c96a2a; }
.btn-share--copied {
  background: #1e3a1e;
  color: #6dbf6d;
  cursor: default;
}
```

## Fallback

If `navigator.clipboard.writeText()` fails (unlikely in Tauri WebView), catch the error silently — the button just doesn't change state. No crash, no error dialog. If this proves unreliable in testing, swap to `tauri-plugin-clipboard-manager`.

## Out of Scope

- No share analytics / tracking
- No email option
- No changes to the widget right-click menu or system tray
