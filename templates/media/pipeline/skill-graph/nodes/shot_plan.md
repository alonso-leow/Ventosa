# Node skill: shot_plan [ai]
Turn the final script into a shot plan the later phases follow. Input: the active script (an accepted script, use as written), the recorded takes index, `K-animations`.

**Do:** for each script line write: the take to use (line id), what is on screen over it (take, motion source, insert, text), the duration, and the sound idea. Mark every line that needs a motion source (renders, animations, inserts) so `motion_source` makes exactly those. Write it to `assets/plans/<ID>-shots.md`.
**Elevate:** a line with no usable take; anything that needs footage or music you do not have the rights to.
**Done:** `ventosa-graph write step-done <ID> shot_plan "plan in assets/plans/<ID>-shots.md"`. A human approves it next (`plan_ok`).


**Plan format and gate (`K-motion`).** Lines needing designed motion carry a spec: overview (title, length, size, fps, style, mood); audio plan (bed, sync points, volume envelope, effects with seconds and purpose); per scene: what is visible, animations with seconds, frame timing, audio, transition out, focus point; technical notes (palette hex, type, easing, assets). Mark each line's source type: take, motion source (HyperFrames or Remotion), 3D (`scene_3d`), generated (`gen_media`), insert. Respect the time budgets (2-frame minimum, 9 frames to read, change something every 8-12 s).
Before `step-done` run the gate in `K-motion`: clear focus per scene, timing in budget, audio supports picture, effects on key moments, purposeful transitions, visible arc, buildable specs, every element serves the story, ending resolves. Reject: random easing, constant bouncing, unclear hierarchy, audio fighting picture, no breathing room, mixed styles, no arc.
Also read, when present: `assets/analysis/<ID>/analysis.md`, `assets/plans/<ID>-cuts.json`, `assets/refs/<ID>-style.md` (follow its style sheet).
