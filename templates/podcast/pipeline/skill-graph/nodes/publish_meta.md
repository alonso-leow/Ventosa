# Node skill: publish_meta [AI]
Propose episode titles and show notes for the edited episode.

**Inputs:** `labelling/labels.json` → thesis; the active script (`episodes/NN-*.md`); `assets/edits/<ID>-chapters.txt`; `tone.md`.

**Do:**
1. ≥3 titles, within the platform's title limit (workspace.json), curious-first like the cold open.
2. ≥2 show-note drafts: first two lines = the hook (what podcast apps show in the list); the sponsor line from workspace.json (if any); chapters from `<ID>-chapters.txt`; sources mentioned in the episode, with links; one listener question.
3. Tags/keywords: comma-separated, ≤500 characters.
4. The feed carries transcripts and chapters (Podcasting 2.0 `podcast:transcript`, `podcast:chapters`): note in the show notes which files the human must upload with the episode.

**Write:** `ventosa-graph write publish <ID> '{"titles":[…],"descriptions":[…],"tags":"…"}'`
**Done when:** ≥3 titles and ≥2 descriptions → `publish` (human picks, edits, approves).
