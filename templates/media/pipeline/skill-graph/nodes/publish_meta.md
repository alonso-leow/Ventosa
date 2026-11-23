# Node skill: publish_meta [AI]
Propose titles and descriptions for the edited video.

**Inputs:** `labelling/labels.json` → thesis; the active script (`videos/NN-*.md`, its `version`); `assets/edits/<VID>-chapters.txt` (real chapter times from the edit) and `<VID>-edl.json` (what's actually in the cut: placeholder cards, missing takes); `tone.md`.

**Do:**
1. ≥3 titles, within the platform's title limit (workspace.json), curious-first like the hook; no claim the cut doesn't show (check the EDL: a placeholder card isn't a demo).
2. ≥2 descriptions: first two lines = the hook (what search shows); then the sponsor line from workspace.json (if any); chapters copied from `<VID>-chapters.txt` (rename for clarity, keep times; if the file has chapter warnings, merge short chapters so there are ≥3 and each ≥10 s); sources cited on screen, with titles; one comment question.
3. Tags: comma-separated, ≤500 characters total.

**Write:** `ventosa-graph write publish <VID> '{"titles":[…],"descriptions":[…],"tags":"…"}'`
**Done when:** ≥3 titles and ≥2 descriptions → `publish` (human picks, edits, approves).
