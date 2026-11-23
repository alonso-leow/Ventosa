"""Pure parts of the auto review: parsing ffprobe/ffmpeg output, turning facts into findings,
severity routing and the loop guard. No I/O, so it can be unit tested (see test_review.py)."""
import json
import re

TARGET_LUFS = -14.0
MAX_AUTO_REVIEWS = 3
# the node that owns a kind of problem; the fix goes back there
ROUTE = {
    "audio": "audio_fx", "loudness": "audio_fx", "sync": "edit", "silence": "edit", "black": "edit",
    "freeze": "edit", "cut": "edit", "duration": "edit", "format": "edit", "track": "edit",
    "color": "color_grade", "grade": "color_grade", "overlay": "motion_source", "graphics": "motion_source",
    "flash": "motion_source", "text": "motion_source", "broll": "edit", "plan": "shot_plan",
    "placeholder": "edit", "sponsor": "edit", "safe_zone": "motion_source",
}
ORDER = ["shot_plan", "motion_source", "audio_fx", "edit", "color_grade"]  # graph order, earliest first
PLATFORM_MAX_SECONDS = {"youtube": 43200, "youtube short": 60, "shorts": 60, "tiktok": 600, "instagram reel": 90,
                        "reels": 90, "x": 140, "twitter": 140, "linkedin": 600}


def finding(check, severity, timecode, msg, evidence="", node=None):
    return {"check": check, "severity": severity, "timecode": round(float(timecode or 0), 2), "message": msg,
            "evidence": evidence, "suggested_node": node or ROUTE.get(check, "edit")}


def key(f):
    """Stable id for a finding, so a human can mark it accepted and it is not flagged again."""
    return f"{f['check']}@{int(f['timecode'] // 2) * 2}"


def parse_ffprobe(text):
    d = json.loads(text)
    v = next((s for s in d.get("streams", []) if s.get("codec_type") == "video"), None)
    a = next((s for s in d.get("streams", []) if s.get("codec_type") == "audio"), None)

    def num(x):
        try:
            return float(x)
        except (TypeError, ValueError):
            return 0.0

    fps = 0.0
    if v and "/" in str(v.get("r_frame_rate", "")):
        n, m = v["r_frame_rate"].split("/")
        fps = num(n) / num(m) if num(m) else 0.0
    return {"duration": num(d.get("format", {}).get("duration")),
            "width": int(v["width"]) if v else 0, "height": int(v["height"]) if v else 0, "fps": fps,
            "vcodec": v.get("codec_name") if v else None, "pix_fmt": v.get("pix_fmt") if v else None,
            "vdur": num(v.get("duration")) if v else 0.0, "acodec": a.get("codec_name") if a else None,
            "adur": num(a.get("duration")) if a else 0.0}


def parse_loudnorm(stderr):
    """The JSON block ffmpeg's loudnorm (print_format=json) writes at the end of stderr."""
    m = re.findall(r"\{[^{}]*\"input_i\"[^{}]*\}", stderr, re.S)
    if not m:
        return None
    d = json.loads(m[-1])
    return {"lufs": float(d["input_i"]), "true_peak": float(d["input_tp"]), "lra": float(d.get("input_lra", 0))}


def _pairs(text, a, b):
    out, start = [], None
    for line in text.splitlines():
        m = re.search(a + r":\s*(-?[\d.]+)", line)
        if m:
            start = float(m.group(1))
        m = re.search(b + r":\s*(-?[\d.]+)", line)
        if m and start is not None:
            out.append((start, float(m.group(1))))
            start = None
    return out


def parse_silencedetect(stderr):
    return _pairs(stderr, "silence_start", "silence_end")


def parse_blackdetect(stderr):
    return [(float(a), float(b)) for a, b in re.findall(r"black_start:(-?[\d.]+)\s+black_end:(-?[\d.]+)", stderr)]


def parse_freezedetect(stderr):
    return _pairs(stderr, "lavfi.freezedetect.freeze_start", "lavfi.freezedetect.freeze_end")


def parse_scene_times(stderr):
    return [float(x) for x in re.findall(r"pts_time:(-?[\d.]+)", stderr)]


def flashes_per_second(luma, fps, jump=25.0):
    """Count big brightness swings (opposing jumps within a second) per one-second window; returns the worst."""
    if not luma or fps <= 0:
        return 0
    n = max(1, int(round(fps)))
    steps = [luma[i + 1] - luma[i] for i in range(len(luma) - 1)]
    worst = 0
    for s in range(0, max(1, len(steps) - n + 1)):
        w = [x for x in steps[s:s + n] if abs(x) >= jump]
        flips = sum(1 for i in range(1, len(w)) if (w[i] > 0) != (w[i - 1] > 0))
        worst = max(worst, (flips + 1) // 2 if flips else 0)
    return worst


def check_format(facts, want, platform=""):
    out = []
    if facts["width"] != want["width"] or facts["height"] != want["height"]:
        out.append(finding("format", "blocker", 0, f"size {facts['width']}x{facts['height']}, workspace wants {want['width']}x{want['height']}"))
    if want.get("fps") and abs(facts["fps"] - want["fps"]) > 0.5:
        out.append(finding("format", "warn", 0, f"{facts['fps']:.2f} fps, workspace wants {want['fps']}"))
    if facts["vcodec"] not in ("h264", "hevc", "vp9", "prores", "av1"):
        out.append(finding("format", "warn", 0, f"video codec {facts['vcodec']}"))
    if facts["vcodec"] == "h264" and facts["pix_fmt"] not in ("yuv420p", None):
        out.append(finding("format", "warn", 0, f"pixel format {facts['pix_fmt']} may not play everywhere (use yuv420p)"))
    if facts["width"] % 2 or facts["height"] % 2:
        out.append(finding("format", "blocker", 0, "odd dimensions"))
    if not facts["acodec"] and not want.get("audio_optional"):
        out.append(finding("track", "blocker", 0, "no audio track"))
    cap = PLATFORM_MAX_SECONDS.get(platform.lower())
    if cap and facts["duration"] > cap:
        out.append(finding("duration", "blocker", cap, f"{facts['duration']:.0f}s exceeds the {platform} limit of {cap}s"))
    return out


def check_plan_duration(duration, planned):
    if not planned:
        return []
    d = abs(duration - planned) / planned
    if d > 0.25:
        return [finding("duration", "warn" if d < 0.5 else "blocker", 0, f"length {duration:.0f}s vs plan {planned:.0f}s ({d:.0%} off)", node="edit")]
    return []


def check_loudness(l, target=TARGET_LUFS):
    if not l:
        return [finding("loudness", "info", 0, "loudness could not be measured")]
    out, off = [], l["lufs"] - target
    if abs(off) > 3:
        out.append(finding("loudness", "blocker", 0, f"{l['lufs']:.1f} LUFS, target {target:.0f} (off by {off:+.1f})"))
    elif abs(off) > 1:
        out.append(finding("loudness", "warn", 0, f"{l['lufs']:.1f} LUFS, target {target:.0f}"))
    if l["true_peak"] > 0:
        out.append(finding("audio", "blocker", 0, f"true peak {l['true_peak']:.1f} dBTP: clipping"))
    elif l["true_peak"] > -1:
        out.append(finding("audio", "warn", 0, f"true peak {l['true_peak']:.1f} dBTP, keep under -1"))
    return out


def check_gaps(kind, spans, duration, warn=2.0, block=5.0, edge=1.0):
    out = []
    for a, b in spans:
        length = b - a
        mid = a > edge and b < duration - edge
        if kind in ("black", "freeze"):
            sev = "blocker" if (length >= block or (kind == "black" and mid and length >= 1.0)) else ("warn" if length >= warn / 2 or mid else "info")
        else:
            sev = "warn" if length >= block else ("info" if length >= warn else None)
        if sev:
            out.append(finding(kind, sev, a, f"{kind} for {length:.1f}s at {a:.1f}s"))
    return out


def check_sync(vdur, adur):
    if not vdur or not adur:
        return []
    d = abs(vdur - adur)
    if d > 0.5:
        return [finding("sync", "blocker", 0, f"audio and video lengths differ by {d:.2f}s")]
    if d > 0.1:
        return [finding("sync", "warn", 0, f"audio and video lengths differ by {d:.2f}s")]
    return []


def check_cuts(cut_times, planned_segments):
    if not planned_segments:
        return []
    exp, got = max(0, planned_segments - 1), len(cut_times)
    if exp and abs(got - exp) > max(3, exp * 0.5):
        return [finding("cut", "warn", 0, f"{got} shot changes detected, the cut list has {planned_segments} segments", node="edit")]
    return []


def check_flash(worst):
    if worst > 3:
        return [finding("flash", "blocker", 0, f"{worst} flashes in one second (limit 3)")]
    return []


def check_text(script_text, sponsor_line="", srt_text=""):
    out = []
    if re.search(r"\b(TODO|TBD|XXX|lorem ipsum)\b|\[\[|\{\{", script_text, re.I):
        out.append(finding("placeholder", "blocker", 0, "placeholder text left in the script"))
    if sponsor_line and sponsor_line.strip():
        hay = (script_text + " " + srt_text).lower()
        if sponsor_line.strip().lower()[:40] not in hay:
            out.append(finding("sponsor", "blocker", 0, "the sponsor line is missing from the script and captions"))
    return out


def visible(findings, accepted):
    """Findings the human has not accepted."""
    acc = set(accepted or [])
    return [f for f in findings if key(f) not in acc]


def decide(findings, accepted, previous_runs):
    """Routing rule. previous_runs: auto reviews already done on this very cut.
    Returns {action: clean|pass|reopen|escalate, node, note, findings}."""
    fs = visible(findings, accepted)
    blockers = [f for f in fs if f["severity"] == "blocker"]
    if not fs:
        return {"action": "clean", "node": None, "note": "auto-review: clean", "findings": []}
    if not blockers:
        return {"action": "pass", "node": None, "note": f"auto-review: {len(fs)} non-blocking finding(s)", "findings": fs}
    if previous_runs >= MAX_AUTO_REVIEWS:
        return {"action": "escalate", "node": None, "note": f"auto-review: {len(blockers)} blocker(s) still open after {MAX_AUTO_REVIEWS} reviews, human decides", "findings": fs}
    nodes = {f["suggested_node"] for f in blockers}
    node = sorted(nodes, key=lambda n: ORDER.index(n) if n in ORDER else 99)[0]
    note = "; ".join(f"{f['check']} at {f['timecode']}s: {f['message']}" for f in blockers if f["suggested_node"] == node)
    return {"action": "reopen", "node": node, "note": note[:900], "findings": fs}


def sample_times(duration, cuts, every=2.0, edge=1.0, cap=120):
    ts = {0.0, round(max(0, duration - edge), 2), round(max(0, duration - 0.05), 2), edge if duration > 2 * edge else 0.0}
    t = 0.0
    while t < duration:
        ts.add(round(t, 2))
        t += every
    for c in cuts:
        ts.add(round(c + 0.05, 2)); ts.add(round(max(0, c - 0.05), 2))
    out = sorted(x for x in ts if 0 <= x < duration)
    if len(out) > cap:
        step = len(out) / cap
        out = [out[int(i * step)] for i in range(cap)]
    return out
