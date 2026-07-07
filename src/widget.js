import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen, emitTo } from '@tauri-apps/api/event';

const widget = document.getElementById('widget');

// Drag the window by clicking the widget background (left-click only, not on buttons)
widget.addEventListener('mousedown', (e) => {
  if (e.button === 0 && !e.target.closest('.group-btn') && !e.target.closest('.widget-close-btn')) {
    getCurrentWindow().startDragging();
  }
});

// Close button — top-right of the widget
document.getElementById('widget-close-btn').addEventListener('click', (e) => {
  e.stopPropagation();
  getCurrentWindow().close();
});

// Parses "rgba(r,g,b,a)"/"rgb(r,g,b)" or "#rgb"/"#rrggbb" into [r,g,b].
function parseColorToRgb(color) {
  if (!color) return null;
  const rgbaMatch = color.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i);
  if (rgbaMatch) {
    return [parseInt(rgbaMatch[1], 10), parseInt(rgbaMatch[2], 10), parseInt(rgbaMatch[3], 10)];
  }
  const hexMatch = color.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hexMatch) {
    let hex = hexMatch[1];
    if (hex.length === 3) hex = hex.split('').map(c => c + c).join('');
    const num = parseInt(hex, 16);
    return [(num >> 16) & 255, (num >> 8) & 255, num & 255];
  }
  return null;
}

function getContrastColor(color, lightVariant, darkVariant) {
  const rgb = parseColorToRgb(color);
  if (!rgb) return lightVariant;
  const [r, g, b] = rgb;
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance > 0.55 ? darkVariant : lightVariant;
}

// ── Widget background color ──────────────────────────────────────────────────
function applyWidgetColor(color) {
  const widget = document.querySelector('.widget');
  widget.style.background = color;
  widget.style.borderColor = color;

  const wordmark = document.getElementById('app-wordmark');
  if (wordmark) {
    const isLight = getContrastColor(color, 'light', 'dark') === 'dark';
    const textEl = wordmark.querySelector('.app-wordmark-text');
    if (textEl) textEl.style.color = isLight ? 'rgba(0,0,0,0.6)' : 'rgba(255,255,255,0.8)';
    const logo = wordmark.querySelector('#app-logo');
    if (logo) logo.src = isLight ? 'takeoff-logo-dark.png' : 'takeoff-logo-light.png';
  }
  const closeBtn = document.getElementById('widget-close-btn');
  if (closeBtn) closeBtn.style.color = getContrastColor(color, 'rgba(255,255,255,0.3)', 'rgba(0,0,0,0.35)');
}

let menuCooldown = false;
function showMenuThrottled(fn) {
  if (menuCooldown) return;
  menuCooldown = true;
  fn();
  setTimeout(() => { menuCooldown = false; }, 600);
}

// ── Drag state (shared by reorder + detach-on-leave) ─────────────────────────
// dragGroupId:  which group is currently grabbed (set on mousedown)
// dragSrcIndex: visual index of that group when the drag started
// dragTargetIndex: current drop target (updated during mousemove)
// isDragging:   true once the cursor has moved ≥ 5 px horizontally
// justDragged:  suppresses the click→launch that would fire after a drag
let dragGroupId     = null;
let dragSrcIndex    = null;
let dragTargetIndex = null;
let isDragging      = false;
let dragStartX      = 0;
let dragGhost       = null;
let dropIndicator   = null;
let justDragged     = false;

// ── Pre-detach state ──────────────────────────────────────────────────────────
// When isDragging activates (≥5 px), we immediately create the floating window
// hidden in the background so it's (usually) ready before the cursor exits.
// pendingDetachGroupId — groupId of the currently pre-creating window.
// preDetachReady       — true once 'detached-group-ready' fires for it.
// pendingCommit        — groupId queued for commit (mouse left before ready).
let pendingDetachGroupId = null;
let preDetachReady       = false;
let pendingCommit        = null;

// Ghost linger: after a detach-on-leave the ghost stays visible until the
// floating window's first render fires 'detached-group-ready'. This prevents
// the blank-screen gap while WebView2 initialises the new window.
let pendingGhostGroupId  = null;
let ghostCleanupTimeout  = null;

function cleanupGhostOnly() {
  if (dragGhost)     { dragGhost.remove();     dragGhost     = null; }
  if (dropIndicator) { dropIndicator.remove(); dropIndicator = null; }
}

function cleanupDrag() {
  isDragging      = false;
  dragGroupId     = null;
  dragSrcIndex    = null;
  dragTargetIndex = null;
  document.body.style.cursor = '';
  cleanupGhostOnly();
  widget.querySelectorAll('.drag-placeholder').forEach(b => b.classList.remove('drag-placeholder'));
}

// Finalises a pre-detach: removes the ghost and tells Rust to reveal the
// hidden floating window at the live cursor position and start a native drag.
function executeCommit(gid) {
  clearTimeout(ghostCleanupTimeout);
  ghostCleanupTimeout  = null;
  pendingGhostGroupId  = null;
  pendingDetachGroupId = null;
  preDetachReady       = false;
  pendingCommit        = null;
  cleanupGhostOnly(); // ghost gone — the window is about to appear at cursor
  invoke('commit_detach', { groupId: gid }).catch(() => {});
}

// ── Reorder: mousemove ────────────────────────────────────────────────────────
document.addEventListener('mousemove', (e) => {
  if (!dragGroupId || e.buttons !== 1) return;

  // Activate drag visuals once cursor moves ≥ 5 px horizontally.
  if (!isDragging && Math.abs(e.clientX - dragStartX) >= 5) {
    isDragging = true;
    document.body.style.cursor = 'grabbing';

    // Pre-create the floating window hidden so it has time to initialise
    // before the cursor leaves the widget (WebView2 takes 150–400 ms).
    pendingDetachGroupId = dragGroupId;
    preDetachReady       = false;
    pendingCommit        = null;
    invoke('pre_detach', { groupId: dragGroupId }).catch(() => {
      pendingDetachGroupId = null; // creation failed; mouseleave will use fallback
    });

    const srcBtn = widget.querySelector(`[data-group-id="${dragGroupId}"]`);
    if (srcBtn) {
      srcBtn.classList.add('drag-placeholder');
      const srcRect = srcBtn.getBoundingClientRect();

      // Ghost: a semi-transparent clone that follows the cursor.
      dragGhost = srcBtn.cloneNode(true);
      dragGhost.removeAttribute('data-group-id');
      dragGhost.style.cssText =
        `position:fixed;top:${srcRect.top}px;pointer-events:none;z-index:9999;` +
        `opacity:0.88;transform:scale(1.06);` +
        `box-shadow:0 6px 24px rgba(0,0,0,0.55);border-radius:8px;`;
      document.body.appendChild(dragGhost);

      // Drop indicator: amber vertical bar between buttons.
      dropIndicator = document.createElement('div');
      dropIndicator.style.cssText =
        `position:fixed;width:3px;top:${srcRect.top}px;height:${srcRect.height}px;` +
        `background:rgba(255,200,80,0.95);border-radius:2px;` +
        `pointer-events:none;z-index:9998;box-shadow:0 0 8px rgba(255,200,80,0.55);`;
      document.body.appendChild(dropIndicator);
    }
  }

  if (!isDragging) return;

  // Move ghost to follow cursor (centred horizontally on it).
  if (dragGhost) {
    dragGhost.style.left = (e.clientX - dragGhost.offsetWidth / 2) + 'px';
  }

  // Collect rects of every non-dragged, non-add group button.
  const btns = [...widget.querySelectorAll('.group-btn:not(.add-btn):not(.drag-placeholder)')]
    .map(b => ({ rect: b.getBoundingClientRect() }));

  // Find target index: the gap that the cursor is currently in.
  let target = btns.length;
  for (let i = 0; i < btns.length; i++) {
    if (e.clientX < btns[i].rect.left + btns[i].rect.width / 2) { target = i; break; }
  }
  dragTargetIndex = target;

  // Position the drop indicator in the chosen gap.
  if (dropIndicator) {
    let x;
    if      (btns.length === 0)      x = e.clientX;
    else if (target === 0)           x = btns[0].rect.left - 5;
    else if (target >= btns.length)  x = btns[btns.length - 1].rect.right + 2;
    else x = (btns[target - 1].rect.right + btns[target].rect.left) / 2 - 1.5;
    dropIndicator.style.left = x + 'px';
  }
});

// ── Reorder: mouseup ──────────────────────────────────────────────────────────
document.addEventListener('mouseup', () => {
  // External drop: user released the mouse while a floating group was hovering
  // over the widget. Attach it immediately. This covers the case where the
  // cursor lands outside the floating pill (on the widget bar itself) when the
  // user lets go — the widget's mouseup fires instead of the floating window's.
  if (hoverCount > 0 && extDropGroupId) {
    const gid = extDropGroupId;
    const idx = extDropTargetIdx >= 0 ? extDropTargetIdx : undefined;
    // Reset hover tracking before attach_group destroys the window.
    hoverCount       = 0;
    extDropGroupId   = null;
    extDropTargetIdx = -1;
    invoke('attach_group', { groupId: gid, insertAt: idx }).catch(() => {});
    cleanupDrag();
    return;
  }

  // If the drag ended inside the widget, destroy the pre-created hidden window.
  if (isDragging && pendingDetachGroupId) {
    invoke('cancel_detach', { groupId: pendingDetachGroupId }).catch(() => {});
    pendingDetachGroupId = null;
    preDetachReady       = false;
    pendingCommit        = null;
    clearTimeout(ghostCleanupTimeout);
    ghostCleanupTimeout  = null;
  }
  if (isDragging && dragGroupId && dragTargetIndex !== null && dragTargetIndex !== dragSrcIndex) {
    justDragged = true;
    invoke('reorder_group', { groupId: dragGroupId, newVisualIndex: dragTargetIndex }).catch(() => {});
  } else if (isDragging) {
    // Dragged but didn't change position — still suppress the click.
    justDragged = true;
  }
  cleanupDrag();
});

// ── Detach on drag off the widget ─────────────────────────────────────────────
// If the mouse leaves the widget window while a button is held, the group
// becomes a floating detached window at the cursor position.
//
// Happy path  — pre_detach fired during the drag (common): the hidden window
// is already rendered. We commit immediately: show it at cursor, start drag.
// The ghost is removed in executeCommit right before the window appears.
//
// Slower path — window not yet ready when cursor exits: we queue the commit.
// The ghost stays alive and executeCommit runs when 'detached-group-ready'
// fires (usually within a few hundred ms).
//
// Fallback    — pre_detach was never started (drag activated right at the
// widget edge): fall back to the old single-step detach + ghost linger.
widget.addEventListener('mouseleave', (e) => {
  if (dragGroupId && e.buttons === 1) {
    const gid = dragGroupId;

    // Clear drag tracking but keep ghost + drop indicator alive for continuity.
    isDragging      = false;
    dragGroupId     = null;
    dragSrcIndex    = null;
    dragTargetIndex = null;
    document.body.style.cursor = '';
    widget.querySelectorAll('.drag-placeholder').forEach(b => b.classList.remove('drag-placeholder'));

    if (pendingDetachGroupId === gid) {
      if (preDetachReady) {
        // Window fully rendered — commit right now (removes ghost inside).
        executeCommit(gid);
      } else {
        // Window still loading — queue commit for when it signals ready.
        pendingCommit = gid;
        // Safety: if the window never becomes ready, clean up after 2 s.
        clearTimeout(ghostCleanupTimeout);
        ghostCleanupTimeout = setTimeout(() => {
          if (pendingCommit) {
            invoke('cancel_detach', { groupId: pendingDetachGroupId }).catch(() => {});
            pendingDetachGroupId = null;
            pendingCommit        = null;
            preDetachReady       = false;
          }
          pendingGhostGroupId = null;
          cleanupGhostOnly();
        }, 2000);
      }
    } else {
      // Fallback: pre-detach wasn't started (drag activated at the very edge).
      pendingGhostGroupId = gid;
      clearTimeout(ghostCleanupTimeout);
      ghostCleanupTimeout = setTimeout(() => {
        pendingGhostGroupId = null;
        cleanupGhostOnly();
      }, 1500);
      invoke('detach_group_at_cursor', { groupId: gid }).catch(() => {});
    }
  } else {
    cleanupDrag();
  }
});

// ── Floating-window ready signal ─────────────────────────────────────────────
// When the detached-group window has rendered its button it fires this event.
listen('detached-group-ready', ({ payload }) => {
  if (payload.groupId === pendingDetachGroupId) {
    // Pre-detach path: the hidden window just rendered.
    preDetachReady = true;
    if (pendingCommit === payload.groupId) {
      // mouseleave already fired while we were waiting — commit now.
      executeCommit(payload.groupId);
    }
    // If pendingCommit is null, mouseleave hasn't fired yet; when it does
    // it will see preDetachReady === true and call executeCommit directly.
  } else if (payload.groupId === pendingGhostGroupId) {
    // Fallback path: old-style single-step detach — remove the linger ghost.
    clearTimeout(ghostCleanupTimeout);
    pendingGhostGroupId = null;
    cleanupGhostOnly();
  }
});

// ── External drop zone (re-attach from floating window) ──────────────────────
let hoverCount         = 0;
let extDropGroupId     = null;   // groupId of the floating window currently over us
let extDropTargetIdx   = -1;     // visual index of the ghost slot (-1 = not placed)
let dropZonePlaceholder = null;  // the ghost slot DOM element
let widgetPhysLeft     = null;   // cached physical left edge of the widget window

async function clearExternalDropZone() {
  if (dropZonePlaceholder) { dropZonePlaceholder.remove(); dropZonePlaceholder = null; }
  extDropGroupId   = null;
  extDropTargetIdx = -1;
  widgetPhysLeft   = null;
  widget.classList.remove('drop-zone-active');
  await measureAndResize();
}

async function updateExternalPlaceholder(groupId, cx, btnW) {
  // Lazily fetch the widget's physical left edge once per hover session.
  if (widgetPhysLeft === null) {
    const rect = await invoke('get_widget_rect').catch(() => null);
    widgetPhysLeft = rect ? rect.x : 0;
  }

  // Convert the floating window's physical centre-x to a logical position
  // relative to the widget's viewport origin (top-left = 0).
  const relPhys    = cx - widgetPhysLeft;
  const logicalRelX = relPhys / window.devicePixelRatio;

  // Find target gap index among attached (non-placeholder, non-add) buttons.
  const btns = [...widget.querySelectorAll('.group-btn:not(.add-btn):not(.drop-zone-placeholder)')];
  let target = btns.length; // default: after all buttons
  for (let i = 0; i < btns.length; i++) {
    const r = btns[i].getBoundingClientRect();
    if (logicalRelX < r.left + r.width / 2) { target = i; break; }
  }

  if (target === extDropTargetIdx && extDropGroupId === groupId) return; // no change

  extDropGroupId   = groupId;
  extDropTargetIdx = target;

  // Create or reuse placeholder element.
  if (!dropZonePlaceholder) {
    dropZonePlaceholder = document.createElement('div');
    dropZonePlaceholder.className = 'group-btn drop-zone-placeholder';
  }
  dropZonePlaceholder.style.width    = btnW + 'px';
  dropZonePlaceholder.style.minWidth = btnW + 'px';

  // Insert at target visual position (before add button if at end).
  const allBtns = [...widget.querySelectorAll('.group-btn:not(.add-btn):not(.drop-zone-placeholder)')];
  if (target < allBtns.length) {
    widget.insertBefore(dropZonePlaceholder, allBtns[target]);
  } else {
    const addBtn = widget.querySelector('.add-btn');
    widget.insertBefore(dropZonePlaceholder, addBtn || null);
  }

  await measureAndResize();

  // Tell the floating window which index it will land at so it can pass it
  // to attach_group on mouse release.
  emitTo(`detached-${groupId}`, 'drop-target-index', target).catch(() => {});
}

// Enter/leave: update the amber glow and create/remove the ghost slot.
listen('group-hovering-widget', async ({ payload }) => {
  hoverCount = Math.max(0, hoverCount + (payload.hovering ? 1 : -1));
  widget.classList.toggle('drop-zone-active', hoverCount > 0);

  if (payload.hovering) {
    await updateExternalPlaceholder(payload.groupId, payload.cx, payload.btnW || 56);
  } else if (extDropGroupId === payload.groupId) {
    await clearExternalDropZone();
  }
});

// Position updates while the floating window stays over the widget — allows
// the ghost slot to track left/right movement without spamming enter/leave.
listen('group-position-update', async ({ payload }) => {
  if (hoverCount > 0) {
    await updateExternalPlaceholder(payload.groupId, payload.cx, payload.btnW || 56);
  }
});

widget.addEventListener('contextmenu', (e) => {
  if (e.target.closest('.group-btn')) return;
  e.preventDefault();
  showMenuThrottled(() =>
    invoke('show_widget_context_menu').catch(err => console.error('Context menu error:', err))
  );
});

listen('widget-color-changed', (e) => applyWidgetColor(e.payload));

listen('add-btn-color-changed', (e) => {
  const btn = document.getElementById('add-group-btn');
  if (btn) btn.style.setProperty('--group-color', e.payload);
});

async function applyLowProfile(enabled) {
  const wordmark = document.getElementById('app-wordmark');
  if (wordmark) wordmark.classList.toggle('hidden', enabled);
  await render();
}

listen('low-profile-changed', (e) => applyLowProfile(e.payload));

const GAP   = 8;
const PAD   = 24;
const WIN_H = 80;

const PERSISTENT_IDS = ['app-wordmark', 'widget-close-btn', 'close-btn-spacer'];

async function render() {
  const config = await invoke('get_config');
  [...widget.children].forEach(el => {
    if (!PERSISTENT_IDS.includes(el.id)) el.remove();
  });

  // Only render non-detached groups; track visual index for reorder.
  const visibleGroups = config.groups.filter(g => !g.detached);
  visibleGroups.forEach((group, visualIdx) => {
    const btn = document.createElement('div');
    btn.className = 'group-btn';
    btn.dataset.groupId = group.id;   // used by drag logic to find the element
    if (group.color) btn.style.setProperty('--group-color', group.color);
    btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;

    btn.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      dragGroupId  = group.id;
      dragSrcIndex = visualIdx;
      dragStartX   = e.clientX;
      isDragging   = false;
    });

    btn.addEventListener('click', () => {
      if (justDragged) { justDragged = false; return; }
      launchGroup(group.id);
    });

    btn.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showMenuThrottled(() =>
        invoke('show_group_context_menu', { groupId: group.id })
          .catch(err => console.error('Context menu error:', err))
      );
    });

    widget.appendChild(btn);
  });

  const addBtn = document.createElement('div');
  addBtn.id = 'add-group-btn';
  addBtn.className = 'group-btn add-btn';
  addBtn.textContent = '+';
  if (config.add_btn_color) addBtn.style.setProperty('--group-color', config.add_btn_color);
  addBtn.addEventListener('click', () => openConfig(null));
  addBtn.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    showMenuThrottled(() =>
      invoke('show_add_btn_context_menu').catch(err => console.error('Context menu error:', err))
    );
  });
  widget.appendChild(addBtn);

  await new Promise(resolve => requestAnimationFrame(resolve));
  const children = [...widget.children].filter(el => el.offsetWidth > 0 && el.id !== 'widget-close-btn');
  let w = PAD;
  children.forEach((child, i) => {
    w += child.offsetWidth;
    if (i < children.length - 1) w += GAP;
  });

  await invoke('resize_widget', { width: Math.ceil(w), height: WIN_H });
  return config; // callers can use this instead of a second get_config round-trip
}

// Recalculate widget width based on current children and resize the window.
// Called after inserting or removing the drop-zone placeholder so the widget
// expands to accommodate the ghost slot.
async function measureAndResize() {
  await new Promise(resolve => requestAnimationFrame(resolve));
  const children = [...widget.children].filter(el => el.offsetWidth > 0 && el.id !== 'widget-close-btn');
  let w = PAD;
  children.forEach((child, i) => {
    w += child.offsetWidth;
    if (i < children.length - 1) w += GAP;
  });
  await invoke('resize_widget', { width: Math.ceil(w), height: WIN_H });
}

async function launchGroup(groupId) {
  try {
    await invoke('launch_group', { groupId });
  } catch (e) {
    console.error('Launch failed:', e);
  }
}

function openConfig(groupId) {
  invoke('open_config_window', { groupId: groupId || null })
    .catch(err => console.error('openConfig error:', err));
}

listen('groups-updated', () => render());

listen('context-menu:edit',   (e) => openConfig(e.payload));
listen('context-menu:delete', (e) => invoke('delete_group', { groupId: e.payload }).catch(() => {}));

// Brief toast shown after copying the share link to clipboard.
function showCopiedToast() {
  const existing = document.getElementById('share-toast');
  if (existing) existing.remove();
  const toast = document.createElement('div');
  toast.id = 'share-toast';
  toast.textContent = '✓  Link copied to clipboard!';
  toast.style.cssText = [
    'position:fixed', 'top:0', 'left:0', 'right:0', 'bottom:0',
    'display:flex', 'align-items:center', 'justify-content:center',
    'background:rgba(20,20,20,0.88)', 'color:#fff',
    'font-size:13px', 'font-weight:700', 'letter-spacing:0.03em',
    'pointer-events:none', 'z-index:9999', 'opacity:1', 'transition:opacity 0.35s',
  ].join(';');
  document.body.appendChild(toast);
  setTimeout(() => { toast.style.opacity = '0'; }, 1500);
  setTimeout(() => { toast.remove(); }, 1850);
}

listen('context-menu:share', () => { showCopiedToast(); });

listen('update-available', () => {
  const btn = document.createElement('div');
  btn.style.cssText = 'position:fixed;bottom:0;left:0;right:0;background:#e07b39;color:#fff;font-size:11px;font-weight:700;padding:3px 0;text-align:center;cursor:pointer;z-index:9999;';
  btn.textContent = '⬆ Update';
  btn.addEventListener('click', () => {
    btn.textContent = 'Downloading…';
    btn.style.cursor = 'default';
    invoke('download_and_install_update').catch(() => {
      btn.textContent = '⬆ Update';
      btn.style.cursor = 'pointer';
    });
  });
  document.body.appendChild(btn);
});

// Init: render the widget, apply saved appearance, then show the window.
// The window starts hidden (tauri.conf.json "visible": false) so the user
// never sees a blank/transparent frame — it pops in fully rendered.
(async () => {
  try {
    const config = await render();
    applyWidgetColor(config.widget_color || 'rgba(0,0,0,0.95)');
    // applyLowProfile may re-render to recalculate width when wordmark is hidden;
    // await it so the window is correctly sized before we show it.
    await applyLowProfile(config.low_profile ?? false);
    let t = null;
    getCurrentWindow().onMoved(({ payload: { x, y } }) => {
      clearTimeout(t);
      t = setTimeout(() => invoke('save_widget_position', { x, y }), 400);
    });
  } catch (e) {
    console.error('Widget init error:', e);
  } finally {
    // Always show — even if something went wrong above the widget must appear.
    getCurrentWindow().show();
  }
})();

window.addEventListener('focus', () => {
  invoke('ensure_widget_on_screen').catch(() => {});
});
