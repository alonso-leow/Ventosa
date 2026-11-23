#!/usr/bin/env python3
"""Ventosa labelling app: http://127.0.0.1:<ports.labelling>/label.html
Saves labelling/labels.json (POST /api/labels). Every save is diffed into pipeline/events.jsonl
(video.new, label.approved, label.more_options, publish.approved) so the AI sees the work.
"""
import json, os, pathlib, sys, urllib.parse
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import common  # noqa: E402
from ws import ROOT, WS, WEB  # noqa: E402
HERE = ROOT / "labelling"
import scripts_lib as SL  # noqa: E402
F = HERE / "labels.json"


def scripts_get(u):
    q = urllib.parse.parse_qs(u.query)
    vid = q.get("vid", [""])[0]
    if u.path == "/api/scripts":
        return {"versions": SL.list_versions(vid), "active": SL.active_info(vid)}
    if u.path == "/api/script":
        slug = q.get("slug", [""])[0]
        t = SL.read(vid, slug)
        return {"slug": slug, "text": t, "errors": SL.validate(vid, t), "beats": SL.beats(t)}


def scripts_post(path, e):
    """CRUD on scripts/<VID>/<slug>.md. Activation = the human's choice: approves the label if needed."""
    vid, slug = e.get("vid", ""), e.get("slug", "")
    ev = common.status.append_event
    if path == "/api/script/save":
        SL.write(vid, slug, e["text"]); ev("script.saved", vid, "human", {"slug": slug})
    elif path == "/api/script/create":
        SL.create(vid, slug, e.get("from") or None, title=e.get("title", ""), option=e.get("option", ""), thesis=e.get("thesis", ""))
        ev("script.created", vid, "human", {"slug": slug, "from": e.get("from", "")})
    elif path == "/api/script/delete":
        SL.delete(vid, slug); ev("script.deleted", vid, "human", {"slug": slug})
    elif path == "/api/script/activate":
        d = json.loads(F.read_text()); v = d["videos"][vid]
        fm, _ = SL.split_fm(SL.read(vid, slug))
        if v["label"].get("status") != "labelled":
            opt = next((o for o in v.get("options", []) if o["key"] == fm.get("option")), None)
            v["thesis"] = fm.get("thesis", "") or v.get("thesis", "")
            if opt and not v.get("stack"):
                v["stack"] = [{"opt": opt["key"], "i": i, "text": t} for i, t in enumerate(opt["paras"])]
            v["label"] = {"status": "labelled", "rev": v["label"].get("rev", 0) + 1, "at": SL.now(), "by_version": slug}
            ev("label.approved", vid, "human", {"rev": v["label"]["rev"], "via_version": slug})
        vf = SL.activate(vid, slug, v["label"]["rev"])
        F.write_text(json.dumps(d, indent=1, ensure_ascii=False))
        ev("script.activated", vid, "human", {"slug": slug, "file": str(vf.relative_to(SL.ROOT)), "label_rev": v["label"]["rev"]})
    else:
        return None
    return {"ok": True, "versions": SL.list_versions(vid), "active": SL.active_info(vid), "labels": json.loads(F.read_text())}


def diff_events(old, new):
    ev = common.status.append_event
    for vid, v in new["videos"].items():
        p = old.get("videos", {}).get(vid)
        if not p:
            ev("video.new", vid, "human", {"title": v.get("title", ""), "notes": v.get("notes", "")}); continue
        if v["label"].get("rev", 0) > p["label"].get("rev", 0):
            ev("label.approved", vid, "human", {"rev": v["label"]["rev"], "thesis": v.get("thesis", ""), "paras": len(v.get("stack", []))})
        if v.get("want_more") and not p.get("want_more"):
            ev("label.more_options", vid, "human", {})
        a, b = p.get("publish", {}), v.get("publish", {})
        if b.get("approved") and not a.get("approved"):
            ev("publish.approved", vid, "human", {"title": b.get("title", "")})


def merge_ai(old, new):
    """Keep what the AI (or /api/uploaded) wrote while the page had an older copy."""
    for vid, o in old.get("videos", {}).items():
        v = new["videos"].get(vid)
        if v is None:
            new["videos"][vid] = o; continue  # the app never deletes videos
        if o.get("label", {}).get("rev", 0) > v.get("label", {}).get("rev", 0):  # server-side activation is newer
            for k in ("label", "thesis", "stack"):
                v[k] = o.get(k, v.get(k))
        known = {x["key"] for x in v.get("options", [])}
        extra = [x for x in o.get("options", []) if x["key"] not in known]
        if extra:
            v["options"] = v.get("options", []) + extra; v["want_more"] = o.get("want_more", False)
        op, vp = o.get("publish", {}), v.setdefault("publish", {})
        for k in ("titles", "descriptions"):
            vp[k] = vp.get(k, []) + [x for x in op.get(k, []) if x not in vp.get(k, [])]
        vp["uploaded"], vp["url"] = op.get("uploaded", False), op.get("url", "")


class H(common.Handler):
    app_id = "labelling"

    def app_for(s):
        # the uploads page shares this server; its page and its API calls count as "uploads"
        if s.path.startswith("/uploads.html") or "uploads.html" in (s.headers.get("Referer") or ""):
            return "uploads"
        return "labelling"

    app_dir = WEB

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path in ("/api/scripts", "/api/script"):
            try:
                return s.send(scripts_get(u))
            except (ValueError, FileNotFoundError) as e:
                return s.send({"error": str(e)}, code=400)
        return super().do_GET()

    def do_POST(s):
        u = urllib.parse.urlparse(s.path)
        if s.common_post(u) is not None:
            return
        if u.path.startswith("/api/script/"):
            try:
                r = scripts_post(u.path, json.loads(s.body() or b"{}"))
            except (ValueError, FileNotFoundError, KeyError) as e:
                return s.send({"error": str(e)}, code=400)
            if r is None:
                return s.send_error(404)
            common.status.refresh()
            return s.send(r)
        if u.path == "/api/labels":
            new = json.loads(s.body())
            if not isinstance(new.get("videos"), dict):
                return s.send_error(400)
            old = json.loads(F.read_text()) if F.exists() else {"videos": {}}
            merge_ai(old, new)
            F.write_text(json.dumps(new, indent=1, ensure_ascii=False))
            diff_events(old, new)
            return s.send(common.status.refresh())
        s.send_error(404)


if __name__ == "__main__":
    common.serve(H, int(os.environ.get("VENTOSA_PORT") or WS["ports"]["labelling"]), "label.html")
