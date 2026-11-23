# Node skill: drafts [AI]
Write one complete, recordable script **version per thesis option**, so the human can pick or mix.

**Inputs:** `labelling/labels.json` → the video's `options` (key, name, style, thesis, paras = what each paragraph argues) and `notes`; the current `videos/NN-*.md` (facts, refs, animations, notes, questions — reuse what's right); knowledge: `tone.md` (law), `meta/meta-script.md` + `meta/addenda.md` (true source + honored answers), `research/codebase-refs.md` (only quote these facts), `research/papers.md` (the host's findings on top), `hyperframes/animations.md`.

**Write** `scripts/<VID>/<KEY>-<slug>.md` (e.g. `scripts/V02/A-neurons-to-numbers.md`), one per option key:
```
---
id: V02
title: "Text to Text vs Sound to Motion"
version: A-neurons-to-numbers
option: A
thesis: <the option's thesis, one sentence>
status: draft
length_min: 10
pillar: … (copy)   concepts / refs / animations / tone / next: copy or adapt from videos/NN-*.md, one key per line like the original
---
# <title> — version A: <option name>

**Logline:** …
**Promise to the viewer:** …
**Acts:** Act I … on `--act1`; Act II … on `--act2`; Act III … on `--act3`.

## Beats

| id | t | act | source | on screen | narration (speakable draft) | reel |
|---|---|---|---|---|---|---|
| B1 | 0:00 | I | CAM | … | <hook, ≤40 s> | yes |
| SP | 0:35 | I | CAM + logo lower-third | brand logo | <sponsor line from workspace.json> | |
| … one row per beat, following the option's paragraphs in order … |

## Notes
## Questions for the host
```
**Rules**
- One option paragraph → one or more beats, same order. Hook first, SP second (exact sponsor text), CTA last.
- Narration = the words the host says on camera: speakable, short sentences, question → answer, caps on the beat, the host's voice from the meta script. Every CAM/HF/SCR/REN beat that the host speaks over has narration. A beat with no narration is a silent card (TITLE).
- Sources: `CAM` (external camera, talking head), `SCR <R-ref or file>`, `HF-<name>` (≤7 distinct per video, from hyperframes/animations.md or a new name + one-line description in Notes), `REN` (screen render, shown muted), `TITLE`.
- `act` I / II / III (transitions `I→II`). `t` = rough start time; pace ≈ 150 words/min.
- Facts only from the knowledge files. Nothing unbuilt presented as built. Follow the content rules in tone.md.
- Bold chapter markers inside narration where a chapter starts: `**Chapter: name.**` (used for platform chapters).
- Length: length_min of the original ±20 %.

**Order matters:** attach **before** writing the first file, then write and register one version at a time. Never write `pipeline/events.jsonl` by hand or bypass `ai_write.py`.

**Register each file** (validates; fix and retry on errors):
`VENTOSA_AGENT=<you> ventosa-graph attach drafts <VID>` once, then `VENTOSA_AGENT=<you> ventosa-graph write draft <VID> scripts/<VID>/<file>.md` per version. The node detaches itself when every option has a version.


**Structure aids (`K-explainer`).** Let versions differ in structure (problem-agitate-solve, before-after-bridge, short, or the `tone.md` acts), not only wording. Check each against the speed table and the common-mistakes list: too wordy, no hook in 3 s, picture trailing the words, too many ideas.
