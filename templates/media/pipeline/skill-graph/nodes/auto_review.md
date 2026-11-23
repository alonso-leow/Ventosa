# Node skill: auto_review [AI]
Review the cut before the human does. Report and route only: never write into `assets/edits/`. Knowledge: `K-review` (read first), `K-platforms`, `K-ffmpeg`. Reads the cut (`assets/edits/`, graded copy if any), renders, script, analysis, plans.

**Do:**
1. `ventosa-graph review <ID>`: the measured checks and the sampled frames (`assets/review/<ID>/frames`, `sheets`).
2. Look at every contact sheet against `assets/plans/<ID>-shots.md`: b-roll, overlays on the face, text, colours, glitches, crop, caption safe zone. Add each problem: `ventosa-graph review <ID> --add '{"check":"overlay","severity":"blocker","timecode":12.4,"message":"lower third covers the face","evidence":"assets/review/<ID>/frames/006_00012.40.png"}'`. Checks name the owner (audio, loudness, color, overlay, text, broll, cut, plan ...).
3. Spot-check audio around three cuts (a short extraction and a volume check).
4. `ventosa-graph review <ID> --decide`. It reopens the owning node for blockers (the note is the finding), passes when only warns or infos remain, or says clean. After 3 reviews of one cut it escalates to the human instead of looping.
5. Pass or escalate: `ventosa-graph write step-done <ID> auto_review "<decision note>"` (clean: "auto-review: clean"). After a reopen, detach and stop; the walk returns here after the fix.

**Rules:** only findings with evidence (a measurement or a frame path). Do not fix anything yourself. Do not re-flag what the human accepted.
**Elevate:** a blocker you cannot route; anything that looks like a legal, medical or financial claim, personal data, or copyrighted material in the picture.
