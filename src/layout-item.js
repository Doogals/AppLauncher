import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);

document.getElementById('pk-name').textContent = name;

const labels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);

async function initDesktopDropdown() {
  const sel = document.getElementById('pk-desktop-sel');
  let desktops = [];
  try {
    desktops = await invoke('get_virtual_desktops');
  } catch { return; }

  if (desktops.length <= 1) {
    document.getElementById('pk-desktop-row').style.display = 'none';
    return;
  }

  for (const vd of desktops) {
    const opt = document.createElement('option');
    opt.value = JSON.stringify(vd.guid);
    opt.textContent = vd.name;
    sel.appendChild(opt);
  }

  try {
    const currentGuid = await invoke('get_current_window_desktop');
    if (currentGuid) {
      const currentStr = JSON.stringify(currentGuid);
      for (const opt of sel.options) {
        if (opt.value === currentStr) { opt.selected = true; break; }
      }
    }
  } catch {}

  sel.addEventListener('change', async () => {
    if (!sel.value) return;
    const guid = JSON.parse(sel.value);
    const label = getCurrentWindow().label;
    try {
      await invoke('move_layout_window_to_desktop', { label, guid });
    } catch {}
  });
}

initDesktopDropdown();

document.getElementById('pk-save').addEventListener('click', async () => {
  await invoke('complete_layout_save', { labels });
});

document.getElementById('pk-cancel').addEventListener('click', async () => {
  await invoke('complete_layout_cancel', { labels });
});
