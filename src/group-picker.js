import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

const list       = document.getElementById('group-list');
const confirmBtn = document.getElementById('confirm-btn');
let selectedId   = null;

async function init() {
  const groups = await invoke('get_free_tier_enforcement').catch(() => null);
  if (!groups || groups.length === 0) {
    // Nothing to enforce — close immediately (shouldn't happen in normal flow).
    await getCurrentWindow().close();
    return;
  }

  groups.forEach(group => {
    const btn = document.createElement('button');
    btn.className = 'gp-btn';
    btn.innerHTML = `<span class="gp-icon">${group.icon}</span><span>${group.name}</span>`;
    btn.addEventListener('click', () => {
      list.querySelectorAll('.gp-btn').forEach(b => b.classList.remove('selected'));
      btn.classList.add('selected');
      selectedId = group.id;
      confirmBtn.disabled = false;
    });
    list.appendChild(btn);
  });
}

confirmBtn.addEventListener('click', async () => {
  if (!selectedId) return;
  confirmBtn.disabled = true;
  try {
    await invoke('apply_free_tier_groups', { keepGroupId: selectedId });
    await getCurrentWindow().close();
  } catch {
    confirmBtn.disabled = false;
  }
});

init();
