#!/usr/bin/env python3
"""Ventosa custom app server (localhost only): serve_app.py <app_dir> <port>

GET  /                 the app's files (index.html, …) from <app_dir>
GET  /common/<file>    shared web kit (apps/web/common)
GET  /api/brand        workspace brand
GET  /api/state        refreshed pipeline state (pipeline/pipeline.json)
GET  /api/app          this app's app.json
POST /api/done         {"subject","note"}: apply the app's effect on its node through
                       ai_write.py (human-done | step-done), then log app.done
POST /api/event        {"kind","subject","data"}: log an app event

Remote mode (VENTOSA_REMOTE=1, the app published to a server by `app_publish`):
no engine there; listens on 0.0.0.0, every done/event is appended to
<app_dir>/events.jsonl, which Ventosa pulls back (`app_pull`) and applies.
"""
import http.server, json, mimetypes, os, pathlib, re, subprocess, sys, urllib.parse

import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import auth  # noqa: E402

ENGINE = pathlib.Path(__file__).resolve().parent
REMOTE = os.environ.get("VENTOSA_REMOTE") == "1"
if REMOTE:
    status = None
    ROOT = pathlib.Path.cwd()
    COMMON = ENGINE / "common"

    def brand_payload():
        return {"name": load_app().get("title", ""), "remote": True}
else:
    sys.path.insert(0, str(ENGINE))
    import status  # noqa: E402
    from ws import ROOT, WEB, brand_payload  # noqa: E402

    COMMON = WEB / "common"
SUBJECT = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,31}$")
APP_DIR = None


def load_app():
    return json.loads((APP_DIR / "app.json").read_text())


def remote_log(kind, e):
    with open(APP_DIR / "events.jsonl", "a") as f:
        f.write(json.dumps({"at": int(time.time()), "kind": kind, **e}, ensure_ascii=False) + "\n")


def safe(base, rel):
    p = (base / rel).resolve()
    return p if p.is_relative_to(base.resolve()) and p.is_file() else None


class Handler(http.server.SimpleHTTPRequestHandler):
    auth_user = None

    def __init__(s, *a, **k):
        super().__init__(*a, directory=str(APP_DIR), **k)

    def parse_request(s):
        return super().parse_request() and auth.guard(s, APP_DIR.name)

    def log_message(s, *a):
        pass

    def send(s, body=b"{}", ctype="application/json", code=200):
        if isinstance(body, (dict, list)):
            body = json.dumps(body, ensure_ascii=False).encode()
        s.send_response(code)
        s.send_header("Content-Type", ctype)
        s.send_header("Content-Length", str(len(body)))
        s.send_header("Cache-Control", "no-store")
        s.end_headers()
        s.wfile.write(body)

    def body(s):
        return s.rfile.read(int(s.headers.get("Content-Length", 0)))

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path == "/api/brand":
            return s.send(brand_payload())
        if u.path == "/api/state":
            return s.send({"remote": True} if REMOTE else status.refresh())
        if u.path == "/api/app":
            return s.send(load_app())
        if u.path.startswith("/common/"):
            f = safe(COMMON, urllib.parse.unquote(u.path[len("/common/"):]))
            if not f:
                return s.send({"error": "not found"}, code=404)
            return s.send(f.read_bytes(), ctype=mimetypes.guess_type(f.name)[0] or "application/octet-stream")
        return super().do_GET()

    def do_POST(s):
        u = urllib.parse.urlparse(s.path)
        try:
            e = json.loads(s.body() or b"{}")
        except ValueError:
            return s.send({"error": "bad json"}, code=400)
        if REMOTE and u.path in ("/api/done", "/api/event"):
            remote_log("app.done" if u.path == "/api/done" else str(e.get("kind") or "app.event"), e)
            return s.send({"ok": True, "queued": True})
        if u.path == "/api/done":
            return s.done(e)
        if u.path == "/api/event":
            data = e.get("data") if isinstance(e.get("data"), dict) else {"value": e.get("data")}
            status.append_event(str(e.get("kind") or "app.event"), str(e.get("subject", "")), "human", {"app": load_app()["id"], **data})
            return s.send({"ok": True})
        s.send({"error": "no such route"}, code=404)

    def done(s, e):
        app = load_app()
        subj, note = str(e.get("subject", "")), str(e.get("note", ""))
        if not SUBJECT.match(subj):
            return s.send({"error": "bad subject"}, code=400)
        eff, node = app.get("effect", "none"), app.get("node", "")
        if eff not in ("human-done", "step-done", "none"):
            return s.send({"error": f"bad effect {eff}"}, code=500)
        if eff != "none" and node:
            argv = ["human-done", subj, node] + ([note] if note else []) if eff == "human-done" else ["step-done", subj, node, note]
            r = subprocess.run([sys.executable, str(ENGINE / "ai_write.py"), *argv], cwd=ROOT,
                               env={**os.environ, "VENTOSA_WORKSPACE": str(ROOT)}, capture_output=True, text=True)
            if r.returncode:
                return s.send({"error": (r.stdout + r.stderr).strip()}, code=409)
        status.append_event("app.done", subj, "human", {"app": app["id"], "node": node, "effect": eff, "note": note})
        v = status.refresh()["videos"].get(subj, {})
        s.send({"ok": True, "subject": subj, "stage": v.get("stage"), "owner": v.get("owner"), "need": v.get("need")})


if __name__ == "__main__":
    APP_DIR = pathlib.Path(sys.argv[1]).resolve()
    port = int(sys.argv[2])
    if not (APP_DIR / "app.json").is_file():
        sys.exit(f"serve_app: no app.json in {APP_DIR}")
    print(f"http://127.0.0.1:{port}/", flush=True)
    http.server.ThreadingHTTPServer(("0.0.0.0" if REMOTE else auth.bind(), port), Handler).serve_forever()
