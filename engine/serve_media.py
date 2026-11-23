#!/usr/bin/env python3
"""Ventosa media-labelling app: http://127.0.0.1:<ports.media-labelling>/media-label.html
Reproduction of what each AI phase made, with an approve / send-back for the human step after it.
GET  /api/review          the human approval steps waiting, each with the files of the phase before it
GET  /file?path=REL       a workspace file (Range supported), only inside the workspace
POST /api/approve         {"vid","step","note"}            human-done for that step
POST /api/reject          {"vid","step","back_to","note"}  reopen the earlier AI step with a note
"""
import json, os, pathlib, sys, urllib.parse
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import common  # noqa: E402
import status  # noqa: E402
from ws import ROOT, WEB, WS  # noqa: E402

MEDIA_EXT = {".mp4", ".mov", ".webm", ".mkv", ".mp3", ".wav", ".m4a", ".flac", ".ogg", ".opus", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".md", ".txt", ".json", ".srt", ".vtt"}


def phase_files(vid, step, evs):
    """Files recorded on the newest done event of an AI phase for this subject."""
    out = []
    for e in reversed(evs):
        d = e.get("data") or {}
        if e.get("vid") != vid:
            continue
        if e["kind"] == "step.done" and d.get("step") == step or (step == "edit" and e["kind"] == "ai.edit.done") or (step == "subtitles" and e["kind"] == "ai.subtitles.done"):
            out = [f["path"] for f in d.get("files", [])] + [d[k] for k in ("file", "srt") if d.get(k)]
            break
    return out


def review():
    st = status.refresh()
    evs = status.events()
    ids = [x["id"] for x in status.GRAPH["steps"]]
    items = []
    for vid, v in st["videos"].items():
        step = v["stage"]
        node = next((x for x in status.GRAPH["steps"] if x["id"] == step), None)
        if not node or node["owner"] != "human" or step not in ids:
            continue
        i = ids.index(step)
        back = next((ids[j] for j in range(i - 1, -1, -1) if status.OWNER.get(ids[j]) == "ai"), "")
        files = phase_files(vid, back, evs) if back else []
        if not files and not step.endswith("_ok") and step != "edit_review" and step != "plan_ok":
            continue
        items.append({"vid": vid, "title": v["title"], "step": step, "need": node["need"], "back_to": back,
                      "files": [{"path": f, "exists": (ROOT / f).is_file(), "kind": pathlib.Path(f).suffix.lower().lstrip(".")} for f in files if pathlib.Path(f).suffix.lower() in MEDIA_EXT]})
    return {"items": items}


class H(common.Handler):
    app_id = "media-labelling"
    app_dir = WEB

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path == "/api/review":
            return s.send(review())
        if u.path == "/file":
            f = common.safe(ROOT, urllib.parse.parse_qs(u.query).get("path", [""])[0])
            return s.file(f) if f else s.send_error(404)
        return common.Handler.do_GET(s)

    def do_POST(s):
        u = urllib.parse.urlparse(s.path)
        if s.common_post(u):
            return
        e = json.loads(s.body() or b"{}")
        vid, step = str(e.get("vid", "")), str(e.get("step", ""))
        if u.path == "/api/approve":
            status.append_event("step.done", vid, "human", {"step": step, "note": str(e.get("note", "ok"))})
        elif u.path == "/api/reject":
            back = str(e.get("back_to", ""))
            if back not in [x["id"] for x in status.GRAPH["steps"]]:
                return s.send({"error": "unknown step"}, code=400)
            status.append_event("step.reopen", vid, "human", {"step": back, "note": str(e.get("note", ""))})
        else:
            return s.send_error(404)
        s.send(review())


if __name__ == "__main__":
    port = int(os.environ.get("VENTOSA_PORT") or WS["ports"].get("media-labelling", 8771))
    common.serve(H, port, "media-label.html")
