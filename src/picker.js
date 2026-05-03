import { invoke } from '@tauri-apps/api/core';

const sizeDisplay = document.getElementById('size-display');

function updateSize() {
  sizeDisplay.textContent = `${window.innerWidth} × ${window.innerHeight}`;
}

updateSize();
window.addEventListener('resize', updateSize);

document.getElementById('set-btn').addEventListener('click', async () => {
  try {
    await invoke('finish_location_picker');
  } catch (err) {
    console.error('finish_location_picker failed:', err);
  }
});

async function cancel() {
  try {
    await invoke('cancel_location_picker');
  } catch (_) {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().close();
  }
}

document.getElementById('cancel-btn').addEventListener('click', cancel);
document.getElementById('close-btn').addEventListener('click', cancel);
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') cancel(); });
