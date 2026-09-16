import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

// Bundled with the app rather than fetched, so this window works offline and
// appears instantly. Vite inlines the JSON at build time.
import notes from './whats-new.json';

const params = new URLSearchParams(window.location.search);
// "welcome" on a fresh install, otherwise the version that was just installed.
// Passed in by Rust so this window never has to work out which one applies.
const entryKey = params.get('entry') || 'welcome';

function textNode(tag, className, text) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  el.textContent = text;
  return el;
}

function render() {
  const entry = notes[entryKey];
  const content = document.getElementById('content');

  // A version with no entry shouldn't reach here — release.ps1 refuses to build
  // one — but showing an empty window would be worse than saying so plainly.
  if (!entry) {
    content.appendChild(textNode(
      'p', 'empty',
      'This version does not have release notes available.',
    ));
    return;
  }

  document.getElementById('title').textContent = entry.title || "What's New";

  if (entry.subtitle) {
    const sub = document.getElementById('subtitle');
    sub.textContent = entry.subtitle;
    sub.style.display = '';
  }

  (entry.sections || []).forEach(section => {
    const wrap = document.createElement('div');
    wrap.className = 'section';

    if (section.heading) {
      wrap.appendChild(textNode('div', 'section-heading', section.heading));
    }

    const ul = document.createElement('ul');
    (section.items || []).forEach(item => ul.appendChild(textNode('li', null, item)));
    wrap.appendChild(ul);

    content.appendChild(wrap);
  });
}

async function init() {
  render();

  // Read from the binary rather than the notes file, so the footer can never
  // disagree with what is actually running.
  invoke('get_app_version')
    .then(v => { document.getElementById('version').textContent = `TakeOff v${v}`; })
    .catch(() => {});

  const close = () => getCurrentWindow().close();
  document.getElementById('close-btn').addEventListener('click', close);
  // Esc closes it too — this is a dismissible notice, not something to trap
  // someone in.
  window.addEventListener('keydown', e => { if (e.key === 'Escape') close(); });

  document.getElementById('close-btn').focus();
}

init();
