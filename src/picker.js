import { invoke } from '@tauri-apps/api/core';

let monitors = [];
const dpr = window.devicePixelRatio || 1;

async function init() {
  try {
    monitors = await invoke('get_monitors');
  } catch (_) {
    monitors = [];
  }
}

function getDisplayName(px, py) {
  for (const m of monitors) {
    if (px >= m.x && px < m.x + m.width && py >= m.y && py < m.y + m.height) {
      return m.name;
    }
  }
  return 'Display 1';
}

const tooltip = document.getElementById('tooltip');

document.addEventListener('mousemove', (e) => {
  const px = Math.round(e.screenX * dpr);
  const py = Math.round(e.screenY * dpr);
  tooltip.textContent = `x: ${px}, y: ${py} · ${getDisplayName(px, py)}`;
  tooltip.style.display = 'block';
  tooltip.style.left = (e.clientX + 18) + 'px';
  tooltip.style.top = (e.clientY + 12) + 'px';
});

document.addEventListener('click', async (e) => {
  const px = Math.round(e.screenX * dpr);
  const py = Math.round(e.screenY * dpr);
  await invoke('finish_location_picker', { x: px, y: py });
});

document.addEventListener('keydown', async (e) => {
  if (e.key === 'Escape') {
    await invoke('cancel_location_picker');
  }
});

init();
