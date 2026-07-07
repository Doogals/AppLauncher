import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const EMOJIS = [
  '💼','📁','🗂️','🖥️','🌐','📧','📅','📝','🔧','⚙️',
  '🚀','🎮','🎵','🎬','📷','💰','🏠','🏢','📚','🔬',
  '🧪','🛒','🤝','📊','📈','⚡','🔒','🛡️','📌','🔗',
  '💡','🎯','🧩','🐍','🦀','🌙','☀️','🔔','📣','🗺️',
  '🎨','🖊️','📦','🧰','🖱️',
];

function urlHostname(url) {
  try { return new URL(url).hostname.replace(/^www\./, ''); }
  catch { return url; }
}

const BROWSER_NAMES = {
  'chrome.exe': 'Chrome', 'msedge.exe': 'Edge', 'brave.exe': 'Brave',
  'firefox.exe': 'Firefox', 'opera.exe': 'Opera', 'operagx.exe': 'Opera GX',
  'vivaldi.exe': 'Vivaldi', 'arc.exe': 'Arc', 'thorium.exe': 'Thorium',
};

function browserDisplayName(item) {
  if (item.browser_name) return item.browser_name;
  if (!item.path) return 'Browser';
  const exe = item.path.replace(/.*[/\\]/, '').toLowerCase();
  return BROWSER_NAMES[exe] || exe.replace(/\.exe$/i, '');
}

// Inline SVG instead of an emoji glyph — the pencil emoji rendered tiny and
// oddly (looked like a needle) at this button size; an SVG stays crisp and
// always inherits the button's text color via currentColor.
const EDIT_ICON_SVG = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>';

// Same rationale as EDIT_ICON_SVG above — a plain "copy" glyph SVG instead of
// an emoji, for crisp rendering at this small button size.
const DUPLICATE_ICON_SVG = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';

// Terminal/console glyph for the "Edit Command Line" button.
const CMDLINE_ICON_SVG = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="vertical-align:-1px;margin-right:3px;"><rect x="2" y="4" width="20" height="16" rx="2"/><path d="M6 9l4 3-4 3M12 15h6"/></svg>';

// Deep-clones an item for the duplicate button, then resets its saved layout
// position/desktop targeting back to default (unset) — duplicates start
// fresh and the user repositions them via Edit Layout like any new item,
// instead of silently overlapping the original at the same saved spot.
// Async because a linked command file needs its own independent copy on the
// Rust side (see duplicate_command_file) — sharing the same path as-is would
// mean clearing/deleting either item's command breaks the other.
async function duplicateItem(item) {
  const clone = JSON.parse(JSON.stringify(item));
  clone.launch_desktop = null;
  clone.launch_x = null;
  clone.launch_y = null;
  clone.launch_width = null;
  clone.launch_height = null;
  clone.launch_virtual_desktop = null;
  clone.launch_desktop_index = null;
  if (clone.command_file_path) {
    try {
      const newPath = await invoke('duplicate_command_file', { path: clone.command_file_path });
      clone.command_file_path = newPath;
      if (newPath !== item.command_file_path) sessionCreatedCommandFiles.push(newPath);
    } catch (e) {
      console.error('duplicate_command_file failed:', e);
      clone.command_file_path = null;
    }
  }
  // Duplicate extra tab scripts the same way.
  if (clone.extra_tab_scripts && clone.extra_tab_scripts.length > 0) {
    const origExtras = item.extra_tab_scripts || [];
    clone.extra_tab_scripts = await Promise.all(clone.extra_tab_scripts.map(async (path, i) => {
      if (!path) return null;
      try {
        const newPath = await invoke('duplicate_command_file', { path });
        if (newPath !== origExtras[i]) sessionCreatedCommandFiles.push(newPath);
        return newPath;
      } catch {
        return null;
      }
    }));
  }
  clone._activeTab = 0; // reset tab selection on duplicate
  return clone;
}

// Universal fallback for items without a stored display_name (older saved
// items, or anything added via the plain File/Program/Folder dialog) — shows
// just the filename instead of the full absolute path.
function fallbackDisplayName(path) {
  if (!path) return '';
  const base = path.replace(/[/\\]+$/, '').replace(/.*[/\\]/, '');
  return base.replace(/\.(exe|lnk|bat|cmd|ps1|sh)$/i, '') || path;
}

// Used by the suggested-apps bar to route browsers through the URL/bookmark
// picker instead of adding them as a bare app launch.
function isBrowserPath(path) {
  if (!path) return false;
  const exe = path.replace(/.*[/\\]/, '').toLowerCase();
  return exe in BROWSER_NAMES;
}

// Gates the "Edit Command Line" button — mirrors the Rust-side
// terminal_shell_kind() in launcher.rs (cmd.exe / powershell.exe / pwsh.exe).
function isTerminalPath(path) {
  if (!path) return false;
  const exe = path.replace(/.*[/\\]/, '').toLowerCase();
  return exe === 'cmd.exe' || exe === 'powershell.exe' || exe === 'pwsh.exe';
}

// Opens the bookmark/URL picker directly for a known browser, skipping the
// "Select Browser" list step since we already know which one was clicked.
function openBrowserUrlPicker(app) {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `<div class="winapp-card"></div>`;
  document.body.appendChild(modal);
  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);
  showBookmarkStep(modal, { name: app.name, path: app.path }, closeModal, null, null);
}

function buildEmojiGrid() {
  const grid = document.getElementById('emoji-grid');
  EMOJIS.forEach(emoji => {
    const btn = document.createElement('button');
    btn.className = 'emoji-btn';
    btn.textContent = emoji;
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      document.getElementById('icon-input').value = emoji;
      grid.style.display = 'none';
    });
    grid.appendChild(btn);
  });
}

function initEmojiPicker() {
  buildEmojiGrid();

  document.getElementById('icon-input').addEventListener('click', (e) => {
    e.stopPropagation();
    const grid = document.getElementById('emoji-grid');
    const rect = e.target.getBoundingClientRect();
    const left = Math.min(rect.left, window.innerWidth - grid.offsetWidth - 8);
    grid.style.top = (rect.bottom + 4) + 'px';
    grid.style.left = left + 'px';
    grid.style.display = grid.style.display === 'none' ? 'grid' : 'none';
  });

  document.addEventListener('click', () => {
    document.getElementById('emoji-grid').style.display = 'none';
  });
}

const params = new URLSearchParams(window.location.search);
const groupId = params.get('id');

let currentItems = [];
let existingGroup = null;
let activeLayoutLabels = null; // set while layout editor is open

// Removes item at idx from currentItems, closes its layout editor window if
// the layout editor is open, and keeps activeLayoutLabels in sync so the
// remaining label→item mapping stays correct for save/cancel.
function removeItemAt(idx) {
  if (activeLayoutLabels) {
    const label = activeLayoutLabels[idx];
    if (label) {
      invoke('close_layout_windows', { labels: [label] }).catch(() => {});
      activeLayoutLabels.splice(idx, 1);
    }
  }
  currentItems.splice(idx, 1);
  renderItems();
}
// Paths created/imported via "Edit Command Line" during this editing
// session. If the window closes without saving, these are the only command
// files cleaned up — anything that already existed before this session
// opened is left alone regardless of what Clear did to it in memory, since
// the unchanged saved config may still legitimately reference it.
let sessionCreatedCommandFiles = [];

// A few extremely common nicknames don't appear as a literal substring of
// the app's real Start Menu name at all (e.g. "cmd" never appears in
// "Command Prompt" — c-o-m-m-a-n-d has no consecutive "cmd"), so a plain
// substring search can never find them by the name people actually type.
// Key is the nickname, value is a substring of the real name it should
// surface.
const APP_SEARCH_ALIASES = {
  cmd: 'command prompt',
};

function appMatchesSearch(nameLower, filterLower) {
  if (nameLower.includes(filterLower)) return true;
  return Object.entries(APP_SEARCH_ALIASES).some(([alias, targetSubstring]) =>
    nameLower.includes(targetSubstring) && (alias.includes(filterLower) || filterLower.includes(alias))
  );
}

async function showWinAppPicker() {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <input type="text" id="winapp-search" placeholder="Search apps..." autocomplete="off" />
        <button class="winapp-close" id="winapp-close">✕</button>
      </div>
      <div class="winapp-list" id="winapp-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => {
    document.removeEventListener('keydown', onKeyDown);
    modal.remove();
  };
  document.getElementById('winapp-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let apps;
  try {
    apps = await invoke('get_installed_apps');
  } catch (e) {
    apps = [];
    document.getElementById('winapp-list').innerHTML =
      '<div class="winapp-empty">Failed to load apps.</div>';
  }

  function renderApps(filter) {
    const list = document.getElementById('winapp-list');
    if (!list) return;
    const filterLower = filter.toLowerCase();
    const filtered = filter
      ? apps.filter(a => appMatchesSearch(a.name.toLowerCase(), filterLower))
      : apps;

    if (filtered.length === 0) {
      list.innerHTML = '<div class="winapp-empty">No apps found</div>';
      return;
    }

    list.innerHTML = '';
    filtered.forEach(app => {
      const row = document.createElement('div');
      row.className = 'winapp-row';
      row.textContent = app.name;
      row.addEventListener('click', async () => {
        let icon_data = null;
        try { icon_data = await invoke('get_file_icon', { path: app.path, args: app.args || '' }); } catch {}
        currentItems.push({ item_type: 'app', path: app.path, value: app.args || null, display_name: app.name, icon_data });
        renderItems();
        closeModal();
      });
      list.appendChild(row);
    });
  }

  renderApps('');
  const searchInput = document.getElementById('winapp-search');
  searchInput.addEventListener('input', (e) => renderApps(e.target.value));
  searchInput.focus();
}

// Suggested apps — a strip of icon-only chips inline next to the "Items"
// label. Shown by default, no menu required. Cross-references the curated
// well-known-app list against what's actually installed (see apps.rs).
let suggestedAppsCache = null;
let suggestionsRefreshStarted = false;

// Renders chips for whatever is currently in suggestedAppsCache,
// filtering out items the user has already added to the group.
function paintSuggestedBar() {
  const wrap = document.getElementById('suggested-wrap');
  const bar  = document.getElementById('suggested-bar');
  if (!bar || !wrap) return;

  const addedPaths = new Set(currentItems.map(i => (i.path || '').toLowerCase()));
  const remaining  = (suggestedAppsCache || []).filter(
    app => !addedPaths.has((app.path || '').toLowerCase())
  );

  if (remaining.length === 0) {
    wrap.style.display = 'none';
    bar.innerHTML = '';
    return;
  }

  wrap.style.display = 'flex';
  bar.innerHTML = '';

  remaining.forEach(app => {
    let chip;
    if (app.icon_data) {
      chip = document.createElement('img');
      chip.className = 'suggested-chip';
      chip.src = `data:image/png;base64,${app.icon_data}`;
    } else {
      chip = document.createElement('div');
      chip.className = 'suggested-chip-fallback';
      chip.textContent = '🖥️';
    }
    chip.title = `Add ${app.name}`;
    chip.addEventListener('click', () => {
      if (app.is_packaged) {
        currentItems.push({ item_type: 'uwp', path: app.path, value: null, display_name: app.name, icon_data: app.icon_data || null });
        renderItems();
      } else if (isBrowserPath(app.path)) {
        openBrowserUrlPicker(app);
      } else {
        currentItems.push({ item_type: 'app', path: app.path, value: app.args || null, display_name: app.name, icon_data: app.icon_data || null });
        renderItems();
      }
    });
    bar.appendChild(chip);
  });
}

async function renderSuggestedBar() {
  const wrap = document.getElementById('suggested-wrap');
  const bar  = document.getElementById('suggested-bar');
  if (!bar || !wrap) return;

  if (suggestedAppsCache === null) {
    // First call this session: load the cached list from config instantly.
    const config = await invoke('get_config');
    const saved  = config.cached_suggestions || [];

    if (saved.length > 0) {
      suggestedAppsCache = saved;
      paintSuggestedBar();
    } else {
      // No cache yet — show the spinner while the background scan runs.
      bar.innerHTML = '<div class="suggested-loading"><span class="suggested-spinner"></span>Finding suggestions…</div>';
    }
  } else {
    paintSuggestedBar();
  }

  // Kick off one background refresh per window session.
  if (!suggestionsRefreshStarted) {
    suggestionsRefreshStarted = true;
    refreshSuggestionsInBackground();
  }
}

// Runs in the background: scans for installed apps, diffs against the cached
// list, removes uninstalled apps, prepends newly-found ones, then saves.
// Never blocks the UI — called fire-and-forget from renderSuggestedBar.
async function refreshSuggestionsInBackground() {
  try {
    const fresh = await invoke('get_suggested_apps');

    // Fetch icons for any new items that don't have one yet.
    // Sequential on purpose — see the note on SHGetFileInfoW thread safety
    // from the original implementation above.
    for (const app of fresh) {
      if (app.icon_data) continue;
      try { app.icon_data = await invoke('get_file_icon', { path: app.path, args: app.args || '' }); } catch {}
    }

    const cached     = suggestedAppsCache || [];
    const freshPaths = new Set(fresh.map(a => (a.path || '').toLowerCase()));
    const cachedPaths = new Set(cached.map(a => (a.path || '').toLowerCase()));

    // Drop anything the user has since uninstalled.
    const surviving = cached.filter(a => freshPaths.has((a.path || '').toLowerCase()));

    // Newly-found apps go to the front of the list.
    const newItems = fresh.filter(a => !cachedPaths.has((a.path || '').toLowerCase()));

    const updated = [...newItems, ...surviving];

    const changed = newItems.length > 0 || surviving.length !== cached.length;
    if (changed) {
      suggestedAppsCache = updated;
      paintSuggestedBar();
    }

    // Always persist the verified list (even if nothing changed, first-run
    // populates it so the next open is instant).
    invoke('save_cached_suggestions', { suggestions: updated }).catch(() => {});
  } catch {
    // Scan failed — keep showing whatever was cached, no visible error.
  }
}

// "Edit Command Line" — Create generates a new app-managed script and opens
// it in the user's default editor; Link imports an existing file (used
// directly if it's already a matching .bat/.ps1/.cmd, copied in once
// otherwise). Either way, command_file_path ends up pointing at a directly
// launchable script — see launcher.rs's terminal_shell_kind handling.
function showCommandLinePicker({ item, idx, onCreated = null }) {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <span class="url-step-title">Edit Command Line</span>
        <button class="winapp-close" id="cmdline-close">✕</button>
      </div>
      <div class="winapp-list" id="cmdline-list">
        <div class="winapp-row" id="cmdline-create">Create Command</div>
        <div class="winapp-row" id="cmdline-link">Link Command</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  document.getElementById('cmdline-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  // Used as the generated script's filename instead of a random id, so it
  // reads as e.g. "Command Prompt.bat" in Notepad/Explorer rather than a
  // UUID. The Rust side de-dupes with " (2)", " (3)", etc. if another item
  // already used the same name.
  const label = item.display_name || fallbackDisplayName(item.path) || 'Command';

  document.getElementById('cmdline-create').addEventListener('click', async () => {
    closeModal();
    try {
      const path = await invoke('create_command_file', { shellPath: item.path, label });
      sessionCreatedCommandFiles.push(path);
      if (onCreated) {
        onCreated(path);
      } else {
        currentItems[idx].command_file_path = path;
        renderItems();
      }
    } catch (e) {
      console.error('create_command_file failed:', e);
    }
  });

  document.getElementById('cmdline-link').addEventListener('click', async () => {
    closeModal();
    try {
      const picked = await invoke('pick_command_file');
      if (!picked) return;
      const path = await invoke('import_linked_command_file', { pickedPath: picked, shellPath: item.path, label });
      if (path !== picked) {
        sessionCreatedCommandFiles.push(path);
      }
      if (onCreated) {
        onCreated(path);
      } else {
        currentItems[idx].command_file_path = path;
        renderItems();
      }
    } catch (e) {
      console.error('import_linked_command_file failed:', e);
    }
  });
}

async function showUrlPicker(editContext = null) {
  if (editContext) {
    const { item, idx } = editContext;
    const modal = document.createElement('div');
    modal.className = 'winapp-modal';
    modal.innerHTML = `<div class="winapp-card"></div>`;
    document.body.appendChild(modal);
    const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
    const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
    modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
    document.addEventListener('keydown', onKeyDown);
    const browser = { name: browserDisplayName(item), path: item.path || '' };
    await showBookmarkStep(modal, browser, closeModal, item, idx);
    return;
  }

  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <span class="url-step-title">Select Browser</span>
        <button class="winapp-close" id="url-close">✕</button>
      </div>
      <div class="winapp-list" id="url-browser-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  document.getElementById('url-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let browsers;
  try {
    browsers = await invoke('get_installed_browsers');
  } catch (e) {
    console.error('get_installed_browsers failed:', e);
    browsers = [];
    document.getElementById('url-browser-list').innerHTML =
      '<div class="winapp-empty">Could not detect browsers.</div>';
    return;
  }

  if (browsers.length === 0) {
    document.getElementById('url-browser-list').innerHTML =
      '<div class="winapp-empty">No supported browsers found.</div>';
    return;
  }

  const browserList = document.getElementById('url-browser-list');
  browserList.innerHTML = '';
  browsers.forEach(browser => {
    const row = document.createElement('div');
    row.className = 'winapp-row';
    row.textContent = browser.name;
    row.addEventListener('click', () => showBookmarkStep(modal, browser, closeModal, null, null));
    browserList.appendChild(row);
  });
}

async function showBookmarkStep(modal, browser, closeModal, existingItem = null, existingIdx = null) {
  const isEdit = existingItem !== null && existingIdx !== null;
  const existingUrls = isEdit
    ? (existingItem.urls?.length > 0 ? existingItem.urls : (existingItem.value ? [existingItem.value] : []))
    : [];

  const card = modal.querySelector('.winapp-card');
  const safeBrowserName = browser.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  card.innerHTML = `
    <div class="winapp-header">
      ${isEdit ? '' : '<button class="url-back-btn" id="url-back">←</button>'}
      <span class="url-step-title">${safeBrowserName} Bookmarks</span>
      <button class="winapp-close" id="url-close2">✕</button>
    </div>
    <div class="url-custom">
      <input type="text" id="bookmark-search" placeholder="Search bookmarks..." autocomplete="off" />
    </div>
    <div class="winapp-list" id="bookmark-list">
      <div class="winapp-empty">Loading bookmarks...</div>
    </div>
    <div class="url-entry">
      <input type="text" id="custom-url-input" placeholder="Or enter a custom URL: https://..." autocomplete="off" />
    </div>
    <div class="url-footer">
      ${isEdit ? '' : '<button class="btn btn-cancel" id="skip-url-btn">Just Open Browser (No URL)</button>'}
      <button class="btn btn-save" id="add-selected-btn" disabled>${isEdit ? 'Save' : 'Add Selected'}</button>
    </div>
  `;

  if (!isEdit) {
    document.getElementById('url-back').addEventListener('click', () => {
      closeModal();
      showUrlPicker();
    });
    document.getElementById('skip-url-btn').addEventListener('click', async () => {
      let icon_data = null;
      try { icon_data = await invoke('get_file_icon', { path: browser.path }); } catch {}
      currentItems.push({ item_type: 'app', path: browser.path, value: null, display_name: browser.name, icon_data });
      renderItems();
      closeModal();
    });
  }
  document.getElementById('url-close2').addEventListener('click', closeModal);

  const customInput = document.getElementById('custom-url-input');
  const addBtn = document.getElementById('add-selected-btn');

  function updateAddBtn() {
    const checkedCount = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none').length;
    const hasCustom = customInput.value.trim().length > 0;
    const total = checkedCount + (hasCustom ? 1 : 0);
    // A url-type item with zero URLs is invalid — it can't launch (there's
    // nothing to open) and used to silently break the whole group's launch.
    // Disabled at zero in both modes now, not just the "add" flow.
    addBtn.disabled = total === 0;
    if (isEdit) {
      addBtn.textContent = total > 0 ? `Save (${total} URL${total === 1 ? '' : 's'})` : 'Save';
    } else {
      addBtn.textContent = total > 0 ? `Add ${total} Selected` : 'Add Selected';
    }
  }

  let bookmarks;
  try {
    bookmarks = await invoke('get_browser_bookmarks', { browserPath: browser.path });
  } catch (e) {
    console.error('get_browser_bookmarks failed:', e);
    bookmarks = [];
  }

  const list = document.getElementById('bookmark-list');
  if (bookmarks.length === 0) {
    list.innerHTML = '<div class="winapp-empty">No bookmarks found.</div>';
  } else {
    list.innerHTML = '';
    bookmarks.forEach(bm => {
      const label = document.createElement('label');
      label.className = 'bookmark-row';
      const safeTitle = bm.title.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeUrl   = bm.url.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      label.innerHTML = `
        <input type="checkbox" class="bookmark-checkbox" />
        <div class="bookmark-info">
          <div class="bookmark-title">${safeTitle}</div>
          <div class="bookmark-url">${safeUrl}</div>
        </div>
      `;
      const cb = label.querySelector('.bookmark-checkbox');
      cb.dataset.url = bm.url;
      if (isEdit && existingUrls.includes(bm.url)) cb.checked = true;
      cb.addEventListener('change', updateAddBtn);
      list.appendChild(label);
    });
  }

  document.getElementById('bookmark-search').addEventListener('input', (e) => {
    const q = e.target.value.trim().toLowerCase();
    modal.querySelectorAll('.bookmark-row').forEach(row => {
      const title = row.querySelector('.bookmark-title')?.textContent.toLowerCase() || '';
      const url   = row.querySelector('.bookmark-url')?.textContent.toLowerCase() || '';
      row.style.display = (!q || title.includes(q) || url.includes(q)) ? '' : 'none';
    });
    updateAddBtn();
  });

  customInput.addEventListener('input', updateAddBtn);

  addBtn.addEventListener('click', async () => {
    const checked = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none');
    const urls = checked.map(cb => cb.dataset.url);
    const customUrl = customInput.value.trim();
    if (customUrl) urls.push(customUrl);

    if (urls.length === 0) return;

    let icon_data = null;
    try { icon_data = await invoke('get_file_icon', { path: browser.path }); } catch {}

    const newItem = {
      item_type: 'url',
      path: browser.path,
      browser_name: browser.name,
      urls,
      value: urls[0] || null,
      icon_data,
      launch_desktop: null,
      launch_x: null,
      launch_y: null,
      launch_width: null,
      launch_height: null,
    };

    if (isEdit) {
      newItem.launch_desktop = existingItem.launch_desktop ?? null;
      newItem.launch_x       = existingItem.launch_x ?? null;
      newItem.launch_y       = existingItem.launch_y ?? null;
      newItem.launch_width   = existingItem.launch_width ?? null;
      newItem.launch_height  = existingItem.launch_height ?? null;
      currentItems[existingIdx] = newItem;
    } else {
      currentItems.push(newItem);
    }

    renderItems();
    closeModal();
  });

  customInput.focus();
  updateAddBtn();
}

async function showSteamPicker() {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="winapp-card">
      <div class="winapp-header">
        <input type="text" id="steam-search" placeholder="Search games..." autocomplete="off" />
        <button class="winapp-close" id="steam-close">✕</button>
      </div>
      <div class="winapp-list" id="steam-list">
        <div class="winapp-empty">Loading...</div>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const onKeyDown = (e) => { if (e.key === 'Escape') closeModal(); };
  const closeModal = () => { document.removeEventListener('keydown', onKeyDown); modal.remove(); };
  document.getElementById('steam-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });
  document.addEventListener('keydown', onKeyDown);

  let games;
  try {
    games = await invoke('get_installed_steam_games');
  } catch (e) {
    games = [];
  }

  function renderGames(filter) {
    const list = document.getElementById('steam-list');
    if (!list) return;
    const filtered = filter
      ? games.filter(g => g.name.toLowerCase().includes(filter.toLowerCase()))
      : games;

    if (filtered.length === 0) {
      list.innerHTML = games.length === 0
        ? '<div class="winapp-empty">Steam not found or no games installed.</div>'
        : '<div class="winapp-empty">No games match your search.</div>';
      return;
    }

    list.innerHTML = '';
    filtered.forEach(game => {
      const row = document.createElement('div');
      row.className = 'winapp-row';
      row.style.display = 'flex';
      row.style.alignItems = 'center';
      row.style.gap = '8px';

      const iconEl = game.icon_data
        ? `<img src="data:image/jpeg;base64,${game.icon_data}" style="width:20px;height:20px;object-fit:contain;border-radius:3px;flex-shrink:0;" alt="" />`
        : `<span style="width:20px;text-align:center;flex-shrink:0;">🎮</span>`;

      const safeName = game.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      row.innerHTML = `${iconEl}<span>${safeName}</span>`;

      row.addEventListener('click', () => {
        if (!currentItems.some(i => i.item_type === 'steam' && i.value === game.appid)) {
          currentItems.push({
            item_type: 'steam',
            value: game.appid,
            path: game.name,
            icon_data: game.icon_data || null,
            launch_desktop: null,
            launch_x: null, launch_y: null, launch_width: null, launch_height: null,
          });
          renderItems();
        }
        closeModal();
      });
      list.appendChild(row);
    });
  }

  renderGames('');
  const searchInput = document.getElementById('steam-search');
  searchInput.addEventListener('input', (e) => renderGames(e.target.value));
  searchInput.focus();
}

async function showLayoutEditor() {
  if (currentItems.length === 0) return;
  if (activeLayoutLabels) return; // already open — prevent duplicate windows

  const total = currentItems.length;
  const layoutLabels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);
  const closeAll = () => invoke('close_layout_windows', { labels: layoutLabels });

  // Get the config window's physical rect to compute fallback positions on the
  // correct monitor. window.screen always reports the *primary* display in
  // WebView2 regardless of which monitor the config window is actually on.
  let configRect = null;
  try { configRect = await invoke('get_window_physical_rect', { label: 'config' }); } catch (_) {}
  // configRect = [x, y, w, h] in physical pixels. Stagger fallbacks 30px apart.
  const fallbackBaseX = configRect ? configRect[0] + 40 : 200;
  const fallbackBaseY = configRect ? configRect[1] + 40 : 200;

  for (let idx = 0; idx < total; idx++) {
    const item = currentItems[idx];
    const hasPos = item.launch_x != null && item.launch_y != null;

    const rawName = item.item_type === 'steam'
      ? (item.path || 'Steam Game')
      : item.item_type === 'url'
        ? browserDisplayName(item)
        : (item.path || item.value || 'Item');
    const safeName = encodeURIComponent(rawName);

    const vdParam = item.launch_virtual_desktop
      ? '&vd=' + encodeURIComponent(JSON.stringify(item.launch_virtual_desktop))
      : '';
    const label = `layout-item-${idx}`;

    // Physical pixel target for this window.
    const physX = hasPos ? item.launch_x : fallbackBaseX + idx * 30;
    const physY = hasPos ? item.launch_y : fallbackBaseY + idx * 30;
    const physW = (hasPos && item.launch_width) ? item.launch_width : 800;
    const physH = (hasPos && item.launch_height) ? item.launch_height : 600;

    // Create the window hidden at a dummy position. We always use
    // set_layout_window_physics (physical pixels) to place it correctly —
    // this avoids per-monitor DPR ambiguity from window.devicePixelRatio,
    // which reflects the config window's monitor, not the item's monitor.
    const win = new WebviewWindow(label, {
      url: `layout-item.html?idx=${idx}&name=${safeName}&total=${total}${vdParam}`,
      title: rawName,
      // Start tiny and hidden — set_layout_window_physics atomically positions,
      // resizes, and shows via a single Win32 SetWindowPos(SWP_SHOWWINDOW) call,
      // so there is no intermediate flash at the wrong size.
      x: 0, y: 0, width: 1, height: 1,
      visible: false,
      resizable: true,
      decorations: true,
      alwaysOnTop: true,
    });

    // Once created: atomically move to exact physical position + show.
    win.once('tauri://created', () => {
      invoke('set_layout_window_physics', { label, x: physX, y: physY, width: physW, height: physH })
        .catch((e) => console.error('[layout] set_layout_window_physics failed for', label, e));
    });
    win.once('tauri://error', (e) => {
      console.error('[layout] tauri://error (creation FAILED) for', label, e);
    });
  }

  activeLayoutLabels = layoutLabels;

  const unlistenSave = await listen('layout-save', ({ payload }) => {
    const { positions, virtual_desktops, virtual_desktop_indices } = payload;
    positions.forEach(([x, y, w, h], i) => {
      if (i < currentItems.length && w > 0 && h > 0) {
        currentItems[i].launch_x = x;
        currentItems[i].launch_y = y;
        currentItems[i].launch_width = w;
        currentItems[i].launch_height = h;
        currentItems[i].launch_virtual_desktop = virtual_desktops?.[i] ?? null;
        // Fallback for when the desktop's GUID stops matching at launch time
        // (virtual desktop GUIDs aren't permanently stable across reboots).
        currentItems[i].launch_desktop_index = virtual_desktop_indices?.[i] ?? null;
      }
    });
    activeLayoutLabels = null;
    unlistenSave();
    unlistenCancel();
    renderItems();
  });

  const unlistenCancel = await listen('layout-cancel', () => {
    activeLayoutLabels = null;
    unlistenSave();
    unlistenCancel();
  });
}

async function fitWindow() {
  await new Promise(resolve => requestAnimationFrame(resolve));
  const h = document.querySelector('.config-window').offsetHeight;
  await getCurrentWindow().setSize(new LogicalSize(420, h));
}

async function init() {
  // Single close handler — covers OS X button and Alt+F4.
  // Save/Cancel buttons use closeConfigWindow() directly instead of close().
  getCurrentWindow().onCloseRequested(async (event) => {
    event.preventDefault();
    await closeConfigWindow();
  });

  initTabs();
  await initSettingsTab();
  initEmojiPicker();

  document.querySelector('.license-details').addEventListener('toggle', fitWindow);

  if (groupId) {
    const config = await invoke('get_config');
    existingGroup = config.groups.find(g => g.id === groupId);
    if (existingGroup) {
      document.getElementById('icon-input').value = existingGroup.icon;
      document.getElementById('name-input').value = existingGroup.name;
      currentItems = [...existingGroup.items];
    }
  }

  // Only an already-saved group can have its color set — a brand new group
  // doesn't exist in the backend's config yet, so there's nothing for
  // open_group_color_window to find until after the first Save & Close.
  const colorBtn = document.getElementById('group-color-btn');
  if (existingGroup) {
    colorBtn.disabled = false;
    colorBtn.title = 'Group Color';
    colorBtn.addEventListener('click', () => {
      invoke('open_group_color_window', { groupId: existingGroup.id })
        .catch(err => console.error('open_group_color_window error:', err));
    });
  }

  renderItems();
  await renderLicenseSection();

  // Silent background validation — check if license is still valid on LS
  invoke('check_license_status').then(status => {
    if (status === 'revoked') {
      const summary = document.getElementById('license-summary');
      if (summary) summary.textContent = '⚠ License Revoked';
      const content = document.getElementById('license-content');
      if (content) content.innerHTML = `
        <p style="font-size:0.78rem; color:#e94560; margin-top:6px;">
          Your license has been revoked. Please contact support.
        </p>
      `;
      fitWindow();
    }
  }).catch(() => {}); // Unreachable = offline, ignore silently
}

function switchTab(name) {
  document.querySelectorAll('.tab').forEach(t =>
    t.classList.toggle('active', t.dataset.tab === name)
  );
  document.getElementById('tab-group').style.display = name === 'group' ? '' : 'none';
  document.getElementById('tab-settings').style.display = name === 'settings' ? '' : 'none';
}

function initTabs() {
  const initialTab = new URLSearchParams(window.location.search).get('tab') || 'group';
  switchTab(initialTab);
  document.querySelectorAll('.tab').forEach(tab => {
    tab.addEventListener('click', () => { switchTab(tab.dataset.tab); fitWindow(); });
  });
}

async function initSettingsTab() {
  const config = await invoke('get_config');
  document.getElementById('hotkey-input').value = config.hotkey || 'Ctrl+Alt+Space';

  // Widget color picking moved to its own window (same tabbed picker the
  // group "Change Color" button uses, in "widget" mode) — keeps this
  // Settings tab from getting cluttered with a 20-swatch grid inline.
  document.getElementById('widget-color-btn').addEventListener('click', () => {
    invoke('open_widget_color_window').catch(err => console.error('open_widget_color_window error:', err));
  });

  initHotkeyRecorder();

  document.getElementById('hotkey-save-btn').addEventListener('click', async () => {
    const hotkey = document.getElementById('hotkey-input').value.trim();
    if (!hotkey) return;
    const statusEl = document.getElementById('hotkey-save-status');
    try {
      await invoke('set_hotkey', { hotkey });
      statusEl.style.color = '#4caf50';
      statusEl.textContent = 'Saved ✓';
      setTimeout(() => { statusEl.textContent = ''; }, 2000);
    } catch (e) {
      statusEl.style.color = '#e94560';
      statusEl.textContent = typeof e === 'string' ? e : 'Failed to save.';
    }
  });

  document.getElementById('export-btn').addEventListener('click', () =>
    invoke('export_config').catch(e => console.error('Export failed:', e))
  );

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
        shareBtn.textContent = '📤 Share TakeOff';
        shareBtn.classList.remove('btn-share--copied');
      }, 2000);
    } catch {
      // clipboard unavailable — fail silently
    }
  });
}

// Keys that only count as modifiers — keep listening until a "real" key
// arrives. Matches the global-hotkey crate's accepted modifier names.
const HOTKEY_MODIFIER_CODES = new Set([
  'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight',
  'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight',
]);

let hotkeyRecording = false;

function initHotkeyRecorder() {
  const recordBtn = document.getElementById('hotkey-record-btn');
  if (!recordBtn) return;

  recordBtn.addEventListener('click', () => {
    if (hotkeyRecording) return;
    hotkeyRecording = true;

    const input = document.getElementById('hotkey-input');
    const statusEl = document.getElementById('hotkey-save-status');
    const previousValue = input.value;

    input.value = 'Press a key combo…';
    input.disabled = true;
    recordBtn.textContent = 'Listening… (Esc to cancel)';
    recordBtn.disabled = true;
    statusEl.style.color = '#888';
    statusEl.textContent = '';

    const stop = () => {
      hotkeyRecording = false;
      document.removeEventListener('keydown', onKeyDown, true);
      input.disabled = false;
      recordBtn.textContent = '⌨ Record';
      recordBtn.disabled = false;
    };

    const onKeyDown = (e) => {
      e.preventDefault();
      e.stopPropagation();

      const hasModifier = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;

      // Bare Escape (no modifiers held) cancels — Ctrl+Escape etc. still works as a combo
      if (e.code === 'Escape' && !hasModifier) {
        input.value = previousValue;
        stop();
        return;
      }

      // Still just a modifier being held down — keep waiting for the real key
      if (HOTKEY_MODIFIER_CODES.has(e.code)) return;

      // Require at least one modifier so we don't register a bare key globally
      if (!hasModifier) {
        statusEl.style.color = '#e94560';
        statusEl.textContent = 'Hold Ctrl, Alt, Shift, or Win plus a key';
        return;
      }

      // e.code already matches the format the global-hotkey crate expects
      // (KeyN, Digit5, Space, ArrowUp, F5, etc.) — no translation needed.
      const parts = [];
      if (e.ctrlKey) parts.push('Ctrl');
      if (e.altKey) parts.push('Alt');
      if (e.shiftKey) parts.push('Shift');
      if (e.metaKey) parts.push('Super');
      parts.push(e.code);

      input.value = parts.join('+');
      statusEl.style.color = '#4caf50';
      statusEl.textContent = 'Recorded — click Save to apply';
      stop();
    };

    document.addEventListener('keydown', onKeyDown, true);
  });
}

function buildExpandPanel(item, idx) {
  const panel = document.createElement('div');
  panel.className = 'item-expand';

  if (item.item_type === 'steam') {
    // Steam items: monitor dropdown only
    const monRow = document.createElement('div');
    monRow.className = 'item-expand-row';
    monRow.innerHTML = `
      <span>Launch on screen</span>
      <select class="steam-monitor-sel" style="flex:1;max-width:180px;background:#1e1e3e;border:1px solid #3a3a6a;border-radius:4px;color:#c8c8d8;font-size:11px;padding:3px 6px;cursor:pointer;">
        <option value="">Any screen (default)</option>
      </select>
    `;
    const sel = monRow.querySelector('.steam-monitor-sel');
    invoke('get_monitors').then(monitors => {
      monitors.forEach(m => {
        const opt = document.createElement('option');
        opt.value = String(m.index);
        opt.textContent = m.is_primary
          ? `Primary (${m.width}×${m.height})`
          : `${m.name} (${m.width}×${m.height})`;
        if (item.launch_desktop !== null && item.launch_desktop !== undefined && item.launch_desktop === m.index) {
          opt.selected = true;
        }
        sel.appendChild(opt);
      });
    }).catch(() => {});
    sel.addEventListener('change', e => {
      currentItems[idx].launch_desktop = e.target.value === '' ? null : parseInt(e.target.value, 10);
    });
    panel.appendChild(monRow);
    return panel;
  }

  // All non-Steam items: optional clear row + type-specific options
  const hasPos = item.launch_x != null && item.launch_y != null;
  if (hasPos) {
    const posRow = document.createElement('div');
    posRow.className = 'item-expand-row';
    posRow.innerHTML = `
      <span style="color:#888;font-size:11px;">Position saved</span>
      <button class="coord-clear" style="background:none;border:none;color:#555;font-size:11px;cursor:pointer;padding:0 4px;" title="Clear">✕ Clear</button>
    `;
    posRow.querySelector('.coord-clear').addEventListener('click', () => {
      currentItems[idx].launch_x = null;
      currentItems[idx].launch_y = null;
      currentItems[idx].launch_width = null;
      currentItems[idx].launch_height = null;
      renderItems();
    });
    panel.appendChild(posRow);
  }

  if (item.item_type === 'script') {
    const runRow = document.createElement('div');
    runRow.className = 'item-expand-row';
    const checked = item.run_in_terminal !== false ? 'checked' : '';
    runRow.innerHTML = `
      <label class="run-toggle">
        <input type="checkbox" class="run-checkbox" ${checked} />
        &#x25B6; Run via cmd
      </label>
    `;
    runRow.querySelector('.run-checkbox').addEventListener('change', (e) => {
      currentItems[idx].run_in_terminal = e.target.checked;
    });
    panel.appendChild(runRow);
  }

  if (item.item_type === 'app') {
    const adminRow = document.createElement('div');
    adminRow.className = 'item-expand-row';
    const checked = item.run_as_admin ? 'checked' : '';
    adminRow.innerHTML = `
      <label class="run-toggle">
        <input type="checkbox" class="admin-checkbox" ${checked} />
        🛡 Run as admin
      </label>
    `;
    adminRow.querySelector('.admin-checkbox').addEventListener('change', (e) => {
      currentItems[idx].run_as_admin = e.target.checked;
    });
    panel.appendChild(adminRow);
  }

  if (item.item_type === 'app' && isTerminalPath(item.path)) {
    const tabCount  = item.tab_count  || 1;
    const activeTab = item._activeTab || 0; // 0-indexed, not persisted

    // ── Tab count row ─────────────────────────────────────────────────────────
    const tabCountRow = document.createElement('div');
    tabCountRow.className = 'item-expand-row';
    tabCountRow.style.justifyContent = 'space-between';
    tabCountRow.innerHTML = `
      <span style="color:#888;font-size:11px;">Tabs</span>
      <div style="display:flex;align-items:center;gap:4px;">
        <button class="tab-stepper" id="tab-dec">−</button>
        <span class="tab-count-display">${tabCount}</span>
        <button class="tab-stepper" id="tab-inc">+</button>
      </div>
    `;
    tabCountRow.querySelector('#tab-dec').addEventListener('click', () => {
      const cur = currentItems[idx].tab_count || 1;
      if (cur <= 1) return;
      currentItems[idx].tab_count = cur - 1;
      // Trim extra_tab_scripts to the new size
      if (currentItems[idx].extra_tab_scripts) {
        currentItems[idx].extra_tab_scripts = currentItems[idx].extra_tab_scripts.slice(0, cur - 2);
      }
      // Clamp active tab
      if ((currentItems[idx]._activeTab || 0) >= currentItems[idx].tab_count) {
        currentItems[idx]._activeTab = currentItems[idx].tab_count - 1;
      }
      renderItems();
    });
    tabCountRow.querySelector('#tab-inc').addEventListener('click', () => {
      const cur = currentItems[idx].tab_count || 1;
      if (cur >= 8) return;
      currentItems[idx].tab_count = cur + 1;
      renderItems();
    });
    panel.appendChild(tabCountRow);

    // ── Tab selector pills (only when tab_count > 1) ──────────────────────────
    if (tabCount > 1) {
      const pillRow = document.createElement('div');
      pillRow.className = 'item-expand-row tab-pill-row';
      for (let t = 0; t < tabCount; t++) {
        const pill = document.createElement('button');
        pill.textContent = `Tab ${t + 1}`;
        pill.className = 'tab-pill' + (t === activeTab ? ' active' : '');
        pill.addEventListener('click', () => {
          currentItems[idx]._activeTab = t;
          renderItems();
        });
        pillRow.appendChild(pill);
      }
      panel.appendChild(pillRow);
    }

    // ── Edit Command Line row (tab-aware) ─────────────────────────────────────
    // Helpers to read/write the script for the currently-active tab.
    const getScript = () => {
      if (activeTab === 0) return currentItems[idx].command_file_path || null;
      const extras = currentItems[idx].extra_tab_scripts || [];
      return extras[activeTab - 1] || null;
    };
    const setScript = (path) => {
      if (activeTab === 0) {
        currentItems[idx].command_file_path = path;
      } else {
        if (!currentItems[idx].extra_tab_scripts) currentItems[idx].extra_tab_scripts = [];
        while (currentItems[idx].extra_tab_scripts.length < activeTab) {
          currentItems[idx].extra_tab_scripts.push(null);
        }
        currentItems[idx].extra_tab_scripts[activeTab - 1] = path;
      }
    };

    const hasCmd   = !!getScript();
    const tabLabel = tabCount > 1 ? ` (Tab ${activeTab + 1})` : '';

    const cmdRow = document.createElement('div');
    cmdRow.className = 'item-expand-row';
    cmdRow.style.justifyContent = 'space-between';
    cmdRow.innerHTML = `
      <span style="color:#888;font-size:11px;">${hasCmd ? `Command attached${tabLabel}` : ''}</span>
      <div style="display:flex;align-items:center;gap:6px;">
        ${hasCmd ? '<button class="cmdline-clear" style="background:none;border:none;color:#555;font-size:11px;cursor:pointer;padding:0 4px;" title="Clear">✕ Clear</button>' : ''}
        <button class="pick-btn cmdline-edit-btn">${CMDLINE_ICON_SVG}Edit Command Line</button>
      </div>
    `;
    cmdRow.querySelector('.cmdline-edit-btn').addEventListener('click', () => {
      const current = getScript();
      if (current) {
        invoke('open_command_file', { path: current }).catch(err => console.error('open_command_file error:', err));
      } else {
        // Pass a modified item snapshot so the picker uses the right label.
        showCommandLinePicker({
          item: currentItems[idx],
          idx,
          onCreated: (path) => { setScript(path); renderItems(); },
        });
      }
    });
    if (hasCmd) {
      cmdRow.querySelector('.cmdline-clear').addEventListener('click', () => {
        setScript(null);
        renderItems();
      });
    }
    panel.appendChild(cmdRow);
  }

  return panel;
}

function renderItems() {
  renderSuggestedBar();

  const list = document.getElementById('items-list');
  list.innerHTML = '';

  if (currentItems.length === 0) {
    list.style.overflowY = 'hidden';
    const empty = document.createElement('div');
    empty.style.cssText = 'color:#4a5568;font-size:0.8rem;text-align:center;padding:12px 0;';
    empty.textContent = 'No items yet';
    list.appendChild(empty);
    return;
  }

  list.style.overflowY = 'auto';

  currentItems.forEach((item, idx) => {
    const wrapper = document.createElement('div');

    const row = document.createElement('div');
    row.className = 'item-row';

    if (item.item_type === 'url') {
      const allUrls = (item.urls && item.urls.length > 0) ? item.urls : (item.value ? [item.value] : []);
      const count = allUrls.length;
      const name = browserDisplayName(item);
      const label = `${name} (${count} URL${count === 1 ? '' : 's'})`;
      const hostnames = allUrls.slice(0, 2).map(urlHostname);
      const subtitle = hostnames.join(', ') + (allUrls.length > 2 ? ` +${allUrls.length - 2}` : '');

      const iconHtml = item.icon_data
        ? `<img src="data:image/png;base64,${item.icon_data}" style="width:20px;height:20px;object-fit:contain;flex-shrink:0;" alt="" />`
        : '<span style="flex-shrink:0;">🌐</span>';

      const safeLabel    = label.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeSubtitle = subtitle.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

      row.innerHTML = `
        ${iconHtml}
        <div class="item-label-multi" title="${safeSubtitle}" style="flex:1;min-width:0;overflow:hidden;">
          <div class="item-label">${safeLabel}</div>
          <div style="font-size:10px;color:#888;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${safeSubtitle}</div>
        </div>
        <button class="edit-url-btn" title="Edit URLs">${EDIT_ICON_SVG}</button>
        <button class="duplicate-btn" title="Duplicate">${DUPLICATE_ICON_SVG}</button>
        <button class="remove-btn">✕</button>
      `;

      row.querySelector('.edit-url-btn').onclick = () => showUrlPicker({ item, idx });
      row.querySelector('.duplicate-btn').onclick = async () => {
        const clone = await duplicateItem(item);
        currentItems.splice(idx + 1, 0, clone);
        renderItems();
      };
      row.querySelector('.remove-btn').onclick = () => removeItemAt(idx);

    } else if (item.item_type === 'steam') {
      const gameName = item.path || 'Unknown Game';
      const safeLabel = gameName.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const iconHtml = item.icon_data
        ? `<img src="data:image/jpeg;base64,${item.icon_data}" style="width:20px;height:20px;object-fit:contain;flex-shrink:0;border-radius:2px;" alt="" />`
        : '<span style="flex-shrink:0;">🎮</span>';
      row.innerHTML = `
        ${iconHtml}
        <span class="item-label" title="${safeLabel}">${safeLabel} <span style="color:#1b9fdb;font-size:10px;font-weight:400;">Steam</span></span>
        <button class="duplicate-btn" title="Duplicate">${DUPLICATE_ICON_SVG}</button>
        <button class="remove-btn">✕</button>
      `;
      row.querySelector('.duplicate-btn').onclick = async () => {
        const clone = await duplicateItem(item);
        currentItems.splice(idx + 1, 0, clone);
        renderItems();
      };
      row.querySelector('.remove-btn').onclick = () => removeItemAt(idx);

    } else {
      // Prefer a curated display_name (set when adding via Windows Apps,
      // Suggested Items, or a browser); otherwise fall back to just the
      // filename rather than showing the whole absolute path. The full path
      // still shows up as a hover tooltip either way.
      const rawLabel = item.display_name || fallbackDisplayName(item.path) || item.path || '';
      const rawTitle = item.path || rawLabel;
      const safeLabel = rawLabel.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeTitle = rawTitle.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const typeEmoji = { app: '🖥️', file: '📄', folder: '📁', script: '⚡', uwp: '🪟' }[item.item_type] || '•';
      const iconHtml = item.icon_data
        ? `<img src="data:image/png;base64,${item.icon_data}" style="width:20px;height:20px;object-fit:contain;flex-shrink:0;" alt="" />`
        : `<span style="flex-shrink:0;">${typeEmoji}</span>`;
      // Browsers added bare (no URL yet, e.g. via "Just Open Browser") can
      // still get URLs added later — same edit button the url-type rows use.
      const isBareBrowser = item.item_type === 'app' && isBrowserPath(item.path);
      const editBtnHtml = isBareBrowser ? `<button class="edit-url-btn" title="Add URLs">${EDIT_ICON_SVG}</button>` : '';
      row.innerHTML = `
        ${iconHtml}
        <span class="item-label" title="${safeTitle}">${safeLabel}</span>
        ${editBtnHtml}
        <button class="duplicate-btn" title="Duplicate">${DUPLICATE_ICON_SVG}</button>
        <button class="remove-btn">✕</button>
      `;
      if (isBareBrowser) {
        row.querySelector('.edit-url-btn').onclick = () => showUrlPicker({ item, idx });
      }
      row.querySelector('.duplicate-btn').onclick = async () => {
        const clone = await duplicateItem(item);
        currentItems.splice(idx + 1, 0, clone);
        renderItems();
      };
      row.querySelector('.remove-btn').onclick = () => removeItemAt(idx);
    }

    row.setAttribute('draggable', 'true');
    row.dataset.index = idx;
    row.addEventListener('dragstart', e => e.dataTransfer.setData('text/plain', idx));
    row.addEventListener('dragover', e => { e.preventDefault(); row.style.opacity = '0.5'; });
    row.addEventListener('dragleave', () => { row.style.opacity = '1'; });
    row.addEventListener('drop', e => {
      e.preventDefault();
      row.style.opacity = '1';
      const fromIdx = parseInt(e.dataTransfer.getData('text/plain'));
      if (fromIdx !== idx) {
        const [moved] = currentItems.splice(fromIdx, 1);
        currentItems.splice(idx, 0, moved);
        renderItems();
      }
    });

    wrapper.appendChild(row);
    wrapper.appendChild(buildExpandPanel(item, idx));
    list.appendChild(wrapper);
  });

  fitWindow();
}

document.getElementById('add-item-btn').onclick = () => {
  const menu = document.getElementById('add-type-menu');
  menu.style.display = menu.style.display === 'none' ? 'block' : 'none';
  fitWindow();
};

document.querySelectorAll('[data-type]').forEach(el => {
  el.addEventListener('click', () => addItem(el.dataset.type));
});

function detectItemType(path) {
  const ext = path.split('.').pop().toLowerCase();
  if (ext === 'exe') return 'app';
  if (['bat', 'cmd', 'ps1'].includes(ext)) return 'script';
  return 'file';
}

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

async function closeConfigWindow(saved = false) {
  // Anything created/imported via "Edit Command Line" this session that
  // didn't end up persisted gets cleaned up here — save_group's own
  // old-vs-new diff (see lib.rs) only catches items that existed in the
  // PREVIOUS saved config, so it can't see a file whose item was created
  // and then removed again within this same session before saving. Files
  // that already existed before this session opened are never in this
  // list, so they're untouched regardless of what Clear did to them in
  // memory — kept here covers exactly what's true after this close:
  // nothing from an unsaved session, or whatever's left in currentItems
  // for a saved one.
  const kept = saved
    ? new Set([
        ...currentItems.map(i => i.command_file_path).filter(Boolean),
        ...currentItems.flatMap(i => (i.extra_tab_scripts || []).filter(Boolean)),
      ])
    : new Set();
  for (const path of sessionCreatedCommandFiles) {
    if (!kept.has(path)) {
      try { await invoke('clear_command_file', { path }); } catch {}
    }
  }
  if (activeLayoutLabels) {
    // complete_layout_cancel (not the bare close_layout_windows) — it also
    // clears the transient LayoutDesktops map on the Rust side. Skipping that
    // left stale virtual-desktop assignments behind for the next layout
    // session that happens to reuse the same window labels (layout-item-0,
    // layout-item-1, ...), which is what caused launches to misbehave after
    // someone abandoned an Edit Layout session without saving or cancelling.
    try { await invoke('complete_layout_cancel', { labels: activeLayoutLabels }); } catch {}
    activeLayoutLabels = null;
  }
  await getCurrentWindow().destroy();
}

// Shown only when Save & Close is hit while an Edit Layout session is still
// open (never went through that session's own Save All Positions / Cancel).
// No backdrop-click or Escape dismissal — this needs an explicit choice so
// position data isn't silently lost.
function confirmLayoutPrompt() {
  return new Promise((resolve) => {
    const modal = document.createElement('div');
    modal.className = 'winapp-modal';
    modal.innerHTML = `
      <div class="winapp-card" style="width:300px;padding:18px;">
        <p style="font-size:13px;font-weight:600;margin-bottom:8px;">Save window positions?</p>
        <p style="font-size:12px;color:#aaa;margin-bottom:16px;">
          Edit Layout is still open and you never hit Save All Positions.
          Save those window positions before closing?
        </p>
        <div style="display:flex;gap:8px;">
          <button class="btn btn-cancel" id="layout-prompt-no" style="flex:1;">Don't Save</button>
          <button class="btn btn-save" id="layout-prompt-yes" style="flex:1;">Save Positions</button>
        </div>
      </div>
    `;
    document.body.appendChild(modal);
    const cleanup = (result) => { modal.remove(); resolve(result); };
    document.getElementById('layout-prompt-yes').addEventListener('click', () => cleanup(true));
    document.getElementById('layout-prompt-no').addEventListener('click', () => cleanup(false));
  });
}

// Runs the same Rust-side logic as the layout editor's own Save All
// Positions / Cancel buttons, and waits for the resulting event so
// currentItems is fully updated (save case) before we read it.
async function resolveLayoutSession(shouldSave) {
  if (!activeLayoutLabels) return;
  const labels = activeLayoutLabels;
  const eventName = shouldSave ? 'layout-save' : 'layout-cancel';
  let resolveFn;
  const settled = new Promise((resolve) => { resolveFn = resolve; });
  const unlisten = await listen(eventName, () => resolveFn());
  try {
    await invoke(shouldSave ? 'complete_layout_save' : 'complete_layout_cancel', { labels });
    await settled;
  } finally {
    unlisten();
  }
}

document.getElementById('save-btn').onclick = async () => {
  const name = document.getElementById('name-input').value.trim();
  const icon = document.getElementById('icon-input').value.trim() || '📁';
  if (!name) { alert('Please enter a group name.'); return; }

  if (activeLayoutLabels && activeLayoutLabels.length > 0) {
    const shouldSave = await confirmLayoutPrompt();
    await resolveLayoutSession(shouldSave);
  }

  const group = {
    id: existingGroup?.id ?? crypto.randomUUID(),
    name,
    icon,
    items: currentItems,
  };

  try {
    await invoke('save_group', { group });
    existingGroup = group; // prevent new UUID on double-click
    await closeConfigWindow(true);
  } catch (e) {
    showErrorModal(String(e));
  }
};

document.getElementById('cancel-btn').onclick = () => closeConfigWindow();

document.getElementById('layout-btn').onclick = () => showLayoutEditor();

// Store URL — update after creating your LemonSqueezy product
const STORE_URL = 'https://tonictechapps.lemonsqueezy.com/checkout/buy/692bf539-a89a-4ff8-9da7-5c93507c21af';

function showErrorModal(msg) {
  const backdrop = document.getElementById('error-modal-backdrop');
  const msgEl    = document.getElementById('error-modal-msg');
  const buyBtn   = document.getElementById('error-modal-buy');
  const okBtn    = document.getElementById('error-modal-ok');

  msgEl.textContent = msg;
  const isFreeTier = msg.includes('Free tier');
  buyBtn.style.display = isFreeTier ? '' : 'none';

  backdrop.classList.add('visible');

  const close = () => backdrop.classList.remove('visible');
  okBtn.onclick  = close;
  buyBtn.onclick = () => { invoke('open_url', { url: STORE_URL }).catch(() => {}); close(); };
  backdrop.onclick = (e) => { if (e.target === backdrop) close(); };
}

async function renderLicenseSection() {
  const config = await invoke('get_config');
  const content = document.getElementById('license-content');
  const summary = document.getElementById('license-summary');
  if (!content || !summary) return;

  if (config.license_key && config.license_instance_id) {
    summary.textContent = '✓ Licensed';
    content.innerHTML = `
      <div class="license-row" style="margin-top: 7px; align-items: center;">
        <span style="flex:1; font-size:0.78rem; color:#4caf50;">
          Active on ${config.license_machine_name || 'this machine'}
        </span>
        <button class="btn btn-cancel license-activate" id="transfer-btn">Transfer</button>
      </div>
    `;
    document.getElementById('transfer-btn').addEventListener('click', async () => {
      const btn = document.getElementById('transfer-btn');
      btn.textContent = 'Deactivating...';
      btn.disabled = true;
      try {
        await invoke('deactivate_license');
        await renderLicenseSection();
        fitWindow();
      } catch (e) {
        btn.textContent = 'Transfer';
        btn.disabled = false;
        const errEl = content.querySelector('.license-err') || document.createElement('p');
        errEl.className = 'license-err license-status';
        errEl.style.color = '#e94560';
        errEl.textContent = typeof e === 'string' ? e : 'Deactivation failed.';
        content.appendChild(errEl);
      }
    });
  } else {
    summary.textContent = '🔑 License';
    content.innerHTML = `
      <div class="license-row">
        <input type="text" class="license-input" id="license-input"
          placeholder="XXXX-XXXX-XXXX-XXXX" autocomplete="off" />
        <button class="btn btn-save license-activate" id="activate-btn">Activate</button>
      </div>
      <p id="license-status" class="license-status"></p>
      <button class="buy-link" id="buy-btn">Buy a license →</button>
    `;
    document.getElementById('activate-btn').addEventListener('click', async () => {
      const key = document.getElementById('license-input').value.trim();
      if (!key) return;
      const btn = document.getElementById('activate-btn');
      btn.textContent = 'Activating...';
      btn.disabled = true;
      try {
        await invoke('activate_license', { key });
        await renderLicenseSection();
        fitWindow();
      } catch (e) {
        btn.textContent = 'Activate';
        btn.disabled = false;
        const status = document.getElementById('license-status');
        if (status) {
          status.textContent = typeof e === 'string' ? e : 'Activation failed.';
          status.style.color = '#e94560';
        }
      }
    });
    document.getElementById('buy-btn').addEventListener('click', () => {
      invoke('open_url', { url: STORE_URL });
    });
  }

  fitWindow();
}

document.getElementById('feedback-btn').addEventListener('click', () => {
  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="feedback-card">
      <div class="winapp-header">
        <span class="url-step-title">Send Feedback</span>
        <button class="winapp-close" id="fb-close">✕</button>
      </div>
      <div style="padding: 12px 12px 8px;">
        <p style="font-size:0.78rem; color:#888; margin-bottom:8px;">We read every message. Thank you!</p>
        <textarea class="feedback-textarea" id="fb-text" placeholder="Tell us what you think, report a bug, or suggest a feature..."></textarea>
      </div>
      <div style="display:flex; gap:8px; padding: 0 12px 12px;">
        <button class="btn btn-cancel" id="fb-cancel" style="flex:1;">Cancel</button>
        <button class="btn btn-save" id="fb-submit" style="flex:1;">Submit</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);

  const close = () => modal.remove();
  document.getElementById('fb-close').addEventListener('click', close);
  document.getElementById('fb-cancel').addEventListener('click', close);
  modal.addEventListener('click', (e) => { if (e.target === modal) close(); });

  document.getElementById('fb-submit').addEventListener('click', () => {
    const text = document.getElementById('fb-text').value.trim();
    if (!text) return;
    invoke('send_feedback', { message: text }).catch(console.error);
    close();
  });

  document.getElementById('fb-text').focus();
});

init();
