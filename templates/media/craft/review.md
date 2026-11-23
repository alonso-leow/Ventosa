# Reviewing a cut before the human (K-review)

The AI reviews first so the human only watches a cut that is already clean. The review reports and routes; it never edits `assets/edits/`.

## Order of work
1. `ventosa-graph review <ID>`: measurements first (no model needed), then frame sampling (one near every shot change, one every ~2 s, the first and last second) into `assets/review/<ID>/frames/` and contact sheets in `sheets/`.
2. Look at every contact sheet against `assets/plans/<ID>-shots.md`. Add what you see with `review <ID> --add '{...}'`.
3. Spot-check audio around cuts (extract a short waveform or run a volume check on the join).
4. `review <ID> --decide`.

## Measured checks
Length against the plan and platform limit · size, fps, codec, pixel format, aspect · audio present · loudness near -14 LUFS and true peak under -1 dBTP · silent gaps · black frames · frozen frames · audio/video length drift · shot-change count against the cut list · more than 3 flashes per second · leftover placeholder text · sponsor line present in script and captions.

## Visual pass (by eye)
Wrong or missing b-roll for a line · overlay covering the face or clipped at an edge · typos or unreadable text · colours off the brand · rendering glitches · wrong aspect crop · captions outside the safe margin · inserts with the wrong subject.

## Severity
| level | meaning | examples | effect |
|---|---|---|---|
| blocker | a viewer would notice or the platform would reject it | wrong size, no audio, clipping, loudness off by more than 3 LU, black frame mid-video, 1 s or longer, frozen 5 s or longer, flashes above 3 per second, placeholder text, missing sponsor line, audio out of sync by half a second | sent back |
| warn | probably wrong, a human can judge | loudness off by 1-3 LU, a 5 s silence, a 1-5 s freeze, length 25-50% off plan, odd shot-change count | shown to the human |
| info | worth knowing | a 2-5 s pause, a measurement that could not be taken | shown in the report |

## Routing
Audio and loudness go to `audio_fx`; colour and grade to `color_grade`; cut, pacing, b-roll, sync, black and frozen frames, length and format to `edit`; overlays, graphics, text and flashing to `motion_source`; plan mismatches to `shot_plan`. With several blockers the earliest node in graph order goes first; the rest are found again on the next review.

## Decision and loop guard
- Blockers: reopen the owning node with the finding as the note, then review again when the fix is done.
- At most 3 automatic reviews per cut. A fourth blocker pass is escalated: the human gets the open findings with the report.
- Only warns or infos: pass on with the report summarised. Nothing found: pass with "auto-review: clean".
- The human can accept a finding (`review <ID> --accept <key>`); an accepted finding is not flagged again for that cut. An approval at `edit_review` counts only for the cut it was given on.
