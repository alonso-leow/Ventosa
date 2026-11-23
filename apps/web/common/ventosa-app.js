// Helper for AI-built apps: window.ventosa.{brand,state,app,done,event}.
// Talks to the app's own server (engine/serve_app.py) on the same origin.
(() => {
  const call = async (method, url, body) => {
    const r = await fetch(url, {
      method, cache: 'no-store',
      headers: body ? {'Content-Type': 'application/json'} : {},
      body: body ? JSON.stringify(body) : undefined,
    });
    const d = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(d.error || r.statusText);
    return d;
  };
  window.ventosa = {
    brand: () => call('GET', '/api/brand'),
    state: () => call('GET', '/api/state'),
    app: () => call('GET', '/api/app'),
    done: (subject, note = '') => call('POST', '/api/done', {subject, note}),
    event: (kind, subject = '', data = {}) => call('POST', '/api/event', {kind, subject, data}),
  };
})();
