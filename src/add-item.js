import { emitTo } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

// This window only chooses the item TYPE. The actual pickers (Windows Apps
// list, bookmark browser, file/folder dialogs, Steam library) all live in the
// config window and operate on its in-memory item list, so the chosen type is
// sent back there rather than duplicating any of that here.
const params = new URLSearchParams(window.location.search);
const targetLabel = decodeURIComponent(params.get('target') || '');

const targetEl = document.getElementById('target');
targetEl.textContent = targetLabel ? `Adding to ${targetLabel}` : '';

document.querySelectorAll('.type-btn').forEach(btn => {
  btn.addEventListener('click', async () => {
    await emitTo('config', 'add-item-type', { type: btn.dataset.type });
    await getCurrentWindow().close();
  });
});

// Cancelling has to tell the config window too, so it can drop the pending
// desktop assignment instead of applying it to whatever gets added next.
async function cancel() {
  await emitTo('config', 'add-item-cancel', {});
  await getCurrentWindow().close();
}

document.getElementById('cancel').addEventListener('click', cancel);
window.addEventListener('keydown', (e) => { if (e.key === 'Escape') cancel(); });
