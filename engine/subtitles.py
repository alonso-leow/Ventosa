#!/usr/bin/env python3
"""Subtitles: a separate pass AFTER the edit is signed. Speech to text with word
timestamps, corrected against the active script, cut into cues by the subtitle
policy, snapped to the video's frame grid and its shot changes, then written as
SRT, VTT and ASS (main video + every reel). Burn-in is optional.

  ventosa-graph subtitles <ID> [--backend auto|whisperx|stable-ts|faster-whisper|whisper.cpp]
                               [--lang es] [--model large-v3] [--burn] [--force] [--no-script]
  ventosa-graph write subtitles-done <ID>     the AI signs the subtitles off (attached to `subtitles`)

Outputs (never in assets/edits/, so the edit stays the newest file there):
  assets/subtitles/<ID>/audio.wav            16 kHz mono, extracted from the signed edit
  assets/subtitles/<ID>/words.json           word timings (cached per edit; --force redoes them)
  assets/subtitles/<ID>/<ID>.srt|.vtt|.ass   the master track, frame-exact
  assets/subtitles/<ID>/reels/<reel>.srt|.ass  9:16 tracks for each reel, re-timed to the reel
  assets/subtitles/<ID>/report.json          QC: cps, durations, gaps, script match, warnings
  assets/subtitles/<ID>/<ID>-subs.mp4        burnt-in copy (only with --burn)

Backends (auto picks the first one installed):
  whisperx       faster-whisper + wav2vec2 forced alignment: best word timings
  stable-ts      aligns the known script text to the audio (model.align) or transcribes
  faster-whisper word_timestamps=True + VAD
  whisper.cpp    whisper-cli -ojf -ml 1 -sow; model from $VENTOSA_WHISPER_MODEL
The Python backends may live in ~/.local/share/ventosa/stt (or $VENTOSA_STT_PYTHON): the script re-runs there.
Workspace overrides: workspace.json "subtitles": {lang, max_chars, max_lines, max_cps, min_dur,
max_dur, gap_frames, snap_frames, pause_break, font, size, backend, model}.
"""
import difflib, hashlib, importlib.util, json, math, os, pathlib, re, shutil, subprocess, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT, WS  # noqa: E402
import status  # noqa: E402
import scripts_lib  # noqa: E402

# ---------------------------------------------------------------- policy
POLICY = {
    "lang": None,          # None = detect
    "max_chars": 42,       # per line
    "max_lines": 2,
    "max_cps": 17.0,       # reading speed, characters per second
    "min_dur": 5 / 6,      # seconds (20 frames at 24 fps)
    "max_dur": 7.0,
    "gap_frames": 2,       # minimum gap between two cues
    "close_frames": 12,    # gaps shorter than this are closed down to gap_frames
    "snap_frames": 7,      # a cue edge this close to a shot change snaps onto it
    "pause_break": 0.5,    # a silence this long always ends a cue
    "linger": 0.25,        # hold after the last word, if there is room
    "font": "DejaVu Sans",
    "size": 0.052,         # font size as a share of the frame height
    "backend": "auto",
    "model": "large-v3",
}
POLICY.update(WS.get("subtitles", {}) or {})
FPS = float(WS["media"]["fps"])
W, H = int(WS["media"]["width"]), int(WS["media"]["height"])
# words a line should not end on (en + es); the break moves before them
WEAK = set("a an the of to in on at for and or but with by from as de la el los las un una y o en con por para del al que se lo su".split())


def die(msg):
    sys.exit("subtitles: " + msg)


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode:
        die(f"{cmd[0]} failed:\n{r.stderr[-1500:]}")
    return r.stdout


def has(mod):
    return importlib.util.find_spec(mod) is not None


# The speech models live in their own venv (Ventosa ▸ settings ▸ tools installs WhisperX there):
# when this python has none of them, re-run under that venv's python.
STT_PY = pathlib.Path(os.environ.get("VENTOSA_STT_PYTHON") or pathlib.Path.home() / ".local/share/ventosa/stt/bin/python")


def reexec_in_stt():
    if any(has(m) for m in ("whisperx", "stable_whisper", "faster_whisper")) or not STT_PY.exists():
        return
    if pathlib.Path(sys.executable).resolve() != STT_PY.resolve():
        os.execv(str(STT_PY), [str(STT_PY), str(pathlib.Path(__file__).resolve()), *sys.argv[1:]])


# ---------------------------------------------------------------- inputs
def signed_edit(vid):
    ed = status.edit_file(vid)
    if not ed:
        die(f"{vid}: no edit in assets/edits/ yet (subtitles run after the edit)")
    if not status.edit_signed(vid, ed, status.events()):
        die(f"{vid}: {ed.name} is not signed off (ventosa-graph write edit-done) — subtitle the final cut only")
    return ed


def probe_fps(path):
    out = run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0", str(path)]).strip()
    if "/" in out:
        a, b = out.split("/")
        if float(b):
            return float(a) / float(b)
    return FPS


def edl(vid):
    f = ROOT / "assets" / "edits" / f"{vid}-edl.json"
    return json.loads(f.read_text()) if f.exists() else {}


def cuts_and_reels(e):
    """Shot changes = segment boundaries of the EDL; reels = (name, start, end) windows."""
    t, cuts, reels = 0.0, [], []
    for r in e.get("segments", []):
        d = float(r.get("dur") or 0)
        if t > 0:
            cuts.append(t)
        reel = ROOT / "assets" / "edits" / "reels" / f"{e.get('vid')}-{r.get('beat')}.mp4"
        if reel.exists():
            reels.append((reel.stem, t, t + d))
        t += d
    return cuts, reels


def script_words(e):
    """The narration of the script the edit was cut from, in order."""
    p = ROOT / e["script"] if e.get("script") else None
    if not p or not p.exists():
        return []
    text = " ".join(b.get("narration", "") for b in scripts_lib.beats(p.read_text()))
    text = re.sub(r"\[[^\]]*\]|\([^)]*\)|\*+", " ", text)   # stage directions, markdown
    return text.split()


# ---------------------------------------------------------------- speech to text
def backend_name(pref):
    order = [("whisperx", lambda: has("whisperx")), ("stable-ts", lambda: has("stable_whisper")),
             ("faster-whisper", lambda: has("faster_whisper")), ("whisper.cpp", lambda: bool(shutil.which("whisper-cli")))]
    if pref != "auto":
        ok = dict(order).get(pref)
        if not ok:
            die(f"unknown backend {pref}")
        if not ok():
            die(f"backend {pref} is not installed (see docs ▸ subtitles)")
        return pref
    for name, ok in order:
        if ok():
            return name
    die("no speech-to-text backend: pip install whisperx (best), stable-ts or faster-whisper, or build whisper.cpp (docs ▸ subtitles)")


def device():
    try:
        import torch  # noqa: F401
        return "cuda" if torch.cuda.is_available() else "cpu"
    except Exception:
        return "cpu"


def words_whisperx(wav, lang, model):
    import whisperx
    dev = device()
    m = whisperx.load_model(model, dev, compute_type="float16" if dev == "cuda" else "int8", language=lang)
    audio = whisperx.load_audio(str(wav))
    res = m.transcribe(audio, batch_size=8)
    am, meta = whisperx.load_align_model(language_code=res["language"], device=dev)
    al = whisperx.align(res["segments"], am, meta, audio, dev, return_char_alignments=False)
    out = [{"w": w["word"].strip(), "s": w.get("start"), "e": w.get("end")} for w in al.get("word_segments", [])]
    return fill_missing(out), res["language"]


def words_stable(wav, lang, model, known):
    import stable_whisper
    m = stable_whisper.load_model(model)
    res = m.align(str(wav), " ".join(known), language=lang or "en") if known else m.transcribe(str(wav), language=lang, regroup=False)
    out = [{"w": w.word.strip(), "s": w.start, "e": w.end} for w in res.all_words()]
    return out, getattr(res, "language", lang)


def words_faster(wav, lang, model):
    from faster_whisper import WhisperModel
    dev = device()
    m = WhisperModel(model, device=dev, compute_type="float16" if dev == "cuda" else "int8")
    segs, info = m.transcribe(str(wav), language=lang, word_timestamps=True, vad_filter=True)
    return [{"w": w.word.strip(), "s": w.start, "e": w.end} for s in segs for w in (s.words or [])], info.language


def words_cpp(wav, lang, out_dir):
    model = os.environ.get("VENTOSA_WHISPER_MODEL")
    if not model or not pathlib.Path(model).exists():
        die("whisper.cpp needs $VENTOSA_WHISPER_MODEL=/path/ggml-large-v3.bin")
    base = out_dir / "whispercpp"
    run(["whisper-cli", "-m", model, "-f", str(wav), "-l", lang or "auto", "-ml", "1", "-sow", "-ojf", "-of", str(base)])
    d = json.loads((base.with_suffix(".json")).read_text())
    out = [{"w": x["text"].strip(), "s": x["offsets"]["from"] / 1000, "e": x["offsets"]["to"] / 1000} for x in d.get("transcription", []) if x["text"].strip()]
    return out, d.get("result", {}).get("language", lang)


def fill_missing(ws):
    """Aligners leave numbers/symbols untimed: give them the gap between their neighbours."""
    for i, w in enumerate(ws):
        if w["s"] is None or w["e"] is None:
            prev = next((x["e"] for x in reversed(ws[:i]) if x["e"] is not None), 0.0)
            nxt = next((x["s"] for x in ws[i + 1:] if x["s"] is not None), prev + 0.3)
            w["s"], w["e"] = prev, max(prev + 0.05, nxt)
    return ws


def norm(w):
    return re.sub(r"[^\w]", "", w.lower())


def correct(words, known):
    """Keep the ASR timing, take the script's spelling where both agree word-for-word or 1:1."""
    if not known:
        return words, None
    a, b = [norm(w["w"]) for w in words], [norm(k) for k in known]
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    out = [dict(w) for w in words]
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == "equal" or (op == "replace" and i2 - i1 == j2 - j1):
            for k in range(i2 - i1):
                out[i1 + k]["w"] = known[j1 + k]
    return out, round(sm.ratio(), 3)


# ---------------------------------------------------------------- cues
def frame(t, fps):
    return int(round(t * fps))


def wrap(text, P):
    """One or two balanced lines; never end a line on a weak word; prefer punctuation."""
    if len(text) <= P["max_chars"]:
        return [text]
    words = text.split()
    best, score = None, 1e9
    for i in range(1, len(words)):
        l1, l2 = " ".join(words[:i]), " ".join(words[i:])
        if len(l1) > P["max_chars"] or len(l2) > P["max_chars"]:
            continue
        s = abs(len(l1) - len(l2)) - (12 if re.search(r"[,.;:!?]$", l1) else 0) + (15 if norm(words[i - 1]) in WEAK else 0)
        if s < score:
            best, score = [l1, l2], s
    return best or [text]


def fits(ws, P):
    text = " ".join(w["w"] for w in ws)
    return len(text) <= P["max_chars"] * P["max_lines"] and wrap(text, P) is not None and len(wrap(text, P)) <= P["max_lines"] and all(len(l) <= P["max_chars"] for l in wrap(text, P))


def group(words, cuts, P):
    cues, cur = [], []
    for i, w in enumerate(words):
        if cur:
            pause = w["s"] - cur[-1]["e"]
            span = w["e"] - cur[0]["s"]
            crosses = any(cur[-1]["e"] <= c <= w["s"] + 0.04 for c in cuts) and cur[-1]["e"] - cur[0]["s"] >= P["min_dur"]
            sentence = re.search(r"[.!?…]$", cur[-1]["w"]) and len(" ".join(x["w"] for x in cur)) >= 18
            if pause >= P["pause_break"] or span > P["max_dur"] or crosses or sentence or not fits(cur + [w], P):
                cues.append(cur)
                cur = []
        cur.append(w)
    if cur:
        cues.append(cur)
    return [{"text": " ".join(x["w"] for x in c), "s": c[0]["s"], "e": c[-1]["e"]} for c in cues]


def time_cues(cues, cuts, fps, end_of_video, P):
    """Seconds → frames: reading time, minimum duration, shot-change snapping, then gaps."""
    cut_f = [frame(c, fps) for c in cuts]
    last = frame(end_of_video, fps)
    cues = [c for c in cues if frame(c["s"], fps) < last]
    for c in cues:
        c["fs"], c["fe"] = frame(c["s"], fps), frame(c["e"] + P["linger"], fps)
        need = max(P["min_dur"], len(c["text"]) / P["max_cps"])
        c["fe"] = max(c["fe"], c["fs"] + math.ceil(need * fps))
        for cf in cut_f:   # snap edges onto a nearby shot change (never earlier than the speech by more than the window)
            if abs(c["fs"] - cf) <= P["snap_frames"]:
                c["fs"] = cf
            if abs(c["fe"] - cf) <= P["snap_frames"] and cf > c["fs"]:
                c["fe"] = cf
    for k, c in enumerate(cues):   # gaps after every edge has moved
        nxt = cues[k + 1]["fs"] if k + 1 < len(cues) else last + P["gap_frames"]
        c["fe"] = min(c["fe"], nxt - P["gap_frames"])
        if 0 < nxt - c["fe"] < P["close_frames"]:
            c["fe"] = nxt - P["gap_frames"]   # chain close cues: no flicker
        c["fe"] = max(min(c["fe"], last), c["fs"] + 1)
        c["lines"] = wrap(c["text"], P)
    return cues


def qc(cues, fps, P, ratio):
    warn = []
    for i, c in enumerate(cues, 1):
        d = (c["fe"] - c["fs"]) / fps
        cps = len(c["text"].replace(" ", "")) / d if d else 99
        if cps > P["max_cps"] + 3:
            warn.append(f"cue {i}: {cps:.1f} cps (reading speed above {P['max_cps']})")
        if d < P["min_dur"] - 1e-6:
            warn.append(f"cue {i}: {d:.2f}s, shorter than {P['min_dur']:.2f}s")
        if any(len(l) > P["max_chars"] for l in c["lines"]) or len(c["lines"]) > P["max_lines"]:
            warn.append(f"cue {i}: line too long")
        if i < len(cues) and cues[i]["fs"] - c["fe"] < P["gap_frames"]:
            warn.append(f"cue {i}: gap under {P['gap_frames']} frames")
    if ratio is not None and ratio < 0.6:
        warn.append(f"script match {ratio:.0%}: the cut diverges from the script (ad-libs?) — ASR text kept where they differ; review")
    return warn


# ---------------------------------------------------------------- writers
def ts(f, fps, sep=","):
    """Frame → timestamp, floored to the millisecond so the frame that starts at f shows it."""
    ms = int(math.floor(f / fps * 1000 + 1e-6))
    h, ms = divmod(ms, 3600000)
    m, ms = divmod(ms, 60000)
    s, ms = divmod(ms, 1000)
    return f"{h:02}:{m:02}:{s:02}{sep}{ms:03}"


def ass_ts(f, fps):
    cs = int(math.floor(f / fps * 100 + 1e-6))
    h, cs = divmod(cs, 360000)
    m, cs = divmod(cs, 6000)
    s, cs = divmod(cs, 100)
    return f"{h}:{m:02}:{s:02}.{cs:02}"


def write_srt(cues, fps, p):
    p.write_text("".join(f"{i}\n{ts(c['fs'], fps)} --> {ts(c['fe'], fps)}\n" + "\n".join(c["lines"]) + "\n\n" for i, c in enumerate(cues, 1)))


def write_vtt(cues, fps, p):
    p.write_text("WEBVTT\n\n" + "".join(f"{ts(c['fs'], fps, '.')} --> {ts(c['fe'], fps, '.')}\n" + "\n".join(c["lines"]) + "\n\n" for c in cues))


def write_ass(cues, fps, p, w, h, P, vertical=False):
    size = round(h * P["size"] * (0.62 if vertical else 1))
    margin_v = round(h * (0.22 if vertical else 0.07))
    head = f"""[Script Info]
ScriptType: v4.00+
PlayResX: {w}
PlayResY: {h}
WrapStyle: 2
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,{P['font']},{size},&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,{max(2, size // 14)},0,2,{round(w * 0.08)},{round(w * 0.08)},{margin_v},1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    lines = []
    for c in cues:
        text = "\\N".join(l.replace("{", "(").replace("}", ")") for l in c["lines"])
        lines.append(f"Dialogue: 0,{ass_ts(c['fs'], fps)},{ass_ts(c['fe'], fps)},Default,,0,0,0,,{text}\n")
    p.write_text(head + "".join(lines))


def reel_cues(words, cuts, start, end, fps, P):
    """A reel's own track: its words re-cut for 9:16 (shorter lines) on the reel's clock."""
    rp = dict(P, max_chars=min(P["max_chars"], 26))
    ws = [{**w, "s": w["s"] - start, "e": w["e"] - start} for w in words if w["s"] >= start - 0.05 and w["e"] <= end + 0.05]
    rc = [c - start for c in cuts if start < c < end]
    return time_cues(group(ws, rc, rp), rc, fps, end - start, rp) if ws else []


# ---------------------------------------------------------------- main
def main(a):
    if not a or a[0].startswith("-"):
        die(__doc__)
    vid, flags = a[0], a[1:]

    def opt(name, default=None):
        return flags[flags.index(name) + 1] if name in flags and flags.index(name) + 1 < len(flags) else default

    P = dict(POLICY)
    if WS["media"].get("audio_only"):
        die("audio-only workspace: no video to subtitle (write a transcript instead)")
    ed = signed_edit(vid)
    fps = probe_fps(ed)
    dur = float(run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(ed)]).strip() or 0)
    out = ROOT / "assets" / "subtitles" / vid
    (out / "reels").mkdir(parents=True, exist_ok=True)
    e = edl(vid)
    cuts, reels = cuts_and_reels(e)
    known = [] if "--no-script" in flags else script_words(e)
    lang = opt("--lang", P["lang"])
    model = opt("--model", P["model"])
    key = hashlib.sha1(f"{ed.name}:{ed.stat().st_size}:{int(ed.stat().st_mtime)}:{model}:{lang}:{len(known)}".encode()).hexdigest()[:16]
    wf = out / "words.json"
    cached = json.loads(wf.read_text()) if wf.exists() else {}
    if cached.get("key") == key and "--force" not in flags:
        words, detected, backend = cached["words"], cached.get("lang"), cached.get("backend")
        print(f"words: cached ({backend})")
    else:
        wav = out / "audio.wav"
        run(["ffmpeg", "-v", "error", "-y", "-i", str(ed), "-vn", "-ac", "1", "-ar", "16000", str(wav)])
        backend = backend_name(opt("--backend", P["backend"]))
        print(f"speech to text: {backend} · {model} · {lang or 'auto'} …", flush=True)
        if backend == "whisperx":
            words, detected = words_whisperx(wav, lang, model)
        elif backend == "stable-ts":
            words, detected = words_stable(wav, lang, model, known)
        elif backend == "faster-whisper":
            words, detected = words_faster(wav, lang, model)
        else:
            words, detected = words_cpp(wav, lang, out)
        words = [w for w in words if w["w"]]
        wf.write_text(json.dumps({"key": key, "backend": backend, "lang": detected, "edit": ed.name, "words": words}, ensure_ascii=False))
    if not words:
        die("no speech found")
    words, ratio = correct(words, known)
    cues = time_cues(group(words, cuts, P), cuts, fps, dur, P)
    write_srt(cues, fps, out / f"{vid}.srt")
    write_vtt(cues, fps, out / f"{vid}.vtt")
    write_ass(cues, fps, out / f"{vid}.ass", W, H, P)
    for name, s, en in reels:
        rc = reel_cues(words, cuts, s, en, fps, P)
        write_srt(rc, fps, out / "reels" / f"{name}.srt")
        write_ass(rc, fps, out / "reels" / f"{name}.ass", 1080, 1920, P, vertical=True)
    warn = qc(cues, fps, P, ratio)
    burnt = ""
    if "--burn" in flags:
        dst = out / f"{vid}-subs.mp4"
        assf = str(out / f"{vid}.ass").replace("\\", "/").replace(":", "\\:").replace("'", "\\'")
        run(["ffmpeg", "-v", "error", "-y", "-i", str(ed), "-vf", f"ass='{assf}'", "-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "copy", "-movflags", "+faststart", str(dst)])
        burnt = str(dst.relative_to(ROOT))
    report = {"vid": vid, "edit": str(ed.relative_to(ROOT)), "fps": fps, "backend": backend, "lang": detected, "cues": len(cues),
              "words": len(words), "script_match": ratio, "shot_changes": len(cuts), "reels": [r[0] for r in reels],
              "policy": P, "warnings": warn, "burnt": burnt}
    (out / "report.json").write_text(json.dumps(report, indent=1, ensure_ascii=False))
    print(f"→ assets/subtitles/{vid}/{vid}.srt|.vtt|.ass · {len(cues)} cues · {len(reels)} reels · script match {ratio if ratio is not None else '—'}")
    for w in warn[:20]:
        print("  warning:", w)
    if len(warn) > 20:
        print(f"  … {len(warn) - 20} more in report.json")


if __name__ == "__main__":
    reexec_in_stt()
    main(sys.argv[1:])
