// White-label: brand, accent and wording come from the workspace (GET /api/brand).
// Pages mark the app name with <h1 data-app="Labelling">; the brand fills the rest.
(async () => {
  let b;
  try { b = await (await fetch('/api/brand', {cache: 'no-store'})).json(); } catch { return; }
  window.VENTOSA_BRAND = b;
  const root = document.documentElement.style;
  if (b.accent && b.accent.toLowerCase() !== '#ff2e2e') { root.setProperty('--p', b.accent); root.setProperty('--red', b.accent); }
  const h1 = document.querySelector('h1[data-app]');
  const app = h1 ? h1.dataset.app : '';
  if (h1) {
    const n = b.name || '', cut = n.toLowerCase() === 'ventosa' ? 4 : Math.ceil(n.length / 2);
    h1.innerHTML = '';
    h1.append(Object.assign(document.createElement('span'), {className: 'w1', textContent: n.slice(0, cut)}),
              Object.assign(document.createElement('span'), {className: 'w2', textContent: n.slice(cut)}),
              Object.assign(document.createElement('small'), {textContent: '  ' + app}));
  }
  document.title = `${b.name} · ${app}`;
  const unit = b.unit || 'video';
  document.querySelectorAll('[data-unit-text]').forEach(el => {
    el.textContent = el.dataset.unitText.replaceAll('{unit}', unit);
  });
  if (b.audio_only) document.body.classList.add('audio-default');
  window.dispatchEvent(new CustomEvent('ventosa-brand', {detail: b}));
})();
