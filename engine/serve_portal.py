#!/usr/bin/env python3
"""Ventosa web portal: http://<host>:8790/ (VENTOSA_PORTAL_PORT)

The team's entry page: HTTP Basic auth against identity.json, then the apps the
user's role may open, per workspace, as links on the same host. Each app asks
for the same login (browsers keep Basic credentials per origin, i.e. per port).
"""
import html, json, os, pathlib, sys, tomllib, urllib.parse
import http.server
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import auth  # noqa: E402

ENGINE = pathlib.Path(__file__).resolve().parent
WEB = ENGINE.parent / "apps" / "web"
APPS = [("dashboard", "dashboard", "dashboard.html", "uploads per platform, tokens per video, phase KPIs"),
        ("uploads", "labelling", "uploads.html", "what is ready to upload"),
        ("labelling", "labelling", "label.html", "label scripts and options"),
        ("teleprompter", "teleprompter", "teleprompter.html", "record takes from the script"),
        ("studio", "studio", "studio.html", "label rendered timelines from video files"),
        ("media-labelling", "media-labelling", "media-label.html", "watch each edited result and approve or send it back")]
DEFAULT_PORTS = {"labelling": 8766, "teleprompter": 8765, "dashboard": 8768, "studio": 8769, "media-labelling": 8771}


def workspaces():
    cfg = pathlib.Path(os.environ.get("XDG_CONFIG_HOME") or pathlib.Path.home() / ".config") / "ventosa" / "config.toml"
    try:
        data = tomllib.loads(cfg.read_text())
    except (OSError, tomllib.TOMLDecodeError):
        return []
    out = []
    for w in data.get("workspaces", []):
        root = pathlib.Path(os.path.expanduser(w.get("root", "")))
        try:
            wj = json.loads((root / "workspace.json").read_text())
        except (OSError, ValueError):
            wj = {}
        out.append({"name": w.get("name", root.name), "ports": {**DEFAULT_PORTS, **(wj.get("ports") or {})}, "brand": (wj.get("brand") or {}).get("name", w.get("name", ""))})
    return out


class H(http.server.BaseHTTPRequestHandler):
    auth_user = None

    def log_message(s, *a):
        pass

    def parse_request(s):
        return super().parse_request() and auth.guard(s, "portal")

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path.startswith("/common/"):
            f = (WEB / "common" / u.path[8:]).resolve()
            if f.is_file() and f.is_relative_to(WEB.resolve()):
                body = f.read_bytes()
                s.send_response(200); s.send_header("Content-Type", "text/css" if f.suffix == ".css" else "text/javascript")
                s.send_header("Content-Length", str(len(body))); s.end_headers(); s.wfile.write(body); return
            return s.send_error(404)
        if u.path == "/api/me":
            return s.json({"user": s.auth_user, "apps": auth.apps_for(s.auth_user) if s.auth_user else ["*"]})
        if u.path != "/":
            return s.send_error(404)
        grants = auth.apps_for(s.auth_user) if s.auth_user else ["*"]
        host = (s.headers.get("Host") or "127.0.0.1").split(":")[0]
        cards = []
        for w in workspaces():
            links = [f'<a class="app" href="http://{html.escape(host)}:{w["ports"].get(port_key, 0)}/{page}"><b>{label}</b><small>{html.escape(hint)}</small></a>'
                     for label, port_key, page, hint in APPS if "*" in grants or label in grants]
            if links:
                cards.append(f'<section class="panel"><div class="panel-head"><small>{html.escape(w["brand"] or w["name"])} · {html.escape(w["name"])}</small></div><div class="apps">{"".join(links)}</div></section>')
        who = html.escape(s.auth_user or "open mode")
        body = f"""<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ventosa portal</title><style>
body{{margin:0;min-height:100vh}} main{{max-width:760px;margin:0 auto;padding:16px;display:flex;flex-direction:column;gap:12px}}
header{{display:flex;align-items:center;gap:8px}} header span{{margin-left:auto;color:var(--dim)}}
.apps{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:1px;background:var(--line)}}
.app{{display:flex;flex-direction:column;gap:2px;padding:12px;background:var(--inset);color:var(--fg);text-decoration:none;min-height:44px}}
.app:hover{{background:var(--hover)}} .app b{{color:var(--o);font-weight:400}} .app small{{color:var(--dim)}}
</style><link rel="stylesheet" href="/common/ventosa.css"></head><body>
<header><h1><span class="w1">vent</span><span class="w2">osa</span></h1><span>{who}</span></header>
<main>{"".join(cards) or '<p class="dim">no apps are granted to you yet — ask an admin (access ▸ roles)</p>'}
<p class="dim">The desktop launcher (ventosa-portal) also has the docs and, if your role has it, the chat.</p></main></body></html>"""
        b = body.encode()
        s.send_response(200); s.send_header("Content-Type", "text/html; charset=utf-8"); s.send_header("Content-Length", str(len(b)))
        s.send_header("Cache-Control", "no-store"); s.end_headers(); s.wfile.write(b)

    def json(s, v):
        b = json.dumps(v).encode()
        s.send_response(200); s.send_header("Content-Type", "application/json"); s.send_header("Content-Length", str(len(b))); s.end_headers(); s.wfile.write(b)


if __name__ == "__main__":
    port = int(os.environ.get("VENTOSA_PORTAL_PORT", 8790))
    print(f"http://127.0.0.1:{port}/", flush=True)
    http.server.ThreadingHTTPServer((auth.bind(), port), H).serve_forever()
