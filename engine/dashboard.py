"""Dashboard data for a workspace (stdlib only): GET /api/dashboard?days=N

- uploads: per platform (and total), from labels.json publish blocks
  (`url`, or `platforms: {name: url}`) with the upload time from the
  `publish.uploaded` events.
- tokens: per subject, the provider-reported tokens from Ventosa's usage log
  ($XDG_DATA_HOME/ventosa/usage.jsonl), split by step, in the window.
- phases: per stage, how many subjects entered it, how many it advanced
  (an `ai.detach` with a result, or a human event of that stage followed by a
  later stage) and how many are there now; completed = subjects at done.
"""
import datetime as dt, json, os, pathlib, sys, time, urllib.parse

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import status  # noqa: E402
from ws import ROOT, WS  # noqa: E402

PLATFORMS = [("youtube", "YouTube"), ("youtu.be", "YouTube"), ("tiktok", "TikTok"), ("instagram", "Instagram"),
             ("facebook", "Facebook"), ("x.com", "X"), ("twitter", "X"), ("linkedin", "LinkedIn"), ("vimeo", "Vimeo"),
             ("spotify", "Spotify"), ("apple.com", "Apple Podcasts"), ("twitch", "Twitch")]


def ts(v):
    if isinstance(v, (int, float)):
        return float(v)
    try:
        return dt.datetime.fromisoformat(str(v)).timestamp()
    except ValueError:
        return 0.0


def platform_of(url, default):
    host = urllib.parse.urlparse(url).netloc.lower()
    for k, name in PLATFORMS:
        if k in host:
            return name
    return default if not host else host.removeprefix("www.")


def stage_of(kind, data, order):
    """The stage an event belongs to (mirror of graph::event_step, plus generic `<stage>.` kinds)."""
    k = kind
    if k in ("ai.detach", "ai.attach", "step.done", "step.reopen"):
        return data.get("step")
    if k == "app.done":
        return data.get("node")
    table = [("ai.options", "options"), ("ai.video.new", "options"), ("video.new", "options"), ("label.more_options", "options"),
             ("ai.draft", "drafts"), ("label.", "label"), ("script.", "label"), ("ai.script", "script"), ("record.", "record"),
             ("ai.edit", "edit"), ("ai.subtitles", "subtitles"), ("ai.publish_meta", "publish_meta"), ("publish.approved", "publish"), ("publish.unapproved", "publish"),
             ("publish.", "upload")]
    for p, s in table:
        if k.startswith(p):
            return s
    head = k.removeprefix("ai.").split(".")[0]
    return head if head in order else None


def usage_path():
    base = pathlib.Path(os.environ.get("XDG_DATA_HOME") or pathlib.Path.home() / ".local/share")
    return base / "ventosa" / "usage.jsonl"


def build(days=30):
    since = time.time() - days * 86400 if days else 0
    state = status.refresh()
    stages = [s["id"] for s in state.get("stages", []) if s.get("id")]
    videos = state.get("videos", {})
    evs = [e for e in status.events() if ts(e.get("at")) >= since] if hasattr(status, "events") else []
    if not evs:
        f = ROOT / "pipeline" / "events.jsonl"
        if f.exists():
            for line in f.read_text().splitlines():
                try:
                    e = json.loads(line)
                except ValueError:
                    continue
                if ts(e.get("at")) >= since:
                    evs.append(e)

    # uploads
    labels = {}
    lf = ROOT / "labelling" / "labels.json"
    if lf.exists():
        try:
            labels = json.loads(lf.read_text()).get("videos", {})
        except ValueError:
            pass
    up_at = {e["vid"]: ts(e["at"]) for e in evs if e.get("kind") == "publish.uploaded"}
    default = WS["platform"]["name"]
    per = {}
    uploads = []
    for vid, v in labels.items():
        pub = v.get("publish") or {}
        links = dict(pub.get("platforms") or {})
        if pub.get("uploaded") and pub.get("url"):
            links.setdefault(platform_of(pub["url"], default), pub["url"])
        elif pub.get("uploaded"):
            links.setdefault(default, "")
        for name, url in links.items():
            at = up_at.get(vid, 0)
            if since and at and at < since:
                continue
            per[name] = per.get(name, 0) + 1
            uploads.append({"vid": vid, "title": v.get("title", videos.get(vid, {}).get("title", "")), "platform": name, "url": url, "at": at})

    # tokens per subject × step
    tokens = {}
    up = usage_path()
    ws_name = os.environ.get("VENTOSA_WS_NAME") or WS["name"]
    if up.exists():
        for line in up.read_text().splitlines():
            try:
                u = json.loads(line)
            except ValueError:
                continue
            if u.get("workspace") not in (ws_name, WS["name"], ROOT.name) or ts(u.get("at", 0)) < since:
                continue
            sub = u.get("subject") or "(chat)"
            step = u.get("step") or u.get("source") or "chat"
            t = tokens.setdefault(sub, {"input": 0, "output": 0, "steps": {}})
            i, o = int(u.get("input", 0)), int(u.get("output", 0))
            t["input"] += i; t["output"] += o
            s = t["steps"].setdefault(step, {"input": 0, "output": 0})
            s["input"] += i; s["output"] += o
    token_rows = [{"vid": k, "title": videos.get(k, {}).get("title", ""), **v} for k, v in tokens.items()]
    token_rows.sort(key=lambda r: r["vid"])

    # phases
    entered = {s: set() for s in stages}
    advanced = {s: set() for s in stages}
    worked = {s: 0 for s in stages}
    for e in evs:
        s = stage_of(e.get("kind", ""), e.get("data") or {}, stages)
        if s in worked:
            worked[s] += 1
            entered[s].add(e.get("vid"))
        if e.get("kind") == "ai.detach":
            r = (e.get("data") or {}).get("result")
            if s in advanced and r and r != s:
                advanced[s].add(e.get("vid"))
            if r in entered:
                entered[r].add(e.get("vid"))
    # a human stage is advanced when the subject was later seen at a later stage
    pos = {s: i for i, s in enumerate(stages)}
    for vid, v in videos.items():
        now_at = pos.get(v.get("stage"), -1)
        for s in stages:
            if vid in entered[s] and pos[s] < now_at:
                advanced[s].add(vid)
    now = {s: sum(1 for v in videos.values() if v.get("stage") == s) for s in stages}
    phases = [{"id": s, "owner": next((x.get("owner", "") for x in state["stages"] if x.get("id") == s), ""),
               "entered": len(entered[s]), "advanced": len(advanced[s]), "now": now[s], "events": worked[s]} for s in stages]
    completed = [vid for vid, v in videos.items() if v.get("stage") == "done"]
    return {"workspace": ws_name, "days": days, "generated": int(time.time()),
            "uploads": {"per_platform": per, "total": sum(per.values()), "rows": uploads},
            "tokens": token_rows, "phases": phases,
            "completed": {"count": len(completed), "subjects": completed},
            "subjects": len(videos)}


if __name__ == "__main__":
    print(json.dumps(build(int(sys.argv[1]) if len(sys.argv) > 1 else 30), indent=1, ensure_ascii=False))
