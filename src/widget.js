import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

const widget = document.getElementById('widget');

// Drag the window by clicking the widget background (left-click only, not on buttons)
widget.addEventListener('mousedown', (e) => {
  if (e.button === 0 && !e.target.closest('.group-btn')) {
    getCurrentWindow().startDragging();
  }
});

// ── Widget background right-click menu (native) ──────────────────────────────
function applyWidgetColor(color) {
  document.querySelector('.widget').style.background = color;
}

let menuCooldown = false;
function showMenuThrottled(fn) {
  if (menuCooldown) return;
  menuCooldown = true;
  fn();
  setTimeout(() => { menuCooldown = false; }, 600);
}

widget.addEventListener('contextmenu', (e) => {
  if (e.target.closest('.group-btn')) return;
  e.preventDefault();
  showMenuThrottled(() =>
    invoke('show_widget_context_menu').catch(err => console.error('Context menu error:', err))
  );
});

listen('widget-color-changed', (e) => applyWidgetColor(e.payload));

const GAP   = 8;
const PAD   = 24;
const WIN_H = 80;

async function render() {
  const config = await invoke('get_config');
  widget.innerHTML = '';

  for (const group of config.groups) {
    const btn = document.createElement('div');
    btn.className = 'group-btn';
    btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
    btn.addEventListener('click', () => launchGroup(group.id));
    btn.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showMenuThrottled(() =>
        invoke('show_group_context_menu', { groupId: group.id })
          .catch(err => console.error('Context menu error:', err))
      );
    });
    widget.appendChild(btn);
  }

  const addBtn = document.createElement('div');
  addBtn.className = 'group-btn add-btn';
  addBtn.textContent = '+';
  addBtn.addEventListener('click', () => openConfig(null));
  widget.appendChild(addBtn);

  // Measure actual rendered button widths instead of using hardcoded estimates
  await new Promise(resolve => requestAnimationFrame(resolve));
  const children = [...widget.children];
  let w = PAD;
  children.forEach((child, i) => {
    w += child.offsetWidth;
    if (i < children.length - 1) w += GAP;
  });

  await invoke('resize_widget', { width: Math.ceil(w), height: WIN_H });
}

async function launchGroup(groupId) {
  try {
    await invoke('launch_group', { groupId });
  } catch (e) {
    console.error('Launch failed:', e);
  }
}

async function openConfig(groupId) {
  const win = new WebviewWindow('config', {
    url: groupId ? `config.html?id=${groupId}` : 'config.html',
    title: groupId ? 'Edit Group' : 'New Group',
    width: 420,
    height: 460,
    center: true,
    decorations: true,
    resizable: false,
    alwaysOnTop: true,
  });
  win.once('tauri://destroyed', () => render());
}

async function deleteGroup(groupId) {
  await invoke('delete_group', { groupId });
  render();
}

// Listen for native context menu selections
listen('context-menu:edit',   (e) => openConfig(e.payload));
listen('context-menu:delete', (e) => deleteGroup(e.payload));

// Show update notification banner when a new version is available
listen('update-available', () => {
  const btn = document.createElement('div');
  btn.style.cssText = 'position:fixed;bottom:0;left:0;right:0;background:#e07b39;color:#fff;font-size:11px;font-weight:700;padding:3px 0;text-align:center;cursor:pointer;z-index:9999;';
  btn.textContent = '⬆ Update';
  btn.addEventListener('click', () => {
    btn.textContent = 'Downloading…';
    btn.style.cursor = 'default';
    invoke('download_and_install_update').catch(() => {
      btn.textContent = '⬆ Update';
      btn.style.cursor = 'pointer';
    });
  });
  document.body.appendChild(btn);
});

// Position saving after render + restore saved color
render().then(async () => {
  const config = await invoke('get_config');
  if (config.widget_color) applyWidgetColor(config.widget_color);
  let t = null;
  getCurrentWindow().onMoved(({ payload: { x, y } }) => {
    clearTimeout(t);
    t = setTimeout(() => invoke('save_widget_position', { x, y }), 400);
  });
}).catch(e => console.error('Widget init error:', e));
