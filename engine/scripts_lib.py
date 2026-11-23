"""Script versions: scripts/<VID>/<slug>.md, one alternative per file. videos/NN-*.md is the ACTIVE copy
(what Record, the graph and the edit read). Activation copies a version there with status: final + label_rev.

Version file = same shape as videos/*.md plus frontmatter keys: version, option, thesis.
"""
import json, re, pathlib, subprocess, sys, datetime

from ws import ROOT, WS, SUBJECTS, ENGINE  # noqa: E402
SCRIPTS = ROOT / "scripts"
VIDEOS = SUBJECTS
HEADER = "| id | t | act | source | on screen | narration (speakable draft) | reel |"
SPONSOR = WS["sponsor"].get("line", "")  # empty: no SP beat required
SLUG = re.compile(r"^[A-Za-z0-9][A-Za-z0-9+-]{0,60}$")


def now():
    return datetime.datetime.now().isoformat(timespec="seconds")


def split_fm(text):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None, text
    fm = {}
    for line in m.group(1).splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            fm[k.strip()] = v.strip()
    return fm, text[m.end():]


def set_fm(text, **kv):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    lines = m.group(1).splitlines() if m else []
    body = text[m.end():] if m else text
    for k, v in kv.items():
        lines = [l for l in lines if not re.match(rf"^{k}\s*:", l)]
        lines.append(f"{k}: {v}")
    return "---\n" + "\n".join(lines) + "\n---\n" + body


def beats(text):
    out, cols = [], None
    for row in text.splitlines():
        if not row.startswith("|"):
            continue
        cells = [c.strip() for c in row.strip().strip("|").split("|")]
        if cells and cells[0] == "id":
            cols = ["narration" if c.lower().startswith("narration") else c.lower() for c in cells]; continue
        if cols is None or set(row) <= set("|-: "):
            continue
        if len(cells) == len(cols):
            out.append(dict(zip(cols, cells)))
    return out


def validate(vid, text):
    errs = []
    fm, body = split_fm(text)
    if fm is None:
        return ["no frontmatter"]
    if fm.get("id") != vid:
        errs.append(f"frontmatter id must be {vid}")
    for k in ("title", "version", "option", "thesis"):
        if not fm.get(k):
            errs.append(f"frontmatter needs {k}")
    if HEADER not in text:
        errs.append("beat table header must be exactly: " + HEADER)
    bs = beats(text)
    if len(bs) < 6:
        errs.append(f"only {len(bs)} beats (need ≥6)")
    ids = [b["id"] for b in bs]
    if len(set(ids)) != len(ids):
        errs.append("duplicate beat ids")
    sp = [b for b in bs if b["id"] == "SP"]
    if not SPONSOR:
        pass
    elif not sp:
        errs.append("missing SP sponsor beat")
    elif sp[0].get("narration", "").rstrip(".") != SPONSOR.rstrip("."):
        errs.append("SP narration must be the fixed sponsor line")
    elif ids.index("SP") > 3:
        errs.append("SP must come right after the hook")
    hf_max = WS.get("rules", {}).get("max_animations", 7)
    hf = {h for b in bs for h in re.findall(r"HF-[a-z0-9-]+", b.get("source", ""))}
    if hf_max and len(hf) > hf_max:
        errs.append(f"{len(hf)} HF animations (max {hf_max})")
    acts = {b.get("act", "") for b in bs}
    if not {"I", "II", "III"} & acts:
        errs.append("act column must use I / II / III")
    for b in bs if WS.get("rules", {}).get("no_music") else []:
        if re.search(r"\b(music bed|soundtrack|synced to a track|licensed track)\b", (b.get("source", "") + b.get("on screen", "")).lower()):
            errs.append(f"{b['id']}: no music (workspace rule)")
    if not any(b.get("narration") for b in bs):
        errs.append("no narration")
    return errs


def vdir(vid):
    return SCRIPTS / vid


def active_info(vid):
    f = vdir(vid) / "_active.json"
    return json.loads(f.read_text()) if f.exists() else {}


def video_file(vid):
    for p in VIDEOS.glob("*.md"):
        if re.search(rf"^id: {vid}$", p.read_text(), re.M):
            return p
    return None


def list_versions(vid):
    act = active_info(vid).get("slug")
    out = []
    for p in sorted(vdir(vid).glob("*.md")) if vdir(vid).exists() else []:
        t = p.read_text(); fm, _ = split_fm(t); fm = fm or {}
        bs = beats(t)
        out.append({"slug": p.stem, "file": str(p.relative_to(ROOT)), "option": fm.get("option", ""), "thesis": fm.get("thesis", ""),
                    "title": fm.get("title", "").strip('"'), "status": fm.get("status", ""), "beats": len(bs),
                    "words": sum(len(b.get("narration", "").split()) for b in bs), "active": p.stem == act,
                    "errors": validate(vid, t), "mtime": datetime.datetime.fromtimestamp(int(p.stat().st_mtime)).isoformat()})
    return out


def check_slug(slug):
    if not slug or not SLUG.match(slug) or slug.startswith("_"):
        raise ValueError("slug: letters, digits and dashes, like B-two-machines")


def read(vid, slug):
    check_slug(slug)
    return (vdir(vid) / f"{slug}.md").read_text()


def write(vid, slug, text):
    check_slug(slug)
    vdir(vid).mkdir(parents=True, exist_ok=True)
    (vdir(vid) / f"{slug}.md").write_text(text)
    if active_info(vid).get("slug") == slug:  # keep the active copy in sync
        sync_active(vid)


def create(vid, slug, from_slug=None, title="", option="", thesis=""):
    check_slug(slug)
    if (vdir(vid) / f"{slug}.md").exists():
        raise ValueError(f"{slug} exists")
    if from_slug:
        t = set_fm(read(vid, from_slug), version=slug, status="draft")
    else:
        vf = video_file(vid)
        base = split_fm(vf.read_text())[0] if vf else {}
        t = (f"---\nid: {vid}\ntitle: \"{title or base.get('title', '').strip(chr(34))}\"\nversion: {slug}\noption: {option or '+'}\n"
             f"thesis: {thesis}\nstatus: draft\npillar: {base.get('pillar', '')}\nlength_min: {base.get('length_min', '8')}\n---\n"
             f"# {title or base.get('title', '').strip(chr(34))}\n\n## Beats\n\n{HEADER}\n|---|---|---|---|---|---|---|\n"
             f"| B1 | 0:00 | I | CAM | | (hook) | yes |\n" + (f"| SP | 0:30 | I | CAM + logo lower-third | {WS['brand']['name']} logo | {SPONSOR} | |\n" if SPONSOR else ""))
    write(vid, slug, t)


def delete(vid, slug):
    check_slug(slug)
    if active_info(vid).get("slug") == slug:
        raise ValueError("can't delete the active version; activate another first")
    (vdir(vid) / f"{slug}.md").unlink()


def sync_active(vid):
    a = active_info(vid)
    text = (vdir(vid) / f"{a['slug']}.md").read_text()
    fm, _ = split_fm(text)
    vf = video_file(vid)
    if not vf:
        n = int(vid[1:]); slug = re.sub(r"[^a-z0-9]+", "-", fm.get("title", vid).strip('"').lower()).strip("-")[:60]
        vf = VIDEOS / f"{n:02d}-{slug}.md"
    vf.write_text(set_fm(text, status="final", label_rev=a.get("label_rev", 0), version=a["slug"]))
    subprocess.run([sys.executable, str(ENGINE / "build_index.py")], check=False, capture_output=True)
    return vf


def activate(vid, slug, label_rev):
    t = read(vid, slug)
    errs = validate(vid, t)
    if errs:
        raise ValueError("; ".join(errs))
    (vdir(vid) / "_active.json").write_text(json.dumps({"slug": slug, "label_rev": label_rev, "at": now()}))
    return sync_active(vid)
