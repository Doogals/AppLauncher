// The widget background (App Settings → Appearance) and group buttons
// (Change Color) each had their own hand-tuned set of 6 colors at different
// opacities — the widget needs to read as a solid bar, group buttons sit on
// top of it as smaller accents at lower opacity. Rather than force them onto
// one identical palette (which would visibly change one or both), this just
// adds the SAME 14 new hues to both, each rendered at that context's own
// existing opacity. The original 6 in each file are left untouched.
export const EXTRA_COLOR_HUES = [
  { label: 'Crimson',  rgb: [55, 10, 18] },
  { label: 'Teal',     rgb: [10, 45, 45] },
  { label: 'Plum',     rgb: [45, 12, 40] },
  { label: 'Olive',    rgb: [42, 42, 12] },
  { label: 'Indigo',   rgb: [25, 15, 55] },
  { label: 'Maroon',   rgb: [40, 12, 18] },
  { label: 'Emerald',  rgb: [8, 48, 35] },
  { label: 'Amber',    rgb: [55, 35, 5] },
  { label: 'Sapphire', rgb: [10, 22, 55] },
  { label: 'Mauve',    rgb: [42, 24, 34] },
  { label: 'Bronze',   rgb: [45, 30, 12] },
  { label: 'Ocean',    rgb: [8, 35, 42] },
  { label: 'Wine',     rgb: [35, 8, 20] },
  { label: 'Sage',     rgb: [28, 35, 26] },
];

export function withAlpha(hues, alpha) {
  return hues.map(({ label, rgb: [r, g, b] }) => ({
    label,
    value: `rgba(${r},${g},${b},${alpha})`,
  }));
}

// 20 fully-opaque, vivid preset colors for the "Solid Colors" tab — distinct
// from the muted/transparent theme palette above, same grid layout (5x4).
// Names deliberately don't overlap with the theme palette's labels.
export const SOLID_COLORS = [
  { label: 'Red',     value: '#e53935' },
  { label: 'Orange',  value: '#fb8c00' },
  { label: 'Yellow',  value: '#fdd835' },
  { label: 'Lime',    value: '#c0ca33' },
  { label: 'Green',   value: '#43a047' },
  { label: 'Mint',    value: '#26a69a' },
  { label: 'Cyan',    value: '#00acc1' },
  { label: 'Sky',     value: '#039be5' },
  { label: 'Blue',    value: '#1e88e5' },
  { label: 'Navy',    value: '#1a237e' },
  { label: 'Violet',  value: '#5e35b1' },
  { label: 'Purple',  value: '#8e24aa' },
  { label: 'Magenta', value: '#d81b60' },
  { label: 'Pink',    value: '#ec407a' },
  { label: 'Salmon',  value: '#ff7043' },
  { label: 'Brown',   value: '#6d4c41' },
  { label: 'Tan',     value: '#a1887f' },
  { label: 'Gray',    value: '#757575' },
  { label: 'Black',   value: '#000000' },
  { label: 'White',   value: '#ffffff' },
];
