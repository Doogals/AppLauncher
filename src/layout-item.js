import { invoke } from '@tauri-apps/api/core';

const params = new URLSearchParams(window.location.search);
const name = decodeURIComponent(params.get('name') || 'Item');
const total = parseInt(params.get('total') || '1', 10);

document.getElementById('pk-name').textContent = name;

const labels = Array.from({ length: total }, (_, i) => `layout-item-${i}`);

// Rust handles: collect positions → emit layout-save to all windows → close all layout windows
document.getElementById('pk-save').addEventListener('click', async () => {
  await invoke('complete_layout_save', { labels });
});

// Rust handles: emit layout-cancel to all windows → close all layout windows
document.getElementById('pk-cancel').addEventListener('click', async () => {
  await invoke('complete_layout_cancel', { labels });
});
