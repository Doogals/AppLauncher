import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';

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

// Picks a light or dark variant depending on the chosen background's own
// brightness (ignoring alpha — the curated colors are all semi-transparent,
// but matching the nominal color's brightness is what people expect when
// they pick something like White, regardless of what's behind the widget).
function getContrastColor(color, lightVariant, darkVariant) {
  const rgb = parseColorToRgb(color);
  if (!rgb) return lightVariant;
  const [r, g, b] = rgb;
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance > 0.55 ? darkVariant : lightVariant;
}

// ── Widget background right-click menu (native) ──────────────────────────────
function applyWidgetColor(color) {
  const widget = document.querySelector('.widget');
  widget.style.background = color;
  // The border was hardcoded to the original navy and never followed the
  // chosen color — harmless against dark colors close to navy, but glaring
  // against something like White (a navy outline around a white bar).
  widget.style.borderColor = color;

  // Wordmark and close button both defaulted to a semi-white tint that
  // assumes a dark widget background — invisible against something like
  // White. Switch to a dark tint instead when the chosen background is light.
  const wordmark = document.getElementById('app-wordmark');
  if (wordmark) wordmark.style.color = getContrastColor(color, 'rgba(255,255,255,0.35)', 'rgba(0,0,0,0.4)');
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

widget.addEventListener('contextmenu', (e) => {
  if (e.target.closest('.group-btn')) return;
  e.preventDefault();
  showMenuThrottled(() =>
    invoke('show_widget_context_menu').catch(err => console.error('Context menu error:', err))
  );
});

listen('widget-color-changed', (e) => applyWidgetColor(e.payload));

async function applyLowProfile(enabled) {
  const wordmark = document.getElementById('app-wordmark');
  if (wordmark) wordmark.classList.toggle('hidden', enabled);
  // Re-measure and resize the window now that the wordmark is shown/hidden
  await render();
}

listen('low-profile-changed', (e) => applyLowProfile(e.payload));

const GAP   = 8;
// Must match .widget's total left+right padding in styles.css (12px each
// side — chosen to match the effective 12px top/bottom visual gap).
const PAD   = 24;
const WIN_H = 80;

const PERSISTENT_IDS = ['app-wordmark', 'widget-close-btn', 'close-btn-spacer'];

async function render() {
  const config = await invoke('get_config');
  // Remove everything except the wordmark + close button so they survive re-renders.
  [...widget.children].forEach(el => {
    if (!PERSISTENT_IDS.includes(el.id)) el.remove();
  });

  for (const group of config.groups) {
    const btn = document.createElement('div');
    btn.className = 'group-btn';
    if (group.color) btn.style.setProperty('--group-color', group.color);
    btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
    btn.addEventListener('click', () => launchGroup(group.id));
    btn.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showMenuThrottled(() =>
        invoke('show_group_context_menu', { groupId: group.id })
          .catch(err => console.error('Context menu error:', err))
      );
    });
    widget.appendChild(btn);
  }

  const addBtn = document.createElement('div');
  addBtn.className = 'group-btn add-btn';
  addBtn.textContent = '+';
  addBtn.addEventListener('click', () => openConfig(null));
  widget.appendChild(addBtn);

  // Measure actual rendered button widths — skip hidden elements (e.g. wordmark in low-profile)
  // and the absolutely-positioned close button, which doesn't take up flex flow space.
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

// Opens config window via Rust so its lifecycle is independent of this window's context.
function openConfig(groupId) {
  invoke('open_config_window', { groupId: groupId || null })
    .catch(err => console.error('openConfig error:', err));
}

// Re-render when a group is saved or deleted (emitted by save_group / delete_group).
listen('groups-updated', () => render());

// Listen for native context menu selections
listen('context-menu:edit',   (e) => openConfig(e.payload));
listen('context-menu:delete', (e) => invoke('delete_group', { groupId: e.payload }).catch(() => {}));
// "Change Color" opens its own small window (see lib.rs ctx-color: handler) —
// the widget itself is too small to host a swatch-picker modal in-page.

// Show update notification banner when a new version is available
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

// Position saving after render + restore saved color
render().then(async () => {
  const config = await invoke('get_config');
  if (config.widget_color) applyWidgetColor(config.widget_color);
  applyLowProfile(config.low_profile ?? false);
  let t = null;
  getCurrentWindow().onMoved(({ payload: { x, y } }) => {
    clearTimeout(t);
    t = setTimeout(() => invoke('save_widget_position', { x, y }), 400);
  });
}).catch(e => console.error('Widget init error:', e));
