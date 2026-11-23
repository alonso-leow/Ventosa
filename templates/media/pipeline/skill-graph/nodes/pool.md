# Node skill: pool [AI]
Gather the pool of candidate angles for a title, before any thesis is written. The next node (thesis options) picks from it.

**Inputs:** `labelling/labels.json` → `title`, `notes`; knowledge `tone.md`, `meta/meta-script.md`, `research/codebase-refs.md`, `research/papers.md`, `meta/structures.md`.

**Do:** write `research/<ID>-pool.md`: 6–10 angles, one line each (`angle · why it is interesting · the evidence it would use`). Mix shapes (demo-first, story, myth-busting, tutorial, manifesto). No thesis wording yet, no drafts.

**Done:** `VENTOSA_AGENT=<you> ventosa-graph attach pool <ID>`, then `ventosa-graph write step-done <ID> pool "wrote research/<ID>-pool.md"`.
