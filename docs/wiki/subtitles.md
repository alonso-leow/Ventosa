# Subtitles
<!-- group: pipeline -->

Subtitles are a separate step after the edit is signed off: `edit` → **`subtitles`** → `publish_meta`. They are made from the final cut, are timed to its frames, and never write into `assets/edits/`. A new edit sends the video back to this step.

## Run it

```
ventosa-graph subtitles V03            # SRT + VTT + ASS, reel tracks, report
ventosa-graph subtitles V03 --burn     # also an open-caption copy (reels, shorts)
ventosa-graph write subtitles-done V03 # the AI signs off (attached to `subtitles`)
```

Everything lands in `assets/subtitles/<ID>/`: `<ID>.srt|.vtt|.ass`, `reels/<reel>.srt|.ass` (9:16, re-timed to each reel), `words.json` (cached word timings), `report.json` (QC) and, with `--burn`, `<ID>-subs.mp4`.

Flags: `--lang es` (skip detection), `--backend whisperx|stable-ts|faster-whisper|whisper.cpp`, `--model large-v3`, `--force` (redo the words), `--no-script` (do not correct against the script).

## How it works

1. **Audio**: 16 kHz mono from the signed edit (ffmpeg).
2. **Words with timestamps**: the first backend installed (see below). Whisper's own segment times drift, so word-level alignment is what makes cues land on the right frame.
3. **Script correction**: the active script's narration is aligned to the recognised words. Where both agree one-for-one, the script's spelling wins (names, terms). The timing always stays the audio's. A low match (ad-libs) is reported, not forced.
4. **Cues by policy**: a pause, a sentence end or the size limit ends a cue. A cue does not run across a shot change it can end on. Shot changes come from the edit's `-edl.json`.
5. **Frames**: times become frames at the edit's real fps. Edges within 7 frames of a cut snap onto it, gaps under 12 frames close to 2, and each cue gets its reading time (≤ 17 cps) and at least 20 frames. Timestamps are floored to the millisecond, so a cue shows on its first frame exactly.
6. **Files**: SRT and VTT for the platforms, ASS (white, outlined, sized to the frame) for burning with ffmpeg + libass.

## The policy

| rule | value |
|---|---|
| per line | ≤ 42 characters (reels 26, up to 3 lines) |
| per cue | ≤ 2 balanced lines, never ending on a weak word |
| reading speed | ≤ 17 characters per second |
| duration | 0.83 s to 7 s |
| gaps | ≥ 2 frames |
| text | what was said, in the script's spelling; never invented |
| translations | derived from the master SRT, same timings |

Override per workspace in `workspace.json`:

```
"subtitles": {"lang": "es", "max_cps": 20, "max_chars": 38, "font": "Inter", "backend": "whisperx"}
```

## Backends

The open-source projects that already do speech to text with frame-usable timing. The pipeline uses the first one it finds:

| backend | what it does | install |
|---|---|---|
| **WhisperX** | faster-whisper transcription + wav2vec2 forced phoneme alignment + VAD: the most accurate word times; diarization optional | `pip install whisperx` (torch; a GPU helps) |
| **stable-ts** | stabilises Whisper timestamps; `align()` fits a known text (our script) to the audio; writes SRT/VTT/ASS itself | `pip install stable-ts` |
| **faster-whisper** | CTranslate2 Whisper, `word_timestamps=True`, VAD filter; fast on CPU | `pip install faster-whisper` |
| **whisper.cpp** | C/C++ Whisper, no Python; `-ml 1 -sow` gives per-word segments, `-ojf` full JSON | build it, then `VENTOSA_WHISPER_MODEL=/path/ggml-large-v3.bin` |

Also worth knowing: **aeneas** and the **Montreal Forced Aligner** (force-align a known transcript, no ASR), **pysubs2** (read/write SRT/ASS from Python), **Aegisub** and **Subtitle Edit** (hand review and styling), **ffmpeg** `subtitles`/`ass` filters (burn-in through libass).

## Review checklist

- `report.json` → `warnings` is empty, or each one is explained.
- Five cues checked in the player at their frames, one across a cut.
- Names, numbers and brand terms match `tone.md` and the trusted sources.
- Reels: the text sits above the platform UI (lower third is kept clear).
