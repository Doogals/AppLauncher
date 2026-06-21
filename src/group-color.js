import { invoke } from '@tauri-apps/api/core';
import { EXTRA_COLOR_HUES, withAlpha, SOLID_COLORS } from './colors.js';

// This window does double duty — group color (?mode=group&id=X) and widget
// color (?mode=widget) — rather than maintaining two near-identical tabbed
// color pickers. Defaults to group mode for backward compatibility.
const params = new URLSearchParams(window.location.search);
const mode = params.get('mode') || 'group';
const groupId = params.get('id');

// Each context keeps its own pre-existing 6 colors at its own opacity (the
// widget needs to read as a solid bar, group buttons sit on top of it as
// smaller accents at lower opacity) — only the 14 extra hues are shared.
const THEME_COLORS = mode === 'widget'
  ? [
      { label: 'Default',  value: 'rgba(22,33,62,0.95)' },
      { label: 'Charcoal', value: 'rgba(30,30,30,0.95)' },
      { label: 'Forest',   value: 'rgba(15,40,25,0.95)' },
      { label: 'Midnight', value: 'rgba(20,10,40,0.95)' },
      { label: 'Rust',     value: 'rgba(60,25,10,0.95)' },
      { label: 'Steel',    value: 'rgba(20,30,45,0.95)' },
      ...withAlpha(EXTRA_COLOR_HUES, 0.95),
    ]
  : [
      { label: 'Default',  value: 'rgba(15,52,96,0.6)' },
      { label: 'Charcoal', value: 'rgba(30,30,30,0.85)' },
      { label: 'Forest',   value: 'rgba(15,40,25,0.85)' },
      { label: 'Midnight', value: 'rgba(20,10,40,0.85)' },
      { label: 'Rust',     value: 'rgba(60,25,10,0.85)' },
      { label: 'Steel',    value: 'rgba(20,30,45,0.85)' },
      ...withAlpha(EXTRA_COLOR_HUES, 0.85),
    ];

async function applyColor(color) {
  try {
    if (mode === 'widget') {
      await invoke('save_widget_color', { color });
    } else {
      await invoke('save_group_color', { groupId, color });
    }
  } catch (e) {
    console.error('Failed to save color:', e);
  }
}

// Renders one tab's swatch grid and wires clicks. Returns nothing — each
// tab's swatches are independent, clicking one doesn't affect the other tab.
function renderSwatchGrid(containerId, colors, currentColor) {
  const container = document.getElementById(containerId);
  const swatches = [];

  const setActive = (activeSwatch) => {
    swatches.forEach(({ el }) => {
      el.classList.remove('active');
      el.style.border = '2px solid rgba(255,255,255,0.12)';
    });
    activeSwatch.classList.add('active');
    activeSwatch.style.border = '2px solid #e0e0e0';
  };

  colors.forEach(({ label, value }) => {
    const swatch = document.createElement('div');
    const isActive = value === currentColor;
    swatch.className = 'color-swatch' + (isActive ? ' active' : '');
    swatch.title = label;
    swatch.style.background = value;
    swatch.style.border = '2px solid ' + (isActive ? '#e0e0e0' : 'rgba(255,255,255,0.12)');

    const lbl = document.createElement('span');
    lbl.className = 'swatch-label';
    lbl.textContent = label;
    swatch.appendChild(lbl);

    // Stays open and applies live — the user can click through colors and
    // compare before closing the window themselves.
    swatch.addEventListener('click', () => {
      setActive(swatch);
      applyColor(value);
    });

    swatches.push({ el: swatch, value });
    container.appendChild(swatch);
  });
}

function initTabs() {
  document.querySelectorAll('.gc-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.gc-tab').forEach(t => t.classList.toggle('active', t === tab));
      document.getElementById('gc-tab-theme').style.display = tab.dataset.tab === 'theme' ? '' : 'none';
      document.getElementById('gc-tab-solid').style.display = tab.dataset.tab === 'solid' ? '' : 'none';
    });
  });
}

async function init() {
  if (mode === 'group' && !groupId) return;

  let currentColor = null;
  try {
    const config = await invoke('get_config');
    if (mode === 'widget') {
      currentColor = config.widget_color || null;
    } else {
      const group = (config.groups || []).find(g => g.id === groupId);
      currentColor = group?.color || null;
    }
  } catch {}

  renderSwatchGrid('group-color-swatches', THEME_COLORS, currentColor);
  renderSwatchGrid('solid-color-swatches', SOLID_COLORS, currentColor);
  initTabs();
}

init();
