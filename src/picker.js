import { invoke } from '@tauri-apps/api/core';

let monitors = [];
let picking = true;

async function init() {
  try {
    monitors = await invoke('get_monitors');
  } catch (_) {
    monitors = [];
  }
}

function getDisplayName(x, y) {
  for (const m of monitors) {
    if (x >= m.x && x < m.x + m.width && y >= m.y && y < m.y + m.height) {
      return m.name;
    }
  }
  return 'Display 1';
}

const tooltip = document.getElementById('tooltip');
const hint = document.getElementById('hint');

document.addEventListener('mousemove', (e) => {
  const x = Math.round(e.screenX);
  const y = Math.round(e.screenY);
  tooltip.textContent = `x: ${x}, y: ${y} · ${getDisplayName(x, y)}`;
  tooltip.style.display = 'block';
  tooltip.style.left = (e.clientX + 18) + 'px';
  tooltip.style.top = (e.clientY + 12) + 'px';
});

document.addEventListener('click', async (e) => {
  if (!picking) return;
  picking = false;
  const x = Math.round(e.screenX);
  const y = Math.round(e.screenY);
  hint.textContent = `Picked: x: ${x}, y: ${y}`;
  try {
    await invoke('finish_location_picker', { x, y });
  } catch (err) {
    console.error('finish_location_picker failed:', err);
    picking = true;
    hint.textContent = 'Click failed — try again · Esc to cancel';
  }
});

document.addEventListener('keydown', async (e) => {
  if (e.key === 'Escape') {
    picking = false;
    try {
      await invoke('cancel_location_picker');
    } catch (_) {}
  }
});

init();
