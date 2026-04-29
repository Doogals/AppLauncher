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

// ── Widget background right-click menu ───────────────────────────────────────
const COLORS = [
  { label: 'Default',    value: 'rgba(22,33,62,0.95)' },
  { label: 'Charcoal',   value: 'rgba(30,30,30,0.95)' },
  { label: 'Forest',     value: 'rgba(15,40,25,0.95)' },
  { label: 'Midnight',   value: 'rgba(20,10,40,0.95)' },
  { label: 'Rust',       value: 'rgba(60,25,10,0.95)' },
  { label: 'Steel',      value: 'rgba(20,30,45,0.95)' },
];

function applyWidgetColor(color) {
  document.querySelector('.widget').style.background = color;
}

function removeContextMenu() {
  document.getElementById('widget-ctx-menu')?.remove();
}

widget.addEventListener('contextmenu', (e) => {
  if (e.target.closest('.group-btn')) return; // group buttons handle their own menu
  e.preventDefault();
  removeContextMenu();

  const menu = document.createElement('div');
  menu.id = 'widget-ctx-menu';
  menu.style.cssText = `
    position:fixed; left:${e.clientX}px; top:${e.clientY}px;
    background:#16213e; border:1px solid #0f3460; border-radius:6px;
    padding:4px 0; z-index:9999; min-width:160px; box-shadow:0 4px 16px rgba(0,0,0,0.5);
    font-family:inherit; font-size:13px;
  `;

  // ── Change Color submenu trigger ──
  const colorItem = document.createElement('div');
  colorItem.style.cssText = 'padding:6px 14px; color:#e0e0e0; cursor:pointer; display:flex; justify-content:space-between; align-items:center;';
  colorItem.innerHTML = '🎨 Change Color <span style="color:#888;font-size:11px;">▶</span>';
  colorItem.addEventListener('mouseenter', (ev) => {
    colorItem.style.background = 'rgba(15,52,96,0.8)';
    document.getElementById('widget-color-sub')?.remove();

    const sub = document.createElement('div');
    sub.id = 'widget-color-sub';
    const rect = colorItem.getBoundingClientRect();
    sub.style.cssText = `
      position:fixed; left:${rect.right + 4}px; top:${rect.top}px;
      background:#16213e; border:1px solid #0f3460; border-radius:6px;
      padding:6px 8px; z-index:10000; box-shadow:0 4px 16px rgba(0,0,0,0.5);
    `;

    // Swatches
    const swatches = document.createElement('div');
    swatches.style.cssText = 'display:grid; grid-template-columns:1fr 1fr; gap:4px; margin-bottom:6px;';
    COLORS.forEach(({ label, value }) => {
      const sw = document.createElement('div');
      sw.style.cssText = `background:${value}; border:1px solid #0f3460; border-radius:4px; padding:4px 8px; cursor:pointer; font-size:11px; color:#e0e0e0; text-align:center;`;
      sw.textContent = label;
      sw.addEventListener('click', () => {
        applyWidgetColor(value);
        invoke('save_widget_color', { color: value });
        removeContextMenu();
        document.getElementById('widget-color-sub')?.remove();
      });
      sw.addEventListener('mouseenter', () => sw.style.borderColor = '#e07b39');
      sw.addEventListener('mouseleave', () => sw.style.borderColor = '#0f3460');
      swatches.appendChild(sw);
    });
    sub.appendChild(swatches);

    // Custom color picker
    const customRow = document.createElement('div');
    customRow.style.cssText = 'display:flex; align-items:center; gap:6px; padding-top:4px; border-top:1px solid #0f3460;';
    const picker = document.createElement('input');
    picker.type = 'color';
    picker.value = '#16213e';
    picker.style.cssText = 'width:28px; height:22px; border:none; border-radius:4px; cursor:pointer; padding:0;';
    const pickerLabel = document.createElement('span');
    pickerLabel.style.cssText = 'font-size:11px; color:#aaa;';
    pickerLabel.textContent = 'Custom';
    picker.addEventListener('input', () => {
      const hex = picker.value;
      const r = parseInt(hex.slice(1,3),16), g = parseInt(hex.slice(3,5),16), b = parseInt(hex.slice(5,7),16);
      const color = `rgba(${r},${g},${b},0.95)`;
      applyWidgetColor(color);
      invoke('save_widget_color', { color });
    });
    picker.addEventListener('change', () => removeContextMenu());
    customRow.appendChild(picker);
    customRow.appendChild(pickerLabel);
    sub.appendChild(customRow);

    document.body.appendChild(sub);
  });
  colorItem.addEventListener('mouseleave', (ev) => {
    if (!ev.relatedTarget?.closest('#widget-color-sub')) {
      colorItem.style.background = '';
    }
  });
  menu.appendChild(colorItem);

  // Divider
  const divider = document.createElement('div');
  divider.style.cssText = 'height:1px; background:#0f3460; margin:2px 0;';
  menu.appendChild(divider);

  // ── Close ──
  const closeItem = document.createElement('div');
  closeItem.style.cssText = 'padding:6px 14px; color:#e94560; cursor:pointer;';
  closeItem.textContent = '✕  Close';
  closeItem.addEventListener('mouseenter', () => closeItem.style.background = 'rgba(233,69,96,0.15)');
  closeItem.addEventListener('mouseleave', () => closeItem.style.background = '');
  closeItem.addEventListener('click', () => getCurrentWindow().close());
  menu.appendChild(closeItem);

  document.body.appendChild(menu);
});

// Dismiss menu on click outside
document.addEventListener('mousedown', (e) => {
  if (!e.target.closest('#widget-ctx-menu') && !e.target.closest('#widget-color-sub')) {
    removeContextMenu();
    document.getElementById('widget-color-sub')?.remove();
  }
});

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
      invoke('show_group_context_menu', { groupId: group.id })
        .catch(err => console.error('Context menu error:', err));
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
listen('update-available', (e) => {
  const version = e.payload;
  const banner = document.createElement('div');
  banner.style.cssText = 'position:fixed;bottom:0;left:0;right:0;background:#e07b39;color:#fff;font-size:11px;padding:4px 8px;display:flex;justify-content:space-between;align-items:center;z-index:9999;';
  banner.innerHTML = `<span>v${version} available</span><a href="https://github.com/Doogals/AppLauncher/releases/latest" target="_blank" style="color:#fff;font-weight:700;text-decoration:underline;">Download</a>`;
  document.body.appendChild(banner);
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
