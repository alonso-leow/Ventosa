# Node skill: drafts [AI]
Write one complete, recordable episode script **version per thesis option**, so the human can pick or mix.

**Inputs:** `labelling/labels.json` → the episode's `options` and `notes`; the current `episodes/NN-*.md` if any; knowledge: `tone.md` (law), `meta/meta-script.md` (the show's voice), `research/` (facts and sources).

**Write** `scripts/<ID>/<KEY>-<slug>.md`, one per option key:
```
---
id: E02
title: "Episode title"
version: A-cold-open
option: A
thesis: <the option's thesis, one sentence>
status: draft
length_min: 25
---
# <title> — version A: <option name>

**Logline:** …
**Promise to the listener:** …

## Beats

| id | t | act | source | on screen | narration (speakable draft) | reel |
|---|---|---|---|---|---|---|
| B1 | 0:00 | I | MIC | | <cold open, ≤40 s> | yes |
| SP | 0:40 | I | MIC | | <sponsor line from workspace.json, if any> | |
| … one row per segment, following the option's paragraphs in order … |

## Notes
## Questions for the host
```
**Rules**
- One option paragraph → one or more beats, same order. Cold open first, SP second (exact sponsor text, if the workspace has one), call to action last.
- Narration = the words the host says on the mic: speakable, short sentences, the show's voice from `tone.md`.
- Sources: `MIC` (host), `GUEST <name>` (guest segment, narration = the questions), `CLIP <file>` (audio clip). `on screen` stays empty unless the episode also ships as video.
- `act` I / II / III. `t` = rough start time; pace ≈ 150 words/min. `reel: yes` marks segments worth cutting as short clips.
- Bold chapter markers inside narration where a chapter starts: `**Chapter: name.**` (feed chapters).
- Facts only from the knowledge files. Follow the content rules in tone.md.

**Order matters:** attach **before** writing the first file, then write and register one version at a time.
`VENTOSA_AGENT=<you> ventosa-graph attach drafts <ID>` once, then `VENTOSA_AGENT=<you> ventosa-graph write draft <ID> scripts/<ID>/<file>.md` per version. The node detaches itself when every option has a version.
