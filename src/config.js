import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';

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
    });
    const customUrl = customInput.value.trim();
    if (customUrl && !currentItems.some(i => i.value === customUrl)) {
      currentItems.push({ item_type: 'url', path: browser.path, value: customUrl });
    }
    renderItems();
    closeModal();
  });

  customInput.focus();
}

async function init() {
  initEmojiPicker();

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
  updateLicenseStatus();
}

function renderItems() {
  const list = document.getElementById('items-list');
  list.innerHTML = '';
  currentItems.forEach((item, idx) => {
    const row = document.createElement('div');
    row.className = 'item-row';
    const label = item.item_type === 'url' ? item.value : item.path;
    const typeIcon = { app: '🖥️', file: '📄', url: '🌐', folder: '📁', script: '⚡' }[item.item_type] || '•';
    row.innerHTML = `
      <span>${typeIcon}</span>
      <span class="item-label" title="${label}">${label}</span>
      <button class="remove-btn">✕</button>
    `;
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
      const toIdx = idx;
      if (fromIdx !== toIdx) {
        const [moved] = currentItems.splice(fromIdx, 1);
        currentItems.splice(toIdx, 0, moved);
        renderItems();
      }
    });

    list.appendChild(row);
  });
}

document.getElementById('add-item-btn').onclick = () => {
  const menu = document.getElementById('add-type-menu');
  menu.style.display = menu.style.display === 'none' ? 'block' : 'none';
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

async function updateLicenseStatus() {
  const config = await invoke('get_config');
  const status = document.getElementById('license-status');
  if (!status) return;
  if (config.license_key) {
    status.textContent = '✓ Licensed — unlimited groups';
    status.style.color = '#4caf50';
  } else {
    status.textContent = 'Free tier: up to 2 groups';
    status.style.color = '#888';
  }
}

const activateBtn = document.getElementById('activate-btn');
if (activateBtn) {
  activateBtn.onclick = async () => {
    const key = document.getElementById('license-input').value.trim();
    try {
      await invoke('activate_license', { key });
      document.getElementById('license-status').textContent = '✓ Activated!';
      document.getElementById('license-status').style.color = '#4caf50';
    } catch (e) {
      document.getElementById('license-status').textContent = e;
      document.getElementById('license-status').style.color = '#e94560';
    }
  };
}

init();
