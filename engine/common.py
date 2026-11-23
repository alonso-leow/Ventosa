"""Shared HTTP handler for the labelling and teleprompter apps (localhost only).

GET  /common/<file>         shared web components (queue monitor, upload list)
GET  /media/<path>          files under assets/ (video playback, drag-out)
GET  /api/pipeline          refreshed pipeline.json
GET  /api/brand             workspace brand (name, accent, unit, platform, agent command)
POST /api/event             {"kind","vid","data"} appended as a human event
POST /api/reveal?path=...   open the file manager on a file under assets/
"""
import http.server, json, pathlib, subprocess, sys, urllib.parse, mimetypes

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import status  # noqa: E402
import auth  # noqa: E402
from ws import ROOT, WS, WEB, brand_payload  # noqa: E402

COMMON = WEB / "common"
ASSETS = ROOT / "assets"


def safe(base, rel):
    p = (base / rel).resolve()
    return p if p.is_relative_to(base.resolve()) and p.is_file() else None


class Handler(http.server.SimpleHTTPRequestHandler):
    app_dir = ROOT
    # the app id Basic auth checks against the user's role (see auth.py)
    app_id = "labelling"
    auth_user = None

    def app_for(s):
        return s.app_id

    def parse_request(s):
        return super().parse_request() and auth.guard(s, s.app_for())

    def __init__(s, *a, **k):
        super().__init__(*a, directory=str(s.app_dir), **k)

    def log_message(s, *a):
        pass

    def send(s, body=b"{}", ctype="application/json", code=200):
        if isinstance(body, (dict, list)):
            body = json.dumps(body, ensure_ascii=False).encode()
        s.send_response(code); s.send_header("Content-Type", ctype); s.send_header("Content-Length", str(len(body)))
        s.send_header("Cache-Control", "no-store"); s.end_headers(); s.wfile.write(body)

    def body(s):
        return s.rfile.read(int(s.headers.get("Content-Length", 0)))

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path == "/api/pipeline":
            return s.send(status.refresh())
        if u.path == "/api/brand":
            return s.send(brand_payload())
        data = {"/labels.json": ROOT / "labelling" / "labels.json", "/clips.json": ROOT / "recording" / "clips.json"}
        if u.path in data:
            f = data[u.path]
            return s.send(f.read_bytes() if f.exists() else b'{"videos": {}}')
        for prefix, base in (("/common/", COMMON), ("/media/", ASSETS)):
            if u.path.startswith(prefix):
                f = safe(base, urllib.parse.unquote(u.path[len(prefix):]))
                if not f:
                    return s.send_error(404)
                return s.file(f)
        return super().do_GET()

    def file(s, f):
        size = f.stat().st_size
        ctype = mimetypes.guess_type(f.name)[0] or "application/octet-stream"
        rng = s.headers.get("Range")
        start, end, code = 0, size - 1, 200
        if rng and rng.startswith("bytes="):
            a, _, b = rng[6:].partition("-")
            start = int(a or 0); end = int(b) if b else size - 1; code = 206
        s.send_response(code); s.send_header("Content-Type", ctype); s.send_header("Accept-Ranges", "bytes")
        s.send_header("Content-Length", str(end - start + 1))
        if code == 206:
            s.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        s.end_headers()
        with f.open("rb") as fh:
            fh.seek(start); left = end - start + 1
            while left > 0:
                chunk = fh.read(min(1 << 20, left))
                if not chunk:
                    break
                try:
                    s.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return
                left -= len(chunk)

    def common_post(s, u):
        if u.path == "/api/event":
            e = json.loads(s.body() or b"{}")
            status.append_event(str(e.get("kind", "note")), str(e.get("vid", "")), "human", e.get("data", {}))
            s.send(status.refresh()); return True
        if u.path == "/api/uploaded":
            e = json.loads(s.body() or b"{}")
            lf = ROOT / "labelling" / "labels.json"
            d = json.loads(lf.read_text())
            pub = d["videos"][e["vid"]].setdefault("publish", {})
            pub["uploaded"] = bool(e.get("uploaded", True)); pub["url"] = str(e.get("url", ""))
            lf.write_text(json.dumps(d, indent=1, ensure_ascii=False))
            status.append_event("publish.uploaded" if pub["uploaded"] else "publish.unuploaded", e["vid"], "human", {"url": pub["url"]})
            s.send(status.refresh()); return True
        if u.path == "/api/reveal":
            rel = urllib.parse.parse_qs(u.query).get("path", [""])[0]
            f = safe(ROOT, rel)
            if not f or not f.is_relative_to(ASSETS.resolve()):
                s.send_error(400); return True
            uri = f.as_uri()
            r = subprocess.run(["gdbus", "call", "--session", "--dest", "org.freedesktop.FileManager1", "--object-path",
                                "/org/freedesktop/FileManager1", "--method", "org.freedesktop.FileManager1.ShowItems", f"['{uri}']", ""],
                               capture_output=True)
            if r.returncode:
                subprocess.Popen(["xdg-open", str(f.parent)])
            s.send({"ok": True, "path": str(f)}); return True
        return None


def serve(handler, port, page):
    print(f"http://127.0.0.1:{port}/{page}")
    http.server.ThreadingHTTPServer((auth.bind(), port), handler).serve_forever()
