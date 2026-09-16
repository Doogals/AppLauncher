import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

const listEl   = document.getElementById('list');
const countEl  = document.getElementById('count');
const addBtn   = document.getElementById('add');

// hwnd -> captured window, for the ones the user has ticked.
let captured = [];
const selected = new Set();

// Items already in the group, sent over when this window opens. Used to flag
// duplicates so pressing Capture twice doesn't silently double everything.
let existingPaths = new Set();

function esc(s) {
  return String(s ?? '')
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function desktopLabel(win, desktopIndexByGuid) {
  if (!win.launch_virtual_desktop) return null;
  const key = win.launch_virtual_desktop.join(',');
  const idx = desktopIndexByGuid.get(key);
  return idx === undefined ? null : `Desktop ${idx + 1}`;
}

function metaFor(win, desktopIndexByGuid) {
  const bits = [];
  const desktop = desktopLabel(win, desktopIndexByGuid);
  if (desktop) bits.push(desktop);
  if (win.launch_width && win.launch_height) {
    bits.push(`${win.launch_width}×${win.launch_height} at ${win.launch_x}, ${win.launch_y}`);
  }
  if (!bits.length && win.path) bits.push(win.path);
  return bits.join('  •  ');
}

function refreshCount() {
  const n = selected.size;
  countEl.textContent = n === 1 ? '1 selected' : `${n} selected`;
  addBtn.disabled = n === 0;
}

// Kept so render() can be re-run when the duplicate list arrives after the
// first paint — see the capture-existing-paths listener.
let lastDesktopMap = new Map();

function render(desktopIndexByGuid = lastDesktopMap) {
  lastDesktopMap = desktopIndexByGuid;
  listEl.innerHTML = '';
  // Rebuilt from scratch below, so the previous run's ticks are stale.
  selected.clear();

  if (!captured.length) {
    listEl.innerHTML =
      '<p class="empty">No windows found to capture.<br />Open the apps you want, then press Rescan.</p>';
    refreshCount();
    return;
  }

  captured.forEach(win => {
    const blocked   = Boolean(win.blocked_reason);
    const duplicate = !blocked && win.path && existingPaths.has(win.path.toLowerCase());

    const row = document.createElement('label');
    row.className = 'row' + (blocked ? ' blocked' : '');

    const notes = [];
    if (blocked) {
      notes.push(`<div class="note blocked-note">${esc(win.blocked_reason)}</div>`);
    } else {
      // Said out loud rather than silently producing a browser item that opens
      // a blank window — a window title is the page title, never the URL.
      if (win.is_browser) {
        notes.push('<div class="note">Open tabs are not captured — add URLs after.</div>');
      }
      if (duplicate) {
        notes.push('<div class="note">Already in this group.</div>');
      }
    }

    row.innerHTML = `
      <input type="checkbox" ${blocked ? 'disabled' : ''} />
      <span class="body">
        <span class="name">${esc(win.display_name)}</span>
        <span class="meta">${esc(win.title || metaFor(win, desktopIndexByGuid))}</span>
        ${win.title ? `<span class="meta">${esc(metaFor(win, desktopIndexByGuid))}</span>` : ''}
        ${notes.join('')}
      </span>
      ${blocked ? '' : `<span class="tag">${win.kind === 'uwp' ? 'Store app' : 'App'}</span>`}
    `;

    const cb = row.querySelector('input');
    // Ticked by default, except the two cases we know are unreliable or
    // unwanted: blocked windows, and ones already in the group.
    if (!blocked && !duplicate) {
      cb.checked = true;
      selected.add(win.hwnd);
    }
    cb.addEventListener('change', () => {
      if (cb.checked) selected.add(win.hwnd);
      else selected.delete(win.hwnd);
      refreshCount();
    });

    listEl.appendChild(row);
  });

  refreshCount();
}

async function scan() {
  listEl.innerHTML = '<p class="loading">Scanning open windows…</p>';

  // Desktop GUIDs mean nothing to a person; map them to the same 1-based
  // numbering the group editor's Desktop dropdown uses.
  const desktopIndexByGuid = new Map();
  try {
    const desktops = await invoke('get_virtual_desktops');
    desktops.forEach((d, i) => desktopIndexByGuid.set(d.guid.join(','), i));
  } catch { /* no desktop info — rows just omit it */ }

  try {
    captured = await invoke('capture_current_windows');
  } catch (err) {
    console.error(err);
    captured = [];
  }
  render(desktopIndexByGuid);
}

document.getElementById('select-all').addEventListener('click', () => {
  listEl.querySelectorAll('input:not(:disabled)').forEach(cb => {
    cb.checked = true;
    cb.dispatchEvent(new Event('change'));
  });
});
document.getElementById('select-none').addEventListener('click', () => {
  listEl.querySelectorAll('input:not(:disabled)').forEach(cb => {
    cb.checked = false;
    cb.dispatchEvent(new Event('change'));
  });
});
document.getElementById('rescan').addEventListener('click', scan);

addBtn.addEventListener('click', async () => {
  const chosen = captured.filter(w => selected.has(w.hwnd));
  await emitTo('config', 'capture-add-items', { items: chosen });
  await getCurrentWindow().close();
});

async function cancel() {
  await getCurrentWindow().close();
}
document.getElementById('cancel').addEventListener('click', cancel);
window.addEventListener('keydown', e => { if (e.key === 'Escape') cancel(); });

// The config window sends its current item paths so duplicates can be flagged.
// The listener must be registered BEFORE announcing we're ready — listen() is
// async, and config replies immediately, so emitting first loses the reply.
async function init() {
  await listen('capture-existing-paths', e => {
    existingPaths = new Set((e.payload?.paths || []).map(p => String(p).toLowerCase()));
    // emitTo resolves when the event is SENT, not when config answers, so this
    // reply can land after the first render. Re-render rather than leaving
    // duplicates unflagged — which would let a second Capture silently add
    // everything twice.
    if (captured.length) render();
  });
  await emitTo('config', 'capture-window-ready', {});
  await scan();
}

init();
