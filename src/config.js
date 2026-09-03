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

// ── Picker row icons ────────────────────────────────────────────────────────
// Extracting an exe icon is a Win32 round-trip per file, so fetching all ~200
// Start Menu entries up front would stall the picker — and renderApps() rebuilds
// the list on every keystroke, which would multiply it. Icons are therefore
// fetched only for rows actually scrolled into view, and cached by path so
// re-renders and repeat searches are free.
const iconCache = new Map();   // path -> base64 PNG, or null if extraction failed

// Icon bytes reach us from several sources with different formats: extracted
// Win32 icons are PNG, Steam's cached art is JPEG on some installs and PNG on
// others. Sniffing the base64 prefix beats hardcoding a type per call site,
// where a mismatch silently renders a broken image.
function dataUrlFor(b64) {
  if (!b64) return '';
  let mime = 'image/png';
  if (b64.startsWith('/9j/')) mime = 'image/jpeg';
  else if (b64.startsWith('AAABAA')) mime = 'image/x-icon';
  else if (b64.startsWith('R0lGOD')) mime = 'image/gif';
  return `data:${mime};base64,${b64}`;
}

function setPickerIcon(el, b64, mime) {
  el.textContent = '';
  const img = document.createElement('img');
  img.src = mime ? `data:${mime};base64,${b64}` : dataUrlFor(b64);
  img.alt = '';
  el.appendChild(img);
}

function makePickerIcon(fallbackEmoji) {
  const span = document.createElement('span');
  span.className = 'picker-icon';
  span.textContent = fallbackEmoji || '\u{1F5A5}\u{FE0F}';   // desktop computer
  return span;
}

const pickerIconObserver = new IntersectionObserver((entries) => {
  entries.forEach(entry => {
    if (!entry.isIntersecting) return;
    pickerIconObserver.unobserve(entry.target);
    loadPickerIcon(entry.target);
  });
// rootMargin so icons are ready slightly before a row scrolls in, rather than
// visibly popping once it arrives.
}, { rootMargin: '120px' });

async function loadPickerIcon(el) {
  const path = el.dataset.iconPath;
  if (!path) return;
  if (iconCache.has(path)) {
    const cached = iconCache.get(path);
    if (cached) setPickerIcon(el, cached);
    return;
  }
  const args = el.dataset.iconArgs || '';
  let data = null;
  try {
    data = await invoke('get_file_icon', { path, args });
  } catch { /* keep the fallback glyph */ }

  // A shortcut can name an icon file that no longer exists, or an index past
  // the end of it. Fall back to the target executable rather than dropping to
  // the placeholder glyph.
  const alt = el.dataset.iconFallback;
  if (!data && alt && alt !== path) {
    try { data = await invoke('get_file_icon', { path: alt, args }); } catch {}
  }

  iconCache.set(path, data);      // cache null too, so failures aren't retried
  if (data) setPickerIcon(el, data);
}

// Attaches an icon element that fills itself in when scrolled into view.
// `fallbackPath` is tried if the primary yields nothing.
function attachLazyIcon(el, path, args, fallbackPath) {
  if (!path) return;
  el.dataset.iconPath = path;
  if (args) el.dataset.iconArgs = args;
  if (fallbackPath) el.dataset.iconFallback = fallbackPath;
  if (iconCache.has(path)) {
    const cached = iconCache.get(path);
    if (cached) setPickerIcon(el, cached);
    return;
  }
  pickerIconObserver.observe(el);
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
      // Packaged (UWP) apps carry a pre-fetched icon, because theirs comes from
      // reading a package asset rather than the exe-icon path — and get_file_icon
      // can't do anything with an AUMID anyway. Everything else loads lazily.
      const icon = makePickerIcon(app.is_packaged ? '\u{1FA9F}' : '\u{1F5A5}\u{FE0F}');
      if (app.icon_data) setPickerIcon(icon, app.icon_data);
      // Prefer the shortcut's own "iconfile|index" reference — that is the icon
      // Explorer shows, and for the many Start Menu entries that point at a
      // shared resource DLL it is the ONLY way to get anything but the generic
      // glyph. Falls back to the target exe. See InstalledApp::icon_source.
      else if (!app.is_packaged) attachLazyIcon(icon, app.icon_source || app.path, app.args, app.path);
      row.appendChild(icon);
      const label = document.createElement('span');
      label.className = 'picker-label';
      label.textContent = app.name;
      row.appendChild(label);
      row.addEventListener('click', async () => {
        let icon_data = null;
        // Same source as the picker row: the .lnk, falling back to the target.
        // Reuse the cache so the icon already fetched for the row isn't
        // extracted a second time.
        const iconPath = app.icon_source || app.path;
        if (iconCache.has(iconPath) && iconCache.get(iconPath)) {
          icon_data = iconCache.get(iconPath);
        } else {
          try { icon_data = await invoke('get_file_icon', { path: iconPath, args: app.args || '' }); } catch {}
        }
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

// Renders chips for whatever is currently in suggestedAppsCache.
// Items stay visible even after being added — users can add the same
// app to a group more than once (e.g. two terminal windows), and the
// bar shouldn't jump around every time they click.
function paintSuggestedBar() {
  const wrap = document.getElementById('suggested-wrap');
  const bar  = document.getElementById('suggested-bar');
  if (!bar || !wrap) return;

  const apps = suggestedAppsCache || [];

  if (apps.length === 0) {
    wrap.style.display = 'none';
    bar.innerHTML = '';
    return;
  }

  wrap.style.display = 'flex';
  bar.innerHTML = '';

  apps.forEach(app => {
    let chip;
    if (app.icon_data) {
      chip = document.createElement('img');
      chip.className = 'suggested-chip';
      chip.src = dataUrlFor(app.icon_data);
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
    // Only a handful of browsers, all visible at once, so no lazy loading
    // needed — but reuse the same cache so a repeat visit is instant.
    const icon = makePickerIcon('\u{1F310}');   // globe
    attachLazyIcon(icon, browser.path, '');
    row.appendChild(icon);
    const label = document.createElement('span');
    label.className = 'picker-label';
    label.textContent = browser.name;
    row.appendChild(label);
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
    <div class="url-custom bookmark-search-row">
      <input type="text" id="bookmark-search" placeholder="Search bookmarks..." autocomplete="off" />
      <button type="button" class="bookmark-clear-btn" id="bookmark-clear" title="Uncheck every selected bookmark">Clear All</button>
    </div>
    <div class="winapp-list" id="bookmark-list">
      <div class="winapp-empty">Loading bookmarks...</div>
    </div>
    <div class="url-entry">
      <input type="text" id="custom-url-input" placeholder="Type a URL, IP or address and press Enter to add" autocomplete="off" />
    </div>
    <div class="url-footer">
      <button class="btn btn-cancel" id="skip-url-btn">Just Open Browser (No URL)</button>
      <button class="btn btn-save" id="add-selected-btn" disabled>${isEdit ? 'Save' : 'Add Selected'}</button>
    </div>
  `;

  if (!isEdit) {
    document.getElementById('url-back').addEventListener('click', () => {
      closeModal();
      showUrlPicker();
    });
  }

  // Available when editing too, not just when adding. Without it, clearing every
  // bookmark from an existing item was a dead end: Save stays disabled at zero
  // URLs (a url-type item with nothing to open breaks the whole group's launch),
  // and there was no other way out of the dialog with a saved change.
  // Converting to an app-type item pointing at the browser is the correct
  // representation of "just open the browser" — same thing the add flow does.
  document.getElementById('skip-url-btn').addEventListener('click', async () => {
    let icon_data = null;
    try { icon_data = await invoke('get_file_icon', { path: browser.path }); } catch {}
    const appItem = {
      item_type: 'app',
      path: browser.path,
      value: null,
      display_name: browser.name,
      icon_data,
    };
    if (isEdit) {
      // Carry over everything the bookmark dialog doesn't own. The user set
      // these elsewhere (layout editor) and would not expect editing URLs to
      // discard their window placement or desktop assignment.
      appItem.launch_desktop         = existingItem.launch_desktop ?? null;
      appItem.launch_x               = existingItem.launch_x ?? null;
      appItem.launch_y               = existingItem.launch_y ?? null;
      appItem.launch_width           = existingItem.launch_width ?? null;
      appItem.launch_height          = existingItem.launch_height ?? null;
      appItem.launch_virtual_desktop = existingItem.launch_virtual_desktop ?? null;
      appItem.launch_desktop_index   = existingItem.launch_desktop_index ?? null;
      currentItems[existingIdx] = appItem;
    } else {
      currentItems.push(appItem);
    }
    renderItems();
    closeModal();
  });
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
      // Many bookmarks come back with no title, which left the URL rendering in
      // the dim secondary style while manually-added entries used the bright
      // primary one — the same list in two different fonts. Promote the URL to
      // the primary line whenever there's no title, and only show the secondary
      // line when it would say something the primary doesn't.
      const hasTitle  = !!(bm.title && bm.title.trim());
      const esc = (s) => String(s ?? '')
        .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const primary   = esc(hasTitle ? bm.title : bm.url);
      const secondary = hasTitle ? esc(bm.url) : '';
      label.innerHTML = `
        <input type="checkbox" class="bookmark-checkbox" />
        <div class="bookmark-info">
          <div class="bookmark-title">${primary}</div>
          ${secondary ? `<div class="bookmark-url">${secondary}</div>` : ''}
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

  // Clears every checked bookmark, including ones currently hidden by the
  // search filter. That is the point of the button: the count on the Save
  // button can say 8 while only a few are on screen, and hunting down the
  // rest by scrolling is exactly the chore this avoids. The custom URL field
  // is deliberately left alone — it isn't a bookmark, and silently wiping
  // something the user typed would be a nasty surprise.
  document.getElementById('bookmark-clear').addEventListener('click', () => {
    modal.querySelectorAll('.bookmark-checkbox').forEach(cb => { cb.checked = false; });
    updateAddBtn();
  });

  // Adds whatever is in the custom field to the list as a checked row, so more
  // than one can be stacked up. Previously the field contributed exactly one URL
  // at save time, making it impossible to add several by hand.
  //
  // NO VALIDATION, deliberately. Anything the user types is accepted verbatim:
  // bare hostnames, IP addresses, ports, localhost, intranet names, custom
  // schemes. The browser is a far better judge of what resolves than a regex
  // here would be, and rejecting input that would have worked is worse than
  // passing through something that doesn't.
  function addCustomUrl(raw) {
    const url = raw.trim();
    if (!url) return;

    // Already listed? Tick that row instead of creating a duplicate.
    const existing = [...modal.querySelectorAll('.bookmark-checkbox')]
      .find(cb => cb.dataset.url === url);
    if (existing) {
      existing.checked = true;
      const row = existing.closest('.bookmark-row');
      if (row) {
        row.style.display = '';
        row.scrollIntoView({ block: 'nearest' });
      }
      customInput.value = '';
      updateAddBtn();
      return;
    }

    const listEl = document.getElementById('bookmark-list');
    // Drop the "No bookmarks found." placeholder if it's still showing.
    listEl.querySelector('.winapp-empty')?.remove();

    const safeUrl = url.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const label = document.createElement('label');
    label.className = 'bookmark-row';
    label.innerHTML = `
      <input type="checkbox" class="bookmark-checkbox" checked />
      <div class="bookmark-info">
        <div class="bookmark-title">${safeUrl}</div>
        <div class="bookmark-url">Added manually</div>
      </div>
    `;
    const cb = label.querySelector('.bookmark-checkbox');
    cb.dataset.url = url;
    cb.addEventListener('change', updateAddBtn);
    // Prepended rather than appended so it's visible straight away instead of
    // buried at the bottom of a long bookmark list.
    listEl.prepend(label);

    customInput.value = '';
    updateAddBtn();
    label.scrollIntoView({ block: 'nearest' });
  }

  customInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();   // stop the dialog's default button from firing
      addCustomUrl(customInput.value);
    }
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
      // These two were missing, so editing an item's bookmarks silently wiped
      // its virtual-desktop assignment: newItem never carried them, and the
      // Rust side defaults absent fields to None. The user's desktop choice
      // disappeared with no indication, and the item then launched wherever
      // the fallback sent it.
      newItem.launch_virtual_desktop = existingItem.launch_virtual_desktop ?? null;
      newItem.launch_desktop_index   = existingItem.launch_desktop_index ?? null;
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
      // Layout now comes from .winapp-row in styles.css, shared with the app and
      // browser pickers, so the three lists line up instead of each doing its own
      // inline flex.
      const iconEl = game.icon_data
        ? `<span class="picker-icon"><img src="${dataUrlFor(game.icon_data)}" alt="" /></span>`
        : `<span class="picker-icon">\u{1F3AE}</span>`;

      const safeName = game.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      row.innerHTML = `${iconEl}<span class="picker-label">${safeName}</span>`;

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

  const total = currentItems.length;
  const layoutLabels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);
  const closeAll = () => invoke('close_layout_windows', { labels: layoutLabels });

  // Pressing Edit Layout during a live session TOPS IT UP rather than being
  // refused. Layout windows carry normal title bars, so any of them can be
  // closed individually — and a window on another virtual desktop keeps the
  // session "open", which used to block reopening the one you just closed on
  // the desktop you are actually looking at.
  //
  // Resuming recreates only the missing windows. Survivors are left strictly
  // alone: not re-created, not re-positioned, and deliberately NOT focused,
  // since focusing a window on another desktop would drag the user over there.
  const resuming = !!activeLayoutLabels;

  if (!resuming) {
    // Fresh session: reclaim the fixed labels (layout-item-0, ...) from any
    // previous run. destroy() is asynchronous on the Rust side and the label
    // stays registered until the window is really gone, so creating too soon
    // fails with "a window with label layout-item-0 already exists" — which
    // only surfaces as tauri://error and made the button look dead.
    try { await invoke('close_layout_windows', { labels: layoutLabels }); } catch (_) {}
    for (let attempt = 0; attempt < 20; attempt++) {
      const existing = await Promise.all(
        layoutLabels.map(l => WebviewWindow.getByLabel(l).catch(() => null))
      );
      if (existing.every(w => !w)) break;
      await new Promise(r => setTimeout(r, 50));   // up to ~1s total
    }
  }

  // Which labels already have a live window right now.
  const liveWindows = await Promise.all(
    layoutLabels.map(l => WebviewWindow.getByLabel(l).catch(() => null))
  );

  // Get the config window's physical rect to compute fallback positions on the
  // correct monitor. window.screen always reports the *primary* display in
  // WebView2 regardless of which monitor the config window is actually on.
  let configRect = null;
  try { configRect = await invoke('get_window_physical_rect', { label: 'config' }); } catch (_) {}

  // Fallback geometry for items with no saved position.
  //
  // These were a fixed 800x600 PHYSICAL pixels, which is the problem: on a
  // scaled display that is far fewer CSS pixels (800 physical at 150% is 533
  // logical), so the window's own content overflowed and it looked tiny. Sizing
  // relative to the monitor scales correctly at any resolution or DPI.
  //
  // The cascade also started 40px from the config window, which now overlaps it
  // since that window is 840 wide — so the layout windows appeared to spawn
  // inside it. Starting from the monitor's own top-left keeps them clear.
  const monitors = await placementMonitors();
  const cfgCx = configRect ? configRect[0] + Math.floor(configRect[2] / 2) : 0;
  const cfgCy = configRect ? configRect[1] + Math.floor(configRect[3] / 2) : 0;
  const mon = monitors.find(m =>
    cfgCx >= m.x && cfgCx < m.x + m.width && cfgCy >= m.y && cfgCy < m.y + m.height
  ) || monitors[0];

  const fallbackW = mon ? Math.round(mon.width  * 0.55) : 1000;
  const fallbackH = mon ? Math.round(mon.height * 0.55) : 700;
  const fallbackBaseX = mon ? mon.x + Math.round(mon.width  * 0.06) : 200;
  const fallbackBaseY = mon ? mon.y + Math.round(mon.height * 0.06) : 200;
  const cascadeStep = 44;   // enough that stacked title bars stay distinguishable
  let layoutErrorShown = false;

  for (let idx = 0; idx < total; idx++) {
    // Still open — leave it exactly as the user left it, including any dragging
    // they have already done that isn't saved yet.
    if (liveWindows[idx]) continue;

    const item = currentItems[idx];
    const hasPos = item.launch_x != null && item.launch_y != null;

    // Heading is the friendly name — the same one the item list shows — with
    // the full path relegated to a small line beneath it. Previously the
    // heading WAS the raw path, which made every window shout an absolute
    // filesystem path at the user instead of naming the app.
    const rawName = item.item_type === 'steam'
      ? (item.path || 'Steam Game')
      : item.item_type === 'url'
        ? browserDisplayName(item)
        : (item.display_name || fallbackDisplayName(item.path) || item.path || 'Item');
    // Secondary detail: the path for anything file-based, the first URL for a
    // browser item. Suppressed by layout-item.js when it merely repeats the name.
    const rawSub = item.item_type === 'url'
      ? ((item.urls && item.urls[0]) || item.value || '')
      : (item.path || '');
    const safeName = encodeURIComponent(rawName);
    const safeSub  = encodeURIComponent(rawSub);

    const vdParam = item.launch_virtual_desktop
      ? '&vd=' + encodeURIComponent(JSON.stringify(item.launch_virtual_desktop))
      : '';
    const label = `layout-item-${idx}`;

    // Physical pixel target for this window.
    const physX = hasPos ? item.launch_x : fallbackBaseX + idx * cascadeStep;
    const physY = hasPos ? item.launch_y : fallbackBaseY + idx * cascadeStep;
    const physW = (hasPos && item.launch_width)  ? item.launch_width  : fallbackW;
    const physH = (hasPos && item.launch_height) ? item.launch_height : fallbackH;

    // Create the window hidden at a dummy position. We always use
    // set_layout_window_physics (physical pixels) to place it correctly —
    // this avoids per-monitor DPR ambiguity from window.devicePixelRatio,
    // which reflects the config window's monitor, not the item's monitor.
    const win = new WebviewWindow(label, {
      url: `layout-item.html?idx=${idx}&name=${safeName}&sub=${safeSub}&total=${total}${vdParam}`,
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
      // Surface it. This previously logged only to the dev console, so a
      // failure to open the layout editor looked identical to the button
      // being broken. Guarded so N failing windows produce one message.
      if (!layoutErrorShown) {
        layoutErrorShown = true;
        activeLayoutLabels = null;   // don't leave the re-entry guard latched
        showErrorModal('Could not open the layout editor. Close any leftover layout windows and try again.');
      }
    });
  }

  activeLayoutLabels = layoutLabels;

  // Only one set of listeners per session. Resuming must not register a second
  // pair — save/cancel would then be handled twice, and the first handler's
  // unlisten() calls would leave the duplicates alive for the next session.
  if (resuming) return;

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

// Width must match the inner_size the window is created with in lib.rs
// (open_config_window_inner and the App Settings builder). This runs after
// every render, so a mismatch here silently overrides the Rust value and the
// window snaps back to the old width.
const CONFIG_WINDOW_WIDTH = 840;

async function fitWindow() {
  await new Promise(resolve => requestAnimationFrame(resolve));
  const h = document.querySelector('.config-window').offsetHeight;
  await getCurrentWindow().setSize(new LogicalSize(CONFIG_WINDOW_WIDTH, h));
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
  invoke('check_license_status').then(async status => {
    if (status === 'revoked') {
      // Clear the local license record — server already revoked it, no API call needed.
      try { await invoke('clear_license_local'); } catch {}
      await renderLicenseSection();
      fitWindow();
      // Enforce group limit now that the license is gone.
      const cfg = await invoke('get_config');
      const visibleGroups = cfg.groups.filter(g => !g.hidden);
      if (visibleGroups.length > 1) showGroupPicker(visibleGroups);
    }
  }).catch(() => {}); // Unreachable = offline, ignore silently

  // Enforce free-tier group limit on config-window open. Catches the case
  // where the user is already unlicensed with >1 visible groups (e.g. the
  // widget picker was somehow skipped, or they opened config directly).
  {
    const initCfg = await invoke('get_config');
    if (!initCfg.license_key || !initCfg.license_instance_id) {
      const visibleGroups = initCfg.groups.filter(g => !g.hidden);
      if (visibleGroups.length > 1) showGroupPicker(visibleGroups);
    }
  }
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

  // Build version at the foot of the tab. Sourced from the binary itself
  // (CARGO_PKG_VERSION) rather than anything in config, so it can't drift.
  invoke('get_app_version').then(v => {
    const el = document.getElementById('app-version');
    if (el) el.textContent = `TakeOff v${v}`;
  }).catch(() => {});

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

// Desktop/monitor lookups for the placement chips. Cached as promises for the
// duration of one render pass so a group with ten items issues two IPC calls
// rather than twenty. renderItems() clears these, so reopening the panel after
// changing desktops or displays picks up the new layout.
let _placementDesktops = null;
let _placementMonitors = null;
function resetPlacementCaches() {
  _placementDesktops = null;
  _placementMonitors = null;
}
function placementDesktops() {
  return (_placementDesktops ??= invoke('get_virtual_desktops').catch(() => []));
}
function placementMonitors() {
  return (_placementMonitors ??= invoke('get_monitors').catch(() => []));
}

// Virtual desktop GUIDs arrive as byte arrays, so compare element-wise.
function sameGuid(a, b) {
  return Array.isArray(a) && Array.isArray(b)
    && a.length === b.length && a.every((v, i) => v === b[i]);
}

// Which monitor a saved rectangle sits on, or undefined if none.
// Tested against the window's CENTRE: Windows reports a few pixels of negative
// offset on ordinary windows (-12 is routine), so a top-left test would place
// such a window off the leftmost monitor and wrongly report it off-screen.
function monitorForItem(item, monitors) {
  if (item.launch_x == null || item.launch_y == null) return undefined;
  const cx = item.launch_x + Math.floor((item.launch_width || 0) / 2);
  const cy = item.launch_y + Math.floor((item.launch_height || 0) / 2);
  return monitors.find(m =>
    cx >= m.x && cx < m.x + m.width && cy >= m.y && cy < m.y + m.height);
}

// A sensible starting rectangle on `m`, preserving the item's existing size if
// it has one so switching screens doesn't also resize the window. 60% of the
// monitor, centred — big enough to be usable, clearly not maximised, and
// obviously deliberate so it reads as a default rather than a glitch.
function defaultRectOnMonitor(m, keepW, keepH) {
  const width  = Math.min(keepW || Math.round(m.width  * 0.6), m.width);
  const height = Math.min(keepH || Math.round(m.height * 0.6), m.height);
  return {
    launch_x: m.x + Math.round((m.width  - width)  / 2),
    launch_y: m.y + Math.round((m.height - height) / 2),
    launch_width: width,
    launch_height: height,
  };
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

  // Desktop + screen pickers, side by side.
  //
  // Both used to require the layout editor: desktop via its per-window
  // dropdown, screen only implicitly by dragging a window onto a monitor. That
  // meant a full drag-the-windows session to set either. They live here now,
  // and a separate read-only summary is unnecessary because the dropdowns show
  // the current state themselves.
  {
    const placeRow = document.createElement('div');
    placeRow.className = 'item-expand-row placement-selects';
    placeRow.innerHTML = `
      <label class="placement-field">
        <span class="placement-field-label">Desktop</span>
        <select class="vd-sel">
          <option value="">First desktop (default)</option>
        </select>
      </label>
      <label class="placement-field">
        <span class="placement-field-label">Screen</span>
        <select class="mon-sel" title="Choosing a screen saves a default position on it. Use Edit Layout to fine-tune.">
          <option value="">Any screen (default)</option>
        </select>
      </label>
    `;
    const vdSel  = placeRow.querySelector('.vd-sel');
    const monSel = placeRow.querySelector('.mon-sel');

    // --- Desktop -------------------------------------------------------------
    placementDesktops().then(desktops => {
      desktops.forEach((d, pos) => {
        const opt = document.createElement('option');
        // Value is the ARRAY POSITION (0-based) — that is what
        // launch_desktop_index means to the launcher. VirtualDesktop.index is
        // 1-based and for display only; confusing the two silently targets the
        // wrong desktop.
        opt.value = String(pos);
        opt.textContent = d.name || `Desktop ${d.index}`;
        if (hasDesktop(item) && sameGuid(d.guid, item.launch_virtual_desktop)) opt.selected = true;
        vdSel.appendChild(opt);
      });
      // A saved desktop that no longer exists must still be represented, or the
      // dropdown would read "First desktop" and misreport where the item goes.
      if (hasDesktop(item) && !desktops.some(d => sameGuid(d.guid, item.launch_virtual_desktop))) {
        const opt = document.createElement('option');
        opt.value = 'missing';
        opt.textContent = 'Saved desktop (missing)';
        opt.selected = true;
        vdSel.appendChild(opt);
        vdSel.classList.add('select-warn');
      }
    }).catch(() => {});

    vdSel.addEventListener('change', async (e) => {
      const v = e.target.value;
      if (v === 'missing') return;
      if (v === '') {
        currentItems[idx].launch_virtual_desktop = null;
        currentItems[idx].launch_desktop_index = null;
      } else {
        const desktops = await placementDesktops();
        const pos = parseInt(v, 10);
        const d = desktops[pos];
        if (d) {
          currentItems[idx].launch_virtual_desktop = d.guid;
          // Stored with the GUID so the launcher can still find the right
          // desktop if Windows regenerates GUIDs later.
          currentItems[idx].launch_desktop_index = pos;
        }
      }
      renderItems();   // regroups, so the item moves to its new section
    });

    // --- Screen --------------------------------------------------------------
    // There is no separate "monitor" field for ordinary items: the launcher
    // works purely from the saved rectangle. So picking a screen writes a
    // concrete default rect on it, which is also what makes an unedited item
    // open somewhere sensible instead of wherever the app feels like.
    placementMonitors().then(monitors => {
      const current = monitorForItem(item, monitors);
      monitors.forEach((m, pos) => {
        const opt = document.createElement('option');
        opt.value = String(pos);
        opt.textContent = m.is_primary ? 'Main screen' : `Screen ${m.index + 1}`;
        opt.title = `${m.name} - ${m.width}x${m.height}`;
        if (current === m) opt.selected = true;
        monSel.appendChild(opt);
      });
      // Saved coordinates that aren't on any connected display — common after
      // unplugging a monitor. Surfaced rather than silently showing "Any".
      if (item.launch_x != null && !current) {
        const opt = document.createElement('option');
        opt.value = 'offscreen';
        opt.textContent = 'Off-screen position';
        opt.selected = true;
        monSel.appendChild(opt);
        monSel.classList.add('select-warn');
      }
    }).catch(() => {});

    monSel.addEventListener('change', async (e) => {
      const v = e.target.value;
      if (v === 'offscreen') return;
      const it = currentItems[idx];
      if (v === '') {
        // "Any screen" means no saved geometry at all — the app opens wherever
        // it normally would. This doubles as the old Clear button.
        it.launch_x = it.launch_y = it.launch_width = it.launch_height = null;
      } else {
        const monitors = await placementMonitors();
        const m = monitors[parseInt(v, 10)];
        if (m) Object.assign(it, defaultRectOnMonitor(m, it.launch_width, it.launch_height));
      }
      renderItems();
    });

    panel.appendChild(placeRow);
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

// ── Launch-order grouping ───────────────────────────────────────────────────
// Mirrors how launch_group_inner actually batches work, so the list reflects
// real launch order instead of hiding it:
//   * items are grouped by target virtual desktop, batches running in
//     first-appearance order;
//   * an item with no desktop is folded into the first desktop's batch;
//   * a URL item with NO saved position and NO desktop is excluded from the
//     batches entirely and opened at the very end as one multi-tab launch, so
//     it can never run first no matter where it sits in the list;
//   * if nothing targets a desktop at all, there is no batching and the list
//     is a plain sequence.
const GROUP_TRAILING_URLS = 'trailing-urls';
const GROUP_FLAT = 'flat';

function hasDesktop(item) {
  return Array.isArray(item.launch_virtual_desktop) && item.launch_virtual_desktop.length > 0;
}

function groupKeyFor(item, anyDesktop) {
  if (item.item_type === 'url' && item.launch_x == null && !hasDesktop(item)) {
    return GROUP_TRAILING_URLS;
  }
  if (!anyDesktop) return GROUP_FLAT;
  return hasDesktop(item) ? 'vd:' + item.launch_virtual_desktop.join(',') : 'vd:first';
}

// Rewrites currentItems so members of a group are contiguous, keeping each
// group's first-appearance order. Without this the UI could show items grouped
// while the array had them interleaved, and dragging within a visual group
// would map onto scattered array indices.
function normaliseItemOrder() {
  const anyDesktop = currentItems.some(hasDesktop);
  const order = [];
  const buckets = new Map();
  // Buckets hold INDICES, not items, so the same permutation can be applied to
  // activeLayoutLabels below.
  currentItems.forEach((item, i) => {
    const key = groupKeyFor(item, anyDesktop);
    if (!buckets.has(key)) { buckets.set(key, []); order.push(key); }
    buckets.get(key).push(i);
  });
  // Trailing URLs always render last because that is when they launch.
  const keys = order.filter(k => k !== GROUP_TRAILING_URLS);
  if (buckets.has(GROUP_TRAILING_URLS)) keys.push(GROUP_TRAILING_URLS);
  const perm = keys.flatMap(k => buckets.get(k));

  currentItems = perm.map(i => currentItems[i]);
  // activeLayoutLabels maps layout windows to items BY POSITION, so it has to
  // travel with any reordering or saved geometry lands on the wrong items.
  // Only remapped when the lengths still correspond — items added or removed
  // mid-session are handled by removeItemAt keeping them in step.
  if (activeLayoutLabels && activeLayoutLabels.length === perm.length) {
    activeLayoutLabels = perm.map(i => activeLayoutLabels[i]);
  }

  return { anyDesktop, showHeaders: keys.length > 1 || buckets.has(GROUP_TRAILING_URLS) };
}

// Tracks a header drag in progress. A module-level flag rather than
// dataTransfer because dragover is not allowed to read transfer data, and the
// item-row handlers need to be able to tell the two kinds of drag apart.
let draggingGroupKey = null;

// Moves an entire desktop group ahead of or behind another, which is what
// decides batch order at launch: launch_group_inner batches by desktop in
// FIRST-APPEARANCE order, so reordering the blocks reorders the batches.
function moveGroupBlock(fromKey, toKey, before) {
  if (fromKey === toKey) return;
  const anyDesktop = currentItems.some(hasDesktop);
  const order = [];
  const buckets = new Map();
  currentItems.forEach((item, i) => {
    const k = groupKeyFor(item, anyDesktop);
    if (!buckets.has(k)) { buckets.set(k, []); order.push(k); }
    buckets.get(k).push(i);            // indices, so labels can follow
  });

  const fromPos = order.indexOf(fromKey);
  if (fromPos === -1) return;
  order.splice(fromPos, 1);
  let toPos = order.indexOf(toKey);
  if (toPos === -1) return;
  if (!before) toPos += 1;
  order.splice(toPos, 0, fromKey);

  // Trailing URLs are pinned last no matter what: they are launched after every
  // batch, so letting them sit anywhere else would be a lie.
  const keys = order.filter(k => k !== GROUP_TRAILING_URLS);
  if (buckets.has(GROUP_TRAILING_URLS)) keys.push(GROUP_TRAILING_URLS);

  const perm = keys.flatMap(k => buckets.get(k));
  currentItems = perm.map(i => currentItems[i]);
  if (activeLayoutLabels && activeLayoutLabels.length === perm.length) {
    activeLayoutLabels = perm.map(i => activeLayoutLabels[i]);
  }
}

// Makes a group header a drag handle for its whole block.
function attachGroupDrag(header, key, list) {
  // "Opens last" cannot move — it is defined by launching after everything.
  if (key === GROUP_TRAILING_URLS || key === GROUP_FLAT) return;

  header.setAttribute('draggable', 'true');
  header.classList.add('item-group-draggable');

  header.addEventListener('dragstart', e => {
    draggingGroupKey = key;
    e.dataTransfer.effectAllowed = 'move';
    header.classList.add('item-group-dragging');
    const ghost = document.createElement('div');
    ghost.className = 'item-drag-ghost';
    ghost.textContent = header.querySelector('.item-group-name')?.textContent || 'Desktop';
    document.body.appendChild(ghost);
    e.dataTransfer.setDragImage(ghost, 14, 14);
    setTimeout(() => ghost.remove(), 0);
  });

  header.addEventListener('dragend', () => {
    draggingGroupKey = null;
    header.classList.remove('item-group-dragging');
    list.querySelectorAll('.group-drop-before, .group-drop-after')
      .forEach(el => el.classList.remove('group-drop-before', 'group-drop-after'));
  });

  header.addEventListener('dragover', e => {
    if (!draggingGroupKey || draggingGroupKey === key) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const rect = header.getBoundingClientRect();
    const after = (e.clientY - rect.top) > rect.height / 2;
    header.classList.toggle('group-drop-after', after);
    header.classList.toggle('group-drop-before', !after);
  });

  header.addEventListener('dragleave', e => {
    if (header.contains(e.relatedTarget)) return;
    header.classList.remove('group-drop-before', 'group-drop-after');
  });

  header.addEventListener('drop', e => {
    e.preventDefault();
    const rect = header.getBoundingClientRect();
    const after = (e.clientY - rect.top) > rect.height / 2;
    header.classList.remove('group-drop-before', 'group-drop-after');
    const from = draggingGroupKey;
    draggingGroupKey = null;
    if (!from || from === key) return;
    moveGroupBlock(from, key, !after);
    renderItems();
  });
}

function makeGroupHeader(key) {
  const header = document.createElement('div');
  header.className = 'item-group-header';
  if (key === GROUP_TRAILING_URLS) {
    header.innerHTML = `<span class="item-group-name">Opens last</span>
      <span class="item-group-note">browser tabs, always after everything else</span>`;
    return header;
  }
  header.innerHTML = `<span class="item-group-name">Desktop</span>`;
  const nameEl = header.querySelector('.item-group-name');
  if (key === 'vd:first') {
    nameEl.textContent = 'First desktop';
    header.insertAdjacentHTML('beforeend',
      '<span class="item-group-note">no desktop set - launches on the first one</span>');
    return header;
  }
  // Resolve the GUID to a readable name once the desktop list arrives.
  const guid = key.slice(3).split(',').map(Number);
  nameEl.textContent = 'Desktop…';
  placementDesktops().then(desktops => {
    const match = desktops.find(d => sameGuid(d.guid, guid));
    nameEl.textContent = match ? (match.name || `Desktop ${match.index}`) : 'Desktop (missing)';
    if (!match) header.classList.add('item-group-header-warn');
  }).catch(() => {});
  return header;
}

function renderItems() {
  renderSuggestedBar();
  // Re-query desktops and monitors once per render, not once per item.
  resetPlacementCaches();

  // Anything appended since an "+ Add to <desktop>" button was pressed joins
  // that desktop. Done here rather than inside each picker because every add
  // path — Windows Apps, URL, File, Folder, Steam — ends by pushing and
  // re-rendering, so this is the one place they all pass through.
  if (pendingAdd && currentItems.length > pendingAdd.baseLen) {
    for (let i = pendingAdd.baseLen; i < currentItems.length; i++) {
      currentItems[i].launch_virtual_desktop = pendingAdd.guid;
      currentItems[i].launch_desktop_index = pendingAdd.index;
    }
    closeAddMenu();
  }

  const list = document.getElementById('items-list');
  list.innerHTML = '';

  if (currentItems.length === 0) {
    list.style.overflowY = 'hidden';
    const empty = document.createElement('div');
    empty.style.cssText = 'color:#4a5568;font-size:0.8rem;text-align:center;padding:12px 0;';
    empty.textContent = 'No items yet';
    list.appendChild(empty);
    renderAddButtons(null);      // nothing to group by yet
    return;
  }

  list.style.overflowY = 'auto';

  const { anyDesktop, showHeaders } = normaliseItemOrder();
  let prevKey = null;
  // Collected for the add buttons: one entry per desktop group, in list order.
  // "Opens last" is excluded — it is defined by having no desktop and no
  // position, so an "add to it" button would be meaningless.
  const addGroups = [];

  // Closes out a section with its own add button, so the button sits at the end
  // of the group it adds to. "Opens last" gets none — that group is defined by
  // having no desktop and no position, so anything added would immediately fall
  // into a different section.
  const emitAddButton = (key) => {
    if (!key || key === GROUP_TRAILING_URLS) return;
    const g = addGroups.find(x => x.key === key);
    if (g) list.appendChild(makeAddButton(g));
  };

  currentItems.forEach((item, idx) => {
    const groupKey = groupKeyFor(item, anyDesktop);
    if (groupKey !== GROUP_TRAILING_URLS && !addGroups.some(g => g.key === groupKey)) {
      addGroups.push({
        key: groupKey,
        guid: hasDesktop(item) ? item.launch_virtual_desktop : null,
        index: hasDesktop(item) ? (item.launch_desktop_index ?? null) : null,
      });
    }
    if (showHeaders && groupKey !== prevKey) {
      if (prevKey !== null) emitAddButton(prevKey);   // close out the previous section
      const header = makeGroupHeader(groupKey);
      attachGroupDrag(header, groupKey, list);
      list.appendChild(header);
      prevKey = groupKey;
    }

    const wrapper = document.createElement('div');
    wrapper.className = 'item-wrapper';

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
        ? `<img src="${dataUrlFor(item.icon_data)}" style="width:24px;height:24px;object-fit:contain;flex-shrink:0;" alt="" />`
        : '<span style="flex-shrink:0;font-size:19px;line-height:1;">🌐</span>';

      const safeLabel    = label.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
      const safeSubtitle = subtitle.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

      row.innerHTML = `
        ${iconHtml}
        <div class="item-label-multi" title="${safeSubtitle}" style="flex:1;min-width:0;overflow:hidden;">
          <div class="item-label">${safeLabel}</div>
          <div style="font-size:11px;color:#888;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${safeSubtitle}</div>
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
        ? `<img src="${dataUrlFor(item.icon_data)}" style="width:24px;height:24px;object-fit:contain;flex-shrink:0;border-radius:2px;" alt="" />`
        : '<span style="flex-shrink:0;font-size:19px;line-height:1;">🎮</span>';
      row.innerHTML = `
        ${iconHtml}
        <span class="item-label" title="${safeLabel}">${safeLabel} <span style="color:#1b9fdb;font-size:11px;font-weight:400;">Steam</span></span>
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
        ? `<img src="${dataUrlFor(item.icon_data)}" style="width:24px;height:24px;object-fit:contain;flex-shrink:0;" alt="" />`
        : `<span style="flex-shrink:0;font-size:19px;line-height:1;">${typeEmoji}</span>`;
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

    row.dataset.index = idx;
    row.dataset.groupKey = groupKey;
    row.dataset.itemType = item.item_type;   // read during dragover
    {
      row.setAttribute('draggable', 'true');
      row.classList.add('item-row-draggable');
      row.addEventListener('dragstart', e => {
        e.dataTransfer.setData('text/plain', String(idx));
        e.dataTransfer.effectAllowed = 'move';
        row.classList.add('item-row-dragging');

        // Replace the default drag ghost (a full-size copy of the row) with a
        // small chip. The default image is as wide as the list and sits under
        // the cursor, hiding the very rows you are aiming between.
        const ghost = document.createElement('div');
        ghost.className = 'item-drag-ghost';
        ghost.textContent = row.querySelector('.item-label')?.textContent?.trim() || 'Item';
        document.body.appendChild(ghost);
        e.dataTransfer.setDragImage(ghost, 14, 14);
        // The browser snapshots the element synchronously, so it can be removed
        // as soon as the current task finishes.
        setTimeout(() => ghost.remove(), 0);
      });
      row.addEventListener('dragend', () => {
        row.classList.remove('item-row-dragging');
        list.querySelectorAll('.drop-before, .drop-after')
          .forEach(el => el.classList.remove('drop-before', 'drop-after'));
      });
      // Drag-over/drop live on the WRAPPER, not the row: an item is the title
      // row plus its expand panel, so the marker has to sit outside both. On
      // the row alone, "after" landed between the title and the dropdowns —
      // visually splitting one item rather than separating two.
      wrapper.addEventListener('dragover', e => {
        // A header drag is in flight — items are not valid targets for it.
        if (draggingGroupKey) { e.dataTransfer.dropEffect = 'none'; return; }
        const from = document.querySelector('.item-row-dragging');
        if (!from) { e.dataTransfer.dropEffect = 'none'; return; }
        // Dropping into a DIFFERENT group reassigns the item to that group's
        // desktop — the obvious reading of the gesture. The one impossible
        // move is a non-URL item into "Opens last", which by definition only
        // holds URL items with no position and no desktop.
        if (groupKey === GROUP_TRAILING_URLS && from.dataset.itemType !== 'url') {
          e.dataTransfer.dropEffect = 'none';
          return;
        }
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        // Indicate the edge the item will actually land on. Which half of the
        // row the cursor is over decides before-vs-after, so the marker always
        // matches where the drop really goes — previously the line was drawn
        // above the target no matter what, while a downward drag inserted below
        // it, so the feedback contradicted the result.
        const rect = wrapper.getBoundingClientRect();
        const after = (e.clientY - rect.top) > rect.height / 2;
        wrapper.classList.toggle('drop-after', after);
        wrapper.classList.toggle('drop-before', !after);
      });
      wrapper.addEventListener('dragleave', e => {
        // Ignore leaves fired while moving between the wrapper's own children
        // (row -> expand panel), which would otherwise flicker the marker off
        // as the cursor crosses from the title into the dropdowns.
        if (wrapper.contains(e.relatedTarget)) return;
        wrapper.classList.remove('drop-before', 'drop-after');
      });
      wrapper.addEventListener('drop', e => {
        e.preventDefault();
        const dropRect = wrapper.getBoundingClientRect();
        const dropAfter = (e.clientY - dropRect.top) > dropRect.height / 2;
        wrapper.classList.remove('drop-before', 'drop-after');
        const fromIdx = parseInt(e.dataTransfer.getData('text/plain'), 10);
        if (Number.isNaN(fromIdx) || fromIdx === idx) return;

        const dragged = currentItems[fromIdx];
        // Crossing into another group means adopting that group's placement.
        // Copied from the item already sitting there rather than recomputed,
        // so the two can never disagree about which desktop the group is.
        if (groupKeyFor(dragged, anyDesktop) !== groupKey) {
          const target = currentItems[idx];
          if (groupKey === GROUP_TRAILING_URLS) {
            if (dragged.item_type !== 'url') return;
            // "Opens last" is defined by having neither a desktop nor a
            // position, so joining it means giving up both.
            dragged.launch_virtual_desktop = null;
            dragged.launch_desktop_index = null;
            dragged.launch_x = dragged.launch_y = null;
            dragged.launch_width = dragged.launch_height = null;
          } else {
            dragged.launch_virtual_desktop = target.launch_virtual_desktop ?? null;
            dragged.launch_desktop_index   = target.launch_desktop_index ?? null;
          }
        }

        // Removing the dragged item first shifts everything after it down one,
        // so an insertion point past the removal has to be decremented. Without
        // this a downward drag lands one slot short of the indicator.
        let insertAt = dropAfter ? idx + 1 : idx;
        if (fromIdx < insertAt) insertAt--;

        const [moved] = currentItems.splice(fromIdx, 1);
        currentItems.splice(insertAt, 0, moved);
        // Keep the layout-window mapping aligned if a session is open — it is
        // positional, so a reorder here has to move the labels too.
        if (activeLayoutLabels && activeLayoutLabels.length === currentItems.length + 1) {
          const [movedLabel] = activeLayoutLabels.splice(fromIdx, 1);
          activeLayoutLabels.splice(insertAt, 0, movedLabel);
        }
        renderItems();
      });
    }

    wrapper.appendChild(row);
    wrapper.appendChild(buildExpandPanel(item, idx));
    list.appendChild(wrapper);
  });

  const grouped = showHeaders && addGroups.length > 1;
  // Final section needs closing out too — the loop only emits when the group
  // changes, so the last one never triggers it.
  if (grouped) emitAddButton(prevKey);

  // Grouped: buttons live inline above, and the area below the list is emptied.
  // Ungrouped: a single "+ Add Item" stays in its original place.
  renderAddButtons(grouped ? addGroups : null);

  fitWindow();
}

// Which add-button currently owns the open type menu (null = closed). Needed so
// that button can show Cancel while the others stay as Add.
let addMenuOpenFor = null;
// Desktop the next added item(s) should join, plus the item count at the moment
// the menu was opened. Adding runs through several async pickers that all end in
// currentItems.push, so rather than threading a target through every one of
// them, anything appended past baseLen is claimed here on the next render.
let pendingAdd = null;

function closeAddMenu() {
  addMenuOpenFor = null;
  pendingAdd = null;
}

// The Add Item window only chooses a type; the pickers themselves live here and
// work on currentItems, so the choice comes back as an event.
listen('add-item-type', ({ payload }) => {
  if (payload?.type) addItem(payload.type);
});
// Cancelled — drop the pending desktop so it can't be applied to some later,
// unrelated add.
listen('add-item-cancel', () => { pendingAdd = null; });

// One add button, belonging to a single group. Rendered INSIDE the items list
// at the end of that group's section, so the button sits exactly where the new
// item will appear rather than in a detached area below the list.
function makeAddButton(g) {
  const btn = document.createElement('div');
  btn.className = 'add-item-btn add-item-btn-inline';
  btn.textContent = '+ Add Item';
  // Held so the picker window can be told where the item is going.
  let targetLabel = '';

  btn.onclick = () => {
    // Record the destination BEFORE the picker window opens. Anything appended
    // to currentItems afterwards is claimed by renderItems and given this
    // desktop — see the pendingAdd handling there.
    pendingAdd = { guid: g.guid, index: g.index, baseLen: currentItems.length };
    invoke('open_add_item_window', { target: targetLabel || null }).catch(() => {});
  };

  // Desktop names resolve asynchronously. Until then the button reads
  // "+ Add Item" — accurate if unhelpful — rather than flashing a wrong number.
  if (g.key !== '__all__') {
    if (g.key === 'vd:first') {
      targetLabel = 'the first desktop';
      btn.textContent = '+ Add to first desktop';
    } else {
      placementDesktops().then(desktops => {
        const match = desktops.find(d => sameGuid(d.guid, g.guid));
        targetLabel = match ? (match.name || `Desktop ${match.index}`) : 'this desktop';
        btn.textContent = '+ Add to ' + targetLabel;
      }).catch(() => {});
    }
  }
  return btn;
}

// The ungrouped case keeps a single button in its original spot below the list.
function renderAddButtons(groups) {
  const host = document.getElementById('add-item-buttons');
  if (!host) return;
  host.innerHTML = '';
  if (groups === null) {
    host.appendChild(makeAddButton({ key: '__all__', guid: null, index: null }));
  }
  // When grouped, the buttons live inside the list instead — see renderItems.
}

// (The [data-type] menu items this used to bind now live in add-item.html and
// come back through the 'add-item-type' event instead.)

function detectItemType(path) {
  const ext = path.split('.').pop().toLowerCase();
  if (ext === 'exe') return 'app';
  if (['bat', 'cmd', 'ps1'].includes(ext)) return 'script';
  return 'file';
}

async function addItem(type) {
  // pendingAdd is deliberately NOT cleared here: the picker that follows is
  // async, and the new item only lands — and gets claimed by renderItems —
  // once it returns.
  addMenuOpenFor = null;

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

function showGroupPicker(groups) {
  const backdrop = document.getElementById('group-picker-backdrop');
  const list     = document.getElementById('group-picker-list');
  const confirm  = document.getElementById('group-picker-confirm');

  list.innerHTML = '';
  let selectedId = null;

  groups.forEach(group => {
    const btn = document.createElement('button');
    btn.className = 'group-picker-btn';
    btn.innerHTML = `<span class="gp-icon">${group.icon}</span><span>${group.name}</span>`;
    btn.addEventListener('click', () => {
      list.querySelectorAll('.group-picker-btn').forEach(b => b.classList.remove('selected'));
      btn.classList.add('selected');
      selectedId = group.id;
      confirm.disabled = false;
    });
    list.appendChild(btn);
  });

  confirm.disabled = true;
  backdrop.classList.add('visible');

  confirm.onclick = async () => {
    if (!selectedId) return;
    confirm.disabled = true;
    try {
      await invoke('apply_free_tier_groups', { keepGroupId: selectedId });
      backdrop.classList.remove('visible');
    } catch (e) {
      confirm.disabled = false;
    }
  };
  // No click-outside-to-close — user must pick a group.
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
        // If the user has more than one visible group, ask which to keep.
        const cfg = await invoke('get_config');
        const visibleGroups = cfg.groups.filter(g => !g.hidden);
        if (visibleGroups.length > 1) {
          showGroupPicker(visibleGroups);
        }
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
        await invoke('unhide_all_groups');
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

document.getElementById('feedback-btn').addEventListener('click', async () => {
  // Shown in the dialog rather than sent silently — this is the same label that
  // appears against the customer's license activation, so it identifies them.
  let machineLabel = '';
  try { machineLabel = await invoke('get_machine_label'); } catch {}
  const safeMachine = machineLabel
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

  const modal = document.createElement('div');
  modal.className = 'winapp-modal';
  modal.innerHTML = `
    <div class="feedback-card">
      <div class="winapp-header">
        <span class="url-step-title">Send Feedback</span>
        <button class="winapp-close" id="fb-close">✕</button>
      </div>
      <div id="fb-body" style="padding: 12px 12px 8px;">
        <p style="font-size:0.78rem; color:#888; margin-bottom:8px;">We read every message. Thank you!</p>
        <textarea class="feedback-textarea" id="fb-text" placeholder="Tell us what you think, report a bug, or suggest a feature..."></textarea>
        <input type="email" class="fb-email" id="fb-email" autocomplete="email" spellcheck="false"
               placeholder="Your email (optional — only if you'd like a reply)" />
        <label class="fb-consent">
          <input type="checkbox" id="fb-include-config" />
          <span>
            <strong>Include my setup for troubleshooting</strong>
            <span class="fb-consent-note">Sends your groups, items and settings so we can reproduce the problem. No license key, and no list of your installed software.</span>
          </span>
        </label>
        ${safeMachine ? `<p class="fb-machine">Sent with this message: <code>${safeMachine}</code></p>` : ''}
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

  const submitBtn = document.getElementById('fb-submit');
  submitBtn.addEventListener('click', async () => {
    const text = document.getElementById('fb-text').value.trim();
    if (!text) return;
    const includeConfig = document.getElementById('fb-include-config').checked;
    const replyEmail = document.getElementById('fb-email').value.trim();

    // Attaching a config makes this a slow enough round-trip that the old
    // fire-and-close left people unsure whether anything had been sent.
    submitBtn.disabled = true;
    submitBtn.textContent = 'Sending...';
    try {
      await invoke('send_feedback', { message: text, includeConfig, replyEmail });
      submitBtn.textContent = 'Sent ✓';
      setTimeout(close, 700);
    } catch (err) {
      console.error(err);
      submitBtn.disabled = false;
      submitBtn.textContent = 'Retry';
      const body = modal.querySelector('#fb-body');
      let errEl = body.querySelector('.fb-error');
      if (!errEl) {
        errEl = document.createElement('p');
        errEl.className = 'fb-error';
        body.appendChild(errEl);
      }
      errEl.textContent = "Couldn't send — check your connection and try again.";
    }
  });

  document.getElementById('fb-text').focus();
});

init();
