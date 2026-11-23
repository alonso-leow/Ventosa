// <mvj-queues> shared queue monitor (Label + Record apps). Polls /api/pipeline.
// Emits window event 'mvj-pick' {detail:{vid, stage}} when a card is clicked,
// and 'mvj-pipeline' {detail: state} on every refresh.
const CSS = `
:host{display:block;font:12px/1.45 var(--mono,ui-monospace,monospace);color:var(--fg,#f2efe9)}
.top{display:flex;gap:10px;align-items:center;margin-bottom:8px;flex-wrap:wrap}
.top b{font-size:14px}.dim{color:var(--dim,#8a8794)}
.board{display:grid;grid-template-columns:repeat(auto-fill,minmax(170px,1fr));gap:8px}
.col{background:var(--pan,#12121a);border:1px solid var(--line,#24232c);border-radius:0;padding:6px;min-height:60px}
.col h4{margin:0 0 6px;font-size:12px;display:flex;justify-content:space-between;text-transform:uppercase;letter-spacing:.04em}
.ai h4{color:var(--p-lit,#b38bff)}.human h4{color:var(--amb,#ffb000)}.done h4{color:var(--ok,#3ddc84)}
.card{border:1px solid var(--line,#24232c);border-radius:0;padding:5px 7px;margin:4px 0;cursor:pointer;background:var(--inset,#050506)}
.card:hover{border-color:var(--dim,#8a8794)}.card .d{color:var(--dim,#8a8794);font-size:11px}
button{background:var(--glass,#111114aa);color:inherit;border:1px solid var(--line,#24232c);border-radius:0;padding:2px 7px;font:10px var(--mono,monospace);cursor:pointer;margin-top:4px}
details{margin-top:8px}summary{cursor:pointer;color:var(--dim,#8a8794)}
.ev{font:11px ui-monospace,monospace;color:var(--dim,#8a8794);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.ev.human{color:var(--amb,#ffb000)}.ev.ai{color:var(--p-lit,#b38bff)}`;

class MvjQueues extends HTMLElement {
  connectedCallback() {
    this.root = this.attachShadow({mode: 'open'});
    this.root.innerHTML = `<style>${CSS}</style><div class="top"><b>Pipeline queues</b><span class="dim" id="sum"></span>
      <span style="flex:1"></span><button id="r">↻ refresh</button></div><div class="board" id="b"></div>
      <details><summary>recent events</summary><div id="ev"></div></details>`;
    this.root.getElementById('r').onclick = () => this.load();
    this.load(); this.t = setInterval(() => this.load(), +(this.getAttribute('interval') || 5000));
    window.addEventListener('mvj-refresh', () => this.load());
  }
  disconnectedCallback() { clearInterval(this.t); }
  async load() {
    let st; try { st = await (await fetch('/api/pipeline', {cache: 'no-store'})).json(); } catch { this.root.getElementById('sum').textContent = 'server offline'; return; }
    window.dispatchEvent(new CustomEvent('mvj-pipeline', {detail: st}));
    const only = (this.getAttribute('stages') || '').split(',').filter(Boolean);
    const b = this.root.getElementById('b'); b.innerHTML = '';
    for (const s of st.stages) {
      if (only.length && !only.includes(s.id)) continue;
      const q = st.queues[s.id] || [];
      const col = document.createElement('div');
      col.className = 'col ' + (s.owner === 'ai' ? 'ai' : s.owner === 'human' ? 'human' : 'done');
      col.innerHTML = `<h4><span>${s.id.replace('_', ' ')} · ${s.owner}</span><span>${q.length}</span></h4>`;
      col.title = s.need;
      for (const vid of q) {
        const v = st.videos[vid], c = document.createElement('div'); c.className = 'card';
        c.innerHTML = `<div><b>${vid}</b> ${esc(v.title)}</div><div class="d">${esc(v.detail || '')}</div>`;
        c.onclick = () => window.dispatchEvent(new CustomEvent('mvj-pick', {detail: {vid, stage: s.id}}));
        if (s.owner === 'ai') {
          const btn = document.createElement('button'); btn.textContent = 'copy AI prompt';
          btn.onclick = e => { e.stopPropagation(); copy((window.VENTOSA_BRAND?.agent_command || 'ventosa-graph attach {step} {vid}').replace('{step}', s.id).replace('{vid}', vid), btn); };
          c.append(btn);
        }
        col.append(c);
      }
      b.append(col);
    }
    const ai = st.stages.filter(s => s.owner === 'ai').reduce((n, s) => n + (st.queues[s.id] || []).length, 0);
    this.root.getElementById('sum').textContent = `${ai} waiting on AI · ${st.human_changes_since_ai} human changes not seen by AI · ` + ((st.attached || []).length ? 'AI attached: ' + st.attached.map(a => `${a.agent} ${a.step} ${a.vid}`).join(', ') : 'no AI attached');
    this.root.getElementById('ev').innerHTML = st.recent_events.slice().reverse()
      .map(e => `<div class="ev ${e.actor}">${e.at.slice(5, 16)} ${e.actor} ${e.kind} ${e.vid} ${esc(JSON.stringify(e.data)).slice(0, 90)}</div>`).join('');
  }
}
const esc = s => String(s).replace(/[&<>"]/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;'}[c]));
export async function copy(text, btn) {
  try { await navigator.clipboard.writeText(text); } catch {
    const t = document.createElement('textarea'); t.value = text; document.body.append(t); t.select(); document.execCommand('copy'); t.remove();
  }
  if (btn) { const o = btn.textContent; btn.textContent = '✓ copied'; setTimeout(() => btn.textContent = o, 1200); }
}
customElements.define('mvj-queues', MvjQueues);
