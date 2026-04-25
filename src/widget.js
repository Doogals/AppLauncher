import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const widget = document.getElementById('widget');
const contextMenu = document.getElementById('context-menu');

// Button dimensions must match styles.css values:
// .group-btn: padding 8px 14px = 28px horizontal, min-width 70px → ~98px per button
// .add-btn:   min-width 50px + 28px padding → ~78px
// .widget:    padding 8px 12px = 24px horizontal, gap 8px
const BTN_W   = 98;
const ADD_W   = 78;
const GAP     = 8;
const PAD     = 24;
const WIN_H   = 80;

function widgetWidth(groupCount) {
  if (groupCount === 0) return PAD + ADD_W;
  return PAD + groupCount * BTN_W + groupCount * GAP + ADD_W;
}

let activeGroupId = null;

async function render() {
  const config = await invoke('get_config');
  widget.innerHTML = '';

  for (const group of config.groups) {
    const btn = document.createElement('div');
    btn.className = 'group-btn';
    btn.innerHTML = `<span class="icon">${group.icon}</span><span class="label">${group.name}</span>`;
    btn.addEventListener('click', () => launchGroup(group.id));
    btn.addEventListener('contextmenu', (e) => showContextMenu(e, group.id));
    widget.appendChild(btn);
  }

  const addBtn = document.createElement('div');
  addBtn.className = 'group-btn add-btn';
  addBtn.innerHTML = `<span>＋</span>`;
  addBtn.addEventListener('click', () => openConfig(null));
  widget.appendChild(addBtn);

  await invoke('resize_widget', {
    width: widgetWidth(config.groups.length),
    height: WIN_H,
  });
}

async function launchGroup(groupId) {
  try {
    await invoke('launch_group', { groupId });
  } catch (e) {
    console.error('Launch failed:', e);
  }
}

function showContextMenu(e, groupId) {
  e.preventDefault();
  activeGroupId = groupId;
  contextMenu.innerHTML = `
    <div class="context-menu-item" id="cm-edit">Edit Group</div>
    <div class="context-menu-item danger" id="cm-delete">Delete Group</div>
  `;
  contextMenu.style.display = 'block';
  contextMenu.style.left = e.clientX + 'px';
  contextMenu.style.top = e.clientY + 'px';
  document.getElementById('cm-edit').onclick   = () => { hideContextMenu(); openConfig(groupId); };
  document.getElementById('cm-delete').onclick = () => { hideContextMenu(); deleteGroup(groupId); };
}

function hideContextMenu() {
  contextMenu.style.display = 'none';
  activeGroupId = null;
}

async function deleteGroup(groupId) {
  await invoke('delete_group', { groupId });
  render();
}

async function openConfig(groupId) {
  const win = new WebviewWindow('config', {
    url: groupId ? `config.html?id=${groupId}` : 'config.html',
    title: groupId ? 'Edit Group' : 'New Group',
    width: 420,
    height: 520,
    decorations: true,
    resizable: false,
    alwaysOnTop: true,
  });
  win.once('tauri://destroyed', () => render());
}

document.addEventListener('click', hideContextMenu);

// Render first, then attach position-saving listener (non-critical)
render()
  .then(() => import('@tauri-apps/api/window'))
  .then(({ getCurrentWindow }) => {
    let t = null;
    getCurrentWindow().onMoved(({ payload: { x, y } }) => {
      clearTimeout(t);
      t = setTimeout(() => invoke('save_widget_position', { x, y }), 400);
    });
  })
  .catch(e => console.error('Widget init error:', e));
