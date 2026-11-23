#!/usr/bin/env python3
"""The only way the AI writes pipeline state. Requires pipeline/skill.py attach <step> <VID> first. Each command validates, writes and logs an ai.* event.

  ai_write.py option VID '{"name","style","thesis","paras":[...]}'   add one thesis option (next letter)
  ai_write.py new VID "Title" ["notes"]                               register a video from a title
  ai_write.py draft VID scripts/VID/A-slug.md                          validate + register a script version (drafts node)
  ai_write.py script-done VID scripts/VID/L2-mix.md                   validate + ACTIVATE a version for the current label rev (script node)
  ai_write.py edit-done VID assets/edits/VID-final.mp4                record the edited file
  ai_write.py subtitles-done VID                                       sign off assets/subtitles/VID/ for the current edit
  ai_write.py publish VID '{"titles":[...],"descriptions":[...],"tags":"a, b"}'   add title/description candidates
  ai_write.py step-done VID STEP ["note"]       generic flow: finish an AI step (needs the attachment)
  ai_write.py human-done VID STEP ["note"]      generic flow: finish a human step
  ai_write.py reopen VID STEP ["note"]          generic flow: send a subject back to a step
"""
import json, os, pathlib, re, subprocess, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT, WS, agent as _agent  # noqa: E402
import status  # noqa: E402
LF = ROOT / "labelling" / "labels.json"


def load():
    return json.loads(LF.read_text()) if LF.exists() else {"videos": {}}


def save(d):
    LF.parent.mkdir(parents=True, exist_ok=True)
    LF.write_text(json.dumps(d, indent=1, ensure_ascii=False))


def die(msg):
    sys.exit("ai_write: " + msg)


STEP_OF = {"option": "options", "draft": "drafts", "script-done": "script", "edit-done": "edit", "subtitles-done": "subtitles", "publish": "publish_meta"}
import scripts_lib  # noqa: E402


def version_arg(vid, path):
    p = (ROOT / path).resolve()
    if p.parent != (scripts_lib.SCRIPTS / vid).resolve() or p.suffix != ".md" or not p.exists():
        die(f"version must be an existing file scripts/{vid}/<slug>.md")
    errs = scripts_lib.validate(vid, p.read_text())
    if errs:
        die("invalid version:\n  - " + "\n  - ".join(errs))
    return p


def artifacts_for(node, vid, since, note):
    """Policy: a phase that makes files finishes only with those files, written since the AI attached
    (so every file in History belongs to exactly one run of one phase). A phase that really has no
    output says why: a note starting `skipped: <reason>` (at least 8 characters of reason)."""
    import glob
    pats = [x.replace("<ID>", vid) for x in node.get("artifacts", [])]
    found = {}
    try:
        t0 = __import__("datetime").datetime.fromisoformat(since).timestamp() - 2 if since else 0
    except ValueError:
        t0 = 0
    for pat in pats:
        for f in glob.glob(str(ROOT / pat), recursive=True):
            fp = pathlib.Path(f)
            try:
                rel = fp.resolve().relative_to(ROOT.resolve())
            except ValueError:
                continue  # never outside the workspace
            if fp.is_file() and fp.stat().st_mtime >= t0:
                found[str(rel)] = fp.stat().st_size
    if pats and not found:
        reason = note.split(":", 1)[1].strip() if note.lower().startswith("skipped:") else ""
        if len(reason) < 8:
            die(f"{node['id']} makes files but none was written since you attached (looked in: {', '.join(pats)}). "
                f"Write them there, or finish with a note `skipped: <why, ≥8 characters>`.")
    return [{"path": k, "size": v} for k, v in sorted(found.items())]


def step_cmd(cmd, vid, rest):
    """Generic flow: finish (or reopen) one step. AI steps need the attachment; human steps don't."""
    if not rest:
        die(f"usage: ai_write.py {cmd} <ID> <step> [note]")
    step = rest[0]; note = rest[1] if len(rest) > 1 else ""
    node = next((x for x in status.GRAPH["steps"] if x["id"] == step), None) or die(f"no step {step}")
    st = status.refresh()
    at = st["videos"].get(vid, {}).get("stage")
    if cmd == "reopen":
        status.append_event("step.reopen", vid, "human", {"step": step, "note": note})
    else:
        if at != step:
            die(f"{vid} is at '{at}', not '{step}'")
        if cmd == "step-done":
            if node["owner"] != "ai":
                die(f"{step} is a human step: use human-done")
            cur = status.attached()
            if not cur or (cur["step"], cur["vid"]) != (step, vid):
                die(f"agent {_agent()} is not attached to {step} {vid}. Run: VENTOSA_AGENT=<you> ventosa-graph attach {step} {vid}")
            files = artifacts_for(node, vid, cur.get("at", ""), note)
            status.append_event("step.done", vid, "ai", {"step": step, "note": note, "files": files, "agent": _agent()})
            (status.ATTACHED_DIR / f"{_agent()}.json").unlink(missing_ok=True)
            status.append_event("ai.detach", vid, "ai", {"step": step})
        else:
            status.append_event("step.done", vid, "human", {"step": step, "note": note})
    st = status.refresh()
    print(vid, "→", st["videos"].get(vid, {}).get("stage"))


def main(a):
    if len(a) < 2:
        die(__doc__)
    cmd, vid = a[0], a[1]
    if cmd in ("step-done", "human-done", "reopen"):
        return step_cmd(cmd, vid, a[2:])
    if cmd in STEP_OF:  # obligatory: attached to exactly this step node for this video
        cur = status.attached()
        if not cur or (cur["step"], cur["vid"]) != (STEP_OF[cmd], vid):
            die(f"agent {_agent()} is not attached to {STEP_OF[cmd]} {vid}. Run: VENTOSA_AGENT=<you> ventosa-graph attach {STEP_OF[cmd]} {vid}")
    d = load()
    if cmd == "new":
        from build_options import empty
        if vid in d["videos"]:
            die(f"{vid} exists")
        d["videos"][vid] = empty(a[2]); d["videos"][vid]["notes"] = a[3] if len(a) > 3 else ""
        save(d); status.append_event("ai.video.new", vid, "ai", {"title": a[2]})
    elif cmd == "option":
        v = d["videos"].get(vid) or die(f"{vid} not in labels.json")
        o = json.loads(a[2])
        if not o.get("thesis") or len(o.get("paras", [])) < 4:
            die("option needs a thesis and ≥4 paras")
        used = {x["key"] for x in v["options"]}
        key = next(chr(c) for c in range(65, 91) if chr(c) not in used)
        v["options"].append({"key": key, "name": o.get("name", key), "style": o.get("style", ""), "thesis": o["thesis"], "paras": o["paras"], "by": "ai"})
        if len(v["options"]) >= 2:
            v["want_more"] = False
        save(d); status.append_event("ai.options.done", vid, "ai", {"key": key})
        print("added option", key)
    elif cmd == "draft":
        p = version_arg(vid, a[2] if len(a) > 2 else "")
        fm, _ = scripts_lib.split_fm(p.read_text())
        status.append_event("ai.draft.done", vid, "ai", {"file": str(p.relative_to(ROOT)), "option": fm.get("option", "")})
        print("registered", p.relative_to(ROOT))
    elif cmd == "script-done":
        v = d["videos"].get(vid) or die(f"{vid} not in labels.json")
        if v["label"].get("status") != "labelled":
            die("label not approved by the human yet")
        p = version_arg(vid, a[2] if len(a) > 2 else "")
        vf = scripts_lib.activate(vid, p.stem, v["label"]["rev"])
        status.append_event("ai.script.done", vid, "ai", {"version": p.stem, "file": str(vf.relative_to(ROOT)), "label_rev": v["label"]["rev"]})
    elif cmd == "edit-done":
        p = (ROOT / a[2]).resolve()
        if not p.is_file() or not p.is_relative_to((ROOT / "assets" / "edits").resolve()) or not p.name.startswith(vid):
            die("file must exist in assets/edits/ and start with the video id")
        status.append_event("ai.edit.done", vid, "ai", {"file": str(p.relative_to(ROOT))})
    elif cmd == "subtitles-done":
        ed = status.edit_file(vid) or die("no edit file")
        sd = ROOT / "assets" / "subtitles" / vid
        srt, rep = sd / f"{vid}.srt", sd / "report.json"
        if not srt.exists() or not rep.exists():
            die(f"run ventosa-graph subtitles {vid} first")
        r = json.loads(rep.read_text())
        if r.get("edit") != str(ed.relative_to(ROOT)) or srt.stat().st_mtime < ed.stat().st_mtime:
            die("the subtitles are older than the edit: run ventosa-graph subtitles again")
        long = [w for w in r.get("warnings", []) if "line too long" in w]
        if long:
            die("fix before signing:\n  - " + "\n  - ".join(long[:10]))
        status.append_event("ai.subtitles.done", vid, "ai", {"edit": r["edit"], "srt": str(srt.relative_to(ROOT)), "cues": r.get("cues"), "warnings": len(r.get("warnings", []))})
    elif cmd == "publish":
        v = d["videos"].get(vid) or die(f"{vid} not in labels.json")
        o, pub = json.loads(a[2]), v.setdefault("publish", {})
        for k in ("titles", "descriptions"):
            pub[k] = pub.get(k, []) + [x for x in o.get(k, []) if x not in pub.get(k, [])]
        if o.get("tags") and not pub.get("tags"):
            pub["tags"] = o["tags"]
        tmax = WS["platform"].get("title_max") or 0
        if tmax and any(len(t) > tmax for t in o.get("titles", [])):
            die(f"{WS['platform']['name']} titles must be ≤{tmax} characters")
        save(d); status.append_event("ai.publish_meta.done", vid, "ai", {"titles": len(pub["titles"]), "descriptions": len(pub["descriptions"])})
    else:
        die(__doc__)
    st = status.refresh()
    stage = st["videos"].get(vid, {}).get("stage")
    if cmd in STEP_OF and stage != STEP_OF[cmd]:  # the node is done: release it
        (status.ATTACHED_DIR / f"{_agent()}.json").unlink(missing_ok=True)
        status.append_event("ai.detach", vid, "ai", {"step": STEP_OF[cmd], "result": stage})
        status.refresh()
    print(vid, "→", stage, "(still attached)" if cmd in STEP_OF and stage == STEP_OF[cmd] else "")


if __name__ == "__main__":
    main(sys.argv[1:])
