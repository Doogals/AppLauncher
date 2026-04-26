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

  if (type === 'url') {
    const url = window.prompt('Enter URL:');
    if (!url) return;

    const config = await invoke('get_config');
    if (!config.preferred_browser) {
      const browser = await open({
        title: 'Select your preferred browser (.exe)',
        filters: [{ name: 'Executable', extensions: ['exe'] }],
      });
      if (browser) await invoke('set_preferred_browser', { path: browser });
    }
    currentItems.push({ item_type: 'url', path: null, value: url });
  } else {
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
  }

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
