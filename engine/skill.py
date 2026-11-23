#!/usr/bin/env python3
"""Attach an AI agent ($VENTOSA_AGENT, default 'main') to exactly one step node of the skill graph. Obligatory before any pipeline write.

  skill.py attach <step> <VID>   check the video is at that step, print the node's skill + inputs, lock it
  skill.py show                  what the AI is attached to
  skill.py detach                release (ai_write.py does this after a successful write)
  skill.py nodes                 list step nodes with owner, skill and done condition
"""
import json, os, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import status  # noqa: E402
from ws import ROOT, agent as _agent  # noqa: E402
P = ROOT / "pipeline"
G = status.GRAPH
STEPS = {x["id"]: x for x in G["steps"]}
KNOW = {x["id"]: x for x in G["knowledge"] + G["files"]}


AGENT = _agent()
MINE = status.ATTACHED_DIR / f"{AGENT}.json"


def main(a):
    cmd = a[0] if a else "show"
    if cmd == "nodes":
        for x in G["steps"]:
            print(f"{x['id']:13} {x['owner']:6} {x.get('skill',''):22} {x.get('done_when','')}")
        return
    if cmd == "show":
        print(json.dumps(status.attachments(), ensure_ascii=False) if status.attachments() else "no agent attached"); return
    if cmd == "detach":
        cur = status.attached()
        MINE.unlink(missing_ok=True)
        if cur:
            status.append_event("ai.detach", cur["vid"], "ai", {"step": cur["step"]})
        print("detached"); return
    if cmd != "attach" or len(a) < 3:
        sys.exit(__doc__)
    step, vid = a[1], a[2]
    node = STEPS.get(step) or sys.exit(f"unknown step {step}; nodes: {', '.join(STEPS)}")
    if node["owner"] != "ai":
        sys.exit(f"{step} is a human step: read pipeline/skill-graph/{node['skill']} and point the human to the app. The AI cannot attach to it.")
    cur = status.attached()
    if cur and (cur["step"], cur["vid"]) != (step, vid):
        sys.exit(f"agent {AGENT} is already attached to {cur['step']} {cur['vid']}: finish it or run skill.py detach first (one step at a time)")
    other = [x for x in status.attachments() if (x.get("step"), x.get("vid")) == (step, vid) and x.get("agent") != AGENT]
    if other:
        sys.exit(f"{step} {vid} is held by agent {other[0]['agent']}; pick another video")
    st = status.refresh()
    at = st["videos"].get(vid, {}).get("stage")
    if at != step:
        sys.exit(f"{vid} is at '{at}', not '{step}'. Attach to the step the video is at (ventosa-graph status --ai).")
    status.ATTACHED_DIR.mkdir(exist_ok=True)
    MINE.write_text(json.dumps({"step": step, "vid": vid, "agent": AGENT, "at": status.now()}))
    status.append_event("ai.attach", vid, "ai", {"step": step, "agent": AGENT})
    status.refresh()
    print(f"ATTACHED ({AGENT}): {step} {vid} — {st['videos'][vid]['title']}  ({st['videos'][vid]['detail'] or node['need']})\n")
    print((P / "skill-graph" / node["skill"]).read_text())
    kinds = {"accepted": "an accepted script: final text, use it as written", "topic": "a topic or theme: an idea to develop, not text to keep",
             "draft": "a draft: improve it, keep what works", "reference": "reference material: read it, quote only what it supports"}
    notes = (status.jload(status.LABELS, {"videos": {}})["videos"].get(vid, {}).get("notes") or "").strip()
    if notes:
        print(f"Comment from the start (it applies to every phase; follow it in this step):\n{notes}")
        import re
        open_vars = sorted(set(re.findall(r"<<\s*([A-Za-z0-9_-]{1,40})\s*>>", notes)))
        if open_vars:
            print("Open variables: " + ", ".join(f"<<{v}>>" for v in open_vars) + ". A <<name>> nobody filled in: resolve it from the title, notes and earlier outputs; when it cannot be known, ask with decision_ask instead of guessing.")
    if node.get("input") in kinds:
        print(f"Input: what was handed to this node counts as {kinds[node['input']]}.")
    if node.get("kind") == "research":
        print(f"Research: use the web_research tool (provider {node.get('search_provider', '?')}, query {node.get('search_query', '{title}')}).")
    print("Knowledge to read:", ", ".join(f"{KNOW[k]['path']}" for k in node.get("reads", []) if k in KNOW))
    print(f"Write only with: ventosa-graph write {node['command']} {vid} …")


if __name__ == "__main__":
    main(sys.argv[1:])
