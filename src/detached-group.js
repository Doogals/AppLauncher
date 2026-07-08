import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen, emitTo } from '@tauri-apps/api/event';

const params  = new URLSearchParams(window.location.search);
const groupId = params.get('id');
const widget  = document.getElementById('widget');

// 4px padding on each side of the transparent wrapper (see detached-group.html).
const PAD = 0;

// Drag the floating pill by moving while holding LMB. We use a movement
// threshold (same approach as the widget bar's reorder logic) so that a
// quick click on the button still fires the click event. Without this,
// calling startDragging() unconditionally on mousedown hands mouse capture
// to the OS move loop, which consumes the mouseup and prevents the click
// event from ever reaching the launch handler.
let dragPending  = false;
let dragOriginX  = 0;
let dragOriginY  = 0;
const DRAG_PX    = 5; // pixels of movement required to start a drag

widget.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  dragPending = true;
  dragOriginX = e.clientX;
  dragOriginY = e.clientY;
});

// Release on either the widget or anywhere outside (e.g. after native drag).
document.addEventListener('mouseup', () => { dragPending = false; });

widget.addEventListener('mousemove', (e) => {
  if (!dragPending) return;
  const dx = e.clientX - dragOriginX;
  const dy = e.clientY - dragOriginY;
  if (dx * dx + dy * dy >= DRAG_PX * DRAG_PX) {
    dragPending = false;
    getCurrentWindow().startDragging();
  }
});

widget.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  invoke('show_detached_group_context_menu', { groupId }).catch(() => {});
});

// Track last-known window size so overlap checks don't need an extra IPC call.
let winW = 100;
let winH = 60;

// 'detached-group-ready' is only meaningful on the very first render — it
// tells the widget to commit the pre-created hidden window or clear the ghost.
// Re-renders triggered by 'groups-updated' must NOT re-emit this signal or
// they could accidentally trigger an executeCommit for an unrelated pending
// pre-detach in the widget.
let isFirstRender = true;

async function render() {
  const config = await invoke('get_config');
  const group  = config.groups.find(g => g.id === groupId);
  // Group was deleted — Rust destroys the window, but guard here too.
  if (!group) return;

  widget.innerHTML = '';

  const btn = document.createElement('div');
  btn.className = 'group-btn';
  if (group.color) btn.style.setProperty('--group-color', group.color);
  btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
  btn.addEventListener('click', () => {
    invoke('launch_group', { groupId: group.id }).catch(() => {});
  });
  widget.appendChild(btn);

  // Size the window to fit the single button (same math as widget.js render).
  await new Promise(resolve => requestAnimationFrame(resolve));
  const w = PAD + btn.offsetWidth;
  const h = btn.offsetHeight;
  winW = Math.ceil(w);
  winH = Math.ceil(h);
  await invoke('resize_detached_group', { groupId, width: winW, height: winH })
    .catch(() => {});

  // Tell the widget this window is fully rendered (first render only).
  // The widget uses this signal to commit the pre-created hidden window or
  // clear the linger ghost. Re-renders (groups-updated) must not re-emit it.
  if (isFirstRender) {
    isFirstRender = false;
    emitTo('widget', 'detached-group-ready', { groupId }).catch(() => {});
  }
}

render();

// Re-render when anything changes (name, icon, color).
listen('groups-updated', () => render());

// Widget tells us which slot index the placeholder is occupying. We store it
// and pass it back to attach_group so the group lands at the right position.
listen('drop-target-index', ({ payload }) => {
  dropTargetIndex = payload;
});

// ── Position persistence ──────────────────────────────────────────────────────
let moveTimer    = null;
let reAttachTimer = null;

// Cached widget rect — fetched once on init, refreshed in the re-attach check.
// Physical pixels, same coordinate space as onMoved payloads.
let widgetRect = null;
async function refreshWidgetRect() {
  widgetRect = await invoke('get_widget_rect').catch(() => null);
}
refreshWidgetRect();

// Whether we are currently signalling the widget's drop-zone glow.
let hoveringWidget = false;

// The visual slot index the widget has assigned for this window when hovering.
// Received via 'drop-target-index' event from the widget and passed to
// attach_group so the group lands where the ghost slot was shown.
let dropTargetIndex = null;

// Guard against the instant-reattach bug: when a group is first detached the
// floating window spawns at the cursor position which may be ON the widget.
// We must not re-attach until the window has clearly moved OUTSIDE the widget
// bounds at least once, confirming the user intentionally dragged it away.
//
// Exception: windows created via pre_detach pass from_drag=1, which means the
// user was ACTIVELY dragging when the window was created and mouseleave will
// have fired before the window becomes visible. hasBeenOutsideWidget starts
// true so that a quick drag-out-and-back gesture still triggers reattach.
let hasBeenOutsideWidget = params.get('from_drag') === '1';

function overlapsWidget(x, y) {
  if (!widgetRect) return false;
  // Check whether this window's centre falls within a generous margin around
  // the widget bar. The margin accounts for the fact that physical pixel
  // coordinates from onMoved and outer_position can differ slightly by DPI.
  const margin = 40;
  const cx = x + winW / 2;
  const cy = y + winH / 2;
  return (
    cx >= widgetRect.x - margin &&
    cx <= widgetRect.x + widgetRect.width  + margin &&
    cy >= widgetRect.y - margin &&
    cy <= widgetRect.y + widgetRect.height + margin
  );
}

// ── tryReAttach ──────────────────────────────────────────────────────────────
// Called 300 ms after the window stops moving. If LMB is still held we
// reschedule at 50 ms intervals so the release is caught promptly even when
// the user hovers over the widget for a long time before letting go.
async function tryReAttach() {
  // Guard: must have moved clearly outside the widget at some point.
  if (!hasBeenOutsideWidget) return;

  // If we're no longer over the widget at all, nothing to do.
  await refreshWidgetRect();
  const pos = await getCurrentWindow().outerPosition().catch(() => null);
  if (!pos || !overlapsWidget(pos.x, pos.y)) return;

  // If LMB is still down the drag hasn't ended — poll again in 50 ms.
  const stillDragging = await invoke('is_mouse_left_pressed').catch(() => false);
  if (stillDragging) {
    reAttachTimer = setTimeout(tryReAttach, 50);
    return;
  }

  // LMB released while over widget → reattach.
  if (hoveringWidget) {
    hoveringWidget = false;
    emitTo('widget', 'group-hovering-widget', {
      groupId, hovering: false, cx: 0, btnW: winW,
    }).catch(() => {});
  }
  // Cancel any pending position save so it doesn't race with attach_group.
  clearTimeout(moveTimer);
  moveTimer = null;
  invoke('attach_group', { groupId, insertAt: dropTargetIndex }).catch(() => {});
}

getCurrentWindow().onMoved(async ({ payload: { x, y } }) => {
  // ── Save position (debounced) ──
  clearTimeout(moveTimer);
  moveTimer = setTimeout(() => {
    invoke('save_detached_position', { groupId, x, y }).catch(() => {});
  }, 400);

  // ── Drop-zone indicator ──
  // Tell the widget to show its amber glow + ghost slot when we're over it.
  const nowOverlapping = overlapsWidget(x, y);

  // Track first time we're clearly outside the widget bounds.
  if (!nowOverlapping) hasBeenOutsideWidget = true;

  if (nowOverlapping !== hoveringWidget) {
    // State changed: send enter/leave event (widget adjusts hoverCount).
    hoveringWidget = nowOverlapping;
    emitTo('widget', 'group-hovering-widget', {
      groupId,
      hovering: hoveringWidget,
      cx: x + winW / 2,
      btnW: winW,
    }).catch(() => {});
  } else if (hoveringWidget) {
    // Still over widget but position changed — update placeholder slot without
    // touching hoverCount (use a separate event so the count stays accurate).
    emitTo('widget', 'group-position-update', {
      groupId,
      cx: x + winW / 2,
      btnW: winW,
    }).catch(() => {});
  }

  // ── Re-attach check (debounced, then polled) ──
  // After 300 ms of no movement, check: are we over the widget AND has the
  // user released the mouse? If LMB is still down we reschedule in 50 ms so
  // we catch the release even when the user hovers for a long time before
  // letting go (without this the timer fires once, returns early because the
  // button is held, and is never re-armed since onMoved stops after release).
  clearTimeout(reAttachTimer);
  reAttachTimer = setTimeout(tryReAttach, 300);
});
