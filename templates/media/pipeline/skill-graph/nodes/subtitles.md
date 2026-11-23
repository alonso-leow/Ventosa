# Node skill: subtitles [AI]
A separate pass after the edit is signed; it never touches `assets/edits/`. Policy: docs ▸ subtitles (the `subtitles` policy in the meta graph).
1) `ventosa-graph subtitles <VID>` → `assets/subtitles/<VID>/`: word timings from the signed edit (WhisperX, else stable-ts, faster-whisper or whisper.cpp), the script's spelling where it matches, cues cut by the policy (≤42 chars × 2 lines, ≤17 cps, 0.83–7 s, ≥2-frame gaps, edges snapped to the EDL's shot changes), written frame-exact as `<VID>.srt|.vtt|.ass`, plus 9:16 tracks in `reels/`. Words are cached per edit; `--force` redoes them; `--lang es` when detection is unsure.
2) Read `report.json`: every warning is either fixed or explained. Low script match = ad-libs: check names, numbers and terms against `tone.md` and the trusted sources, never invent words that were not said. Spot-check five cues in a player at their frames (`media_play`), including one across a shot change.
3) Fix text only in the `.srt` by hand if needed, then rerun with the same flags so `.vtt`/`.ass`/reels follow (cached words, so it is fast). Never retime by hand: change the policy in `workspace.json` → `subtitles` instead.
4) `--burn` only when the platform needs open captions (reels, shorts); the upload keeps the sidecar `.srt` otherwise.
5) `ventosa-graph write subtitles-done <VID>` (refuses tracks older than the edit or with overlong lines). A new edit reopens this step.

6) Open-caption look (`K-captions`): plain, bold, outline, glow or box; brand colour; word highlight only on reels and shorts; keep clear of the face and the platform's interface band. Short-form always gets captions.
