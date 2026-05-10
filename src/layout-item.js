import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);

document.getElementById('pk-name').textContent = name;

// Derive all window labels from the deterministic pattern
const labels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);

async function closeAll() {
  for (const label of labels) {
    try {
      const win = WebviewWindow.getByLabel(label);
      if (win) await win.close();
    } catch {}
  }
}

document.getElementById('pk-save').addEventListener('click', async () => {
  const positions = await invoke('get_all_layout_positions', { labels });
  await emit('layout-save', { positions });
  await closeAll();
});

document.getElementById('pk-cancel').addEventListener('click', async () => {
  await emit('layout-cancel');
  await closeAll();
});
