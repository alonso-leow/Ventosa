#!/usr/bin/env python3
"""Edit node: cut <VID> from the kept camera takes + inserts, following the ACTIVE script's beat table.

  python3 pipeline/edit.py V02            → assets/edits/V02-final.mp4 (+ -edl.json, -chapters.txt, reels/)
  python3 pipeline/edit.py V02 --dry-run  → print the edit decision list and what's missing, render nothing
  python3 pipeline/edit.py V02 --rough    → preview: text cards where takes are missing, written to
                                            assets/edits/rough/V02-rough.mp4 (never counts as the edit, no reels)

Rules (tone.md): the camera is the frame; no music (inserts are muted, audio = the take's voice);
act backgrounds --act1/2/3; 1920×1080, 30 fps, H.264 + AAC, −14 LUFS.
Insert lookup per beat, first match wins:
  assets/inserts/<VID>-<beat>.*            explicit insert for that beat (drop a file here to override)
  assets/renders|screen, hyperframes/renders  file whose name starts with a source token (HF-red-taste, R-wgsl, REN-aurora…)
Sources: "CAM" camera only · "CAM + X" camera, then X full-frame for the last 60 % (voice continues)
         · "X" (no CAM) X full-frame over the voice · no narration → silent title card (3 s).
Missing insert → placeholder card (no-CAM beats) or camera only (CAM beats); always listed.
"""
import json, pathlib, re, subprocess, sys, tempfile, shutil

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT, WS  # noqa: E402
import scripts_lib as SL  # noqa: E402

W, H, FPS = WS["media"]["width"], WS["media"]["height"], WS["media"]["fps"]
ACT_BG = {"I": "0x07070b", "II": "0x0b1024", "III": "0x1a0a12"}
FONT = subprocess.run(["fc-match", "-f", "%{file}", "sans-serif:bold"], capture_output=True, text=True).stdout or ""
SEARCH = [ROOT / "assets" / "renders", ROOT / "assets" / "screen", ROOT / "hyperframes" / "renders"]
MEDIA = {".mp4", ".mov", ".webm", ".mkv", ".m4v", ".gif", ".png", ".jpg"}
SPONSOR_LT = WS["sponsor"].get("lower_third", "")


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError("ffmpeg failed:\n" + " ".join(map(str, cmd))[:600] + "\n" + r.stderr[-1500:])
    return r


def duration(f):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(f)], capture_output=True, text=True)
    try:
        return float(r.stdout.strip())
    except ValueError:
        return 0.0


def has_video(f):
    r = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "stream=codec_type", "-of", "csv=p=0", str(f)], capture_output=True, text=True)
    return "video" in r.stdout


def act_of(b):
    a = b.get("act", "I").split("→")[-1].strip()
    return a if a in ACT_BG else "I"


def tokens(source):
    return [t for t in re.findall(r"\b(?:HF|REN|SCR|R)-[A-Za-z0-9-]+", source)]


def find_insert(vid, beat, source):
    d = ROOT / "assets" / "inserts"
    for p in sorted(d.glob(f"{vid}-{beat}.*")) if d.exists() else []:
        if p.suffix.lower() in MEDIA:
            return p
    for tok in tokens(source):
        for base in SEARCH:
            if base.exists():
                for p in sorted(base.rglob(f"{tok}*")):
                    if p.suffix.lower() in MEDIA:
                        return p
    return None


def needs_insert(source):
    s = source.upper()
    return bool(re.search(r"\b(HF|REN|SCR)\b|HF-|REN-|SCR ", s))


def kept_take(line):
    for t in reversed(line.get("takes", [])):
        if t.get("status") == "kept" and t.get("file") and (ROOT / t["file"]).exists():
            return ROOT / t["file"]
    return None


def textfile(tmp, name, text):
    p = tmp / f"{name}.txt"
    p.write_text(text)
    return str(p).replace(":", "\\:")


def draw(tmp, name, text, size, y, color="white", box=False):
    f = f":fontfile='{FONT}'" if FONT else ""
    b = ":box=1:boxcolor=black@0.55:boxborderw=18" if box else ""
    return f"drawtext=textfile='{textfile(tmp, name, text)}'{f}:fontsize={size}:fontcolor={color}:x=(w-text_w)/2:y={y}{b}"


def wrap(text, n=46):
    for a, b in (("→", " > "), ("←", " < "), ("↔", " <> "), ("×", "x"), ("…", "..."), ("`", "")):
        text = text.replace(a, b)
    words, lines, cur = text.split(), [], ""
    for w in words:
        if len(cur) + len(w) + 1 > n:
            lines.append(cur); cur = w
        else:
            cur = (cur + " " + w).strip()
    lines.append(cur)
    return "\n".join(lines[:6])


NORM_V = f"scale={W}:{H}:force_original_aspect_ratio=decrease,pad={W}:{H}:(ow-iw)/2:(oh-ih)/2:color=black,fps={FPS},format=yuv420p,setsar=1"
ENC = ["-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", "-movflags", "+faststart"]


def card(tmp, out, dur, act, title, sub="", silent_audio=None):
    """Act-background card with text. Audio: the take's voice (silent_audio=file) or silence."""
    vf = draw(tmp, out.stem + "-t", wrap(title, 34), 72, "(h-text_h)/2-60") + ("," + draw(tmp, out.stem + "-s", wrap(sub, 60), 34, "h*0.66", "0xbbbbbb") if sub else "")
    cmd = ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", f"color=c={ACT_BG[act]}:s={W}x{H}:r={FPS}:d={dur:.3f}"]
    if silent_audio:
        cmd += ["-i", str(silent_audio), "-map", "0:v", "-map", "1:a"]
    else:
        cmd += ["-f", "lavfi", "-i", f"anullsrc=r=48000:cl=stereo:d={dur:.3f}", "-map", "0:v", "-map", "1:a"]
    run(cmd + ["-vf", vf + ",format=yuv420p", "-t", f"{dur:.3f}"] + ENC + [str(out)])


def segment(tmp, i, b, take, ins, plan, fade_from=None):
    out = tmp / f"seg{i:03d}.mp4"
    act = act_of(b)
    dur = duration(take)
    src = b.get("source", "")
    cam_frame = src.strip().upper().startswith("CAM") and has_video(take)
    lower = draw(tmp, f"lt{i}", SPONSOR_LT, 40, "h-150", box=True) if b["id"] == "SP" else ""
    fade = f",fade=t=in:st=0:d=0.35:color={ACT_BG[act]}" if fade_from else ""
    if ins is None and not cam_frame:  # no picture: placeholder card over the voice
        card(tmp, out, dur, act, f"[{', '.join(tokens(src)) or src or 'insert'}]", b.get("on screen", ""), silent_audio=take)
        plan["kind"] = "card+voice"
        return out
    if ins is None:  # camera only
        vf = NORM_V + ("," + lower if lower else "") + fade
        run(["ffmpeg", "-v", "error", "-y", "-i", str(take), "-map", "0:v:0", "-map", "0:a:0?", "-vf", vf, "-t", f"{dur:.3f}"] + ENC + [str(out)])
        plan["kind"] = "camera"
        return out
    # camera (or card) + insert. Inserts are muted; voice from the take.
    still = ins.suffix.lower() in (".png", ".jpg")
    cut = round(dur * 0.4, 3) if cam_frame else 0.0
    base = ["-i", str(take)] if cam_frame else ["-f", "lavfi", "-i", f"color=c={ACT_BG[act]}:s={W}x{H}:r={FPS}:d={dur:.3f}"]
    ins_in = (["-loop", "1", "-t", f"{dur:.3f}", "-i", str(ins)] if still else ["-stream_loop", "-1", "-t", f"{dur:.3f}", "-i", str(ins)])
    fc = (f"[0:v]{NORM_V}[b];[1:v]{NORM_V},setpts=PTS-STARTPTS+{cut}/TB[i];"
          f"[b][i]overlay=0:0:enable='gte(t,{cut})':eof_action=pass" + ("," + lower if lower else "") + fade + "[v]")
    cmd = ["ffmpeg", "-v", "error", "-y"] + base + ins_in
    if cam_frame:
        cmd += ["-filter_complex", fc, "-map", "[v]", "-map", "0:a:0?"]
    else:
        cmd += ["-i", str(take), "-filter_complex", fc, "-map", "[v]", "-map", "2:a:0"]
    run(cmd + ["-t", f"{dur:.3f}"] + ENC + [str(out)])
    plan["kind"] = "camera+insert" if cam_frame else "insert+voice"
    plan["insert_at"] = cut
    return out


def chapters(plan_rows):
    out, t = [], 0.0
    for p in plan_rows:
        m = re.search(r"\*\*Chapter:\s*([^*]+?)\.?\*\*", p.get("narration", ""))
        if not out:
            out.append((0.0, "Intro"))
        elif m:
            out.append((t, m.group(1).strip()))
        t += p["dur"]
    warn = []
    if len(out) < 3:
        warn.append("YouTube needs ≥3 chapters")
    for (a, _), (b, n) in zip(out, out[1:] + [(t, "")]):
        if b - a < 10:
            warn.append(f"chapter at {int(a)}s shorter than 10 s")
    fmt = lambda s: f"{int(s // 60)}:{int(s % 60):02d}" if s < 3600 else f"{int(s // 3600)}:{int(s % 3600 // 60):02d}:{int(s % 60):02d}"
    return "\n".join(f"{fmt(a)} {n}" for a, n in out), warn


def main(a):
    if not a:
        sys.exit(__doc__)
    vid, dry, rough = a[0], "--dry-run" in a, "--rough" in a
    vf = SL.video_file(vid) or sys.exit(f"no active script for {vid} (videos/*.md with id: {vid})")
    fm, _ = SL.split_fm(vf.read_text())
    bs = SL.beats(vf.read_text())
    clips = json.loads((ROOT / "recording" / "clips.json").read_text())["videos"].get(vid, {}).get("lines", [])
    lines = {l["beat"]: l for l in clips}
    rows, missing = [], []
    for b in bs:
        l = lines.get(b["id"])
        take = kept_take(l) if l else None
        ins = find_insert(vid, b["id"], b.get("source", "")) if needs_insert(b.get("source", "")) else None
        row = {"beat": b["id"], "act": act_of(b), "source": b.get("source", ""), "narration": b.get("narration", ""),
               "take": str(take.relative_to(ROOT)) if take else "", "insert": str(ins.relative_to(ROOT)) if ins else "", "dur": 0.0}
        if needs_insert(b.get("source", "")) and not ins:
            missing.append(f"{b['id']}: insert for '{b.get('source', '')}' (drop a file at assets/inserts/{vid}-{b['id']}.mp4)")
        if not b.get("narration"):
            row["kind"] = "title"; row["dur"] = 3.0
        elif not take:
            missing.append(f"{b['id']}: no kept take ({l['status'] if l else 'not in clips.json'})")
            if not rough:
                continue
            row["kind"] = "card-missing-take"; row["dur"] = max(2.0, len(b["narration"].split()) / 2.5)
        else:
            row["dur"] = duration(take)
        rows.append(row)
    edits = ROOT / "assets" / "edits"; edits.mkdir(parents=True, exist_ok=True)
    print(f"{vid}: {len(rows)} segments, {sum(r['dur'] for r in rows) / 60:.1f} min (version {fm.get('version', '?')})")
    for m in missing:
        print("  missing:", m)
    if dry:
        for r in rows:
            print(f"  {r['beat']:5} {r['act']:3} {r.get('kind', 'take'):18} {r['dur']:6.1f}s  {r['take'] or '-'}  {r['insert'] or ''}")
        return
    tmp = pathlib.Path(tempfile.mkdtemp(prefix=f"mvj-edit-{vid}-"))
    try:
        segs, prev_act = [], None
        for i, r in enumerate(rows):
            b = next(x for x in bs if x["id"] == r["beat"])
            out = tmp / f"seg{i:03d}.mp4"
            if r.get("kind") == "title":
                card(tmp, out, 3.0, r["act"], b.get("on screen") or fm.get("title", "").strip('"'))
            elif r.get("kind") == "card-missing-take":
                card(tmp, out, r["dur"], r["act"], "[missing take " + r["beat"] + "]", b.get("narration", ""))
            else:
                out = segment(tmp, i, b, ROOT / r["take"], ROOT / r["insert"] if r["insert"] else None, r, fade_from=prev_act if prev_act and prev_act != r["act"] else None)
            r["dur"] = duration(out)
            segs.append(out); prev_act = r["act"]
            print(f"  ✓ {r['beat']:5} {r.get('kind', ''):16} {r['dur']:5.1f}s", flush=True)
        if not segs:
            sys.exit("nothing to edit: no kept takes yet")
        lst = tmp / "list.txt"
        lst.write_text("".join(f"file '{s}'\n" for s in segs))
        joined = tmp / "joined.mp4"
        run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(lst), "-c", "copy", str(joined)])
        if rough:
            (edits / "rough").mkdir(exist_ok=True)
        final = edits / "rough" / f"{vid}-rough.mp4" if rough else edits / f"{vid}-final.mp4"
        run(["ffmpeg", "-v", "error", "-y", "-i", str(joined), "-c:v", "copy", "-af", "loudnorm=I=-14:TP=-1:LRA=11",
             "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-movflags", "+faststart", str(final)])
        # reels: 9:16 crops of reel beats that have a take
        reels = edits / "reels"; reels.mkdir(exist_ok=True)
        nre = 0
        for r, s in zip(rows, segs):
            b = next(x for x in bs if x["id"] == r["beat"])
            if not rough and b.get("reel", "").lower().startswith("y") and r["take"]:
                run(["ffmpeg", "-v", "error", "-y", "-i", str(s), "-vf", "crop=ih*9/16:ih,scale=1080:1920", "-af", "loudnorm=I=-14:TP=-1:LRA=11"] + ENC + [str(reels / f"{vid}-{r['beat']}.mp4")])
                nre += 1
        chap, warn = chapters(rows)
        stem = edits / "rough" / vid if rough else edits / vid
        pathlib.Path(f"{stem}-chapters.txt").write_text(chap + "\n")
        pathlib.Path(f"{stem}-edl.json").write_text(json.dumps({"vid": vid, "version": fm.get("version", ""), "script": str(vf.relative_to(ROOT)),
                                                           "segments": rows, "missing": missing, "chapter_warnings": warn}, indent=1, ensure_ascii=False))
        print(f"→ {final.relative_to(ROOT)} ({duration(final) / 60:.1f} min), {nre} reels, chapters:\n{chap}")
        for w in warn:
            print("  chapter warning:", w)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main(sys.argv[1:])
