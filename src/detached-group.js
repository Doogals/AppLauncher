import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

const params  = new URLSearchParams(window.location.search);
const groupId = params.get('id');
const widget  = document.getElementById('widget');

// 4px padding on each side of the transparent wrapper (see detached-group.html).
const PAD = 0;

// Drag the floating pill by clicking anywhere on it (not on the button itself
// when it's a click — mousedown is fine since startDragging takes over).
widget.addEventListener('mousedown', (e) => {
  if (e.button === 0) getCurrentWindow().startDragging();
});

widget.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  invoke('show_detached_group_context_menu', { groupId }).catch(() => {});
});

async function render() {
  const config = await invoke('get_config');
  const group  = config.groups.find(g => g.id === groupId);
  // Group was deleted — Rust destroys the window, but guard here too.
  if (!group) return;

  widget.innerHTML = '';

  const btn = document.createElement('div');
  btn.className = 'group-btn';
  if (group.color) btn.style.setProperty('--group-color', group.color);
  btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
  btn.addEventListener('click', () => {
    invoke('launch_group', { groupId: group.id }).catch(() => {});
  });
  widget.appendChild(btn);

  // Size the window to fit the single button (same math as widget.js render).
  await new Promise(resolve => requestAnimationFrame(resolve));
  const w = PAD + btn.offsetWidth;
  const h = btn.offsetHeight;
  await invoke('resize_detached_group', { groupId, width: Math.ceil(w), height: Math.ceil(h) })
    .catch(() => {});
}

render();

// Persist position when the window is moved (debounced, physical pixels).
let moveTimer = null;
getCurrentWindow().onMoved(({ payload: { x, y } }) => {
  clearTimeout(moveTimer);
  moveTimer = setTimeout(() => {
    invoke('save_detached_position', { groupId, x, y }).catch(() => {});
  }, 400);
});

// Re-render when anything changes (name, icon, color).
listen('groups-updated', () => render());
