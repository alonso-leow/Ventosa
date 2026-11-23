#!/usr/bin/env python3
"""Import thesis options from meta/structures.md into labelling/labels.json.
Merges: human choices (stack, thesis, label, publish) and extra options (C, D…) are kept.
Options already in labels.json are only refreshed while the video is unlabelled.
"""
import json, re, pathlib, datetime
import sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT, SUBJECTS, ID_RE  # noqa: E402
OUT = ROOT / "labelling" / "labels.json"
HEAD = re.compile(r'^\*\*([A-Z]): "(.+?)"\*\* \((.+?)\)\. \*Thesis: (.+?)\*\s*$')


def parse():
    vids, cur, opt = {}, None, None
    src = ROOT / "meta" / "structures.md"
    for line in (src.read_text().splitlines() if src.exists() else []):
        m = re.match(r"^## (" + ID_RE + r") (.+)$", line)
        if m:
            cur = vids.setdefault(m.group(1), {"title": m.group(2).strip(), "options": []}); opt = None; continue
        m = HEAD.match(line)
        if m and cur is not None:
            opt = {"key": m.group(1), "name": m.group(2), "style": m.group(3), "thesis": m.group(4), "paras": [], "by": "ai"}
            cur["options"].append(opt); continue
        m = re.match(r"^\d+\. (.+)$", line)
        if m and opt is not None:
            opt["paras"].append(m.group(1))
    return vids


def empty(title):
    return {"title": title, "options": [], "stack": [], "thesis": "", "notes": "", "want_more": False,
            "label": {"status": "todo", "rev": 0, "at": ""},
            "publish": {"titles": [], "descriptions": [], "tags": "", "title": "", "description": "", "approved": False, "uploaded": False, "url": ""}}


def main():
    old = json.loads(OUT.read_text()) if OUT.exists() else {"videos": {}}
    titles = {}
    for f in SUBJECTS.glob("*.md"):
        m = re.search(r'^id: (' + ID_RE + r').*?^title: "?(.+?)"?$', f.read_text(), re.S | re.M)
        if m:
            titles[m.group(1)] = m.group(2)
    for vid, v in parse().items():
        e = old["videos"].setdefault(vid, empty(titles.get(vid, v["title"])))
        if e["label"]["status"] != "labelled":
            keep = [o for o in e["options"] if o["key"] not in {x["key"] for x in v["options"]}]
            e["options"] = v["options"] + keep
    old["updated"] = datetime.datetime.now().isoformat(timespec="seconds")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(old, indent=1, ensure_ascii=False))
    print(len(old["videos"]), "videos,", sum(len(v["options"]) for v in old["videos"].values()), "options")


if __name__ == "__main__":
    main()
