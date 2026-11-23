# Node skill: options [AI]
Turn a title (plus the human's notes) into thesis paths. Each option later gets a full script version at `drafts`.

**Inputs:** `labelling/labels.json` → the episode's `title`, `notes`, existing `options`, `want_more`; the script in `episodes/` if one exists; knowledge: `tone.md`, `meta/meta-script.md`, `meta/addenda.md`, `research/codebase-refs.md`, `research/papers.md`, `meta/structures.md` (expected shape).

**Do:**
1. Write ≥2 options, or exactly one more when `want_more` is true. Each is a different angle (demo-first, story, myth-busting, tutorial, manifesto…), not a reword.
2. Each option: `name` (2–5 words), `style` (one word), `thesis` (one sentence the whole episode proves), `paras` (7–12 lines, one per paragraph, saying what it argues).
3. Frame: para 1 `Hook: …` (≤40 s, curious/sensory), para 2 `Sponsor line.`, three acts (perception → numbers → pixels), last para the CTA linked to the next episode.
4. Only claims backed by codebase-refs; nothing unbuilt promised as built. Follow the content rules in tone.md.

**Write (one call per option):** `ventosa-graph write option <ID> '{"name":"…","style":"…","thesis":"…","paras":["Hook: …","Sponsor line.",…]}'`
New episode from chat: `ventosa-graph write new <ID> "Title" "notes"` (no attachment needed), then attach to `options`.

**Done when:** ≥2 options and `want_more` false → `drafts` (AI writes one script version per option).
