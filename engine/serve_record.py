#!/usr/bin/env python3
"""Ventosa teleprompter app: http://127.0.0.1:<ports.teleprompter>/teleprompter.html
Saves clips.json (POST /api/clips) and takes (POST /api/upload?name=V02-B1-t1.webm -> assets/camera/).
External camera: files pasted into the inbox (recording/config.json, default assets/camera/inbox/) are listed
(GET /api/inbox, ffprobe + thumbnails), previewed (GET /inbox/<name>) and imported as takes (POST /api/import).
Every save is diffed and logged to pipeline/events.jsonl so the AI sees the recording work.
Shared routes (/api/pipeline, /api/event, /common, /media) come from pipeline/common.py.
"""
import datetime, json, pathlib, re, shutil, subprocess, sys, urllib.parse
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT, WS, WEB, ENGINE  # noqa: E402
HERE = ROOT / "recording"; HERE.mkdir(parents=True, exist_ok=True)
import common  # noqa: E402
CAM = ROOT / "assets" / "camera"; CAM.mkdir(parents=True, exist_ok=True)
CFG = HERE / "config.json"
VIDEO_EXT = {".mp4", ".mov", ".mts", ".m2ts", ".mkv", ".webm", ".avi", ".mxf", ".wav", ".m4a", ".mp3"}
_probe = {}


def inbox_dir():
    c = json.loads(CFG.read_text()) if CFG.exists() else {}
    p = pathlib.Path(c.get("inbox", "assets/camera/inbox")).expanduser()
    p = p if p.is_absolute() else ROOT / p
    p.mkdir(parents=True, exist_ok=True)
    return p


def probe(f):
    key = (f.name, f.stat().st_mtime, f.stat().st_size)
    if key in _probe:
        return _probe[key]
    info = {}
    try:
        r = subprocess.run(["ffprobe", "-v", "error", "-print_format", "json", "-show_format", "-show_streams", str(f)], capture_output=True, text=True, timeout=20)
        j = json.loads(r.stdout or "{}")
        fmt = j.get("format", {})
        v = next((x for x in j.get("streams", []) if x.get("codec_type") == "video"), {})
        tags = {k.lower(): v2 for k, v2 in (fmt.get("tags") or {}).items()}
        info = {"duration": float(fmt.get("duration", 0) or 0), "vcodec": v.get("codec_name", ""), "width": v.get("width", 0), "height": v.get("height", 0),
                "created": tags.get("creation_time", ""), "audio_only": not v}
    except Exception as e:  # unreadable file: still listed
        info = {"error": str(e)}
    _probe[key] = info
    return info


def inbox_list():
    d = inbox_dir(); out = []
    for f in sorted(d.iterdir()):
        if f.is_file() and f.suffix.lower() in VIDEO_EXT:
            st = f.stat()
            i = probe(f)
            created = i.get("created") or datetime.datetime.fromtimestamp(st.st_mtime, datetime.timezone.utc).isoformat()
            out.append({**i, "name": f.name, "size": st.st_size, "created": created, "created_from": "camera" if i.get("created") else "file mtime",
                        "playable": i.get("vcodec") in ("h264", "vp8", "vp9", "av1") or i.get("audio_only", False)})
    out.sort(key=lambda x: x["created"])
    return {"inbox": str(d), "files": out}


def thumb(name):
    src = inbox_dir() / name
    if not src.is_file() or src.name != name:
        return None
    tdir = CAM / ".thumbs"; tdir.mkdir(exist_ok=True)
    t = tdir / (name + ".jpg")
    if not t.exists() or t.stat().st_mtime < src.stat().st_mtime:
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", "1", "-i", str(src), "-frames:v", "1", "-vf", "scale=320:-2", str(t)], capture_output=True, timeout=30)
    return t if t.exists() else None


def do_import(assign):
    """Move inbox files to assets/camera/<line>-t<take><ext> and record them as takes (kept / discarded)."""
    f = HERE / "clips.json"
    old = json.loads(f.read_text()); new = json.loads(f.read_text())
    lines = {l["id"]: l for v in new["videos"].values() for l in v["lines"]}
    d = inbox_dir(); done = []
    for a in assign:
        dec = a.get("decision")
        if dec not in ("kept", "discarded"):
            continue
        src = d / a["name"]
        l = lines.get(a.get("line", ""))
        if not src.is_file() or src.name != a["name"] or not l:
            continue
        ext = src.suffix.lower()
        target_dir = CAM if dec == "kept" else CAM / "discarded"
        target_dir.mkdir(exist_ok=True)
        dst = target_dir / f"{l['id']}-t{l['take']}{ext}"
        n = 1
        while dst.exists():
            n += 1; dst = target_dir / f"{l['id']}-t{l['take']}-{n}{ext}"
        shutil.move(str(src), dst)
        rel = str(dst.relative_to(ROOT))
        l["takes"].append({"n": l["take"], "file": rel, "status": dec, "at": datetime.datetime.now().isoformat(timespec="seconds"), "source": "external", "original": a["name"]})
        if dec == "kept":
            l["status"] = "recorded"; l["decision"] = ""
        else:
            l["status"] = "discarded"; l["decision"] = ""; l["take"] += 1
        l.pop("marks", None) if dec == "kept" else None
        done.append({"name": a["name"], "file": rel, "line": l["id"], "decision": dec})
    new["updated"] = datetime.datetime.now().isoformat(timespec="seconds")
    f.write_text(json.dumps(new, ensure_ascii=False))
    diff_events(old, new)
    subprocess.run([sys.executable, str(ENGINE / "index_only.py")], check=False)
    return {"imported": done, "clips": new}


ROUGH = ROOT / "assets" / "edits" / "rough"
_rough = {}


def rough_start(vid):
    if not re.fullmatch(r"V\d+", vid):
        return {"error": "bad video id"}
    if vid in _rough and _rough[vid].poll() is None:
        return rough_state(vid)
    ROUGH.mkdir(parents=True, exist_ok=True)
    log = (ROUGH / f"{vid}.log").open("w")
    _rough[vid] = subprocess.Popen([sys.executable, str(ENGINE / "edit.py"), vid, "--rough"], stdout=log, stderr=subprocess.STDOUT, cwd=ROOT)
    common.status.append_event("record.roughcut", vid, "human", {})
    return rough_state(vid)


def rough_state(vid):
    p = _rough.get(vid)
    lf, out = ROUGH / f"{vid}.log", ROUGH / f"{vid}-rough.mp4"
    return {"running": bool(p and p.poll() is None), "exit": p.poll() if p else None,
            "log": lf.read_text()[-4000:] if lf.exists() else "", "file": f"edits/rough/{vid}-rough.mp4" if out.exists() else "",
            "mtime": out.stat().st_mtime if out.exists() else 0}


def diff_events(old, new):
    o = {l["id"]: l for v in old.get("videos", {}).values() for l in v["lines"]}
    for vid, v in new.get("videos", {}).items():
        for l in v["lines"]:
            p = o.get(l["id"])
            if not p or p["status"] != l["status"] or len(p.get("takes", [])) != len(l.get("takes", [])):
                common.status.append_event(f"record.{l['status']}", vid, "human", {"line": l["id"], "take": l.get("take"), "decision": l.get("decision", "")})
        ids = {l["id"] for l in v["lines"]}
        for lid in (k for k, p in o.items() if k.startswith(vid + "-") and k not in ids):
            common.status.append_event("record.dropped", vid, "human", {"line": lid})


SESS = HERE / "sessions"


def tplog_append(body):
    """Append teleprompter events ({session, events:[{t, ev, ...}]}) to recording/sessions/<session>.jsonl."""
    sid = re.sub(r"[^\w.-]", "", str(body.get("session", "")))[:64]
    evs = [e for e in body.get("events", []) if isinstance(e, dict) and e.get("t") and e.get("ev")]
    if not sid or not evs:
        return {"error": "need session and events"}
    SESS.mkdir(parents=True, exist_ok=True)
    with (SESS / f"{sid}.jsonl").open("a") as f:
        for e in evs:
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
    return {"saved": len(evs)}


class H(common.Handler):
    app_id = "teleprompter"
    app_dir = WEB

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path == "/api/inbox":
            return s.send(inbox_list())
        if u.path == "/api/roughcut":
            return s.send(rough_state(urllib.parse.parse_qs(u.query).get("vid", [""])[0]))
        if u.path == "/api/thumb":
            t = thumb(urllib.parse.parse_qs(u.query).get("name", [""])[0])
            return s.file(t) if t else s.send_error(404)
        if u.path.startswith("/inbox/"):
            f = common.safe(inbox_dir(), urllib.parse.unquote(u.path[7:]))
            return s.file(f) if f else s.send_error(404)
        return super().do_GET()

    def do_POST(s):
        u = urllib.parse.urlparse(s.path)
        if s.common_post(u) is not None:
            return
        if u.path == "/api/clips":
            body = s.body(); new = json.loads(body)
            f = HERE / "clips.json"
            old = json.loads(f.read_text()) if f.exists() else {}
            f.write_text(body.decode())
            diff_events(old, new)
            subprocess.run([sys.executable, str(ENGINE / "index_only.py")], check=False)
            return s.send()
        if u.path == "/api/roughcut":
            return s.send(rough_start(urllib.parse.parse_qs(u.query).get("vid", [""])[0]))
        if u.path == "/api/import":
            return s.send(do_import(json.loads(s.body() or b"{}").get("assign", [])))
        if u.path == "/api/inbox/config":
            p = pathlib.Path(json.loads(s.body() or b"{}").get("inbox", "")).expanduser()
            if not p.is_dir():
                return s.send({"error": f"not a folder: {p}"}, code=400)
            CFG.write_text(json.dumps({"inbox": str(p)}))
            return s.send(inbox_list())
        if u.path == "/api/tplog":
            return s.send(tplog_append(json.loads(s.body() or b"{}")))
        if u.path == "/api/upload":
            name = urllib.parse.parse_qs(u.query).get("name", [""])[0]
            if not re.fullmatch(r"[\w.-]+\.(webm|weba|mp4|ogg|wav)", name):
                return s.send_error(400)
            (CAM / name).write_bytes(s.body())
            return s.send({"file": f"assets/camera/{name}"})
        s.send_error(404)


if __name__ == "__main__":
    common.serve(H, WS["ports"]["teleprompter"], "teleprompter.html")
