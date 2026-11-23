#!/usr/bin/env python3
"""Auto review: the AI checks a cut before the human sees it. It never edits assets/edits/; it
measures, samples frames, reports, and sends blockers back to the node that owns them.

  ventosa-graph review <ID>                     run the deterministic checks, sample frames, write the report
  ventosa-graph review <ID> --add '<json>'      add a finding from the visual pass (check, severity, timecode, message, evidence)
  ventosa-graph review <ID> --decide            route: clean / pass / reopen the owning node / escalate to the human
  ventosa-graph review <ID> --accept <key>      the human accepts a finding so it is not flagged again

Outputs: assets/review/<ID>/report.json, report.md, frames/ (samples), sheets/ (contact sheets).
Checks and routing live in review_checks.py (unit tested)."""
import json, pathlib, re, subprocess, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import status  # noqa: E402
import review_checks as rc  # noqa: E402
from ws import ROOT, WS  # noqa: E402


def die(m):
    sys.exit("review: " + m)


def sh(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.stdout, r.stderr, r.returncode


def out_dir(vid):
    d = ROOT / "assets" / "review" / vid
    d.mkdir(parents=True, exist_ok=True)
    return d


def cut_file(vid):
    """The cut under review: the graded copy when there is one, else the signed edit."""
    ed = status.edit_file(vid)
    if not ed:
        die(f"{vid}: no edit in assets/edits/")
    g = sorted(ed.parent.glob(f"{vid}-graded.*"), key=lambda p: p.stat().st_mtime)
    return g[-1] if g and g[-1].stat().st_mtime >= ed.stat().st_mtime else ed


def luma_series(path, fps=10):
    _, err, _ = sh(["ffmpeg", "-v", "info", "-i", str(path), "-vf", f"fps={fps},signalstats,metadata=print:key=lavfi.signalstats.YAVG", "-an", "-f", "null", "-"])
    return [float(x) for x in re.findall(r"YAVG=([\d.]+)", err)]


def run_checks(vid):
    cut = cut_file(vid)
    p = str(cut)
    o, _, rcode = sh(["ffprobe", "-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", p])
    if rcode:
        die("ffprobe failed on " + p)
    facts = rc.parse_ffprobe(o)
    media = WS.get("media", {})
    want = {"width": int(media.get("width", 1920)), "height": int(media.get("height", 1080)), "fps": float(media.get("fps", 30)), "audio_optional": bool(media.get("audio_only") is False and WS.get("rules", {}).get("no_audio"))}
    fs = rc.check_format(facts, want, WS.get("platform", {}).get("name", ""))
    cuts_json = ROOT / "assets" / "plans" / f"{vid}-cuts.json"
    segs, planned = 0, 0.0
    if cuts_json.exists():
        try:
            cj = json.loads(cuts_json.read_text())
            lst = cj if isinstance(cj, list) else [cj]
            for t in lst:
                for s in t.get("segments", []):
                    segs += 1
                    planned += float(s["endSeconds"]) - float(s["startSeconds"])
        except (ValueError, KeyError):
            fs.append(rc.finding("plan", "info", 0, "cuts.json could not be read"))
    fs += rc.check_plan_duration(facts["duration"], planned)
    if facts["acodec"]:
        _, e, _ = sh(["ffmpeg", "-hide_banner", "-i", p, "-af", f"loudnorm=I={rc.TARGET_LUFS}:TP=-1:print_format=json", "-vn", "-f", "null", "-"])
        fs += rc.check_loudness(rc.parse_loudnorm(e))
        _, e, _ = sh(["ffmpeg", "-hide_banner", "-i", p, "-af", "silencedetect=noise=-50dB:d=2", "-vn", "-f", "null", "-"])
        fs += rc.check_gaps("silence", rc.parse_silencedetect(e), facts["duration"])
    fs += rc.check_sync(facts["vdur"], facts["adur"])
    _, e, _ = sh(["ffmpeg", "-hide_banner", "-i", p, "-vf", "blackdetect=d=0.5:pix_th=0.1", "-an", "-f", "null", "-"])
    fs += rc.check_gaps("black", rc.parse_blackdetect(e), facts["duration"])
    _, e, _ = sh(["ffmpeg", "-hide_banner", "-i", p, "-vf", "freezedetect=n=-60dB:d=2", "-an", "-f", "null", "-"])
    fs += rc.check_gaps("freeze", rc.parse_freezedetect(e), facts["duration"])
    _, e, _ = sh(["ffmpeg", "-hide_banner", "-i", p, "-vf", "select='gt(scene,0.35)',showinfo", "-an", "-f", "null", "-"])
    cuts = rc.parse_scene_times(e)
    fs += rc.check_cuts(cuts, segs)
    fs += rc.check_flash(rc.flashes_per_second(luma_series(cut), 10))
    vf = ROOT / "videos"
    script = "\n".join(x.read_text() for x in sorted(vf.glob(f"{vid}*.md"))) if vf.exists() else ""
    srt = ROOT / "assets" / "subtitles" / vid / f"{vid}.srt"
    fs += rc.check_text(script, (WS.get("sponsor") or {}).get("line", ""), srt.read_text() if srt.exists() else "")
    return cut, facts, cuts, fs


def sample_frames(vid, cut, facts, cuts):
    d = out_dir(vid)
    fr, sh_dir = d / "frames", d / "sheets"
    for x in (fr, sh_dir):
        x.mkdir(exist_ok=True)
        for old in x.glob("*.png"):
            old.unlink()
    times = rc.sample_times(facts["duration"], cuts)
    for i, t in enumerate(times):
        sh(["ffmpeg", "-v", "error", "-y", "-ss", f"{t}", "-i", str(cut), "-frames:v", "1", "-vf", "scale=480:-2", str(fr / f"{i:03d}_{t:08.2f}.png")])
    frames = sorted(fr.glob("*.png"))
    for s in range(0, len(frames), 12):
        sh(["ffmpeg", "-v", "error", "-y", "-pattern_type", "glob", "-i", str(fr / "*.png"), "-vf", f"select='between(n\\,{s}\\,{s + 11})',tile=4x3", "-frames:v", "1", str(sh_dir / f"sheet{s // 12:02d}.png")])
    return len(frames)


def report_path(vid):
    return out_dir(vid) / "report.json"


def load_report(vid):
    p = report_path(vid)
    return json.loads(p.read_text()) if p.exists() else die(f"run ventosa-graph review {vid} first")


def accepted(vid):
    p = out_dir(vid) / "accepted.json"
    return json.loads(p.read_text()) if p.exists() else []


def write_md(vid, rep):
    L = [f"# Auto review {vid}", f"cut: `{rep['cut']}`  ·  {rep['facts']['width']}x{rep['facts']['height']} {rep['facts']['fps']:.1f} fps {rep['facts']['duration']:.1f}s", ""]
    for sev in ("blocker", "warn", "info"):
        rows = [f for f in rep["findings"] if f["severity"] == sev]
        if rows:
            L.append(f"## {sev} ({len(rows)})")
            for f in rows:
                acc = " (accepted)" if rc.key(f) in rep.get("accepted", []) else ""
                L.append(f"- `{rc.key(f)}` {f['timecode']}s [{f['check']}] {f['message']} -> {f['suggested_node']}{acc}" + (f" ({f['evidence']})" if f["evidence"] else ""))
            L.append("")
    if not rep["findings"]:
        L.append("auto-review: clean")
    (out_dir(vid) / "report.md").write_text("\n".join(L) + "\n")


def previous_runs(vid, cut):
    rel = str(cut.relative_to(ROOT))
    return sum(1 for e in status.events() if e["kind"] == "ai.review.decided" and e["vid"] == vid and e["data"].get("cut") == rel)


def main(a):
    if not a:
        die(__doc__)
    vid, flags = a[0], a[1:]
    if "--accept" in flags:
        k = flags[flags.index("--accept") + 1]
        acc = sorted(set(accepted(vid)) | {k})
        (out_dir(vid) / "accepted.json").write_text(json.dumps(acc))
        rep = load_report(vid); rep["accepted"] = acc; report_path(vid).write_text(json.dumps(rep, indent=1)); write_md(vid, rep)
        print("accepted", k); return
    if "--add" in flags:
        f = json.loads(flags[flags.index("--add") + 1])
        f = rc.finding(f["check"], f.get("severity", "warn"), f.get("timecode", 0), f["message"], f.get("evidence", ""), f.get("suggested_node"))
        rep = load_report(vid); rep["findings"].append(f); report_path(vid).write_text(json.dumps(rep, indent=1)); write_md(vid, rep)
        print("added", rc.key(f)); return
    if "--decide" in flags:
        rep = load_report(vid)
        cut = cut_file(vid)
        d = rc.decide(rep["findings"], accepted(vid), previous_runs(vid, cut))
        status.append_event("ai.review.decided", vid, "ai", {"cut": str(cut.relative_to(ROOT)), "action": d["action"], "node": d["node"], "blockers": sum(1 for f in d["findings"] if f["severity"] == "blocker")})
        rep["decision"] = {k: d[k] for k in ("action", "node", "note")}
        report_path(vid).write_text(json.dumps(rep, indent=1)); write_md(vid, rep)
        if d["action"] == "reopen":
            r = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("ai_write.py")), "reopen", vid, d["node"], d["note"]], capture_output=True, text=True)
            print(r.stdout.strip() or r.stderr.strip())
        print(json.dumps(rep["decision"], ensure_ascii=False))
        return
    cut, facts, cuts, fs = run_checks(vid)
    n = sample_frames(vid, cut, facts, cuts)
    rep = {"vid": vid, "cut": str(cut.relative_to(ROOT)), "facts": facts, "shot_changes": cuts, "frames": n, "findings": fs, "accepted": accepted(vid), "decision": None}
    report_path(vid).write_text(json.dumps(rep, indent=1)); write_md(vid, rep)
    c = {s: sum(1 for f in fs if f["severity"] == s) for s in ("blocker", "warn", "info")}
    print(f"{vid}: {c['blocker']} blocker, {c['warn']} warn, {c['info']} info; {n} frames in assets/review/{vid}/frames (look at sheets/), report.md written")


if __name__ == "__main__":
    main(sys.argv[1:])
