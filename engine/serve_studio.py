#!/usr/bin/env python3
"""Ventosa label studio: http://127.0.0.1:<ports.studio>/studio.html

Labels a dataset of timelines from **rendered video files** (not frames drawn
live): the studio renders each example once with the workspace's render
command, caches the video by the example's content, and the person labels the
playing file — a verdict, notes, and marks at moments of the timeline.

Config (first found wins, merged over the defaults): workspace.json "studio",
$XDG_CONFIG_HOME/ventosa/studio.json.
  examples_dir   folder of example JSON files (one timeline each)
  render         command; {example} {out} {id} {w} {h} are filled in; it must
                 write one .mp4 into {out}
  cache_dir      where rendered videos are kept (<id>-<hash>.mp4)
  status_file    {id: approved|pending|rejected} (the app that trains reads it)
  labels_file    full labels: verdict, notes, marks [{t, tick, tag, note}], by, at
  concurrency    render workers (default 1)
  rate           ticks per second of the timeline (default 32)

GET  /api/studio                 config + every example with its cache and label state
GET  /api/example?id=            the example's metadata and keys (ticks)
GET  /video/<id>.mp4             the cached render (range requests)
POST /api/render {"ids":[...], "force":false}   queue renders (bulk)
POST /api/render/cancel          drop queued renders
POST /api/label {"id", "verdict", "notes", "marks"}   save a label (+ status_file)
"""
import hashlib, json, os, pathlib, queue, shlex, shutil, subprocess, sys, tempfile, threading, time, urllib.parse
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import common  # noqa: E402
from ws import ROOT, WEB, WS  # noqa: E402

HOME = pathlib.Path.home()
XDG_CONFIG = pathlib.Path(os.environ.get("XDG_CONFIG_HOME") or HOME / ".config")
XDG_CACHE = pathlib.Path(os.environ.get("XDG_CACHE_HOME") or HOME / ".cache")
DEFAULTS = {
    # MetalVJ's Colourist dataset: the example-proof renderer writes <out>/<id>.mp4 with --video
    "examples_dir": str(XDG_CACHE / "metalvj/data/dataset/colourist/examples"),
    "render": "example-proof --example {example} --out {out} --video --w {w} --h {h}",
    "renderer": str(XDG_CACHE / "metalvj/target/release/example-proof"),
    "cache_dir": str(XDG_CACHE / "ventosa/studio" / WS["name"]),
    "status_file": str(HOME / ".local/share/metalvj/review-status.json"),
    "labels_file": "",
    "concurrency": 1,
    "rate": 32,
    "w": 640, "h": 360,
    "tags": ["good", "drift", "too busy", "too dark", "washed", "off beat", "hook", "fix"],
}


def config():
    c = dict(DEFAULTS)
    f = XDG_CONFIG / "ventosa/studio.json"
    if f.exists():
        try:
            c.update(json.loads(f.read_text()))
        except ValueError:
            pass
    c.update(WS.get("studio") or {})
    ex = pathlib.Path(os.path.expanduser(c["examples_dir"]))
    c["examples_dir"] = str(ex)
    if not c["labels_file"]:
        c["labels_file"] = str(ex.parent / "studio-labels.json")
    return c


CFG = config()
EX = pathlib.Path(CFG["examples_dir"])
CACHE = pathlib.Path(os.path.expanduser(CFG["cache_dir"]))
LOCK = threading.Lock()
JOBS = {}          # id -> {"state": queued|rendering|done|failed, "log": str, "at": float}
Q = queue.Queue()


def renderer_cmd():
    """The render command with its binary resolved (the configured path, else PATH)."""
    cmd = CFG["render"]
    exe = shlex.split(cmd)[0]
    r = pathlib.Path(os.path.expanduser(CFG.get("renderer") or ""))
    if shutil.which(exe) is None and r.is_file():
        cmd = shlex.quote(str(r)) + cmd[len(exe):]
    return cmd


def key_of(p):
    """Cache key: the example's bytes + the command + the renderer binary's mtime."""
    h = hashlib.sha1(p.read_bytes())
    h.update(CFG["render"].encode())
    exe = shutil.which(shlex.split(renderer_cmd())[0]) or ""
    if exe:
        h.update(str(os.stat(exe).st_mtime_ns).encode())
    return h.hexdigest()[:12]


def cached(eid):
    p = EX / f"{eid}.json"
    if not p.is_file():
        return None
    f = CACHE / f"{eid}-{key_of(p)}.mp4"
    return f if f.is_file() else None


def jload(p, default):
    try:
        return json.loads(pathlib.Path(p).read_text())
    except (OSError, ValueError):
        return default


def worker():
    while True:
        eid = Q.get()
        with LOCK:
            if JOBS.get(eid, {}).get("state") != "queued":
                continue
            JOBS[eid] = {"state": "rendering", "log": "", "at": time.time()}
        p = EX / f"{eid}.json"
        out = pathlib.Path(tempfile.mkdtemp(prefix=f"studio-{eid}-"))
        cmd = renderer_cmd().format(example=shlex.quote(str(p)), out=shlex.quote(str(out)), id=eid, w=CFG["w"], h=CFG["h"])
        try:
            r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=3600)
            vids = sorted(out.rglob("*.mp4"), key=lambda f: f.stat().st_mtime)
            if r.returncode == 0 and vids:
                CACHE.mkdir(parents=True, exist_ok=True)
                for old in CACHE.glob(f"{eid}-*.mp4"):
                    old.unlink(missing_ok=True)
                shutil.move(str(vids[-1]), CACHE / f"{eid}-{key_of(p)}.mp4")
                state, log = "done", ""
            else:
                state, log = "failed", (r.stderr or r.stdout or "no .mp4 written")[-1500:]
        except (subprocess.TimeoutExpired, OSError) as e:
            state, log = "failed", str(e)
        shutil.rmtree(out, ignore_errors=True)
        with LOCK:
            JOBS[eid] = {"state": state, "log": log, "at": time.time()}


for _ in range(max(1, int(CFG.get("concurrency", 1)))):
    threading.Thread(target=worker, daemon=True).start()


def listing():
    labels = jload(CFG["labels_file"], {})
    status = jload(CFG["status_file"], {})
    rows = []
    for p in sorted(EX.glob("*.json")):
        eid = p.stem
        try:
            d = json.loads(p.read_text())
        except ValueError:
            continue
        lab = labels.get(eid, {})
        with LOCK:
            job = dict(JOBS.get(eid, {}))
        rows.append({"id": eid, "model": d.get("model", ""), "song": d.get("song", ""), "title": d.get("title", ""), "kind": d.get("kind", ""),
                     "keys": len(d.get("keys", [])), "span": d.get("span"), "cached": bool(cached(eid)),
                     "job": job.get("state", ""), "log": job.get("log", ""),
                     "verdict": lab.get("verdict") or status.get(eid, ""), "marks": len(lab.get("marks", [])), "notes": lab.get("notes", "")})
    cfg = {k: CFG[k] for k in ("examples_dir", "render", "cache_dir", "status_file", "labels_file", "rate", "tags")}
    cfg["renderer_found"] = shutil.which(shlex.split(renderer_cmd())[0]) is not None
    return {"config": cfg, "examples": rows, "queued": sum(1 for j in JOBS.values() if j["state"] in ("queued", "rendering"))}


class H(common.Handler):
    app_id = "studio"
    app_dir = WEB

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        q = urllib.parse.parse_qs(u.query)
        if u.path == "/api/studio":
            return s.send(listing())
        if u.path == "/api/example":
            eid = q.get("id", [""])[0]
            p = EX / f"{eid}.json"
            if not p.is_file() or "/" in eid:
                return s.send_error(404)
            d = json.loads(p.read_text())
            lab = jload(CFG["labels_file"], {}).get(eid, {})
            return s.send({"id": eid, "meta": {k: d.get(k) for k in ("model", "song", "title", "story", "kind", "keywords", "span", "part")},
                           "keys": [{"tick": k.get("tick"), "n": len(k.get("set", {})), "why": k.get("why", "")} for k in d.get("keys", [])],
                           "rate": d.get("rate", CFG["rate"]), "label": lab, "cached": bool(cached(eid))})
        if u.path.startswith("/video/") and u.path.endswith(".mp4"):
            f = cached(urllib.parse.unquote(u.path[7:-4]))
            return s.file(f) if f else s.send_error(404)
        return common.Handler.do_GET(s)

    def do_POST(s):
        u = urllib.parse.urlparse(s.path)
        e = json.loads(s.body() or b"{}")
        if u.path == "/api/render":
            n = 0
            for eid in e.get("ids", []):
                if "/" in eid or not (EX / f"{eid}.json").is_file():
                    continue
                if cached(eid) and not e.get("force"):
                    continue
                with LOCK:
                    if JOBS.get(eid, {}).get("state") in ("queued", "rendering"):
                        continue
                    JOBS[eid] = {"state": "queued", "log": "", "at": time.time()}
                Q.put(eid); n += 1
            return s.send({"queued": n})
        if u.path == "/api/render/cancel":
            with LOCK:
                for k, j in JOBS.items():
                    if j["state"] == "queued":
                        JOBS[k] = {"state": "", "log": "cancelled", "at": time.time()}
            return s.send({"ok": True})
        if u.path == "/api/label":
            eid = e.get("id", "")
            if "/" in eid or not (EX / f"{eid}.json").is_file():
                return s.send_error(400)
            verdict = e.get("verdict", "pending")
            if verdict not in ("approved", "pending", "rejected"):
                return s.send_error(400)
            with LOCK:
                labels = jload(CFG["labels_file"], {})
                marks = [{"t": float(m.get("t", 0)), "tick": int(m.get("tick", 0)), "tag": str(m.get("tag", ""))[:40], "note": str(m.get("note", ""))[:400]} for m in e.get("marks", [])]
                labels[eid] = {"verdict": verdict, "notes": str(e.get("notes", ""))[:4000], "marks": sorted(marks, key=lambda m: m["t"]),
                               "by": s.auth_user or "local", "at": int(time.time())}
                pathlib.Path(CFG["labels_file"]).write_text(json.dumps(labels, indent=1, ensure_ascii=False))
                # the status file keeps only pending|approved (the trainer's contract); rejected counts as pending there
                sf = pathlib.Path(os.path.expanduser(CFG["status_file"]))
                st = jload(sf, {})
                st[eid] = "approved" if verdict == "approved" else "pending"
                sf.parent.mkdir(parents=True, exist_ok=True)
                sf.write_text(json.dumps(dict(sorted(st.items())), indent=1))
            return s.send({"ok": True, "label": labels[eid]})
        return s.send_error(404)


if __name__ == "__main__":
    port = int(os.environ.get("VENTOSA_PORT") or WS["ports"].get("studio", 8769))
    common.serve(H, port, "studio.html")
