#!/usr/bin/env python3
"""Ventosa skill graph: computes each subject's pipeline stage from the files both apps write.

Sources of truth (nothing else is read):
  labelling/labels.json   options, chosen stack + thesis, publish texts   (labelling app)
  recording/clips.json    per-line recording state and takes               (teleprompter app)
  <subjects_dir>/NN-*.md          frontmatter status: final + label_rev            (AI writes final script)
  assets/edits/<VID>*.mp4|webm|mov                                         (AI edit via filmcraft)
  pipeline/events.jsonl   append-only log of human and AI events

Writes pipeline/pipeline.json (apps read it) and pipeline/skill-graph.json (node-link graph).

  python3 pipeline/status.py            refresh + one line per video
  python3 pipeline/status.py --ai       AI work queue (what the skill should do next)
  python3 pipeline/status.py --changes  human events since the AI last acknowledged
  python3 pipeline/status.py --ack      move the AI cursor to now
  python3 pipeline/status.py --event KIND VID [json]   log an AI event (e.g. ai.script.done V02)
"""
import os
import json, pathlib, re, sys, datetime

from ws import ROOT, WS, SUBJECTS, agent as _agent  # noqa: E402
P = ROOT / "pipeline"
LABELS = ROOT / "labelling" / "labels.json"
CLIPS = ROOT / "recording" / "clips.json"
EVENTS = P / "events.jsonl"
CURSOR = P / ".ai_cursor"
EDITS = ROOT / "assets" / "edits"
EDIT_EXT = tuple(WS["media"]["edit_ext"])

# step nodes come from the skill graph (single source): pipeline/skill-graph/steps.json
GRAPH = json.loads((P / "skill-graph" / "steps.json").read_text())
STAGES = [(x["id"], x["owner"], x["need"]) for x in GRAPH["steps"]]
ATTACHED_DIR = P / "attached"  # one file per agent: <agent>.json {step, vid, agent, at}
SCRIPTS = ROOT / "scripts"
OWNER = {s: o for s, o, _ in STAGES}
NEED = {s: n for s, _, n in STAGES}
OWNER.setdefault("done", "-"); NEED.setdefault("done", "")
DONE_LINE = {"recorded", "parked"}


def now():
    return datetime.datetime.now().isoformat(timespec="seconds")


def jload(p, default):
    try:
        return json.loads(p.read_text())
    except Exception:
        return default


def append_event(kind, vid="", actor="", data=None):
    P.mkdir(exist_ok=True)
    actor = actor or ("ai" if kind.startswith("ai.") else "human")
    row = {"at": now(), "kind": kind, "vid": vid, "actor": actor, "data": data or {}}
    who = os.environ.get("VENTOSA_USER", "")
    try:
        import auth
        who = getattr(auth.LOCAL, "user", None) or who
    except Exception:
        pass
    if who and actor == "human":
        row["user"] = who  # who made the entry (the logged-in account), for the live monitor
    with EVENTS.open("a") as f:
        f.write(json.dumps(row, ensure_ascii=False) + "\n")


def events():
    if not EVENTS.exists():
        return []
    out = []
    for l in EVENTS.read_text().splitlines():
        try:
            out.append(json.loads(l))
        except Exception:
            pass
    return out


def script_meta():
    out = {}
    for f in sorted(SUBJECTS.glob("*.md")):
        fm = re.match(r"^---\n(.*?)\n---\n", f.read_text(), re.S)
        if not fm:
            continue
        m = dict((k.strip(), v.strip().strip('"')) for k, v in (l.split(":", 1) for l in fm.group(1).splitlines() if ":" in l))
        if m.get("id"):
            out[m["id"]] = {"file": str(f.relative_to(ROOT)), "status": m.get("status", ""), "label_rev": int(m.get("label_rev", "0") or 0), "title": m.get("title", "")}
    return out


def edit_file(vid):
    if not EDITS.exists():
        return None
    c = sorted((p for p in EDITS.glob(f"{vid}*") if p.suffix.lower() in EDIT_EXT), key=lambda p: p.stat().st_mtime)
    return c[-1] if c else None


SUBS = ROOT / "assets" / "subtitles"


def subs_signed(vid, ed, evs):
    """Subtitles count once subtitles-done logged them for this very edit."""
    rel = str(ed.relative_to(ROOT))
    return any(e["kind"] == "ai.subtitles.done" and e["vid"] == vid and e["data"].get("edit") == rel for e in evs)


def edit_signed(vid, ed, evs):
    """The edit counts only after ai_write.py edit-done logged it, at or after the file's last write."""
    rel = str(ed.relative_to(ROOT))
    mt = datetime.datetime.fromtimestamp(int(ed.stat().st_mtime)).isoformat(timespec="seconds")
    return any(e["kind"] == "ai.edit.done" and e["vid"] == vid and e["data"].get("file") == rel and e["at"] >= mt for e in evs)


# A custom graph ("flow": "generic" in workspace.json): steps run in graph order
# and each one finishes with a step.done event (ventosa-graph write step-done /
# human-done). The media flow keeps its file-based rules below.
GENERIC = WS.get("flow") == "generic"
ORDER = [x["id"] for x in GRAPH["steps"] if x["id"] != "done"]


# the human's thesis step before any draft ("thesis"; older graphs call it "pick")
THESIS = "thesis" if "thesis" in OWNER else ("pick" if "pick" in OWNER else "")
# phases between recording and the cut, and between the cut and subtitles/publish, in graph order
_IDS = [x["id"] for x in GRAPH["steps"]]
PRE = _IDS[_IDS.index("record") + 1:_IDS.index("edit")] if "record" in _IDS and "edit" in _IDS else []
_AFTER = next((x for x in ("subtitles", "publish_meta") if x in _IDS), "")
POST = _IDS[_IDS.index("edit") + 1:_IDS.index(_AFTER)] if "edit" in _IDS and _AFTER else []


def step_done_since(vid, step, evs, since):
    """A step.done for this subject and step at or after `since` (an ISO time), not reopened since."""
    ok = False
    for e in evs:
        if e.get("vid") == vid and (e.get("data") or {}).get("step") == step:
            if e["kind"] == "step.done" and e["at"] >= since:
                ok = True
            elif e["kind"] == "step.reopen":
                ok = False
    return ok


def step_done(vid, step, evs):
    """A step.done event for this subject and step, not reopened since (media flow phases)."""
    ok = False
    for e in evs:
        if e.get("vid") == vid and (e.get("data") or {}).get("step") == step:
            if e["kind"] == "step.done":
                ok = True
            elif e["kind"] == "step.reopen":
                ok = False
    return ok


def generic_stage(vid, evs):
    done = set()
    for e in evs:
        if e.get("vid") != vid:
            continue
        step = (e.get("data") or {}).get("step", "")
        if e["kind"] == "step.done":
            done.add(step)
        elif e["kind"] == "step.reopen":
            done.discard(step)
    for step in ORDER:
        if step not in done:
            return step, ""
    return "done", ""


def compute():
    evs = events()
    labels = jload(LABELS, {"videos": {}})
    clips = jload(CLIPS, {"videos": {}})
    scripts = script_meta()
    vids = {}
    for vid in sorted(set(labels["videos"]) | set(scripts)):
        if GENERIC and vid not in labels["videos"]:
            continue
        lv = labels["videos"].get(vid, {})
        sc = scripts.get(vid, {})
        cl = clips["videos"].get(vid, {}).get("lines", [])
        pub = lv.get("publish", {})
        lab = lv.get("label", {})
        rev = lab.get("rev", 0)
        rec = sum(1 for l in cl if l["status"] in DONE_LINE)
        open_lines = [l["id"] for l in cl if l["status"] not in DONE_LINE]
        ed = edit_file(vid)
        newest_take = 0.0
        for l in cl:
            for t in l.get("takes", []):
                f = ROOT / t.get("file", "") if t.get("file") else None
                if t.get("status") == "kept" and f and f.exists():
                    newest_take = max(newest_take, f.stat().st_mtime)
        detail = ""
        if GENERIC:
            stage, detail = generic_stage(vid, evs)
        elif "pool" in OWNER and len(lv.get("options", [])) < 2 and not step_done(vid, "pool", evs):
            stage = "pool"
            detail = "no options pool yet"
        elif len(lv.get("options", [])) < 2 or lv.get("want_more"):
            stage = "options"
            detail = "human asked for one more option" if lv.get("want_more") else f"{len(lv.get('options', []))} option(s)"
        elif THESIS and lab.get("status") != "labelled" and not lv.get("thesis", "").strip() and not step_done(vid, THESIS, evs):
            stage = THESIS
            detail = "select the thesis in the Label app (Thesis tab)"
        elif lab.get("status") != "labelled" and missing_drafts(vid, lv.get("options", []), evs, lv.get("thesis", "")):
            stage = "drafts"
            detail = "no version yet for option " + ", ".join(missing_drafts(vid, lv.get("options", []), evs, lv.get("thesis", "")))
        elif lab.get("status") != "labelled":
            stage = "label"
        elif sc.get("status") != "final" or sc.get("label_rev", 0) < rev:
            stage = "script"
            detail = "no script file yet" if not sc else ("label changed after final script" if sc.get("status") == "final" else f"script status {sc.get('status') or '?'}")
        elif not cl or open_lines:
            stage = "record"
            detail = "clips not indexed: run recording/build_index.py" if not cl else f"{rec}/{len(cl)} lines done"
        elif not (ed and ed.stat().st_mtime >= newest_take and edit_signed(vid, ed, evs)):
            # phases before the cut, in graph order; each finishes with step-done (a note says "skipped" when not needed)
            stage = next((x for x in PRE if not step_done(vid, x, evs)), "edit")
            if stage != "edit":
                detail = NEED[stage]
            else:
                detail = ("a newer take was kept after the edit" if ed.stat().st_mtime < newest_take else "edit file not signed off (ai_write edit-done)") if ed else "no edit file"
        elif not pub.get("uploaded") and any(not step_done_since(vid, x, evs, datetime.datetime.fromtimestamp(int(ed.stat().st_mtime)).isoformat(timespec="seconds")) for x in POST):
            # phases after the cut (grade, review): each counts only for this very edit
            stage = next(x for x in POST if not step_done_since(vid, x, evs, datetime.datetime.fromtimestamp(int(ed.stat().st_mtime)).isoformat(timespec="seconds")))
            detail = NEED[stage]
        elif "subtitles" in OWNER and not pub.get("uploaded") and not subs_signed(vid, ed, evs):
            stage = "subtitles"
            detail = "subtitles not signed off for this edit (ventosa-graph subtitles, then write subtitles-done)" if (SUBS / vid / f"{vid}.srt").exists() else "no subtitles yet: ventosa-graph subtitles " + vid
        elif len(pub.get("titles", [])) < 3 or len(pub.get("descriptions", [])) < 2:
            stage = "publish_meta"
        elif not pub.get("approved"):
            stage = "publish"
        elif not pub.get("uploaded"):
            stage = "upload"
        else:
            stage = "done"
        vids[vid] = {
            "title": lv.get("title") or sc.get("title", ""), "stage": stage, "owner": OWNER[stage],
            "need": NEED[stage], "detail": detail, "script": sc.get("file", ""),
            "lines": {"done": rec, "total": len(cl), "open": open_lines[:12]},
            "edit": str(ed.relative_to(ROOT)) if ed else "", "label_rev": rev,
        }
    queues = {s: [v for v, x in vids.items() if x["stage"] == s] for s, _, _ in STAGES}
    uploads = []
    for vid, x in vids.items():
        pub = labels["videos"].get(vid, {}).get("publish", {})
        if pub.get("approved"):
            uploads.append({"vid": vid, "title": pub.get("title", ""), "description": pub.get("description", ""),
                            "tags": pub.get("tags", ""), "file": x["edit"], "abs": str(ROOT / x["edit"]) if x["edit"] else "", "uploaded": bool(pub.get("uploaded")), "url": pub.get("url", "")})
    ev = evs
    cur = CURSOR.read_text().strip() if CURSOR.exists() else ""
    changes = [e for e in ev if e.get("actor") == "human" and e["at"] > cur]
    return {"generated": now(), "stages": [{"id": s, "owner": o, "need": n} for s, o, n in STAGES],
            "videos": vids, "queues": queues, "uploads": uploads,
            "human_changes_since_ai": len(changes), "attached": attachments(), "recent_events": ev[-40:]}


def attachments():
    return [jload(f, {}) for f in sorted(ATTACHED_DIR.glob("*.json"))] if ATTACHED_DIR.exists() else []


def attached(agent=None):
    """The attachment held by an agent (default: $VENTOSA_AGENT or 'main')."""
    agent = agent or _agent()
    return jload(ATTACHED_DIR / f"{agent}.json", None)


def missing_drafts(vid, options, evs=None, thesis=""):
    """An option counts as drafted once ai_write.py draft registered a version for it (and the file still exists).
    When the human has chosen a thesis, only the option carrying it needs a draft."""
    chosen = [o for o in options if thesis.strip() and o.get("thesis", "").strip() == thesis.strip()]
    options = chosen or options
    have = set()
    for e in evs if evs is not None else events():
        if e["kind"] == "ai.draft.done" and e["vid"] == vid and (ROOT / e["data"].get("file", "-")).exists():
            have.add(e["data"].get("option", ""))
    return [o["key"] for o in options if o["key"] not in have]


def skill_graph(st):
    """Node-link graph: step nodes (each with its skill), knowledge, files, apps, videos at their step."""
    nodes, links = [], []
    for x in GRAPH["steps"]:
        nodes.append({"id": f"STEP-{x['id']}", "kind": "step", "owner": x["owner"], "need": x["need"],
                      "skill": f"pipeline/skill-graph/{x['skill']}" if x.get("skill") else "", "command": x.get("command", ""), "done_when": x.get("done_when", "")})
        for r in x.get("reads", []):
            links.append({"source": f"STEP-{x['id']}", "target": r, "relation": "reads"})
        for w in x.get("writes", []):
            links.append({"source": f"STEP-{x['id']}", "target": w, "relation": "writes"})
        if x.get("app"):
            links.append({"source": x["app"], "target": f"STEP-{x['id']}", "relation": "works_on"})
        elif x["owner"] == "ai":
            links.append({"source": "SKILL-ai", "target": f"STEP-{x['id']}", "relation": "attaches_to"})
    for a, b in zip(GRAPH["steps"], GRAPH["steps"][1:]):
        links.append({"source": f"STEP-{a['id']}", "target": f"STEP-{b['id']}", "relation": "next"})
    nodes.append({"id": "SKILL-ai", "kind": "actor", "rule": GRAPH["rule"], "attached": attachments()})
    for k in ("knowledge", "files", "apps"):
        for x in GRAPH[k]:
            nodes.append({**x, "kind": k.rstrip("s") if k != "knowledge" else "knowledge"})
    for vid, x in st["videos"].items():
        nodes.append({"id": vid, "kind": "video", "label": x["title"], "stage": x["stage"]})
        links.append({"source": vid, "target": f"STEP-{x['stage']}", "relation": "at_step"})
        for key, rel in (("script", "script"), ("edit", "edit")):
            if x[key]:
                nodes.append({"id": f"FILE-{x[key]}", "kind": "artifact", "path": x[key]})
                links.append({"source": vid, "target": f"FILE-{x[key]}", "relation": rel})
    return {"directed": True, "multigraph": False, "graph": {"name": GRAPH["name"], "generated": st["generated"], "rule": GRAPH["rule"]}, "nodes": nodes, "links": links}


def refresh():
    st = compute()
    P.mkdir(exist_ok=True)
    (P / "pipeline.json").write_text(json.dumps(st, indent=1, ensure_ascii=False))
    (P / "skill-graph.json").write_text(json.dumps(skill_graph(st), indent=1, ensure_ascii=False))
    return st


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["--event"]:
        append_event(a[1], a[2] if len(a) > 2 else "", "ai", json.loads(a[3]) if len(a) > 3 else {})
        a = []
    if a[:1] == ["--ack"]:
        CURSOR.write_text(now()); print("ai cursor →", CURSOR.read_text()); sys.exit()
    st = refresh()
    if a[:1] == ["--changes"]:
        cur = CURSOR.read_text().strip() if CURSOR.exists() else ""
        for e in events():
            if e.get("actor") == "human" and e["at"] > cur:
                print(e["at"], e["kind"], e["vid"], json.dumps(e["data"], ensure_ascii=False)[:160])
        sys.exit()
    for vid, x in st["videos"].items():
        if a[:1] == ["--ai"] and x["owner"] != "ai":
            continue
        print(f"{vid:4} {x['stage']:12} {x['owner']:5} {x['title'][:48]:48} {x['detail'] or x['need']}")
    if a[:1] != ["--ai"]:
        print({s: len(v) for s, v in st["queues"].items() if v}, "| human changes since AI:", st["human_changes_since_ai"])
