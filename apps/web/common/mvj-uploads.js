// <mvj-uploads> shared upload list: approved texts with copy buttons + the edited file to drag
// into YouTube Studio. Listens to 'mvj-pipeline' (from <mvj-queues>) or polls itself.
import {copy} from './mvj-queues.js';
const CSS = `
:host{display:block;font:12px/1.45 var(--mono,ui-monospace,monospace);color:var(--fg,#f2efe9)}
.top{display:flex;gap:10px;align-items:center;margin-bottom:8px}.top b{font-size:14px}.dim{color:var(--dim,#8a8794)}
.u{border:1px solid var(--line,#24232c);border-radius:0;padding:10px;margin:8px 0;background:var(--pan,#12121a)}
.u.done{opacity:.55}
.f{display:grid;grid-template-columns:90px 1fr auto;gap:6px 8px;align-items:start;margin:4px 0}
.f .k{color:var(--dim,#8a8794);padding-top:3px}.f .v{white-space:pre-wrap;max-height:7.5em;overflow:auto;background:var(--inset,#050506);border-radius:0;padding:4px 6px}
button,input{background:var(--inset,#050506);color:inherit;border:1px solid var(--line,#24232c);border-radius:0;padding:3px 8px;font:10px var(--mono,monospace);cursor:pointer}
.file{display:inline-flex;gap:6px;align-items:center;border:1px dashed var(--amb,#ffb000);border-radius:0;padding:6px 10px;cursor:grab;user-select:none}
.row{display:flex;gap:6px;flex-wrap:wrap;align-items:center;margin-top:6px}`;

class MvjUploads extends HTMLElement {
  connectedCallback() {
    this.root = this.attachShadow({mode: 'open'});
    this.root.innerHTML = `<style>${CSS}</style><div class="top"><b>Upload list</b><span class="dim" id="sum"></span></div><div id="l"></div>
      <p class="dim">Drag the file chip onto your platform's upload area. If your browser refuses the drag, use "show in folder" and drag from the file manager, or "copy path" and paste it into the file chooser (Ctrl+L).</p>`;
    window.addEventListener('mvj-pipeline', e => this.render(e.detail));
    if (!document.querySelector('mvj-queues')) { this.poll(); this.t = setInterval(() => this.poll(), 5000); }
  }
  disconnectedCallback() { clearInterval(this.t); }
  async poll() { try { this.render(await (await fetch('/api/pipeline', {cache: 'no-store'})).json()); } catch {} }
  render(st) {
    const key = JSON.stringify(st.uploads); if (key === this.last) return; this.last = key;
    const l = this.root.getElementById('l'); l.innerHTML = '';
    const todo = st.uploads.filter(u => !u.uploaded).length;
    this.root.getElementById('sum').textContent = `${todo} to upload · ${st.uploads.length - todo} uploaded`;
    if (!st.uploads.length) l.innerHTML = '<p class="dim">Nothing approved yet. Approve a title and description in Label ▸ Publish.</p>';
    for (const u of st.uploads.sort((a, b) => a.uploaded - b.uploaded)) {
      const d = document.createElement('div'); d.className = 'u' + (u.uploaded ? ' done' : '');
      d.innerHTML = `<b>${u.vid}</b> ${u.uploaded ? '✓ uploaded ' + (u.url ? `<a href="${u.url}" target="_blank" style="color:inherit">${u.url}</a>` : '') : ''}`;
      for (const [k, v] of [['title', u.title], ['description', u.description], ['tags', u.tags]]) {
        const f = document.createElement('div'); f.className = 'f';
        f.innerHTML = `<span class="k">${k}</span><div class="v"></div>`; f.querySelector('.v').textContent = v || '—';
        const b = document.createElement('button'); b.textContent = 'copy'; b.onclick = () => copy(v || '', b); f.append(b); d.append(f);
      }
      const row = document.createElement('div'); row.className = 'row';
      if (u.file) {
        const name = u.file.split('/').pop(), url = location.origin + '/media/' + u.file.replace(/^assets\//, '');
        const chip = document.createElement('a'); chip.className = 'file'; chip.draggable = true; chip.href = url; chip.download = name;
        chip.textContent = '🎞 ' + name;
        chip.addEventListener('dragstart', e => {
          const mime = name.endsWith('.webm') ? 'video/webm' : name.endsWith('.mov') ? 'video/quicktime' : 'video/mp4';
          e.dataTransfer.setData('DownloadURL', `${mime}:${name}:${url}`);
          e.dataTransfer.setData('text/uri-list', url);
        });
        const rev = document.createElement('button'); rev.textContent = 'show in folder';
        rev.onclick = () => fetch('/api/reveal?path=' + encodeURIComponent(u.file), {method: 'POST'});
        const cp = document.createElement('button'); cp.textContent = 'copy path'; cp.onclick = () => copy(u.abs, cp);
        row.append(chip, rev, cp);
      } else row.append(Object.assign(document.createElement('span'), {className: 'dim', textContent: 'no edit file yet'}));
      const url = Object.assign(document.createElement('input'), {placeholder: 'Published URL (optional)', value: u.url || ''}); url.style.flex = '1';
      const mk = document.createElement('button'); mk.textContent = u.uploaded ? 'undo uploaded' : 'mark uploaded';
      mk.onclick = async () => {
        const st2 = await (await fetch('/api/uploaded', {method: 'POST', body: JSON.stringify({vid: u.vid, uploaded: !u.uploaded, url: url.value})})).json();
        window.dispatchEvent(new CustomEvent('mvj-uploaded', {detail: {vid: u.vid, uploaded: !u.uploaded, url: url.value}}));
        window.dispatchEvent(new CustomEvent('mvj-refresh')); this.render(st2);
      };
      row.append(url, mk); d.append(row); l.append(d);
    }
  }
}
customElements.define('mvj-uploads', MvjUploads);
