import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';

const EMOJIS = [
  '💼','📁','🗂️','🖥️','🌐','📧','📅','📝','🔧','⚙️',
  '🚀','🎮','🎵','🎬','📷','💰','🏠','🏢','📚','🔬',
  '🧪','🛒','🤝','📊','📈','⚡','🔒','🛡️','📌','🔗',
  '💡','🎯','🧩','🐍','🦀','🌙','☀️','🔔','📣','🗺️',
  '🎨','🖊️','📦','🧰','🖱️',
];

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
    const filtered = filter
      ? apps.filter(a => a.name.toLowerCase().includes(filter.toLowerCase()))
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
      row.addEventListener('click', () => {
        if (!currentItems.some(i => i.path === app.path)) {
          currentItems.push({ item_type: 'app', path: app.path, value: app.args || null });
        }
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

async function showUrlPicker() {
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
  const closeModal = () => {
    document.removeEventListener('keydown', onKeyDown);
    modal.remove();
  };
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
    row.addEventListener('click', () => showBookmarkStep(modal, browser, closeModal));
    browserList.appendChild(row);
  });
}

async function showBookmarkStep(modal, browser, closeModal) {
  const card = modal.querySelector('.winapp-card');
  const safeBrowserName = browser.name.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  card.innerHTML = `
    <div class="winapp-header">
      <button class="url-back-btn" id="url-back">←</button>
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
      <button class="btn btn-save" id="add-selected-btn" disabled>Add Selected</button>
    </div>
  `;

  document.getElementById('url-back').addEventListener('click', () => {
    closeModal();
    showUrlPicker();
  });
  document.getElementById('url-close2').addEventListener('click', closeModal);

  const customInput = document.getElementById('custom-url-input');
  const addBtn = document.getElementById('add-selected-btn');

  function updateAddBtn() {
    const checkedCount = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none').length;
    const hasCustom = customInput.value.trim().length > 0;
    const total = checkedCount + (hasCustom ? 1 : 0);
    addBtn.disabled = total === 0;
    addBtn.textContent = total > 0 ? `Add ${total} Selected` : 'Add Selected';
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
      label.querySelector('.bookmark-checkbox').dataset.url = bm.url;
      label.querySelector('.bookmark-checkbox').addEventListener('change', updateAddBtn);
      list.appendChild(label);
    });
  }

  // Search input filters the bookmark list
  document.getElementById('bookmark-search').addEventListener('input', (e) => {
    const q = e.target.value.trim().toLowerCase();
    modal.querySelectorAll('.bookmark-row').forEach(row => {
      const title = row.querySelector('.bookmark-title')?.textContent.toLowerCase() || '';
      const url   = row.querySelector('.bookmark-url')?.textContent.toLowerCase() || '';
      row.style.display = (!q || title.includes(q) || url.includes(q)) ? '' : 'none';
    });
    updateAddBtn();
  });

  // Custom URL input only affects the Add button state — no list filtering
  customInput.addEventListener('input', updateAddBtn);

  addBtn.addEventListener('click', () => {
    const checked = [...modal.querySelectorAll('.bookmark-checkbox:checked')]
      .filter(cb => cb.closest('.bookmark-row')?.style.display !== 'none');
    checked.forEach(cb => {
      const url = cb.dataset.url;
      if (url && !currentItems.some(i => i.value === url)) {
        currentItems.push({ item_type: 'url', path: browser.path, value: url });
      }
      cb.checked = false;
    });
    const customUrl = customInput.value.trim();
    if (customUrl && !currentItems.some(i => i.value === customUrl)) {
      currentItems.push({ item_type: 'url', path: browser.path, value: customUrl });
    }
    customInput.value = '';
    renderItems();
    closeModal();
  });

  customInput.focus();
}

async function fitWindow() {
  await new Promise(resolve => requestAnimationFrame(resolve));
  const h = document.querySelector('.config-window').offsetHeight;
  await getCurrentWindow().setSize(new LogicalSize(420, h));
}

async function init() {
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
      renderItems();
    }
  }
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
}

async function showPickerOverlay(idx) {
  const win = getCurrentWindow();
  await win.setResizable(true);

  const overlay = document.createElement('div');
  overlay.id = 'picker-overlay';
  overlay.innerHTML = `
    <div id="pk-body">
      <div id="pk-cross">&#x2316;</div>
      <div id="pk-hint">Move &amp; resize this window to set the launch position and size<br><small>Drag the title bar to move &nbsp;&bull;&nbsp; drag edges to resize</small></div>
      <div id="pk-size">-- &times; --</div>
    </div>
    <div id="pk-footer">
      <button id="pk-cancel">Cancel</button>
      <button id="pk-set">Confirm Position &amp; Size</button>
    </div>
  `;
  document.body.appendChild(overlay);

  function updateSize() {
    const el = document.getElementById('pk-size');
    if (el) el.textContent = window.innerWidth + ' \xd7 ' + window.innerHeight;
  }
  updateSize();
  window.addEventListener('resize', updateSize);

  let unlistenClose = null;

  function cleanup(save) {
    window.removeEventListener('resize', updateSize);
    if (unlistenClose) unlistenClose();
    overlay.remove();
    win.setResizable(false);
    if (save) renderItems();
  }

  return new Promise(async (resolve) => {
    unlistenClose = await win.onCloseRequested((event) => {
      event.preventDefault();
      cleanup(false);
      resolve();
    });

    document.getElementById('pk-set').addEventListener('click', async () => {
      const pos = await win.outerPosition();
      const size = await win.innerSize();
      currentItems[idx].launch_x = pos.x;
      currentItems[idx].launch_y = pos.y;
      currentItems[idx].launch_width = size.width;
      currentItems[idx].launch_height = size.height;
      cleanup(true);
      resolve();
    });

    document.getElementById('pk-cancel').addEventListener('click', () => {
      cleanup(false);
      resolve();
    });
  });
}

function buildExpandPanel(item, idx) {
  const panel = document.createElement('div');
  panel.className = 'item-expand';
  const hasCoord = item.launch_x != null && item.launch_y != null;
  const hasSize = item.launch_width != null && item.launch_height != null;
  const coordText = hasCoord
    ? `x:${item.launch_x} y:${item.launch_y}${hasSize ? `  ${item.launch_width}\xd7${item.launch_height}` : ''}`
    : 'not set';
  panel.innerHTML = `
    <div class="item-expand-row">
      <span>Launch at</span>
      <span class="coord-display${hasCoord ? '' : ' coord-empty'}">${coordText}</span>
      ${hasCoord ? '<button class="coord-clear" title="Clear">✕</button>' : ''}
      <button class="pick-btn">&#x1f4cd; Pick</button>
    </div>
  `;

  const clearBtn = panel.querySelector('.coord-clear');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      currentItems[idx].launch_x = null;
      currentItems[idx].launch_y = null;
      currentItems[idx].launch_width = null;
      currentItems[idx].launch_height = null;
      renderItems();
    });
  }

  panel.querySelector('.pick-btn').addEventListener('click', () => showPickerOverlay(idx));

  return panel;
}

function renderItems() {
  const list = document.getElementById('items-list');
  list.innerHTML = '';

  currentItems.forEach((item, idx) => {
    const wrapper = document.createElement('div');

    const row = document.createElement('div');
    row.className = 'item-row';
    const rawLabel = item.item_type === 'url' ? item.value : item.path;
    const safeLabel = (rawLabel || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const typeIcon = { app: '🖥️', file: '📄', url: '🌐', folder: '📁', script: '⚡' }[item.item_type] || '•';
    row.innerHTML = `
      <span>${typeIcon}</span>
      <span class="item-label" title="${safeLabel}">${safeLabel}</span>
      <span class="item-chevron" title="Launch targeting">›</span>
      <button class="remove-btn">✕</button>
    `;

    let expandEl = null;
    row.querySelector('.item-chevron').addEventListener('click', () => {
      const chevron = row.querySelector('.item-chevron');
      if (expandEl) {
        expandEl.remove();
        expandEl = null;
        chevron.classList.remove('open');
        row.classList.remove('expanded');
      } else {
        chevron.classList.add('open');
        row.classList.add('expanded');
        expandEl = buildExpandPanel(item, idx);
        wrapper.appendChild(expandEl);
      }
      fitWindow();
    });

    row.querySelector('.remove-btn').onclick = () => { currentItems.splice(idx, 1); renderItems(); };

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

async function addItem(type) {
  document.getElementById('add-type-menu').style.display = 'none';

  if (type === 'winapp') {
    await showWinAppPicker();
    return;
  }

  if (type === 'url') {
    await showUrlPicker();
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
  currentItems.push({ item_type: type, path: selected, value: null });

  renderItems();
}

document.getElementById('save-btn').onclick = async () => {
  const name = document.getElementById('name-input').value.trim();
  const icon = document.getElementById('icon-input').value.trim() || '📁';
  if (!name) { alert('Please enter a group name.'); return; }

  const group = {
    id: existingGroup?.id ?? crypto.randomUUID(),
    name,
    icon,
    items: currentItems,
  };

  try {
    await invoke('save_group', { group });
    await getCurrentWindow().close();
  } catch (e) {
    alert(e);
  }
};

document.getElementById('cancel-btn').onclick = async () => {
  await getCurrentWindow().close();
};

// Store URL — update after creating your LemonSqueezy product
const STORE_URL = 'https://tonictechapps.lemonsqueezy.com/checkout/buy/e14ee8eb-1a79-42a8-85a7-30aa23e66c61';

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
