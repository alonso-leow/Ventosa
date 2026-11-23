---
name: ventosa-subtitles
description: Make subtitles for a Ventosa video after its edit is signed off. A separate pass that turns speech into frame-exact SRT/VTT/ASS (and 9:16 reel tracks) from the final cut, corrected against the script, cut by the subtitle policy, optionally burnt in. Use for "subtitle V03", "captions", "srt for the reels", or a workspace at the `subtitles` step.
---

# Ventosa subtitles

Subtitles are their own step, **after** `edit` and before `publish_meta`. They are made from the signed final cut, never from takes, and never write into `assets/edits/`. A new edit reopens the step.

## Pipeline

1. **Attach.** `VENTOSA_AGENT=<you> ventosa-graph -w <root> attach subtitles <ID>`. Its output is the node skill. Read it.
2. **Run.** `ventosa-graph -w <root> subtitles <ID> [--lang es] [--backend whisperx] [--burn]`. In order:
   - extract 16 kHz mono audio from the signed edit;
   - word timestamps: WhisperX (forced alignment), else stable-ts (`align` with the known script), else faster-whisper (`word_timestamps` + VAD), else whisper.cpp (`-ml 1 -sow -ojf`). Cached per edit in `words.json`;
   - correct the spelling against the active script's narration (difflib, 1:1 matches only; the timing stays the ASR's);
   - cut cues by the policy, convert to frames at the edit's real fps, snap the edges to the EDL's shot changes, keep the gaps;
   - write `<ID>.srt`, `.vtt` and `.ass` (brand-neutral white with an outline), and re-time a track for every reel in `reels/`;
   - write `report.json` (QC), and with `--burn` an open-caption copy `<ID>-subs.mp4`.
3. **Review.** Every `warnings` entry in `report.json` is fixed or explained. Spot-check five cues at their frames with `media_play`, one of them across a shot change.
4. **Sign.** `ventosa-graph -w <root> write subtitles-done <ID>`. It refuses tracks older than the edit, and lines over the limit.

## Policy (the `subtitles` policy in the meta graph)

| rule | value |
|---|---|
| characters per line | ≤ 42 (reels 26, up to 3 lines) |
| lines per cue | ≤ 2, balanced, never ending on a weak word (a, the, de, la…) |
| reading speed | ≤ 17 characters per second |
| duration | 5/6 s (20 frames at 24 fps) to 7 s |
| gap between cues | ≥ 2 frames; gaps under 12 frames close to 2 (no flicker) |
| shot changes | a cue never spans a cut it can end on; edges within 7 frames snap onto the cut |
| breaks | a pause ≥ 0.5 s, a sentence end, or the size limit ends a cue |
| timing | frames first, then timestamps floored to the unit (ms / cs), so the cue shows on its first frame exactly |
| text | what was said, in the script's spelling. Never add words that were not spoken. Numbers, names and terms as in `tone.md` and the trusted sources |
| languages | the master track is the spoken language. Translations are derived tracks from the master SRT with the same timings, never re-timed |
| open captions | `--burn` only for platforms without sidecar captions (reels, shorts). The long video uploads the `.srt` |

Change values per workspace in `workspace.json` → `"subtitles": {"max_cps": 20, "lang": "es", …}`. Never retime cues by hand.

## Elevate (decision_ask, never guess)

- Script match under 60% and the report shows unclear words (inaudible, names): ask the human, do not invent.
- Profanity, a real person's name, or a sponsor line that differs from the script.
- A language other than the workspace's: is it a translation job?

## Install a backend

`pip install whisperx` (best timings, needs torch), or `pip install stable-ts` or `pip install faster-whisper`. For CPU-only machines, build whisper.cpp and set `VENTOSA_WHISPER_MODEL=/path/ggml-large-v3.bin`. ffmpeg must have libass for `--burn`. Docs ▸ subtitles has the details.
