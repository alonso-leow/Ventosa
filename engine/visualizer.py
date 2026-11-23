#!/usr/bin/env python3
"""Simple visualizer: an elegant YouTube video for a song, rendered with ffmpeg.

For an independent musician who just exported a song: a slow palette gradient,
the notes as coloured bars (from a MIDI file: a scrolling piano roll, each note
coloured by pitch along the palette; without MIDI: a constant-Q note spectrum),
the song info as text placeholders, and a progress line.

  visualizer.py --audio song.wav --out video.mp4 [--midi song.mid]
                [--title T] [--artist A] [--line "Album · 2026"] [--line "link"]
                [--palette "#8b3dff,#ff8a1f,#0b0b10"] [--size 1920x1080] [--fps 30]
                [--seconds N]   (render only the first N seconds, for a preview)

Stdlib + ffmpeg/ffprobe. Placeholders left empty print as "Song title",
"Artist" so the layout shows what to fill.
"""
import argparse, json, os, pathlib, struct, subprocess, sys, tempfile


# ---------------------------------------------------------------- MIDI (SMF 0/1)

def _varlen(b, i):
    v = 0
    while True:
        c = b[i]
        i += 1
        v = (v << 7) | (c & 0x7F)
        if c < 0x80:
            return v, i


def read_midi(path):
    """Notes as (start_s, end_s, pitch, velocity), with the tempo map applied."""
    b = pathlib.Path(path).read_bytes()
    if b[:4] != b"MThd":
        raise ValueError("not a MIDI file")
    _, ntrk, div = struct.unpack(">HHH", b[8:14])
    if div & 0x8000:
        raise ValueError("SMPTE time division is not supported")
    i, events, tempos = 14, [], [(0, 500000)]
    for _ in range(ntrk):
        if b[i:i + 4] != b"MTrk":
            break
        ln = struct.unpack(">I", b[i + 4:i + 8])[0]
        j, end, tick, status = i + 8, i + 8 + ln, 0, 0
        while j < end:
            d, j = _varlen(b, j)
            tick += d
            c = b[j]
            if c == 0xFF:
                kind, ln2 = b[j + 1], 0
                ln2, k = _varlen(b, j + 2)
                if kind == 0x51 and ln2 == 3:
                    tempos.append((tick, int.from_bytes(b[k:k + 3], "big")))
                j = k + ln2
                continue
            if c in (0xF0, 0xF7):
                ln2, k = _varlen(b, j + 1)
                j = k + ln2
                continue
            if c & 0x80:
                status, j = c, j + 1
            hi = status & 0xF0
            if hi in (0x80, 0x90, 0xA0, 0xB0, 0xE0):
                d1, d2 = b[j], b[j + 1]
                j += 2
                if hi == 0x90 and d2 > 0:
                    events.append((tick, 1, status & 0x0F, d1, d2))
                elif hi == 0x80 or (hi == 0x90 and d2 == 0):
                    events.append((tick, 0, status & 0x0F, d1, 0))
            else:
                j += 1
        i = end
    tempos.sort()

    def seconds(t):
        s, last_tick, us = 0.0, 0, 500000
        for tt, tempo in tempos:
            if tt >= t:
                break
            s += (tt - last_tick) * us / div / 1e6
            last_tick, us = tt, tempo
        return s + (t - last_tick) * us / div / 1e6

    events.sort(key=lambda e: (e[0], e[1]))
    held, notes = {}, []
    for tick, on, ch, pitch, vel in events:
        key = (ch, pitch)
        if on:
            held.setdefault(key, []).append((tick, vel))
        elif held.get(key):
            st, v = held[key].pop(0)
            notes.append((seconds(st), seconds(tick), pitch, v))
    return sorted(notes)


# ---------------------------------------------------------------- render

def hex_rgb(h):
    h = h.strip().lstrip("#")
    return tuple(int(h[k:k + 2], 16) for k in (0, 2, 4))


def mix(a, b, k):
    return tuple(round(x + (y - x) * k) for x, y in zip(a, b))


def ramp(pal, k):
    """Colour at k in [0,1] along the palette (all but the last colour, which is the ground)."""
    cols = pal[:-1] if len(pal) > 2 else pal
    if len(cols) == 1:
        return cols[0]
    k = min(max(k, 0.0), 1.0) * (len(cols) - 1)
    n = min(int(k), len(cols) - 2)
    return mix(cols[n], cols[n + 1], k - n)


def hx(c):
    return "0x%02x%02x%02x" % c


def duration(audio):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", audio], capture_output=True, text=True)
    return float(json.loads(out.stdout or "{}").get("format", {}).get("duration", 0) or 0)


def font():
    for q in ("Hack:bold", "sans:bold", "sans"):
        r = subprocess.run(["fc-match", "-f", "%{file}", q], capture_output=True, text=True)
        if r.returncode == 0 and r.stdout.strip():
            return r.stdout.strip()
    return ""


def render_roll(notes, pal, pps, roll_h, width, tmp):
    """The whole piano roll as one wide transparent strip (x = seconds · pps)."""
    lo, hi = min(n[2] for n in notes), max(n[2] for n in notes)
    span = max(hi - lo + 1, 12)
    row = roll_h / span
    boxes = []
    for s, e, p, v in notes:
        k = (p - lo) / max(span - 1, 1)
        c = ramp(pal, k)
        alpha = 0.55 + 0.45 * v / 127
        y = int((span - 1 - (p - lo)) * row)
        w = max(int((e - s) * pps), 3)
        h = max(int(row * 0.8), 2)
        boxes.append(f"drawbox=x={int(s * pps)}:y={y}:w={w}:h={h}:color={hx(c)}@{alpha:.2f}:t=fill:replace=1")
        # a lighter cap on the note's start: the attack
        boxes.append(f"drawbox=x={int(s * pps)}:y={y}:w={min(w, 4)}:h={h}:color={hx(mix(c, (255, 255, 255), 0.5))}@0.95:t=fill:replace=1")
    gp = pathlib.Path(tmp) / "roll.txt"
    gp.write_text(f"color=c=black@0.0:s={width}x{roll_h}:d=1,format=rgba," + ",".join(boxes) + "[out]")
    out = pathlib.Path(tmp) / "roll.png"
    r = subprocess.run(["ffmpeg", "-y", "-v", "error", "-/filter_complex", str(gp), "-map", "[out]", "-frames:v", "1", str(out)], capture_output=True, text=True)
    if r.returncode:
        raise SystemExit(f"visualizer: piano roll: {r.stderr.strip()[-400:]}")
    return out


def build(a, dur, tmp):
    """Filter graph and the extra inputs it reads (after the audio, input 0)."""
    W, H = (int(x) for x in a.size.lower().split("x"))
    pal = [hex_rgb(c) for c in a.palette.split(",") if c.strip()] or [(139, 61, 255), (255, 138, 31), (11, 11, 16)]
    ground = pal[-1]
    fps = a.fps
    f, inputs = [], []
    # ground: a slow gradient of the palette, darkened toward the ground colour
    g = [hx(mix(c, ground, 0.55)) for c in pal[:-1] or pal] + [hx(ground)]
    cs = ":".join(f"c{k}={c}" for k, c in enumerate(g[:8]))
    f.append(f"gradients=s={W}x{H}:{cs}:n={len(g[:8])}:type=radial:speed=0.004:duration={dur:.3f}:rate={fps},format=rgba[bg]")
    roll_top, roll_h = int(H * 0.12), int(H * 0.52) // 2 * 2
    play_x = int(W * 0.32)
    last = "bg"
    notes = read_midi(a.midi)[: a.max_notes] if a.midi else []
    if notes:
        pps = W / max(a.window, 1.0)  # pixels per second
        width = int(min(dur, notes[-1][1] + 1) * pps) + 8
        roll = render_roll(notes, pal, pps, roll_h, min(width, 60000), tmp)
        inputs += ["-loop", "1", "-framerate", str(fps), "-i", str(roll)]
        # the strip slides left; the note under the playhead is the one sounding
        f.append(f"[{last}][1:v]overlay=x='{play_x}-t*{pps:.3f}':y={roll_top}:shortest=0:eof_action=pass[r0]")
        f.append(f"[r0]drawbox=x={play_x}:y={roll_top}:w=2:h={roll_h}:color=white@0.55:t=fill[roll]")
        last = "roll"
    else:
        # no MIDI: the notes heard, as a constant-Q bar spectrum in the palette
        c1, c2 = ramp(pal, 0.15), ramp(pal, 0.85)
        sch = "|".join(f"{x / 255:.2f}" for x in (*c1, *c2))
        f.append(f"[0:a]showcqt=s={W}x{roll_h}:fps={fps}:sono_h=0:bar_g=2:bar_v=6:axis=0:bar_t=0.5:cscheme={sch},format=rgba,colorkey=0x000000:0.08:0.25[cqt]")
        f.append(f"[{last}][cqt]overlay=0:{roll_top}:shortest=0[roll]")
        last = "roll"
    # song info placeholders (text files: no escaping)
    ff = a.font or font()
    title = a.title or "Song title"
    artist = a.artist or "Artist"
    texts = [(title, int(H * 0.075), "white", int(H * 0.70)), (artist, int(H * 0.04), hx(ramp(pal, 0.9)), int(H * 0.70) + int(H * 0.095))]
    y = int(H * 0.70) + int(H * 0.16)
    for line in a.line or ["Album · Year", "Listen everywhere: link in description"]:
        texts.append((line, int(H * 0.026), "white@0.7", y))
        y += int(H * 0.04)
    draws = []
    for n, (txt, size, col, ty) in enumerate(texts):
        p = pathlib.Path(tmp) / f"t{n}.txt"
        p.write_text(txt)
        fontpart = f"fontfile='{ff}':" if ff else ""
        draws.append(f"drawtext={fontpart}textfile='{p}':fontsize={size}:fontcolor={col}:x={int(W * 0.06)}:y={ty}")
    draws.append(f"drawbox=x=0:y={H - 6}:w=iw:h=6:color=white@0.12:t=fill")
    f.append(f"[{last}]" + ",".join(draws) + "[txt]")
    # progress line: a bar of the palette's middle colour sliding in from the left
    f.append(f"color=c={hx(ramp(pal, 0.5))}:s={W}x6:r={fps}:d={dur:.3f}[bar]")
    f.append(f"[txt][bar]overlay=x='-{W}+{W}*t/{dur:.3f}':y={H - 6}:shortest=1,format=yuv420p[v]")
    return ";\n".join(f), inputs


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--audio", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--midi")
    ap.add_argument("--title", default="")
    ap.add_argument("--artist", default="")
    ap.add_argument("--line", action="append")
    ap.add_argument("--palette", default="#8b3dff,#ff8a1f,#0b0b10", help="comma-separated colours; the last one is the ground")
    ap.add_argument("--size", default="1920x1080")
    ap.add_argument("--fps", type=int, default=30)
    ap.add_argument("--window", type=float, default=8.0, help="seconds of notes across the screen")
    ap.add_argument("--max-notes", type=int, default=4000)
    ap.add_argument("--seconds", type=float, default=0.0)
    ap.add_argument("--font", default="")
    ap.add_argument("--print", action="store_true", help="print the filter graph and exit")
    a = ap.parse_args()
    dur = duration(a.audio)
    if dur <= 0:
        sys.exit(f"visualizer: cannot read the length of {a.audio}")
    if a.seconds > 0:
        dur = min(dur, a.seconds)
    with tempfile.TemporaryDirectory() as tmp:
        graph, inputs = build(a, dur, tmp)
        if a.print:
            print(graph)
            return
        gp = pathlib.Path(tmp) / "graph.txt"
        gp.write_text(graph)
        cmd = ["ffmpeg", "-y", "-v", "error", "-stats", "-i", a.audio, *inputs, "-/filter_complex", str(gp), "-map", "[v]", "-map", "0:a",
               "-t", f"{dur:.3f}", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", a.out]
        r = subprocess.run(cmd)
        if r.returncode:
            sys.exit(r.returncode)
    print(os.path.abspath(a.out))


if __name__ == "__main__":
    main()
