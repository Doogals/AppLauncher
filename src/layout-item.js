import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);
const label = getCurrentWindow().label;

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

  // Pre-select the saved desktop; for items with no saved VD, use the current desktop
  // (never default to Desktop 1, which would physically move the window there).
  const vdParam = params.get('vd');
  let savedGuid;
  if (vdParam) {
    savedGuid = JSON.parse(decodeURIComponent(vdParam));
  } else {
    try {
      savedGuid = await invoke('get_current_virtual_desktop_guid');
    } catch { savedGuid = null; }
    if (!savedGuid) savedGuid = desktops[0].guid;
  }
  const savedStr = JSON.stringify(savedGuid);
  for (const opt of sel.options) {
    if (opt.value === savedStr) {
      opt.selected = true;
      break;
    }
  }
  // Always register with Rust so complete_layout_save captures it without requiring a change.
  invoke('set_layout_item_desktop', { label, guid: savedGuid }).catch(() => {});

  sel.addEventListener('change', async () => {
    const guid = JSON.parse(sel.value);
    try {
      await invoke('set_layout_item_desktop', { label, guid });
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
